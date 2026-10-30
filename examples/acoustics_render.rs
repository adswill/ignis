//! Ignis acoustics: headless renderer. Drives the real solver (sim.wgsl + matter.wgsl, dispatched exactly like
//! gpu.rs's step loop) with src/acoustics.{rs,wgsl} attached, and writes what the player would hear as WAV.
//!
//!   cargo build --release --example acoustics_render
//!   ./target/release/examples/acoustics_render                       # all scenes
//!   SCENES=campfire,rocket SECS=12 OUT=/abs/dir ./target/release/examples/acoustics_render
//!   per scene overrides: WARM=<frames before recording>, LX/LY=<listener cell>, SPEED=<sim speed>
//!
//! GpuSim keeps its buffers private, so this example builds a minimal replica of the compute half of gpu.rs
//! (same WGSL modules, bindings parsed from the WGSL declarations, step list / WALL_EVERY parsed from gpu.rs
//! so it follows other teams' edits) and hands its U0/META/WALL buffers to `GpuAcoustics`, exactly as gpu.rs
//! will once integrated.
#![allow(dead_code, unused_imports, unused_variables, unused_mut, clippy::all)]

#[path = "../src/gpu.rs"]
mod gpu;
#[path = "../src/scene.rs"]
mod scene;
#[path = "../src/audio.rs"]
mod audio;
use audio::acoustics;

use acoustics::{Analyzer, FrameInfo, GpuAcoustics, Synth};
use eframe::wgpu;
use std::collections::HashMap;

const W: u32 = gpu::W;
const H: u32 = gpu::H;
const N: u64 = (W as u64) * (H as u64);
const SIM_SRC: &str = include_str!("../src/sim.wgsl");
const MATTER_SRC: &str = include_str!("../src/matter.wgsl");
const VIS_SRC: &str = include_str!("../src/vis.wgsl");
const LOOK_SRC: &str = include_str!("../src/matter_look.wgsl");
const GPU_RS: &str = include_str!("../src/gpu.rs");

fn env<T: std::str::FromStr>(k: &str, d: T) -> T {
    std::env::var(k).ok().and_then(|v| v.parse().ok()).unwrap_or(d)
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Params {
    w: u32,
    h: u32,
    nops: u32,
    view: u32,
    dt: f32,
    p_fuel: f32,
    fan_pow: f32,
    time: f32,
    ign_on: u32,
    frame: u32,
    srgb: u32,
    _p1: u32,
    p_o2: f32,
    p_air: f32,
    cam: [f32; 4],
    _p2: [f32; 2],
}

fn parse_bindings(src: &str) -> Vec<(u32, String, String)> {
    let mut v: Vec<(u32, String, String)> = Vec::new();
    for line in src.lines() {
        let l = line.trim();
        if l.starts_with("//") || !l.contains("@group(0)") {
            continue;
        }
        let Some(bp) = l.find("@binding(") else { continue };
        let b: u32 = l[bp + 9..].split(')').next().and_then(|s| s.trim().parse().ok()).unwrap_or(99);
        let Some(vp) = l.find("var") else { continue };
        let rest = &l[vp + 3..];
        let (kind, after) = if rest.starts_with('<') {
            let close = rest.find('>').unwrap_or(0);
            (rest[1..close].replace(' ', ""), &rest[close + 1..])
        } else {
            (String::new(), rest)
        };
        let name = after.trim().split(':').next().unwrap_or("").trim().to_string();
        if !v.iter().any(|x| x.0 == b) {
            v.push((b, kind, name));
        }
    }
    v.sort_by_key(|x| x.0);
    v
}

fn parse_wg(src: &str) -> HashMap<String, (u32, u32)> {
    let mut m = HashMap::new();
    let mut rest = src;
    while let Some(p) = rest.find("@workgroup_size(") {
        let r = &rest[p + 16..];
        let close = r.find(')').unwrap_or(0);
        let nums: Vec<u32> = r[..close].split(',').filter_map(|s| s.trim().trim_end_matches('u').parse().ok()).collect();
        let after = &r[close..];
        if let Some(fp) = after.find("fn ") {
            let name: String = after[fp + 3..].chars().take_while(|c| c.is_alphanumeric() || *c == '_').collect();
            m.insert(name, (nums.first().copied().unwrap_or(1), nums.get(1).copied().unwrap_or(1)));
        }
        rest = &r[close..];
    }
    m
}

fn parse_list(src: &str, key: &str) -> Option<Vec<String>> {
    let p = src.find(key)?;
    let r = &src[p..];
    let a = r.find('[')?;
    let b = r[a..].find(']')? + a;
    let v: Vec<String> = r[a + 1..b].split(',').map(|s| s.trim().trim_matches('"').to_string()).filter(|s| !s.is_empty()).collect();
    if v.is_empty() {
        None
    } else {
        Some(v)
    }
}

fn parse_const(src: &str, key: &str) -> Option<u32> {
    let p = src.find(key)?;
    let r = &src[p + key.len()..];
    let e = r.find('=')?;
    r[e + 1..].trim_start().chars().take_while(|c| c.is_ascii_digit()).collect::<String>().parse().ok()
}

/// Minimal copy of GpuSim's compute half.
struct Sim {
    bufs: HashMap<String, wgpu::Buffer>,
    bg: wgpu::BindGroup,
    pipes: HashMap<String, wgpu::ComputePipeline>,
    wg: HashMap<String, (u32, u32)>,
    steps: Vec<String>,
    every_n: usize,
    wall_every: u32,
    step_count: u32,
}

impl Sim {
    fn new(device: &wgpu::Device) -> Sim {
        let src = format!("{}\n{}\n{}\n{}", SIM_SRC, MATTER_SRC, VIS_SRC, LOOK_SRC);
        let binds = parse_bindings(&src);
        let flux = (((W + 1) * H + W * (H + 1)) as u64) * 32;
        let size_of = |name: &str| -> u64 {
            match name {
                "P" => 256,
                "U0" | "U1" | "U2" | "WALL" => N * 32,
                "META" => N * 4,
                "OPS" => (gpu::MAX_OPS * 48) as u64,
                "FLUX" => flux,
                "AUX" => 3 * 8192 * 4, // gpu.rs AUX_WORDS (calm / active / solid tile flags)
                "STATS" => 256,
                _ => N * 64,
            }
        };
        let mut bufs = HashMap::new();
        let mut entries = Vec::new();
        for (b, kind, name) in &binds {
            let uni = kind == "uniform";
            let usage = if uni { wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST } else { wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::COPY_SRC };
            bufs.insert(name.clone(), device.create_buffer(&wgpu::BufferDescriptor { label: Some(name), size: size_of(name), usage, mapped_at_creation: false }));
            let ty = if uni {
                wgpu::BufferBindingType::Uniform
            } else {
                wgpu::BufferBindingType::Storage { read_only: kind == "storage" || kind == "storage,read" }
            };
            entries.push(wgpu::BindGroupLayoutEntry { binding: *b, visibility: wgpu::ShaderStages::COMPUTE, ty: wgpu::BindingType::Buffer { ty, has_dynamic_offset: false, min_binding_size: None }, count: None });
        }
        let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: Some("sim"), entries: &entries });
        let bge: Vec<wgpu::BindGroupEntry> = binds.iter().map(|(b, _, n)| wgpu::BindGroupEntry { binding: *b, resource: bufs[n].as_entire_binding() }).collect();
        let bg = device.create_bind_group(&wgpu::BindGroupDescriptor { label: Some("sim"), layout: &bgl, entries: &bge });
        let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: None, bind_group_layouts: &[&bgl], push_constant_ranges: &[] });
        device.push_error_scope(wgpu::ErrorFilter::Validation);
        let module = unsafe { device.create_shader_module_trusted(wgpu::ShaderModuleDescriptor { label: Some("sim"), source: wgpu::ShaderSource::Wgsl(src.clone().into()) }, wgpu::ShaderRuntimeChecks::unchecked()) };
        if let Some(e) = pollster::block_on(device.pop_error_scope()) {
            panic!("sim module failed to compile (another team mid-edit? retry): {e}");
        }
        let steps = parse_list(GPU_RS, "steps: [").unwrap_or_else(|| ["rk1", "rk2", "post", "wall", "wall_m", "commit"].iter().map(|s| s.to_string()).collect());
        let every_n = GPU_RS.find("if k >= ").and_then(|p| GPU_RS[p + 8..].chars().take_while(|c| c.is_ascii_digit()).collect::<String>().parse().ok()).unwrap_or(3);
        let wall_every = parse_const(GPU_RS, "const WALL_EVERY: u32").unwrap_or(4);
        let mut pipes = HashMap::new();
        let mut names: Vec<String> = vec!["init".into(), "apply_ops".into(), "act".into(), "matter_move_a".into(), "matter_move_b".into()];
        names.extend(steps.iter().cloned());
        for e in names {
            device.push_error_scope(wgpu::ErrorFilter::Validation);
            let p = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some(&e),
                layout: Some(&pl),
                module: &module,
                entry_point: Some(&e),
                compilation_options: wgpu::PipelineCompilationOptions { zero_initialize_workgroup_memory: false, ..Default::default() },
                cache: None,
            });
            match pollster::block_on(device.pop_error_scope()) {
                None => {
                    pipes.insert(e, p);
                }
                Some(err) => eprintln!("skipping kernel `{e}`: {err}"),
            }
        }
        Sim { bufs, bg, pipes, wg: parse_wg(&src), steps, every_n, wall_every, step_count: 0 }
    }

    fn groups(&self, name: &str) -> (u32, u32) {
        let (wx, wy) = self.wg.get(name).copied().unwrap_or((16, 16));
        let (cw, ch) = if name.starts_with("matter_move") { (W / 2 + 1, H / 2 + 1) } else { (W, H) };
        (cw.div_ceil(wx), ch.div_ceil(wy))
    }

    fn disp(&self, pass: &mut wgpu::ComputePass<'_>, name: &str) {
        if let Some(p) = self.pipes.get(name) {
            pass.set_pipeline(p);
            let (gx, gy) = self.groups(name);
            pass.dispatch_workgroups(gx, gy, 1);
        }
    }

    /// The step loop of GpuSim::frame, with the acoustics microphone sampled after every step.
    fn encode(&mut self, enc: &mut wgpu::CommandEncoder, steps: u32, init: bool, ops: bool, mut ac: Option<&mut GpuAcoustics>) {
        if init || ops {
            // gpu.rs clears the calm flags whenever the state is edited outside the solver
            enc.clear_buffer(&self.bufs["AUX"], 0, Some(((W.div_ceil(16) * H.div_ceil(8)) as u64 * 4 + 3) / 4 * 4));
        }
        let mut pass = enc.begin_compute_pass(&wgpu::ComputePassDescriptor { label: Some("sim"), timestamp_writes: None });
        pass.set_bind_group(0, &self.bg, &[]);
        if init {
            self.disp(&mut pass, "init");
        }
        if ops {
            self.disp(&mut pass, "apply_ops");
        }
        let list = self.steps.clone();
        for _ in 0..steps {
            self.step_count += 1;
            let walls_now = self.step_count % self.wall_every == 0;
            if let Some(p) = self.pipes.get("act") {
                // active-tile mask (gpu.rs: N_TILES.div_ceil(64) workgroups)
                pass.set_pipeline(p);
                pass.dispatch_workgroups((W.div_ceil(16) * H.div_ceil(8)).div_ceil(64), 1, 1);
            }
            for (k, name) in list.iter().enumerate() {
                if k >= self.every_n && !walls_now {
                    break;
                }
                self.disp(&mut pass, name);
            }
            if walls_now {
                let mv = if self.step_count % (2 * self.wall_every) == 0 { "matter_move_a" } else { "matter_move_b" };
                self.disp(&mut pass, mv);
            }
            if let Some(ac) = ac.as_deref_mut() {
                ac.encode_mic(&mut pass, &self.bg);
            }
        }
    }
}

fn read_buf(device: &wgpu::Device, queue: &wgpu::Queue, src: &wgpu::Buffer, size: u64) -> Vec<u8> {
    let dst = device.create_buffer(&wgpu::BufferDescriptor { label: None, size, usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
    let mut e = device.create_command_encoder(&Default::default());
    e.copy_buffer_to_buffer(src, 0, &dst, 0, size);
    queue.submit([e.finish()]);
    dst.slice(..).map_async(wgpu::MapMode::Read, |_| {});
    device.poll(wgpu::Maintain::Wait);
    let v = dst.slice(..).get_mapped_range().to_vec();
    v
}

/// HIP=1 diagnostics: where are the high-pressure gas cells and the fast (jet) cells at the end of the recording?
fn hi_pressure_report(device: &wgpu::Device, queue: &wgpu::Queue, sim: &Sim) {
    let u: Vec<f32> = bytemuck::cast_slice(&read_buf(device, queue, &sim.bufs["U0"], N * 32)).to_vec();
    let m: Vec<u32> = bytemuck::cast_slice(&read_buf(device, queue, &sim.bufs["META"], N * 4)).to_vec();
    let rs = [1.80625f32, 0.903125, 1.032142857, 1.111538462];
    let cv = [5.827f32, 2.258, 2.580, 4.446];
    let mut hi: Vec<(f32, u32, u32, f32, f32, u32)> = Vec::new();
    let mut fast = 0;
    let (mut fx, mut fy) = (0.0f64, 0.0f64);
    let mut fs = [0.0f64; 6];
    for i in 0..(W * H) as usize {
        let t = m[i] & 0xff;
        if matches!(t, 1 | 6 | 8) { continue; }
        let c = &u[i * 8..i * 8 + 8];
        let rho = (c[0] + c[1] + c[2] + c[3]).max(0.002);
        let r: f32 = (0..4).map(|k| c[k] * rs[k]).sum();
        let cvv: f32 = (0..4).map(|k| c[k] * cv[k]).sum();
        let ke = 0.5 * (c[4] * c[4] + c[5] * c[5]) / rho;
        let p = (r / cvv.max(1e-6) * (c[6] - ke)).max(0.002);
        let tt = p / r.max(1e-6);
        let spd = (c[4] * c[4] + c[5] * c[5]).sqrt() / rho;
        let x = (i as u32) % W;
        let y = (i as u32) / W;
        // solid neighbours (pocket indicator)
        let mut sol = 0;
        for (dx, dy) in [(-1i32, 0i32), (1, 0), (0, -1), (0, 1)] {
            let (ax, ay) = (x as i32 + dx, y as i32 + dy);
            if ax < 0 || ay < 0 || ax >= W as i32 || ay >= H as i32 { continue; }
            if matches!(m[(ay as u32 * W + ax as u32) as usize] & 0xff, 1 | 6 | 8) { sol += 1; }
        }
        if p > 2.0 { hi.push((p, x, y, tt * 300.0, spd * 293.0, sol)); }
        if spd > 1.5 {
            fast += 1;
            fx += x as f64;
            fy += y as f64;
            let uy = c[5] / rho / spd;
            let cc = ((1.0 + r / cvv.max(1e-6)) * p / rho).sqrt();
            fs[0] += uy as f64;
            fs[1] += (uy < -0.7) as u32 as f64;
            fs[2] += (tt * 300.0) as f64;
            fs[3] += p as f64;
            fs[4] += (spd / cc) as f64;
            fs[5] += (spd > 0.45 * cc) as u32 as f64;
        }
    }
    hi.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap());
    println!("    HIP: {} gas cells > 2 atm, {} cells > 440 m/s (centroid {:.0},{:.0})", hi.len(), fast, fx / fast.max(1) as f64, fy / fast.max(1) as f64);
    let nf = fast.max(1) as f64;
    println!("    fast cells: mean uy/|u| {:+.2}, rising frac {:.2}, mean T {:.0} K, mean p {:.2} atm, mean Mach {:.2}, Mach>0.45 frac {:.2}", fs[0] / nf, fs[1] / nf, fs[2] / nf, fs[3] / nf, fs[4] / nf, fs[5] / nf);
    for h in hi.iter().take(4) {
        println!("      p {:.2} atm at ({}, {}) T {:.0} K |u| {:.0} m/s solid nbrs {}", h.0, h.1, h.2, h.3, h.4, h.5);
    }
}

fn load_scene(name: &str) -> scene::Scene {
    match name {
        "rocket" => scene::Scene::rocket(),
        "flare" => scene::Scene::flare(),
        "campfire" => scene::Scene::campfire(),
        "srm" => scene::Scene::solid_rocket(),
        "demo" => scene::Scene::demolition(),
        "chem" => scene::Scene::chem_lab(),
        "jet" => scene::Scene::turbojet(),
        "fireball" => scene::Scene::fireball(),
        o => scene::Scene::by_name(o).unwrap_or_else(scene::Scene::empty),
    }
}

/// (listener cell, warm-up frames before recording) per scene
fn scene_setup(name: &str) -> ([f32; 2], u32) {
    match name {
        "flare" => ([200.0, 200.0], 240),
        // the torch that lights the pile is a violent (pressure-driven) heat op: record the fire once it has caught
        "campfire" => ([196.0, 250.0], 240),
        "jet" => ([330.0, 250.0], 200),
        "rocket" => ([200.0, 245.0], 80),
        "srm" => ([300.0, 200.0], 10),
        "demo" => ([180.0, 250.0], 0),
        "chem" => ([250.0, 230.0], 0),
        "fireball" => ([170.0, 220.0], 0),
        // bubbling / fizzing references (Minnaert bubbles): listener just above the water
        "sodium" => ([256.0, 205.0], 0),
        "acids" => ([90.0, 220.0], 0),
        _ => ([256.0, 144.0], 0),
    }
}

/// in-place radix-2 FFT (re, im)
fn fft(re: &mut [f32], im: &mut [f32]) {
    let n = re.len();
    let mut j = 0;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j |= bit;
        if i < j {
            re.swap(i, j);
            im.swap(i, j);
        }
    }
    let mut len = 2;
    while len <= n {
        let ang = -2.0 * std::f32::consts::PI / len as f32;
        for i in (0..n).step_by(len) {
            for k in 0..len / 2 {
                let (c, s) = ((ang * k as f32).cos(), (ang * k as f32).sin());
                let (a, b) = (i + k, i + k + len / 2);
                let tr = re[b] * c - im[b] * s;
                let ti = re[b] * s + im[b] * c;
                re[b] = re[a] - tr;
                im[b] = im[a] - ti;
                re[a] += tr;
                im[a] += ti;
            }
        }
        len <<= 1;
    }
}

/// Octave-band levels (dB re full scale) and spectral centroid of interleaved stereo audio, per segment.
fn spectrum_report(audio: &[f32], sr: u32, seg_s: f32) {
    let n = 4096;
    let centres = [31.5f32, 63.0, 125.0, 250.0, 500.0, 1000.0, 2000.0, 4000.0, 8000.0, 16000.0];
    let seg = (seg_s * sr as f32) as usize;
    let frames = audio.len() / 2;
    let mut t0 = 0;
    println!("    octave bands (dBFS)  31   63  125  250  500   1k   2k   4k   8k  16k | centroid  L/R balance");
    while t0 + n <= frames {
        let t1 = (t0 + seg).min(frames);
        let mut pow = vec![0.0f64; n / 2];
        let mut cnt = 0;
        let (mut el, mut er) = (0.0f64, 0.0f64);
        let mut p = t0;
        while p + n <= t1 {
            let mut re: Vec<f32> = (0..n).map(|k| 0.5 * (audio[2 * (p + k)] + audio[2 * (p + k) + 1]) * (0.5 - 0.5 * (2.0 * std::f32::consts::PI * k as f32 / n as f32).cos())).collect();
            let mut im = vec![0.0f32; n];
            fft(&mut re, &mut im);
            for k in 0..n / 2 {
                pow[k] += (re[k] * re[k] + im[k] * im[k]) as f64;
            }
            for k in 0..n {
                el += (audio[2 * (p + k)] as f64).powi(2);
                er += (audio[2 * (p + k) + 1] as f64).powi(2);
            }
            cnt += 1;
            p += n;
        }
        if cnt == 0 {
            break;
        }
        let df = sr as f32 / n as f32;
        let mut line = String::new();
        let (mut num, mut den) = (0.0f64, 0.0f64);
        for (k, v) in pow.iter().enumerate() {
            num += v * (k as f64 * df as f64);
            den += v;
        }
        for c in centres {
            let (lo, hi) = (c / 2f32.sqrt(), c * 2f32.sqrt());
            let e: f64 = pow.iter().enumerate().filter(|(k, _)| { let f = *k as f32 * df; f >= lo && f < hi }).map(|(_, v)| *v).sum();
            // window power normalisation (Hann: sum w^2 = 3n/8), one-sided
            let lvl = 10.0 * (2.0 * e / (cnt as f64 * 0.375 * (n * n) as f64) + 1e-12).log10();
            line.push_str(&format!("{:>5.0}", lvl));
        }
        let bal = 10.0 * ((er + 1e-12) / (el + 1e-12)).log10();
        println!("    {:>4.1}-{:>4.1}s          {} | {:>6.0} Hz  {:+.1} dB", t0 as f32 / sr as f32, t1 as f32 / sr as f32, line, num / den.max(1e-20), bal);
        t0 = t1;
    }
}

// ---------------------------------------------------------------- feature-frame dump / replay
// DUMP=1 writes <OUT>/<scene>.frames (all read-back frames, grouped by the GPU frame they were harvested on);
// REPLAY=1 re-runs only the CPU half (Analyzer + Synth) from that file: identical audio, no GPU, ~1 s per scene.
fn dump_frames(path: &str, groups: &[Vec<acoustics::FeatureFrame>]) {
    let mut v: Vec<u32> = Vec::new();
    v.push(groups.len() as u32);
    for g in groups {
        v.push(g.len() as u32);
        for ff in g {
            let i = ff.info;
            v.push(i.steps);
            for x in [i.sim_time, i.real_dt, i.speed, i.listener[0], i.listener[1], i.cam[0], i.cam[1], i.cam[2], i.cam[3], i.fan_pow] {
                v.push(x.to_bits());
            }
            v.push(i.paused as u32 | ((i.reset as u32) << 1));
            v.push(ff.mic.len() as u32);
            for m in &ff.mic {
                v.extend(m.iter().map(|x| x.to_bits()));
            }
            v.push(ff.part.len() as u32);
            v.extend(ff.part.iter().map(|x| x.to_bits()));
        }
    }
    let _ = std::fs::write(path, bytemuck::cast_slice::<u32, u8>(&v));
}

fn load_frames(path: &str) -> Option<Vec<Vec<acoustics::FeatureFrame>>> {
    let b = std::fs::read(path).ok()?;
    let v: Vec<u32> = b.chunks_exact(4).map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect();
    let mut p = 0usize;
    let mut nx = || {
        p += 1;
        v[p - 1]
    };
    let ng = nx() as usize;
    let mut out = Vec::with_capacity(ng);
    for _ in 0..ng {
        let n = nx() as usize;
        let mut g = Vec::with_capacity(n);
        for _ in 0..n {
            let steps = nx();
            let mut f = [0.0f32; 10];
            for x in f.iter_mut() {
                *x = f32::from_bits(nx());
            }
            let fl = nx();
            let info = FrameInfo { steps, sim_time: f[0], real_dt: f[1], speed: f[2], listener: [f[3], f[4]], cam: [f[5], f[6], f[7], f[8]], fan_pow: f[9], paused: fl & 1 != 0, reset: fl & 2 != 0 };
            let nm = nx() as usize;
            let mic = (0..nm).map(|_| [f32::from_bits(nx()), f32::from_bits(nx()), f32::from_bits(nx()), f32::from_bits(nx())]).collect();
            let np = nx() as usize;
            let part = (0..np).map(|_| f32::from_bits(nx())).collect();
            g.push(acoustics::FeatureFrame { info, mic, part });
        }
        out.push(g);
    }
    Some(out)
}

/// CPU half of a render: analysis + synthesis + console report.
struct Rec {
    an: Analyzer,
    syn: Synth,
    audio: Vec<f32>,
    shocks: u32,
    breaks: u32,
    sr: u32,
}

impl Rec {
    fn new(sr: u32) -> Rec {
        let mut syn = Synth::new(sr as f32);
        syn.master_target = env("MASTER", 0.9);
        syn.bus.wet = env("REVERB", audio::BUS_WET);
        // diagnostics: per-layer mix overrides
        syn.mix.lfe = env("MIX_LFE", syn.mix.lfe);
        syn.mix.microphone = env("MIX_MIC", syn.mix.microphone);
        syn.mix.combustion = env("MIX_COMB", syn.mix.combustion);
        syn.mix.jet = env("MIX_JET", syn.mix.jet);
        syn.mix.crackle = env("MIX_CRACKLE", syn.mix.crackle);
        syn.mix.machinery = env("MIX_MACH", syn.mix.machinery);
        syn.mix.structure = env("MIX_STRUCT", syn.mix.structure);
        Rec { an: Analyzer::new(), syn, audio: Vec::new(), shocks: 0, breaks: 0, sr }
    }

    fn feed_ctl(&mut self, ctl: acoustics::Ctl, render: bool) {
        self.syn.push(ctl);
        if render {
            let n = (self.sr as f32 / 60.0) as usize;
            self.syn.render(n, &mut self.audio);
        }
    }

    /// frames harvested on recorded GPU frame `fr_rec`; `render`: one frame of audio is due
    fn feed(&mut self, ffs: Vec<acoustics::FeatureFrame>, fr_rec: u32, fps: f32, render: bool) {
        let sr = self.sr;
        for ff in ffs {
            let ctl = self.an.process(&ff);
            for e in &ctl.events {
                match e {
                    acoustics::Ev::Shock { dp, .. } => {
                        self.shocks += 1;
                        if self.shocks <= 12 {
                            println!("    shock at t={:.2}s dp={:.0} Pa ({:.0} dB peak)", self.audio.len() as f32 / (2.0 * sr as f32), dp, acoustics::spl(*dp));
                        }
                    }
                    acoustics::Ev::Break { amount, f1, .. } => {
                        self.breaks += 1;
                        if self.breaks <= 6 {
                            println!("    break at t={:.2}s cells={amount} f1={f1:.0} Hz", self.audio.len() as f32 / (2.0 * sr as f32));
                        }
                    }
                }
            }
            let t = self.an.tel;
            if fr_rec % 60 == 30 {
                let b = ctl.bands.iter().fold((0.0f32, 0.0f32, 0.0f32, 0.0f32, 0.0f32, 0.0f32), |a, b| (a.0.max(b.comb.p), a.1.max(b.fss.p), a.2.max(b.turb.p), a.3 + b.wood.rate + b.prop.rate + b.mburn.rate, a.4 + b.boil.rate + b.fizz.rate, a.5.max(b.fan.p)));
                println!(
                    "  t={:>4.1}s heat {:>9.0} W  |dQ/dt| {:>9.2e} W/s  comb {:>5.1} dB  jet U {:>5.0} m/s D {:.3} L {:.2} m N {:>5.0} -> {:>5.1} dB  turb {:>5.1} dB  fan {:>5.1} dB @ {:.0} Hz  pops {:>5.1}/s bubbles {:>5.0}/s  mic {:>6.0} -> {:>5.0} Pa wind {:>5.1} Pa pmax {:.2}",
                    fr_rec as f32 / fps,
                    t.heat_w,
                    t.dqdt_ws,
                    acoustics::spl(b.0),
                    t.jet_u,
                    t.jet_d,
                    t.jet_l,
                    t.jet_n,
                    acoustics::spl(b.1),
                    acoustics::spl(b.2),
                    acoustics::spl(b.5),
                    t.fan_bpf,
                    b.3,
                    b.4,
                    t.mic_env,
                    t.mic_out,
                    t.wind,
                    t.pmax
                );
                if std::env::var("DEBUG").is_ok() {
                    for (k, b) in ctl.bands.iter().enumerate() {
                        if b.comb.p > 1e-4 || b.fss.p > 1e-4 || b.hiss.p > 1e-4 || b.turb.p > 1e-4 {
                            println!("      band {k}: comb {:.3} Pa {:.0} Hz pan {:+.2} r {:.2} | fss {:.3} Pa {:.0} Hz lss {:.3} Pa {:.0} Hz rumble {:.3} Pa {:.0} Hz | turb {:.3} Pa {:.0} Hz | hiss {:.4} Pa {:.0} Hz | wood {:.1}/s crackle {:.0}/s {:.2} Pa | creak {:.1}/s", b.comb.p, b.comb.f, b.comb.pan, b.comb.r, b.fss.p, b.fss.f, b.lss.p, b.lss.f, b.rumble.p, b.rumble.f, b.turb.p, b.turb.f, b.hiss.p, b.hiss.f, b.wood.rate, b.jet_crackle.rate, b.jet_crackle.amp, b.creak.rate);
                        }
                    }
                }
            }
            self.syn.push(ctl);
        }
        if render {
            self.syn.render((sr as f32 / fps).round() as usize, &mut self.audio);
        }
    }
}

fn finish(name: &str, out_dir: &str, rec: &Rec, dropped: u32, t_start: std::time::Instant) {
    let sr = rec.sr;
    let audio = &rec.audio;
    let path = format!("{out_dir}/{name}.wav");
    let mut pk = 0.0f32;
    let mut ss = 0.0f64;
    let mut bad = 0;
    for &v in audio {
        if !v.is_finite() {
            bad += 1;
        }
        pk = pk.max(v.abs());
        ss += (v as f64) * (v as f64);
    }
    let rms = (ss / audio.len().max(1) as f64).sqrt();
    acoustics::write_wav(&path, audio, sr).expect("write wav");
    spectrum_report(audio, sr, env("SEG", 2.0));
    println!(
        "{name}: {:.1} s audio, peak {:.3}, rms {:.4} ({:.1} dBFS), non-finite {bad}, shocks {}, breaks {}, dropped {} -> {path}  [{:.1} s wall]",
        audio.len() as f32 / (2.0 * sr as f32),
        pk,
        rms,
        20.0 * rms.max(1e-9).log10(),
        rec.shocks,
        rec.breaks,
        dropped,
        t_start.elapsed().as_secs_f32()
    );
}

fn main() {
    let scenes: Vec<String> = std::env::var("SCENES").unwrap_or_else(|_| "flare,campfire,jet,rocket,demo,chem".into()).split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
    let out_dir = std::env::var("OUT").unwrap_or_else(|_| "/tmp/ignis".into());
    let _ = std::fs::create_dir_all(&out_dir);
    let secs: f32 = env("SECS", 12.0);
    let speed: f32 = env("SPEED", 1.0);
    let sr: u32 = 48_000;
    let fps = 60.0f32;

    if let Ok(kind) = std::env::var("SYNTH") {
        // synthetic controls (no solver): isolates one source model, e.g. SYNTH=explosion|wood|fan
        let mut rec = Rec::new(sr);
        let dp: f32 = env("DP", 20_000.0);
        for fr in 0..(secs * fps) as u32 {
            let mut ctl = acoustics::Ctl { real_dt: 1.0 / fps, steps: 19, paused: false, bands: Default::default(), mic: vec![Default::default(); 10], events: vec![] };
            let t = fr as f32 / fps;
            match kind.as_str() {
                "explosion" => {
                    // a quiet campfire, then one blast at t = 2 s
                    ctl.bands[3].comb = acoustics::Src { p: 0.02, f: 200.0, pan: 0.0, r: 2.0 };
                    if fr == (2.0 * fps) as u32 {
                        ctl.mic[4] = acoustics::MicSample { env: dp, p: dp, pan: 0.0, wind: 0.0 };
                        ctl.events.push(acoustics::Ev::Shock { dp, pan: 0.0, idx: 4, size: 1.0 });
                    }
                }
                "wood" => {
                    ctl.bands[3].comb = acoustics::Src { p: 0.02, f: 200.0, pan: 0.0, r: 2.0 };
                    ctl.bands[3].wood = acoustics::Rate { rate: env("RATE", 40.0), amp: 0.25 / 2.0, pan: 0.0, r: 2.0 };
                }
                "boil" => {
                    // water boiling at ~1 m: bubble rate from the number of boiling cells
                    ctl.bands[3].boil = acoustics::Rate { rate: env("RATE", 600.0), amp: 0.012 / 1.0, pan: 0.0, r: 1.0 };
                }
                _ => {}
            }
            rec.feed_ctl(ctl, fr > 3);
        }
        finish(&kind, &out_dir, &rec, 0, std::time::Instant::now());
        return;
    }

    if std::env::var("REPLAY").is_ok() {
        let src_dir = std::env::var("FRAMES").unwrap_or_else(|_| out_dir.clone());
        for name in &scenes {
            let t_start = std::time::Instant::now();
            let Some(groups) = load_frames(&format!("{src_dir}/{name}.frames")) else {
                eprintln!("{name}: no {src_dir}/{name}.frames (render once with DUMP=1)");
                continue;
            };
            if std::env::var("LIVE").is_ok() {
                // play through the real output device with the app's API (audio::Audio::feed), in real time
                let Some(mut a) = audio::Audio::new() else {
                    println!("no audio device");
                    return;
                };
                a.set_volume(env("VOL", 0.3));
                println!("live: {name}");
                for g in groups {
                    a.feed(g);
                    std::thread::sleep(std::time::Duration::from_secs_f32(1.0 / fps));
                }
                a.set_muted(true);
                std::thread::sleep(std::time::Duration::from_millis(300));
                continue;
            }
            let mut rec = Rec::new(sr);
            for (k, g) in groups.into_iter().enumerate() {
                rec.feed(g, k as u32, fps, k >= 2);
            }
            finish(name, &out_dir, &rec, 0, t_start);
        }
        return;
    }

    let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::default());
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default())).expect("adapter");
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default(), None)).expect("device");
    println!("adapter: {}", adapter.get_info().name);

    if std::env::var("BENCH_AC").is_ok() {
        // GPU cost of the acoustics kernels on a burning scene: 200 feature passes / 2000 mic samples per submit
        let mut sim = Sim::new(&device);
        let sc = load_scene("rocket");
        queue.write_buffer(&sim.bufs["META"], 0, bytemuck::cast_slice(&sc.meta));
        let mut ac = GpuAcoustics::new(&device, &sim.bufs["U0"], &sim.bufs["META"], &sim.bufs["WALL"], W, H);
        ac.prepare(&queue, &FrameInfo::default());
        let mut enc = device.create_command_encoder(&Default::default());
        sim.encode(&mut enc, 400, true, false, None);
        queue.submit([enc.finish()]);
        device.poll(wgpu::Maintain::Wait);
        for (label, kind) in [("features x200", 0), ("mic x2000", 1), ("sim step x200", 2)] {
            let mut best = f64::MAX;
            for _ in 0..3 {
                let mut enc = device.create_command_encoder(&Default::default());
                match kind {
                    0 => {
                        let mut pass = enc.begin_compute_pass(&wgpu::ComputePassDescriptor { label: None, timestamp_writes: None });
                        for _ in 0..200 {
                            ac.encode_features_in(&mut pass);
                        }
                    }
                    1 => {
                        let mut pass = enc.begin_compute_pass(&wgpu::ComputePassDescriptor { label: None, timestamp_writes: None });
                        for _ in 0..2000 {
                            ac.encode_mic(&mut pass, &sim.bg);
                        }
                    }
                    _ => sim.encode(&mut enc, 200, false, false, None),
                }
                let t = std::time::Instant::now();
                queue.submit([enc.finish()]);
                device.poll(wgpu::Maintain::Wait);
                best = best.min(t.elapsed().as_secs_f64());
                ac.harvest(&device);
                let _ = ac.take_frames();
            }
            println!("{label}: {:.3} ms total, {:.4} ms each", best * 1e3, best * 1e3 / if kind == 1 { 2000.0 } else { 200.0 });
        }
        return;
    }
    for name in &scenes {
        let t_start = std::time::Instant::now();
        let mut sc = load_scene(name);
        let (lis, warm0) = scene_setup(name);
        let warm: u32 = env("WARM", warm0);
        let lis = [env("LX", lis[0]), env("LY", lis[1])];
        let mut sim = Sim::new(&device);
        let mut ac = GpuAcoustics::new(&device, &sim.bufs["U0"], &sim.bufs["META"], &sim.bufs["WALL"], W, H);
        ac.jet_m = env("JET_M", ac.jet_m);
        ac.jet_u = env("JET_U", ac.jet_u);
        let mut rec = Rec::new(sr);
        let mut groups: Vec<Vec<acoustics::FeatureFrame>> = Vec::new();
        queue.write_buffer(&sim.bufs["META"], 0, bytemuck::cast_slice(&sc.meta));

        let real_dt = 1.0 / fps;
        let steps_per_frame = gpu::DT.recip() * 32.0 * speed / fps; // TPS = 32 tu per real second
        let mut step_acc = 0.0f32;
        let rec_frames = (secs * fps) as u32;
        let total = warm + rec_frames + 3;
        let mut sim_time = 0.0f32;
        let mut first_rec = true;
        for fr in 0..total {
            step_acc += steps_per_frame;
            let steps = step_acc.floor() as u32;
            step_acc -= steps as f32;
            // feed ramps exactly as the bench (in bench frames of 20 steps)
            let bench_f = sim_time / (20.0 * gpu::DT);
            let r = ((bench_f + 1.0) * 20.0 * gpu::DT / 40.0).min(1.0);
            let r = r * r * (3.0 - 2.0 * r);
            let ops = if fr == 0 { sc.ops.clone() } else { vec![] };
            let nops = ops.len().min(gpu::MAX_OPS);
            sim_time += steps as f32 * gpu::DT;
            let p = Params {
                w: W,
                h: H,
                nops: nops as u32,
                view: 0,
                dt: gpu::DT,
                p_fuel: 1.0 + (sc.inlet_p - 1.0) * r,
                fan_pow: 1.0,
                time: sim_time,
                ign_on: 1,
                frame: fr,
                srgb: 0,
                _p1: 0,
                p_o2: 1.0 + (sc.o2_p - 1.0) * r,
                p_air: 2.0,
                cam: [0.0, 0.0, W as f32, H as f32],
                _p2: [gpu::DT * sim.wall_every as f32, gpu::GRAVITY],
            };
            queue.write_buffer(&sim.bufs["P"], 0, bytemuck::bytes_of(&p));
            if nops > 0 {
                queue.write_buffer(&sim.bufs["OPS"], 0, bytemuck::cast_slice(&ops[..nops]));
            }
            let recording = fr >= warm && std::env::var("NOAC").is_err();
            let info = FrameInfo { steps, sim_time, real_dt, speed, listener: lis, cam: [0.0, 0.0, W as f32, H as f32], fan_pow: 1.0, paused: false, reset: recording && first_rec };
            if recording {
                ac.prepare(&queue, &info);
            }
            let mut enc = device.create_command_encoder(&Default::default());
            sim.encode(&mut enc, steps, fr == 0, nops > 0, if recording { Some(&mut ac) } else { None });
            if recording {
                ac.encode_frame(&device, &mut enc);
                first_rec = false;
            }
            queue.submit([enc.finish()]);
            device.poll(wgpu::Maintain::Wait);
            if recording {
                ac.harvest(&device);
                let ffs = ac.take_frames();
                if std::env::var("DUMP").is_ok() {
                    groups.push(ffs.clone());
                }
                rec.feed(ffs, fr - warm, fps, fr >= warm + 2);
            }
        }
        if std::env::var("HIP").is_ok() {
            hi_pressure_report(&device, &queue, &sim);
        }
        if std::env::var("DUMP").is_ok() {
            dump_frames(&format!("{out_dir}/{name}.frames"), &groups);
        }
        finish(name, &out_dir, &rec, ac.dropped, t_start);
    }
}
