//! Ignis physical acoustics: the sound you hear is computed from the solver's own fields.
//!
//! Pipeline (one frame):
//!   GPU  `acoustics.wgsl`  mic_sample (every sim step) -> MIC ring: gas pressure at the listener's left ear,
//!                          centre, right ear + gas speed at the listener (the solver's real acoustic field:
//!                          blast waves arrive with their physical propagation delay and reflect off walls).
//!                          features (every frame) -> PART: per band (8 columns across the view) heat release
//!                          and its moments, Proudman turbulence noise, Lighthill jet terms (U, width, length),
//!                          shocks, burning wood / propellant / matter, boiling, gas-releasing reactions, wall
//!                          stress / imminent failures (+ modal frequency of the material), fans, inlet flux.
//!   GPU->CPU               asynchronous readback through a ring of 4 MAP_READ buffers (never stalls; ~2 frames late).
//!   main thread `Analyzer` converts to SI (1 cell = 1 cm, 1 tu = 34 us, p unit = 1 atm, v unit = 293 m/s) and
//!                          evaluates the textbook source models (below) -> `Ctl` (sound pressures in Pa at the
//!                          listener, spectral peaks in Hz, pan, distance, event rates, shock events).
//!   audio thread `Synth`   renders them (filtered-noise spectra, Poisson grain processes, modal resonators),
//!                          pans, applies distance/air absorption, a loudness mapping, soft limiter and master volume.
//!
//! Source models (all SI):
//!   * combustion noise (Strahle 1971, monopole):  p(r) = (gamma-1)/(4 pi c0^2 r) * dQdot/dt.  dQdot/dt has a
//!     resolved part (frame-to-frame change of the solver's heat release, per physical second) and a sub-grid
//!     part 2 pi f_pk I Qdot (turbulent heat-release fluctuation intensity I ~ 0.15 / sqrt(coherent patches)).
//!     Spectrum: rises ~f^2 below / falls ~f^-2..-3 above f_pk (Rajaram & Lieuwen 2009), f_pk ~ 180 Hz / sqrt(L_m),
//!     brighter for hotter flames.
//!   * jet / rocket noise: Lighthill W = K rho0 U^8 A / c0^5 (K = 3e-5) for subsonic jets, capped by the rocket
//!     acoustic efficiency W = 0.5 % of the jet's mechanical power (Eldred, NASA SP-8072) once supersonic.
//!     U = energy-flux weighted jet speed, D = jet width and L = length from the second moments of the jet cells.
//!     Tam's two-source spectrum: fine-scale part peaking at St = f D / U = 0.35, large-scale part at St = 0.15,
//!     plus a low "rumble" from the whole plume (f = 0.2 U / L). Supersonic jets: crackle (Ffowcs Williams 1975)
//!     as a Poisson train of shocklets whose rate follows the shock cells found inside the jet.
//!   * turbulence noise: Proudman (1952) P = 38 rho eps Mt^5 from the LES sub-grid turbulence (Smagorinsky).
//!   * wood / propellant / matter crackle: Poisson pop processes, rate ~ burning surface cells, pop amplitudes
//!     power-law distributed (P(A) ~ A^-2, crackling noise), each pop a 0.3-4 ms band-limited burst (+ a low
//!     "thock" for the big ones). Boiling / fizzing: Minnaert bubble resonances f = 3.26 Hz m / R.
//!   * structures: 4-mode resonators (free-bar ratios 1 : 2.76 : 5.40 : 8.93) with the material's frequency and Q,
//!     excited by stick-slip impulse trains whose rate follows the stress change rate (creak / groan), and by
//!     fractures (walls lost that were near failure last frame: crack + ring + debris pings).
//!   * fans: blade-pass tone B U_tip / (pi D) with harmonics + dipole broadband (W ~ rho0 U^6 D^2 / c0^3).
//!   * gas inlets: hiss = Lighthill noise of the small inlet jets.
//!   * microphone: the solver's pressure at the ears drives the "heard pressure" layer (shock fronts become a
//!     crack + thump + rumble tail, reflections and room reverberation come out of the solver itself), the gas
//!     speed at the ear gives wind buffeting, and the direct waveform feeds an infrasonic LFE channel.
//!
//! Time mapping (slow motion). With c_sim = 1.18 cells/tu standing for 347 m/s, 1 tu = 34 us of physical time and
//! the app runs 32 tu per real second: the picture is ~920x slow motion (`phys::slow_motion(speed)`). Played in
//! physical time the solver's waveform at the ear is sub-millisecond; played in game time it is infrasonic. Ignis
//! therefore renders a *transient-preserving time stretch* of the physical sound field, like slow-motion film
//! audio: envelopes, arrival times, echo spacing and event order follow game time (in sync with the picture),
//! while spectra, pressures and individual grains (pops, cracks, bubble pings, shock fronts) keep their physical
//! frequencies and durations. Stationary sources therefore sound as they would in real time, booms keep their
//! relative timing and shape (Friedlander-like envelope taken from the microphone), and sim speed only changes
//! the tempo of events.
//!
//! Two clocks. The solver's gas dynamics run at physical speed (c, shocks, pressure-driven jets), but its
//! chemistry and buoyancy are tuned to the game clock (a log pile that burns for a game minute is a real
//! campfire; gravity is ~S^2 g). Taken literally the 2D campfire would release 38 MW. Therefore:
//!   * heat release and its derivative are evaluated per game second (Qdot / S): a 15-50 kW campfire;
//!   * jet / Proudman / wind terms only count pressure-driven flow (|u| > 440 m/s and local Mach > 0.45 for jets,
//!     > 220 m/s for Proudman turbulence, > 120 m/s for wind at the ear); the slower flame- and buoyancy-driven
//!     flow speeds are inflated by the game clock and their noise IS the combustion monopole;
//!   * the microphone's sustained pressure "sloshing" (driven by the same game-clock heat release) is removed by
//!     a transient extractor (adaptive floor + a gate opened by detected shocks for 1.5 game seconds), so the
//!     solver's real blasts, their reflections and reverberation come through while flames stay model-driven.
//!
//! Loudness: every layer is computed as a physical sound pressure (Pa) at the listener (spherical spreading from
//! the source centroid, near-field softened by source size, ISO-9613-like air absorption low-pass). Real levels
//! span 40-200 dB SPL, so the output uses a documented compressive mapping (`loud`: 0.22 dB of output per dB SPL
//! up to 130 dB, 0.10 above; 75 dB flare ~ -31 dBFS, 160 dB rocket ~ -16 dBFS rms; transients above the
//! continuous sound 0.25 dB/dB on top) applied as ONE gain to the total sound, so simultaneous sources keep their
//! physical balance and spectra; a 1 ms lookahead limiter and a transparent clipper protect the output.
//!
//! The 2D slice is interpreted acoustically as a source as deep as it is wide (flames: depth = flame size;
//! jets: round nozzle of the slot width) instead of the 1 m depth used by the thrust readouts, otherwise every
//! flame would be a 1 m wide wall of fire.
//!
//! GPU cost (M2 Pro): features ~0.16 ms per frame (gas cells checkerboard-sampled, alternating per frame),
//! microphone ~3-5 us per sample, one sample every `mic_every` = 2 steps (~0.05 ms per frame at 1x).
#![allow(dead_code)]

use bytemuck::{Pod, Zeroable};
use eframe::wgpu;
use std::collections::VecDeque;
use std::f32::consts::PI;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// Spatial bands across the view (stereo resolution of the synthesis layers).
pub const NB: usize = 8;
/// Row slabs per band in the feature reduction (= workgroups per band).
pub const NS: usize = 8;
/// Feature slots per band (must match `NF` in acoustics.wgsl).
pub const NF: usize = 48;
/// Max microphone samples per frame (>= MAX_STEPS of the app).
pub const MIC_CAP: usize = 128;
const MIC_BYTES: u64 = 16 * (MIC_CAP as u64 + 1);
const PART_BYTES: u64 = (NS * NB * NF * 4) as u64;
const N_STAGING: usize = 4;

/// Feature slots (same as the `F_*` constants in acoustics.wgsl).
pub mod f {
    pub const Q: usize = 0;
    pub const QX: usize = 1;
    pub const QY: usize = 2;
    pub const QXX: usize = 3;
    pub const QYY: usize = 4;
    pub const FLN: usize = 5;
    pub const FLT: usize = 6;
    pub const PR: usize = 7;
    pub const PRX: usize = 8;
    pub const PRY: usize = 9;
    pub const PRU: usize = 10;
    pub const JN: usize = 11;
    pub const JKE: usize = 12;
    pub const JU4: usize = 13;
    pub const JRHO: usize = 14;
    pub const JX: usize = 15;
    pub const JY: usize = 16;
    pub const JXX: usize = 17;
    pub const JYY: usize = 18;
    pub const JXY: usize = 19;
    pub const UMAX: usize = 20;
    pub const VORT: usize = 21;
    pub const SHN: usize = 22;
    pub const SHS: usize = 23;
    pub const SHJ: usize = 24;
    pub const PMAX: usize = 25;
    pub const WOODN: usize = 26;
    pub const WOODR: usize = 27;
    pub const PROPN: usize = 28;
    pub const MBN: usize = 29;
    pub const MBR: usize = 30;
    pub const BOILN: usize = 31;
    pub const BOILR: usize = 32;
    pub const FIZN: usize = 33;
    pub const FIZR: usize = 34;
    pub const DETN: usize = 35;
    pub const WALLS: usize = 36;
    pub const SW: usize = 37;
    pub const SWF: usize = 38;
    pub const SWQ: usize = 39;
    pub const SMAX: usize = 40;
    pub const FAIL: usize = 41;
    pub const FAILF: usize = 42;
    pub const FANN: usize = 43;
    pub const FANU: usize = 44;
    pub const INN: usize = 45;
    pub const INF: usize = 46;
    pub const INR: usize = 47;
    pub const IS_MAX: [usize; 3] = [UMAX, PMAX, SMAX];
}

/// Physical scale of the solver (same as app.rs readouts: MS_PER_UNIT, MPS_PER_UNIT).
pub mod phys {
    /// physical seconds per sim time unit (1 cm / 347 m/s * 1.183 cells/tu)
    pub const T_UNIT_S: f32 = 3.4e-5;
    pub const DX_M: f32 = 0.01;
    pub const P_UNIT_PA: f32 = 101_325.0;
    pub const RHO_UNIT: f32 = 1.177;
    pub const V_UNIT: f32 = 293.0;
    pub const C0: f32 = 347.0;
    pub const RHO0: f32 = 1.177;
    /// characteristic impedance of air (Pa s / m)
    pub const Z0: f32 = RHO0 * C0;
    pub const DT: f32 = 0.028;
    /// sim time units per real second at 1x (app.rs TPS)
    pub const TPS: f32 = 32.0;
    /// Watts per (energy unit / tu) in one cell of 1 m depth
    pub const W_PER_Q: f32 = P_UNIT_PA * DX_M * DX_M / T_UNIT_S;
    /// game-time / physical-time ratio (~920 at 1x)
    pub fn slow_motion(speed: f32) -> f32 {
        1.0 / (TPS * speed.max(1e-3) * T_UNIT_S)
    }
}

// ================================================================================================ GPU side

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct AcParams {
    w: u32,
    h: u32,
    nb: u32,
    ns: u32,
    lx: f32,
    ly: f32,
    ear: f32,
    dt: f32,
    cam_x: f32,
    cam_w: f32,
    burn_thr: f32,
    jet_u: f32,
    mic_cap: u32,
    jet_m: f32,
    frame: u32,
    _p: u32,
}

/// What the app knows about this frame (passed to `GpuAcoustics::prepare`).
#[derive(Clone, Copy, Debug)]
pub struct FrameInfo {
    /// sim steps encoded this frame (0 when paused)
    pub steps: u32,
    /// sim time (tu) at the end of the frame
    pub sim_time: f32,
    /// real seconds this frame represents (frame interval)
    pub real_dt: f32,
    /// sim speed multiplier (1 = normal)
    pub speed: f32,
    /// listener position in cells (camera centre or mouse)
    pub listener: [f32; 2],
    /// visible region x, y, w, h in cells (the bands span it)
    pub cam: [f32; 4],
    pub fan_pow: f32,
    pub paused: bool,
    /// true on the frame a scene was loaded / reset (clears the analyser's history)
    pub reset: bool,
}

impl Default for FrameInfo {
    fn default() -> Self {
        FrameInfo { steps: 0, sim_time: 0.0, real_dt: 1.0 / 60.0, speed: 1.0, listener: [256.0, 144.0], cam: [0.0, 0.0, 512.0, 288.0], fan_pow: 1.0, paused: false, reset: false }
    }
}

/// One frame of read-back acoustic data.
#[derive(Clone)]
pub struct FeatureFrame {
    pub info: FrameInfo,
    /// per sim step: pressure (atm) at left ear, centre, right ear; gas speed at the listener (cells/tu)
    pub mic: Vec<[f32; 4]>,
    /// raw partial sums [(slab * NB + band) * NF + f]
    pub part: Vec<f32>,
}

impl FeatureFrame {
    /// Per-band features (slabs summed, max slots max-reduced).
    pub fn bands(&self) -> [[f32; NF]; NB] {
        let mut out = [[0.0f32; NF]; NB];
        for s in 0..NS {
            for b in 0..NB {
                let base = (s * NB + b) * NF;
                for k in 0..NF {
                    let v = self.part.get(base + k).copied().unwrap_or(0.0);
                    let v = if v.is_finite() { v } else { 0.0 };
                    if f::IS_MAX.contains(&k) {
                        out[b][k] = out[b][k].max(v);
                    } else {
                        out[b][k] += v;
                    }
                }
            }
        }
        out
    }
}

#[derive(PartialEq, Clone, Copy)]
enum StState {
    Free,
    Copied,
    Mapping,
}

struct Staging {
    buf: wgpu::Buffer,
    state: StState,
    flag: Arc<AtomicBool>,
    info: FrameInfo,
}

/// Physics constants shared with the solver: extracted from sim.wgsl so the acoustic source terms always match it.
const SHARED_CONSTS: [(&str, &str); 19] = [
    ("T_CHEM_MIN", "const T_CHEM_MIN: f32 = 2.7;"),
    ("T_DISS0", "const T_DISS0: f32 = 8.0;"),
    ("T_DISS1", "const T_DISS1: f32 = 12.0;"),
    ("RHO_MIN", "const RHO_MIN: f32 = 0.002;"),
    ("P_MIN", "const P_MIN: f32 = 0.002;"),
    ("Q_FUEL", "const Q_FUEL: f32 = 485.0;"),
    ("T_ACT", "const T_ACT: f32 = 60.0;"),
    ("A_PRE", "const A_PRE: f32 = 282000.0;"),
    ("D_SPEC", "const D_SPEC: f32 = 0.3;"),
    ("C_EDC", "const C_EDC: f32 = 4.0;"),
    ("T_EDC", "const T_EDC: f32 = 4.0;"),
    ("CS2", "const CS2: f32 = 0.06;"),
    ("NU_T_MAX", "const NU_T_MAX: f32 = 1.5;"),
    ("K_WOOD", "const K_WOOD: f32 = 8.0;"),
    ("T_WOOD", "const T_WOOD: f32 = 12.0;"),
    ("T_PROP_IGN", "const T_PROP_IGN: f32 = 2.5;"),
    ("PR_T", "const PR_T: f32 = 0.7;"),
    ("RS", "const RS = vec4<f32>(1.80625, 0.903125, 1.032142857, 1.111538462);"),
    ("CV", "const CV = vec4<f32>(5.827, 2.258, 2.580, 4.446);"),
];

/// acoustics.wgsl with the solver constants prepended (taken from sim.wgsl when present there).
pub fn shader_source() -> String {
    let sim = include_str!("sim.wgsl");
    let mut head = String::from("// ---- constants extracted from sim.wgsl by acoustics.rs\n");
    for (name, fallback) in SHARED_CONSTS {
        let mut found = None;
        for line in sim.lines() {
            let l = line.trim_start();
            if let Some(rest) = l.strip_prefix("const ") {
                let id: String = rest.chars().take_while(|c| c.is_alphanumeric() || *c == '_').collect();
                if id == name {
                    if let Some(end) = l.find(';') {
                        found = Some(l[..=end].to_string());
                    }
                    break;
                }
            }
        }
        head.push_str(found.as_deref().unwrap_or(fallback));
        head.push('\n');
    }
    head.push_str(&include_str!("acoustics.wgsl").replace("//@REDUCE@", &reduce_code()));
    head
}

/// Unrolled reduction of the private accumulators (constant indices only, so they stay in registers):
/// 6 rounds of 8 features; 128 threads each fold 8 partials of one feature, then 8 threads fold 16.
fn reduce_code() -> String {
    let mut c = String::new();
    for r in 0..NF / 8 {
        for j in 0..8 {
            c.push_str(&format!("    RED[{}u + li] = ACC[{}];\n", j * 128, r * 8 + j));
        }
        c.push_str(&format!(
            "    workgroupBarrier();\n    {{\n        let j = li >> 4u;\n        let base = j * 128u + (li & 15u) * 8u;\n        let mx = is_max_slot({r}u * 8u + j);\n        var a = RED[base];\n        for (var q = 1u; q < 8u; q++) {{ let o = RED[base + q]; a = select(a + o, max(a, o), mx); }}\n        RED2[li] = a;\n    }}\n    workgroupBarrier();\n    if (li < 8u) {{\n        let mx = is_max_slot({r}u * 8u + li);\n        var a = RED2[li * 16u];\n        for (var q = 1u; q < 16u; q++) {{ let o = RED2[li * 16u + q]; a = select(a + o, max(a, o), mx); }}\n        PART[out + {r}u * 8u + li] = a;\n    }}\n    workgroupBarrier();\n"
        ));
    }
    c
}

/// GPU half of the acoustics system: owns its pipelines/buffers and reads the solver's U0/META/WALL.
pub struct GpuAcoustics {
    params: wgpu::Buffer,
    mic: wgpu::Buffer,
    part: wgpu::Buffer,
    bg: wgpu::BindGroup,
    p_mic: wgpu::ComputePipeline,
    p_feat: wgpu::ComputePipeline,
    staging: Vec<Staging>,
    info: FrameInfo,
    w: u32,
    h: u32,
    /// half distance between the ears in cells
    pub ear: f32,
    /// flame-cell threshold on q = Q_FUEL * burn_rate
    pub burn_thr: f32,
    /// jet threshold on |u| (cells/tu; 1.5 = 440 m/s: pressure-driven engine / rocket jets; slower flows near flames
    /// are inflated by the game-clock chemistry and are covered by the combustion and Proudman terms)
    pub jet_u: f32,
    /// jet threshold on the local Mach number: momentum-driven jets, not the thermal-expansion flow of flames
    /// (that is combustion noise, already counted by the monopole term)
    pub jet_m: f32,
    ready: VecDeque<FeatureFrame>,
    /// microphone sample period in sim steps
    pub mic_every: u32,
    step_ctr: u32,
    frame: u32,
    /// frames whose data could not be copied because every staging buffer was busy
    pub dropped: u32,
}

impl GpuAcoustics {
    /// `u0`, `meta`, `wall`: the solver's buffers (bound read-only here). `w`, `h`: grid size.
    pub fn new(device: &wgpu::Device, u0: &wgpu::Buffer, meta: &wgpu::Buffer, wall: &wgpu::Buffer, w: u32, h: u32) -> Self {
        let params = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("ac params"),
            size: std::mem::size_of::<AcParams>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let sto = |label: &str, size: u64| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size,
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::COPY_SRC,
                mapped_at_creation: false,
            })
        };
        let mic = sto("ac mic", MIC_BYTES);
        let part = sto("ac part", PART_BYTES);
        let e = |binding: u32, ty: wgpu::BufferBindingType| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Buffer { ty, has_dynamic_offset: false, min_binding_size: None },
            count: None,
        };
        let ro = wgpu::BufferBindingType::Storage { read_only: true };
        let rw = wgpu::BufferBindingType::Storage { read_only: false };
        let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("acoustics bgl"),
            entries: &[e(0, wgpu::BufferBindingType::Uniform), e(1, ro), e(2, ro), e(3, ro), e(4, rw), e(5, rw)],
        });
        let bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("acoustics bg"),
            layout: &bgl,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: params.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: u0.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 2, resource: meta.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 3, resource: wall.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 4, resource: mic.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 5, resource: part.as_entire_binding() },
            ],
        });
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("acoustics"), source: wgpu::ShaderSource::Wgsl(shader_source().into()) });
        let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some("acoustics"), bind_group_layouts: &[&bgl], push_constant_ranges: &[] });
        let mk = |entry: &str| {
            device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some(entry),
                layout: Some(&pl),
                module: &module,
                entry_point: Some(entry),
                // every workgroup-memory slot is written before it is read: skip the (serial, on Metal) zero-fill
                compilation_options: wgpu::PipelineCompilationOptions { zero_initialize_workgroup_memory: false, ..Default::default() },
                cache: None,
            })
        };
        let staging = (0..N_STAGING)
            .map(|k| Staging {
                buf: device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some(&format!("ac staging {k}")),
                    size: MIC_BYTES + PART_BYTES,
                    usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                }),
                state: StState::Free,
                flag: Arc::new(AtomicBool::new(false)),
                info: FrameInfo::default(),
            })
            .collect();
        GpuAcoustics {
            params,
            mic,
            part,
            bg,
            p_mic: mk("mic_sample"),
            p_feat: mk("features"),
            staging,
            info: FrameInfo::default(),
            w,
            h,
            ear: 2.0,
            burn_thr: 0.5,
            jet_u: 1.5,
            jet_m: 0.45,
            ready: VecDeque::new(),
            mic_every: 2,
            step_ctr: 0,
            frame: 0,
            dropped: 0,
        }
    }

    /// Call once per frame BEFORE the sim compute pass is encoded (writes the uniform used by both kernels).
    pub fn prepare(&mut self, queue: &wgpu::Queue, info: &FrameInfo) {
        self.info = *info;
        self.frame = self.frame.wrapping_add(1);
        let cw = info.cam[2].max(1.0);
        let p = AcParams {
            w: self.w,
            h: self.h,
            nb: NB as u32,
            ns: NS as u32,
            lx: info.listener[0].clamp(0.5, self.w as f32 - 0.5),
            ly: info.listener[1].clamp(0.5, self.h as f32 - 0.5),
            ear: self.ear,
            dt: phys::DT,
            cam_x: info.cam[0],
            cam_w: cw,
            burn_thr: self.burn_thr,
            jet_u: self.jet_u,
            mic_cap: MIC_CAP as u32,
            jet_m: self.jet_m,
            frame: self.frame,
            _p: 0,
        };
        queue.write_buffer(&self.params, 0, bytemuck::bytes_of(&p));
    }

    /// Microphone sample: call inside the sim compute pass after each complete step (after `post` / the wall
    /// pipelines of that step). Changes bind group 0, so it re-binds `restore` (the sim bind group) afterwards.
    /// Samples every `mic_every`-th step (default 2: ~570 samples per real second at 1x, plenty for the
    /// envelopes and shock arrivals; each sample is a tiny dispatch with a pass barrier, ~5-10 us).
    pub fn encode_mic(&mut self, pass: &mut wgpu::ComputePass<'_>, restore: &wgpu::BindGroup) {
        self.step_ctr = self.step_ctr.wrapping_add(1);
        if self.step_ctr % self.mic_every.max(1) != 0 {
            return;
        }
        pass.set_pipeline(&self.p_mic);
        pass.set_bind_group(0, &self.bg, &[]);
        pass.dispatch_workgroups(1, 1, 1);
        pass.set_bind_group(0, restore, &[]);
    }

    /// Once per frame after the sim pass: harvests finished readbacks, encodes the feature reduction, copies
    /// this frame's mic + features into a free staging buffer and clears the mic counter. Never blocks.
    /// Must be called exactly once per submitted command buffer (it maps the buffers copied by the previous call,
    /// which therefore must have been submitted already) - true for GpuSim::frame inside the egui callback.
    pub fn encode_frame(&mut self, device: &wgpu::Device, enc: &mut wgpu::CommandEncoder) {
        self.harvest(device);
        self.encode_features(enc);
        if let Some(s) = self.staging.iter_mut().find(|s| s.state == StState::Free) {
            enc.copy_buffer_to_buffer(&self.mic, 0, &s.buf, 0, MIC_BYTES);
            enc.copy_buffer_to_buffer(&self.part, 0, &s.buf, MIC_BYTES, PART_BYTES);
            s.state = StState::Copied;
            s.info = self.info;
        } else {
            self.dropped += 1;
        }
        enc.clear_buffer(&self.mic, 0, Some(16));
    }

    /// Just the feature reduction pass (no readback); `encode_frame` calls it.
    pub fn encode_features(&self, enc: &mut wgpu::CommandEncoder) {
        let mut pass = enc.begin_compute_pass(&wgpu::ComputePassDescriptor { label: Some("acoustics"), timestamp_writes: None });
        self.encode_features_in(&mut pass);
    }

    /// The feature reduction dispatch inside an existing compute pass (leaves bind group 0 = acoustics).
    pub fn encode_features_in(&self, pass: &mut wgpu::ComputePass<'_>) {
        pass.set_pipeline(&self.p_feat);
        pass.set_bind_group(0, &self.bg, &[]);
        pass.dispatch_workgroups(NB as u32, NS as u32, 1);
    }

    /// Finish mapped readbacks and start mapping the buffers copied in previously submitted frames.
    pub fn harvest(&mut self, device: &wgpu::Device) {
        device.poll(wgpu::Maintain::Poll);
        for s in self.staging.iter_mut() {
            match s.state {
                StState::Mapping if s.flag.load(Ordering::Acquire) => {
                    {
                        let data = s.buf.slice(..).get_mapped_range();
                        let fl: &[f32] = bytemuck::cast_slice(&data[..]);
                        let n = (fl[0].to_bits() as usize).min(MIC_CAP);
                        let mic = (0..n).map(|k| [fl[4 + 4 * k], fl[5 + 4 * k], fl[6 + 4 * k], fl[7 + 4 * k]]).collect();
                        let off = (MIC_BYTES / 4) as usize;
                        let part = fl[off..off + NS * NB * NF].to_vec();
                        self.ready.push_back(FeatureFrame { info: s.info, mic, part });
                    }
                    s.buf.unmap();
                    s.state = StState::Free;
                }
                StState::Copied => {
                    let flag = s.flag.clone();
                    flag.store(false, Ordering::Release);
                    s.buf.slice(..).map_async(wgpu::MapMode::Read, move |_| flag.store(true, Ordering::Release));
                    s.state = StState::Mapping;
                }
                _ => {}
            }
        }
    }

    /// Frames that finished reading back, oldest first.
    pub fn take_frames(&mut self) -> Vec<FeatureFrame> {
        let mut v: Vec<FeatureFrame> = self.ready.drain(..).collect();
        v.sort_by(|a, b| a.info.sim_time.partial_cmp(&b.info.sim_time).unwrap_or(std::cmp::Ordering::Equal));
        v
    }
}

// ================================================================================================ analysis (main thread)

/// One synthesis source: rms sound pressure at the listener (Pa), spectral peak (Hz), pan (-1..1), distance (m).
#[derive(Clone, Copy, Default, Debug)]
pub struct Src {
    pub p: f32,
    pub f: f32,
    pub pan: f32,
    pub r: f32,
}

/// A Poisson event process: events per real second, median peak pressure at the listener (Pa), pan, distance.
#[derive(Clone, Copy, Default, Debug)]
pub struct Rate {
    pub rate: f32,
    pub amp: f32,
    pub pan: f32,
    pub r: f32,
}

#[derive(Clone, Copy, Default, Debug)]
pub struct BandCtl {
    pub comb: Src,
    /// 0..1 how hot (bright) the flame is
    pub comb_bright: f32,
    pub fss: Src,
    pub lss: Src,
    pub rumble: Src,
    pub turb: Src,
    pub fan: Src,
    pub hiss: Src,
    pub jet_crackle: Rate,
    /// waveform asymmetry of the large-scale jet noise (skewness ~ 6x this): crackling hot supersonic jets
    pub lss_asym: f32,
    pub wood: Rate,
    pub prop: Rate,
    pub mburn: Rate,
    pub boil: Rate,
    pub fizz: Rate,
    pub creak: Rate,
    pub creak_f1: f32,
    pub creak_q: f32,
}

/// Microphone sample (one per sim step): envelope of the high-passed pressure at the listener, the signed
/// high-passed pressure (LFE), the left/right balance and wind buffeting, all in Pa.
#[derive(Clone, Copy, Default, Debug)]
pub struct MicSample {
    pub env: f32,
    pub p: f32,
    pub pan: f32,
    pub wind: f32,
}

#[derive(Clone, Copy, Debug)]
pub enum Ev {
    /// shock front at the listener: pressure jump (Pa), direction, index of the mic sample it arrived on,
    /// `size` 0..1 hint of a nearby detonation (bigger thump/rumble)
    Shock { dp: f32, pan: f32, idx: usize, size: f32 },
    /// structure fracture: cells lost, modal frequency (Hz) and Q, position
    Break { amount: f32, f1: f32, q: f32, pan: f32, r: f32 },
}

/// Everything the synthesiser needs for one frame.
#[derive(Clone, Debug)]
pub struct Ctl {
    pub real_dt: f32,
    pub steps: u32,
    pub paused: bool,
    pub bands: [BandCtl; NB],
    pub mic: Vec<MicSample>,
    pub events: Vec<Ev>,
}

/// Debug/telemetry numbers of the last analysed frame (SI).
#[derive(Clone, Copy, Default, Debug)]
pub struct Telemetry {
    pub heat_w: f32,
    pub dqdt_ws: f32,
    pub jet_u: f32,
    pub jet_d: f32,
    pub jet_w: f32,
    pub jet_n: f32,
    pub jet_l: f32,
    pub fan_bpf: f32,
    pub mic_env: f32,
    pub mic_out: f32,
    pub wind: f32,
    pub shocks: u32,
    pub wood_cells: f32,
    pub loudest_band_db: f32,
    /// max gas pressure anywhere (atm) and the mean Mach number of the jet cells (diagnostics)
    pub pmax: f32,
    pub jet_mach: f32,
}

pub struct Analyzer {
    init: bool,
    prev_q: [f32; NB],
    dq_ema: [f32; NB],
    prev_sw: [f32; NB],
    prev_walls: [f32; NB],
    prev_fail: [f32; NB],
    fail_cool: [f32; NB],
    p_slow: [f32; 3],
    env: [f32; 3],
    prev_pc: f32,
    run: f32,
    run_len: f32,
    dnoise: f32,
    cool: f32,
    pan_s: f32,
    det: f32,
    floor: f32,
    gate: f32,
    /// distance of the listener in front of the sim plane (m): keeps panning smooth and levels finite
    pub listener_z_m: f32,
    pub tel: Telemetry,
}

/// Distance of the time-mapped quantities: physical seconds represented by one sim step.
const STEP_S: f32 = phys::DT * phys::T_UNIT_S;

impl Default for Analyzer {
    fn default() -> Self {
        Self::new()
    }
}

impl Analyzer {
    pub fn new() -> Self {
        Analyzer {
            init: false,
            prev_q: [0.0; NB],
            dq_ema: [0.0; NB],
            prev_sw: [0.0; NB],
            prev_walls: [0.0; NB],
            prev_fail: [0.0; NB],
            fail_cool: [0.0; NB],
            p_slow: [1.0; 3],
            env: [0.0; 3],
            prev_pc: 0.0,
            run: 0.0,
            run_len: 0.0,
            dnoise: 0.0,
            cool: 0.0,
            pan_s: 0.0,
            det: 0.0,
            floor: 0.0,
            gate: 0.0,
            listener_z_m: 0.6,
            tel: Telemetry::default(),
        }
    }

    /// pan and distance of a point dx, dy (cells from the listener), softened by the source size (m)
    fn place(&self, dx: f32, dy: f32, size_m: f32) -> (f32, f32) {
        let x = dx * phys::DX_M;
        let y = dy * phys::DX_M;
        let z = self.listener_z_m;
        let r = (x * x + y * y + z * z + 0.25 * size_m * size_m).sqrt().max(0.3);
        let pan = (0.9 * x / (x * x + z * z).sqrt().max(0.05)).clamp(-0.7, 0.7) * (x.abs() / (x.abs() + 0.5 * size_m + 1e-3)).sqrt();
        (pan, r)
    }

    pub fn process(&mut self, ff: &FeatureFrame) -> Ctl {
        let info = ff.info;
        let b = ff.bands();
        if !self.init || info.reset {
            *self = Analyzer { listener_z_m: self.listener_z_m, ..Analyzer::new() };
            self.init = true;
            for k in 0..NB {
                self.prev_walls[k] = b[k][f::WALLS];
                self.prev_sw[k] = b[k][f::SW];
            }
            if let Some(m) = ff.mic.first() {
                self.p_slow = [m[0], m[1], m[2]];
            }
        }
        let dt_tu = info.steps as f32 * phys::DT;
        let real_dt = info.real_dt.clamp(1e-3, 0.2);
        let mut bands = [BandCtl::default(); NB];
        let mut events = Vec::new();
        let mut tel = Telemetry::default();
        let cw = info.cam[2].max(1.0) / NB as f32;
        let km = 0.3 / (4.0 * PI * phys::C0 * phys::C0); // (gamma-1)/(4 pi c0^2)
        let slow = phys::slow_motion(info.speed);
        self.det = (self.det * (-real_dt / 1.0).exp()).max(0.0);

        // The flame and the jet are each ONE acoustic source whatever the screen bands cut them into: their size
        // (which sets the spectral peak and the acoustic depth of the 2D slice) comes from the moments summed
        // over all bands; the bands only distribute the power in space (pan, distance).
        let sum = |i: usize| b.iter().map(|s| s[i]).sum::<f32>();
        let gq = sum(f::Q);
        let lf_glob = if gq > 1e-6 {
            let mx = sum(f::QX) / gq;
            let my = sum(f::QY) / gq;
            let vx = (sum(f::QXX) / gq - mx * mx).max(0.0);
            let vy = (sum(f::QYY) / gq - my * my).max(0.0);
            ((12.0 * vx.max(vy)).sqrt() * phys::DX_M).clamp(0.03, 3.0)
        } else {
            0.1
        };
        let gfln = sum(f::FLN);
        tel.pmax = b.iter().map(|s| s[f::PMAX]).fold(0.0f32, f32::max);
        let jet = {
            let nj = sum(f::JN);
            let ke = sum(f::JKE);
            if nj >= 3.0 && ke > 0.0 {
                let u = sum(f::JU4) / (2.0 * ke) * phys::V_UNIT;
                let rho_j = sum(f::JRHO) / nj * phys::RHO_UNIT;
                let mx = sum(f::JX) / nj;
                let my = sum(f::JY) / nj;
                let cxx = (sum(f::JXX) / nj - mx * mx).max(0.0);
                let cyy = (sum(f::JYY) / nj - my * my).max(0.0);
                let cxy = sum(f::JXY) / nj - mx * my;
                let lmax = 0.5 * (cxx + cyy) + (0.25 * (cxx - cyy) * (cxx - cyy) + cxy * cxy).sqrt();
                let len = (12.0 * lmax).sqrt().max(1.0);
                let d_cells = (nj / len).clamp(1.0, len);
                let d = d_cells * phys::DX_M;
                let area = 0.25 * PI * d * d;
                let w_l = 3e-5 * phys::RHO0 * u.powi(8) * area / phys::C0.powi(5);
                let w_e = 0.005 * 0.5 * rho_j * u * u * u * area;
                let w = 1.0 / (1.0 / w_l.max(1e-20) + 1.0 / w_e.max(1e-20));
                Some((u, d, len * phys::DX_M, w, ke, nj))
            } else {
                None
            }
        };
        for k in 0..NB {
            let s = &b[k];
            let bc = &mut bands[k];
            let band_dx = info.cam[0] + (k as f32 + 0.5) * cw - info.listener[0];
            let (band_pan, band_r) = self.place(band_dx, 0.0, 0.3);

            // ---------------- combustion (monopole)
            let q = s[f::Q];
            let (mut qpan, mut qr, mut lf) = (band_pan, band_r, 0.1f32);
            if q > 1e-6 {
                let mx = s[f::QX] / q;
                let my = s[f::QY] / q;
                let vx = (s[f::QXX] / q - mx * mx).max(0.0);
                let vy = (s[f::QYY] / q - my * my).max(0.0);
                lf = ((12.0 * vx.max(vy)).sqrt() * phys::DX_M).clamp(0.03, 3.0);
                let (p, r) = self.place(mx, my, lf);
                qpan = p;
                qr = r;
            }
            // The solver's chemistry and buoyancy run on the game clock (a fire that burns for a game minute is
            // a real campfire), while its gas dynamics run at physical speed. Heat release is therefore taken per
            // game second: Qdot_phys / S (a 40 kW campfire instead of a 38 MW one), and so is its derivative.
            // acoustic depth of the slice and spectral peak: the whole flame's size (a band may hold a sliver of it)
            let lsrc = lf.max(lf_glob);
            let qdot = q * phys::W_PER_Q * lsrc.min(1.0) / slow; // W
            if info.steps > 0 {
                let dq = (qdot - self.prev_q[k]) / (dt_tu * phys::T_UNIT_S * slow); // W per game second
                self.dq_ema[k] += (dq.abs() - self.dq_ema[k]) * 0.3;
                self.prev_q[k] = qdot;
            }
            let nfl = s[f::FLN];
            let tf = if nfl > 0.0 { s[f::FLT] / nfl * 300.0 } else { 300.0 };
            let fpk = (180.0 / lsrc.max(0.05).sqrt() * (tf / 2000.0).max(0.2).sqrt()).clamp(60.0, 1500.0);
            // coherent patches: the whole flame's flame-cell count, shared by the bands in proportion
            let i_eff = COMB_I0 / (1.0 + gfln.max(nfl) / 16.0).sqrt();
            let p_sg = km / qr * 2.0 * PI * fpk * i_eff * qdot;
            let p_res = km / qr * self.dq_ema[k] * RESOLVED_DQ_GAIN;
            bc.comb = Src { p: (p_sg * p_sg + p_res * p_res).sqrt(), f: fpk, pan: qpan, r: qr };
            bc.comb_bright = ((tf - 1200.0) / 1500.0).clamp(0.0, 1.0);
            tel.heat_w += qdot;
            tel.dqdt_ws += self.dq_ema[k];

            // ---------------- jets (Lighthill / Eldred / Tam), one source shared by the bands it crosses
            let nj = s[f::JN];
            if let (Some((u, d, l_m, w_tot, ke_tot, nj_tot)), true) = (jet, nj >= 1.0 && s[f::JKE] > 0.0) {
                let w = w_tot * s[f::JKE] / ke_tot;
                let (pan, r) = self.place(s[f::JX] / nj, s[f::JY] / nj, l_m.min(2.0));
                let p = (w * phys::Z0 / (4.0 * PI * r * r)).sqrt();
                let mach = u / phys::C0;
                let ms = (mach - 1.0).max(0.0);
                // Peak Strouhal numbers fall for hot supersonic jets: the dominant large-scale (Mach-wave) source sits
                // at the end of the supersonic core, many diameters downstream (Tam 1996: St ~ 0.15-0.2 for cold
                // M < 1 jets; rockets St_D ~ 0.02-0.05 on the exit diameter, Eldred NASA SP-8072).
                let st_l = 0.17 / (1.0 + 0.4 * ms);
                let st_f = 0.35 / (1.0 + 0.15 * ms);
                let share_l = 0.35 + 0.35 * ((mach - 0.8) / 1.5).clamp(0.0, 1.0);
                // Tam's model has two components; the low "rumble" of big jets IS the large-scale hump at low St
                // (kept as a separate, very weak whole-plume layer only for long plumes)
                let share_r = 0.0 * l_m;
                let share_f = 1.0 - share_l - share_r;
                let fr = |sn: f32| (sn * u / d).clamp(20.0, 16000.0);
                bc.fss = Src { p: p * share_f.sqrt(), f: fr(st_f), pan, r };
                bc.lss = Src { p: p * share_l.sqrt(), f: fr(st_l), pan, r };
                bc.rumble = Src { p: p * share_r.sqrt(), f: (0.5 * fr(st_l)).min(0.2 * u / l_m).max(20.0), pan, r };
                // crackle (Ffowcs Williams 1975): shocklets from the shock cells embedded in a supersonic jet
                let shj = s[f::SHJ];
                if mach > 1.0 && shj > 0.0 {
                    let c = ((mach - 1.0) / 1.5).min(1.0);
                    // a few hundred shocklets per second for the whole jet (shared by the bands it crosses)
                    let share = s[f::JKE] / ke_tot;
                    bc.jet_crackle = Rate { rate: (6.0 * shj).min(CRACKLE_MAX_RATE * share) * c, amp: CRACKLE_AMP * p * c.sqrt(), pan, r };
                }
                // crackle is a source property (Ffowcs Williams et al. 1975): in hot, highly supersonic jets the
                // large-scale waves leave the source with sharp, tall compressions and broad, shallow rarefactions;
                // measured pressure skewness 0.3-0.6 for rockets and afterburning engines (Gee et al. 2013)
                bc.lss_asym = LSS_ASYM_MAX * ((mach - 1.5) / 2.0).clamp(0.0, 1.0);
                if k == 0 || w_tot > tel.jet_w {
                    tel.jet_w = w_tot;
                    tel.jet_u = u;
                    tel.jet_d = d;
                    tel.jet_n = nj_tot;
                    tel.jet_l = l_m;
                }
            }

            // ---------------- turbulence (Proudman)
            if s[f::PR] > 0.0 {
                let pr = s[f::PR];
                let w = PROUDMAN_W * pr;
                let up = s[f::PRU] / pr * phys::V_UNIT;
                let (pan, r) = self.place(s[f::PRX] / pr, s[f::PRY] / pr, 0.5);
                bc.turb = Src { p: (w * phys::Z0 / (4.0 * PI * r * r)).sqrt(), f: (up / 0.02).clamp(150.0, 12000.0), pan, r };
            }

            // ---------------- crackle / pyrolysis, boiling, reactions
            let crack_pos = if q > 1e-6 { (qpan, qr) } else { (band_pan, band_r) };
            let wn = s[f::WOODN];
            if wn > 0.0 {
                // median pop peak 0.25 Pa at 1 m (82 dB; burning-wood pops measure ~80-95 dB peak at 1 m). Audible wood crackle
                // is sparse (a few to ~60 pops/s) and heavy-tailed: denser Poisson trains merge into hiss (kurtosis -> 3)
                bc.wood = Rate { rate: (0.05 * wn + 12.0 * s[f::WOODR]).min(60.0), amp: 0.25 / crack_pos.1, pan: crack_pos.0, r: crack_pos.1 };
                tel.wood_cells += wn;
            }
            if s[f::PROPN] > 0.0 {
                bc.prop = Rate { rate: (1.5 * s[f::PROPN]).min(1500.0), amp: 0.03 / crack_pos.1, pan: crack_pos.0, r: crack_pos.1 };
            }
            if s[f::MBN] > 0.0 {
                bc.mburn = Rate { rate: (0.12 * s[f::MBN]).min(300.0), amp: 0.05 / band_r, pan: band_pan, r: band_r };
            }
            if s[f::BOILN] > 0.0 {
                bc.boil = Rate { rate: (2.0 * s[f::BOILN]).min(3000.0), amp: 0.012 / band_r, pan: band_pan, r: band_r };
            }
            if s[f::FIZN] > 0.0 {
                bc.fizz = Rate { rate: (1.0 * s[f::FIZN]).min(1500.0), amp: 0.02 / band_r, pan: band_pan, r: band_r };
            }

            // ---------------- structures
            let sw = s[f::SW];
            if sw > 0.0 {
                let dsw = (sw - self.prev_sw[k]).max(0.0) / real_dt;
                // acoustic emission follows the release of strain energy (stress changes), not a static load
                let rate = (2.0 * dsw.sqrt()).min(30.0);
                let smax = s[f::SMAX].min(1.5);
                bc.creak = Rate { rate, amp: 0.06 * (1.0 + 6.0 * smax * smax) / band_r, pan: band_pan, r: band_r };
                bc.creak_f1 = (s[f::SWF] / sw).clamp(60.0, 3000.0);
                bc.creak_q = (s[f::SWQ] / sw).clamp(5.0, 2000.0);
            }
            if info.steps > 0 {
                self.prev_sw[k] = sw;
            }
            let walls = s[f::WALLS];
            let lost = (self.prev_walls[k] - walls).max(0.0);
            let broke = lost.min(self.prev_fail[k].max(s[f::FAIL]));
            self.fail_cool[k] -= real_dt;
            if broke >= 1.0 && self.fail_cool[k] <= 0.0 {
                let f1 = if s[f::FAIL] > 0.0 { s[f::FAILF] / s[f::FAIL] } else if bc.creak_f1 > 0.0 { bc.creak_f1 } else { 380.0 };
                events.push(Ev::Break { amount: broke, f1, q: if bc.creak_q > 0.0 { bc.creak_q } else { 600.0 }, pan: band_pan, r: band_r });
                self.fail_cool[k] = 0.08;
            }
            self.prev_walls[k] = walls;
            self.prev_fail[k] = s[f::FAIL];

            // ---------------- fans (blade passing)
            let nf = s[f::FANN];
            if nf > 0.0 {
                let uf = s[f::FANU] / nf * phys::V_UNIT;
                let dfan = (nf.sqrt() * 2.0 * phys::DX_M).clamp(0.05, 2.0);
                // blade tip speed: the solver's pressure-driven gas speed is not a blade speed; real fan tips are <= transonic
                let utip = (uf.min(300.0) + 15.0 * info.fan_pow.max(0.0).sqrt()).min(380.0);
                let bpf = (7.0 * utip / (PI * dfan)).clamp(30.0, 4000.0);
                let w = 2e-5 * phys::RHO0 * utip.powi(6) * dfan * dfan / phys::C0.powi(3);
                bc.fan = Src { p: (w * phys::Z0 / (4.0 * PI * band_r * band_r)).sqrt(), f: bpf, pan: band_pan, r: band_r };
                tel.fan_bpf = bpf;
            }

            // ---------------- gas inlets (small-jet Lighthill hiss)
            let ni = s[f::INN];
            if ni > 0.0 && s[f::INR] > 0.0 {
                let u = s[f::INF] / s[f::INR] * phys::V_UNIT;
                // only pressure-driven inlet flow (as for the Proudman and jet terms): the slower feed flow that
                // sustains a flame is a game-clock quantity (a 40 kW flare is fed at ~1 m/s, which is silent)
                let pd = ((u - INLET_PD_MS) / 80.0).clamp(0.0, 1.0);
                let w = pd * 3e-5 * phys::RHO0 * u.powi(8) * ni * phys::DX_M * phys::DX_M / phys::C0.powi(5);
                // round nozzle of the same area as the inlet's gas face (as in the Lighthill power above): St 0.2
                let d = (4.0 * ni / PI).sqrt() * phys::DX_M;
                bc.hiss = Src { p: (w * phys::Z0 / (4.0 * PI * band_r * band_r)).sqrt(), f: (0.2 * u / d).clamp(150.0, 12000.0), pan: band_pan, r: band_r };
            }
            if s[f::DETN] > 0.0 {
                self.det = 1.0;
            }
            let lvl = [bc.comb.p, bc.fss.p, bc.lss.p, bc.turb.p, bc.fan.p].iter().fold(0.0f32, |a, &v| a.max(v));
            tel.loudest_band_db = tel.loudest_band_db.max(spl(lvl));
        }

        // ---------------- microphone
        let mut mic = Vec::with_capacity(ff.mic.len());
        // sim time between microphone samples (GpuAcoustics::mic_every steps)
        let dts = if ff.mic.is_empty() { phys::DT } else { dt_tu / ff.mic.len() as f32 };
        let a_slow = (dts / MIC_HP_TU).min(1.0);
        let d_env = (-dts / MIC_ENV_TU).exp();
        let a_floor = dts / (MIC_FLOOR_S * phys::TPS * info.speed.max(0.05));
        let d_gate = (-dts / (MIC_GATE_S * phys::TPS * info.speed.max(0.05))).exp();
        let a_noise = (dts / 5.6).min(1.0);
        let a_pan = (dts / 1.4).min(1.0);
        for (idx, m) in ff.mic.iter().enumerate() {
            let mut pp = [0.0f32; 3];
            for e in 0..3 {
                let v = if m[e].is_finite() { m[e] } else { 1.0 };
                self.p_slow[e] += (v - self.p_slow[e]) * a_slow;
                pp[e] = (v - self.p_slow[e]) * phys::P_UNIT_PA;
                self.env[e] = pp[e].abs().max(self.env[e] * d_env);
            }
            let pc = pp[1];
            let d = pc - self.prev_pc;
            self.prev_pc = pc;
            self.dnoise += (d.abs() - self.dnoise) * a_noise;
            let pan_now = ((self.env[2] - self.env[0]) / (self.env[2] + self.env[0] + 50.0) * 3.0).clamp(-1.0, 1.0);
            self.pan_s += (pan_now - self.pan_s) * a_pan;
            self.cool = (self.cool - dts).max(0.0);
            if d > 0.0 {
                self.run += d;
                self.run_len += dts;
            } else {
                let thr = SHOCK_MIN_PA.max(12.0 * self.dnoise).max(0.3 * self.floor);
                if self.run > thr && self.run_len <= SHOCK_RISE_TU && self.cool <= 0.0 {
                    self.gate = 1.0;
                    events.push(Ev::Shock { dp: self.run, pan: pan_now, idx, size: self.det });
                    tel.shocks += 1;
                    self.cool = 1.1;
                }
                self.run = 0.0;
                self.run_len = 0.0;
            }
            // wind buffeting only from fast (pressure-driven) flow over the listener; slower flow speeds are
            // inflated by the game-clock buoyancy/chemistry
            let u = if m[3].is_finite() { (m[3] * phys::V_UNIT - WIND_MIN_MS).max(0.0) } else { 0.0 };
            // transient extraction: the sustained pressure "sloshing" driven by the game-clock chemistry is
            // removed by an adaptive floor (3 game-seconds memory); blasts, reflections and their reverb pass
            self.floor += (self.env[1] - self.floor) * a_floor;
            self.gate *= d_gate;
            let env = (self.env[1] - 1.5 * self.floor).max(0.0) * self.gate * MIC_GAIN;
            mic.push(MicSample { env, p: pc * MIC_GAIN, pan: self.pan_s, wind: 0.5 * phys::RHO0 * u * u * WIND_FRACTION });
            tel.mic_env = tel.mic_env.max(self.env[1]);
            tel.mic_out = tel.mic_out.max(env);
            tel.wind = tel.wind.max(0.5 * phys::RHO0 * u * u * WIND_FRACTION);
        }
        self.tel = tel;
        Ctl { real_dt, steps: info.steps, paused: info.paused || info.steps == 0, bands, mic, events }
    }
}

/// rms heat-release fluctuation intensity of one coherent flame patch (patches of 16 cells add incoherently)
pub const COMB_I0: f32 = 0.05;
/// inlet flow speed (m/s) above which the inlet flow counts as pressure-driven (Proudman threshold: 0.5 jet_u)
const INLET_PD_MS: f32 = 200.0;
/// median shocklet peak relative to the rms jet noise pressure (crackle: peaks of 3-5 sigma, Gee et al. 2013)
pub const CRACKLE_AMP: f32 = 3.0;
/// largest quadratic asymmetry of the large-scale jet noise (pressure skewness ~ 6x)
const LSS_ASYM_MAX: f32 = 0.14;
/// most shocklets per second for a whole jet
const CRACKLE_MAX_RATE: f32 = 1500.0;
/// largest shocklet relative to the median
const CRACKLE_CAP: f32 = 14.0;
/// shocklet relaxation time: CRACKLE_TAU0 + CRACKLE_TAU1 * U(0,1) seconds (physical)
const CRACKLE_TAU0: f32 = 0.00008;
const CRACKLE_TAU1: f32 = 0.00022;
/// weight of the solver-resolved dQ/dt term relative to the textbook monopole formula
pub const RESOLVED_DQ_GAIN: f32 = 1.0;
/// Proudman: W per unit of the GPU sum (38 * rho_unit * (dx^2 / t_unit^3) * cell volume of 0.1 m depth)
pub const PROUDMAN_W: f32 = 38.0 * phys::RHO_UNIT * (phys::DX_M * phys::DX_M / (phys::T_UNIT_S * phys::T_UNIT_S * phys::T_UNIT_S)) * phys::DX_M * phys::DX_M * 0.1;
/// high-pass time constant of the microphone (removes slow pressurisation / hydrostatics), sim tu
const MIC_HP_TU: f32 = 25.0;
/// envelope release of the microphone, sim tu (~0.06 s game time at 1x)
const MIC_ENV_TU: f32 = 2.0;
/// smallest pressure jump reported as a shock (Pa) (~140 dB peak)
const SHOCK_MIN_PA: f32 = 2000.0;
/// longest pressure rise (sim tu) still counted as one shock front (the solver smears a front over ~3 cells)
const SHOCK_RISE_TU: f32 = 5.6;
/// level of the solver microphone relative to the source models (the solver's pressure field near flames is
/// inflated by its game-clock chemistry; blasts are physical and stay far above the other layers)
const MIC_GAIN: f32 = 0.05;
/// adaptive floor memory of the microphone transient extractor (game seconds)
const MIC_FLOOR_S: f32 = 1.0;
/// how long the microphone layer stays open after a detected shock (game seconds): the blast's own
/// positive/negative phase, reflections and the solver's room reverberation
const MIC_GATE_S: f32 = 1.5;
/// fraction of the dynamic pressure heard as wind buffeting at the ear
const WIND_FRACTION: f32 = 0.003;
/// flow speed (m/s) below which the listener feels no wind
const WIND_MIN_MS: f32 = 120.0;

/// dB SPL of an rms pressure (Pa)
pub fn spl(p: f32) -> f32 {
    20.0 * (p.max(1e-9) / 20e-6).log10()
}

/// Loudness mapping (documented in the header): output level (dBFS rms per channel at centre pan) vs SPL of
/// the total continuous sound: 0.22 dB per dB up to 130 dB SPL (75 dB -> -31 dBFS, 100 dB -> -25.6, 130 dB ->
/// -19), 0.10 dB per dB above (160 dB -> -16, 170 dB -> -15): loud jets and rockets keep ~15-20 dB of headroom
/// for their crackle and the limiter stays out of their crest factor. Applied to the TOTAL pressure (see
/// `Synth::push`): every layer gets the same gain `loud(P)/P`, so the balance between simultaneous sources and
/// the spectral shapes stay physical (linear). Transients louder than the continuous sound follow the steeper
/// `loud_ev` curve on top of it (a blast stays far above a roaring rocket).
#[inline]
pub fn loud(p: f32) -> f32 {
    if p <= 1e-7 {
        return 0.0;
    }
    let l = spl(p);
    let db = if l <= LOUD_KNEE { LOUD_AT_KNEE + LOUD_K_LO * (l - LOUD_KNEE) } else { LOUD_AT_KNEE + LOUD_K_HI * (l - LOUD_KNEE) };
    // amplitude whose per-channel rms at centre pan (gain 0.707) is `db`
    std::f32::consts::SQRT_2 * 10f32.powf(db / 20.0)
}
/// gain of the mapping for transients relative to the continuous sound (0.25 dB per dB above it)
#[inline]
fn loud_ev_ratio(p: f32, p_ref: f32) -> f32 {
    if p <= p_ref {
        1.0
    } else {
        (p_ref / p).powf(1.0 - LOUD_K_EV)
    }
}
const LOUD_KNEE: f32 = 130.0;
const LOUD_AT_KNEE: f32 = -19.0;
const LOUD_K_LO: f32 = 0.22;
const LOUD_K_HI: f32 = 0.10;
const LOUD_K_EV: f32 = 0.25;
/// gain of the loudness mapping at total pressure p (Pa -> output units), finite at p = 0
#[inline]
fn loud_gain(p: f32) -> f32 {
    let p = p.max(2e-3);
    loud(p) / p
}

// ================================================================================================ synthesis (audio thread)

#[derive(Clone, Copy)]
struct Rng(u32);
impl Rng {
    #[inline]
    fn u(&mut self) -> f32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x;
        ((x >> 8) as f32 + 0.5) * (1.0 / 16_777_216.0)
    }
    /// unit-variance white noise
    #[inline]
    fn w(&mut self) -> f32 {
        (self.u() * 2.0 - 1.0) * 1.732_050_8
    }
}

/// Topology-preserving-transform state-variable filter (stable under fast modulation).
#[derive(Clone, Copy, Default)]
struct Svf {
    ic1: f32,
    ic2: f32,
    a1: f32,
    a2: f32,
    a3: f32,
    k: f32,
}
impl Svf {
    fn new(fc: f32, q: f32, sr: f32) -> Self {
        let mut s = Svf::default();
        s.set(fc, q, sr);
        s
    }
    fn set(&mut self, fc: f32, q: f32, sr: f32) {
        let fc = fc.clamp(8.0, 0.45 * sr);
        let g = (PI * fc / sr).tan();
        self.k = 1.0 / q.max(0.05);
        self.a1 = 1.0 / (1.0 + g * (g + self.k));
        self.a2 = g * self.a1;
        self.a3 = g * self.a2;
    }
    /// (low, band, high)
    #[inline]
    fn tick(&mut self, x: f32) -> (f32, f32, f32) {
        let v3 = x - self.ic2;
        let v1 = self.a1 * self.ic1 + self.a2 * v3;
        let v2 = self.ic2 + self.a2 * self.ic1 + self.a3 * v3;
        self.ic1 = 2.0 * v1 - self.ic1;
        self.ic2 = 2.0 * v2 - self.ic2;
        (v2, v1, x - self.k * v1 - v2)
    }
}

/// Transparent safety clipper: linear up to 0.9 (so it never colours the crest factor / skewness of crackle
/// and blast fronts that the lookahead limiter already kept below its ceiling), smooth knee to 1.0 above.
#[inline]
fn soft(x: f32) -> f32 {
    let a = x.abs();
    if a <= 0.9 {
        x
    } else {
        x.signum() * (0.9 + 0.1 * ((a - 0.9) / 0.1).tanh())
    }
}

#[inline]
fn pan_gains(p: f32) -> (f32, f32) {
    let a = (p.clamp(-1.0, 1.0) + 1.0) * 0.25 * PI;
    (a.cos(), a.sin())
}

/// Spectral shape of a filtered-noise layer, relative to its peak frequency f: high-pass at hp*f and low-pass at
/// lp*f (2-pole with the given Q, or 1-pole when the Q is 0), and an extra 1-pole low-pass at lp1*f (0 = none).
/// 1-pole edges give the f^2 rise / f^-2 fall of the textbook jet and combustion noise spectra. The output is
/// normalised to unit rms with the shape's equivalent noise bandwidth, computed numerically (`Shape::enbw`).
#[derive(Clone, Copy)]
struct Shape {
    hp: f32,
    hq: f32,
    lp: f32,
    lq: f32,
    lp1: f32,
}
impl Shape {
    /// power response at x = frequency / f
    fn h2(&self, x: f32) -> f32 {
        let h = x / self.hp;
        let hp = if self.hq > 0.0 { h.powi(4) / ((1.0 - h * h).powi(2) + (h / self.hq).powi(2)) } else { h * h / (1.0 + h * h) };
        let l = x / self.lp;
        let lp = if self.lq > 0.0 { 1.0 / ((1.0 - l * l).powi(2) + (l / self.lq).powi(2)) } else { 1.0 / (1.0 + l * l) };
        let l1 = if self.lp1 > 0.0 { 1.0 / (1.0 + (x / self.lp1).powi(2)) } else { 1.0 };
        hp * lp * l1
    }
    /// equivalent noise bandwidth in units of f: integral of h2 over x (log-spaced trapezoid, 1e-3..1e3)
    fn enbw(&self) -> f32 {
        let n = 600;
        let mut acc = 0.0;
        let mut prev = (1e-3f32, self.h2(1e-3));
        for i in 1..=n {
            let x = 10f32.powf(-3.0 + 6.0 * i as f32 / n as f32);
            let y = self.h2(x);
            acc += 0.5 * (y + prev.1) * (x - prev.0);
            prev = (x, y);
        }
        acc
    }
}
// combustion noise: f^2 below the peak, ~f^-2 to f^-4 above (Rajaram & Lieuwen 2009: f^-2..-3 over 1-2 decades)
const SH_COMB: Shape = Shape { hp: 1.0, hq: 0.0, lp: 1.0, lq: 0.0, lp1: 3.5 };
// jet noise (Tam's similarity spectra): large-scale F-spectrum, narrower; fine-scale G-spectrum, broad
const SH_LSS: Shape = Shape { hp: 1.0, hq: 0.0, lp: 1.0, lq: 0.0, lp1: 10.0 };
const SH_FSS: Shape = Shape { hp: 0.6, hq: 0.0, lp: 1.6, lq: 0.0, lp1: 12.0 };
const SH_RUMBLE: Shape = Shape { hp: 0.7, hq: 0.7, lp: 1.0, lq: 0.0, lp1: 4.0 };
const SH_TURB: Shape = Shape { hp: 1.0, hq: 0.0, lp: 1.0, lq: 0.0, lp1: 6.0 };
const SH_HISS: Shape = Shape { hp: 0.7, hq: 0.0, lp: 1.4, lq: 0.0, lp1: 8.0 };
const SH_FANBB: Shape = Shape { hp: 0.6, hq: 0.8, lp: 2.0, lq: 0.7, lp1: 5.0 };
const SH_BODY: Shape = Shape { hp: 0.25, hq: 0.7, lp: 1.0, lq: 0.9, lp1: 3.0 };
const SH_BROAD: Shape = Shape { hp: 0.35, hq: 0.7, lp: 3.0, lq: 0.6, lp1: 8.0 };

/// Smoothed control value with per-block linear ramps.
#[derive(Clone, Copy, Default)]
struct Sm {
    v: f32,
    t: f32,
    step: f32,
}
impl Sm {
    fn block(&mut self, a: f32, n: usize) {
        let nv = self.v + (self.t - self.v) * a;
        self.step = (nv - self.v) / n as f32;
    }
    #[inline]
    fn tick(&mut self) -> f32 {
        self.v += self.step;
        self.v
    }
}

#[derive(Clone, Copy)]
struct NoiseLayer {
    sh: Shape,
    bw: f32,
    hp: Svf,
    lp: Svf,
    hz: f32,
    hc: f32,
    lz: f32,
    lc: f32,
    y1: f32,
    c1: f32,
    norm: f32,
    amp: Sm,
    pan: Sm,
    f: f32,
    ft: f32,
    /// quadratic waveform asymmetry: unit-rms u -> u + asym (u^2 - 1)  (skewness ~ 6 asym, zero mean)
    asym: f32,
}
impl NoiseLayer {
    fn new(sh: Shape, f: f32, sr: f32) -> Self {
        let mut l = NoiseLayer { sh, bw: sh.enbw(), hp: Svf::default(), lp: Svf::default(), hz: 0.0, hc: 1.0, lz: 0.0, lc: 1.0, y1: 0.0, c1: 1.0, norm: 1.0, amp: Sm::default(), pan: Sm::default(), f, ft: f, asym: 0.0 };
        l.retune(sr);
        l
    }
    fn retune(&mut self, sr: f32) {
        let f = self.f;
        let pole = |fc: f32| 1.0 - (-2.0 * PI * fc.min(0.45 * sr) / sr).exp();
        if self.sh.hq > 0.0 {
            self.hp.set(self.sh.hp * f, self.sh.hq, sr);
        } else {
            self.hc = pole(self.sh.hp * f);
        }
        if self.sh.lq > 0.0 {
            self.lp.set(self.sh.lp * f, self.sh.lq, sr);
        } else {
            self.lc = pole(self.sh.lp * f);
        }
        self.c1 = if self.sh.lp1 > 0.0 { pole(self.sh.lp1 * f) } else { 1.0 };
        // unit rms (white noise of unit variance has a one-sided density of 2 / sr per Hz); the band above
        // Nyquist is lost for high f, which the cap keeps from being over-compensated
        self.norm = (sr / (2.0 * self.bw * f)).sqrt().min(40.0);
    }
    fn target(&mut self, amp: f32, f: f32, pan: f32) {
        self.amp.t = amp;
        self.ft = f.max(10.0);
        self.pan.t = pan;
    }
    fn active(&self) -> bool {
        self.amp.v > 1e-6 || self.amp.t > 1e-6
    }
    fn block(&mut self, a: f32, n: usize, sr: f32) {
        self.amp.block(a, n);
        self.pan.block(a, n);
        let nf = (self.f.ln() + (self.ft.ln() - self.f.ln()) * a).exp();
        if (nf / self.f - 1.0).abs() > 0.002 {
            self.f = nf;
            self.retune(sr);
        }
    }
    #[inline]
    fn tick(&mut self, w: f32) -> (f32, f32) {
        let h = if self.sh.hq > 0.0 {
            self.hp.tick(w).2
        } else {
            self.hz += (w - self.hz) * self.hc;
            w - self.hz
        };
        let l = if self.sh.lq > 0.0 {
            self.lp.tick(h).0
        } else {
            self.lz += (h - self.lz) * self.lc;
            self.lz
        };
        self.y1 += (l - self.y1) * self.c1;
        let u = self.norm * self.y1;
        let a = self.amp.tick() * if self.asym != 0.0 { u + self.asym * (u * u - 1.0) } else { u };
        let (gl, gr) = pan_gains(self.pan.tick());
        (a * gl, a * gr)
    }
}

/// 2-pole modal resonator (unit-peak impulse response).
#[derive(Clone, Copy, Default)]
struct Mode {
    y1: f32,
    y2: f32,
    a1: f32,
    a2: f32,
    b0: f32,
}
impl Mode {
    fn set(&mut self, f: f32, q: f32, sr: f32) {
        let f = f.min(0.45 * sr);
        let w = 2.0 * PI * f / sr;
        let r = (-PI * f / (q.max(1.0) * sr)).exp();
        self.a1 = 2.0 * r * w.cos();
        self.a2 = r * r;
        self.b0 = w.sin();
    }
    #[inline]
    fn tick(&mut self, x: f32) -> f32 {
        let y = self.b0 * x + self.a1 * self.y1 - self.a2 * self.y2;
        self.y2 = self.y1;
        self.y1 = y;
        y
    }
}
const MODE_RATIOS: [f32; 4] = [1.0, 2.756, 5.404, 8.933];
const MODE_GAINS: [f32; 4] = [1.0, 0.6, 0.4, 0.25];

#[derive(Clone, Copy, PartialEq)]
enum GK {
    Noise,
    NoiseHp,
    Ping,
    /// crackle shocklet: instantaneous compression, exponential relaxation, shallow zero-mean rarefaction tail
    Shocklet,
    /// blast wave at the listener: Friedlander waveform (1 - t/T) exp(-t/T) (zero net impulse), sharp front
    Friedlander,
}

/// Short sound grain (pop, crack, bubble ping, debris ping, thump).
#[derive(Clone, Copy)]
struct Grain {
    on: bool,
    kind: GK,
    delay: u32,
    env: f32,
    dec: f32,
    att: f32,
    amp: f32,
    gl: f32,
    gr: f32,
    svf: Svf,
    re: f32,
    im: f32,
    w: f32,
    chirp: f32,
    cnt: u32,
    c: f32,
    s: f32,
    /// second envelope (shocklet rarefaction tail) and its decay; Friedlander: 1 / (T sr)
    e2: f32,
    d2: f32,
}
impl Default for Grain {
    fn default() -> Self {
        Grain { on: false, kind: GK::Noise, delay: 0, env: 0.0, dec: 0.0, att: 1.0, amp: 0.0, gl: 0.0, gr: 0.0, svf: Svf::default(), re: 1.0, im: 0.0, w: 0.0, chirp: 1.0, cnt: 0, c: 1.0, s: 0.0, e2: 0.0, d2: 0.0 }
    }
}

/// Long low-frequency tail of an explosion (lowpassed noise with a falling cutoff).
#[derive(Clone, Copy, Default)]
struct Rumble {
    on: bool,
    env: f32,
    dec: f32,
    amp: f32,
    f: f32,
    fdec: f32,
    lp: Svf,
    lp2: Svf,
    gl: f32,
    gr: f32,
    cnt: u32,
}

#[derive(Clone, Copy)]
struct BandVoice {
    comb: NoiseLayer,
    fss: NoiseLayer,
    lss: NoiseLayer,
    rumble: NoiseLayer,
    turb: NoiseLayer,
    hiss: NoiseLayer,
    fan_bb: NoiseLayer,
    fan_amp: Sm,
    fan_f: f32,
    fan_ft: f32,
    fan_ph: [f32; 3],
    fan_pan: f32,
    modes: [Mode; 4],
    mode_f: f32,
    mode_q: f32,
    mode_pan: f32,
    stick_left: u32,
    stick_cnt: f32,
    stick_period: f32,
    stick_amp: f32,
    flick: f32,
    flick_t: f32,
    flick_cnt: u32,
    air: [f32; 2],
    air_c: f32,
    /// lss asymmetry weighted by the band's large-scale pressure (the asymmetry is applied to the summed field)
    lss_asym_t: f32,
    /// pending strike of the modal structure (fracture), applied as an impulse force (sine-phase response)
    strike: f32,
    ctl: BandCtl,
}

impl BandVoice {
    fn new(sr: f32) -> Self {
        let mut modes = [Mode::default(); 4];
        for (k, m) in modes.iter_mut().enumerate() {
            m.set(380.0 * MODE_RATIOS[k], 600.0, sr);
        }
        BandVoice {
            comb: NoiseLayer::new(SH_COMB, 300.0, sr),
            fss: NoiseLayer::new(SH_FSS, 2000.0, sr),
            lss: NoiseLayer::new(SH_LSS, 800.0, sr),
            rumble: NoiseLayer::new(SH_RUMBLE, 60.0, sr),
            turb: NoiseLayer::new(SH_TURB, 1000.0, sr),
            hiss: NoiseLayer::new(SH_HISS, 3000.0, sr),
            fan_bb: NoiseLayer::new(SH_FANBB, 600.0, sr),
            fan_amp: Sm::default(),
            fan_f: 300.0,
            fan_ft: 300.0,
            fan_ph: [0.0; 3],
            fan_pan: 0.0,
            modes,
            mode_f: 380.0,
            mode_q: 600.0,
            mode_pan: 0.0,
            stick_left: 0,
            stick_cnt: 0.0,
            stick_period: 1.0,
            stick_amp: 0.0,
            flick: 1.0,
            flick_t: 1.0,
            flick_cnt: 0,
            air: [0.0; 2],
            air_c: 1.0,
            strike: 0.0,
            lss_asym_t: 0.0,
            ctl: BandCtl::default(),
        }
    }
}

/// Per-layer mix gains (1 = physical balance).
#[derive(Clone, Copy, Debug)]
pub struct Mix {
    pub combustion: f32,
    pub jet: f32,
    pub crackle: f32,
    pub microphone: f32,
    pub structure: f32,
    pub machinery: f32,
    pub lfe: f32,
}
impl Default for Mix {
    fn default() -> Self {
        // lfe: the solver's game-time waveform below 40 Hz is a slow-motion rendering, not a physical sound:
        // kept as a faint body under blasts only
        Mix { combustion: 1.0, jet: 1.0, crackle: 1.0, microphone: 1.0, structure: 1.0, machinery: 1.0, lfe: 0.1 }
    }
}

struct MicEntry {
    s: MicSample,
    a_body: f32,
    a_broad: f32,
    a_wind: f32,
    lfe_g: f32,
    dur: f32,
    ev: Option<Ev>,
}

/// The physically driven synthesiser. Feed it `Ctl`s (one per sim frame) and pull stereo samples.
pub struct Synth {
    sr: f32,
    rng: Rng,
    bands: Vec<BandVoice>,
    grains: Vec<Grain>,
    gi: usize,
    rumbles: [Rumble; 4],
    // microphone
    fifo: VecDeque<MicEntry>,
    mic_pos: f32,
    cur: Option<MicEntry>,
    body: NoiseLayer,
    broad: NoiseLayer,
    wind: NoiseLayer,
    lfe_lp: Svf,
    lfe_lp2: Svf,
    lfe_hp: Svf,
    mic_last: [f32; 6],
    // large-scale jet field asymmetry: amount, running mean square per ear and its smoothing coefficient
    lss_asym: f32,
    lss_ms: [f32; 2],
    lss_ms_c: f32,
    // loudness: smoothed total sound pressure of the continuous layers (Pa) and the shared gain loud(P)/P
    p_ref: f32,
    g_cont: f32,
    // output: DC block + master here, then the shared output bus (audio.rs: reverb, lookahead limiter, clipper)
    dc: [f32; 4],
    /// shared output bus (reverb return `bus.wet` is 0 for offline renders unless set)
    pub bus: crate::audio::Bus,
    pub mix: Mix,
    master: f32,
    pub master_target: f32,
    fade: f32,
    fade_t: f32,
    block_left: usize,
    frame_samples: f32,
}

const BLOCK: usize = 64;
const MAX_GRAINS: usize = 160;

impl Synth {
    pub fn new(sr: f32) -> Self {
        Synth {
            sr,
            rng: Rng(0x9E37_79B9),
            bands: (0..NB).map(|_| BandVoice::new(sr)).collect(),
            grains: vec![Grain::default(); MAX_GRAINS],
            gi: 0,
            rumbles: [Rumble::default(); 4],
            fifo: VecDeque::with_capacity(1024),
            mic_pos: 0.0,
            cur: None,
            body: NoiseLayer::new(SH_BODY, 90.0, sr),
            broad: NoiseLayer::new(SH_BROAD, 1200.0, sr),
            wind: NoiseLayer::new(SH_BODY, 120.0, sr),
            lfe_lp: Svf::new(40.0, 0.7, sr),
            lfe_lp2: Svf::new(40.0, 0.7, sr),
            lfe_hp: Svf::new(15.0, 0.7, sr),
            mic_last: [0.0; 6],
            lss_asym: 0.0,
            lss_ms: [0.0; 2],
            lss_ms_c: 1.0 - (-1.0 / (0.03 * sr)).exp(),
            p_ref: 0.0,
            g_cont: loud_gain(0.0),
            dc: [0.0; 4],
            bus: crate::audio::Bus::new(sr),
            mix: Mix::default(),
            master: 0.8,
            master_target: 0.8,
            fade: 0.0,
            fade_t: 1.0,
            block_left: 0,
            frame_samples: sr / 60.0,
        }
    }

    pub fn sample_rate(&self) -> f32 {
        self.sr
    }

    /// Hand over one analysed frame.
    pub fn push(&mut self, ctl: Ctl) {
        self.fade_t = if ctl.paused { 0.0 } else { 1.0 };
        self.frame_samples = ctl.real_dt * self.sr;
        let mut p2 = 0.0f32;
        for (k, b) in ctl.bands.iter().enumerate() {
            self.bands[k].ctl = *b;
            for v in [b.comb.p, b.fss.p, b.lss.p, b.rumble.p, b.turb.p, b.hiss.p] {
                p2 += v * v;
            }
            p2 += 1.5 * b.fan.p * b.fan.p;
        }
        // total continuous sound pressure: fast attack, 0.4 s release (a compressor's program-dependent gain)
        let pt = p2.sqrt();
        let rel = 1.0 - (-ctl.real_dt / 0.4).exp();
        self.p_ref = if pt > self.p_ref { self.p_ref + (pt - self.p_ref) * 0.5 } else { self.p_ref + (pt - self.p_ref) * rel };
        self.g_cont = loud_gain(self.p_ref);
        let n = ctl.mic.len();
        if n > 0 {
            let dur = ctl.real_dt * self.sr / n as f32;
            let mut evs: Vec<Option<Ev>> = vec![None; n];
            for e in &ctl.events {
                if let Ev::Shock { idx, .. } = e {
                    evs[(*idx).min(n - 1)] = Some(*e);
                }
            }
            for (k, s) in ctl.mic.iter().enumerate() {
                let a = self.ev_amp(s.env);
                self.fifo.push_back(MicEntry {
                    s: *s,
                    a_body: a * 0.9,
                    a_broad: a * 0.45,
                    a_wind: s.wind * self.g_cont,
                    lfe_g: if s.env > 1e-3 { (a / s.env).min(0.05) } else { 0.0 },
                    dur,
                    ev: evs[k].take(),
                });
            }
            // keep latency bounded if the consumer stalled
            while self.fifo.len() > 8 * n.max(1) {
                self.fifo.pop_front();
            }
        }
        for e in ctl.events {
            if let Ev::Break { .. } = e {
                self.trigger(e);
            }
        }
    }

    /// output amplitude of an event of peak pressure p (Pa): linear on top of the continuous sound, compressed
    /// by itself once it dominates
    #[inline]
    fn ev_amp(&self, p: f32) -> f32 {
        p * self.g_cont * loud_ev_ratio(0.5 * p, self.p_ref.max(2e-3))
    }

    fn spawn(&mut self, g: Grain) {
        // take a free slot, else steal round-robin
        for _ in 0..MAX_GRAINS {
            self.gi = (self.gi + 1) % MAX_GRAINS;
            if !self.grains[self.gi].on {
                self.grains[self.gi] = g;
                return;
            }
        }
        self.gi = (self.gi + 1) % MAX_GRAINS;
        self.grains[self.gi] = g;
    }

    fn decay_for(sr: f32, secs: f32) -> f32 {
        (-1.0 / (secs.max(1e-4) * sr)).exp()
    }

    fn noise_grain(sr: f32, fc: f32, q: f32, secs: f32, amp: f32, pan: f32, hp: bool, delay: f32) -> Grain {
        let (gl, gr) = pan_gains(pan);
        Grain {
            on: true,
            kind: if hp { GK::NoiseHp } else { GK::Noise },
            delay: (delay * sr) as u32,
            env: 1.0,
            dec: Self::decay_for(sr, secs),
            att: 1.0,
            amp: amp * if hp { 1.0 } else { (1.0 / q.max(0.3)).sqrt() * 2.0 },
            gl,
            gr,
            svf: Svf::new(fc, q, sr),
            ..Default::default()
        }
    }

    fn ping(sr: f32, f: f32, secs: f32, amp: f32, pan: f32, chirp_per_s: f32, delay: f32) -> Grain {
        let (gl, gr) = pan_gains(pan);
        let w = 2.0 * PI * f.min(0.45 * sr) / sr;
        Grain {
            on: true,
            kind: GK::Ping,
            delay: (delay * sr) as u32,
            env: 1.0,
            dec: Self::decay_for(sr, secs),
            att: 0.0,
            amp,
            gl,
            gr,
            re: 1.0,
            im: 0.0,
            w,
            chirp: chirp_per_s.powf(32.0 / sr),
            c: w.cos(),
            s: w.sin(),
            ..Default::default()
        }
    }

    /// power-law (P(A) ~ A^-2) amplitude around a median, capped at 20x
    fn powerlaw(&mut self, median: f32) -> f32 {
        let u = self.rng.u().max(0.025);
        0.5 * median / u
    }

    fn trigger(&mut self, e: Ev) {
        let m = self.mix;
        match e {
            Ev::Shock { dp, pan, size, .. } => {
                // the blast wave itself: a Friedlander pulse with an instantaneous front (the crack) and a positive
                // phase of physical duration T+ (~W^1/3 scaling; longer for nearby detonations), then the
                // negative phase; the solver's own waveform keeps arriving through the microphone layers
                let a = self.ev_amp(dp) * m.microphone;
                let tp = ((0.0008 + 0.0025 * size.min(1.0)) * (dp / 10_000.0).max(0.05).powf(0.2)).clamp(0.0004, 0.008);
                let (gl, gr) = pan_gains(pan);
                self.spawn(Grain { on: true, kind: GK::Friedlander, env: 1.0, dec: (-1.0 / (tp * self.sr)).exp(), att: 0.0, amp: a, gl, gr, d2: 1.0 / (tp * self.sr), ..Default::default() });
                let big = ((dp / 3000.0).max(0.0).powf(0.25) + size).min(2.5);
                if dp > 10000.0 {
                    if let Some(r) = self.rumbles.iter_mut().find(|r| !r.on) {
                        let (gl, gr) = pan_gains(pan * 0.5);
                        // reverberant / ground-reflected low-frequency tail, exponential decay
                        let secs = 0.6 + 0.8 * big;
                        *r = Rumble {
                            on: true,
                            env: 0.0,
                            dec: (-1.0 / (secs * self.sr)).exp(),
                            amp: a * 0.35,
                            f: 300.0,
                            fdec: (0.25f32).powf(1.0 / (secs * self.sr)),
                            lp: Svf::new(300.0, 0.7, self.sr),
                            lp2: Svf::new(300.0, 0.7, self.sr),
                            gl,
                            gr,
                            cnt: 0,
                        };
                    }
                }
            }
            Ev::Break { amount, f1, q, pan, r } => {
                let p = 3.0 * amount.sqrt() / r; // Pa: a fracturing cm-section member at 1 m ~ 3 Pa per cell
                let a = self.ev_amp(p) * m.structure;
                let g = Self::noise_grain(self.sr, 3000.0 + 2000.0 * self.rng.u(), 0.8, 0.012, a * 1.2, pan, false, 0.0);
                self.spawn(g);
                // ring the band's structure hard
                let k = self.band_of_pan(pan);
                self.bands[k].strike += a * 0.8;
                self.bands[k].mode_f = f1;
                self.bands[k].mode_q = q;
                // debris pings
                let nd = (2.0 + amount.sqrt() * 2.0).min(8.0) as usize;
                for _ in 0..nd {
                    let f = f1 * (3.0 + 9.0 * self.rng.u());
                    let d = 0.05 + 0.5 * self.rng.u();
                    let amp = a * (0.15 + 0.35 * self.rng.u());
                    let pp = (pan + 0.4 * (self.rng.u() - 0.5)).clamp(-1.0, 1.0);
                    let g = Self::ping(self.sr, f, (q / (PI * f)).clamp(0.02, 0.6), amp, pp, 1.0, d);
                    self.spawn(g);
                }
            }
        }
    }

    fn band_of_pan(&self, pan: f32) -> usize {
        let mut best = 0;
        let mut bd = f32::MAX;
        for (k, b) in self.bands.iter().enumerate() {
            let d = (b.ctl.creak.pan - pan).abs();
            if d < bd {
                bd = d;
                best = k;
            }
        }
        best
    }

    fn control_block(&mut self) {
        let sr = self.sr;
        let n = BLOCK;
        let a = 1.0 - (-(n as f32) / (0.04 * sr)).exp();
        let afade = 1.0 - (-(n as f32) / (0.25 * sr)).exp();
        self.fade += (self.fade_t - self.fade) * afade;
        self.master += (self.master_target - self.master) * a;
        let m = self.mix;
        let bt = n as f32 / sr;
        let gc = self.g_cont;
        {
            let (mut num, mut den) = (0.0f32, 0.0f32);
            for b in &self.bands {
                num += b.lss_asym_t;
                den += b.ctl.lss.p;
            }
            let t = if den > 1e-6 { num / den } else { 0.0 };
            self.lss_asym += (t - self.lss_asym) * a;
        }
        for k in 0..NB {
            let c = self.bands[k].ctl;
            let fade = self.fade;
            {
                let b = &mut self.bands[k];
                // flame flicker: smooth random modulation at a few Hz (physical-time texture)
                b.flick_cnt += n as u32;
                if b.flick_cnt as f32 > sr / 6.0 {
                    b.flick_cnt = 0;
                    b.flick_t = 0.7 + 0.6 * self.rng.u();
                }
                b.flick += (b.flick_t - b.flick) * 0.08;
                let fl = b.flick;
                let g = gc;
                // hotter flames burn brighter: the roll-off above the peak extends (no separate band)
                let lp1 = 3.0 + 3.0 * c.comb_bright;
                if (b.comb.sh.lp1 - lp1).abs() > 0.05 {
                    b.comb.sh.lp1 = lp1;
                    b.comb.retune(sr);
                }
                b.comb.target(c.comb.p * g * fade * m.combustion * fl, c.comb.f, c.comb.pan);
                b.fss.target(c.fss.p * g * fade * m.jet, c.fss.f, c.fss.pan);
                b.lss.target(c.lss.p * g * fade * m.jet, c.lss.f, c.lss.pan);
                b.lss_asym_t = c.lss_asym * c.lss.p;
                b.rumble.target(c.rumble.p * g * fade * m.jet, c.rumble.f, c.rumble.pan);
                b.turb.target(c.turb.p * g * fade * m.jet, c.turb.f, c.turb.pan);
                b.hiss.target(c.hiss.p * g * fade * m.machinery, c.hiss.f, c.hiss.pan);
                b.fan_bb.target(c.fan.p * g * 0.7 * fade * m.machinery, 2.0 * c.fan.f.max(15.0), c.fan.pan);
                b.fan_amp.t = c.fan.p * g * 0.7 * fade * m.machinery;
                b.fan_ft = c.fan.f.max(15.0);
                b.fan_pan = c.fan.pan;
                for l in [&mut b.comb, &mut b.fss, &mut b.lss, &mut b.rumble, &mut b.turb, &mut b.hiss, &mut b.fan_bb] {
                    l.block(a, n, sr);
                }
                b.fan_amp.block(a, n);
                b.fan_f += (b.fan_ft - b.fan_f) * a;
                // modal structure follows the stressed material
                if c.creak_f1 > 0.0 && ((c.creak_f1 / b.mode_f - 1.0).abs() > 0.01 || (c.creak_q / b.mode_q - 1.0).abs() > 0.05) {
                    b.mode_f = c.creak_f1;
                    b.mode_q = c.creak_q;
                }
                for (j, md) in b.modes.iter_mut().enumerate() {
                    md.set(b.mode_f * MODE_RATIOS[j], b.mode_q, sr);
                }
                b.mode_pan = c.creak.pan;
                // air absorption: one-pole low-pass at the -3 dB point of alpha(f) r (alpha ~ 1.5e-9 f^2 dB/m)
                let r = c.comb.r.max(c.fss.r).max(0.3);
                let fc = (44_700.0 / r.sqrt()).min(0.45 * sr);
                b.air_c = 1.0 - (-2.0 * PI * fc / sr).exp();
            }
            // Poisson grain processes (expected count this block = rate * bt)
            let procs: [(Rate, u8); 7] = [(c.wood, 0), (c.prop, 1), (c.mburn, 2), (c.boil, 3), (c.fizz, 4), (c.jet_crackle, 5), (c.creak, 6)];
            for (r, kind) in procs {
                if r.rate <= 0.0 || r.amp <= 0.0 || fade < 0.05 {
                    continue;
                }
                let lam = r.rate * bt;
                // Poisson-distributed count (inverse CDF)
                let mut cnt = 0;
                let u = self.rng.u();
                let mut p = (-lam).exp();
                let mut cum = p;
                while u > cum && cnt < 32 {
                    cnt += 1;
                    p *= lam / cnt as f32;
                    cum += p;
                }
                for _ in 0..cnt {
                    let delay = self.rng.u() * bt;
                    // pops / crackle: power-law sizes up to 20x the median (jet shocklets: CRACKLE_CAP)
                    let amp_pa = self.powerlaw(r.amp).min(r.amp * if kind == 5 { CRACKLE_CAP } else { 40.0 });
                    let pan = (r.pan + 0.25 * (self.rng.u() - 0.5)).clamp(-1.0, 1.0);
                    match kind {
                        0 | 2 => {
                            // wood / matter pop: gas-pocket rupture, 0.3-4 ms band-limited burst; big ones add a "thock"
                            let a = self.ev_amp(amp_pa) * m.crackle * fade;
                            let fc = 900.0 * (6.0f32).powf(self.rng.u());
                            let g = Self::noise_grain(self.sr, fc, 1.2 + self.rng.u(), 0.0004 + 0.003 * self.rng.u(), a, pan, false, delay);
                            self.spawn(g);
                            if amp_pa > 4.0 * r.amp {
                                let g = Self::ping(self.sr, 250.0 + 450.0 * self.rng.u(), 0.006 + 0.012 * self.rng.u(), a * 0.6, pan, 0.8, delay);
                                self.spawn(g);
                            }
                        }
                        1 => {
                            let a = self.ev_amp(amp_pa) * m.crackle * fade;
                            let g = Self::noise_grain(self.sr, 2000.0 + 5000.0 * self.rng.u(), 0.8, 0.0003 + 0.001 * self.rng.u(), a, pan, false, delay);
                            self.spawn(g);
                        }
                        3 | 4 => {
                            // Minnaert bubble: f = 3.26 / R, R 0.5-2 mm (boiling) or 1-4 mm (gas release); rises in pitch
                            let rad = if kind == 3 { 0.0005 * (4.0f32).powf(self.rng.u()) } else { 0.001 * (4.0f32).powf(self.rng.u()) };
                            let f = 3.26 / rad;
                            let a = self.ev_amp(amp_pa) * m.crackle * fade;
                            let g = Self::ping(self.sr, f, 10.0 / (PI * f), a, pan, 1.5, delay);
                            self.spawn(g);
                            if kind == 3 && self.rng.u() < 0.5 {
                                // bubble burst at the surface: a faint tick (boiling noise spans ~1-6 kHz)
                                let g = Self::noise_grain(self.sr, 4000.0 + 3000.0 * self.rng.u(), 0.7, 0.0002, a * 0.2, pan, true, delay);
                                self.spawn(g);
                            }
                        }
                        5 => {
                            // supersonic jet crackle: steepened shocklets (sharp compression, slower relaxation)
                            let a = self.ev_amp(amp_pa) * m.jet * fade;
                            let tau = CRACKLE_TAU0 + CRACKLE_TAU1 * self.rng.u();
                            // the rarefaction that follows recovers on the time scale of the large-scale waves the
                            // shocklet rides on (zero mean overall), so it stays under the jet's own hump
                            let t2 = (0.6 / (2.0 * PI * c.lss.f.max(30.0))).max(3.0 * tau);
                            let (gl, gr) = pan_gains(pan);
                            self.spawn(Grain {
                                on: true,
                                kind: GK::Shocklet,
                                delay: (delay * self.sr) as u32,
                                env: 1.0,
                                dec: Self::decay_for(self.sr, tau),
                                att: 0.0,
                                amp: a,
                                gl,
                                gr,
                                e2: tau / t2,
                                d2: Self::decay_for(self.sr, t2),
                                ..Default::default()
                            });
                        }
                        _ => {
                            // stick-slip creak: an impulse train into the band's modes
                            let sa = self.ev_amp(amp_pa) * m.structure * fade;
                            let b = &mut self.bands[k];
                            if b.stick_left == 0 {
                                b.stick_left = 4 + (self.rng.u() * 30.0) as u32;
                                let rate = 25.0 + 120.0 * self.rng.u();
                                b.stick_period = sr / rate;
                                b.stick_cnt = delay * sr;
                                b.stick_amp = sa;
                            }
                        }
                    }
                }
            }
        }
    }

    /// Next stereo sample.
    pub fn next(&mut self) -> (f32, f32) {
        self.next_with((0.0, 0.0))
    }

    /// Next stereo sample with `extra` (another source, e.g. the fallback engine of audio.rs) mixed in before the
    /// master gain and the shared output bus.
    pub fn next_with(&mut self, extra: (f32, f32)) -> (f32, f32) {
        if self.block_left == 0 {
            self.control_block();
            self.block_left = BLOCK;
        }
        self.block_left -= 1;
        let sr = self.sr;
        let mut l = 0.0f32;
        let mut r = 0.0f32;

        // ---- bands
        let (mut lss_l, mut lss_r) = (0.0f32, 0.0f32);
        for k in 0..NB {
            let w0 = self.rng.w();
            let w1 = self.rng.w();
            let w2 = self.rng.w();
            let w3 = self.rng.w();
            let b = &mut self.bands[k];
            let (mut bl, mut br) = (0.0f32, 0.0f32);
            macro_rules! lay {
                ($x:expr, $w:expr) => {
                    if $x.active() {
                        let (a, c) = $x.tick($w);
                        bl += a;
                        br += c;
                    }
                };
            }
            lay!(b.comb, w0);
            lay!(b.fss, w1);
            if b.lss.active() {
                let (a, c) = b.lss.tick(w2);
                lss_l += a;
                lss_r += c;
            }
            lay!(b.rumble, w3);
            lay!(b.turb, w3 * 0.7 + w0 * 0.714);
            lay!(b.hiss, w2 * 0.6 + w1 * 0.8);
            lay!(b.fan_bb, w0 * 0.8 - w2 * 0.6);
            let fa = b.fan_amp.tick();
            if fa > 1e-6 || b.fan_amp.t > 1e-6 {
                let mut tone = 0.0;
                for (h, ph) in b.fan_ph.iter_mut().enumerate() {
                    *ph += 2.0 * PI * b.fan_f * (h + 1) as f32 / sr;
                    if *ph > 2.0 * PI {
                        *ph -= 2.0 * PI;
                    }
                    tone += ph.sin() * [1.0, 0.45, 0.2][h];
                }
                let (gl, gr) = pan_gains(b.fan_pan);
                let t = tone * fa * 1.2;
                bl += t * gl;
                br += t * gr;
            }
            // structure modes (stick-slip impulses, fracture strikes)
            let mut x = b.strike;
            b.strike = 0.0;
            if b.stick_left > 0 {
                b.stick_cnt -= 1.0;
                if b.stick_cnt <= 0.0 {
                    x = b.stick_amp * (0.5 + 0.5 * w2.abs().min(1.0));
                    b.stick_left -= 1;
                    b.stick_cnt += b.stick_period * (0.85 + 0.3 * (w3 * 0.3 + 0.5).clamp(0.0, 1.0));
                }
            }
            if b.modes[0].y1.abs() > 1e-7 || x != 0.0 {
                let mut y = 0.0;
                for (j, md) in b.modes.iter_mut().enumerate() {
                    y += md.tick(x * MODE_GAINS[j]) * MODE_GAINS[j];
                }
                let (gl, gr) = pan_gains(b.mode_pan);
                bl += y * gl;
                br += y * gr;
            }
            b.air[0] += (bl - b.air[0]) * b.air_c;
            b.air[1] += (br - b.air[1]) * b.air_c;
            l += b.air[0];
            r += b.air[1];
        }

        // ---- the large-scale jet field: one waveform at each ear, made asymmetric (crackle) as a whole
        if self.lss_asym > 0.0 {
            let x = [lss_l, lss_r];
            let mut y = [0.0f32; 2];
            for c in 0..2 {
                self.lss_ms[c] += (x[c] * x[c] - self.lss_ms[c]) * self.lss_ms_c;
                let sd = self.lss_ms[c].sqrt().max(1e-6);
                let u = (x[c] / sd).clamp(-6.0, 6.0);
                y[c] = sd * (u + self.lss_asym * (u * u - 1.0));
            }
            l += y[0];
            r += y[1];
        } else {
            l += lss_l;
            r += lss_r;
        }

        // ---- grains
        for g in self.grains.iter_mut() {
            if !g.on {
                continue;
            }
            if g.delay > 0 {
                g.delay -= 1;
                continue;
            }
            let v = match g.kind {
                GK::Noise => {
                    let (_, bp, _) = g.svf.tick(self.rng.w());
                    bp
                }
                GK::NoiseHp => {
                    let (_, _, hp) = g.svf.tick(self.rng.w());
                    hp
                }
                GK::Ping => {
                    let (re, im) = (g.re * g.c - g.im * g.s, g.re * g.s + g.im * g.c);
                    g.re = re;
                    g.im = im;
                    g.cnt += 1;
                    if g.cnt >= 32 {
                        g.cnt = 0;
                        let n = (g.re * g.re + g.im * g.im).sqrt().max(1e-6);
                        g.re /= n;
                        g.im /= n;
                        if g.chirp != 1.0 {
                            g.w = (g.w * g.chirp).min(PI * 0.9);
                            g.c = g.w.cos();
                            g.s = g.w.sin();
                        }
                    }
                    // 1 ms attack avoids clicks
                    g.att = (g.att + 1.0 / (0.001 * sr)).min(1.0);
                    im * g.att
                }
                GK::Shocklet => {
                    // 2-sample front (a physical shock rise, band-limited by air absorption over ~1 m)
                    g.att = (g.att + 0.5).min(1.0);
                    let v = g.att * (g.env - g.e2) * g.amp;
                    g.env *= g.dec;
                    g.e2 *= g.d2;
                    l += v * g.gl;
                    r += v * g.gr;
                    if g.env < 1e-4 && g.e2 < 1e-4 {
                        g.on = false;
                    }
                    continue;
                }
                GK::Friedlander => {
                    g.att = (g.att + 0.5).min(1.0);
                    let x = g.cnt as f32 * g.d2;
                    g.cnt += 1;
                    let v = g.att * (1.0 - x) * g.env * g.amp;
                    g.env *= g.dec;
                    l += v * g.gl;
                    r += v * g.gr;
                    if x > 14.0 {
                        g.on = false;
                    }
                    continue;
                }
            };
            let s = v * g.env * g.amp;
            l += s * g.gl;
            r += s * g.gr;
            g.env *= g.dec;
            if g.env < 1e-4 {
                g.on = false;
            }
        }
        for rm in self.rumbles.iter_mut() {
            if !rm.on {
                continue;
            }
            rm.cnt += 1;
            if rm.cnt & 0xFFFF >= 64 {
                rm.cnt &= 0x8000_0000;
                rm.f = (rm.f * rm.fdec.powi(64)).max(50.0);
                rm.lp.set(rm.f, 0.7, sr);
                rm.lp2.set(rm.f, 0.7, sr);
            }
            let w = self.rng.w();
            let (a, _, _) = rm.lp.tick(w);
            let (b, _, _) = rm.lp2.tick(a);
            // unit rms of the 4-pole low-pass at f (ENBW ~ 1.1 f)
            let s = b * rm.env * rm.amp * (sr / (2.2 * rm.f)).sqrt();
            l += s * rm.gl;
            r += s * rm.gr;
            if rm.cnt & 0x8000_0000 == 0 {
                // 30 ms build-up (the tail grows out of the blast), then exponential decay
                rm.env += 1.0 / (0.03 * sr);
                if rm.env >= 1.0 {
                    rm.env = 1.0;
                    rm.cnt |= 0x8000_0000;
                }
            } else {
                rm.env *= rm.dec;
            }
            if rm.env < 1e-4 && rm.cnt & 0x8000_0000 != 0 {
                rm.on = false;
            }
        }

        // ---- microphone (stretched solver pressure at the ears)
        let (ml, mr) = self.mic_tick();
        l += ml;
        r += mr;

        // ---- output: extra (fallback engine), DC block, master, then the shared bus (reverb, limiter, clipper)
        let x = [l + extra.0, r + extra.1];
        let mut y = [0.0f32; 2];
        for c in 0..2 {
            let v = x[c] - self.dc[2 * c] + 0.9995 * self.dc[2 * c + 1];
            self.dc[2 * c] = x[c];
            self.dc[2 * c + 1] = v;
            y[c] = v * self.master;
        }
        self.bus.process(y)
    }

    fn mic_tick(&mut self) -> (f32, f32) {
        // advance through the FIFO of mic samples (each lasts `dur` audio samples); mild rate correction keeps
        // the latency near one frame
        let target = self.frame_samples * 1.25;
        let occ: f32 = self.fifo.iter().map(|e| e.dur).sum::<f32>() + self.cur.as_ref().map(|e| e.dur - self.mic_pos).unwrap_or(0.0);
        let speed = (1.0 + 0.15 * (occ / target.max(1.0) - 1.0)).clamp(0.85, 1.2);
        self.mic_pos += speed;
        let need_new = match &self.cur {
            Some(e) => self.mic_pos >= e.dur,
            None => true,
        };
        if need_new {
            if let Some(e) = self.fifo.pop_front() {
                if let Some(c) = &self.cur {
                    self.mic_pos -= c.dur;
                } else {
                    self.mic_pos = 0.0;
                }
                self.mic_pos = self.mic_pos.clamp(0.0, e.dur);
                if let Some(ev) = e.ev {
                    self.trigger(ev);
                }
                self.cur = Some(e);
            } else if self.cur.is_some() {
                // underflow: hold, decaying
                self.mic_pos = self.cur.as_ref().map(|e| e.dur).unwrap_or(0.0);
                for v in self.mic_last.iter_mut() {
                    *v *= 0.9995;
                }
            }
        }
        if let Some(e) = &self.cur {
            if !need_new || !self.fifo.is_empty() || self.mic_pos < e.dur {
                // interpolate towards the next entry
                let t = (self.mic_pos / e.dur.max(1e-3)).clamp(0.0, 1.0);
                let nx = self.fifo.front().unwrap_or(e);
                let lerp = |a: f32, b: f32| a + (b - a) * t;
                self.mic_last = [lerp(e.a_body, nx.a_body), lerp(e.a_broad, nx.a_broad), lerp(e.a_wind, nx.a_wind), lerp(e.s.pan, nx.s.pan), lerp(e.s.p, nx.s.p), lerp(e.lfe_g, nx.lfe_g)];
            }
        }
        let m = self.mix;
        let fade = self.fade.max(0.0);
        let [ab, abr, aw, pan, p, lg] = self.mic_last;
        self.body.amp.v = ab * m.microphone * fade;
        self.body.amp.step = 0.0;
        self.body.pan.v = pan;
        self.broad.amp.v = abr * m.microphone * fade;
        self.broad.amp.step = 0.0;
        self.broad.pan.v = pan;
        self.wind.amp.v = aw * m.microphone * fade;
        self.wind.amp.step = 0.0;
        self.wind.pan.v = 0.0;
        let mut l = 0.0;
        let mut r = 0.0;
        for lay in [&mut self.body, &mut self.broad, &mut self.wind] {
            if lay.amp.v > 1e-6 {
                let (a, b) = lay.tick(self.rng.w());
                l += a;
                r += b;
            }
        }
        // infrasonic LFE: the solver's own waveform (game time), band-limited to < 40 Hz
        let (x, _, _) = self.lfe_lp.tick(p * lg * m.lfe * fade);
        let (x, _, _) = self.lfe_lp2.tick(x);
        // band-pass 15-40 Hz: the game-time drift below 15 Hz would only eat limiter headroom
        let (_, _, x) = self.lfe_hp.tick(x);
        (l + x, r + x)
    }

    /// Render `frames` stereo frames, appended interleaved to `out`.
    pub fn render(&mut self, frames: usize, out: &mut Vec<f32>) {
        out.reserve(frames * 2);
        for _ in 0..frames {
            let (l, r) = self.next();
            out.push(l);
            out.push(r);
        }
    }
}

// Live output (device, mixer, reverb, limiter) lives in audio.rs: `audio::Audio::feed(GpuAcoustics::take_frames())`.

/// 16-bit stereo WAV writer (for offline renders).
pub fn write_wav(path: &str, data: &[f32], sr: u32) -> std::io::Result<()> {
    use std::io::Write;
    let mut f = std::io::BufWriter::new(std::fs::File::create(path)?);
    let n = (data.len() * 2) as u32;
    f.write_all(b"RIFF")?;
    f.write_all(&(36 + n).to_le_bytes())?;
    f.write_all(b"WAVEfmt ")?;
    f.write_all(&16u32.to_le_bytes())?;
    f.write_all(&1u16.to_le_bytes())?;
    f.write_all(&2u16.to_le_bytes())?;
    f.write_all(&sr.to_le_bytes())?;
    f.write_all(&(sr * 4).to_le_bytes())?;
    f.write_all(&4u16.to_le_bytes())?;
    f.write_all(&16u16.to_le_bytes())?;
    f.write_all(b"data")?;
    f.write_all(&n.to_le_bytes())?;
    for &v in data {
        f.write_all(&((v.clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes())?;
    }
    Ok(())
}
