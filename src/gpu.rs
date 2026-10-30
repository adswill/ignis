use bytemuck::{Pod, Zeroable};
use eframe::wgpu;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

#[path = "vfx.rs"]
mod vfx;
use vfx::Vfx;

pub const W: u32 = 512;
pub const H: u32 = 288;
/// Default buoyancy strength.
pub const GRAVITY: f32 = 0.0015;
/// Fixed simulation time step (CFL-safe: speed is capped in the shader).
pub const DT: f32 = 0.028;
/// Adaptive time step (sim.wgsl `act`): dt = clamp(CFL / (max(|u|,|v|) + c), DT, DT_MAX), chosen on the GPU every step from
/// the previous step's wave speed. DT is the floor, so fast/hot scenes (rocket, blasts) run exactly as before and only
/// calm scenes take larger steps.
pub const DT_MAX: f32 = 0.06;
pub const CFL_TARGET: f32 = 0.4;
pub const MAX_OPS: usize = 256;
/// Wall heat/damage passes run every N gas steps (with an N times larger step).
const WALL_EVERY: u32 = 4;
/// Tile size of the workgroup-memory kernels `rk1`/`rk2`/`post` (must match `RK_TX`/`RK_TY` in sim.wgsl).
const RK_TX: u32 = 16;
const RK_TY: u32 = 8;
const N_TILES: u32 = W.div_ceil(RK_TX) * H.div_ceil(RK_TY);
/// AUX layout (sim.wgsl): calm flags at 0, active flags at 8192, solid bits at 16384 (u32 words)
const AUX_WORDS: u64 = 4 * 8192;
/// Word offset of the reciprocating-engine state/config block in AUX (engine.wgsl)
pub const ENG_WORD: u64 = 24576;
const AUX_CALM_BYTES: u64 = ((N_TILES as u64 * 4) + 3) / 4 * 4;
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
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
    /// bit0: sparse (active-tile) stepping
    sflags: u32,
    p_o2: f32,
    p_air: f32,
    cam: [f32; 4],
    _p2: [f32; 2],
    dt_min: f32,
    dt_max: f32,
    cfl: f32,
    tscale: f32,
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub struct Op {
    pub kind: u32,
    /// 0 disc, 1 filled box, 2 hollow box, 3 line
    pub shape: u32,
    pub x0: i32,
    pub y0: i32,
    pub x1: i32,
    pub y1: i32,
    pub r: i32,
    pub tile: u32,
    pub mat: u32,
    pub dir: u32,
    pub _p: [u32; 2],
}

static ENGINE_UPLOAD: std::sync::Mutex<Option<Vec<u32>>> = std::sync::Mutex::new(None);
static ENGINE_OMEGA: std::sync::Mutex<Option<f32>> = std::sync::Mutex::new(None);

/// Hand the GPU a new reciprocating-engine configuration image (engine.rs builds it; empty = no engine). Applied at the
/// start of the next frame, together with the scene layout the caller loads.
pub fn upload_engine(words: Vec<u32>) {
    *ENGINE_UPLOAD.lock().unwrap() = Some(words);
}

/// Change the crank speed (rad per simulation time unit) of the running engine.
pub fn set_engine_omega(omega: f32) {
    *ENGINE_OMEGA.lock().unwrap() = Some(omega);
}

pub struct FrameInput {
    pub steps: u32,
    pub ops: Vec<Op>,
    pub view: u32,
    pub p_fuel: f32,
    pub p_o2: f32,
    pub p_air: f32,
    pub fan_pow: f32,
    /// buoyancy acceleration (cells / time unit^2), 0 = off
    pub gravity: f32,
    /// visible region in cells: x, y, w, h
    pub cam: [f32; 4],
    pub ign_on: bool,
    pub time: f32,
    pub frame: u32,
    /// new layout + full gas reset
    pub scene: Option<Vec<u32>>,
    /// new layout, gas kept (undo/redo)
    pub meta_only: Option<Vec<u32>>,
    /// reset gas and wall temperatures, keep layout
    pub reset_gas: bool,
    pub probe: Option<(i32, i32)>,
}

#[derive(Clone, Copy, Default)]
pub struct ProbeData {
    pub u: [f32; 8],
    pub meta: u32,
    /// temperature, damage, stress ratio
    pub wall: [f32; 4],
}

#[derive(Clone, Copy, Default)]
pub struct Stats {
    pub max_p: f32,
    pub max_t: f32,
    pub max_wall_heat: f32,
    pub burn: f32,
    pub walls: u32,
    pub max_stress: f32,
    pub thrust: [f32; 2],
    /// Time step of the last step (time units; 1 unit = 0.034 ms), its Courant number dt*(max(|u|,|v|)+c)/dx, the largest
    /// wave speed max(|u|,|v|)+c (cells / time unit) and the simulated time since the last scene load (time units).
    pub dt: f32,
    pub cfl: f32,
    pub max_speed: f32,
    pub sim_time: f64,
    /// Conservation accounting, cumulative since the last scene load / gas reset (same units as `conserved_totals`):
    /// mass / energy that entered through inlets, vents and the domain edges ...
    pub bnd_mass: f64,
    pub bnd_energy: f64,
    /// ... and that physical sources added (wall/matter heat exchange and emission, fans, igniters, soot radiation,
    /// burnt-through walls, user edits). Numerical drift = (total − total₀) − Δbnd − Δsrc.
    pub src_mass: f64,
    pub src_energy: f64,
}

/// Heat of combustion per unit fuel mass in sim units (= sim.wgsl `Q_FUEL`; soot carries 2·Q per unit).
pub const Q_FUEL: f64 = 485.0;

/// Conserved totals of a `read_state` snapshot over all gas cells (tiles 0,2,3,4,5,7): (mass, energy) where energy is
/// the total energy E (internal + kinetic) plus the chemical energy of unburnt fuel and soot, so combustion itself
/// does not count as a change. Compare with `Stats::{bnd,src}_*` to get the true numerical drift.
pub fn conserved_totals(state: &[f32], meta: &[u32]) -> (f64, f64) {
    let (mut m, mut e) = (0.0f64, 0.0f64);
    for (i, &mt) in meta.iter().enumerate() {
        if matches!(mt & 0xff, 1 | 6 | 8) {
            continue;
        }
        let c = &state[i * 8..i * 8 + 8];
        m += (c[0] + c[1] + c[2] + c[3]) as f64;
        e += c[6] as f64 + Q_FUEL * c[0] as f64 + 2.0 * Q_FUEL * c[7] as f64;
    }
    (m, e)
}

#[derive(PartialEq)]
enum ProbeState {
    Idle,
    Copied,
    Mapping,
}

/// Tuning aid (bench / debugging): `SIMDEF="T_DISS1=11.0,K_COND=0.4,RECON=1u"` overrides the value of scalar `const`s of the
/// sim/matter/vis shader module without a rebuild. Unknown names are ignored with a warning.
trait PipeOverrides { fn pipe_overrides(self) -> String; }
impl PipeOverrides for String {
    fn pipe_overrides(self) -> String {
        let Ok(spec) = std::env::var("SIMDEF") else { return self };
        let mut src = self;
        for kv in spec.split(',').filter(|k| !k.is_empty()) {
            let Some((k, v)) = kv.split_once('=') else { continue };
            let pat = format!("\nconst {}: ", k.trim());
            if let Some(a) = src.find(&pat) {
                let b = a + pat.len();
                let ty_end = src[b..].find(" = ").map(|e| b + e + 3);
                let semi = src[b..].find(';').map(|e| b + e);
                if let (Some(ve), Some(se)) = (ty_end, semi) { src.replace_range(ve..se, v.trim()); }
            } else {
                eprintln!("SIMDEF: unknown const {k}");
            }
        }
        src
    }
}

pub type PreHook = Box<dyn FnMut(&wgpu::Queue, u32, bool) + Send + Sync>;
pub type StepHook = Box<dyn for<'a, 'b> FnMut(&'a mut wgpu::ComputePass<'b>, &'a wgpu::BindGroup) + Send + Sync>;
pub type PostHook = Box<dyn FnMut(&wgpu::Device, &mut wgpu::CommandEncoder) + Send + Sync>;

pub struct GpuSim {
    pub pre_hook: Option<PreHook>,
    pub step_hook: Option<StepHook>,
    pub post_hook: Option<PostHook>,
    params: wgpu::Buffer,
    ops: wgpu::Buffer,
    meta: wgpu::Buffer,
    u0: wgpu::Buffer,
    cbg: wgpu::BindGroup,
    p_init: wgpu::ComputePipeline,
    p_ops: wgpu::ComputePipeline,
    p_move: [wgpu::ComputePipeline; 2],
    p_act: wgpu::ComputePipeline,
    /// engine_step, engine_piston, engine_valve (engine.wgsl); dispatched each step only while a scene has an engine
    p_eng: [wgpu::ComputePipeline; 3],
    has_engine: bool,
    aux: wgpu::Buffer,
    sparse: bool,
    last_gravity: f32,
    steps: Vec<wgpu::ComputePipeline>, // rk1, rk2 (fused flux+update), post (incl. LES viscosity), wall (tile 1), wall_m (matter), commit (incl. copy-back)
    vis: Vec<wgpu::ComputePipeline>,   // colorize, blurh, blurv
    srgb: bool,
    probe_buf: wgpu::Buffer,
    probe_state: ProbeState,
    probe_flag: Arc<AtomicBool>,
    pub probe: Option<ProbeData>,
    pub stats: Stats,
    step_count: u32,
    wall: wgpu::Buffer,
    stats_buf: wgpu::Buffer,
    p_stats: wgpu::ComputePipeline,
    /// last raw cumulative accounting words (STATS slots 8..12) and the f64 totals unwrapped from them
    acc_raw: [u32; 4],
    acc: [f64; 4],
    /// bumped on every scene load / gas reset; an in-flight readback from an older generation is ignored
    acc_gen: u32,
    probe_gen: u32,
    /// CFL-adaptive time step on/off (default on; `IGNIS_ADT=0` env disables). Off = fixed `DT` like before.
    pub adaptive_dt: bool,
    /// Scene time-scale factor (default 1): physical time simulated per frame = steps * DT * time_scale. Slow scenes
    /// (campfire, flare) are then fast-forwarded with larger/more steps; the app must multiply its clock by it.
    pub time_scale: f32,
    /// mean dt (time units) of the last read-back frame; steps are scaled by DT/dt_avg so a frame keeps its nominal physical time
    dt_avg: f32,
    dt_valid: bool,
    dt_raw: [u32; 2],
    dt_acc: [f64; 2],
    /// closed-loop physical-time control: requested time so far, at the last probe copy, the last measured shortfall, step remainder
    req_total: f64,
    req_at_copy: f64,
    t_err: f64,
    carry: f32,
    /// steps actually encoded in the last `frame()` (<= requested when dt > DT) and their summed time (time units, predicted)
    pub steps_issued: u32,
    // rendering / VFX (vfx.rs: fx.wgsl, particles.wgsl, render.wgsl)
    vfx: Vfx,
}

fn storage(device: &wgpu::Device, label: &str, size: u64) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some(label),
        size,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    })
}

impl GpuSim {
    pub fn buffers(&self) -> (&wgpu::Buffer, &wgpu::Buffer, &wgpu::Buffer) { (&self.u0, &self.meta, &self.wall) }
    pub fn new(device: &wgpu::Device, target: wgpu::TextureFormat) -> Self {
        let n = (W * H) as u64;
        let params = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("params"),
            size: std::mem::size_of::<Params>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let ops = storage(device, "ops", (MAX_OPS * std::mem::size_of::<Op>()) as u64);
        let u0 = storage(device, "u0", n * 32);
        let u1 = storage(device, "u1", n * 32);
        let u2 = storage(device, "u2", n * 32);
        let meta = storage(device, "meta", n * 4);
        let wall = storage(device, "wall", n * 32);
        let stats_buf = storage(device, "stats", 64);
        // AUX (binding 7, the old FLUX slot): active-region tile flags, see sim.wgsl
        let aux = storage(device, "aux", AUX_WORDS * 4);
        let probe_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("probe"),
            size: 128,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let rw = |binding: u32, ro: bool| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Storage { read_only: ro },
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        };
        let uni = |vis: wgpu::ShaderStages| wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: vis,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        };
        let cbgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("compute bgl"),
            entries: &[uni(wgpu::ShaderStages::COMPUTE), rw(1, false), rw(2, false), rw(3, false), rw(4, false), rw(5, false), rw(6, true), rw(7, false), rw(8, false)],
        });
        let cbg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("compute bg"),
            layout: &cbgl,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: params.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: u0.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 2, resource: u1.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 3, resource: u2.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 4, resource: meta.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 5, resource: wall.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 6, resource: ops.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 7, resource: aux.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 8, resource: stats_buf.as_entire_binding() },
            ],
        });
        let sim_desc = wgpu::ShaderModuleDescriptor {
            label: Some("sim"),
            // Perf: read-only lookup tables declared `var<private>` are copied into every thread's private memory at
            // kernel entry by naga/Metal (26 element tables x 66 entries ≈ 8 KB per thread for any kernel that touches
            // matter_update or absorb_matter_emission). As module `const`s they become program-scope `constant` data.
            // None of them is ever written, so the rewrite is exact.
            source: wgpu::ShaderSource::Wgsl(
                format!("{}\n{}\n{}\n{}\n{}", include_str!("sim.wgsl"), include_str!("matter.wgsl"), include_str!("vis.wgsl"), include_str!("matter_look.wgsl"), include_str!("engine.wgsl"))
                    .replace("\nvar<private> ", "\nconst ")
                    .pipe_overrides()
                    .into(),
            ),
        };
        // Perf: naga's runtime bounds checks cost ~8% of the solver on Metal. All solver indexing is clamped
        // (cid, mat_of), so the sim module is compiled unchecked; COMBUSTOR_CHECKED=1 restores the checks for debugging.
        let sim_mod = if std::env::var("COMBUSTOR_CHECKED").is_ok() {
            device.create_shader_module(sim_desc)
        } else {
            unsafe { device.create_shader_module_trusted(sim_desc, wgpu::ShaderRuntimeChecks::unchecked()) }
        };
        let cpl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: None,
            bind_group_layouts: &[&cbgl],
            push_constant_ranges: &[],
        });
        let mk = |entry: &str| {
            device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some(entry),
                layout: Some(&cpl),
                module: &sim_mod,
                entry_point: Some(entry),
                compilation_options: Default::default(),
                cache: None,
            })
        };
        // solver step kernels: they never read workgroup memory before writing it, so skip WebGPU's zero-fill
        // of workgroup memory (on Metal it is done serially by one thread and cost ~25% of the fused RK kernels)
        let mk_step = |entry: &str| {
            device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some(entry),
                layout: Some(&cpl),
                module: &sim_mod,
                entry_point: Some(entry),
                compilation_options: wgpu::PipelineCompilationOptions { zero_initialize_workgroup_memory: false, ..Default::default() },
                cache: None,
            })
        };
        let vfx = Vfx::new(device, target, &params, &u0, &u1, &u2, &meta, &wall);

        GpuSim {
            params,
            ops,
            meta,
            u0,
            cbg,
            p_init: mk("init"),
            p_ops: mk("apply_ops"),
            p_move: [mk("matter_move_a"), mk("matter_move_b")],
            p_act: mk("act"),
            p_eng: [mk("engine_step"), mk("engine_piston"), mk("engine_valve")],
            has_engine: false,
            aux,
            sparse: std::env::var("IGNIS_DENSE").map(|v| v == "0").unwrap_or(true),
            last_gravity: f32::NAN,
            steps: ["rk1", "rk2", "post", "wall", "wall_m", "commit"].iter().map(|e| mk_step(e)).collect(),
            vis: ["colorize", "blurh", "blurv", "bdown", "bwideh", "bwidev"].iter().map(|e| mk(e)).collect(),
            srgb: target.is_srgb(),
            probe_buf,
            probe_state: ProbeState::Idle,
            probe_flag: Arc::new(AtomicBool::new(false)),
            probe: None,
            stats: Stats::default(),
            step_count: 0,
            wall,
            stats_buf,
            p_stats: mk("stats"),
            acc_raw: [0; 4],
            acc: [0.0; 4],
            acc_gen: 0,
            probe_gen: 0,
            adaptive_dt: std::env::var("IGNIS_ADT").map(|v| v != "0").unwrap_or(true),
            time_scale: 1.0,
            dt_avg: DT,
            dt_valid: false,
            dt_raw: [0; 2],
            dt_acc: [0.0; 2],
            req_total: 0.0,
            req_at_copy: 0.0,
            t_err: 0.0,
            carry: 0.0,
            steps_issued: 0,
            vfx,
            pre_hook: None,
            step_hook: None,
            post_hook: None,
        }
    }

    /// Encode this frame's edits, simulation steps and visualisation passes.
    pub fn frame(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, enc: &mut wgpu::CommandEncoder, inp: &FrameInput) {
        // finish any probe readback
        device.poll(wgpu::Maintain::Poll);
        match self.probe_state {
            ProbeState::Mapping if self.probe_flag.load(Ordering::Acquire) => {
                {
                    let data = self.probe_buf.slice(..).get_mapped_range();
                    let f: &[f32] = bytemuck::cast_slice(&data[..]);
                    let m: &[u32] = bytemuck::cast_slice(&data[..]);
                    let mut p = ProbeData::default();
                    p.u.copy_from_slice(&f[0..8]);
                    p.meta = m[8];
                    p.wall.copy_from_slice(&f[12..16]);
                    self.probe = Some(p);
                    let fx = |k: usize| m[16 + k] as i32 as f32 / 1000.0;
                    if self.probe_gen == self.acc_gen {
                        for k in 0..4 {
                            let raw = m[24 + k];
                            self.acc[k] += raw.wrapping_sub(self.acc_raw[k]) as i32 as f64 / 1000.0;
                            self.acc_raw[k] = raw;
                        }
                        // adaptive dt: cumulative time (x1e6, wrapping) and step counter
                        let (dts, dn) = (m[30].wrapping_sub(self.dt_raw[0]), m[31].wrapping_sub(self.dt_raw[1]));
                        self.dt_raw = [m[30], m[31]];
                        self.dt_acc[0] += dts as f64 / 1.0e6;
                        self.dt_acc[1] += dn as f64;
                        if dn > 0 && dn < 100_000 {
                            let a = (dts as f32 / 1.0e6 / dn as f32).clamp(DT, DT_MAX);
                            self.dt_avg = if self.dt_valid { 0.5 * (self.dt_avg + a) } else { a };
                            self.dt_valid = true;
                        }
                        self.t_err = self.req_at_copy - self.dt_acc[0];
                    }
                    let (lam, dt_last) = (f32::from_bits(m[28]), f32::from_bits(m[29]));
                    self.stats = Stats {
                        max_p: f[16],
                        max_t: f[17],
                        max_wall_heat: f[18],
                        burn: fx(3),
                        walls: m[20],
                        max_stress: f[21],
                        thrust: [fx(6), fx(7)],
                        dt: if dt_last > 0.0 { dt_last } else { DT },
                        cfl: if lam.is_finite() { lam * dt_last } else { 0.0 },
                        max_speed: if lam.is_finite() { lam } else { 0.0 },
                        sim_time: self.dt_acc[0],
                        bnd_mass: self.acc[0],
                        bnd_energy: self.acc[1],
                        src_mass: self.acc[2],
                        src_energy: self.acc[3],
                    };
                }
                self.probe_buf.unmap();
                self.probe_state = ProbeState::Idle;
            }
            ProbeState::Copied => {
                let flag = self.probe_flag.clone();
                flag.store(false, Ordering::Release);
                self.probe_buf.slice(..).map_async(wgpu::MapMode::Read, move |_| flag.store(true, Ordering::Release));
                self.probe_state = ProbeState::Mapping;
            }
            _ => {}
        }

        let nops = inp.ops.len().min(MAX_OPS);
        let params = Params {
            w: W,
            h: H,
            nops: nops as u32,
            view: inp.view,
            dt: DT,
            p_fuel: inp.p_fuel,
            fan_pow: inp.fan_pow,
            time: inp.time,
            ign_on: inp.ign_on as u32,
            frame: inp.frame,
            srgb: self.srgb as u32,
            sflags: self.sparse as u32,
            p_o2: inp.p_o2,
            p_air: inp.p_air,
            cam: inp.cam,
            _p2: [DT * WALL_EVERY as f32, inp.gravity],
            dt_min: DT,
            dt_max: if self.adaptive_dt && self.dt_valid { DT_MAX } else { DT },
            cfl: CFL_TARGET,
            tscale: self.time_scale,
        };
        queue.write_buffer(&self.params, 0, bytemuck::bytes_of(&params));
        if let Some(words) = ENGINE_UPLOAD.lock().unwrap().take() {
            // an empty image removes the engine; otherwise it replaces the whole config block (crank angle restarts at its value)
            self.has_engine = !words.is_empty();
            if self.has_engine {
                queue.write_buffer(&self.aux, ENG_WORD * 4, bytemuck::cast_slice(&words));
            }
        }
        if let Some(w) = ENGINE_OMEGA.lock().unwrap().take() {
            if self.has_engine {
                queue.write_buffer(&self.aux, ENG_WORD * 4, bytemuck::bytes_of(&w));
            }
        }
        if nops > 0 {
            queue.write_buffer(&self.ops, 0, bytemuck::cast_slice(&inp.ops[..nops]));
        }
        if let Some(meta) = inp.scene.as_ref().or(inp.meta_only.as_ref()) {
            queue.write_buffer(&self.meta, 0, bytemuck::cast_slice(meta));
        }
        let (gx, gy) = (W.div_ceil(16), H.div_ceil(16));
        // Steps to encode: the frame's nominal physical time is steps * DT * time_scale; with a larger adaptive dt fewer
        // steps cover the same time (calm scenes get cheaper), never more than the nominal count.
        let want = inp.steps as f32 * self.time_scale.max(0.0);
        let n_steps = if inp.steps == 0 {
            0
        } else if self.adaptive_dt && self.dt_valid {
            let budget = want * DT;
            self.req_total += budget as f64;
            // closed loop on the simulated time the GPU really reports (3 frames late): add a fraction of the shortfall
            let t = budget + (0.3 * self.t_err as f32).clamp(-0.5 * budget, 0.5 * budget);
            let nf = t / self.dt_avg + self.carry;
            let n = (nf.floor().max(1.0) as u32).min((want.ceil() as u32).max(1));
            self.carry = (nf - n as f32).clamp(-1.0, 1.0);
            n
        } else {
            self.req_total += (want * DT) as f64;
            (want.round() as u32).max(1)
        };
        self.steps_issued = n_steps;
        if let Some(h) = self.pre_hook.as_mut() { h(queue, inp.steps, inp.scene.is_some() || inp.reset_gas); }
        // anything that edits the state outside the solver invalidates the calm flags: step every tile once
        if inp.scene.is_some() || inp.meta_only.is_some() || inp.reset_gas || nops > 0 || inp.gravity != self.last_gravity {
            enc.clear_buffer(&self.aux, 0, Some(AUX_CALM_BYTES));
            self.last_gravity = inp.gravity;
        }
        if inp.scene.is_some() || inp.reset_gas {
            // restart the conservation accounting with the new gas state
            enc.clear_buffer(&self.stats_buf, 0, None);
            self.acc_raw = [0; 4];
            self.acc = [0.0; 4];
            self.dt_raw = [0; 2];
            self.dt_acc = [0.0; 2];
            self.dt_valid = false;
            self.dt_avg = DT;
            self.req_total = 0.0;
            self.req_at_copy = 0.0;
            self.t_err = 0.0;
            self.carry = 0.0;
            self.acc_gen = self.acc_gen.wrapping_add(1);
        }
        {
            let mut pass = enc.begin_compute_pass(&wgpu::ComputePassDescriptor { label: Some("sim"), timestamp_writes: None });
            pass.set_bind_group(0, &self.cbg, &[]);
            if inp.scene.is_some() || inp.reset_gas {
                pass.set_pipeline(&self.p_init);
                pass.dispatch_workgroups(gx, gy, 1);
            }
            if nops > 0 {
                pass.set_pipeline(&self.p_ops);
                pass.dispatch_workgroups(gx, gy, 1);
            }
            for _ in 0..n_steps {
                self.step_count += 1;
                let walls_now = self.step_count % WALL_EVERY == 0;
                // active-tile mask for this step (from the calm flags `post` wrote last step)
                pass.set_pipeline(&self.p_act);
                pass.dispatch_workgroups(N_TILES.div_ceil(64), 1, 1);
                if self.has_engine {
                    pass.set_pipeline(&self.p_eng[0]);
                    pass.dispatch_workgroups(1, 1, 1);
                    pass.set_pipeline(&self.p_eng[1]);
                    pass.dispatch_workgroups(8, 1, 1);
                    pass.set_pipeline(&self.p_eng[2]);
                    pass.dispatch_workgroups(8, 1, 1);
                }
                for (k, p) in self.steps.iter().enumerate() {
                    if k >= 3 && !walls_now {
                        break;
                    }
                    pass.set_pipeline(p);
                    // all step kernels run on the RK tile grid (wall/wall_m/commit skip tiles without solids)
                    pass.dispatch_workgroups(W.div_ceil(RK_TX), H.div_ceil(RK_TY), 1);
                }
                // powders and liquids move every 4th step, alternating the Margolus block offset
                if walls_now {
                    let mv = if self.step_count % (2 * WALL_EVERY) == 0 { &self.p_move[0] } else { &self.p_move[1] };
                    pass.set_pipeline(mv);
                    pass.dispatch_workgroups((W / 2 + 1).div_ceil(16), (H / 2 + 1).div_ceil(16), 1);
                }
                if let Some(h) = self.step_hook.as_mut() { h(&mut pass, &self.cbg); }
            }
        }
        // per-frame slots 0..8 only; 8..12 are the cumulative conservation accounting
        enc.clear_buffer(&self.stats_buf, 0, Some(32));
        {
            let mut pass = enc.begin_compute_pass(&wgpu::ComputePassDescriptor { label: Some("vis"), timestamp_writes: None });
            pass.set_bind_group(0, &self.cbg, &[]);
            pass.set_pipeline(&self.p_stats);
            pass.dispatch_workgroups(gx, gy, 1);
            for p in &self.vis {
                pass.set_pipeline(p);
                pass.dispatch_workgroups(gx, gy, 1);
            }
        }
        if let Some(h) = self.post_hook.as_mut() { h(device, enc); }
        // rendering / VFX passes (vfx.rs)
        self.vfx.frame(queue, enc, (inp.steps as f32 * self.time_scale.max(0.0)).round() as u32, inp.scene.is_some() || inp.reset_gas);
        if self.probe_state == ProbeState::Idle {
            if let Some((x, y)) = inp.probe {
                if x >= 0 && y >= 0 && (x as u32) < W && (y as u32) < H {
                    let i = (y as u32 * W + x as u32) as u64;
                    enc.copy_buffer_to_buffer(&self.u0, i * 32, &self.probe_buf, 0, 32);
                    enc.copy_buffer_to_buffer(&self.meta, i * 4, &self.probe_buf, 32, 4);
                    enc.copy_buffer_to_buffer(&self.wall, i * 16, &self.probe_buf, 48, 16);
                    enc.copy_buffer_to_buffer(&self.stats_buf, 0, &self.probe_buf, 64, 64);
                    self.probe_gen = self.acc_gen;
                    self.req_at_copy = self.req_total;
                    self.probe_state = ProbeState::Copied;
                }
            }
        }
    }

    pub fn draw(&self, rp: &mut wgpu::RenderPass<'_>) {
        self.vfx.draw(rp);
    }

    /// Blocking readback of the tile layout (for undo snapshots and saving).
    /// Blocking readback of the cumulative conservation accounting [bnd mass, bnd energy, src mass, src energy]
    /// (exactly in step with a `read_state` done right after; also refreshes `stats`' accounting fields).
    pub fn read_accounting(&mut self, device: &wgpu::Device, queue: &wgpu::Queue) -> [f64; 4] {
        let dst = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: 16,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let mut e = device.create_command_encoder(&Default::default());
        e.copy_buffer_to_buffer(&self.stats_buf, 32, &dst, 0, 16);
        queue.submit([e.finish()]);
        dst.slice(..).map_async(wgpu::MapMode::Read, |_| {});
        device.poll(wgpu::Maintain::Wait);
        let raw: Vec<u32> = bytemuck::cast_slice(&dst.slice(..).get_mapped_range()).to_vec();
        for k in 0..4 {
            self.acc[k] += raw[k].wrapping_sub(self.acc_raw[k]) as i32 as f64 / 1000.0;
            self.acc_raw[k] = raw[k];
        }
        self.stats.bnd_mass = self.acc[0];
        self.stats.bnd_energy = self.acc[1];
        self.stats.src_mass = self.acc[2];
        self.stats.src_energy = self.acc[3];
        self.acc
    }

    pub fn read_meta(&self, device: &wgpu::Device, queue: &wgpu::Queue) -> Vec<u32> {
        let size = (W * H * 4) as u64;
        let dst = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let mut e = device.create_command_encoder(&Default::default());
        e.copy_buffer_to_buffer(&self.meta, 0, &dst, 0, size);
        queue.submit([e.finish()]);
        dst.slice(..).map_async(wgpu::MapMode::Read, |_| {});
        device.poll(wgpu::Maintain::Wait);
        let v = bytemuck::cast_slice(&dst.slice(..).get_mapped_range()).to_vec();
        v
    }

    /// Blocking readback of wall/matter state (temperature, flag, code, remaining) for headless tests.
    pub fn read_wall(&self, device: &wgpu::Device, queue: &wgpu::Queue) -> Vec<f32> {
        let size = (W * H * 16) as u64;
        let dst = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let mut e = device.create_command_encoder(&Default::default());
        e.copy_buffer_to_buffer(&self.wall, 0, &dst, 0, size);
        queue.submit([e.finish()]);
        dst.slice(..).map_async(wgpu::MapMode::Read, |_| {});
        device.poll(wgpu::Maintain::Wait);
        let v = bytemuck::cast_slice(&dst.slice(..).get_mapped_range()).to_vec();
        v
    }

    /// Synchronous readback of the gas state, for headless testing.
    pub fn read_state(&self, device: &wgpu::Device, queue: &wgpu::Queue) -> (Vec<f32>, Vec<u32>) {
        let n = (W * H) as u64;
        let read = |src: &wgpu::Buffer, size: u64| -> Vec<u8> {
            let dst = device.create_buffer(&wgpu::BufferDescriptor {
                label: None,
                size,
                usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            let mut e = device.create_command_encoder(&Default::default());
            e.copy_buffer_to_buffer(src, 0, &dst, 0, size);
            queue.submit([e.finish()]);
            dst.slice(..).map_async(wgpu::MapMode::Read, |_| {});
            device.poll(wgpu::Maintain::Wait);
            let v = dst.slice(..).get_mapped_range().to_vec();
            v
        };
        let s = read(&self.u0, n * 32);
        let m = read(&self.meta, n * 4);
        (bytemuck::cast_slice(&s).to_vec(), bytemuck::cast_slice(&m).to_vec())
    }
}
