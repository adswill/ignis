//! IGNIS physics-validation harness (read-only science audit, see tools/VALIDATION.md).
//!
//! Standalone: builds its own buffers + compute pipelines from the *same* shader sources the game uses
//! (src/sim.wgsl + src/matter.wgsl + src/vis.wgsl) on a small grid of its own size, so tests can upload exact
//! gas states (premixed mixtures, reservoirs) and read the fields back. Nothing in src/ is modified.
//!
//!   cargo build --release --example physval
//!   ./target/release/examples/physval <test> [args]
//!   tests: tad | flame <phi> [len] [tmax] | flamesweep [tmax] | turb [tmax] | autosweep [tmax] | cvexpl
//!          | nozzlesweep [throat] [inlet tile] [steps] | jet <p0> [throat] [steps] [stride]  (VENT=1: ambient outlets)
//!          | rocket [steps] [p_inj] | rocketp [steps] | boil [id] [steps] | plume [T_src] [g] [steps] (HOLD=n)
//!          | meltsweep [T_gas] [tmax] [u_gas] | matter <id,id,..> [open|closed] [T_ign] [steps] (CH=, PR=)
//!   experiments without editing src: CONST="NAME=VALUE;..."  REPL="old|||new@@@old2|||new2"  SRC_DIR=<dir>
use eframe::wgpu;
use std::collections::HashMap;

const DT: f32 = 0.028;
const WALL_EVERY: u32 = 4;
// gas constants and cv per species (CH4, O2, N2, products) - mirror of sim.wgsl RS / CV
const RS: [f32; 4] = [1.80625, 0.903125, 1.032142857, 1.111538462];
const CV: [f32; 4] = [5.827, 2.258, 2.580, 4.446];
const Y_AIR: [f32; 4] = [0.0, 0.233, 0.767, 0.0];

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Params {
    w: u32, h: u32, nops: u32, view: u32,
    dt: f32, p_fuel: f32, fan_pow: f32, time: f32,
    ign_on: u32, frame: u32, srgb: u32, _p1: u32,
    p_o2: f32, p_air: f32, cam_x: f32, cam_y: f32,
    cam_w: f32, cam_h: f32, wall_dt: f32, gravity: f32,
    _pad: [[f32; 4]; 11], // headroom in case the WGSL Params grows (uniform buffer may be larger than the struct)
}

/// Shader text: the game's sources read at runtime (SRC_DIR, default ./src) so experiments can override
/// constants without touching the files: CONST="Q_FUEL=485;T_EDC=3.5" rewrites `const NAME: T = ...;` lines,
/// REPL="old text|||new text" (several separated by '@@@') does literal replacements.
fn shader_source() -> String {
    let dir = std::env::var("SRC_DIR").unwrap_or("src".into());
    let rd = |f: &str| std::fs::read_to_string(format!("{dir}/{f}")).expect("shader source");
    let mut src = format!("{}\n{}\n{}", rd("sim.wgsl"), rd("matter.wgsl"), rd("vis.wgsl"));
    if let Ok(c) = std::env::var("CONST") {
        for kv in c.split(';').filter(|s| !s.is_empty()) {
            let (k, v) = kv.split_once('=').expect("NAME=VALUE");
            let pat = format!("const {}:", k.trim());
            let mut out = String::new();
            let mut hit = false;
            for line in src.lines() {
                if line.trim_start().starts_with(&pat) {
                    let head = &line[..line.find('=').unwrap() + 1];
                    out.push_str(&format!("{head} {};", v.trim()));
                    hit = true;
                } else { out.push_str(line); }
                out.push('\n');
            }
            assert!(hit, "constant {k} not found");
            eprintln!("override {k} = {v}");
            src = out;
        }
    }
    if let Ok(r) = std::env::var("REPL") {
        for pair in r.split("@@@") {
            let (a, b) = pair.split_once("|||").expect("old|||new");
            assert!(src.contains(a), "REPL text not found: {a}");
            src = src.replace(a, b);
            eprintln!("replaced: {a}");
        }
    }
    src
}

struct Sim {
    device: wgpu::Device,
    queue: wgpu::Queue,
    w: u32,
    h: u32,
    params: Params,
    pbuf: wgpu::Buffer,
    u0: wgpu::Buffer,
    meta: wgpu::Buffer,
    wall: wgpu::Buffer,
    bg: wgpu::BindGroup,
    pipes: HashMap<&'static str, wgpu::ComputePipeline>,
    wg: HashMap<&'static str, (u32, u32)>,
    step_list: Vec<&'static str>,
    n_gas: usize,
    step_count: u32,
    pub time: f32,
}

fn storage(device: &wgpu::Device, size: u64) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: size.max(16),
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    })
}

impl Sim {
    fn new(w: u32, h: u32) -> Sim {
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::default());
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default())).expect("adapter");
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default(), None)).expect("device");
        let n = (w * h) as u64;
        let pbuf = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: std::mem::size_of::<Params>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let u0 = storage(&device, n * 32);
        let u1 = storage(&device, n * 32);
        // vis.wgsl reuses U1/U2 with a quarter-res bloom tail; give them room
        let u2 = storage(&device, n * 32 * 2);
        let u1b = u1;
        let meta = storage(&device, n * 4);
        let wall = storage(&device, n * 32);
        let ops = storage(&device, 256 * 48);
        let flux = storage(&device, (((w + 1) * h + w * (h + 1)) as u64) * 32);
        let stats = storage(&device, 256);
        let rw = |binding: u32, ro: bool| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Storage { read_only: ro }, has_dynamic_offset: false, min_binding_size: None },
            count: None,
        };
        let uni = wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None },
            count: None,
        };
        let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: None,
            entries: &[uni, rw(1, false), rw(2, false), rw(3, false), rw(4, false), rw(5, false), rw(6, true), rw(7, false), rw(8, false)],
        });
        let bufs = [&pbuf, &u0, &u1b, &u2, &meta, &wall, &ops, &flux, &stats];
        let entries: Vec<_> = bufs.iter().enumerate().map(|(k, b)| wgpu::BindGroupEntry { binding: k as u32, resource: b.as_entire_binding() }).collect();
        let bg = device.create_bind_group(&wgpu::BindGroupDescriptor { label: None, layout: &bgl, entries: &entries });
        let src = shader_source();
        // workgroup size of each entry point, parsed from the source (the physics team fuses/re-tiles kernels)
        let mut wg: HashMap<String, (u32, u32)> = HashMap::new();
        let mut pending: Option<(u32, u32)> = None;
        for line in src.lines() {
            let l = line.trim();
            if let Some(i) = l.find("@workgroup_size(") {
                let inner = &l[i + 16..];
                let inner = &inner[..inner.find(')').unwrap()];
                let v: Vec<u32> = inner.split(',').map(|t| t.trim().trim_end_matches('u').parse().unwrap_or(1)).collect();
                pending = Some((v[0], *v.get(1).unwrap_or(&1)));
            }
            if let (Some(ws), Some(i)) = (pending, l.find("fn ")) {
                let name = &l[i + 3..];
                let name = &name[..name.find('(').unwrap_or(name.len())];
                wg.insert(name.trim().to_string(), ws);
                pending = None;
            }
        }
        let has_turb = wg.contains_key("turb");
        let step_list: Vec<&'static str> = if has_turb { vec!["rk1", "rk2", "turb", "post", "wall", "wall_copy", "commit"] } else { vec!["rk1", "rk2", "post", "wall", "wall_copy", "commit"] };
        let n_gas = if has_turb { 4 } else { 3 };
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: None, source: wgpu::ShaderSource::Wgsl(src.into()) });
        let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: None, bind_group_layouts: &[&bgl], push_constant_ranges: &[] });
        let mut pipes = HashMap::new();
        for e in ["init", "rk1", "rk2", "turb", "post", "wall", "wall_copy", "commit", "matter_move_a", "matter_move_b"] {
            if !wg.contains_key(e) { continue; }
            let p = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some(e),
                layout: Some(&pl),
                module: &module,
                entry_point: Some(e),
                compilation_options: wgpu::PipelineCompilationOptions { zero_initialize_workgroup_memory: false, ..Default::default() },
                cache: None,
            });
            pipes.insert(e, p);
        }
        let params = Params {
            w, h, nops: 0, view: 0,
            dt: DT, p_fuel: 1.0, fan_pow: 1.0, time: 0.0,
            ign_on: 0, frame: 0, srgb: 0, _p1: 0,
            p_o2: 1.0, p_air: 1.0, cam_x: 0.0, cam_y: 0.0,
            cam_w: w as f32, cam_h: h as f32, wall_dt: DT * WALL_EVERY as f32, gravity: 0.0,
            _pad: [[0.0; 4]; 11],
        };
        let wg: HashMap<&'static str, (u32, u32)> = pipes.keys().map(|k| (*k, wg[*k])).collect();
        let mut s = Sim { device, queue, w, h, params, pbuf, u0, meta, wall, bg, pipes, wg, step_list, n_gas, step_count: 0, time: 0.0 };
        s.dispatch(&["init"]);
        s
    }
    fn n(&self) -> usize { (self.w * self.h) as usize }
    fn dispatch(&mut self, names: &[&str]) {
        self.queue.write_buffer(&self.pbuf, 0, bytemuck::bytes_of(&self.params));
        let mut enc = self.device.create_command_encoder(&Default::default());
        {
            let mut pass = enc.begin_compute_pass(&Default::default());
            pass.set_bind_group(0, &self.bg, &[]);
            for nm in names {
                pass.set_pipeline(&self.pipes[nm]);
                let ws = self.wg[nm];
                pass.dispatch_workgroups(self.w.div_ceil(ws.0), self.h.div_ceil(ws.1), 1);
            }
        }
        self.queue.submit([enc.finish()]);
    }
    /// advance `steps` gas steps (walls/matter every 4th step, like gpu.rs)
    fn run(&mut self, steps: u32) {
        self.queue.write_buffer(&self.pbuf, 0, bytemuck::bytes_of(&self.params));
        let mut done = 0;
        while done < steps {
            let chunk = (steps - done).min(400);
            let mut enc = self.device.create_command_encoder(&Default::default());
            {
                let mut pass = enc.begin_compute_pass(&Default::default());
                pass.set_bind_group(0, &self.bg, &[]);
                let (gx, gy) = (self.w.div_ceil(16), self.h.div_ceil(16));
                for _ in 0..chunk {
                    self.step_count += 1;
                    let walls = self.step_count % WALL_EVERY == 0;
                    // rk kernels use 16x8 workgroups
                    for (k, nm) in self.step_list.iter().enumerate() {
                        if k >= self.n_gas && !walls { break; }
                        pass.set_pipeline(&self.pipes[nm]);
                        let ws = self.wg[nm];
                        pass.dispatch_workgroups(self.w.div_ceil(ws.0), self.h.div_ceil(ws.1), 1);
                    }
                    if walls {
                        let mv = if self.step_count % (2 * WALL_EVERY) == 0 { "matter_move_a" } else { "matter_move_b" };
                        pass.set_pipeline(&self.pipes[mv]);
                        pass.dispatch_workgroups((self.w / 2 + 1).div_ceil(16), (self.h / 2 + 1).div_ceil(16), 1);
                    }
                }
            }
            self.queue.submit([enc.finish()]);
            done += chunk;
        }
        self.time += steps as f32 * DT;
        self.params.time = self.time;
    }
    fn upload_state(&self, s: &[f32]) { self.queue.write_buffer(&self.u0, 0, bytemuck::cast_slice(s)); }
    fn upload_meta(&self, m: &[u32]) { self.queue.write_buffer(&self.meta, 0, bytemuck::cast_slice(m)); }
    fn upload_wall(&self, wv: &[f32]) {
        // both halves (current + next)
        self.queue.write_buffer(&self.wall, 0, bytemuck::cast_slice(wv));
        self.queue.write_buffer(&self.wall, (wv.len() * 4) as u64, bytemuck::cast_slice(wv));
    }
    fn read(&self, buf: &wgpu::Buffer, bytes: u64) -> Vec<u8> {
        let st = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: bytes,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let mut enc = self.device.create_command_encoder(&Default::default());
        enc.copy_buffer_to_buffer(buf, 0, &st, 0, bytes);
        self.queue.submit([enc.finish()]);
        st.slice(..).map_async(wgpu::MapMode::Read, |_| {});
        self.device.poll(wgpu::Maintain::Wait);
        let v = st.slice(..).get_mapped_range().to_vec();
        v
    }
    fn state(&self) -> Vec<f32> { bytemuck::cast_slice(&self.read(&self.u0, self.n() as u64 * 32)).to_vec() }
    fn walls(&self) -> Vec<f32> { bytemuck::cast_slice(&self.read(&self.wall, self.n() as u64 * 16)).to_vec() }
    fn metas(&self) -> Vec<u32> { bytemuck::cast_slice(&self.read(&self.meta, self.n() as u64 * 4)).to_vec() }
}

// ---- gas thermo mirror ----
fn dot4(a: &[f32], b: &[f32; 4]) -> f32 { a[0] * b[0] + a[1] * b[1] + a[2] * b[2] + a[3] * b[3] }
fn gas_state(y: [f32; 4], p: f32, t: f32) -> [f32; 8] {
    let rm = dot4(&y, &RS);
    let rho = p / (rm * t);
    [y[0] * rho, y[1] * rho, y[2] * rho, y[3] * rho, 0.0, 0.0, p * dot4(&y, &CV) / rm, 0.0]
}
#[derive(Debug, Clone, Copy)]
struct Cell { rho: f32, u: f32, v: f32, p: f32, t: f32, y: [f32; 4], soot: f32 }
fn cell(s: &[f32], i: usize) -> Cell {
    let c = &s[i * 8..i * 8 + 8];
    let rho = (c[0] + c[1] + c[2] + c[3]).max(0.002);
    let (u, v) = (c[4] / rho, c[5] / rho);
    let gm1 = dot4(&c[0..4], &RS) / dot4(&c[0..4], &CV);
    let p = gm1 * (c[6] - 0.5 * rho * (u * u + v * v));
    let rm = dot4(&c[0..4], &RS) / rho;
    Cell { rho, u, v, p, t: p / (rho * rm), y: [c[0] / rho, c[1] / rho, c[2] / rho, c[3] / rho], soot: c[7] }
}
/// premixed CH4-like fuel + air at equivalence ratio phi (mass fractions)
fn premix(phi: f32) -> [f32; 4] {
    let fa = phi * 0.233 / 4.0;
    [fa / (1.0 + fa), 0.233 / (1.0 + fa), 0.767 / (1.0 + fa), 0.0]
}
/// fuel mole (volume) fraction of a mixture
fn xfuel(y: [f32; 4]) -> f32 { y[0] * RS[0] / dot4(&y, &RS) }

fn meta_id(tile: u32, mat: u32) -> u32 { tile | (mat << 8) }

fn arg<T: std::str::FromStr>(a: &[String], k: usize, d: T) -> T { a.get(k).and_then(|s| s.parse().ok()).unwrap_or(d) }

// ---- tests ----

/// 1-D freely propagating premixed flame (open both ends, no walls, no gravity).
/// Returns (flame speed relative to unburnt gas, burnt-gas T, peak T, pressure jump, extinguished?)
fn flame(phi: f32, len: u32, tmax_time: f32, verbose: bool) -> (f32, f32, f32, bool) {
    let (w, h) = (len, 8u32);
    let mut sim = Sim::new(w, h);
    let y = premix(phi);
    let mut st = vec![0f32; (w * h) as usize * 8];
    for j in 0..h {
        for i in 0..w {
            let k = (j * w + i) as usize;
            // ignition kernel: burnt-ish hot pocket at the left (open) end
            let db = y[0].min(y[1] / 4.0);
            let yb = [y[0] - db, y[1] - 4.0 * db, y[2], 5.0 * db];
            let c = if i >= 2 && i < 14 { gas_state(yb, 1.0, tad_theory(phi).0.max(4.5)) } else { gas_state(y, 1.0, 1.0) };
            st[k * 8..k * 8 + 8].copy_from_slice(&c);
        }
    }
    sim.upload_state(&st);
    // pressure outlets (vent tiles, held at 1 atm ambient) at both ends
    let mut meta = vec![0u32; (w * h) as usize];
    for j in 0..h { for i in [0u32, 1, w - 2, w - 1] { meta[(j * w + i) as usize] = 7; } }
    sim.upload_meta(&meta);
    let row = (h / 2) as usize;
    let mut hist: Vec<(f32, f32)> = vec![];
    let mut uhist: Vec<f32> = vec![];
    let mut tb = 0.0;
    let mut tpk = 0.0f32;
    let chunk = 200;
    loop {
        sim.run(chunk);
        let s = sim.state();
        // front = right-most cell whose fuel is below half of the fresh value (interpolated)
        let mut front = -1.0f32;
        let ypb = 5.0 * y[0].min(y[1] / 4.0);
        for x in 8..w as usize - 12 {
            let c = cell(&s, row * w as usize + x);
            tpk = tpk.max(c.t);
            if c.y[3] > 0.5 * ypb { front = x as f32; } else if x > 16 { break; }
        }
        // fresh-gas velocity just ahead of the front, burnt T 25 cells behind
        let fx = front.max(0.0) as usize;
        let ahead = cell(&s, row * w as usize + (fx + 8).min(w as usize - 1));
        if fx > 30 { tb = cell(&s, row * w as usize + fx - 25).t; }
        hist.push((sim.time, front));
        uhist.push(ahead.u);
        if verbose {
            println!("t {:7.1} front {:6.1} u_ahead {:+.4} p_ahead {:.4} Tb {:.3}", sim.time, front, ahead.u, ahead.p, tb);
        }
        if front > w as f32 - 40.0 || sim.time > tmax_time || front < 0.0 { break; }
    }
    // fit speed over the second half of the history
    let pts: Vec<_> = hist.iter().filter(|p| p.1 > 30.0).cloned().collect();
    let ext = pts.len() < 4;
    let sp = if !ext {
        let k0 = pts.len() * 2 / 5;
        let q = &pts[k0..];
        let n = q.len() as f32;
        let (mt, mx) = (q.iter().map(|p| p.0).sum::<f32>() / n, q.iter().map(|p| p.1).sum::<f32>() / n);
        let num: f32 = q.iter().map(|p| (p.0 - mt) * (p.1 - mx)).sum();
        let den: f32 = q.iter().map(|p| (p.0 - mt) * (p.0 - mt)).sum();
        num / den.max(1e-9)
    } else { 0.0 };
    let nu = uhist.len();
    let ufresh = if nu > 4 { uhist[nu * 2 / 5..].iter().sum::<f32>() / (nu - nu * 2 / 5) as f32 } else { 0.0 };
    // flame speed relative to fresh gas: fresh gas is pushed by expansion when ignition is at an open end only
    // weakly; we report the lab-frame front speed minus fresh-gas velocity measured at the end
    if verbose { println!("lab speed {sp:.4}  fresh-gas u {ufresh:.4}"); }
    (sp - ufresh, tb, tpk, ext)
}

fn tad_theory(phi: f32) -> (f32, f32) {
    // complete combustion with the sim's constants: Q_FUEL per fuel mass, constant species cv
    let q = 360.0;
    let y = premix(phi);
    let db = y[0].min(y[1] / 4.0);
    let yp = [y[0] - db, y[1] - 4.0 * db, y[2], 5.0 * db];
    let cvp = dot4(&yp, &CV);
    let cpp = cvp + dot4(&yp, &RS);
    (1.0 + q * db / cpp, 1.0 + q * db / cvp)
}

fn real_tad(phi: f32) -> f32 {
    // equilibrium constant-pressure Tad of CH4-air, 300 K, 1 atm (GRI-Mech/Cantera reference values)
    let tab = [(0.5, 1480.0), (0.6, 1665.0), (0.7, 1838.0), (0.8, 1997.0), (0.9, 2134.0), (1.0, 2226.0), (1.1, 2211.0), (1.2, 2137.0), (1.3, 2057.0), (1.4, 1980.0), (1.5, 1910.0)];
    interp(&tab, phi)
}
fn real_sl(phi: f32) -> f32 {
    // laminar burning velocity CH4-air 300 K 1 atm, cm/s (consensus of Vagelopoulos & Egolfopoulos 1998, Park 2011, Varea 2012)
    let tab = [(0.6, 9.0), (0.7, 17.5), (0.8, 26.0), (0.9, 33.0), (1.0, 37.0), (1.1, 38.0), (1.2, 32.0), (1.3, 22.0), (1.4, 14.0), (1.5, 9.0)];
    interp(&tab, phi)
}
fn interp(tab: &[(f32, f32)], x: f32) -> f32 {
    if x <= tab[0].0 { return tab[0].1; }
    for k in 1..tab.len() {
        if x <= tab[k].0 {
            let (a, b) = (tab[k - 1], tab[k]);
            return a.1 + (b.1 - a.1) * (x - a.0) / (b.0 - a.0);
        }
    }
    tab[tab.len() - 1].1
}

/// 0-D (uniform box, edges are zero-gradient so a uniform state stays uniform): ignition delay.
fn autoign(y: [f32; 4], t0: f32, tmax: f32) -> (f32, f32) {
    let (w, h) = (16u32, 8u32);
    let mut sim = Sim::new(w, h);
    let mut st = vec![0f32; (w * h) as usize * 8];
    for k in 0..(w * h) as usize { st[k * 8..k * 8 + 8].copy_from_slice(&gas_state(y, 1.0, t0)); }
    sim.upload_state(&st);
    let mut tig = -1.0;
    let mut chunk = 1u32;
    let mut tend = t0;
    while sim.time < tmax {
        sim.run(chunk);
        let s = sim.state();
        let c = cell(&s, (4 * w + 8) as usize);
        tend = c.t;
        if c.t > t0 + 0.5 * (tad_cv_of(y) - 1.0) && tig < 0.0 { tig = sim.time; break; }
        chunk = (chunk * 2).min(2000);
    }
    (tig, tend)
}
fn tad_cv_of(y: [f32; 4]) -> f32 {
    let db = y[0].min(y[1] / 4.0);
    let yp = [y[0] - db, y[1] - 4.0 * db, y[2], 5.0 * db];
    1.0 + 360.0 * db / dot4(&yp, &CV)
}

/// closed-vessel (constant volume) explosion: uniform premix, ignited everywhere with a tiny nudge.
fn cvexpl(phi: f32) -> (f32, f32) {
    let (w, h) = (16u32, 8u32);
    let mut sim = Sim::new(w, h);
    let y = premix(phi);
    let mut st = vec![0f32; (w * h) as usize * 8];
    for k in 0..(w * h) as usize { st[k * 8..k * 8 + 8].copy_from_slice(&gas_state(y, 1.0, 4.5)); }
    sim.upload_state(&st);
    sim.run(4000);
    let s = sim.state();
    let c = cell(&s, (4 * w + 8) as usize);
    // isochoric heating from 4.5 -> product state; report the rise relative to the 300 K start equivalent
    (c.t, c.p)
}

/// build a planar converging nozzle fed by an air-inlet plenum held at p0 (tile 4 inlet, T=1).
/// Returns (mass flow through the throat per unit depth, theory, exit Mach)
fn nozzle(p0: f32, throat: u32, steps: u32, inlet_tile: u32) -> (f32, f32, Vec<f32>) {
    let (w, h) = (256u32, 96u32);
    let mut sim = Sim::new(w, h);
    let mut meta = vec![0u32; (w * h) as usize];
    let cy = h as i32 / 2;
    let half_ch = 30i32; // chamber half height
    let x_in = 4i32;
    let x_conv = 60i32; // start of convergence
    let x_th = 90i32; // throat
    for y in 0..h as i32 {
        for x in 0..w as i32 {
            let k = (y * w as i32 + x) as usize;
            let dy = (y - cy).abs();
            if x <= x_th {
                // half height of the gas passage at x
                let hh = if x < x_conv { half_ch } else {
                    let f = (x - x_conv) as f32 / (x_th - x_conv) as f32;
                    (half_ch as f32 + (throat as f32 / 2.0 - half_ch as f32) * f).round() as i32
                };
                if dy < hh { if x < x_in { meta[k] = meta_id(inlet_tile, 0); } }
                else if dy < hh + 6 { meta[k] = meta_id(1, 4); }
            }
        }
    }
    if std::env::var("VENT").is_ok() {
        // ambient pressure outlets on the right/top/bottom edges (downstream of the nozzle)
        for y in 0..h as i32 { for x in 0..w as i32 {
            let k = (y * w as i32 + x) as usize;
            if x > x_th + 2 && (x >= w as i32 - 2 || y < 2 || y >= h as i32 - 2) { meta[k] = 7; }
        } }
    }
    sim.upload_meta(&meta);
    let mut wv = vec![0f32; (w * h) as usize * 4];
    for k in 0..(w * h) as usize { wv[k * 4] = 1.0; wv[k * 4 + 3] = 1.0; }
    sim.upload_wall(&wv);
    sim.params.p_air = p0;
    sim.params.p_o2 = p0;
    sim.params.p_fuel = p0;
    sim.run(steps);
    let s = sim.state();
    // mass flux through the throat column, plus axial profile
    let mut md = 0.0;
    for y in 0..h as i32 {
        let k = (y * w as i32 + x_th) as usize;
        if meta[k] & 0xff == 0 { md += s[k * 8 + 4]; }
    }
    let gam: f32 = 1.4;
    let gg = gam.sqrt() * (2.0 / (gam + 1.0)).powf((gam + 1.0) / (2.0 * (gam - 1.0)));
    let theory = gg * p0 * throat as f32 / (1.0f32 * 1.0).sqrt();
    let mut prof = vec![];
    for x in 0..w as i32 {
        let c = cell(&s, (cy * w as i32 + x) as usize);
        prof.push(c.p);
        let a = (1.4 * c.p / c.rho).sqrt();
        prof.push(c.u / a);
    }
    (md, theory, prof)
}


// ---------- more tests ----------

fn fill(w: u32, h: u32, c: [f32; 8]) -> Vec<f32> {
    let mut st = vec![0f32; (w * h) as usize * 8];
    for k in 0..(w * h) as usize { st[k * 8..k * 8 + 8].copy_from_slice(&c); }
    st
}
fn wall_init(w: u32, h: u32) -> Vec<f32> {
    let mut wv = vec![0f32; (w * h) as usize * 4];
    for k in 0..(w * h) as usize { wv[k * 4] = 1.0; wv[k * 4 + 3] = 1.0; }
    wv
}

/// 2-D flame through decaying synthetic turbulence of rms u'. Returns (S_T relative to fresh gas, u'_measured)
fn turb_flame(up: f32, len: u32, h: u32, tmax: f32) -> (f32, f32) {
    let w = len;
    let mut sim = Sim::new(w, h);
    let y = premix(1.0);
    let db = y[0].min(y[1] / 4.0);
    let yb = [y[0] - db, y[1] - 4.0 * db, y[2], 5.0 * db];
    let ypb = 5.0 * db;
    let mut st = vec![0f32; (w * h) as usize * 8];
    // streamfunction from a few random-phase modes (wavelengths 8..32 cells), periodic in y
    let mut seed = 12345u32;
    let mut rnd = || { seed = seed.wrapping_mul(1664525).wrapping_add(1013904223); (seed >> 8) as f32 / 16777216.0 };
    let modes: Vec<(f32, f32, f32, f32)> = (0..24).map(|_| {
        let lam = 8.0 + 24.0 * rnd();
        let th = 6.2832 * rnd();
        (6.2832 / lam * th.cos(), 6.2832 / lam * th.sin(), 6.2832 * rnd(), lam)
    }).collect();
    let psi = |x: f32, yy: f32| -> f32 { modes.iter().map(|m| (m.3 / 6.2832) * (m.0 * x + m.1 * yy + m.2).sin()).sum() };
    let mut uv = vec![(0f32, 0f32); (w * h) as usize];
    let mut ss = 0.0;
    for j in 0..h { for i in 0..w {
        let (x, yy) = (i as f32, j as f32);
        let u = (psi(x, yy + 0.5) - psi(x, yy - 0.5));
        let v = -(psi(x + 0.5, yy) - psi(x - 0.5, yy));
        uv[(j * w + i) as usize] = (u, v);
        ss += u * u + v * v;
    } }
    let rms = (ss / (2.0 * (w * h) as f32)).sqrt().max(1e-9);
    for j in 0..h { for i in 0..w {
        let k = (j * w + i) as usize;
        let mut c = if i >= 2 && i < 14 { gas_state(yb, 1.0, tad_theory(1.0).0) } else { gas_state(y, 1.0, 1.0) };
        if i >= 14 {
            let rho = c[0] + c[1] + c[2] + c[3];
            let (u, v) = (uv[k].0 * up / rms, uv[k].1 * up / rms);
            c[4] = rho * u; c[5] = rho * v; c[6] += 0.5 * rho * (u * u + v * v);
        }
        st[k * 8..k * 8 + 8].copy_from_slice(&c);
    } }
    sim.upload_state(&st);
    let mut meta = vec![0u32; (w * h) as usize];
    for j in 0..h { for i in [0u32, 1, w - 2, w - 1] { meta[(j * w + i) as usize] = 7; } }
    sim.upload_meta(&meta);
    let mut hist = vec![];
    loop {
        sim.run(200);
        let s = sim.state();
        let mut burnt = 0.0;
        let mut ufr = 0.0f32; let mut nfr = 0.0f32;
        let mut up2 = 0.0f32;
        for j in 0..h as usize { for x in 4..w as usize - 12 {
            let c = cell(&s, j * w as usize + x);
            burnt += (c.y[3] / ypb).clamp(0.0, 1.0);
            if c.y[3] < 0.02 { ufr += c.u; nfr += 1.0; up2 += c.v * c.v; }
        } }
        let xb = burnt / h as f32;
        hist.push((sim.time, xb, ufr / nfr.max(1.0), (up2 / nfr.max(1.0)).sqrt()));
        if xb > w as f32 - 80.0 || sim.time > tmax || nfr < 1.0 { break; }
    }
    let k0 = hist.len() * 2 / 5;
    let q = &hist[k0..];
    let n = q.len() as f32;
    let (mt, mx) = (q.iter().map(|p| p.0).sum::<f32>() / n, q.iter().map(|p| p.1).sum::<f32>() / n);
    let num: f32 = q.iter().map(|p| (p.0 - mt) * (p.1 - mx)).sum();
    let den: f32 = q.iter().map(|p| (p.0 - mt) * (p.0 - mt)).sum();
    let uf = q.iter().map(|p| p.2).sum::<f32>() / n;
    let vrms = q.iter().map(|p| p.3).sum::<f32>() / n;
    (num / den.max(1e-9) - uf, vrms)
}

/// Rocket (same layout style as scene.rs rocket()): showerhead injector with 1 CH4 row per `per` rows,
/// all inlets at `pin`, igniters behind the face, chamber -> sin-contoured throat -> bell (area ratio 4).
/// Returns [Pc static, Tc, mdot, O/F, c*, u_exit, thrust, mdot_exit, A_throat]
fn rocket(pin: f32, per: i32, steps: u32) -> [f32; 9] {
    let (w, h) = (256u32, 96u32);
    let mut sim = Sim::new(w, h);
    let mut meta = vec![0u32; (w * h) as usize];
    let cy = h as i32 / 2;
    let (x_face, x_conv, x_th, x_exit) = (8i32, 88i32, 108i32, 168i32);
    let half_at = |x: i32| -> i32 {
        if x < x_conv { 22 } else if x < x_th {
            let t = (x - x_conv) as f32 / (x_th - x_conv) as f32; (22.0 - 17.0 * (t * 1.5708).sin()).round() as i32
        } else { let t = (x - x_th) as f32 / (x_exit - x_th) as f32; (5.0 + 15.0 * t.powf(0.6)).round() as i32 }
    };
    let mut put = |x: i32, y: i32, v: u32| { if x >= 0 && y >= 0 && x < w as i32 && y < h as i32 { meta[(y * w as i32 + x) as usize] = v; } };
    for x in x_face..=x_exit { let hh = half_at(x); for t in 0..6 { let m = if t < 2 { 6 } else { 0 }; put(x, cy - hh - 1 - t, meta_id(1, m)); put(x, cy + hh + t, meta_id(1, m)); } }
    for x in x_face - 5..x_face { for y in cy - 28..cy + 28 { put(x, y, meta_id(1, 0)); } }
    for y in cy - 22..cy + 22 { put(x_face, y, meta_id(if (y - cy).rem_euclid(per) == 0 { 2 } else { 3 }, 0)); }
    for x in x_face + 3..x_face + 6 { put(x, cy - 23, meta_id(6, 0)); put(x, cy + 22, meta_id(6, 0)); }
    // ambient pressure outlets on the far edges
    for y in 0..h as i32 { for x in 0..w as i32 { if x >= w as i32 - 2 || ((y < 2 || y >= h as i32 - 2) && x > x_exit) { put(x, y, 7); } } }
    sim.upload_meta(&meta);
    sim.upload_wall(&wall_init(w, h));
    sim.upload_state(&fill(w, h, gas_state(Y_AIR, 1.0, 1.0)));
    sim.params.ign_on = 1;
    // ramp the feed like the game does (avoids a cold-start blow-out)
    let ramp = 1500u32;
    for k in 0..15u32 {
        let r = (k + 1) as f32 / 15.0;
        sim.params.p_fuel = 1.0 + (pin - 1.0) * r;
        sim.params.p_o2 = 1.0 + (pin - 1.0) * r;
        sim.run(ramp / 15);
    }
    sim.run(steps);
    let mut acc = [0f32; 9];
    let navg = 10;
    let hth = half_at(x_th);
    let inside = |y: i32| y >= cy - hth && y < cy + hth;
    let at = (0..h as i32).filter(|&y| inside(y) && meta[(y * w as i32 + x_th) as usize] & 0xff == 0).count() as f32;
    for _ in 0..navg {
        sim.run(100);
        let s = sim.state();
        let (mut pc, mut tc, mut nc) = (0.0, 0.0, 0.0);
        for y in (cy - 12)..(cy + 12) { for x in 60..85 {
            let c = cell(&s, (y * w as i32 + x) as usize);
            let a2 = 1.25 * c.p / c.rho;
            pc += c.p * (1.0 + 0.125 * (c.u * c.u + c.v * c.v) / a2).powf(5.0); tc += c.t; nc += 1.0;
        } }
        let (mut md, mut mf, mut mo) = (0.0, 0.0, 0.0);
        for y in 0..h as i32 {
            let k = (y * w as i32 + x_th) as usize;
            if meta[k] & 0xff != 0 || !inside(y) { continue; }
            let c = &s[k * 8..k * 8 + 8];
            let rho = c[0] + c[1] + c[2] + c[3];
            let u = c[4] / rho;
            md += c[4];
            mf += u * (c[0] + 0.2 * c[3]);
            mo += u * (c[1] + 0.8 * c[3]);
        }
        let (mut thr, mut ue, mut me) = (0.0, 0.0, 0.0);
        let xe = x_exit - 1;
        for y in 0..h as i32 {
            let k = (y * w as i32 + xe) as usize;
            if meta[k] & 0xff != 0 { continue; }
            if y < cy - half_at(xe) || y >= cy + half_at(xe) { continue; }
            let c = cell(&s, k);
            thr += c.rho * c.u * c.u + (c.p - 1.0); ue += c.rho * c.u * c.u; me += c.rho * c.u;
        }
        let v = [pc / nc, tc / nc, md, mo / mf.max(1e-6), (pc / nc) * at / md.max(1e-6), ue / me.max(1e-6), thr, me, at];
        for k in 0..9 { acc[k] += v[k] / navg as f32; }
    }
    acc
}

/// water pool on an igniter (hot plate). Prints T and remaining mass of the water vs time.
fn boil(id: u32, steps: u32, hot: bool) {
    let (w, h) = (64u32, 48u32);
    let mut sim = Sim::new(w, h);
    let mut meta = vec![0u32; (w * h) as usize];
    let mut wv = wall_init(w, h);
    // steel tray: floor y=40..43, walls x=20,43; igniter strip below the floor
    for x in 20..44 { for y in 40..44 { meta[(y * w + x) as usize] = meta_id(1, 0); } }
    for y in 28..40 { for x in [20u32, 21, 42, 43] { meta[(y * w + x) as usize] = meta_id(1, 0); } }
    for x in 20..44 { meta[(44 * w + x) as usize] = meta_id(6, 0); }
    let mut cells = vec![];
    for y in 34..40 { for x in 22..42 { let k = (y * w + x) as usize; meta[k] = meta_id(8, id); wv[k * 4] = 1.0; wv[k * 4 + 1] = 0.0; cells.push(k); } }
    sim.upload_meta(&meta);
    sim.upload_wall(&wv);
    sim.upload_state(&fill(w, h, gas_state(Y_AIR, 1.0, 1.0)));
    sim.params.ign_on = hot as u32;
    sim.params.gravity = 0.0015;
    let mut t = 0;
    println!("time(units)  ms   nliquid  meanT(K)  minT  maxT  mean_rem  floorT(K)");
    while t < steps {
        sim.run(400);
        t += 400;
        let m = sim.metas();
        let wl = sim.walls();
        let (mut n, mut ts, mut rs, mut tmin, mut tmax) = (0.0f32, 0.0f32, 0.0f32, 99.0f32, 0.0f32);
        for k in 0..(w * h) as usize {
            if m[k] & 0xff == 8 && (m[k] >> 8) & 0xff == id {
                n += 1.0; ts += wl[k * 4]; rs += wl[k * 4 + 3]; tmin = tmin.min(wl[k * 4]); tmax = tmax.max(wl[k * 4]);
            }
        }
        let fl = wl[((41 * w + 32) * 4) as usize];
        println!("{:8.0} {:6.2} {:5} {:7.1} {:6.1} {:6.1} {:.3} {:7.1}", sim.time, sim.time * 0.034, n, 300.0 * ts / n.max(1.0), 300.0 * tmin, 300.0 * tmax, rs / n.max(1.0), 300.0 * fl);
        if n == 0.0 { break; }
    }
    let _ = cells;
}

/// buoyant plume over a hot patch (held at T_src); measures centreline velocity vs height and buoyancy flux.
fn plume(tsrc: f32, grav: f32, steps: u32) {
    let (w, h) = (192u32, 256u32);
    let mut sim = Sim::new(w, h);
    sim.params.gravity = grav;
    let base = fill(w, h, gas_state(Y_AIR, 1.0, 1.0));
    sim.upload_state(&base);
    let mut meta = vec![0u32; (w * h) as usize];
    // floor
    for x in 0..w { for y in h - 4..h { meta[(y * w + x) as usize] = meta_id(1, 5); } }
    sim.upload_meta(&meta);
    sim.upload_wall(&wall_init(w, h));
    let (x0, x1, ys) = (w / 2 - 6, w / 2 + 6, h - 6);
    let mut tfront = vec![];
    let mut done = 0;
    let hold: u32 = std::env::var("HOLD").ok().and_then(|v| v.parse().ok()).unwrap_or(100);
    while done < steps {
        sim.run(hold);
        done += hold;
        // hold the source patch hot (re-upload just those cells)
        let mut s = sim.state();
        for y in ys..h - 4 { for x in x0..x1 {
            let k = (y * w + x) as usize;
            let c = gas_state(Y_AIR, 1.0, tsrc);
            s[k * 8..k * 8 + 8].copy_from_slice(&c);
        } }
        sim.upload_state(&s);
        // plume front: highest row with T > 1.05 on the centreline
        let mut top = h;
        for y in 0..h { if cell(&s, (y * w + w / 2) as usize).t > 1.05 { top = y; break; } }
        tfront.push((sim.time, (h - 6 - top) as f32));
    }
    let s = sim.state();
    println!("plume front height vs time:");
    let every = (tfront.len() / 15).max(1);
    for (k, p) in tfront.iter().enumerate() { if k % every == 0 { println!("  t {:7.0}  z {:5.0}", p.0, p.1); } }
    println!("height  w_centre  T_centre  B(=int g*(1-rho/rho_a)*v dx)  w/B^(1/3)  halfwidth");
    for z in [10u32, 20, 40, 60, 90, 120, 160] {
        let y = h - 6 - z;
        let (mut b, mut wmax, mut tc) = (0.0, 0.0f32, 0.0);
        let mut hw = 0.0;
        for x in 0..w {
            let c = cell(&s, (y * w + x) as usize);
            let ra = c.p; // cold-air density at this pressure (R=1, T=1)
            b += grav * (1.0 - c.rho / ra) * (-c.v);
            if -c.v > wmax { wmax = -c.v; tc = c.t; }
        }
        for x in 0..w { let c = cell(&s, (y * w + x) as usize); if -c.v > 0.5 * wmax { hw += 0.5; } }
        println!("{z:5}  {wmax:.4}  {:6.0}K  {b:.3e}  {:.2}  {hw:.1}", tc * 300.0, wmax / b.max(1e-12).cbrt());
    }
}

/// wall block of material `mat` (8x8 cells) in a furnace of hot products at `tgas` moving at `ugas` (re-imposed
/// every 20 steps). Returns (time of first melted cell, time block gone, surface T at the end of the run)
fn melt(mat: u32, tgas: f32, ugas: f32, tmax: f32) -> (f32, f32, f32) {
    let (w, h) = (48u32, 48u32);
    let mut sim = Sim::new(w, h);
    let mut meta = vec![0u32; (w * h) as usize];
    for y in 20..28 { for x in 20..28 { meta[(y * w + x) as usize] = meta_id(1, mat); } }
    sim.upload_meta(&meta);
    sim.upload_wall(&wall_init(w, h));
    let mut hot = gas_state([0.0, 0.0, 0.7, 0.3], 1.0, tgas);
    let rho = hot[0] + hot[1] + hot[2] + hot[3];
    hot[4] = rho * ugas; hot[6] += 0.5 * rho * ugas * ugas;
    sim.upload_state(&fill(w, h, hot));
    let mut first = -1.0;
    let mut tsurf = 0.0;
    while sim.time < tmax {
        sim.run(40);
        let m = sim.metas();
        let k = (20 * w + 23) as usize;
        tsurf = sim.walls()[k * 4];
        let nleft = (20..28).flat_map(|y| (20..28).map(move |x| (y * w + x) as usize)).filter(|&k| m[k] & 0xff == 1).count();
        if first < 0.0 && nleft < 64 { first = sim.time; }
        if nleft == 0 { return (first, sim.time, tsurf); }
        let mut s = sim.state();
        for k in 0..(w * h) as usize { if m[k] & 0xff == 0 { s[k * 8..k * 8 + 8].copy_from_slice(&hot); } }
        sim.upload_state(&s);
    }
    (first, -1.0, tsurf)
}

/// matter reaction test: block of `ids` (cycled in a checkerboard / stripes) lit on its left face
fn matter_test(ids: &[u32], closed: bool, ign_t: f32, steps: u32) {
    let (w, h) = (96u32, 64u32);
    let mut sim = Sim::new(w, h);
    let mut meta = vec![0u32; (w * h) as usize];
    let mut wv = wall_init(w, h);
    let (bx0, bx1, by0, by1) = (30u32, 62u32, 40u32, 56u32);
    if closed {
        for y in 20..60u32 { for x in 24..68u32 {
            if y < 22 || y >= 58 || x < 26 || x >= 66 { meta[(y * w + x) as usize] = meta_id(1, 4); }
        } }
    } else {
        for x in 0..w { for y in 56..60 { meta[(y * w + x) as usize] = meta_id(1, 5); } }
    }
    for y in by0..by1 { for x in bx0..bx1 {
        let id = ids[((x + y) as usize) % ids.len()];
        let k = (y * w + x) as usize;
        meta[k] = meta_id(8, id);
        wv[k * 4] = 1.0;
        wv[k * 4 + 1] = 0.1; // settled (keeps our temperature instead of E_TINIT)
        if x < bx0 + 2 { wv[k * 4] = ign_t; }
    } }
    sim.upload_meta(&meta);
    sim.upload_wall(&wv);
    sim.upload_state(&fill(w, h, gas_state(Y_AIR, 1.0, 1.0)));
    sim.params.gravity = 0.0015;
    let n0 = ((bx1 - bx0) * (by1 - by0)) as f32;
    println!("time  ms  frac_left  T_matter_max(K)  T_gas_max(K)  p_max  molten(24)  slag(25)  burn_front_x");
    let mut t = 0;
    while t < steps {
        let ch: u32 = std::env::var("CH").ok().and_then(|v| v.parse().ok()).unwrap_or(100);
        sim.run(ch);
        t += ch;
        let m = sim.metas();
        let wl = sim.walls();
        let s = sim.state();
        let (mut nleft, mut tm, mut nmol, mut nslag) = (0.0, 0.0f32, 0, 0);
        let mut front = 0;
        for y in 0..h { for x in 0..w {
            let k = (y * w + x) as usize;
            if m[k] & 0xff == 8 {
                let e = (m[k] >> 8) & 0xff;
                if ids.contains(&e) { nleft += wl[k * 4 + 3]; }
                if e == 24 { nmol += 1; } else if e == 25 { nslag += 1; }
                tm = tm.max(wl[k * 4]);
                if wl[k * 4] > 3.0 && x > front { front = x; }
            }
        } }
        let (mut tg, mut pm) = (0.0f32, 0.0f32);
        for k in 0..(w * h) as usize { if m[k] & 0xff == 0 { let c = cell(&s, k); tg = tg.max(c.t); pm = pm.max(c.p); } }
        if t % std::env::var("PR").ok().and_then(|v| v.parse().ok()).unwrap_or(400u32) == 0 || nleft < 1.0 {
            println!("{:6.0} {:6.2} {:.3} {:6.0} {:6.0} {:6.2} {:4} {:4} {:3}", sim.time, sim.time * 0.034, nleft / n0, tm * 300.0, tg * 300.0, pm, nmol, nslag, front);
        }
        if nleft < 1.0 && t > 400 { break; }
    }
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let test = a.get(1).map(|s| s.as_str()).unwrap_or("tad");
    match test {
        "tad" => {
            println!("phi  xCH4%  Tad_p(sim theory K)  Tad_v(sim theory K)  real Tad_p K");
            for k in 0..11 {
                let phi = 0.5 + 0.1 * k as f32;
                let (tp, tv) = tad_theory(phi);
                println!("{phi:.1}  {:5.2}  {:7.0}  {:7.0}  {:7.0}", 100.0 * xfuel(premix(phi)), tp * 300.0, tv * 300.0, real_tad(phi));
            }
        }
        "flame" => {
            let phi = arg(&a, 2, 1.0f32);
            let len = arg(&a, 3, 512u32);
            let r = flame(phi, len, arg(&a, 4, 3000.0f32), true);
            println!("phi {phi} S {:.5} units = {:.2} m/s  Tb {:.0} K  Tpeak {:.0} K ext {}", r.0, r.0 * 293.0, r.1 * 300.0, r.2 * 300.0, r.3);
        }
        "flamesweep" => {
            let tmax = arg(&a, 2, 1500.0f32);
            println!("phi  xCH4%  S_sim(units)  S_sim(m/s)  S/S(1)  real S_L(cm/s)  real ratio  Tb_sim(K)  Tpeak(K)  real Tad(K)");
            let mut s1 = 0.0;
            let mut rows = vec![];
            for phi in [1.0f32, 0.5, 0.6, 0.65, 0.7, 0.75, 0.8, 0.9, 1.1, 1.2, 1.3, 1.5, 1.7, 2.0, 2.5, 3.0, 4.0] {
                let r = flame(phi, 512, tmax, false);
                if phi == 1.0 { s1 = r.0; }
                rows.push((phi, r));
            }
            rows.sort_by(|x, y| x.0.partial_cmp(&y.0).unwrap());
            for (phi, r) in rows {
                println!("{phi:.2}  {:5.2}  {:.5}  {:6.2}  {:.2}  {:5.1}  {:.2}  {:5.0}  {:5.0}  {:5.0}  {}", 100.0 * xfuel(premix(phi)), r.0, r.0 * 293.0, r.0 / s1, real_sl(phi), real_sl(phi) / 37.0, r.1 * 300.0, r.2 * 300.0, real_tad(phi), if r.3 { "NO FLAME" } else { "" });
            }
        }
        "autosweep" => {
            println!("T0(K)  delay(units)  delay(ms)  T_end(K)");
            for t0 in [1.5f32, 1.8, 2.0, 2.3, 2.7, 3.0, 3.33, 3.67, 4.0, 4.5, 5.0, 6.0] {
                let (d, te) = autoign(premix(1.0), t0, arg(&a, 2, 60000.0f32));
                println!("{:5.0}  {:9.2}  {:9.4}  {:5.0}", t0 * 300.0, d, d * 0.034, te * 300.0);
            }
        }
        "cvexpl" => {
            for phi in [0.6f32, 0.8, 1.0, 1.2, 1.5] {
                let (t, p) = cvexpl(phi);
                println!("phi {phi:.1}: T {:.0} K  p {:.2} atm (start 4.5 = 1350 K, 1 atm)", t * 300.0, p);
            }
        }
        "nozzlesweep" => {
            let th = arg(&a, 2, 16u32);
            let tile = arg(&a, 3, 4u32);
            println!("p0(atm)  mdot_sim  mdot_theory  ratio  exit-plane Mach  max Mach");
            for p0 in [1.5f32, 2.0, 3.0, 4.0, 6.0, 8.0, 12.0, 16.0] {
                let (md, th_, prof) = nozzle(p0, th, arg(&a, 4, 6000u32), tile);
                let mx = (0..prof.len() / 2).map(|i| prof[2 * i + 1]).fold(0.0f32, f32::max);
                println!("{p0:5.1}  {md:8.3}  {th_:8.3}  {:.3}  {:.2}  {:.2}", md / th_, prof[2 * 90 + 1], mx);
            }
        }
        "jet" => {
            let p0 = arg(&a, 2, 4.0f32);
            let (_md, _t, prof) = nozzle(p0, arg(&a, 3, 16u32), arg(&a, 4, 8000u32), 4);
            for x in (0..256).step_by(arg(&a, 5, 2usize)) { println!("x {x} p {:.3} M {:.2}", prof[2 * x], prof[2 * x + 1]); }
        }
        "turb" => {
            println!("u'(units)  u'(m/s)  S_T(units)  S_T/S_L  measured v_rms in fresh gas");
            let sl = turb_flame(0.0, 512, 64, arg(&a, 2, 1500.0f32)).0;
            println!("0  0  {sl:.4}  1.00");
            for up in [0.02f32, 0.05, 0.1, 0.2] {
                let (st, vr) = turb_flame(up, 512, 64, arg(&a, 2, 1500.0f32));
                println!("{up:.3}  {:.1}  {st:.4}  {:.2}  {vr:.4}", up * 293.0, st / sl);
            }
        }
        "rocket" => {
            println!("p_inj  1-in-N fuel rows | Pc(atm) Tc(K)  mdot  O/F   c*(m/s)  u_exit(m/s)  Isp_eff(s) = thrust/mdot/g0   A_t");
            let pin = arg(&a, 3, 12.0f32);
            for per in [2i32, 3, 4, 5, 6, 8] {
                let r = rocket(pin, per, arg(&a, 2, 6000u32));
                println!("{pin:.0} {per} | {:6.2} {:5.0} {:7.3} {:5.2} {:6.0} {:6.0} {:6.1} {:.0}", r[0], r[1] * 300.0, r[2], r[3], r[4] * 293.0, r[5] * 293.0, r[6] / r[7].max(1e-6) * 293.0 / 9.81, r[8]);
            }
        }
        "rocketp" => {
            println!("p_inj | Pc(atm) Tc(K) mdot  mdot/Pc  O/F  c*(m/s)");
            for pin in [4.0f32, 8.0, 12.0, 16.0, 24.0] {
                let r = rocket(pin, 4, arg(&a, 2, 6000u32));
                println!("{pin:.0} | {:6.2} {:5.0} {:7.3} {:.3} {:5.2} {:6.0}", r[0], r[1] * 300.0, r[2], r[2] / r[0], r[3], r[4] * 293.0);
            }
        }
        "boil" => boil(arg(&a, 2, 19u32), arg(&a, 3, 40000u32), true),
        "plume" => plume(arg(&a, 2, 3.0f32), arg(&a, 3, 0.0015f32), arg(&a, 4, 8000u32)),
        "meltsweep" => {
            let tg = arg(&a, 2, 10.0f32);
            let ug = arg(&a, 4, 0.0f32);
            println!("gas {:.0} K at {:.0} m/s. mat | first-cell melt (units, ms) | block gone (units) | surface T at end (K) | M_TMELT (K)", tg * 300.0, ug * 293.0);
            for m in 0..7u32 {
                let (f, g, ts) = melt(m, tg, ug, arg(&a, 3, 3000.0f32));
                println!("{m} | {f:8.0} {:7.2} | {g:8.0} | {:6.0}", f * 0.034, ts * 300.0);
            }
        }
        "matter" => {
            let ids: Vec<u32> = a.get(2).map(|s| s.split(',').filter_map(|v| v.parse().ok()).collect()).unwrap_or(vec![57]);
            matter_test(&ids, a.get(3).map(|s| s == "closed").unwrap_or(false), arg(&a, 4, 6.0f32), arg(&a, 5, 20000u32));
        }
        _ => println!("unknown test"),
    }
}
