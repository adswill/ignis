//! Rocket-engine demos (propulsion team, rocket section). Built only through the public `Scene` API.
//! Grid 512 x 288 cells of ~1 cm, y grows downwards, all engines fire towards +x (thrust shows as negative x force).
//! Real structure rules that keep these alive: titanium jacket >= 12 cells, cooled-copper liner on every wall cell
//! within ~2.5 cells of gas, igniter at the injector face (see `Scene::rocket`).
#![allow(dead_code)]

use crate::gpu::{Op, H, W};
use crate::scene::{Scene, AIR_IN, FUEL_IN, IGNITER, INCONEL, MATTER, O2_IN, STEEL, WALL};

const TITANIUM: u32 = 3;
const COOLED_CU: u32 = 6;
const COPPER: u32 = 2;

type Ctor = fn() -> Scene;
/// (bench name, title, one-line description, constructor) — the single source for `NAMES` and `all()`.
const TABLE: [(&str, &str, &str, Ctor); 12] = [
    ("rk_aerospike", "Bell vs aerospike", "Same 12 atm chamber and throat area; the bell (area ratio 4) over-expands at sea level, the linear aerospike's free boundary adapts to ambient and hugs the spike", aerospike),
    ("rk_expansion", "Nozzle expansion: under / ideal / over", "Same throat and 12 atm chamber, exit/throat area ratio 1.5, 2.5 and 4.5: Mach-disk jet, ideal shock-diamond jet, internal shock with flow separation", expansion),
    ("rk_dualbell", "Dual-bell nozzle", "Base bell for sea level (area ratio 2.5) plus an extension (ratio 7) behind a wall inflection: at 1 atm the jet detaches at the kink", dual_bell),
    ("rk_srm_core", "Core-burning (BATES) motor", "Three cast grain segments with a through bore and exposed ends in a lined case, head plate open to the bore: fast, high-thrust burn with measurable thrust", srm_core),
    ("rk_srm_star", "Grain cross-sections: circular bore vs star", "End-on slices of the same case: a round bore (burning perimeter grows, progressive) beside a 6-point star bore (perimeter roughly constant, neutral)", srm_grains),
    ("rk_hybrid", "Hybrid rocket: paraffin and oxygen", "Gaseous oxygen is injected down the port of a paraffin wax grain; the wax melts, vaporises and burns in a boundary-layer diffusion flame, finishing in an empty post-combustion chamber", hybrid),
    ("rk_monoprop", "Monopropellant thruster: 90% peroxide", "Hydrogen peroxide drips through a 1-cell feed onto a manganese-dioxide catalyst pack and decomposes to steam and oxygen at ~1000 K: a hot but flameless exhaust through a cooled nozzle", monoprop),
    ("rk_tanks", "Pressure-fed bipropellant with finite tanks", "Sealed warm-gas pressurised fuel and oxidiser tanks (about 4 atm) feed an oxidiser core and fuel wall film through lines: no pinned inlets, so thrust decays as the tanks blow down and the flame goes out near 3 atm", tanks),
    ("rk_coldgas", "Cold-gas thruster", "Pure oxygen at 8 atm expands through a converging-diverging micro nozzle (area ratio 1.7, ideal for 1 atm): no combustion, a clean supersonic jet with shock cells", coldgas),
    ("rk_rcs", "RCS thruster pair", "Two cold-gas thrusters on opposite faces and opposite ends of a small body fire in opposite directions: zero net force, a pure couple that would spin it", rcs),
    ("rk_expander", "Regeneratively cooled engine", "Fuel enters at the nozzle exit, flows up a 3-cell cooling channel hugging the cooled-copper liner, and is injected into the chamber at the head: the visible jacket layer is the cooling circuit", expander),
    ("rk_srm_end", "End-burning solid motor", "Cast KNO3/sugar end-burner in a cooled case: one flat burning face, constant surface area, steady chamber pressure and thrust", srm_end),
];

pub const NAMES: [&str; TABLE.len()] = {
    let mut a = [""; TABLE.len()];
    let mut i = 0;
    while i < TABLE.len() {
        a[i] = TABLE[i].0;
        i += 1;
    }
    a
};

pub fn all() -> Vec<(&'static str, &'static str, fn() -> Scene)> {
    TABLE.iter().map(|e| (e.1, e.2, e.3)).collect()
}

// ------------------------------------------------------------------------------------------------
// helpers

fn fill(s: &mut Scene, x0: i32, y0: i32, x1: i32, y1: i32, tile: u32, mat: u32) {
    for x in x0.min(x1)..=x0.max(x1) {
        for y in y0.min(y1)..=y0.max(y1) {
            s.put(x, y, tile, mat, 0);
        }
    }
}

fn op(kind: u32, shape: u32, x0: i32, y0: i32, x1: i32, y1: i32, r: i32) -> Op {
    Op { kind, shape, x0, y0, x1, y1, r, tile: 0, mat: 0, dir: 0, _p: [0; 2] }
}

fn tile_at(s: &Scene, x: i32, y: i32) -> u32 {
    if x < 0 || y < 0 || x >= W as i32 || y >= H as i32 {
        return 1;
    }
    s.meta[(y as u32 * W + x as u32) as usize] & 0xff
}

/// Regenerative-cooling layer: every wall cell within ~2.5 cells of open gas / an inlet becomes cooled copper, the rest
/// of the jacket stays titanium. Only touches wall tiles inside the box (so tanks etc. outside keep their material).
fn liner(s: &mut Scene, x0: i32, y0: i32, x1: i32, y1: i32) {
    let snap = s.meta.clone();
    let w = W as i32;
    for y in y0.max(0)..=y1.min(H as i32 - 1) {
        for x in x0.max(0)..=x1.min(w - 1) {
            if (snap[(y * w + x) as usize] & 0xff) != WALL {
                continue;
            }
            let mut near = false;
            'scan: for dy in -2..=2i32 {
                for dx in -2..=2i32 {
                    if dx * dx + dy * dy > 6 {
                        continue;
                    }
                    let (xx, yy) = (x + dx, y + dy);
                    if xx >= 0 && yy >= 0 && xx < w && yy < H as i32 {
                        let t = snap[(yy * w + xx) as usize] & 0xff;
                        if t == 0 || t == FUEL_IN || t == O2_IN || t == AIR_IN {
                            near = true;
                            break 'scan;
                        }
                    }
                }
            }
            s.meta[(y * w + x) as usize] = WALL | ((if near { COOLED_CU } else { TITANIUM }) << 8);
        }
    }
}

/// Convergent-divergent planar bell. Chamber of half height `hc` from `x_face` to `x_conv`, sinusoidal contraction to the
/// throat (half `ht`) at `x_th`, parabolic bell (t^0.6) to half `he` at `x_exit`. Returns the gas half-height function.
#[derive(Clone, Copy)]
struct Nozzle {
    cy: i32,
    x_face: i32,
    x_conv: i32,
    x_th: i32,
    x_exit: i32,
    hc: f32,
    ht: f32,
    he: f32,
    /// dual-bell: (x of the inflection, half height there, half height at the exit)
    dual: Option<(i32, f32, f32)>,
}

impl Nozzle {
    fn half(&self, x: i32) -> i32 {
        if x < self.x_conv {
            self.hc.round() as i32
        } else if x < self.x_th {
            let t = (x - self.x_conv) as f32 / (self.x_th - self.x_conv) as f32;
            (self.hc - (self.hc - self.ht) * (t * std::f32::consts::FRAC_PI_2).sin()).round() as i32
        } else if let Some((xi, h1, h2)) = self.dual {
            if x < xi {
                let t = (x - self.x_th) as f32 / (xi - self.x_th) as f32;
                (self.ht + (h1 - self.ht) * t.powf(0.6)).round() as i32
            } else {
                let t = (x - xi) as f32 / (self.x_exit - xi) as f32;
                (h1 + (h2 - h1) * t.powf(0.5)).round() as i32
            }
        } else {
            let t = (x - self.x_th) as f32 / (self.x_exit - self.x_th) as f32;
            (self.ht + (self.he - self.ht) * t.powf(0.6)).round() as i32
        }
    }
}

/// Wall + injector + igniters for one bipropellant engine. `fuel_every`: one fuel row per `fuel_every` rows (rest O2).
fn bell_engine(s: &mut Scene, n: &Nozzle, jacket: i32, fuel_every: i32) {
    for x in n.x_face..=n.x_exit {
        let h = n.half(x);
        for t in 0..jacket {
            s.put(x, n.cy - h - 1 - t, WALL, TITANIUM, 0);
            s.put(x, n.cy + h + t, WALL, TITANIUM, 0);
        }
    }
    let hc = n.hc.round() as i32;
    fill(s, n.x_face - 5, n.cy - hc - jacket, n.x_face - 1, n.cy + hc + jacket - 1, WALL, STEEL);
    for y in n.cy - hc..n.cy + hc {
        let t = if (y - n.cy).rem_euclid(fuel_every) == 0 { FUEL_IN } else { O2_IN };
        s.put(n.x_face, y, t, 0, 0);
    }
    for y in n.cy - 2..=n.cy + 1 {
        s.put(n.x_face, y, IGNITER, 0, 0);
    }
    for x in n.x_face + 3..n.x_face + 6 {
        s.put(x, n.cy - hc - 1, IGNITER, 0, 0);
        s.put(x, n.cy + hc, IGNITER, 0, 0);
    }
}

// ------------------------------------------------------------------------------------------------
// 2. under- / ideally / over-expanded nozzles

/// Per-engine measurement hook for multi-engine scenes (`Some(k)` would build only engine k); off in the shipped scenes.
fn only() -> Option<usize> {
    None
}

pub fn expansion() -> Scene {
    let mut s = Scene::empty();
    // 12 atm chamber, gamma ~1.2, 1 atm ambient: ideal planar Ae/At = 2.5 (Me 2.27); 1.5 is under-expanded, 4.5 over-expanded
    let ht = 4.0;
    for (k, (cy, ratio)) in [(40, 1.5f32), (146, 2.5), (250, 4.5)].into_iter().enumerate() {
        if only().map_or(false, |o| o != k) {
            continue;
        }
        let n = Nozzle { cy, x_face: 50, x_conv: 110, x_th: 135, x_exit: 205, hc: 14.0, ht, he: ht * ratio, dual: None };
        bell_engine(&mut s, &n, 12, 4);
    }
    liner(&mut s, 0, 0, W as i32 - 1, H as i32 - 1);
    s.inlet_p = 12.0;
    s.o2_p = 12.0;
    s
}

// ------------------------------------------------------------------------------------------------
// 1. bell vs linear aerospike, same chamber pressure, same throat area, same ambient

/// Planar (linear) aerospike: two chamber channels either side of a cooled spike, a throat slot of `gt` cells each side
/// (total throat area 2 gt = the bell's 2 ht), flow turning round the lip then expanding on the ramp (truncated at a 6-cell base).
fn aerospike_engine(s: &mut Scene, cy: i32, x_face: i32) {
    let (hs0, ch, gt, jacket) = (12.0f32, 14.0f32, 4.0f32, 12);
    let (x_conv, x_th, x_tail) = (x_face + 55, x_face + 85, x_face + 85 + 80);
    // gas-side outer (cowl) radius and spike surface
    let cowl = |x: i32| -> f32 {
        if x < x_conv {
            hs0 + ch
        } else if x < x_th {
            let t = (x - x_conv) as f32 / (x_th - x_conv) as f32;
            hs0 + ch - (ch - gt) * t.powf(1.4)
        } else {
            1e9
        }
    };
    let spike = |x: i32| -> f32 {
        if x <= x_th {
            hs0
        } else {
            let t = ((x - x_th) as f32 / (x_tail - x_th) as f32).min(1.0);
            (hs0 * (1.0 - t).powf(2.0)).max(3.0)
        }
    };
    for x in x_face..=x_tail {
        let sh = spike(x).round() as i32;
        for y in cy - sh..cy + sh {
            s.put(x, y, WALL, TITANIUM, 0);
        }
        if x <= x_th + 12 {
            let c = if x <= x_th { cowl(x).round() as i32 } else { gt.round() as i32 + hs0 as i32 };
            let th = if x <= x_th { jacket } else { (jacket as f32 * (1.0 - (x - x_th) as f32 / 18.0)).round().max(6.0) as i32 };
            for t in 0..th {
                s.put(x, cy - c - 1 - t, WALL, TITANIUM, 0);
                s.put(x, cy + c + t, WALL, TITANIUM, 0);
            }
        }
    }
    let top = (hs0 + ch) as i32 + jacket;
    fill(s, x_face - 5, cy - top, x_face - 1, cy + top - 1, WALL, STEEL);
    for y in cy - (hs0 + ch) as i32..cy + (hs0 + ch) as i32 {
        if (y - cy).abs() < hs0 as i32 {
            continue;
        }
        let t = if (y - cy).rem_euclid(4) == 0 { FUEL_IN } else { O2_IN };
        s.put(x_face, y, t, 0, 0);
    }
    for sgn in [-1, 1] {
        let yc = cy + sgn * (hs0 as i32 + 7);
        for y in yc - 2..=yc + 1 {
            s.put(x_face, y, IGNITER, 0, 0);
        }
        let yo = if sgn < 0 { cy - (hs0 + ch) as i32 - 1 } else { cy + (hs0 + ch) as i32 };
        for x in x_face + 3..x_face + 6 {
            s.put(x, yo, IGNITER, 0, 0);
        }
    }
}

pub fn aerospike() -> Scene {
    let mut s = Scene::empty();
    if only() != Some(1) {
        let n = Nozzle { cy: 62, x_face: 50, x_conv: 110, x_th: 135, x_exit: 205, hc: 14.0, ht: 4.0, he: 16.0, dual: None };
        bell_engine(&mut s, &n, 12, 4);
    }
    if only() != Some(0) {
        aerospike_engine(&mut s, 214, 50);
    }
    liner(&mut s, 0, 0, W as i32 - 1, H as i32 - 1);
    // the cowl lip is attacked from three sides at the throat: inconel there instead of thin cooled copper
    for x in 128..=150i32 {
        for y in 150..280i32 {
            if (y - 214).abs() > 13 && s.meta[(y as u32 * W + x as u32) as usize] == (WALL | (COOLED_CU << 8)) {
                s.put(x, y, WALL, INCONEL, 0);
            }
        }
    }
    s.inlet_p = 12.0;
    s.o2_p = 12.0;
    s
}

// ------------------------------------------------------------------------------------------------
// 3. dual-bell nozzle

pub fn dual_bell() -> Scene {
    let mut s = Scene::empty();
    // throat half 4; base bell to half 10 (ratio 2.5, ideal at 12 atm / 1 atm) at x=175, extension to half 28 (ratio 7)
    let n = Nozzle { cy: 144, x_face: 50, x_conv: 110, x_th: 135, x_exit: 235, hc: 14.0, ht: 4.0, he: 28.0, dual: Some((175, 10.0, 28.0)) };
    bell_engine(&mut s, &n, 12, 4);
    liner(&mut s, 0, 0, W as i32 - 1, H as i32 - 1);
    s.inlet_p = 12.0;
    s.o2_p = 12.0;
    s
}

// ------------------------------------------------------------------------------------------------
// 4. solid motors (cast KNO3/sugar grains burn only at the exposed surface, r = a p^n)

const KNO3: u32 = 13;
const SUGAR: u32 = 14;
const SORBITOL: u32 = 74;
const NANO3: u32 = 70;

/// KNO3 / fuel cast grain cell (65:35 by cell fraction ~ 65:35 by mass for rocket candy).
fn grain_cell(s: &mut Scene, x: i32, y: i32, fuel: u32) {
    grain_mix(s, x, y, KNO3, fuel);
}

fn grain_mix(s: &mut Scene, x: i32, y: i32, ox: u32, fuel: u32) {
    let h = ((x as u32).wrapping_mul(73856093) ^ (y as u32).wrapping_mul(19349663)) % 100;
    s.put(x, y, MATTER, if h < 65 { ox } else { fuel }, 1);
}

fn torch(s: &mut Scene, x: i32, y: i32, r: i32) {
    s.ops.push(op(5, 0, x, y, x, y, r));
}

/// Wall tube following `n.half`: 2-cell cooled-copper liner (insulation stand-in) inside a 12-cell titanium jacket.
fn case_walls(s: &mut Scene, n: &Nozzle) {
    for x in n.x_face..=n.x_exit {
        let h = n.half(x);
        for t in 0..12 {
            let m = if t < 2 { COOLED_CU } else { TITANIUM };
            s.put(x, n.cy - h - 1 - t, WALL, m, 0);
            s.put(x, n.cy + h + t, WALL, m, 0);
        }
    }
}

/// End-burning motor: the grain fills the case from the head end to `x_e`; one flat burning face moves towards the head.
/// (The thrust readout cannot see the load carried through the grain into the head, so only chamber pressure is meaningful.)
pub fn srm_end() -> Scene {
    let mut s = Scene::empty();
    let cy = 144;
    let hh = 12; // case inner half height
    let (x_h, x_g, x_e) = (66, 76, 161); // head plate, grain start, grain end (85 cells of propellant)
    let n = Nozzle { cy, x_face: x_h, x_conv: 188, x_th: 206, x_exit: 250, hc: hh as f32, ht: 8.0, he: 18.0, dual: None };
    case_walls(&mut s, &n);
    fill(&mut s, x_h, cy - hh - 12, x_g - 1, cy + hh + 11, WALL, TITANIUM);
    fill(&mut s, x_g - 2, cy - hh, x_g - 1, cy + hh - 1, WALL, COOLED_CU);
    for x in x_g..x_e {
        for y in cy - hh..cy + hh {
            grain_cell(&mut s, x, y, SUGAR);
        }
    }
    torch(&mut s, x_e + 3, cy, 4);
    s
}

/// Core-burning motor, longitudinal slice: three grain segments (BATES-style, outer surface inhibited by the liner)
/// with a bore through all of them. The head plate is open to the bore, so the thrust readout sees the head load.
pub fn srm_core() -> Scene {
    let mut s = Scene::empty();
    let cy = 144;
    let hh = 42; // case inner half height
    let (x_h, x_g) = (66, 76);
    let len = 24;
    let x_e = x_g + len;
    let bore = 14;
    let n = Nozzle { cy, x_face: x_h, x_conv: x_e + 14, x_th: x_e + 44, x_exit: x_e + 100, hc: hh as f32, ht: 28.0, he: 38.0, dual: None };
    case_walls(&mut s, &n);
    fill(&mut s, x_h, cy - hh - 12, x_g - 1, cy + hh + 11, WALL, TITANIUM);
    fill(&mut s, x_h + 6, cy - hh, x_g - 1, cy - bore - 1, WALL, COOLED_CU);
    fill(&mut s, x_h + 6, cy + bore, x_g - 1, cy + hh - 1, WALL, COOLED_CU);
    for x in x_g..x_e {
        for y in cy - hh..cy + hh {
            if (y - cy).abs() >= bore {
                grain_mix(&mut s, x, y, NANO3, SORBITOL);
            }
        }
    }
    for k in 0..3 {
        torch(&mut s, x_g + 4 + 8 * k, cy, 8);
    }
    s
}

/// Radial-slot cross-section motor: grain (cast KNO3/sugar/sorbitol) between a bore and a phenolic-lined steel ring, a straight
/// exit slot through the +x side. `bore(theta)` is the bore radius at that polar angle.
fn cross_motor(s: &mut Scene, cx: i32, cy: i32, bore: &dyn Fn(f32) -> f32) {
    let (r_grain, r_liner, r_case) = (33.0f32, 35.0f32, 44.0f32);
    let half = 15; // exit slot half width (throat 30 cells)
    for x in cx - 46..=cx + 46 {
        for y in cy - 46..=cy + 46 {
            let (dx, dy) = ((x - cx) as f32, (y - cy) as f32);
            let r = (dx * dx + dy * dy).sqrt();
            let th = dy.atan2(dx);
            if r < bore(th) {
                continue;
            }
            let slot = dx > 0.0 && dy.abs() < (half + 2) as f32;
            if slot && dy.abs() < half as f32 {
                continue;
            }
            if slot && r <= r_case + 0.5 {
                s.put(x, y, MATTER, if r > r_grain { 85 } else { 87 }, 0);
            } else if r < r_grain {
                grain_mix(s, x, y, NANO3, if ((x * 7 + y * 13) % 5) < 3 { SORBITOL } else { 67 });
            } else if r < r_liner {
                s.put(x, y, MATTER, 87, 0);
            } else if r < r_case {
                s.put(x, y, WALL, TITANIUM, 0);
            }
        }
    }
}

pub fn srm_grains() -> Scene {
    let mut s = Scene::empty();
    let circ = |_t: f32| 14.0;
    // 6-point star: tip radius 24, valley radius 9
    let star = |t: f32| {
        let f = (t * 6.0 / std::f32::consts::TAU).rem_euclid(1.0);
        let w = 1.0 - (2.0 * f - 1.0).abs();
        9.0 + 15.0 * w
    };
    if only() != Some(1) {
        cross_motor(&mut s, 170, 76, &circ);
        torch(&mut s, 160, 76, 3);
    }
    if only() != Some(0) {
        cross_motor(&mut s, 170, 214, &star);
        torch(&mut s, 160, 214, 3);
    }
    s
}

// ------------------------------------------------------------------------------------------------
// 5. hybrid rocket

const WAX: u32 = 35;

/// Paraffin hybrid: O2 injected through the head, wax grain with a central port, post-combustion chamber, nozzle.
pub fn hybrid() -> Scene {
    let mut s = Scene::empty();
    let cy = 144;
    let (hh, port) = (18, 6);
    let (x_h, x_g) = (60, 80);
    let len = 90;
    let x_e = x_g + len;
    let n = Nozzle { cy, x_face: x_h, x_conv: x_e + 16, x_th: x_e + 38, x_exit: x_e + 85, hc: hh as f32, ht: 5.0, he: 12.0, dual: None };
    case_walls(&mut s, &n);
    fill(&mut s, x_h, cy - hh - 12, x_h + 4, cy + hh + 11, WALL, TITANIUM);
    // injector face: the O2 row only covers the port
    for y in cy - port..cy + port {
        s.put(x_h + 5, y, O2_IN, 0, 0);
    }
    fill(&mut s, x_h + 5, cy - hh, x_h + 5, cy - port - 1, WALL, COOLED_CU);
    fill(&mut s, x_h + 5, cy + port, x_h + 5, cy + hh - 1, WALL, COOLED_CU);
    for x in x_g..x_e {
        for y in cy - hh..cy + hh {
            if (y - cy).abs() >= port {
                s.put(x, y, MATTER, WAX, 0);
            }
        }
    }
    s.o2_p = 10.0;
    s.inlet_p = 1.0;
    torch(&mut s, x_g + 6, cy, 5);
    s
}

// ------------------------------------------------------------------------------------------------
// 6. monopropellant (H2O2 over a catalyst pack); fires downwards (gravity feeds the liquid)

const HTP: u32 = 45;
const MNO2: u32 = 46;

pub fn monoprop() -> Scene {
    let mut s = Scene::empty();
    let cx = 256;
    // propellant tank on top (steel), narrow feed tube
    let (t_top, t_bot) = (14, 50);
    for x in cx - 22..=cx + 22 {
        for y in t_top..=t_bot + 12 {
            let inside = x > cx - 19 && x < cx + 19 && y > t_top + 2 && y <= t_bot;
            if !inside {
                s.put(x, y, WALL, STEEL, 0);
            }
        }
    }
    fill(&mut s, cx - 16, t_top + 3, cx + 16, t_bot, MATTER, HTP);
    // 1-cell feed hole through the tank floor
    for y in t_bot + 1..=t_bot + 12 {
        s.put(cx, y, 0, 0, 0);
    }
    // chamber: half 24, y from 62 to 160; throat half 8 at y=190; bell to half 13 at y=230
    let half = |y: i32| -> i32 {
        if y < 150 {
            24
        } else if y < 184 {
            let t = (y - 150) as f32 / 34.0;
            (24.0 - 16.0 * (t * std::f32::consts::FRAC_PI_2).sin()).round() as i32
        } else {
            let t = (y - 184) as f32 / 46.0;
            (8.0 + 5.0 * t.powf(0.6)).round() as i32
        }
    };
    for y in t_bot + 13..=230 {
        let h = if y < t_bot + 13 + 1 { 1 } else { half(y).max(1) };
        for t in 0..12 {
            s.put(cx - h - 1 - t, y, WALL, TITANIUM, 0);
            s.put(cx + h + t, y, WALL, TITANIUM, 0);
        }
    }
    // cap joining the tank floor and the chamber walls
    fill(&mut s, cx - 38, t_bot + 1, cx - 1, t_bot + 12, WALL, TITANIUM);
    fill(&mut s, cx + 1, t_bot + 1, cx + 38, t_bot + 12, WALL, TITANIUM);
    // catalyst pack: a screen shelf with MnO2 on top (gas passes through the side gaps)
    liner(&mut s, cx - 60, t_bot + 13, cx + 60, 235);
    fill(&mut s, cx - 15, 112, cx + 15, 115, WALL, INCONEL);
    fill(&mut s, cx - 15, 105, cx - 13, 111, WALL, INCONEL);
    fill(&mut s, cx + 13, 105, cx + 15, 111, WALL, INCONEL);
    fill(&mut s, cx - 12, 106, cx + 12, 111, MATTER, MNO2);
    s
}

// ------------------------------------------------------------------------------------------------
// 7. pressure-fed engine with finite tanks (blowdown)

fn gas_box(s: &mut Scene, x0: i32, y0: i32, x1: i32, y1: i32, boxes: &mut Vec<(i32, i32, i32, i32)>) {
    fill(s, x0, y0, x1, y1, 0, 0);
    boxes.push((x0, y0, x1, y1));
}

pub fn tanks() -> Scene {
    let mut s = Scene::empty();
    let cy = 144;
    let x_face = 170;
    // one big titanium block, then carve the gas volumes out of it
    fill(&mut s, 14, cy - 92, x_face - 1, cy + 91, WALL, TITANIUM);
    let mut ox: Vec<(i32, i32, i32, i32)> = vec![];
    let mut fu: Vec<(i32, i32, i32, i32)> = vec![];
    gas_box(&mut s, 20, cy - 28, 100, cy + 27, &mut ox); // ox tank
    gas_box(&mut s, 101, cy - 6, x_face - 1, cy + 5, &mut ox); // ox line, opens into the chamber centre
    for sgn in [-1i32, 1] {
        let (a, b) = if sgn < 0 { (cy - 88, cy - 49) } else { (cy + 48, cy + 87) };
        gas_box(&mut s, 30, a, 109, b, &mut fu); // fuel tank
        if sgn < 0 {
            gas_box(&mut s, 106, cy - 48, 109, cy - 14, &mut fu); // riser
            gas_box(&mut s, 110, cy - 13, x_face - 1, cy - 10, &mut fu); // fuel line, opens at the chamber wall
        } else {
            gas_box(&mut s, 106, cy + 13, 109, cy + 47, &mut fu);
            gas_box(&mut s, 110, cy + 9, x_face - 1, cy + 12, &mut fu);
        }
    }
    // chamber and nozzle (planar, hc 14, throat 4, exit 6)
    let n = Nozzle { cy, x_face, x_conv: 215, x_th: 235, x_exit: 305, hc: 14.0, ht: 4.0, he: 6.5, dual: None };
    for x in x_face..=n.x_exit {
        let h = n.half(x);
        for t in 0..12 {
            s.put(x, cy - h - 1 - t, WALL, TITANIUM, 0);
            s.put(x, cy + h + t, WALL, TITANIUM, 0);
        }
    }
    // igniters on the head face between the oxidiser core and the fuel film
    for y in [cy - 9, cy - 8, cy + 6, cy + 7] {
        s.put(x_face, y, IGNITER, 0, 0);
    }
    liner(&mut s, x_face - 8, 0, W as i32 - 1, H as i32 - 1);
    // fill: evacuate the lines/tanks, add propellant gas, heat it (warm-gas pressurisation) and let it cool to ~1200 K (hotter fuel pyrolyses to soot in the tank)
    s.ops.push(op(7, 1, 14, cy - 92, x_face - 1, cy + 91, 0));
    for _ in 0..21 {
        s.ops.push(op(7, 1, 14, cy - 92, x_face - 1, cy + 91, 0));
    }
    for _ in 0..12 {
        for &(a, b, c, d) in &fu {
            s.ops.push(op(2, 1, a, b, c, d, 0));
        }
    }
    for _ in 0..16 {
        for &(a, b, c, d) in &ox {
            s.ops.push(op(3, 1, a, b, c, d, 0));
        }
    }
    for &(a, b, c, d) in fu.iter().chain(ox.iter()) {
        s.ops.push(op(5, 1, a, b, c, d, 0));
    }
    for k in 0..11 {
        for &(a, b, c, d) in fu.iter().chain(if k < 6 { ox.iter() } else { [].iter() }) {
            s.ops.push(op(6, 1, a, b, c, d, 0));
        }
    }
    s
}

// ------------------------------------------------------------------------------------------------
// 8. cold-gas thruster and RCS pair

/// Micro nozzle along y. `dir` -1 fires up (towards smaller y), +1 down. Plenum half width `hp`, throat `ht`, exit `he`;
/// the wall spans y_in (inside the body, converging starts) .. y_in + dir * (lc + ld).
fn y_jet(s: &mut Scene, x: i32, y_in: i32, dir: i32, hp: f32, ht: f32, he: f32, lc: i32, ld: i32) {
    for k in 0..=lc + ld {
        let h = if k < lc {
            let t = k as f32 / lc as f32;
            hp - (hp - ht) * (t * std::f32::consts::FRAC_PI_2).sin()
        } else {
            let t = (k - lc) as f32 / ld as f32;
            ht + (he - ht) * t.powf(0.6)
        };
        let h = h.round() as i32;
        let y = y_in + dir * k;
        for t in 0..10 {
            s.put(x - h - 1 - t, y, WALL, TITANIUM, 0);
            s.put(x + h + t, y, WALL, TITANIUM, 0);
        }
    }
}

/// Hollow body (6-cell titanium shell) with O2 feed blocks in the middle; interior gas is the plenum for the thrusters.
fn gas_body(s: &mut Scene, x0: i32, y0: i32, x1: i32, y1: i32) {
    fill(s, x0, y0, x1, y1, WALL, TITANIUM);
    fill(s, x0 + 6, y0 + 6, x1 - 6, y1 - 6, 0, 0);
    let xm = (x0 + x1) / 2;
    fill(s, xm - 6, (y0 + y1) / 2 - 4, xm + 6, (y0 + y1) / 2 + 4, O2_IN, 0);
}

pub fn coldgas() -> Scene {
    let mut s = Scene::empty();
    let cy = 144;
    // plenum fed by an O2 inlet block, nozzle along +x: half 8 -> throat 4 -> exit 6.8 (area ratio 1.7)
    let n = Nozzle { cy, x_face: 80, x_conv: 130, x_th: 150, x_exit: 215, hc: 14.0, ht: 4.0, he: 6.8, dual: None };
    for x in n.x_face..=n.x_exit {
        let h = n.half(x);
        for t in 0..12 {
            s.put(x, cy - h - 1 - t, WALL, TITANIUM, 0);
            s.put(x, cy + h + t, WALL, TITANIUM, 0);
        }
    }
    fill(&mut s, n.x_face - 6, cy - 26, n.x_face - 1, cy + 25, WALL, TITANIUM);
    fill(&mut s, n.x_face, cy - 5, n.x_face + 8, cy + 4, O2_IN, 0);
    s.o2_p = 8.0;
    s.inlet_p = 1.0;
    s
}

pub fn rcs() -> Scene {
    let mut s = Scene::empty();
    let (x0, y0, x1, y1) = (170, 120, 340, 168);
    gas_body(&mut s, x0, y0, x1, y1);
    // upper thruster at the left end fires up, lower thruster at the right end fires down
    y_jet(&mut s, x0 + 36, y0 + 8, -1, 12.0, 3.0, 5.1, 10, 26);
    y_jet(&mut s, x1 - 36, y1 - 8, 1, 12.0, 3.0, 5.1, 10, 26);
    // open the shell where the thrusters attach
    fill(&mut s, x0 + 36 - 12, y0, x0 + 36 + 11, y0 + 7, 0, 0);
    fill(&mut s, x1 - 36 - 12, y1 - 7, x1 - 36 + 11, y1, 0, 0);
    s.o2_p = 8.0;
    s.inlet_p = 1.0;
    s
}

// ------------------------------------------------------------------------------------------------
// 9. regeneratively cooled engine (fuel circulates through the nozzle jacket before injection)

pub fn expander() -> Scene {
    let mut s = Scene::empty();
    let cy = 144;
    let x_face = 96;
    let n = Nozzle { cy, x_face, x_conv: 165, x_th: 195, x_exit: 280, hc: 16.0, ht: 5.0, he: 12.5, dual: None };
    let hc = n.hc as i32;
    // wall layers outward from the gas: 3 cooled-copper liner, 8 titanium, 3-cell cooling channel (gas), 10 titanium
    for x in x_face..=n.x_exit {
        let h = n.half(x);
        for t in 0..24 {
            if (11..14).contains(&t) {
                continue;
            }
            let m = if t < 3 { COOLED_CU } else { TITANIUM };
            s.put(x, cy - h - 1 - t, WALL, m, 0);
            s.put(x, cy + h + t, WALL, m, 0);
        }
    }
    // head: steel plate with a showerhead injector (1 fuel row in 6 (fuel feed pressure 22 atm vs 12 atm O2, to drive the cooling circuit)), igniters as in the base engine
    fill(&mut s, x_face - 6, cy - hc - 24, x_face - 1, cy + hc + 23, WALL, STEEL);
    for y in cy - hc..cy + hc {
        let t = if (y - cy).rem_euclid(6) == 0 { FUEL_IN } else { O2_IN };
        s.put(x_face, y, t, 0, 0);
    }
    for y in cy - 2..=cy + 1 {
        s.put(x_face, y, IGNITER, 0, 0);
    }
    for x in x_face + 5..x_face + 8 {
        s.put(x, cy - hc - 1, IGNITER, 0, 0);
        s.put(x, cy + hc, IGNITER, 0, 0);
    }
    // the cooling circuit: fuel enters at the nozzle exit, runs up the channel inside the jacket and is injected as a wall film through a 2-cell tunnel at the head corners
    for t in 11..14 {
        s.put(n.x_exit, cy - n.half(n.x_exit) - 1 - t, FUEL_IN, 0, 0);
        s.put(n.x_exit, cy + n.half(n.x_exit) + t, FUEL_IN, 0, 0);
    }
    for x in x_face..x_face + 2 {
        for t in 0..11 {
            s.put(x, cy - hc - 1 - t, 0, 0, 0);
            s.put(x, cy + hc + t, 0, 0, 0);
        }
    }
    // prime the channel with fuel (a real engine's circuit is full before start)
    let mut pts: Vec<(i32, i32)> = vec![];
    for x in [x_face, x_face + 30, 160, 175, 185, 195, 205, 220, 240, 260, n.x_exit - 2] {
        pts.push((x, n.half(x)));
    }
    for _ in 0..10 {
        for w in pts.windows(2) {
            let ((xa, ha), (xb, hb)) = (w[0], w[1]);
            s.ops.push(op(2, 3, xa, cy - ha - 13, xb, cy - hb - 13, 1));
            s.ops.push(op(2, 3, xa, cy + ha + 12, xb, cy + hb + 12, 1));
        }
    }
    s.inlet_p = 22.0;
    s.o2_p = 12.0;
    s
}
