//! Ignis audio: ONE system. This module owns the output device (cpal stream), the mixer, the room reverb and the
//! master limiter (`Bus`); the sound itself is computed by `acoustics.rs` from the solver's own fields
//! (virtual microphone + source models), see that module's header. API for the app:
//!
//!   let mut audio = Audio::new();                         // Option<Audio>; None = no device
//!   let link = acoustics::Link::new(device, u0, meta, wall, W, H);  // once, after GpuSim::new (GpuSim::buffers())
//!   //   gpu.rs calls link.pre / link.step / link.post through three hooks (see ARCHITECTURE.md "Acoustics integration")
//!   // every app frame, before the paint callback:
//!   link.set_frame(acoustics::Listener { pos: camera_centre_cells, cam, speed, real_dt, sim_time, paused, fan_pow });
//!   // every app frame, after the previous frame was submitted:
//!   audio.feed(link.take_frames());                       // acoustic features read back from the GPU (~2 frames late)
//!   audio.set_volume(v); audio.set_muted(m);              // master (settings); link.set_enabled(!m) saves the GPU work
//!   // optional fallback while the GPU features are unavailable (headless, old GPU path): the stats-driven synth
//!   audio.update(&AudioSnapshot { .. });                  // is crossfaded in automatically when `feed` is idle
//!
//! The listener is the camera centre.
//!
//! Fallback (legacy) engine: procedural audio synthesised in real time on the cpal audio
//! thread from a handful of parameters derived from simulation state. No samples.
//!
//! Main thread: `Audio::update(&AudioSnapshot)` once per frame. It maps physical
//! quantities to synth targets, detects events (pressure spikes, sudden wall loss)
//! and hands both to the audio thread through a mutex the callback only `try_lock`s
//! (never blocks the audio thread). The audio thread smooths every parameter per
//! sample (no zipper noise / clicks), mixes the layers, adds a small room reverb,
//! and runs a peak limiter + soft clipper on the output.
//!
//! Layers: fire (roar body + flicker + crackle/pop impulses), rocket/jet roar
//! (rumble + mid roar + broadband hiss + supersonic crackle), explosions (crack,
//! thump, swept body, rolling rumble tail), debris pings (modal metal resonators),
//! gas-inlet hiss, boiling sizzle, structural creaks (stick-slip into metal modes).
#![allow(dead_code)]

// The acoustics engine is a child module so that main.rs needs no extra `mod` line: `crate::audio::acoustics::*`.
#[path = "acoustics.rs"]
pub mod acoustics;
use acoustics::{Analyzer, Ctl, FeatureFrame, Mix, Synth};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::f32::consts::PI;
use std::sync::{mpsc, Arc, Mutex};

/// Per-frame inputs. All fields are plain values the game already has.
/// Units are physical (convert from gpu::Stats with the app's constants).
#[derive(Clone, Copy, Debug)]
pub struct AudioSnapshot {
    /// Real seconds since last frame.
    pub dt: f32,
    pub paused: bool,
    /// Simulation speed multiplier (1.0 = normal).
    pub sim_speed: f32,
    /// Peak gas pressure, atm (ambient ~1).
    pub max_p_atm: f32,
    /// Peak gas temperature, K.
    pub max_t_k: f32,
    /// Fuel burn rate, kg/s.
    pub burn_kgs: f32,
    /// Net thrust magnitude, kN.
    pub thrust_kn: f32,
    /// Intact wall cells (drops = destruction).
    pub walls: u32,
    /// Worst structural stress 0..1 (1 = failing).
    pub max_stress: f32,
    /// Hottest wall 0..1 of melting.
    pub max_wall_heat: f32,
    /// Gas inlet activity 0..1 (e.g. how far feed pressures are above ambient).
    pub inlet: f32,
    /// Boiling/sizzle intensity 0..1 (optional hook; 0 if unknown).
    pub boil: f32,
    /// Extra explosion impulse to fire this frame (0 = none, 1 = big). Optional hook.
    pub explosion: f32,
    /// True on the frame a scene was loaded/cleared (resets event baselines).
    pub scene_reset: bool,
}

impl Default for AudioSnapshot {
    fn default() -> Self {
        Self {
            dt: 1.0 / 60.0,
            paused: false,
            sim_speed: 1.0,
            max_p_atm: 1.0,
            max_t_k: 300.0,
            burn_kgs: 0.0,
            thrust_kn: 0.0,
            walls: 0,
            max_stress: 0.0,
            max_wall_heat: 0.0,
            inlet: 0.0,
            boil: 0.0,
            explosion: 0.0,
            scene_reset: false,
        }
    }
}

/// Material class whose sound signature the stats-driven engine uses (crackle, hiss, chuffing, boom character).
/// Pick with `Audio::set_signature` (the app can derive it from the dominant burning element / scene).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Signature {
    /// wood / generic hydrocarbon fire (campfire)
    #[default]
    Generic,
    /// black powder / gunpowder: fizz with sparse spits, then a dull whoomph
    BlackPowder,
    /// magnesium / flash powders: bright sizzle, very dense high-frequency ticks, hard bright flash
    Magnesium,
    /// thermite: steady low hiss and roar with rare spatter pops, no boom
    Thermite,
    /// TNT / high explosive: almost silent until a single very hard crack (short, bright, very loud)
    Tnt,
    /// cast propellant grain burn: steady roar, chuffing at low chamber pressure
    GrainBurn,
}

impl Signature {
    pub const ALL: [Signature; 6] = [Signature::Generic, Signature::BlackPowder, Signature::Magnesium, Signature::Thermite, Signature::Tnt, Signature::GrainBurn];
    pub fn name(self) -> &'static str {
        match self {
            Signature::Generic => "wood fire",
            Signature::BlackPowder => "black powder",
            Signature::Magnesium => "magnesium",
            Signature::Thermite => "thermite",
            Signature::Tnt => "tnt",
            Signature::GrainBurn => "grain burn",
        }
    }
    fn params(self) -> SigParams {
        let g = SigParams::default();
        match self {
            Signature::Generic => g,
            // fizz: many short mid ticks, sparse spits; a little hiss
            Signature::BlackPowder => SigParams { rate: 2.2, fc_lo: 1800.0, fc_hi: 4500.0, decay: 0.7, hiss: 0.5, pop: 0.3, low: 0.5, crack: 0.9, boom_body: 1.3, ..g },
            // sizzle: very dense bright ticks, strong high hiss, almost no low body
            Signature::Magnesium => SigParams { rate: 5.0, fc_lo: 4500.0, fc_hi: 9000.0, decay: 0.45, hiss: 1.6, pop: 0.05, low: 0.2, crack: 1.5, boom_body: 0.6, ..g },
            // thermite: steady hiss + low roar, sparse spatter
            Signature::Thermite => SigParams { rate: 0.5, fc_lo: 2500.0, fc_hi: 7000.0, decay: 1.2, hiss: 1.2, pop: 0.2, low: 1.3, crack: 0.5, boom_body: 0.5, ..g },
            // TNT: nothing but the crack when it goes
            Signature::Tnt => SigParams { rate: 0.1, fc_lo: 3000.0, fc_hi: 6000.0, decay: 0.5, hiss: 0.0, pop: 0.0, low: 0.2, crack: 2.4, boom_body: 0.7, ..g },
            // grain: steady roar, chuffing when the chamber pressure is low
            Signature::GrainBurn => SigParams { rate: 0.8, fc_lo: 1200.0, fc_hi: 3500.0, decay: 1.0, hiss: 0.7, pop: 0.1, low: 1.5, crack: 0.8, chuff_hz: 11.0, chuff_depth: 0.85, ..g },
        }
    }
}

/// Numerical form of a `Signature` (all multipliers; `SigParams::default()` = wood fire).
#[derive(Clone, Copy)]
struct SigParams {
    /// crackle rate multiplier
    rate: f32,
    /// crackle centre frequency range at full heat, Hz
    fc_lo: f32,
    fc_hi: f32,
    /// crackle decay multiplier
    decay: f32,
    /// steady high-frequency hiss level (x burn)
    hiss: f32,
    /// deep pop rate multiplier
    pop: f32,
    /// low-frequency body multiplier
    low: f32,
    /// initial blast crack multiplier
    crack: f32,
    /// blast body multiplier
    boom_body: f32,
    /// chuffing frequency (0 = none) and depth at low chamber pressure
    chuff_hz: f32,
    chuff_depth: f32,
}

impl Default for SigParams {
    fn default() -> Self {
        Self { rate: 1.0, fc_lo: 1200.0, fc_hi: 6700.0, decay: 1.0, hiss: 0.0, pop: 1.0, low: 1.0, crack: 1.0, boom_body: 1.0, chuff_hz: 0.0, chuff_depth: 0.0 }
    }
}

#[derive(Clone, Copy, Default)]
struct Targets {
    /// 0..1: how close the chamber is to ambient (1 at <= 2 atm, 0 above 6 atm): drives chuffing
    lowp: f32,
    fire: f32,
    heat: f32,
    roar: f32,
    hiss: f32,
    sizzle: f32,
    stress: f32,
    active: f32,
    master: f32,
}

#[derive(Clone, Copy)]
enum Event {
    Boom { strength: f32 },
    Debris { amount: f32 },
}

struct Shared {
    t: Targets,
    events: Vec<Event>,
    sig: SigParams,
}

pub struct Audio {
    _stream: cpal::Stream,
    shared: Arc<Mutex<Shared>>,
    tx: mpsc::SyncSender<Msg>,
    pub sample_rate: u32,
    volume: f32,
    muted: bool,
    det: Detector,
    pub analyzer: Analyzer,
    /// frames fed (diagnostics)
    pub fed: u64,
    sig: Signature,
    /// booms still travelling to the listener: (seconds left, strength)
    pending: Vec<(f32, f32)>,
}

/// Main thread -> audio thread messages of the acoustic path.
enum Msg {
    Ctl(Box<Ctl>),
    Master(f32),
    Mix(Mix),
    Wet(f32),
}

/// Event detection state (main thread).
struct Detector {
    init: bool,
    prev_walls: u32,
    p_slow: f32,
    prev_p: f32,
    cooldown: f32,
    lost_acc: f32,
}

impl Detector {
    fn new() -> Self {
        Self { init: false, prev_walls: 0, p_slow: 1.0, prev_p: 1.0, cooldown: 0.0, lost_acc: 0.0 }
    }

    fn step(&mut self, s: &AudioSnapshot, out: &mut Vec<Event>) {
        let dt = s.dt.clamp(1e-4, 0.1);
        self.cooldown = (self.cooldown - dt).max(0.0);
        if s.explosion > 0.0 {
            out.push(Event::Boom { strength: s.explosion.min(3.0) });
        }
        if !self.init || s.scene_reset {
            self.init = true;
            self.prev_walls = s.walls;
            self.p_slow = s.max_p_atm;
            self.prev_p = s.max_p_atm;
            self.lost_acc = 0.0;
            return;
        }
        if s.paused {
            self.prev_walls = s.walls;
            self.prev_p = s.max_p_atm;
            return;
        }
        // wall destruction: sudden loss -> boom + debris; slow loss (melting) -> occasional pings
        let lost = self.prev_walls.saturating_sub(s.walls) as f32;
        self.prev_walls = s.walls;
        if lost >= 10.0 && self.cooldown <= 0.0 {
            out.push(Event::Boom { strength: (lost.sqrt() / 12.0).clamp(0.25, 3.0) });
            out.push(Event::Debris { amount: (lost.sqrt() / 4.0).clamp(0.5, 6.0) });
            self.cooldown = 0.2;
        } else if lost > 0.0 {
            self.lost_acc += lost;
            if self.lost_acc >= 3.0 {
                out.push(Event::Debris { amount: (self.lost_acc / 8.0).clamp(0.15, 1.0) });
                self.lost_acc = 0.0;
            }
        }
        // pressure spikes relative to a slow baseline
        let p = s.max_p_atm.max(0.0);
        let dp = p - self.prev_p;
        let excess = p - self.p_slow;
        if dp > 1.0 && excess > (0.6 * self.p_slow).max(2.5) && self.cooldown <= 0.0 {
            out.push(Event::Boom { strength: (excess / 12.0).sqrt().clamp(0.2, 3.0) });
            self.cooldown = 0.25;
        }
        self.prev_p = p;
        let a = 1.0 - (-dt / 0.6).exp();
        self.p_slow += (p - self.p_slow) * a;
    }
}

fn targets_from(s: &AudioSnapshot, master: f32) -> Targets {
    let heat = ((s.max_t_k - 500.0) / 2500.0).clamp(0.0, 1.0);
    let fire = (1.0 - (-s.burn_kgs.max(0.0) / 2.5).exp()).max(0.0);
    let fire = (fire * (0.55 + 0.45 * heat.sqrt())).clamp(0.0, 1.0);
    let roar = (1.0 - (-s.thrust_kn.max(0.0) / 12.0).exp()).clamp(0.0, 1.0);
    let active = if s.paused { 0.0 } else { s.sim_speed.clamp(0.0, 1.0).max(0.25) };
    Targets {
        lowp: (1.0 - (s.max_p_atm - 2.0) / 4.0).clamp(0.0, 1.0),
        fire,
        heat,
        roar,
        hiss: s.inlet.clamp(0.0, 1.0),
        sizzle: s.boil.clamp(0.0, 1.0),
        stress: s.max_stress.clamp(0.0, 1.0),
        active,
        master,
    }
}

impl Audio {
    /// Opens the default output device. None if there is no device or it fails.
    pub fn new() -> Option<Audio> {
        let host = cpal::default_host();
        let device = host.default_output_device()?;
        let sup = device.default_output_config().ok()?;
        let fmt = sup.sample_format();
        let config = sup.config();
        let sr = config.sample_rate;
        let ch = config.channels as usize;
        let shared = Arc::new(Mutex::new(Shared { t: Targets { master: 1.0, ..Default::default() }, events: Vec::with_capacity(64), sig: SigParams::default() }));
        let (tx, rx) = mpsc::sync_channel::<Msg>(64);
        let stream = match fmt {
            cpal::SampleFormat::F32 => build::<f32>(&device, config, ch, sr, shared.clone(), rx),
            cpal::SampleFormat::I16 => build::<i16>(&device, config, ch, sr, shared.clone(), rx),
            cpal::SampleFormat::U16 => build::<u16>(&device, config, ch, sr, shared.clone(), rx),
            cpal::SampleFormat::I32 => build::<i32>(&device, config, ch, sr, shared.clone(), rx),
            _ => None,
        }?;
        stream.play().ok()?;
        let mut a = Audio { _stream: stream, shared, tx, sample_rate: sr, volume: 0.8, muted: false, det: Detector::new(), analyzer: Analyzer::new(), fed: 0, sig: Signature::Generic, pending: Vec::new() };
        a.push_master();
        Some(a)
    }

    /// Acoustic path: hand over the frames read back from `GpuAcoustics::take_frames()` (call once per frame; an
    /// empty vector is fine). While frames keep arriving the fallback engine fades out.
    pub fn feed(&mut self, frames: Vec<FeatureFrame>) {
        for ff in frames {
            let ctl = self.analyzer.process(&ff);
            let _ = self.tx.try_send(Msg::Ctl(Box::new(ctl)));
            self.fed += 1;
        }
    }

    /// Fallback path (stats-driven synth). Call once per frame; only audible while `feed` is idle (> 0.6 s).
    pub fn update(&mut self, snap: &AudioSnapshot) {
        let mut ev = Vec::new();
        self.det.step(snap, &mut ev);
        if !snap.paused {
            // sound travels at 347 m/s: only count down in game time
            let dt = snap.dt.clamp(0.0, 0.1) * snap.sim_speed.max(0.0);
            self.pending.retain_mut(|(t, s)| {
                *t -= dt;
                if *t <= 0.0 {
                    ev.push(Event::Boom { strength: *s });
                    false
                } else {
                    true
                }
            });
        }
        let t = targets_from(snap, 1.0);
        if let Ok(mut sh) = self.shared.lock() {
            sh.t = t;
            sh.sig = self.sig.params();
            if sh.events.len() < 64 {
                sh.events.extend(ev);
            }
        }
    }

    /// Material sound signature of the stats-driven engine (crackle / hiss / chuff / blast character).
    pub fn set_signature(&mut self, sig: Signature) {
        self.sig = sig;
    }
    pub fn signature(&self) -> Signature {
        self.sig
    }
    /// Explosion `distance_m` metres from the listener: the report arrives after distance / 347 m/s of game time
    /// and is quieter with distance (inverse law, 1 m reference scaled so 30 m is about 1/3).
    pub fn boom_at(&mut self, strength: f32, distance_m: f32) {
        let d = distance_m.max(0.0);
        let s = (strength / (1.0 + d / 15.0)).clamp(0.05, 3.0);
        if d < 1.0 {
            self.boom(s);
        } else {
            self.pending.push((d / 347.0, s));
        }
    }
    /// Flip the mute (hotkey M). Returns the new state. The app stores it in `Settings::muted`.
    pub fn toggle_muted(&mut self) -> bool {
        self.set_muted(!self.muted);
        self.muted
    }

    /// Fire an explosion directly in the fallback engine (strength ~0.2 small pop .. 3 huge).
    pub fn boom(&mut self, strength: f32) {
        if let Ok(mut sh) = self.shared.lock() {
            sh.events.push(Event::Boom { strength: strength.clamp(0.05, 3.0) });
        }
    }

    /// Relative level of the acoustic layers (combustion, jet, crackle, microphone, structure, machinery, LFE).
    pub fn set_mix(&mut self, m: Mix) {
        let _ = self.tx.try_send(Msg::Mix(m));
    }
    /// Room reverb return (0 = dry; default `BUS_WET`).
    pub fn set_reverb(&mut self, wet: f32) {
        let _ = self.tx.try_send(Msg::Wet(wet.clamp(0.0, 2.0)));
    }
    pub fn set_volume(&mut self, v: f32) {
        self.volume = v.clamp(0.0, 1.5);
        self.push_master();
    }
    pub fn volume(&self) -> f32 {
        self.volume
    }
    pub fn set_muted(&mut self, m: bool) {
        self.muted = m;
        self.push_master();
    }
    pub fn muted(&self) -> bool {
        self.muted
    }
    fn push_master(&mut self) {
        let m = if self.muted { 0.0 } else { self.volume };
        let _ = self.tx.try_send(Msg::Master(m));
    }
}

fn build<T>(device: &cpal::Device, config: cpal::StreamConfig, ch: usize, sr: u32, shared: Arc<Mutex<Shared>>, rx: mpsc::Receiver<Msg>) -> Option<cpal::Stream>
where
    T: cpal::SizedSample + cpal::FromSample<f32>,
{
    let srf = sr as f32;
    let mut eng = Engine::new(srf);
    let mut syn = Synth::new(srf);
    syn.bus.wet = BUS_WET;
    let mut local_ev: Vec<Event> = Vec::with_capacity(64);
    // fallback engine gain: 1 while no acoustic frame arrived for 0.6 s, else faded to 0
    let mut legacy_g = 1.0f32;
    let mut since_ctl: u32 = u32::MAX / 2;
    let k_leg = 1.0 - (-1.0 / (0.25 * srf)).exp();
    let idle_samples = (0.6 * srf) as u32;
    device
        .build_output_stream::<T, _, _>(
            config,
            move |data: &mut [T], _| {
                if let Ok(mut sh) = shared.try_lock() {
                    eng.target = sh.t;
                    eng.sig = sh.sig;
                    local_ev.extend(sh.events.drain(..));
                }
                for e in local_ev.drain(..) {
                    eng.trigger(e);
                }
                while let Ok(m) = rx.try_recv() {
                    match m {
                        Msg::Ctl(c) => {
                            syn.push(*c);
                            since_ctl = 0;
                        }
                        Msg::Master(v) => syn.master_target = v,
                        Msg::Mix(x) => syn.mix = x,
                        Msg::Wet(w) => syn.bus.wet = w,
                    }
                }
                let ch = ch.max(1);
                for frame in data.chunks_mut(ch) {
                    since_ctl = since_ctl.saturating_add(1);
                    let want = if since_ctl > idle_samples { 1.0 } else { 0.0 };
                    legacy_g += (want - legacy_g) * k_leg;
                    let extra = if legacy_g > 1e-4 {
                        let (a, b) = eng.next_dry();
                        (a * legacy_g, b * legacy_g)
                    } else {
                        (0.0, 0.0)
                    };
                    let (l, r) = syn.next_with(extra);
                    for (i, s) in frame.iter_mut().enumerate() {
                        let v = if ch == 1 {
                            0.5 * (l + r)
                        } else {
                            match i {
                                0 => l,
                                1 => r,
                                _ => 0.0,
                            }
                        };
                        *s = T::from_sample(v);
                    }
                }
            },
            |e| eprintln!("audio stream error: {e}"),
            None,
        )
        .ok()
}

// ---------------------------------------------------------------- DSP primitives

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
        (x >> 8) as f32 * (1.0 / 16_777_216.0)
    }
    #[inline]
    fn w(&mut self) -> f32 {
        self.u() * 2.0 - 1.0
    }
}

/// Topology-preserving-transform state variable filter (Simper). Stable under fast modulation.
#[derive(Clone, Copy, Default)]
struct Svf {
    ic1: f32,
    ic2: f32,
}
#[derive(Clone, Copy)]
struct SvfCoef {
    a1: f32,
    a2: f32,
    a3: f32,
    k: f32,
}
impl SvfCoef {
    fn new(fc: f32, q: f32, sr: f32) -> Self {
        let g = (PI * fc.clamp(10.0, sr * 0.45) / sr).tan();
        let k = 1.0 / q.max(0.05);
        let a1 = 1.0 / (1.0 + g * (g + k));
        let a2 = g * a1;
        Self { a1, a2, a3: g * a2, k }
    }
}
impl Svf {
    /// returns (lp, bp_normalised, hp)
    #[inline]
    fn run(&mut self, x: f32, c: &SvfCoef) -> (f32, f32, f32) {
        let v3 = x - self.ic2;
        let v1 = c.a1 * self.ic1 + c.a2 * v3;
        let v2 = self.ic2 + c.a2 * self.ic1 + c.a3 * v3;
        self.ic1 = 2.0 * v1 - self.ic1;
        self.ic2 = 2.0 * v2 - self.ic2;
        (v2, v1 * c.k, x - c.k * v1 - v2)
    }
}

#[derive(Clone, Copy, Default)]
struct Pink {
    b0: f32,
    b1: f32,
    b2: f32,
}
impl Pink {
    #[inline]
    fn run(&mut self, w: f32) -> f32 {
        self.b0 = 0.99765 * self.b0 + w * 0.099_046;
        self.b1 = 0.96300 * self.b1 + w * 0.296_516_4;
        self.b2 = 0.57000 * self.b2 + w * 1.052_691_3;
        (self.b0 + self.b1 + self.b2 + w * 0.1848) * 0.25
    }
}

#[derive(Clone, Copy, Default)]
struct Brown(f32);
impl Brown {
    #[inline]
    fn run(&mut self, w: f32) -> f32 {
        self.0 = (self.0 + 0.02 * w) * (1.0 / 1.02);
        self.0 * 3.5
    }
}

/// Smoothly wandering random value (flicker / turbulence LFO).
#[derive(Clone, Copy)]
struct Wander {
    v: f32,
    target: f32,
    vel: f32,
}
impl Wander {
    fn new() -> Self {
        Self { v: 0.5, target: 0.5, vel: 0.0 }
    }
    /// rate: target changes per second
    #[inline]
    fn run(&mut self, rng: &mut Rng, rate: f32, sr: f32) -> f32 {
        if rng.u() < rate / sr {
            self.target = rng.u();
        }
        // critically-damped-ish second-order glide
        let w = rate * 2.0 / sr;
        self.vel += (self.target - self.v) * w * w * 4.0 - self.vel * w * 2.0;
        self.v += self.vel;
        self.v.clamp(-0.2, 1.2)
    }
}

#[inline]
fn soft(x: f32) -> f32 {
    // tanh approximation, |err| small, monotone
    let x = x.clamp(-3.0, 3.0);
    x * (27.0 + x * x) / (27.0 + 9.0 * x * x)
}

// ---------------------------------------------------------------- voices

/// Short filtered noise burst: crackles, pops, supersonic shock crackle, sizzle pops.
#[derive(Clone, Copy)]
struct Burst {
    on: bool,
    env: f32,
    decay: f32,
    c: SvfCoef,
    f: Svf,
    amp: f32,
    pl: f32,
    pr: f32,
}

/// Damped sinusoid via rotating phasor: metal pings / debris.
#[derive(Clone, Copy)]
struct Ping {
    on: bool,
    re: f32,
    im: f32,
    cr: f32,
    ci: f32,
    amp: f32,
    pl: f32,
    pr: f32,
    delay: u32,
}

struct Boom {
    t: f32,
    s: f32,
    phase: f32,
    body: Svf,
    tail: Svf,
    brown: Brown,
    brown2: Brown,
    roll: Wander,
    pl: f32,
    pr: f32,
}

struct Creak {
    on: bool,
    t: f32,
    dur: f32,
    f0: f32,
    f1: f32,
    ph: f32,
    base: f32,
    amp: f32,
    modes: [Svf; 3],
    pl: f32,
    pr: f32,
}

struct Comb {
    buf: Vec<f32>,
    i: usize,
    store: f32,
}
impl Comb {
    #[inline]
    fn run(&mut self, x: f32, fb: f32, damp: f32) -> f32 {
        let y = self.buf[self.i];
        self.store = y * (1.0 - damp) + self.store * damp;
        self.buf[self.i] = x + self.store * fb;
        self.i = (self.i + 1) % self.buf.len();
        y
    }
}
struct Allpass {
    buf: Vec<f32>,
    i: usize,
}
impl Allpass {
    #[inline]
    fn run(&mut self, x: f32) -> f32 {
        let b = self.buf[self.i];
        self.buf[self.i] = x + b * 0.5;
        self.i = (self.i + 1) % self.buf.len();
        b - x
    }
}

struct Reverb {
    cl: Vec<Comb>,
    cr: Vec<Comb>,
    al: Vec<Allpass>,
    ar: Vec<Allpass>,
}
impl Reverb {
    fn new(sr: f32) -> Self {
        let sc = sr / 44100.0;
        let mk = |n: usize| Comb { buf: vec![0.0; ((n as f32 * sc) as usize).max(8)], i: 0, store: 0.0 };
        let ma = |n: usize| Allpass { buf: vec![0.0; ((n as f32 * sc) as usize).max(8)], i: 0 };
        let combs = [1116usize, 1188, 1277, 1356, 1422, 1491];
        Self {
            cl: combs.iter().map(|&n| mk(n)).collect(),
            cr: combs.iter().map(|&n| mk(n + 23)).collect(),
            al: [556usize, 441, 341].iter().map(|&n| ma(n)).collect(),
            ar: [579usize, 464, 364].iter().map(|&n| ma(n)).collect(),
        }
    }
    #[inline]
    fn run(&mut self, l: f32, r: f32) -> (f32, f32) {
        let x = (l + r) * 0.5 * 0.1;
        let (mut ol, mut or) = (0.0, 0.0);
        for c in self.cl.iter_mut() {
            ol += c.run(x, 0.82, 0.35);
        }
        for c in self.cr.iter_mut() {
            or += c.run(x, 0.82, 0.35);
        }
        for a in self.al.iter_mut() {
            ol = a.run(ol);
        }
        for a in self.ar.iter_mut() {
            or = a.run(or);
        }
        (ol, or)
    }
}

// ---------------------------------------------------------------- engine

struct Engine {
    sr: f32,
    rng: Rng,
    target: Targets,
    cur: Targets,
    // per-parameter smoothing coefficients
    k_fast: f32,
    k_med: f32,
    k_slow: f32,
    // noise state (decorrelated L/R where width matters)
    pink_l: Pink,
    pink_r: Pink,
    pink_m: Pink,
    brown_m: Brown,
    // fire
    flick: Wander,
    flick2: Wander,
    fire_lp: Svf,
    fire_bp_l: Svf,
    fire_bp_r: Svf,
    // roar
    turb: Wander,
    rum: Svf,
    rum2: Svf,
    mid_l: Svf,
    mid_r: Svf,
    hs_l: Svf,
    hs_r: Svf,
    // inlet hiss & sizzle
    in_l: Svf,
    in_r: Svf,
    sz_l: Svf,
    sz_r: Svf,
    sz_env: f32,
    // pools
    bursts: Vec<Burst>,
    pings: Vec<Ping>,
    booms: Vec<Boom>,
    creaks: Vec<Creak>,
    // coefficient cache (updated every CTRL samples)
    ctr: u32,
    c_fire_lp: SvfCoef,
    c_fire_bp: SvfCoef,
    c_rum: SvfCoef,
    c_rum2: SvfCoef,
    c_mid: SvfCoef,
    c_hs: SvfCoef,
    c_in: SvfCoef,
    c_sz: SvfCoef,
    flk: f32,
    trb: f32,
    sig: SigParams,
    chuff_ph: f32,
    chuff: f32,
    hiss_l: Svf,
    hiss_r: Svf,
    c_hiss: SvfCoef,
    // output
    rev: Reverb,
    lim_env: f32,
    lim_g: f32,
    dc_xl: f32,
    dc_yl: f32,
    dc_xr: f32,
    dc_yr: f32,
    master: f32,
}

const CTRL: u32 = 32;

impl Engine {
    fn new(sr: f32) -> Self {
        let k = |tau: f32| 1.0 - (-1.0 / (tau * sr)).exp();
        let z = SvfCoef::new(1000.0, 0.7, sr);
        let burst = Burst { on: false, env: 0.0, decay: 0.0, c: z, f: Svf::default(), amp: 0.0, pl: 0.0, pr: 0.0 };
        let ping = Ping { on: false, re: 0.0, im: 0.0, cr: 0.0, ci: 0.0, amp: 0.0, pl: 0.0, pr: 0.0, delay: 0 };
        Self {
            sr,
            rng: Rng(0x9E37_79B9),
            target: Targets { master: 0.8, ..Default::default() },
            cur: Targets::default(),
            k_fast: k(0.03),
            k_med: k(0.12),
            k_slow: k(0.35),
            pink_l: Pink::default(),
            pink_r: Pink::default(),
            pink_m: Pink::default(),
            brown_m: Brown::default(),
            flick: Wander::new(),
            flick2: Wander::new(),
            fire_lp: Svf::default(),
            fire_bp_l: Svf::default(),
            fire_bp_r: Svf::default(),
            turb: Wander::new(),
            rum: Svf::default(),
            rum2: Svf::default(),
            mid_l: Svf::default(),
            mid_r: Svf::default(),
            hs_l: Svf::default(),
            hs_r: Svf::default(),
            in_l: Svf::default(),
            in_r: Svf::default(),
            sz_l: Svf::default(),
            sz_r: Svf::default(),
            sz_env: 0.0,
            bursts: vec![burst; 48],
            pings: vec![ping; 48],
            booms: Vec::with_capacity(8),
            creaks: Vec::with_capacity(4),
            ctr: 0,
            c_fire_lp: z,
            c_fire_bp: z,
            c_rum: z,
            c_rum2: z,
            c_mid: z,
            c_hs: z,
            c_in: SvfCoef::new(2800.0, 0.6, sr),
            c_sz: SvfCoef::new(6000.0, 1.4, sr),
            flk: 0.5,
            trb: 0.5,
            sig: SigParams::default(),
            chuff_ph: 0.0,
            chuff: 1.0,
            hiss_l: Svf::default(),
            hiss_r: Svf::default(),
            c_hiss: SvfCoef::new(5000.0, 0.6, sr),
            rev: Reverb::new(sr),
            lim_env: 0.0,
            lim_g: 1.0,
            dc_xl: 0.0,
            dc_yl: 0.0,
            dc_xr: 0.0,
            dc_yr: 0.0,
            master: 0.0,
        }
    }

    fn pan(&mut self, spread: f32) -> (f32, f32) {
        let p = 0.5 + (self.rng.u() - 0.5) * spread; // 0..1
        let a = p * PI * 0.5;
        (a.cos(), a.sin())
    }

    fn spawn_burst(&mut self, fc: f32, q: f32, decay_s: f32, amp: f32, spread: f32) {
        let (pl, pr) = self.pan(spread);
        let sr = self.sr;
        if let Some(b) = self.bursts.iter_mut().find(|b| !b.on) {
            b.on = true;
            b.env = 1.0;
            b.decay = (-1.0 / (decay_s * sr)).exp();
            b.c = SvfCoef::new(fc, q, sr);
            b.f = Svf::default();
            b.amp = amp;
            b.pl = pl;
            b.pr = pr;
        }
    }

    fn spawn_ping(&mut self, f: f32, decay_s: f32, amp: f32, delay_s: f32) {
        let (pl, pr) = self.pan(0.9);
        let sr = self.sr;
        if let Some(p) = self.pings.iter_mut().find(|p| !p.on) {
            let w = 2.0 * PI * f.min(sr * 0.45) / sr;
            let r = (-1.0 / (decay_s * sr)).exp();
            p.on = true;
            p.re = 1.0;
            p.im = 0.0;
            p.cr = r * w.cos();
            p.ci = r * w.sin();
            p.amp = amp;
            p.pl = pl;
            p.pr = pr;
            p.delay = (delay_s * sr) as u32;
        }
    }

    fn trigger(&mut self, e: Event) {
        match e {
            Event::Boom { strength } => {
                if self.booms.len() >= 6 {
                    self.booms.remove(0);
                }
                let (pl, pr) = self.pan(0.5);
                self.booms.push(Boom {
                    t: 0.0,
                    s: strength,
                    phase: 0.0,
                    body: Svf::default(),
                    tail: Svf::default(),
                    brown: Brown::default(),
                    brown2: Brown::default(),
                    roll: Wander::new(),
                    pl,
                    pr,
                });
                // shrapnel ticks after big blasts
                if strength > 0.8 {
                    let n = (strength * 5.0) as usize;
                    for _ in 0..n {
                        let f = 900.0 + self.rng.u() * 3500.0;
                        let d = 0.3 + self.rng.u() * 1.2;
                        let a = 0.04 + self.rng.u() * 0.05;
                        let dl = 0.15 + self.rng.u().powi(2) * 1.4;
                        let dc = 0.04 + self.rng.u() * 0.12;
                        self.spawn_ping(f, dc, a, dl * d.min(1.0));
                    }
                }
            }
            Event::Debris { amount } => {
                let n = (3.0 + amount * 6.0) as usize;
                for i in 0..n {
                    // each fragment rings a couple of inharmonic plate modes
                    let base = 350.0 + self.rng.u().powi(2) * 2400.0;
                    let a = (0.05 + 0.12 * self.rng.u().powi(3)) * amount.sqrt().min(1.6);
                    let dl = (i as f32 / n as f32) * (0.25 + 0.5 * amount.min(2.0)) + self.rng.u() * 0.08;
                    let dec = 0.05 + self.rng.u() * 0.25;
                    self.spawn_ping(base, dec, a, dl);
                    let m2 = 2.32 + self.rng.u() * 0.5;
                    self.spawn_ping(base * m2, dec * 0.6, a * 0.5, dl);
                }
            }
        }
    }

    fn spawn_creak(&mut self, strength: f32) {
        if self.creaks.len() >= 3 {
            return;
        }
        let (pl, pr) = self.pan(0.8);
        let u = self.rng.u();
        let f0 = 14.0 + self.rng.u() * 40.0;
        self.creaks.push(Creak {
            on: true,
            t: 0.0,
            dur: 0.35 + u * 1.1,
            f0,
            f1: f0 * (0.6 + self.rng.u() * 1.0),
            ph: 0.0,
            base: 140.0 + self.rng.u() * 260.0,
            amp: 0.18 * strength,
            modes: [Svf::default(); 3],
            pl,
            pr,
        });
    }

    fn control(&mut self) {
        let sr = self.sr;
        let c = self.cur;
        let n = CTRL as f32;
        // fire spectrum: hotter -> brighter, bigger -> lower body
        let flk = self.flk;
        self.c_fire_lp = SvfCoef::new(90.0 + 260.0 * c.heat + 120.0 * flk, 0.8, sr);
        self.c_fire_bp = SvfCoef::new(380.0 + 900.0 * c.heat * (0.7 + 0.6 * flk), 0.9, sr);
        // roar
        let r = c.roar;
        self.c_rum = SvfCoef::new(45.0 + 90.0 * r, 0.9, sr);
        self.c_rum2 = SvfCoef::new(110.0 + 220.0 * r, 0.6, sr);
        self.c_mid = SvfCoef::new(350.0 + 1100.0 * r * (0.85 + 0.3 * self.trb), 0.7, sr);
        self.c_hs = SvfCoef::new(1500.0 + 2500.0 * r, 0.5, sr);

        // stochastic triggers evaluated at control rate
        let dt = n / sr;
        let act = c.active;
        // fire crackles: rate rises steeply with burn rate
        let sg = self.sig;
        let rate = (3.0 + 70.0 * c.fire) * c.fire.sqrt() * act * sg.rate;
        // dense crackle needs several bursts per control tick
        let mut nb = (rate * dt).floor();
        if self.rng.u() < rate * dt - nb {
            nb += 1.0;
        }
        for _ in 0..(nb as u32).min(4) {
            let big = self.rng.u().powi(4);
            let fc = sg.fc_lo + self.rng.u() * (sg.fc_hi - sg.fc_lo) * (0.5 + 0.5 * c.heat);
            let amp = (0.08 + 0.55 * big) * (0.4 + 0.6 * c.fire);
            let (q, d) = (1.2 + self.rng.u() * 3.0, (0.0015 + 0.006 * self.rng.u()) * sg.decay);
            self.spawn_burst(fc, q, d, amp, 1.4);
        }
        // occasional deeper pops (resin/gas pockets)
        if self.rng.u() < (0.8 + 5.0 * c.fire) * c.fire * act * dt * sg.pop {
            let fc = 250.0 + self.rng.u() * 600.0;
            let d = 0.012 + 0.02 * self.rng.u();
            self.spawn_burst(fc, 0.9, d, 0.35 * (0.3 + c.fire), 1.0);
        }
        // chuffing: low-pressure combustion instability (L* mode), amplitude pulsing at ~chuff_hz
        if sg.chuff_hz > 0.0 {
            self.chuff_ph = (self.chuff_ph + sg.chuff_hz * (0.8 + 0.4 * self.rng.u()) * dt).fract();
            let pulse = 0.5 + 0.5 * (self.chuff_ph * 2.0 * PI).sin();
            self.chuff = 1.0 - sg.chuff_depth * c.lowp * (1.0 - pulse.powi(2));
        } else {
            self.chuff = 1.0;
        }
        self.c_hiss = SvfCoef::new(sg.fc_lo + 0.5 * (sg.fc_hi - sg.fc_lo), 0.6, sr);
        // supersonic jet crackle (shock-associated), only at high thrust
        let jr = ((r - 0.35) / 0.65).clamp(0.0, 1.0);
        if self.rng.u() < 90.0 * jr * jr * act * dt {
            let fc = 600.0 + self.rng.u() * 2500.0;
            let (d, a) = (0.0012 + 0.002 * self.rng.u(), 0.25 + 0.4 * self.rng.u().powi(3));
            self.spawn_burst(fc, 0.5, d, a, 0.7);
        }
        // sizzle pops
        if self.rng.u() < 260.0 * c.sizzle * act * dt {
            self.sz_env = (self.sz_env + 0.5 + self.rng.u()).min(2.5);
        }
        // structural creaks under stress
        let st = ((c.stress - 0.55) / 0.45).clamp(0.0, 1.0);
        if self.rng.u() < (0.15 + 2.5 * st * st) * st * act * dt {
            self.spawn_creak(0.5 + 0.5 * st);
        }
    }

    /// Legacy offline path: dry mix + its own master / limiter.
    fn next(&mut self) -> (f32, f32) {
        let (l, r) = self.next_dry();
        self.master_chain(l, r)
    }

    /// Stats-driven mix up to (and including) its room reverb; no master gain / limiter (the shared `Bus` adds those).
    #[inline]
    fn next_dry(&mut self) -> (f32, f32) {
        // smooth targets
        let t = self.target;
        let (kf, km, ks) = (self.k_fast, self.k_med, self.k_slow);
        let c = &mut self.cur;
        c.fire += (t.fire * t.active - c.fire) * km;
        c.heat += (t.heat - c.heat) * ks;
        c.roar += (t.roar * t.active - c.roar) * km;
        c.hiss += (t.hiss * t.active - c.hiss) * km;
        c.sizzle += (t.sizzle * t.active - c.sizzle) * km;
        c.stress += (t.stress - c.stress) * kf;
        c.active += (t.active - c.active) * ks;
        self.master += (t.master - self.master) * kf;
        let c = self.cur;

        if self.ctr == 0 {
            self.control();
        }
        self.ctr = (self.ctr + 1) % CTRL;

        let sr = self.sr;
        let rng = &mut self.rng;
        let wl = rng.w();
        let wr = rng.w();
        let wm = rng.w();
        let pl = self.pink_l.run(wl);
        let pr = self.pink_r.run(wr);
        let pm = self.pink_m.run(wm);
        let bm = self.brown_m.run(wm);

        let (mut l, mut r) = (0.0f32, 0.0f32);
        let mut send = 0.0f32;

        // ---- fire: low roar + flickering mid body
        if c.fire > 1e-4 {
            self.flk = self.flick.run(rng, 7.0, sr);
            let f2 = self.flick2.run(rng, 2.0, sr);
            let gust = (0.55 + 0.45 * self.flk) * (0.75 + 0.25 * f2);
            let (lp, _, _) = self.fire_lp.run(bm, &self.c_fire_lp);
            let (_, bpl, _) = self.fire_bp_l.run(pl, &self.c_fire_bp);
            let (_, bpr, _) = self.fire_bp_r.run(pr, &self.c_fire_bp);
            let a = c.fire.sqrt() * gust * self.chuff;
            let low = lp * 0.55 * a * self.sig.low;
            l += low + bpl * 0.22 * a * (0.5 + c.heat);
            r += low + bpr * 0.22 * a * (0.5 + c.heat);
            if self.sig.hiss > 0.0 {
                let (_, _, hl) = self.hiss_l.run(wl, &self.c_hiss);
                let (_, _, hr) = self.hiss_r.run(wr, &self.c_hiss);
                let h = 0.05 * self.sig.hiss * c.fire * gust * self.chuff;
                l += hl * h;
                r += hr * h;
            }
        }

        // ---- rocket / jet roar
        if c.roar > 1e-4 {
            self.trb = self.turb.run(rng, 9.0, sr);
            let am = 0.82 + 0.18 * self.trb;
            let (rum, _, _) = self.rum.run(bm, &self.c_rum);
            let (_, rum2, _) = self.rum2.run(bm * 0.6 + pm * 0.4, &self.c_rum2);
            let (_, ml, _) = self.mid_l.run(pl, &self.c_mid);
            let (_, mr, _) = self.mid_r.run(pr, &self.c_mid);
            let (_, _, hl) = self.hs_l.run(wl, &self.c_hs);
            let (_, _, hr) = self.hs_r.run(wr, &self.c_hs);
            let g = c.roar * (0.4 + 0.6 * self.chuff);
            let low = soft(rum * 1.6 * g) * 0.55 + rum2 * 0.35 * g;
            let mid = 0.30 * g * am;
            let hi = 0.07 * g.powf(1.6) * am;
            l += (low + ml * mid + hl * hi) * am;
            r += (low + mr * mid + hr * hi) * am;
            send += 0.25 * g;
        }

        // ---- gas inlet hiss
        if c.hiss > 1e-4 {
            let (_, _, hl) = self.in_l.run(wl, &self.c_in);
            let (_, _, hr) = self.in_r.run(wr, &self.c_in);
            l += hl * 0.06 * c.hiss;
            r += hr * 0.06 * c.hiss;
        }

        // ---- boiling sizzle: steady fizz + dense micro-pops
        if c.sizzle > 1e-4 || self.sz_env > 1e-4 {
            self.sz_env *= 0.9965;
            let (_, sl, _) = self.sz_l.run(wl, &self.c_sz);
            let (_, sr2, _) = self.sz_r.run(wr, &self.c_sz);
            let a = c.sizzle * (0.12 + 0.35 * self.sz_env);
            l += sl * a;
            r += sr2 * a;
        }

        // ---- bursts (crackle, pops, shock crackle)
        for b in self.bursts.iter_mut() {
            if !b.on {
                continue;
            }
            let x = rng.w() * b.env;
            b.env *= b.decay;
            let (_, bp, _) = b.f.run(x, &b.c);
            let v = bp * b.amp;
            l += v * b.pl;
            r += v * b.pr;
            if b.env < 1e-4 {
                b.on = false;
            }
        }

        // ---- pings (metal debris / shrapnel)
        for p in self.pings.iter_mut() {
            if !p.on {
                continue;
            }
            if p.delay > 0 {
                p.delay -= 1;
                continue;
            }
            let re = p.re * p.cr - p.im * p.ci;
            let im = p.re * p.ci + p.im * p.cr;
            p.re = re;
            p.im = im;
            let v = im * p.amp;
            l += v * p.pl;
            r += v * p.pr;
            if re * re + im * im < 1e-8 {
                p.on = false;
            }
        }

        // ---- explosions
        let mut bl_sum = 0.0;
        let mut br_sum = 0.0;
        for b in self.booms.iter_mut() {
            let tt = b.t;
            let s = b.s;
            // initial crack: very short bright noise (N-wave front)
            let crack = rng.w() * (-tt / 0.006).exp() * (0.6 + 0.4 * s.min(2.0)) * self.sig.crack;
            // thump: pitch-falling sine, the "chest hit"
            let f = 32.0 + 85.0 * (-tt / (0.05 + 0.02 * s)).exp();
            b.phase += 2.0 * PI * f / sr;
            if b.phase > 2.0 * PI {
                b.phase -= 2.0 * PI;
            }
            let atk = 1.0 - (-tt / 0.0015).exp();
            let thump = b.phase.sin() * atk * (-tt / (0.22 + 0.18 * s)).exp() * (0.8 + 0.5 * s.min(2.0));
            // body: noise through a lowpass that sweeps down (fireball expansion)
            let fc = 160.0 + 5000.0 * (-tt / (0.08 + 0.05 * s)).exp();
            let cb = SvfCoef::new(fc, 0.7, sr);
            let wn = rng.w();
            let (body, _, _) = b.body.run(b.brown.run(wn) * 0.5 + wn * 0.5, &cb);
            let body = body * (-tt / (0.35 + 0.4 * s)).exp() * atk * (0.7 + 0.4 * s.min(2.0)) * self.sig.boom_body;
            // rolling rumble tail (reflections, thunder-like)
            let roll = b.roll.run(rng, 4.0, sr);
            let ct = SvfCoef::new(90.0 + 60.0 * roll, 0.8, sr);
            let (tail, _, _) = b.tail.run(b.brown2.run(rng.w()), &ct);
            let tenv = (1.0 - (-tt / 0.12).exp()) * (-tt / (1.0 + 1.2 * s)).exp();
            let tail = tail * tenv * (0.4 + 0.6 * roll) * (0.9 + 0.4 * s.min(2.0));
            let g = s.min(3.0).sqrt();
            let mono = (soft(thump * 1.2) * 0.9 + tail * 0.9) * g;
            let side = (crack * 0.5 + body * 0.9) * g;
            bl_sum += mono + side * b.pl * 1.4;
            br_sum += mono + side * b.pr * 1.4;
            send += (body.abs() + tail.abs()) * 0.6 * g;
            b.t += 1.0 / sr;
        }
        self.booms.retain(|b| b.t < 3.0 + 3.0 * b.s.min(2.0));
        l += bl_sum;
        r += br_sum;

        // ---- creaks: stick-slip pulse train exciting metal modes
        for k in self.creaks.iter_mut() {
            let u = k.t / k.dur;
            let f0 = k.f0 + (k.f1 - k.f0) * u;
            k.ph += f0 * (1.0 + 0.4 * rng.w()) / sr;
            let mut x = rng.w() * 0.02;
            if k.ph >= 1.0 {
                k.ph -= 1.0;
                x += 0.6 + 0.4 * rng.u();
            }
            let env = (PI * u).sin().powi(2) * k.amp;
            let drift = 1.0 + 0.04 * (PI * 2.0 * u).sin();
            let mut v = 0.0;
            for (i, m) in [1.0f32, 2.76, 5.40].iter().enumerate() {
                let c = SvfCoef::new(k.base * m * drift, 18.0, sr);
                let (_, bp, _) = k.modes[i].run(x, &c);
                v += bp * [1.0, 0.6, 0.3][i];
            }
            let v = v * env * 2.5;
            l += v * k.pl;
            r += v * k.pr;
            k.t += 1.0 / sr;
            if k.t >= k.dur {
                k.on = false;
            }
        }
        self.creaks.retain(|k| k.on);

        // ---- room reverb
        let (rl, rr) = self.rev.run(send * 0.5 + (l + r) * 0.08, send * 0.5 + (l + r) * 0.08);
        l += rl * 0.35;
        r += rr * 0.35;
        (l, r)
    }

    fn master_chain(&mut self, mut l: f32, mut r: f32) -> (f32, f32) {
        // ---- master, DC block, limiter, soft clip
        l *= self.master;
        r *= self.master;
        let yl = l - self.dc_xl + 0.9995 * self.dc_yl;
        self.dc_xl = l;
        self.dc_yl = yl;
        let yr = r - self.dc_xr + 0.9995 * self.dc_yr;
        self.dc_xr = r;
        self.dc_yr = yr;
        let peak = yl.abs().max(yr.abs());
        if peak > self.lim_env {
            self.lim_env += (peak - self.lim_env) * 0.5;
        } else {
            self.lim_env += (peak - self.lim_env) * 0.0002;
        }
        let thr = 0.7;
        let gt = if self.lim_env > thr { thr / self.lim_env } else { 1.0 };
        if gt < self.lim_g {
            self.lim_g += (gt - self.lim_g) * 0.3;
        } else {
            self.lim_g += (gt - self.lim_g) * 0.0005;
        }
        let ol = soft(yl * self.lim_g);
        let or = soft(yr * self.lim_g);
        if ol.is_finite() && or.is_finite() {
            (ol, or)
        } else {
            // never let a NaN poison the device; reset filter states lazily
            self.lim_env = 0.0;
            (0.0, 0.0)
        }
    }
}

// ---------------------------------------------------------------- shared output bus (mixer end: reverb, limiter)

/// Look-ahead of the master limiter in samples (1 ms at 48 kHz).
pub const LOOKAHEAD: usize = 48;
/// Limiter ceiling (peak, before the safety clipper).
pub const LIM_CEIL: f32 = 0.89;
/// Default room-reverb return of the output bus (the solver already provides the real reflections of the domain,
/// this only adds a little ambience / envelopment on top).
pub const BUS_WET: f32 = 0.2;

/// Transparent safety clipper: linear up to 0.9 (so it never colours the crest factor / skewness of crackle
/// and blast fronts that the lookahead limiter already kept below its ceiling), smooth knee to 1.0 above.
#[inline]
pub fn soft_clip(x: f32) -> f32 {
    let a = x.abs();
    if a <= 0.9 {
        x
    } else {
        x.signum() * (0.9 + 0.1 * ((a - 0.9) / 0.1).tanh())
    }
}

/// Output bus shared by every sound source: room reverb send/return, 1 ms look-ahead peak limiter (attack
/// ~LOOKAHEAD/5, hold for the look-ahead, ~80 ms release), output DC blocker (~4 Hz; the limiter acts
/// asymmetrically on skewed crackle / blast waveforms) and the safety clipper.
pub struct Bus {
    sr: f32,
    rev: Reverb,
    /// reverb return gain (0 = off)
    pub wet: f32,
    la_buf: [[f32; 2]; LOOKAHEAD],
    la_pos: usize,
    lim_hold: u32,
    lim_t: f32,
    lim_g: f32,
    /// last input peak (for meters)
    pub peak: f32,
    dc_out: [f32; 4],
}

impl Bus {
    pub fn new(sr: f32) -> Self {
        Bus { sr, rev: Reverb::new(sr), wet: 0.0, la_buf: [[0.0; 2]; LOOKAHEAD], la_pos: 0, lim_hold: 0, lim_t: 1.0, lim_g: 1.0, peak: 0.0, dc_out: [0.0; 4] }
    }

    /// One stereo sample in (already at the final level, i.e. after the master gain) -> limited output sample.
    #[inline]
    pub fn process(&mut self, x: [f32; 2]) -> (f32, f32) {
        let mut y = x;
        if self.wet > 0.0 {
            let (rl, rr) = self.rev.run(x[0], x[1]);
            y[0] += rl * self.wet;
            y[1] += rr * self.wet;
        }
        let pk = y[0].abs().max(y[1].abs());
        self.peak = pk;
        let need = if pk > LIM_CEIL { LIM_CEIL / pk } else { 1.0 };
        if need <= self.lim_t {
            self.lim_t = need;
            self.lim_hold = LOOKAHEAD as u32;
        } else if self.lim_hold > 0 {
            self.lim_hold -= 1;
        } else {
            self.lim_t += (1.0 - self.lim_t) * (1.0 / (0.08 * self.sr));
        }
        let k = if self.lim_t < self.lim_g { 5.0 / LOOKAHEAD as f32 } else { 1.0 };
        self.lim_g += (self.lim_t - self.lim_g) * k;
        let out = self.la_buf[self.la_pos];
        self.la_buf[self.la_pos] = y;
        self.la_pos = (self.la_pos + 1) % LOOKAHEAD;
        let mut o = [out[0] * self.lim_g, out[1] * self.lim_g];
        for c in 0..2 {
            let v = o[c] - self.dc_out[2 * c] + 0.9995 * self.dc_out[2 * c + 1];
            self.dc_out[2 * c] = o[c];
            self.dc_out[2 * c + 1] = v;
            o[c] = soft_clip(v);
        }
        if o[0].is_finite() && o[1].is_finite() {
            (o[0], o[1])
        } else {
            self.la_buf = [[0.0; 2]; LOOKAHEAD];
            self.dc_out = [0.0; 4];
            self.lim_g = 1.0;
            self.lim_t = 1.0;
            (0.0, 0.0)
        }
    }
}

// ---------------------------------------------------------------- offline render + demo

/// Scripted 8 s scene used by the demo (fire, ignition, rocket, explosion, creaks).
fn demo_snapshot(t: f32, dt: f32) -> AudioSnapshot {
    let mut s = AudioSnapshot { dt, walls: 5000, ..Default::default() };
    s.inlet = if t > 0.3 { 0.8 } else { 0.0 };
    s.burn_kgs = if t > 0.8 { ((t - 0.8) * 3.0).min(6.0) } else { 0.0 };
    s.max_t_k = 300.0 + s.burn_kgs * 400.0;
    s.thrust_kn = if t > 2.5 { ((t - 2.5) * 20.0).min(40.0) } else { 0.0 };
    s.max_p_atm = 1.0 + s.thrust_kn * 0.3;
    if (5.0..5.05).contains(&t) {
        s.max_p_atm = 60.0;
    }
    if t > 5.05 {
        s.walls = 4400;
        s.thrust_kn = 0.0;
        s.burn_kgs = (6.0 - (t - 5.05) * 4.0).max(0.5);
    }
    s.max_stress = if (3.5..5.0).contains(&t) { 0.95 } else { 0.2 };
    s.boil = if t > 6.0 { 0.7 } else { 0.0 };
    s
}

/// Render `secs` of the demo script offline (no device). Returns interleaved stereo.
pub fn render_offline(secs: f32, sr: u32) -> Vec<f32> {
    let mut eng = Engine::new(sr as f32);
    let mut det = Detector::new();
    let fps = 60.0;
    let spf = (sr as f32 / fps) as usize;
    let frames = (secs * fps) as usize;
    let mut out = Vec::with_capacity(frames * spf * 2);
    for f in 0..frames {
        let t = f as f32 / fps;
        let s = demo_snapshot(t, 1.0 / fps);
        let mut ev = Vec::new();
        det.step(&s, &mut ev);
        eng.target = targets_from(&s, 0.8);
        for e in ev {
            eng.trigger(e);
        }
        for _ in 0..spf {
            let (l, r) = eng.next();
            out.push(l);
            out.push(r);
        }
    }
    out
}

/// Named review scenes: (name, signature, seconds). Scripted from the same quantities the app feeds.
pub const REVIEW_SCENES: [(&str, Signature, f32); 8] = [
    ("campfire", Signature::Generic, 8.0),
    ("rocket", Signature::Generic, 8.0),
    ("blackpowder", Signature::BlackPowder, 6.0),
    ("magnesium", Signature::Magnesium, 5.0),
    ("thermite", Signature::Thermite, 7.0),
    ("tnt", Signature::Tnt, 5.0),
    ("graincast", Signature::GrainBurn, 9.0),
    ("explosion_far", Signature::Generic, 5.0),
];

/// (snapshot, booms to fire this frame as (strength, distance_m))
fn review_snapshot(name: &str, t: f32, dt: f32) -> AudioSnapshot {
    let mut s = AudioSnapshot { dt, walls: 5000, ..Default::default() };
    let ramp = |t0: f32, rate: f32, max: f32| if t > t0 { ((t - t0) * rate).min(max) } else { 0.0 };
    match name {
        "campfire" => {
            s.burn_kgs = ramp(0.5, 1.0, 1.2) * (0.85 + 0.15 * (t * 2.3).sin());
            s.max_t_k = 900.0 + 400.0 * (s.burn_kgs / 1.2);
        }
        "rocket" => {
            // ignition at 1 s, thrust to 35 kN, chamber 30 atm; shut-down tail at 6.5 s
            let on = if t < 6.5 { ramp(1.0, 14.0, 35.0) } else { (35.0 - (t - 6.5) * 40.0).max(0.0) };
            s.thrust_kn = on;
            s.burn_kgs = on / 8.0;
            s.max_t_k = 300.0 + on * 85.0;
            s.max_p_atm = 1.0 + on * 0.8;
            s.inlet = if t > 0.5 { 0.7 } else { 0.0 };
        }
        "blackpowder" => {
            s.burn_kgs = if (0.5..3.5).contains(&t) { 2.0 } else { 0.0 };
            s.max_t_k = 300.0 + s.burn_kgs * 600.0;
        }
        "magnesium" => {
            s.burn_kgs = if (0.5..3.0).contains(&t) { 3.0 } else { 0.0 };
            s.max_t_k = 300.0 + s.burn_kgs * 900.0;
        }
        "thermite" => {
            s.burn_kgs = ramp(0.5, 2.0, 1.5);
            s.max_t_k = 300.0 + s.burn_kgs * 1800.0;
        }
        "graincast" => {
            // 0-4.5 s: low chamber pressure (chuffs); then pressure rises and it settles to a steady roar
            let p = if t < 4.5 { 1.4 } else { (1.4 + (t - 4.5) * 4.0).min(12.0) };
            s.max_p_atm = p;
            s.burn_kgs = ramp(0.5, 2.0, 2.5);
            s.thrust_kn = ramp(0.5, 3.0, 10.0);
            s.max_t_k = 300.0 + s.burn_kgs * 800.0;
        }
        _ => {}
    }
    s
}

fn review_booms(name: &str, t0: f32, t1: f32) -> Vec<(f32, f32)> {
    let ev: &[(f32, f32, f32)] = match name {
        "blackpowder" => &[(3.5, 1.2, 0.0)],
        "magnesium" => &[(3.0, 0.8, 0.0)],
        "tnt" => &[(1.0, 2.0, 0.0)],
        "explosion_far" => &[(0.5, 1.5, 0.0), (2.0, 1.5, 200.0)],
        "rocket" => &[],
        _ => &[],
    };
    ev.iter().filter(|e| e.0 >= t0 && e.0 < t1).map(|e| (e.1, e.2)).collect()
}

/// Render one review scene offline (stereo interleaved). Booms arrive after distance / 347 m/s.
pub fn render_review(name: &str, sig: Signature, secs: f32, sr: u32) -> Vec<f32> {
    let mut eng = Engine::new(sr as f32);
    eng.sig = sig.params();
    let mut det = Detector::new();
    let fps = 60.0;
    let spf = (sr as f32 / fps) as usize;
    let frames = (secs * fps) as usize;
    let mut out = Vec::with_capacity(frames * spf * 2);
    let mut pending: Vec<(f32, f32)> = Vec::new();
    let mut master = Bus::new(sr as f32);
    master.wet = BUS_WET;
    for f in 0..frames {
        let t = f as f32 / fps;
        let s = review_snapshot(name, t, 1.0 / fps);
        let mut ev = Vec::new();
        det.step(&s, &mut ev);
        for (st, d) in review_booms(name, t, t + 1.0 / fps) {
            let g = st / (1.0 + d / 15.0);
            pending.push((d / 347.0, g));
        }
        pending.retain_mut(|(w, g)| {
            *w -= 1.0 / fps;
            if *w <= 0.0 {
                ev.push(Event::Boom { strength: *g });
                false
            } else {
                true
            }
        });
        eng.target = targets_from(&s, 0.8);
        for e in ev {
            eng.trigger(e);
        }
        for _ in 0..spf {
            let (l, r) = eng.next_dry();
            let (l, r) = master.process([l * 0.8, r * 0.8]);
            out.push(l);
            out.push(r);
        }
    }
    out
}

fn write_wav(path: &str, data: &[f32], sr: u32) -> std::io::Result<()> {
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

/// Headless smoke test: offline render with per-second level report (optionally
/// written to $AUDIO_WAV), then a few seconds of live playback if a device exists
/// (skipped when AUDIO_NO_DEVICE is set).
pub fn demo_cli() {
    let sr = 48_000;
    let secs = 8.0;
    let buf = render_offline(secs, sr);
    let mut bad = 0usize;
    for (i, chunk) in buf.chunks(sr as usize * 2).enumerate() {
        let mut pk = 0.0f32;
        let mut ss = 0.0f64;
        for &v in chunk {
            if !v.is_finite() {
                bad += 1;
            }
            pk = pk.max(v.abs());
            ss += (v as f64) * (v as f64);
        }
        let rms = (ss / chunk.len().max(1) as f64).sqrt();
        println!("t={i}s  peak={pk:.3}  rms={rms:.4}");
    }
    println!("non-finite samples: {bad}");
    if let Ok(p) = std::env::var("AUDIO_WAV") {
        match write_wav(&p, &buf, sr) {
            Ok(()) => println!("wrote {p}"),
            Err(e) => println!("wav write failed: {e}"),
        }
    }
    if let Ok(dir) = std::env::var("AUDIO_SCENES_DIR") {
        let _ = std::fs::create_dir_all(&dir);
        for (name, sig, secs) in REVIEW_SCENES {
            let b = render_review(name, sig, secs, sr);
            let mut pk = 0.0f32;
            let mut ss = 0.0f64;
            let mut bad = 0;
            for &v in &b {
                if !v.is_finite() {
                    bad += 1;
                }
                pk = pk.max(v.abs());
                ss += (v as f64) * (v as f64);
            }
            let path = format!("{dir}/{name}.wav");
            let _ = write_wav(&path, &b, sr);
            println!("{name:14} sig={:12} peak={pk:.3} rms={:.4} nonfinite={bad} -> {path}", sig.name(), (ss / b.len().max(1) as f64).sqrt());
        }
    }
    if std::env::var("AUDIO_NO_DEVICE").is_ok() {
        return;
    }
    match Audio::new() {
        None => println!("no audio device; live test skipped"),
        Some(mut a) => {
            println!("live audio at {} Hz", a.sample_rate);
            a.set_volume(std::env::var("AUDIO_VOL").ok().and_then(|v| v.parse().ok()).unwrap_or(0.5));
            let dt = 1.0 / 60.0;
            let mut t = 0.0f32;
            while t < 4.0 {
                let s = demo_snapshot(t + 1.5, dt);
                a.update(&s);
                std::thread::sleep(std::time::Duration::from_secs_f32(dt));
                t += dt;
            }
            a.set_muted(true);
            std::thread::sleep(std::time::Duration::from_millis(200));
            println!("live test done");
        }
    }
}
