//! IGNIS GPU / CPU profiler (read-only on the game's sources).
//!
//! Build + run:
//!   cargo build --release --example gpu_profile
//!   ./target/release/examples/gpu_profile                 # default scenes, 19 steps/frame (1x speed)
//!   SCENES=rocket,campfire STEPS=19 WARM=120 FRAMES=40 RES=1536x864,2880x1620 ./target/release/examples/gpu_profile
//!   PROFILE_CSV=/abs/out.csv ...                          # also dump the per-kernel table as CSV
//!   DISPATCH=mirror ...                                   # legacy: 16x16 grid for every kernel (default: from @workgroup_size)
//!
//! What it measures (Metal supports timestamps at pass boundaries only, so per-kernel numbers come from a
//! replica that puts every dispatch in its own compute pass):
//!   A. "real": the game's own `GpuSim` (src/gpu.rs, included via #[path]) driven like the in-app loop.
//!      GPU time of `GpuSim::frame` (all compute + copies) via encoder timestamps, GPU time of the render
//!      pass (`GpuSim::draw`) at each RES, CPU encode time of `frame()`, submit-to-idle wall time.
//!   B. "replica": the same WGSL modules (sim+matter+vis, fx, particles) with bind groups derived by parsing
//!      the WGSL `@group/@binding` declarations and kernel lists parsed from gpu.rs/vfx.rs, so it tracks
//!      other teams' edits. Every dispatch is timed in its own pass -> ranked per-kernel ms/frame.
//!   C. calibration: an empty pass, an empty full-grid dispatch and a 64 B/cell copy kernel
//!      (bandwidth roof) to separate fixed overhead from memory-bound work.
//!   D. memory: buffer/texture accounting + process phys_footprint (`footprint`), RSS.
#![allow(dead_code, unused_imports, unused_variables, unused_mut, clippy::all)]

#[path = "../src/gpu.rs"]
mod gpu;
#[path = "../src/scene.rs"]
mod scene;

use eframe::wgpu;
use std::collections::HashMap;
use std::time::Instant;

const W: u32 = gpu::W;
const H: u32 = gpu::H;
const N: u64 = (W as u64) * (H as u64);

const SIM_SRC: &str = include_str!("../src/sim.wgsl");
const MATTER_SRC: &str = include_str!("../src/matter.wgsl");
const VIS_SRC: &str = include_str!("../src/vis.wgsl");
const FX_SRC: &str = include_str!("../src/fx.wgsl");
const PART_SRC: &str = include_str!("../src/particles.wgsl");
const GPU_RS: &str = include_str!("../src/gpu.rs");
const VFX_RS: &str = include_str!("../src/vfx.rs");

fn env<T: std::str::FromStr>(k: &str, d: T) -> T {
    std::env::var(k).ok().and_then(|v| v.parse().ok()).unwrap_or(d)
}

// ------------------------------------------------------------------------------------------------
// Params replica (must match gpu.rs `Params`, 80 B; written into a 256 B uniform so growth is harmless)
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

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct FxParams {
    dt: f32,
    ph0: f32,
    ph1: f32,
    reset: u32,
    src: u32,
    stime: f32,
    _p: [u32; 2],
}

// ------------------------------------------------------------------------------------------------
// Tiny source parsers (keep the replica in sync with the other teams' edits)

#[derive(Clone, Debug, PartialEq)]
enum BKind {
    Uniform,
    Ro,
    Rw,
    Other(String),
}

/// `@group(g) @binding(b) var<...> NAME: ...` declarations of group 0.
fn parse_bindings(src: &str) -> Vec<(u32, BKind, String)> {
    let mut v = Vec::new();
    for line in src.lines() {
        let l = line.trim();
        if l.starts_with("//") {
            continue;
        }
        let Some(gp) = l.find("@group(") else { continue };
        let g: u32 = l[gp + 7..].split(')').next().and_then(|s| s.trim().parse().ok()).unwrap_or(99);
        if g != 0 {
            continue;
        }
        let Some(bp) = l.find("@binding(") else { continue };
        let b: u32 = l[bp + 9..].split(')').next().and_then(|s| s.trim().parse().ok()).unwrap_or(99);
        let Some(vp) = l.find("var") else { continue };
        let rest = &l[vp + 3..];
        let (kind, after) = if rest.starts_with('<') {
            let close = rest.find('>').unwrap_or(0);
            let q = rest[1..close].replace(' ', "");
            let k = match q.as_str() {
                "uniform" => BKind::Uniform,
                "storage,read_write" => BKind::Rw,
                "storage" | "storage,read" => BKind::Ro,
                o => BKind::Other(o.to_string()),
            };
            (k, &rest[close + 1..])
        } else {
            (BKind::Other(l.to_string()), rest)
        };
        let name = after.trim().split(':').next().unwrap_or("").trim().to_string();
        if !v.iter().any(|(bb, _, _): &(u32, BKind, String)| *bb == b) {
            v.push((b, kind, name));
        }
    }
    v.sort_by_key(|x| x.0);
    v
}

/// entry point -> @workgroup_size
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

/// quoted strings of the first `[ ... ]` after `key` in `src`
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

// ------------------------------------------------------------------------------------------------
// Replica

struct Module {
    label: String,
    bg: wgpu::BindGroup,
    pipes: HashMap<String, wgpu::ComputePipeline>,
    wg: HashMap<String, (u32, u32)>,
}

#[derive(Clone)]
enum Item {
    Disp { m: usize, name: String, groups: (u32, u32) },
    ClearStats,
    CopyLayers,
}

struct Replica {
    bufs: HashMap<String, wgpu::Buffer>,
    sizes: Vec<(String, u64)>,
    mods: Vec<Module>,
    layers: wgpu::Texture,
    layers_n: u32,
    np: u32,
    step_list: Vec<String>,
    vis_list: Vec<String>,
    wall_every: u32,
    /// steps[0..every_n] run every step, the rest only on wall steps (`if k >= N && !walls_now` in gpu.rs)
    every_n: usize,
    step_count: u32,
    fx_t: f32,
    fx_src: u32,
    mirror: bool,
    noise_done: bool,
}

fn mkbuf(device: &wgpu::Device, label: &str, size: u64, uniform: bool) -> wgpu::Buffer {
    let usage = if uniform {
        wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST
    } else {
        wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::COPY_SRC
    };
    device.create_buffer(&wgpu::BufferDescriptor { label: Some(label), size, usage, mapped_at_creation: false })
}

impl Replica {
    fn new(device: &wgpu::Device) -> Self {
        let layers_n = parse_const(VFX_RS, "const LAYERS: u32").unwrap_or(8);
        let noise = parse_const(VFX_RS, "const NOISE: u32").unwrap_or(256) as u64;
        let np = parse_const(PART_SRC, "const NP: u32").unwrap_or(32768);
        let mut sizes = Vec::new();
        let mut bufs = HashMap::new();
        let flux = (((W + 1) * H + W * (H + 1)) as u64) * 32;
        // names as used in the WGSL; sizes as in gpu.rs / vfx.rs
        let known: Vec<(&str, u64, bool)> = vec![
            ("P", 256, true),
            ("FP", 256, true),
            ("U0", N * 32, false),
            ("U1", N * 32, false),
            ("U2", N * 32, false),
            ("META", N * 4, false),
            ("WALL", N * 32, false),
            ("OPS", (gpu::MAX_OPS * 48) as u64, false),
            ("FLUX", flux, false),
            ("STATS", 256, false),
            ("UVB", N * 32, false),
            ("OUT", (layers_n as u64 * N + noise * noise) * 8, false),
            ("PARTS", np as u64 * 32, false),
            ("CTR", 256, false),
        ];
        for (name, size, uni) in known {
            bufs.insert(name.to_string(), mkbuf(device, name, size, uni));
            sizes.push((name.to_string(), size));
        }
        let layers = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("layers"),
            size: wgpu::Extent3d { width: W, height: H, depth_or_array_layers: layers_n },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba16Float,
            usage: wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        sizes.push(("layers tex".into(), layers_n as u64 * N * 8));
        sizes.push(("noise tex".into(), noise * noise * 8));

        let step_list = parse_list(GPU_RS, "steps: [").unwrap_or_else(|| ["rk1", "rk2", "turb", "post", "wall", "wall_copy", "commit"].iter().map(|s| s.to_string()).collect());
        let vis_list = parse_list(GPU_RS, "vis: [").unwrap_or_else(|| ["colorize", "blurh", "blurv", "bdown", "bwideh", "bwidev"].iter().map(|s| s.to_string()).collect());
        let wall_every = parse_const(GPU_RS, "const WALL_EVERY: u32").unwrap_or(4);
        let every_n = GPU_RS.find("if k >= ").and_then(|p| GPU_RS[p + 8..].chars().take_while(|c| c.is_ascii_digit()).collect::<String>().parse().ok()).unwrap_or(4);

        let mut r = Replica {
            bufs,
            sizes,
            mods: vec![],
            layers,
            layers_n,
            np,
            step_list,
            vis_list,
            wall_every,
            every_n,
            step_count: 0,
            fx_t: 0.0,
            fx_src: 0,
            mirror: std::env::var("DISPATCH").map(|v| v == "mirror").unwrap_or(false),
            noise_done: false,
        };
        let sim_src = format!("{}\n{}\n{}", SIM_SRC, MATTER_SRC, VIS_SRC);
        let mut sim_entries: Vec<String> = vec!["init".into(), "apply_ops".into(), "matter_move_a".into(), "matter_move_b".into(), "stats".into()];
        sim_entries.extend(r.step_list.iter().cloned());
        sim_entries.extend(r.vis_list.iter().cloned());
        let m0 = r.build(device, "sim", &sim_src, &sim_entries);
        let m1 = r.build(device, "fx", FX_SRC, &["noisegen".into(), "fx".into()]);
        let m2 = r.build(device, "particles", PART_SRC, &["spark_update".into(), "spark_spawn".into()]);
        r.mods = vec![m0, m1, m2];
        r
    }

    fn build(&mut self, device: &wgpu::Device, label: &str, src: &str, entries: &[String]) -> Module {
        let binds = parse_bindings(src);
        let mut layout_entries = Vec::new();
        for (b, kind, name) in &binds {
            let ty = match kind {
                BKind::Uniform => wgpu::BufferBindingType::Uniform,
                BKind::Ro => wgpu::BufferBindingType::Storage { read_only: true },
                BKind::Rw => wgpu::BufferBindingType::Storage { read_only: false },
                BKind::Other(o) => panic!("{label}: binding {b} ({name}) has unsupported type {o}"),
            };
            layout_entries.push(wgpu::BindGroupLayoutEntry {
                binding: *b,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Buffer { ty, has_dynamic_offset: false, min_binding_size: None },
                count: None,
            });
            if !self.bufs.contains_key(name) {
                // unknown buffer added by another team: give it a generous zeroed scratch buffer
                let size = if *kind == BKind::Uniform { 256 } else { N * 64 };
                eprintln!("  [{label}] note: unknown binding {b} `{name}` -> {} KiB scratch buffer", size / 1024);
                self.bufs.insert(name.clone(), mkbuf(device, name, size, *kind == BKind::Uniform));
                self.sizes.push((name.clone(), size));
            }
        }
        let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: Some(label), entries: &layout_entries });
        let bg_entries: Vec<wgpu::BindGroupEntry> = binds.iter().map(|(b, _, name)| wgpu::BindGroupEntry { binding: *b, resource: self.bufs[name].as_entire_binding() }).collect();
        let bg = device.create_bind_group(&wgpu::BindGroupDescriptor { label: Some(label), layout: &bgl, entries: &bg_entries });
        let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: None, bind_group_layouts: &[&bgl], push_constant_ranges: &[] });
        device.push_error_scope(wgpu::ErrorFilter::Validation);
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some(label), source: wgpu::ShaderSource::Wgsl(src.to_string().into()) });
        if let Some(e) = pollster::block_on(device.pop_error_scope()) {
            panic!("{label}: shader module failed to compile (another team mid-edit?): {e}");
        }
        let mut pipes = HashMap::new();
        for e in entries {
            if pipes.contains_key(e) {
                continue;
            }
            device.push_error_scope(wgpu::ErrorFilter::Validation);
            let p = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some(e),
                layout: Some(&pl),
                module: &module,
                entry_point: Some(e),
                compilation_options: Default::default(),
                cache: None,
            });
            match pollster::block_on(device.pop_error_scope()) {
                None => {
                    pipes.insert(e.clone(), p);
                }
                Some(err) => eprintln!("  [{label}] skipping `{e}`: {err}"),
            }
        }
        Module { label: label.into(), bg, pipes, wg: parse_wg(src), }
    }

    fn groups(&self, m: usize, name: &str) -> (u32, u32) {
        let (wx, wy) = self.mods[m].wg.get(name).copied().unwrap_or((16, 16));
        if name == "noisegen" {
            return (256u32.div_ceil(wx), 256u32.div_ceil(wy));
        }
        if name == "spark_update" {
            return (self.np.div_ceil(wx * wy), 1);
        }
        let (cw, ch) = if name.starts_with("matter_move") { (W / 2 + 1, H / 2 + 1) } else { (W, H) };
        if self.mirror {
            (cw.div_ceil(16), ch.div_ceil(16))
        } else {
            (cw.div_ceil(wx), ch.div_ceil(wy))
        }
    }

    fn disp(&self, m: usize, name: &str) -> Option<Item> {
        if !self.mods[m].pipes.contains_key(name) {
            return None;
        }
        Some(Item::Disp { m, name: name.to_string(), groups: self.groups(m, name) })
    }

    /// The per-frame work list, in the same order as GpuSim::frame + Vfx::frame.
    fn frame_items(&mut self, steps: u32, init: bool, ops: bool) -> Vec<Item> {
        let mut v = Vec::new();
        if init {
            v.extend(self.disp(0, "init"));
        }
        if ops {
            v.extend(self.disp(0, "apply_ops"));
        }
        let list = self.step_list.clone();
        for _ in 0..steps {
            self.step_count += 1;
            let walls_now = self.step_count % self.wall_every == 0;
            for (k, name) in list.iter().enumerate() {
                if k >= self.every_n && !walls_now {
                    break;
                }
                v.extend(self.disp(0, name));
            }
            if walls_now {
                let mv = if self.step_count % (2 * self.wall_every) == 0 { "matter_move_a" } else { "matter_move_b" };
                v.extend(self.disp(0, mv));
            }
        }
        v.push(Item::ClearStats);
        v.extend(self.disp(0, "stats"));
        for name in self.vis_list.clone() {
            v.extend(self.disp(0, &name));
        }
        if !self.noise_done {
            v.extend(self.disp(1, "noisegen"));
            self.noise_done = true;
        }
        v.extend(self.disp(1, "fx"));
        v.extend(self.disp(2, "spark_update"));
        v.extend(self.disp(2, "spark_spawn"));
        v.push(Item::CopyLayers);
        v
    }

    fn write_params(&mut self, queue: &wgpu::Queue, f: u32, steps: u32, sc: &scene::Scene, nops: u32) {
        let r = ((f as f32 + 1.0) * steps.max(1) as f32 * gpu::DT / 40.0).min(1.0);
        let r = r * r * (3.0 - 2.0 * r);
        let p = Params {
            w: W,
            h: H,
            nops,
            view: 0,
            dt: gpu::DT,
            p_fuel: 1.0 + (sc.inlet_p - 1.0) * r,
            fan_pow: 1.0,
            time: f as f32 * steps as f32 * gpu::DT,
            ign_on: 1,
            frame: f,
            srgb: 0,
            _p1: 0,
            p_o2: 1.0 + (sc.o2_p - 1.0) * r,
            p_air: 2.0,
            cam: [0.0, 0.0, W as f32, H as f32],
            _p2: [gpu::DT * self.wall_every as f32, gpu::GRAVITY],
        };
        queue.write_buffer(&self.bufs["P"], 0, bytemuck::bytes_of(&p));
        let dtf = steps as f32 * gpu::DT;
        let t0 = self.fx_t;
        self.fx_t += dtf;
        let t1 = self.fx_t;
        let period = 6.0f32;
        let mut reset = 0;
        for k in 0..2 {
            let o = 0.5 * k as f32;
            if (t1 / period + o).floor() != (t0 / period + o).floor() {
                reset |= 1 << k;
            }
        }
        let fp = FxParams { dt: dtf, ph0: (t1 / period).fract(), ph1: (t1 / period + 0.5).fract(), reset, src: self.fx_src, stime: t1, _p: [0; 2] };
        self.fx_src ^= 1;
        queue.write_buffer(&self.bufs["FP"], 0, bytemuck::bytes_of(&fp));
    }

    fn copy_layers(&self, enc: &mut wgpu::CommandEncoder) {
        enc.copy_buffer_to_texture(
            wgpu::TexelCopyBufferInfo { buffer: &self.bufs["OUT"], layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(W * 8), rows_per_image: Some(H) } },
            wgpu::TexelCopyTextureInfo { texture: &self.layers, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
            wgpu::Extent3d { width: W, height: H, depth_or_array_layers: self.layers_n },
        );
    }

    /// Encode items. `ts`: None = untimed and fused (consecutive dispatches of one module share a pass);
    /// Some(qs) = every item timed in its own pass. Returns the labels of the timed items in query order.
    fn encode(&self, enc: &mut wgpu::CommandEncoder, items: &[Item], ts: Option<&wgpu::QuerySet>, inside_enc: bool) -> Vec<String> {
        let mut labels = Vec::new();
        match ts {
            None => {
                let mut k = 0;
                while k < items.len() {
                    match &items[k] {
                        Item::ClearStats => {
                            enc.clear_buffer(&self.bufs["STATS"], 0, None);
                            k += 1;
                        }
                        Item::CopyLayers => {
                            self.copy_layers(enc);
                            k += 1;
                        }
                        Item::Disp { m, .. } => {
                            let m0 = *m;
                            let mut pass = enc.begin_compute_pass(&wgpu::ComputePassDescriptor { label: None, timestamp_writes: None });
                            pass.set_bind_group(0, &self.mods[m0].bg, &[]);
                            while k < items.len() {
                                let Item::Disp { m, name, groups } = &items[k] else { break };
                                if *m != m0 {
                                    break;
                                }
                                pass.set_pipeline(&self.mods[m0].pipes[name]);
                                pass.dispatch_workgroups(groups.0, groups.1, 1);
                                k += 1;
                            }
                        }
                    }
                }
            }
            Some(qs) => {
                for it in items {
                    let q = (labels.len() * 2) as u32;
                    match it {
                        Item::Disp { m, name, groups } => {
                            let mut pass = enc.begin_compute_pass(&wgpu::ComputePassDescriptor {
                                label: Some(name),
                                timestamp_writes: Some(wgpu::ComputePassTimestampWrites { query_set: qs, beginning_of_pass_write_index: Some(q), end_of_pass_write_index: Some(q + 1) }),
                            });
                            pass.set_bind_group(0, &self.mods[*m].bg, &[]);
                            pass.set_pipeline(&self.mods[*m].pipes[name]);
                            pass.dispatch_workgroups(groups.0, groups.1, 1);
                            labels.push(format!("{}:{}", self.mods[*m].label, name));
                        }
                        Item::ClearStats => {
                            enc.clear_buffer(&self.bufs["STATS"], 0, None);
                        }
                        Item::CopyLayers => {
                            if inside_enc {
                                enc.write_timestamp(qs, q);
                            }
                            self.copy_layers(enc);
                            if inside_enc {
                                enc.write_timestamp(qs, q + 1);
                                labels.push("copy:out->layers".into());
                            }
                        }
                    }
                }
            }
        }
        labels
    }
}

// ------------------------------------------------------------------------------------------------
// Timestamp helper

struct Ts {
    qs: wgpu::QuerySet,
    resolve: wgpu::Buffer,
    read: wgpu::Buffer,
    cap: u32,
    period: f64,
}

impl Ts {
    fn new(device: &wgpu::Device, queue: &wgpu::Queue, cap: u32) -> Self {
        let qs = device.create_query_set(&wgpu::QuerySetDescriptor { label: Some("ts"), ty: wgpu::QueryType::Timestamp, count: cap });
        let resolve = device.create_buffer(&wgpu::BufferDescriptor { label: None, size: cap as u64 * 8, usage: wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC, mapped_at_creation: false });
        let read = device.create_buffer(&wgpu::BufferDescriptor { label: None, size: cap as u64 * 8, usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
        Ts { qs, resolve, read, cap, period: queue.get_timestamp_period() as f64 }
    }
    fn resolve(&self, enc: &mut wgpu::CommandEncoder, n: u32) {
        if n == 0 {
            return;
        }
        enc.resolve_query_set(&self.qs, 0..n, &self.resolve, 0);
        enc.copy_buffer_to_buffer(&self.resolve, 0, &self.read, 0, n as u64 * 8);
    }
    /// blocking read of n raw ticks -> ns
    fn read(&self, device: &wgpu::Device, n: u32) -> Vec<f64> {
        if n == 0 {
            return vec![];
        }
        let sl = self.read.slice(0..n as u64 * 8);
        sl.map_async(wgpu::MapMode::Read, |_| {});
        device.poll(wgpu::Maintain::Wait);
        let v: Vec<f64> = bytemuck::cast_slice::<u8, u64>(&sl.get_mapped_range()).iter().map(|&t| t as f64 * self.period).collect();
        self.read.unmap();
        v
    }
}

fn dur_ms(v: &[f64], k: usize) -> f64 {
    let d = v[2 * k + 1] - v[2 * k];
    if d.is_finite() && d >= 0.0 && d < 1e9 {
        d * 1e-6
    } else {
        0.0
    }
}

fn load_scene(name: &str) -> scene::Scene {
    match name {
        "rocket" => scene::Scene::rocket(),
        "empty" => scene::Scene::empty(),
        "vessels" => scene::Scene::vessels(),
        "flame" => scene::Scene::flame_test(),
        "fireball" => scene::Scene::fireball(),
        "flare" => scene::Scene::flare(),
        "campfire" => scene::Scene::campfire(),
        "srm" => scene::Scene::solid_rocket(),
        "demo" => scene::Scene::demolition(),
        "chem" => scene::Scene::chem_lab(),
        "melt" => scene::Scene::melt_test(),
        "jet" => scene::Scene::turbojet(),
        o => scene::Scene::by_name(o).unwrap_or_else(|| {
            eprintln!("unknown scene {o}, using empty");
            scene::Scene::empty()
        }),
    }
}

fn mean(v: &[f64]) -> f64 {
    if v.is_empty() {
        0.0
    } else {
        v.iter().sum::<f64>() / v.len() as f64
    }
}
fn pct(v: &[f64], p: f64) -> f64 {
    if v.is_empty() {
        return 0.0;
    }
    let mut s = v.to_vec();
    s.sort_by(|a, b| a.partial_cmp(b).unwrap());
    s[((s.len() - 1) as f64 * p).round() as usize]
}

fn footprint_mb() -> String {
    let pid = std::process::id().to_string();
    let rss = std::process::Command::new("ps").args(["-o", "rss=", "-p", &pid]).output().ok().and_then(|o| String::from_utf8(o.stdout).ok()).and_then(|s| s.trim().parse::<f64>().ok()).map(|kb| kb / 1024.0).unwrap_or(0.0);
    let fp = std::process::Command::new("footprint").arg(&pid).output().ok().and_then(|o| String::from_utf8(o.stdout).ok()).unwrap_or_default();
    let phys = fp.lines().find(|l| l.contains("phys_footprint:")).map(|l| l.trim().to_string()).unwrap_or_else(|| "phys_footprint: n/a".into());
    let iog = fp.lines().filter(|l| l.contains("IOAccelerator") || l.contains("IOSurface")).map(|l| l.split_whitespace().take(3).collect::<Vec<_>>().join(" ")).collect::<Vec<_>>().join("; ");
    format!("RSS {rss:.0} MB | {phys} | {iog}")
}

// ------------------------------------------------------------------------------------------------

fn main() {
    let scenes: Vec<String> = std::env::var("SCENES").unwrap_or_else(|_| "empty,rocket,campfire,chem,demo".into()).split(',').map(|s| s.trim().to_string()).collect();
    let steps: u32 = env("STEPS", 19);
    let warm: u32 = env("WARM", 120);
    let frames: u32 = env("FRAMES", 40);
    let res: Vec<(u32, u32)> = std::env::var("RES")
        .unwrap_or_else(|_| "1536x864,2880x1620".into())
        .split(',')
        .filter_map(|s| {
            let mut it = s.split('x');
            Some((it.next()?.parse().ok()?, it.next()?.parse().ok()?))
        })
        .collect();

    let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::default());
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions { power_preference: wgpu::PowerPreference::HighPerformance, ..Default::default() })).expect("adapter");
    let info = adapter.get_info();
    let af = adapter.features();
    let want = wgpu::Features::TIMESTAMP_QUERY | wgpu::Features::TIMESTAMP_QUERY_INSIDE_ENCODERS | wgpu::Features::TIMESTAMP_QUERY_INSIDE_PASSES;
    let feats = af & want;
    println!("# IGNIS gpu_profile");
    println!("adapter: {} ({:?}, {:?})", info.name, info.backend, info.device_type);
    println!(
        "timestamp features: TIMESTAMP_QUERY={} INSIDE_ENCODERS={} INSIDE_PASSES={}",
        feats.contains(wgpu::Features::TIMESTAMP_QUERY),
        feats.contains(wgpu::Features::TIMESTAMP_QUERY_INSIDE_ENCODERS),
        feats.contains(wgpu::Features::TIMESTAMP_QUERY_INSIDE_PASSES)
    );
    if !feats.contains(wgpu::Features::TIMESTAMP_QUERY) {
        eprintln!("no TIMESTAMP_QUERY on this adapter: cannot profile GPU passes");
        return;
    }
    // Encoder-level write_timestamp is exposed on Metal but returns unusable values on Apple GPUs (stage-boundary
    // sampling only), so by default brackets are written by tiny timed passes. TS_ENC=1 opts back in.
    let inside_enc = feats.contains(wgpu::Features::TIMESTAMP_QUERY_INSIDE_ENCODERS) && std::env::var("TS_ENC").is_ok();
    let mut limits = wgpu::Limits::default();
    limits.max_storage_buffers_per_shader_stage = adapter.limits().max_storage_buffers_per_shader_stage.max(8);
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor { label: None, required_features: feats, required_limits: limits, memory_hints: Default::default() }, None)).expect("device");
    println!("grid {W}x{H} = {N} cells | steps/frame {steps} | warm-up {warm} | measured {frames} frames | dispatch {}", if std::env::var("DISPATCH").as_deref() == Ok("mirror") { "mirror(gpu.rs 16x16 grid)" } else { "cover(per @workgroup_size)" });
    println!("mem before: {}", footprint_mb());
    // other GPU users on the machine make every number noisy: list them
    if let Ok(o) = std::process::Command::new("ps").args(["-Ao", "pid=,command="]).output() {
        let me = std::process::id().to_string();
        let others: Vec<String> = String::from_utf8_lossy(&o.stdout).lines().map(|l| l.trim().to_string()).filter(|l| l.contains("target/release") && !l.starts_with(&format!("{me} ")) && !l.contains("rustc") && !l.contains("cargo") && !l.contains("zsh -c") && !l.contains("bash -c") && !l.contains("timeout ")).collect();
        if !others.is_empty() {
            println!("WARNING: {} other GPU-capable processes running during the measurement (numbers will be inflated; trust the min columns):", others.len());
            for l in others.iter().take(6) {
                println!("  {}", &l[..l.len().min(120)]);
            }
        }
    }

    let ts = Ts::new(&device, &queue, 4096);
    let mut csv = String::from("scene,kernel,dispatches_per_frame,ms_per_dispatch,ms_per_frame,pct\n");

    // ---------------- C. calibration ----------------
    {
        let cal_src = format!(
            "@group(0) @binding(0) var<storage, read_write> A: array<vec4<f32>>;\n@group(0) @binding(1) var<storage, read_write> B: array<vec4<f32>>;\n\
             @compute @workgroup_size(16, 16) fn empty_k(@builtin(global_invocation_id) g: vec3<u32>) {{ }}\n\
             @compute @workgroup_size(16, 16) fn copy_k(@builtin(global_invocation_id) g: vec3<u32>) {{ if (g.x >= {W}u || g.y >= {H}u) {{ return; }} let i = g.y * {W}u + g.x; B[2u*i] = A[2u*i] * 1.0001; B[2u*i+1u] = A[2u*i+1u]; }}\n\
             @compute @workgroup_size(16, 16) fn stencil_k(@builtin(global_invocation_id) g: vec3<u32>) {{ if (g.x >= {W}u || g.y >= {H}u) {{ return; }} let x = i32(g.x); let y = i32(g.y); var s = vec4<f32>(0.0); for (var dy = -2; dy <= 2; dy++) {{ for (var dx = -2; dx <= 2; dx++) {{ if (abs(dx) + abs(dy) > 2) {{ continue; }} let j = u32(clamp(y + dy, 0, {H} - 1)) * {W}u + u32(clamp(x + dx, 0, {W} - 1)); s += A[2u*j] + A[2u*j+1u]; }} }} B[2u*(g.y*{W}u+g.x)] = s; }}\n"
        );
        let a = mkbuf(&device, "calA", N * 32, false);
        let b = mkbuf(&device, "calB", N * 32, false);
        let rw = |binding| wgpu::BindGroupLayoutEntry { binding, visibility: wgpu::ShaderStages::COMPUTE, ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Storage { read_only: false }, has_dynamic_offset: false, min_binding_size: None }, count: None };
        let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: None, entries: &[rw(0), rw(1)] });
        let bg = device.create_bind_group(&wgpu::BindGroupDescriptor { label: None, layout: &bgl, entries: &[wgpu::BindGroupEntry { binding: 0, resource: a.as_entire_binding() }, wgpu::BindGroupEntry { binding: 1, resource: b.as_entire_binding() }] });
        let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: None, bind_group_layouts: &[&bgl], push_constant_ranges: &[] });
        let md = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: None, source: wgpu::ShaderSource::Wgsl(cal_src.into()) });
        let mk = |e: &str| device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor { label: Some(e), layout: Some(&pl), module: &md, entry_point: Some(e), compilation_options: Default::default(), cache: None });
        let pe = mk("empty_k");
        let pc = mk("copy_k");
        let pst = mk("stencil_k");
        let reps = 50u32;
        // q0/1: empty pass; q2/3: 1 empty dispatch; q4/5: reps empty dispatches in one pass; q6/7: reps copies in one pass; q8/9: 1 copy; q10/11: reps stencils
        let mut sums = [f64::MAX; 6];
        let rounds = 15;
        for _ in 0..rounds {
            let mut enc = device.create_command_encoder(&Default::default());
            let tw = |q: u32| Some(wgpu::ComputePassTimestampWrites { query_set: &ts.qs, beginning_of_pass_write_index: Some(q), end_of_pass_write_index: Some(q + 1) });
            {
                let _p = enc.begin_compute_pass(&wgpu::ComputePassDescriptor { label: None, timestamp_writes: tw(0) });
            }
            let jobs: [(&wgpu::ComputePipeline, u32, u32); 5] = [(&pe, 1, 2), (&pe, reps, 4), (&pc, reps, 6), (&pc, 1, 8), (&pst, reps, 10)];
            for (p, n, q) in jobs {
                let mut pass = enc.begin_compute_pass(&wgpu::ComputePassDescriptor { label: None, timestamp_writes: tw(q) });
                pass.set_bind_group(0, &bg, &[]);
                pass.set_pipeline(p);
                for _ in 0..n {
                    pass.dispatch_workgroups(W.div_ceil(16), H.div_ceil(16), 1);
                }
            }
            ts.resolve(&mut enc, 12);
            queue.submit([enc.finish()]);
            let v = ts.read(&device, 12);
            for k in 0..6 {
                sums[k] = sums[k].min(dur_ms(&v, k));
            }
        }
        let s: Vec<f64> = sums.to_vec(); // min over rounds (robust to other GPU users)
        let copy_ms = s[3] / reps as f64;
        let gbs = (N as f64 * 64.0) / (copy_ms * 1e-3) / 1e9;
        println!("\n## C. Calibration");
        println!("| probe | ms |\n|---|---|");
        println!("| empty compute pass (timestamps only) | {:.4} |", s[0]);
        println!("| pass with 1 empty full-grid dispatch | {:.4} |", s[1]);
        println!("| empty full-grid dispatch, amortised over {reps} in one pass | {:.4} |", s[2] / reps as f64);
        println!("| 64 B/cell copy (2 vec4 in + 2 vec4 out), amortised | {:.4}  (~{gbs:.0} GB/s effective) |", copy_ms);
        println!("| 64 B/cell copy, single dispatch in its own pass | {:.4} |", s[4]);
        println!("| 13-point radius-2 stencil read of 2 vec4 (rk-like gather, no LDS), amortised | {:.4} |", s[5] / reps as f64);
    }

    for sname in &scenes {
        let sc = load_scene(sname);
        println!("\n## Scene `{sname}`");

        // ---------------- B. replica, per-kernel ----------------
        let mut rep = Replica::new(&device);
        queue.write_buffer(&rep.bufs["META"], 0, bytemuck::cast_slice(&sc.meta));
        let nops = sc.ops.len().min(gpu::MAX_OPS);
        if nops > 0 {
            queue.write_buffer(&rep.bufs["OPS"], 0, bytemuck::cast_slice(&sc.ops[..nops]));
        }
        let t_warm = Instant::now();
        for f in 0..warm {
            rep.write_params(&queue, f, steps, &sc, if f == 0 { nops as u32 } else { 0 });
            let items = rep.frame_items(steps, f == 0, f == 0 && nops > 0);
            let mut enc = device.create_command_encoder(&Default::default());
            rep.encode(&mut enc, &items, None, inside_enc);
            queue.submit([enc.finish()]);
            if f % 16 == 15 {
                device.poll(wgpu::Maintain::Wait);
            }
        }
        device.poll(wgpu::Maintain::Wait);
        let warm_s = t_warm.elapsed().as_secs_f64();
        // fused-timed: wall clock of untimed fused frames (CPU encode + GPU, serialised)
        let mut fused_wall = Vec::new();
        let mut fused_enc = Vec::new();
        for f in warm..warm + frames.min(20) {
            rep.write_params(&queue, f, steps, &sc, 0);
            let items = rep.frame_items(steps, false, false);
            let t = Instant::now();
            let mut enc = device.create_command_encoder(&Default::default());
            rep.encode(&mut enc, &items, None, inside_enc);
            let cb = enc.finish();
            fused_enc.push(t.elapsed().as_secs_f64() * 1e3);
            queue.submit([cb]);
            device.poll(wgpu::Maintain::Wait);
            fused_wall.push(t.elapsed().as_secs_f64() * 1e3);
        }
        // split-timed
        let mut acc: HashMap<String, (f64, u32)> = HashMap::new();
        let mut samples: HashMap<String, Vec<f64>> = HashMap::new();
        let mut order: Vec<String> = Vec::new();
        let mut frame_sum = Vec::new();
        let mut frame_span = Vec::new();
        let base = warm + frames.min(20);
        for f in base..base + frames {
            rep.write_params(&queue, f, steps, &sc, 0);
            let items = rep.frame_items(steps, false, false);
            let mut enc = device.create_command_encoder(&Default::default());
            let labels = rep.encode(&mut enc, &items, Some(&ts.qs), inside_enc);
            let nq = (labels.len() * 2) as u32;
            assert!(nq <= ts.cap, "too many timed items: {}", labels.len());
            ts.resolve(&mut enc, nq);
            queue.submit([enc.finish()]);
            let v = ts.read(&device, nq);
            let mut sum = 0.0;
            for (k, l) in labels.iter().enumerate() {
                let d = dur_ms(&v, k);
                sum += d;
                let e = acc.entry(l.clone()).or_insert((0.0, 0));
                if e.1 == 0 && !order.contains(l) {
                    order.push(l.clone());
                }
                e.0 += d;
                e.1 += 1;
                samples.entry(l.clone()).or_default().push(d);
            }
            frame_sum.push(sum);
            if !v.is_empty() {
                frame_span.push((v[v.len() - 1] - v[0]) * 1e-6);
            }
        }
        // median per dispatch: robust against other processes using the GPU at the same time
        let mut rows: Vec<(String, f64, f64, f64)> = acc
            .iter()
            .map(|(k, (_t, c))| {
                let med = pct(&samples[k], 0.5);
                let n = *c as f64 / frames as f64;
                (k.clone(), n, med, med * n)
            })
            .collect();
        let total: f64 = rows.iter().map(|r| r.3).sum();
        rows.sort_by(|a, b| b.3.partial_cmp(&a.3).unwrap());
        println!("\n### B. Replica per-kernel GPU time ({steps} steps/frame, each dispatch in its own timed pass; median per dispatch, min in brackets)");
        println!("| rank | kernel | dispatches/frame | ms/dispatch | ms/frame | % of GPU frame | groups |\n|---|---|---|---|---|---|---|");
        for (i, (k, n, per, pf)) in rows.iter().enumerate() {
            let short = k.split(':').nth(1).unwrap_or(k);
            let m = if k.starts_with("sim:") { 0 } else if k.starts_with("fx:") { 1 } else { 2 };
            let g = if k.starts_with("copy") { "-".to_string() } else { let (a, b) = rep.groups(m, short); let (wx, wy) = rep.mods[m].wg.get(short).copied().unwrap_or((0, 0)); format!("{a}x{b} of {wx}x{wy}") };
            println!("| {} | `{}` | {:.2} | {:.4} ({:.4}) | {:.3} | {:.1}% | {} |", i + 1, k, n, per, pct(&samples[k], 0.0), pf, 100.0 * pf / total.max(1e-9), g);
            csv.push_str(&format!("{sname},{k},{n:.3},{per:.5},{pf:.4},{:.2}\n", 100.0 * pf / total.max(1e-9)));
        }
        // grouped by stage
        let grp = |pred: &dyn Fn(&str) -> bool| rows.iter().filter(|r| pred(&r.0)).map(|r| r.3).sum::<f64>();
        let rk = grp(&|k| k == "sim:rk1" || k == "sim:rk2");
        let step_other = grp(&|k| ["sim:turb", "sim:post"].contains(&k));
        let wallp = grp(&|k| ["sim:wall", "sim:wall_m", "sim:wall_copy", "sim:commit"].contains(&k) || k.starts_with("sim:matter_move"));
        let vis = grp(&|k| k == "sim:stats" || rep.vis_list.iter().any(|v| k == format!("sim:{v}")));
        let fxp = grp(&|k| k.starts_with("fx:") || k.starts_with("particles:") || k.starts_with("copy"));
        println!("\n| stage | ms/frame | % |\n|---|---|---|");
        for (n, v) in [("flux+RK (rk1, rk2)", rk), ("turb + post (every step)", step_other), ("wall/wall_m/commit/matter (every WALL_EVERY-th step)", wallp), ("stats + colorize + bloom", vis), ("fx + sparks + layer copy", fxp)] {
            println!("| {n} | {v:.3} | {:.1}% |", 100.0 * v / total.max(1e-9));
        }
        println!(
            "\nreplica: sum of kernel medians {:.3} ms/frame (frame-sum p95 {:.3}); first-to-last timestamp span {:.3} ms (includes inter-pass gaps); untimed fused frame wall clock {:.3} ms (CPU encode {:.3} ms); warm-up {warm} frames in {:.2} s = {:.2} ms/frame pipelined",
            total,
            pct(&frame_sum, 0.95),
            mean(&frame_span),
            mean(&fused_wall),
            mean(&fused_enc),
            warm_s,
            warm_s * 1e3 / warm as f64
        );
        println!("per sim step (rk1+rk2+turb+post + 1/{} of wall work): {:.4} ms", rep.wall_every, (rk + step_other + wallp) / steps.max(1) as f64);
        // ---------------- B2. isolated micro-bench + shader experiments ----------------
        if std::env::var("MICRO").map(|v| v != "0").unwrap_or(true) {
            let reps: u32 = env("MICRO_REPS", 5);
            let mut names: Vec<String> = rep.step_list.clone();
            names.extend(["matter_move_a".to_string(), "stats".to_string()]);
            names.extend(rep.vis_list.iter().cloned());
            let base = micro(&device, &queue, &ts, &rep, &rep.mods[0], &names, reps);
            println!("\n### B2. Isolated kernels: {reps} back-to-back dispatches in ONE pass (no per-pass overhead), min of {} rounds", micro_rounds());
            println!("| kernel | ms/dispatch | ratio to 64 B/cell copy |\n|---|---|---|");
            let copy_ref: f64 = std::env::var("COPY_REF").ok().and_then(|v| v.parse().ok()).unwrap_or(0.044);
            for (n, t) in &base {
                println!("| `{n}` | {t:.4} | {:.1}x |", t / copy_ref);
                csv.push_str(&format!("{sname},micro:{n},1,{t:.5},{t:.5},0\n"));
            }
            // shader experiments: same buffers/state, modified WGSL (timing only, results are not physical)
            let sim_src = format!("{}\n{}\n{}", SIM_SRC, MATTER_SRC, VIS_SRC);
            let experiments: Vec<(&str, &str, String, Vec<&str>)> = vec![
                (
                    "wall: matter_update call removed (cost of the inlined matter code in the wall kernel)",
                    "WALL[n + i] = matter_update(i, x, y, m, ntw, w.w, exposed, gp_sum / ex, gp_max, gt_max, o2_sum / ex, dt);",
                    "WALL[n + i] = vec4<f32>(ntw, w.y, w.z, w.w + 0.0 * ex);".to_string(),
                    vec!["wall"],
                ),
                (
                    "wall: gas cells return without the WALL[n+i] copy (ping-pong instead of wall_copy)",
                    "if (tl != 1u && tl != 8u) { WALL[n + i] = w; return; }",
                    "if (tl != 1u && tl != 8u) { return; }".to_string(),
                    vec!["wall"],
                ),
                (
                    "post: absorb_matter_emission path removed",
                    "if (tj == 8u) { s = absorb_matter_emission(s, j, dt); }",
                    "".to_string(),
                    vec!["post"],
                ),
            ];
            println!("\n| experiment | kernel | baseline ms | variant ms | saving |\n|---|---|---|---|---|");
            for (desc, from, to, ks) in experiments {
                if !sim_src.contains(from) {
                    println!("| {desc} | - | - | - | anchor not found (source changed) |");
                    continue;
                }
                let vsrc = sim_src.replacen(from, &to, 1);
                let ents: Vec<String> = ks.iter().map(|s| s.to_string()).collect();
                let vm = rep.build(&device, "variant", &vsrc, &ents);
                // interleave baseline and variant rounds so both see the same GPU contention; keep the minimum
                let mut vt: Vec<(String, f64)> = ents.iter().map(|e| (e.clone(), f64::MAX)).collect();
                let mut bt: Vec<(String, f64)> = vt.clone();
                for _ in 0..4 {
                    for (slot, (k, t)) in bt.iter_mut().zip(micro(&device, &queue, &ts, &rep, &rep.mods[0], &ents, reps)) {
                        slot.1 = slot.1.min(t);
                        let _ = k;
                    }
                    for (slot, (_k, t)) in vt.iter_mut().zip(micro(&device, &queue, &ts, &rep, &vm, &ents, reps)) {
                        slot.1 = slot.1.min(t);
                    }
                }
                for ((k, t), (_, b)) in vt.into_iter().zip(bt) {
                    println!("| {desc} | `{k}` | {b:.4} | {t:.4} | {:.0}% |", 100.0 * (b - t) / b.max(1e-9));
                }
            }
        }
        if sname == &scenes[0] {
            let tot: u64 = rep.sizes.iter().map(|s| s.1).sum();
            println!("\n### D. GPU memory (replica allocation = the game's layout)");
            println!("| resource | MiB |\n|---|---|");
            let mut sz = rep.sizes.clone();
            sz.sort_by(|a, b| b.1.cmp(&a.1));
            for (n, s) in sz.iter().filter(|s| s.1 > 4096) {
                println!("| {n} | {:.2} |", *s as f64 / 1048576.0);
            }
            println!("| **total** | **{:.2}** |", tot as f64 / 1048576.0);
            println!("process after replica alloc: {}", footprint_mb());
        }
        drop(rep);

        // ---------------- A. the real GpuSim ----------------
        let fmt = wgpu::TextureFormat::Bgra8Unorm;
        let mut sim = gpu::GpuSim::new(&device, fmt);
        let mut inp = gpu::FrameInput {
            steps,
            ops: sc.ops.clone(),
            view: 0,
            p_fuel: 1.0,
            p_o2: 1.0,
            p_air: 2.0,
            fan_pow: 1.0,
            gravity: gpu::GRAVITY,
            cam: [0.0, 0.0, W as f32, H as f32],
            ign_on: true,
            time: 0.0,
            frame: 0,
            scene: Some(sc.meta.clone()),
            meta_only: None,
            reset_gas: false,
            probe: Some((W as i32 / 2, H as i32 / 2)),
        };
        let targets: Vec<(wgpu::Texture, wgpu::TextureView)> = res
            .iter()
            .map(|&(w, h)| {
                let t = device.create_texture(&wgpu::TextureDescriptor {
                    label: Some("target"),
                    size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: fmt,
                    usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                    view_formats: &[],
                });
                let v = t.create_view(&Default::default());
                (t, v)
            })
            .collect();
        let advance = |inp: &mut gpu::FrameInput, f: u32| {
            inp.scene = None;
            inp.ops.clear();
            inp.frame = f;
            let r = ((f as f32 + 1.0) * steps.max(1) as f32 * gpu::DT / 40.0).min(1.0);
            let r = r * r * (3.0 - 2.0 * r);
            inp.p_fuel = 1.0 + (sc.inlet_p - 1.0) * r;
            inp.p_o2 = 1.0 + (sc.o2_p - 1.0) * r;
            inp.time += steps as f32 * gpu::DT;
        };
        for f in 0..warm {
            let mut enc = device.create_command_encoder(&Default::default());
            sim.frame(&device, &queue, &mut enc, &inp);
            queue.submit([enc.finish()]);
            advance(&mut inp, f);
            if f % 16 == 15 {
                device.poll(wgpu::Maintain::Wait);
            }
        }
        device.poll(wgpu::Maintain::Wait);
        // Pipelined throughput (how the app runs: frames queued back to back). Wall clock / frames is the
        // steady-state GPU cost per frame as long as CPU encode is faster than the GPU (it is: see CPU row).
        // Pass-boundary bracket timestamps around GpuSim::frame are NOT used: Metal may overlap a resource-free
        // bracket pass with neighbouring encoders, so they give meaningless constants.
        let k_frames = frames.max(10);
        let mut fcount = warm;
        let mut run = |sim: &mut gpu::GpuSim, inp: &mut gpu::FrameInput, st: u32, target: Option<usize>, timed: bool| -> (f64, f64, Vec<f64>) {
            inp.steps = st;
            let mut enc_ms = Vec::new();
            let mut rts = Vec::new();
            // one untimed frame to settle
            let t = Instant::now();
            for f in 0..k_frames {
                let mut enc = device.create_command_encoder(&Default::default());
                let te = Instant::now();
                sim.frame(&device, &queue, &mut enc, inp);
                enc_ms.push(te.elapsed().as_secs_f64() * 1e3);
                if let Some(k) = target {
                    let view = &targets[k].1;
                    let tw = if timed { Some(wgpu::RenderPassTimestampWrites { query_set: &ts.qs, beginning_of_pass_write_index: Some(2 * f), end_of_pass_write_index: Some(2 * f + 1) }) } else { None };
                    let mut rp = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                        label: Some("draw"),
                        color_attachments: &[Some(wgpu::RenderPassColorAttachment { view, resolve_target: None, ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store } })],
                        depth_stencil_attachment: None,
                        timestamp_writes: tw,
                        occlusion_query_set: None,
                    });
                    sim.draw(&mut rp);
                }
                queue.submit([enc.finish()]);
                advance(inp, fcount);
                fcount += 1;
            }
            device.poll(wgpu::Maintain::Wait);
            let per = t.elapsed().as_secs_f64() * 1e3 / k_frames as f64;
            if timed && target.is_some() {
                let mut enc = device.create_command_encoder(&Default::default());
                ts.resolve(&mut enc, 2 * k_frames);
                queue.submit([enc.finish()]);
                let v = ts.read(&device, 2 * k_frames);
                for f in 0..k_frames as usize {
                    rts.push(dur_ms(&v, f));
                }
            }
            (per, mean(&enc_ms), rts)
        };
        // empty-submit baseline
        let t = Instant::now();
        for _ in 0..k_frames {
            queue.submit([device.create_command_encoder(&Default::default()).finish()]);
        }
        device.poll(wgpu::Maintain::Wait);
        let empty_ms = t.elapsed().as_secs_f64() * 1e3 / k_frames as f64;
        let (full, enc_full, _) = run(&mut sim, &mut inp, steps, None, false);
        let (zero, enc_zero, _) = run(&mut sim, &mut inp, 0, None, false);
        let (full2, _, _) = run(&mut sim, &mut inp, steps, None, false);
        let full = full.min(full2);
        let mut rrows = Vec::new();
        for k in 0..targets.len() {
            let (with_draw, _, rts) = run(&mut sim, &mut inp, steps, Some(k), true);
            rrows.push((with_draw, rts));
        }
        let mut lat = Vec::new();
        inp.steps = steps;
        for f in 0..k_frames {
            let t = Instant::now();
            let mut enc = device.create_command_encoder(&Default::default());
            sim.frame(&device, &queue, &mut enc, &inp);
            queue.submit([enc.finish()]);
            device.poll(wgpu::Maintain::Wait);
            lat.push(t.elapsed().as_secs_f64() * 1e3);
            advance(&mut inp, fcount + f);
        }
        println!("\n### A. Real `GpuSim` (src/gpu.rs as built), pipelined throughput");
        println!("| measure | ms/frame |\n|---|---|");
        println!("| empty submit baseline | {:.3} |", empty_ms);
        println!("| GpuSim::frame, {steps} steps (sim + vis + fx + sparks + copies) | **{:.3}** |", full);
        println!("| GpuSim::frame, 0 steps (fixed per-frame cost: stats, colorize, bloom, fx, sparks) | {:.3} |", zero);
        println!("| => per sim step | {:.4} |", (full - zero) / steps.max(1) as f64);
        for (k, &(w, h)) in res.iter().enumerate() {
            let (wd, rts) = &rrows[k];
            println!("| frame + draw() at {w}x{h}: total {:.3}; render pass alone (timestamps) median {:.3} / min {:.3} | {:.3} |", wd, pct(rts, 0.5), pct(rts, 0.0), wd - full);
        }
        println!("| CPU encode of GpuSim::frame ({steps} steps / 0 steps) | {:.3} / {:.3} |", enc_full, enc_zero);
        println!("| serialised latency encode+submit+wait (median / p95) | {:.3} / {:.3} |", pct(&lat, 0.5), pct(&lat, 0.95));
        println!("| => sustainable 1x-speed fps (GPU-bound, no UI) | {:.0} |", 1000.0 / (full + rrows.first().map(|r| r.0 - full).unwrap_or(0.0)).max(1e-3));
        csv.push_str(&format!("{sname},REAL_frame,1,{0:.5},{0:.4},100\n", full));
        csv.push_str(&format!("{sname},REAL_fixed,1,{0:.5},{0:.4},0\n", zero));
        for (k, &(w, h)) in res.iter().enumerate() {
            csv.push_str(&format!("{sname},REAL_render_{w}x{h},1,{0:.5},{0:.4},0\n", pct(&rrows[k].1, 0.5)));
        }
        if let Some(st) = Some(sim.stats) {
            println!("sanity: pmax {:.2} Tmax {:.2} burn {:.3} walls {}", st.max_p, st.max_t, st.burn, st.walls);
        }
        drop(sim);
        drop(targets);
        device.poll(wgpu::Maintain::Wait);
    }
    println!("\nmem at end: {}", footprint_mb());
    if let Ok(p) = std::env::var("PROFILE_CSV") {
        let _ = std::fs::write(&p, csv);
        println!("csv -> {p}");
    }
}

/// `reps` back-to-back dispatches of each kernel inside one timed pass; min over 3 rounds, ms per dispatch.
fn micro(device: &wgpu::Device, queue: &wgpu::Queue, ts: &Ts, rep: &Replica, m: &Module, names: &[String], reps: u32) -> Vec<(String, f64)> {
    let mut out = Vec::new();
    let midx = rep.mods.iter().position(|x| x.label == m.label).unwrap_or(0);
    for n in names {
        let Some(p) = m.pipes.get(n) else { continue };
        let (wx, wy) = m.wg.get(n).copied().unwrap_or((16, 16));
        let (cw, ch) = if n.starts_with("matter_move") { (W / 2 + 1, H / 2 + 1) } else { (W, H) };
        let g = (cw.div_ceil(wx), ch.div_ceil(wy));
        let mut best = f64::MAX;
        for _ in 0..micro_rounds() {
            let mut enc = device.create_command_encoder(&Default::default());
            {
                let mut pass = enc.begin_compute_pass(&wgpu::ComputePassDescriptor {
                    label: Some(n),
                    timestamp_writes: Some(wgpu::ComputePassTimestampWrites { query_set: &ts.qs, beginning_of_pass_write_index: Some(0), end_of_pass_write_index: Some(1) }),
                });
                pass.set_bind_group(0, &m.bg, &[]);
                pass.set_pipeline(p);
                for _ in 0..reps {
                    pass.dispatch_workgroups(g.0, g.1, 1);
                }
            }
            ts.resolve(&mut enc, 2);
            queue.submit([enc.finish()]);
            let v = ts.read(device, 2);
            best = best.min(dur_ms(&v, 0) / reps as f64);
        }
        out.push((n.clone(), best));
    }
    out
}

fn micro_rounds() -> u32 {
    env("MICRO_ROUNDS", 9)
}

fn rep_np() -> u32 {
    parse_const(PART_SRC, "const NP: u32").unwrap_or(32768)
}
