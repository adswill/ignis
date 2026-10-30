//! Reciprocating engine designer: builds a cylinder-and-head layout and the GPU configuration image (engine.wgsl)
//! from design parameters. Geometry is a horizontal bore with the head on the right; one cell is about 1 cm.
//!
//! The piston is a kinematic crank-slider driven at a prescribed speed (an infinitely heavy flywheel); valves, spark
//! plugs and injectors follow cam timing in crank degrees. Everything else (intake flow, compression heating,
//! flame propagation, expansion, blowdown, scavenging) is solved by the gas dynamics.

use crate::gpu::{H, W};
use crate::scene::Scene;

const WALL: u32 = 1;
const VENT: u32 = 7;
const PISTON: u32 = 9;
const VALVE_CLOSED: u32 = 10;
const SPARK: u32 = 12;
const INJ_OFF: u32 = 13;
const PREMIX: u32 = 15;
const AIR_IN: u32 = 4;
const TITANIUM: u32 = 3;
const INCONEL: u32 = 4;

// word offsets inside the engine block (must match engine.wgsl)
const W_OMEGA: usize = 0;
const W_THETA: usize = 1;
const W_NPIST: usize = 2;
const W_NCELLS: usize = 3;
const W_PERIOD: usize = 4;
const W_THICK: usize = 5;
const W_PIST: usize = 16;
const W_VALVE: usize = 80;
const W_SPARK: usize = 144;
const W_INJ: usize = 208;
const W_CELLS: usize = 272;
pub const MAX_CYLINDERS: usize = 4;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Cycle {
    /// four-stroke, premixed charge, spark ignition
    Otto,
    /// four-stroke, air only, timed fuel injection near TDC, compression ignition (hot-surface assisted)
    Diesel,
    /// two-stroke, ported (blower-scavenged), premixed charge, spark ignition
    TwoStroke,
}

#[derive(Clone, Debug)]
pub struct EngineParams {
    pub cycle: Cycle,
    pub cylinders: usize,
    /// bore in cells (rows), stroke in cells
    pub bore: i32,
    pub stroke: i32,
    /// connecting rod length in cells
    pub rod: f32,
    /// combustion chamber height at TDC in cells (compression ratio = (stroke + clearance) / clearance)
    pub clearance: i32,
    /// simulation time units per crank revolution
    pub rev_units: f32,
    /// valve timing in crank degrees of the cam cycle (720 for four-stroke)
    pub ivo: f32,
    pub ivc: f32,
    pub evo: f32,
    pub evc: f32,
    /// spark (Otto) or injection (Diesel) start, crank degrees of the cam cycle, and duration
    pub ign_start: f32,
    pub ign_dur: f32,
    /// mixture equivalence ratio of the intake charge (Otto/TwoStroke)
    pub phi: f32,
    /// intake manifold pressure in atm (1 = naturally aspirated, above 1 = turbo/supercharged)
    pub boost: f32,
    /// head and cylinder wall material
    pub wall_mat: u32,
    /// cylinder wall thickness in cells (strength scales with it)
    pub wall: i32,
}

impl EngineParams {
    pub fn compression_ratio(&self) -> f32 {
        (self.stroke + self.clearance) as f32 / self.clearance as f32
    }

    pub fn displacement_cells(&self) -> i32 {
        self.bore * self.stroke * self.cylinders as i32
    }

    pub fn otto() -> Self {
        EngineParams {
            cycle: Cycle::Otto,
            cylinders: 1,
            bore: 40,
            stroke: 72,
            rod: 140.0,
            clearance: 9,
            rev_units: 1400.0,
            ivo: 0.0,
            ivc: 220.0,
            evo: 500.0,
            evc: 718.0,
            ign_start: 358.0,
            ign_dur: 10.0,
            phi: 0.9,
            boost: 1.0,
            wall_mat: TITANIUM,
            wall: 18,
        }
    }

    /// smaller bore for multi-cylinder layouts that must fit the 288-row domain
    pub fn compact() -> Self {
        EngineParams { bore: 24, stroke: 44, rod: 85.0, clearance: 5, rev_units: 900.0, wall: 10, ..Self::otto() }
    }

    pub fn diesel() -> Self {
        EngineParams {
            cycle: Cycle::Diesel,
            clearance: 4,
            ign_start: 345.0,
            ign_dur: 24.0,
            wall_mat: INCONEL,
            ..Self::otto()
        }
    }

    pub fn two_stroke() -> Self {
        EngineParams { cycle: Cycle::TwoStroke, ign_start: 160.0, ign_dur: 14.0, rev_units: 1100.0, boost: 1.6, ..Self::otto() }
    }

    pub fn with_cylinders(mut self, n: usize) -> Self {
        self.cylinders = n.clamp(1, MAX_CYLINDERS);
        self
    }

    /// cam-cycle phase of each cylinder in degrees (firing intervals evenly spaced)
    fn phases(&self) -> Vec<f32> {
        let n = self.cylinders;
        match (self.cycle, n) {
            (Cycle::TwoStroke, 1) => vec![0.0],
            (Cycle::TwoStroke, 2) => vec![0.0, 180.0],
            (Cycle::TwoStroke, 3) => vec![0.0, 240.0, 120.0],
            (Cycle::TwoStroke, _) => vec![0.0, 180.0, 90.0, 270.0],
            (_, 1) => vec![0.0],
            (_, 2) => vec![0.0, 360.0],
            (_, 3) => vec![0.0, 480.0, 240.0],
            _ => vec![0.0, 540.0, 180.0, 360.0], // firing order 1-3-4-2
        }
    }

    pub fn period_deg(&self) -> f32 {
        if self.cycle == Cycle::TwoStroke {
            360.0
        } else {
            720.0
        }
    }

    /// Build the layout and the GPU engine image.
    pub fn build(&self) -> Scene {
        let mut s = Scene::empty();
        let wmat: u32 = std::env::var("ENG_MAT").ok().and_then(|v| v.parse().ok()).unwrap_or(self.wall_mat);
        let thick = (self.bore / 5).clamp(5, 8); // piston body thickness
        let wt = self.wall; // cylinder wall thickness (cells)
        let head_t = 14i32; // head block thickness (valves sit in it)
        let chan = 40i32; // port channel length
        let n = self.cylinders;
        let pitch = self.bore + 2 * wt + 6;
        let xh = 300i32; // inner face of the head wall
        let x_tdc = xh - 1 - self.clearance;
        let x_bdc = x_tdc - self.stroke;
        let x_rear = x_bdc - thick - 10;
        let centre = (H / 2) as i32;
        let period = self.period_deg().to_radians();
        let omega = std::f32::consts::TAU / self.rev_units;
        let phases = self.phases();
        let mut words = vec![0u32; 1024];
        let fw = |v: f32| v.to_bits();
        words[W_OMEGA] = fw(omega);
        words[W_THETA] = fw(0.0);
        words[W_NPIST] = n as u32;
        words[W_PERIOD] = fw(period);
        words[W_THICK] = thick as u32;
        let mut cells: Vec<u32> = vec![];
        let ports = if self.cycle == Cycle::TwoStroke { 0 } else { (self.bore * 7 / 20).max(5) };

        for k in 0..n {
            let cy = centre + ((k as f32 - (n as f32 - 1.0) / 2.0) * pitch as f32).round() as i32;
            let y0 = cy - self.bore / 2;
            let y1 = cy + self.bore / 2; // exclusive
            let ph = phases[k].to_radians();
            // --- walls: cylinder barrel and head block
            for x in x_rear..xh {
                for t in 0..wt {
                    s.put(x, y0 - 1 - t, WALL, wmat, 0);
                    s.put(x, y1 + t, WALL, wmat, 0);
                }
            }
            for x in xh..xh + head_t + chan {
                for y in y0 - wt..y1 + wt {
                    s.put(x, y, WALL, wmat, 0);
                }
            }
            // --- piston (initial position at crank angle 0)
            let head0 = piston_head_cell(x_tdc as f32, self.stroke as f32, self.rod, ph);
            for x in head0 - thick + 1..=head0 {
                for y in y0..y1 {
                    s.put(x, y, PISTON, k as u32, 0);
                }
            }
            let b = W_PIST + k * 8;
            words[b] = fw(x_tdc as f32);
            words[b + 1] = fw(self.stroke as f32);
            words[b + 2] = fw(self.rod);
            words[b + 3] = fw(ph);
            words[b + 4] = y0 as u32;
            words[b + 5] = self.bore as u32;
            words[b + 6] = head0 as u32;
            words[b + 7] = head0 as u32;

            match self.cycle {
                Cycle::Otto | Cycle::Diesel => {
                    let (iv, ev) = (2 * k, 2 * k + 1);
                    let iy = y0 + 2; // intake rows
                    let ey = y1 - 2 - ports; // exhaust rows
                    for (id, ry) in [(iv, iy), (ev, ey)] {
                        // valve plug through the head, then the port channel
                        for x in xh..xh + head_t {
                            for y in ry..ry + ports {
                                s.put(x, y, VALVE_CLOSED, id as u32, 0);
                                cells.push(((y as u32) * W + x as u32) | (0 << 20));
                            }
                        }
                        for x in xh + head_t..xh + head_t + chan {
                            for y in ry..ry + ports {
                                s.put(x, y, 0, 0, 0);
                            }
                        }
                    }
                    // intake charge (premixed) or air; exhaust to ambient
                    let last = xh + head_t + chan - 1;
                    for y in iy..iy + ports {
                        if self.cycle == Cycle::Otto {
                            s.put(last, y, PREMIX, (self.phi * 10.0).round() as u32, 0);
                        } else {
                            s.put(last, y, AIR_IN, 0, 0);
                        }
                    }
                    for y in ey..ey + ports {
                        s.put(last, y, VENT, 0, 0);
                    }
                    let cam = |deg: f32| deg.to_radians();
                    let vb = W_VALVE + iv * 4;
                    words[vb] = fw(cam(self.ivo));
                    words[vb + 1] = fw(cam(self.ivc));
                    words[vb + 2] = fw(ph);
                    let eb = W_VALVE + ev * 4;
                    words[eb] = fw(cam(self.evo));
                    words[eb + 1] = fw(cam(self.evc));
                    if std::env::var("ENG_NOEXH").is_ok() {
                        words[eb + 1] = words[eb]; // debug: exhaust valve never opens
                    }
                    words[eb + 2] = fw(ph);
                    if self.cycle == Cycle::Otto {
                        // spark plug on the head face, centre of the chamber
                        for y in [cy - 1, cy] {
                            s.put(xh, y, SPARK, k as u32, 0);
                        }
                        let sb = W_SPARK + k * 4;
                        words[sb] = fw(cam(self.ign_start));
                        words[sb + 1] = fw(cam(self.ign_start + self.ign_dur));
                        words[sb + 2] = fw(ph);
                        if std::env::var("ENG_NOSPARK").is_ok() {
                            words[sb + 1] = words[sb]; // debug: motoring run, no ignition
                        }
                    } else {
                        // injector nozzle at the head centre; a glow plug (hot surface) beside it
                        for y in [cy, cy + 1] {
                            s.put(xh, y, INJ_OFF, k as u32, 0);
                            cells.push(((y as u32) * W + xh as u32) | (1 << 20));
                        }
                        s.put(xh, cy - 2, 6, 0, 0);
                        let jb = W_INJ + k * 4;
                        words[jb] = fw(cam(self.ign_start));
                        words[jb + 1] = fw(cam(self.ign_start + self.ign_dur));
                        words[jb + 2] = fw(ph);
                    }
                }
                Cycle::TwoStroke => {
                    // ported: a blower-fed intake port in the top wall just above BDC, an exhaust port in the bottom
                    // wall that opens earlier (higher up the bore) so the cylinder blows down before it is scavenged
                    let ix0 = x_bdc + 1;
                    let ex0 = x_bdc + self.stroke * 28 / 100;
                    let pw = (self.bore * 9 / 20).max(6);
                    for x in ix0..ix0 + pw {
                        for t in 0..wt {
                            s.put(x, y0 - 1 - t, 0, 0, 0);
                        }
                    }
                    for x in ex0..ex0 + pw {
                        for t in 0..wt {
                            s.put(x, y1 + t, 0, 0, 0);
                        }
                    }
                    // intake plenum above and exhaust vent below the port columns
                    for x in ix0..ix0 + pw {
                        s.put(x, y0 - wt, PREMIX, (self.phi * 10.0).round() as u32, 0);
                    }
                    for x in ex0..ex0 + pw {
                        s.put(x, y1 + wt - 1, VENT, 0, 0);
                    }
                    for y in [cy - 1, cy] {
                        s.put(xh, y, SPARK, k as u32, 0);
                    }
                    let sb = W_SPARK + k * 4;
                    words[sb] = fw(self.ign_start.to_radians());
                    words[sb + 1] = fw((self.ign_start + self.ign_dur).to_radians());
                    words[sb + 2] = fw(ph);
                }
            }
        }
        words[W_NCELLS] = cells.len().min(512) as u32;
        for (i, c) in cells.iter().take(512).enumerate() {
            words[W_CELLS + i] = *c;
        }
        s.inlet_p = if self.cycle == Cycle::Diesel { 90.0 } else { 3.0 };
        s.o2_p = 3.0;
        s.p_air = self.boost;
        s.engine = words;
        s
    }
}

/// Head cell of the piston for a crank-slider at angle `a` (rad); mirrors piston_head() in engine.wgsl.
pub fn piston_head_cell(x_tdc: f32, stroke: f32, rod: f32, a: f32) -> i32 {
    let r = 0.5 * stroke;
    let sn = a.sin();
    let d = r * (1.0 - a.cos()) + rod - (rod * rod - r * r * sn * sn).max(0.0).sqrt();
    (x_tdc - d + 0.5).floor() as i32
}

/// Opposed-piston two-stroke engine (Junkers Jumo / Napier Deltic layout): two pistons face each other in one bore with
/// no head. The left piston uncovers the intake ports, the right one the exhaust ports and leads it by `lead` degrees,
/// so the cylinder blows down, is scavenged left to right (uniflow) and both pistons compress the charge between them.
pub fn opposed(diesel: bool) -> Scene {
    let mut s = Scene::empty();
    let (bore, stroke, rod, clr, wt) = (36i32, 62i32, 124.0f32, 7i32, 14i32);
    let thick = 6i32;
    let wmat = INCONEL;
    let cy = (H / 2) as i32;
    let y0 = cy - bore / 2;
    let y1 = cy + bore / 2;
    let xc = 256i32; // chamber centre
    let x_ltdc = xc - clr;
    let x_lbdc = x_ltdc - stroke;
    let x_rtdc = xc + clr;
    let x_rbdc = x_rtdc + stroke;
    let lead = 14.0f32.to_radians();
    let period = std::f32::consts::TAU;
    let omega = std::f32::consts::TAU / 1100.0;
    let fw = |v: f32| v.to_bits();
    let mut words = vec![0u32; 1024];
    words[W_OMEGA] = fw(omega);
    words[W_NPIST] = 2;
    words[W_PERIOD] = fw(period);
    words[W_THICK] = thick as u32;
    // barrel walls over the full length of the bore
    for x in x_lbdc - thick - 8..x_rbdc + thick + 9 {
        for t in 0..wt {
            s.put(x, y0 - 1 - t, WALL, wmat, 0);
            s.put(x, y1 + t, WALL, wmat, 0);
        }
    }
    // pistons: left (normal) and right (mirrored: negative rod length)
    for (k, (xt, rd, ph)) in [(x_ltdc, rod, 0.0f32), (x_rtdc, -rod, lead)].into_iter().enumerate() {
        let head0 = piston_head_cell(xt as f32, stroke as f32 * if k == 0 { 1.0 } else { -1.0 }, rd.abs(), ph)
            .max(i32::MIN);
        let _ = head0;
        let sg = if k == 0 { 1.0 } else { -1.0 };
        let r = 0.5 * stroke as f32;
        let sn = ph.sin();
        let d = r * (1.0 - ph.cos()) + rd.abs() - (rd.abs() * rd.abs() - r * r * sn * sn).max(0.0).sqrt();
        let head = (xt as f32 - sg * d + 0.5).floor() as i32;
        let (xa, xb) = if k == 0 { (head - thick + 1, head) } else { (head, head + thick - 1) };
        for x in xa..=xb {
            for y in y0..y1 {
                s.put(x, y, PISTON, k as u32, 0);
            }
        }
        let b = W_PIST + k * 8;
        words[b] = fw(xt as f32);
        words[b + 1] = fw(stroke as f32);
        words[b + 2] = fw(rd);
        words[b + 3] = fw(ph);
        words[b + 4] = y0 as u32;
        words[b + 5] = bore as u32;
        words[b + 6] = head as u32;
        words[b + 7] = head as u32;
    }
    // ports: intake (top wall) just above the left piston's BDC, exhaust (bottom wall) opening earlier at the right end
    let pw = bore * 9 / 20;
    let ix0 = x_lbdc + 1;
    let ex1 = x_rbdc - 1;
    let ex_start = x_rbdc - stroke * 30 / 100 - pw;
    let _ = ex1;
    for x in ix0..ix0 + pw {
        for t in 0..wt {
            s.put(x, y0 - 1 - t, 0, 0, 0);
        }
        s.put(x, y0 - wt, PREMIX, 9, 0);
    }
    for x in ex_start..ex_start + pw {
        for t in 0..wt {
            s.put(x, y1 + t, 0, 0, 0);
        }
        s.put(x, y1 + wt - 1, VENT, 0, 0);
    }
    // spark plug in the barrel wall at mid-chamber (or a glow-heated wall for the diesel variant)
    for x in [xc - 1, xc] {
        s.put(x, y0 - 1, SPARK, 0, 0);
    }
    let sb = W_SPARK;
    let (on, off) = if diesel { (0.0, 0.0) } else { (338.0f32.to_radians(), 350.0f32.to_radians()) };
    words[sb] = fw(on);
    words[sb + 1] = fw(off);
    words[sb + 2] = fw(0.0);
    s.inlet_p = 3.0;
    s.o2_p = 3.0;
    s.p_air = 1.6;
    s.engine = words;
    s
}

// ---- presets for the template browser and the bench ----

fn otto1() -> Scene {
    EngineParams::otto().build()
}
fn otto4() -> Scene {
    EngineParams::compact().with_cylinders(4).build()
}
fn otto2() -> Scene {
    EngineParams::otto().with_cylinders(2).build()
}
fn high_comp() -> Scene {
    EngineParams { clearance: 6, ..EngineParams::otto() }.build()
}
fn lean_burn() -> Scene {
    EngineParams { phi: 0.7, clearance: 6, ..EngineParams::otto() }.build()
}
fn rich_race() -> Scene {
    EngineParams { phi: 1.15, rev_units: 420.0, ign_start: 330.0, ..EngineParams::otto() }.build()
}
fn turbo() -> Scene {
    EngineParams { boost: 1.8, clearance: 12, ..EngineParams::otto() }.build()
}
fn atkinson() -> Scene {
    EngineParams { ivc: 300.0, clearance: 6, ign_start: 352.0, ..EngineParams::otto() }.build()
}
fn opposed_otto() -> Scene {
    opposed(false)
}
fn diesel1() -> Scene {
    EngineParams::diesel().build()
}
fn diesel3() -> Scene {
    EngineParams { bore: 30, stroke: 56, rod: 108.0, clearance: 4, rev_units: 1100.0, wall: 12, ..EngineParams::diesel() }.with_cylinders(3).build()
}
fn two1() -> Scene {
    EngineParams::two_stroke().build()
}
fn two2() -> Scene {
    EngineParams::two_stroke().with_cylinders(2).build()
}

pub const PRESETS: [(&str, &str, &str, fn() -> Scene); 13] = [
    ("eng_otto1", "Gasoline engine, single cylinder", "Four-stroke spark-ignition engine: intake, compression, spark, power, exhaust. Compression ratio 10:1; the piston, valves and spark follow crank-angle timing and the gas does the rest.", otto1),
    ("eng_otto2", "Gasoline engine, parallel twin", "Two cylinders 360 degrees apart (even firing).", otto2),
    ("eng_otto4", "Gasoline engine, inline four", "Four cylinders firing 1-3-4-2, 180 degrees apart: smooth power overlap. Change the intake pressure (Air feed) to open or close the throttle.", otto4),
    ("eng_highcomp", "High-compression gasoline engine", "Compression ratio 13:1: more efficient, but the end gas gets hot enough to knock if the mixture is rich.", high_comp),
    ("eng_lean", "Lean-burn gasoline engine", "Equivalence ratio 0.7 at high compression: cooler, cleaner, slower flame.", lean_burn),
    ("eng_race", "Rich high-revving gasoline engine", "Rich mixture (phi 1.15) at about 7000 rpm equivalent with advanced spark.", rich_race),
    ("eng_turbo", "Turbocharged gasoline engine", "Intake charge at 1.8 atm with a lower compression ratio (7:1) to keep knock away: about 1.8 times the mass per cycle, so a much stronger power stroke.", turbo),
    ("eng_atkinson", "Atkinson-cycle engine", "The intake valve stays open far into the compression stroke and pushes part of the charge back out: a smaller effective compression with the full expansion stroke, efficient but weak.", atkinson),
    ("eng_opposed", "Opposed-piston two-stroke", "Two pistons face each other in one cylinder with no head. The left piston uncovers the intake ports, the right one the exhaust ports and leads it: the cylinder blows down, is scavenged in one direction and both pistons compress the charge between them.", opposed_otto),
    ("eng_diesel1", "Diesel engine, single cylinder", "Air only is compressed to about 45 atm, fuel is injected near top dead centre and ignites from the compression heat and a glow plug. No spark.", diesel1),
    ("eng_diesel3", "Diesel engine, three cylinders", "Three cylinders 240 degrees apart.", diesel3),
    ("eng_two1", "Two-stroke engine, single cylinder", "Ported two-stroke with a blower-fed intake port and an exhaust port that opens earlier; a power stroke every revolution.", two1),
    ("eng_two2", "Two-stroke engine, twin", "Two ported cylinders 180 degrees apart.", two2),
];

pub fn by_name(name: &str) -> Option<Scene> {
    PRESETS.iter().find(|p| p.0 == name).map(|p| (p.3)())
}
