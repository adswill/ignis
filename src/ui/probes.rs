//! Measurement instruments: point probes and line samplers placed in the viewport, sampled from a
//! blocking state readback a few times per second, plus whole-domain diagnostics (mass / energy
//! drift, CFL number). CSV export of every probe time series and of line profiles.
use super::{gas, App, MPS_PER_UNIT, MS_PER_UNIT};
use crate::gpu::{GpuSim, DT, H, W};
use crate::scene;
use eframe::egui::Color32;
use std::collections::VecDeque;

/// One sample of a cell. Solid cells (walls, matter) report NaN for gas-only fields.
#[derive(Clone, Copy, Default)]
pub struct Sample {
    pub t_ms: f32,
    pub p_atm: f32,
    pub t_k: f32,
    pub speed: f32,
    pub vx: f32,
    pub vy: f32,
    pub rho: f32,
    pub y_fuel: f32,
    pub y_o2: f32,
    pub y_n2: f32,
    pub y_prod: f32,
    pub soot: f32,
    pub mach: f32,
    #[allow(dead_code)]
    pub solid: bool,
}

pub struct Probe {
    pub id: u32,
    pub x: i32,
    pub y: i32,
    pub color: Color32,
    pub series: VecDeque<Sample>,
    #[allow(dead_code)]
    pub visible: bool,
}

pub struct LineSampler {
    pub id: u32,
    pub a: (i32, i32),
    pub b: (i32, i32),
    pub color: Color32,
    pub profile: Vec<(f32, Sample)>,
}

impl LineSampler {
    pub fn length_cm(&self) -> f32 {
        (((self.b.0 - self.a.0).pow(2) + (self.b.1 - self.a.1).pow(2)) as f32).sqrt()
    }
}

#[derive(Clone, Copy, PartialEq)]
pub enum Place {
    Probe,
    Line(Option<(i32, i32)>),
}

/// Probe colours: distinguishable on dark grey and against each other (no accent blue clash).
pub const PROBE_COLS: [Color32; 8] = [
    Color32::from_rgb(230, 159, 0),
    Color32::from_rgb(86, 180, 233),
    Color32::from_rgb(0, 158, 115),
    Color32::from_rgb(240, 228, 66),
    Color32::from_rgb(204, 121, 167),
    Color32::from_rgb(213, 94, 0),
    Color32::from_rgb(170, 170, 255),
    Color32::from_rgb(200, 200, 200),
];

#[derive(Default)]
pub struct ProbeSet {
    pub probes: Vec<Probe>,
    pub lines: Vec<LineSampler>,
    next_id: u32,
    timer: f32,
    pub selected: Option<u32>,
}

impl ProbeSet {
    pub fn reset_series(&mut self) {
        for p in &mut self.probes {
            p.series.clear();
        }
        for l in &mut self.lines {
            l.profile.clear();
        }
    }

    pub fn add_probe(&mut self, x: i32, y: i32) -> u32 {
        self.next_id += 1;
        let color = PROBE_COLS[(self.next_id as usize - 1) % PROBE_COLS.len()];
        self.probes.push(Probe { id: self.next_id, x, y, color, series: VecDeque::new(), visible: true });
        self.timer = 1.0;
        self.next_id
    }

    pub fn add_line(&mut self, a: (i32, i32), b: (i32, i32)) -> u32 {
        self.next_id += 1;
        let color = PROBE_COLS[(self.next_id as usize - 1) % PROBE_COLS.len()];
        self.lines.push(LineSampler { id: self.next_id, a, b, color, profile: vec![] });
        self.timer = 1.0;
        self.next_id
    }

    pub fn is_empty(&self) -> bool {
        self.probes.is_empty() && self.lines.is_empty()
    }
}

/// Whole-domain diagnostics, refreshed about once per second.
#[derive(Default, Clone, Copy)]
pub struct Diagnostics {
    pub valid: bool,
    pub mass0: f64,
    pub energy0: f64,
    pub mass: f64,
    pub energy: f64,
    pub cfl: f32,
    pub vmax: f32,
    pub timer: f32,
    /// wall-clock ms the last blocking readback took
    pub readback_ms: f32,
}

impl Diagnostics {
    pub fn mass_drift(&self) -> f64 {
        if self.mass0 > 0.0 {
            (self.mass - self.mass0) / self.mass0 * 100.0
        } else {
            0.0
        }
    }
    pub fn energy_drift(&self) -> f64 {
        if self.energy0 > 0.0 {
            (self.energy - self.energy0) / self.energy0 * 100.0
        } else {
            0.0
        }
    }
}

fn sample_cell(st: &[f32], meta: &[u32], wall: Option<&[f32]>, x: i32, y: i32, t_ms: f32) -> Sample {
    let x = x.clamp(0, W as i32 - 1) as usize;
    let y = y.clamp(0, H as i32 - 1) as usize;
    let i = y * W as usize + x;
    let tile = meta.get(i).copied().unwrap_or(0) & 0xff;
    if tile == scene::WALL || tile == scene::MATTER || tile == scene::IGNITER {
        let t = wall.and_then(|w| w.get(i * 4).copied()).unwrap_or(f32::NAN) * 300.0;
        return Sample { t_ms, p_atm: f32::NAN, t_k: t, speed: f32::NAN, vx: f32::NAN, vy: f32::NAN, rho: f32::NAN, y_fuel: f32::NAN, y_o2: f32::NAN, y_n2: f32::NAN, y_prod: f32::NAN, soot: f32::NAN, mach: f32::NAN, solid: true };
    }
    let mut u = [0f32; 8];
    u.copy_from_slice(&st[i * 8..i * 8 + 8]);
    let g = gas(&u);
    let r = g.rho;
    Sample {
        t_ms,
        p_atm: g.p,
        t_k: g.t * 300.0,
        speed: g.speed * MPS_PER_UNIT,
        vx: u[4] / r * MPS_PER_UNIT,
        vy: -u[5] / r * MPS_PER_UNIT,
        rho: r * 1.2,
        y_fuel: u[0].max(0.0) / r,
        y_o2: u[1].max(0.0) / r,
        y_n2: u[2].max(0.0) / r,
        y_prod: u[3].max(0.0) / r,
        soot: u[7].max(0.0) / r,
        mach: g.mach,
        solid: false,
    }
}

impl App {
    /// Click handler while a placement mode is active.
    pub(super) fn place_click(&mut self, at: (i32, i32)) {
        match self.place {
            Some(Place::Probe) => {
                let id = self.probes.add_probe(at.0, at.1);
                self.probes.selected = Some(id);
                self.log_info(&format!("Probe P{id} placed at x = {} cm, y = {} cm", at.0, H as i32 - 1 - at.1));
                self.place = None;
                self.dock_bottom = true;
                self.bottom_tab = super::ws::BottomTab::Probes;
            }
            Some(Place::Line(None)) => self.place = Some(Place::Line(Some(at))),
            Some(Place::Line(Some(a))) => {
                if a != at {
                    let id = self.probes.add_line(a, at);
                    self.log_info(&format!("Line sampler L{id} placed ({:.0} cm)", self.probes.lines.last().map(|l| l.length_cm()).unwrap_or(0.0)));
                    self.dock_bottom = true;
                    self.bottom_tab = super::ws::BottomTab::Profiles;
                }
                self.place = None;
            }
            None => {}
        }
    }

    pub(super) fn place_anchor(&self) -> Option<(i32, i32)> {
        match self.place {
            Some(Place::Line(a)) => a,
            _ => None,
        }
    }

    /// Screenshot hook: a probe pair and a sampler through the current scene's hottest region.
    pub(super) fn demo_probes(&mut self) {
        self.probes.add_probe(300, 144);
        self.probes.add_probe(400, 144);
        self.probes.add_line((180, 144), (500, 144));
    }

    /// Blocking readbacks for probes (4 Hz) and diagnostics (1 Hz). Skipped when paused.
    pub(super) fn sample_probes(&mut self, frame: &eframe::Frame, dt_real: f32, steps: u32) {
        self.probes.timer += dt_real;
        self.diag.timer += dt_real;
        let want_probes = !self.probes.is_empty() && self.probes.timer >= 0.5 && (steps > 0 || self.probes.probes.iter().all(|p| p.series.is_empty()));
        let want_diag = self.diag.timer >= 2.0 && (steps > 0 || !self.diag.valid);
        if !want_probes && !want_diag {
            return;
        }
        let Some(rs) = frame.wgpu_render_state() else { return };
        let r = rs.renderer.read();
        let Some(sim) = r.callback_resources.get::<GpuSim>() else { return };
        let t0 = std::time::Instant::now();
        let (st, meta) = sim.read_state(&rs.device, &rs.queue);
        let need_wall = want_probes && {
            let solid = |x: i32, y: i32| {
                let i = (y.clamp(0, H as i32 - 1) * W as i32 + x.clamp(0, W as i32 - 1)) as usize;
                matches!(meta.get(i).map(|v| v & 0xff), Some(t) if t == scene::WALL || t == scene::MATTER)
            };
            self.probes.probes.iter().any(|p| solid(p.x, p.y)) || !self.probes.lines.is_empty()
        };
        let wall = if need_wall { Some(sim.read_wall(&rs.device, &rs.queue)) } else { None };
        drop(r);
        let t_ms = self.sim_time * MS_PER_UNIT;
        if want_probes {
            self.probes.timer = 0.0;
            for p in &mut self.probes.probes {
                let s = sample_cell(&st, &meta, wall.as_deref(), p.x, p.y, t_ms);
                // the first readback can land before the GPU state is initialised: skip all-zero samples
                if !(s.t_k.is_finite() && s.t_k > 1.0) || (!s.solid && !(s.rho > 1e-4)) {
                    continue;
                }
                p.series.push_back(s);
                if p.series.len() > 2400 {
                    p.series.pop_front();
                }
            }
            for l in &mut self.probes.lines {
                let len = l.length_cm().max(1.0);
                let n = (len.ceil() as usize + 1).min(600);
                l.profile.clear();
                for k in 0..n {
                    let f = k as f32 / (n - 1).max(1) as f32;
                    let x = l.a.0 as f32 + (l.b.0 - l.a.0) as f32 * f;
                    let y = l.a.1 as f32 + (l.b.1 - l.a.1) as f32 * f;
                    l.profile.push((f * len, sample_cell(&st, &meta, wall.as_deref(), x.round() as i32, y.round() as i32, t_ms)));
                }
            }
        }
        if want_diag {
            self.diag.timer = 0.0;
            let (mut m, mut e, mut cfl, mut vmax) = (0f64, 0f64, 0f32, 0f32);
            for (i, &v) in meta.iter().enumerate() {
                let t = v & 0xff;
                if t == scene::WALL || t == scene::MATTER || t == scene::IGNITER {
                    continue;
                }
                let c = &st[i * 8..i * 8 + 8];
                let rho = (c[0] + c[1] + c[2] + c[3]).max(1e-4);
                m += rho as f64;
                e += c[6] as f64;
                let (vx, vy) = (c[4] / rho, c[5] / rho);
                let sp = (vx * vx + vy * vy).sqrt();
                let cv = (c[0] * 5.827 + c[1] * 2.258 + c[2] * 2.580 + c[3] * 4.446).max(1e-4);
                let rr = c[0] * 1.80625 + c[1] * 0.903125 + c[2] * 1.032142857 + c[3] * 1.111538;
                let ein = (c[6] - 0.5 * rho * sp * sp).max(0.0);
                let p = rr / cv * ein;
                let snd = ((1.0 + rr / cv) * p / rho).max(0.0).sqrt();
                if (sp + snd).is_finite() {
                    cfl = cfl.max((sp + snd) * DT);
                    vmax = vmax.max(sp);
                }
            }
            if !self.diag.valid {
                self.diag.mass0 = m;
                self.diag.energy0 = e;
                self.diag.valid = true;
            }
            self.diag.mass = m;
            self.diag.energy = e;
            self.diag.cfl = cfl;
            self.diag.vmax = vmax * MPS_PER_UNIT;
        }
        self.diag.readback_ms = t0.elapsed().as_secs_f32() * 1000.0;
    }

    /// File > Export probe data (CSV): one row per sample time, columns per probe.
    pub(super) fn export_probes_csv(&mut self) {
        if self.probes.probes.is_empty() && self.probes.lines.is_empty() {
            self.log_warn("No probes or line samplers to export");
            return;
        }
        let Some(path) = rfd::FileDialog::new().add_filter("CSV", &["csv"]).set_file_name("ignis_probes.csv").save_file() else { return };
        let mut out = String::new();
        out += &format!("# Ignis probe export, setup: {}\n", self.scene_name);
        out += "# units: t ms, x/y cm (y up), p kPa, T K, |v| m/s, vx/vy m/s, rho kg/m3, Y mass fraction\n";
        out += "probe,x_cm,y_cm,t_ms,p_kPa,T_K,speed_ms,vx_ms,vy_ms,rho_kgm3,mach,Y_CH4,Y_O2,Y_N2,Y_products,Y_soot\n";
        for p in &self.probes.probes {
            for s in &p.series {
                out += &format!(
                    "P{},{},{},{:.4},{:.3},{:.2},{:.3},{:.3},{:.3},{:.5},{:.4},{:.5},{:.5},{:.5},{:.5},{:.6}\n",
                    p.id,
                    p.x,
                    H as i32 - 1 - p.y,
                    s.t_ms,
                    s.p_atm * 101.325,
                    s.t_k,
                    s.speed,
                    s.vx,
                    s.vy,
                    s.rho,
                    s.mach,
                    s.y_fuel,
                    s.y_o2,
                    s.y_n2,
                    s.y_prod,
                    s.soot
                );
            }
        }
        for l in &self.probes.lines {
            out += &format!("# line L{} from ({}, {}) to ({}, {}) cm; s = distance along the line\n", l.id, l.a.0, H as i32 - 1 - l.a.1, l.b.0, H as i32 - 1 - l.b.1);
            out += "line,s_cm,t_ms,p_kPa,T_K,speed_ms,Y_CH4,Y_O2,Y_products,Y_soot\n";
            for (d, s) in &l.profile {
                out += &format!("L{},{:.2},{:.4},{:.3},{:.2},{:.3},{:.5},{:.5},{:.5},{:.6}\n", l.id, d, s.t_ms, s.p_atm * 101.325, s.t_k, s.speed, s.y_fuel, s.y_o2, s.y_prod, s.soot);
            }
        }
        match std::fs::write(&path, out) {
            Ok(_) => self.toast(&format!("Exported probe data to {}", path.display())),
            Err(e) => self.log_warn(&format!("Export failed: {e}")),
        }
    }
}
