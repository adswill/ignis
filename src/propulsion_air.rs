//! Air-breathing propulsion demos (propulsion team, AIR). Built only through the public `Scene` API
//! (`Scene::empty`, `put`, `meta`/`inlet_p`/`o2_p`/`ops`) and the tile/material/element ids from scene.rs.
//! Grid is 512 x 288 cells of ~1 cm (y grows downwards, flow is +x). Compressors are Fan tiles (a momentum
//! source), the air feed is the open duct mouth (ambient) or the air inlet column (ram pressure).
#![allow(dead_code)]

use crate::gpu::Op;
use crate::scene::{Scene, AIR_IN, FAN, FUEL_IN, IGNITER, INCONEL, STEEL, WALL};

const ALU: u32 = 1;
const TITANIUM: u32 = 3;
const CERAMIC: u32 = 5;
const COOLED_CU: u32 = 6;

// ---------------------------------------------------------------------------------------------
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

/// Piecewise-linear profile through `pts` (x, value), rounded to whole cells.
fn prof(pts: &[(i32, i32)], x: i32) -> i32 {
    if x <= pts[0].0 {
        return pts[0].1;
    }
    for k in 1..pts.len() {
        let (xa, ya) = pts[k - 1];
        let (xb, yb) = pts[k];
        if x <= xb {
            let t = (x - xa) as f32 / (xb - xa).max(1) as f32;
            return (ya as f32 + (yb - ya) as f32 * t).round() as i32;
        }
    }
    pts[pts.len() - 1].1
}

/// Axisymmetric-in-2D duct: gas fills `cy-h .. cy+h-1`, `th` wall rows on each side with `mat(x, t)`
/// (t = 0 is the layer touching the gas). `pts` = (x, inner half height).
fn duct(s: &mut Scene, cy: i32, pts: &[(i32, i32)], th: i32, mat: &dyn Fn(i32, i32) -> u32) {
    for x in pts[0].0..=pts[pts.len() - 1].0 {
        let h = prof(pts, x);
        for t in 0..th {
            let m = mat(x, t);
            s.put(x, cy - h - 1 - t, WALL, m, 0);
            s.put(x, cy + h + t, WALL, m, 0);
        }
    }
}

fn fans(s: &mut Scene, x0: i32, x1: i32, y0: i32, y1: i32) {
    for x in x0..=x1 {
        for y in y0..=y1 {
            s.put(x, y, FAN, 0, 0);
        }
    }
}

/// Wall-mounted fuel injector slots on both sides of a duct (replace the innermost wall layer).
fn wall_injectors(s: &mut Scene, cy: i32, h: i32, x0: i32, x1: i32) {
    for x in x0..=x1 {
        s.put(x, cy - h - 1, FUEL_IN, 0, 0);
        s.put(x, cy + h, FUEL_IN, 0, 0);
    }
}

fn wall_igniters(s: &mut Scene, cy: i32, h: i32, x0: i32, x1: i32) {
    for x in x0..=x1 {
        s.put(x, cy - h - 1, IGNITER, 0, 0);
        s.put(x, cy + h, IGNITER, 0, 0);
    }
}

/// V-gutter flame holder: apex upstream at (x, y), two arms opening downstream, `len` cells long.
fn gutter(s: &mut Scene, x: i32, y: i32, len: i32, mat: u32) {
    for k in 0..len {
        let d = (k + 1) / 2;
        s.put(x + k, y - d, WALL, mat, 0);
        s.put(x + k, y + d, WALL, mat, 0);
    }
    s.put(x, y, WALL, mat, 0);
}

/// Spray bar: a column of fuel injection points inside the stream.
fn spray_bar(s: &mut Scene, x: i32, cy: i32, half: i32, pitch: i32) {
    let mut y = cy - half;
    while y < cy + half {
        s.put(x, y, FUEL_IN, 0, 0);
        y += pitch;
    }
}

// ---------------------------------------------------------------------------------------------
// 1. turbojet with afterburner

/// One turbojet (x0 = inlet lip). Core: fan compressor, can combustor with flame holder, turbine nozzle guide
/// vanes (the restriction), then the reheat section (diffuser, spray bar, V-gutters) and a convergent nozzle.
fn turbojet_core(s: &mut Scene, cy: i32, x0: i32, reheat: bool) {
    let hot = |x: i32, t: i32| -> u32 {
        if x < x0 + 28 {
            STEEL
        } else if t < 2 {
            COOLED_CU // film-cooled liner
        } else {
            INCONEL
        }
    };
    // inner half height: inlet 17, combustor 20, turbine throat 12, reheat diffuser to 23, convergent exit.
    // the nozzle is variable in real engines: it opens for reheat so the core's operating line is unchanged.
    let exit = if reheat { 14 } else { 11 };
    let pts = [
        (x0, 17),
        (x0 + 30, 17),
        (x0 + 38, 20),
        (x0 + 92, 20),
        (x0 + 106, 13), // turbine nozzle guide vanes
        (x0 + 118, 13), // turbine stage
        (x0 + 140, 23), // reheat diffuser
        (x0 + 205, 23),
        (x0 + 232, exit), // convergent nozzle
        (x0 + 240, exit),
    ];
    duct(s, cy, &pts, 5, &hot);
    // inlet fan / compressor stage
    fans(s, x0 + 8, x0 + 7 + 14, cy - 17, cy + 16);
    // primary combustor: wall injectors, spark, flame holder (V-gutter) in the middle of the can
    wall_injectors(s, cy, 20, x0 + 48, x0 + 47 + 2);
    wall_igniters(s, cy, 20, x0 + 62, x0 + 64);
    gutter(s, x0 + 68, cy - 6, 9, COOLED_CU);
    gutter(s, x0 + 68, cy + 6, 9, COOLED_CU);
    if reheat {
        // reheat: a spray bar and three V-gutters, ignited by the hot core gas and a hot-streak igniter
        let n = 2;
        for k in 0..n {
            let y = cy - 16 + (32 * (2 * k + 1)) / (2 * n);
            for j in 0..1 {
                s.put(x0 + 146, y + j, FUEL_IN, 0, 0);
            }
        }
        for k in -1..=1 {
            gutter(s, x0 + 156, cy + k * 14, 11, COOLED_CU);
        }
        wall_igniters(s, cy, 23, x0 + 160, x0 + 162);
    }
}

fn turbojet_afterburner() -> Scene {
    let mut s = Scene::empty();
    turbojet_core(&mut s, 76, 36, false); // dry, top
    turbojet_core(&mut s, 212, 36, true); // wet, bottom
    s.inlet_p = 3.6;
    s
}

/// Variants for measuring each engine alone (not in the registry).
fn ab_single(reheat: bool) -> Scene {
    let mut s = Scene::empty();
    turbojet_core(&mut s, 144, 36, reheat);
    s.inlet_p = 3.6;
    s
}

// ---------------------------------------------------------------------------------------------
// 2. high-bypass turbofan

fn turbofan() -> Scene {
    let mut s = Scene::empty();
    let cy = 144;
    let x0 = 46;
    // nacelle: big inlet, bypass nozzle closes slightly (convergent)
    let nac = [(x0, 30), (x0 + 175, 30), (x0 + 200, 24)];
    duct(&mut s, cy, &nac, 4, &|x, t| if x < x0 + 175 { ALU } else if t < 2 { COOLED_CU } else { STEEL });
    // core: splitter at x0+28, compressor 6, combustor 9, turbine throat 5, core nozzle protrudes to x0+232
    let core = [
        (x0 + 28, 6),
        (x0 + 60, 6),
        (x0 + 72, 9),
        (x0 + 135, 9),
        (x0 + 148, 5),
        (x0 + 160, 5),
        (x0 + 175, 8),
        (x0 + 200, 8),
        (x0 + 230, 6),
    ];
    duct(&mut s, cy, &core, 5, &|x, t| if x < x0 + 60 { TITANIUM } else if t == 0 { COOLED_CU } else { INCONEL });
    // splitter nose: blunt leading edge in front of the core wall
    for x in x0 + 26..x0 + 28 {
        fill(&mut s, x, cy - 11, x, cy - 7, WALL, TITANIUM);
        fill(&mut s, x, cy + 6, x, cy + 10, WALL, TITANIUM);
    }
    // fan (full inlet height, large mass flow) and the core's own compressor
    fans(&mut s, x0 + 6, x0 + 13, cy - 30, cy + 29);
    fans(&mut s, x0 + 34, x0 + 33 + 6, cy - 6, cy + 5);
    // core combustor: fuel and igniter side by side so the first fuel is lit at once
    wall_injectors(&mut s, cy, 9, x0 + 84, x0 + 85);
    wall_igniters(&mut s, cy, 9, x0 + 88, x0 + 90);
    gutter(&mut s, x0 + 94, cy - 1, 8, COOLED_CU);
    s.inlet_p = 3.6;
    s
}

// ---------------------------------------------------------------------------------------------
// 3. ramjet

fn ramjet() -> Scene {
    let mut s = Scene::empty();
    let cy = 144;
    let x0 = 40;
    // cowl: lip half 18, throat 11 at x0+44, subsonic diffuser to the can (half 22), nozzle throat 12, bell to 17
    let cowl = [
        (x0 + 1, 18),
        (x0 + 44, 14),
        (x0 + 58, 14),
        (x0 + 90, 22),
        (x0 + 175, 22),
        (x0 + 205, 14),
        (x0 + 215, 14),
        (x0 + 255, 19),
    ];
    duct(&mut s, cy, &cowl, 5, &|x, t| {
        if x < x0 + 80 {
            STEEL
        } else if t < 2 {
            COOLED_CU
        } else {
            INCONEL
        }
    });
    // ram air: the air-inlet column stands for the free stream at the cowl lip (total pressure p_air)
    for y in cy - 18..cy + 18 {
        s.put(x0, y, AIR_IN, 0, 0);
    }
    // centrebody: a double-wedge spike (tip upstream) leaves an annular throat
    for x in x0 + 1..=x0 + 78 {
        let h = prof(&[(x0 + 1, 0), (x0 + 22, 3), (x0 + 44, 6), (x0 + 58, 6), (x0 + 78, 0)], x);
        for y in cy - h..cy + h {
            s.put(x, y, WALL, STEEL, 0);
        }
    }
    // combustor: spray bars + V-gutter flame holders + igniters
    spray_bar(&mut s, x0 + 100, cy, 22, 22);
    for k in -1..=1 {
        gutter(&mut s, x0 + 112, cy + k * 14, 11, INCONEL);
    }
    wall_igniters(&mut s, cy, 22, x0 + 118, x0 + 120);
    s.inlet_p = 1.6;
    s
}

// ---------------------------------------------------------------------------------------------
// 4. scramjet

fn scramjet() -> Scene {
    let mut s = Scene::empty();
    let cy = 144;
    let x0 = 30;
    // direct-connect rig: fan-pressurised plenum -> Laval facility nozzle (about Mach 1.9) -> constant-area isolator
    // -> strut + wall fuel injection -> gently diverging combustor (3.9 deg) -> expansion nozzle
    let pts = [
        (x0, 16),
        (x0 + 40, 16),
        (x0 + 62, 7), // facility nozzle throat
        (x0 + 90, 10), // supersonic leg (area ratio 1.45)
        (x0 + 135, 10), // isolator
        (x0 + 215, 16), // diverging combustor
        (x0 + 275, 26), // expansion nozzle
    ];
    duct(&mut s, cy, &pts, 5, &|x, t| {
        if x < x0 + 80 {
            TITANIUM
        } else if t < 2 {
            COOLED_CU
        } else {
            INCONEL
        }
    });
    for y in cy - 16..cy + 16 {
        s.put(x0, y, AIR_IN, 0, 0);
    }
    fans(&mut s, x0 + 6, x0 + 5 + 8, cy - 16, cy + 15);
    // central strut injector: a thin wedge (tip upstream) with fuel ports on its trailing edge, plus wall injectors
    for k in 0..16 {
        for y in cy - 1..=cy {
            s.put(x0 + 118 + k, y, WALL, COOLED_CU, 0);
        }
    }
    for y in [cy - 1, cy] {
        s.put(x0 + 134, y, FUEL_IN, 0, 0);
    }
    wall_injectors(&mut s, cy, 10, x0 + 126, x0 + 125 + 1);
    wall_igniters(&mut s, cy, 10, x0 + 140, x0 + 142);
    s.inlet_p = 2.0;
    s
}

// ---------------------------------------------------------------------------------------------
// 5. valveless pulsejet (Lockheed U-tube)

fn pulsejet_u() -> Scene {
    let mut s = Scene::empty();
    // Lockheed/Hiller-style valveless pulsejet, about 1.2 m long: a shared combustion chamber with a short
    // venturi intake (top) and a long resonant tailpipe (bottom), both opening rearward (+x). No valves: the
    // intake's gas inertia and the pressure cycle do the work.
    let hh = 5; // tube half height (10 cells = 10 cm bore)
    let (yi, yt) = (131, 131 + 26); // tube axes
    let (xc0, xc1) = (50, 50 + 25); // chamber (gas) x range
    let th = 3;
    let (top, bot) = (yi - hh, yt + hh - 1); // chamber gas rows
    // chamber shell
    fill(&mut s, xc0 - th, top - th, xc1, top - 1, WALL, INCONEL);
    fill(&mut s, xc0 - th, bot + 1, xc1, bot + th, WALL, INCONEL);
    fill(&mut s, xc0 - th, top - th, xc0 - 1, bot + th, WALL, INCONEL);
    // intake tube: x xc1..x_in, tailpipe: xc1..x_tail
    let x_in = xc1 + 40;
    let x_tail = xc1 + 85;
    for (yc, xe) in [(yi, x_in), (yt, x_tail)] {
        fill(&mut s, xc1 + 1, yc - hh - th, xe, yc - hh - 1, WALL, INCONEL);
        fill(&mut s, xc1 + 1, yc + hh, xe, yc + hh + th - 1, WALL, INCONEL);
    }
    // partition between the two tubes (the wall of the U) continues from the chamber to the exits
    fill(&mut s, xc1 + 1, yi + hh + th, x_in, yt - hh - th - 1, 0, 0);
    // venturi on the intake: the tube narrows to half the bore for 10 cells, fuel is drawn in at the throat
    for x in xc1 + 10..xc1 + 26 {
        let k = (4 - ((x - xc1 - 18).abs() / 2)).clamp(0, 3);
        fill(&mut s, x, yi - hh, x, yi - hh + k - 1, WALL, INCONEL);
        fill(&mut s, x, yi + hh - k, x, yi + hh - 1, WALL, INCONEL);
    }
    for x in xc1 + 18..xc1 + 18 + 1 {
        s.put(x, yi - hh + 2, FUEL_IN, 0, 0);
        s.put(x, yi + hh - 3, FUEL_IN, 0, 0);
    }
    // starter air / free-stream feed at the intake mouth (pinned at the air-inlet pressure)
    for y in yi - hh..yi + hh {
        s.put(x_in, y, AIR_IN, 0, 0);
    }
    // glow plug at the chamber head (relights every pulse of fresh charge)
    for y in (yi + yt) / 2 - 1..(yi + yt) / 2 + 1 {
        s.put(xc0, y, IGNITER, 0, 0);
    }
    // charge the chamber and tubes with a near-stoichiometric mix so the first cycle fires at once
    s.ops.push(op(2, 1, xc0 + 1, top, xc1 + 20, bot, 0));
    s.ops.push(op(2, 1, xc0 + 1, top, xc1 + 20, bot, 0));
    s.inlet_p = 1.3;
    s
}

// ---------------------------------------------------------------------------------------------
// 6. pulse-detonation tube (PDE) - NOT a rotating detonation engine

fn pde_tube() -> Scene {
    let mut s = Scene::empty();
    let cy = 144;
    let (x0, x1) = (60, 60 + 260);
    let hh = 8; // 16 cm bore
    let th = 8;
    fill(&mut s, x0 - th, cy - hh - th, x0 - 1, cy + hh + th - 1, WALL, TITANIUM);
    fill(&mut s, x0, cy - hh - th, x1, cy - hh - 1, WALL, TITANIUM);
    fill(&mut s, x0, cy + hh, x1, cy + hh + th - 1, WALL, TITANIUM);
    // Shchelkin spiral analogue: periodic orifice rings that fold the flame and speed it up
    let mut x = x0 + 24;
    while x < x1 - 40 {
        fill(&mut s, x, cy - hh, x + 1, cy - hh + 2, WALL, TITANIUM);
        fill(&mut s, x, cy + hh - 3, x + 1, cy + hh - 1, WALL, TITANIUM);
        x += 14;
    }
    // head end: fuel and air fill ports and a spark
    for y in cy - hh..cy + hh {
        let t = if (y - cy).rem_euclid(4) == 0 { FUEL_IN } else { AIR_IN };
        s.put(x0, y, t, 0, 0);
    }
    for y in cy - 1..=cy {
        s.put(x0, y, IGNITER, 0, 0);
    }
    // the tube starts full of a stoichiometric, oxygen-enriched charge
    for _ in 0..2 {
        s.ops.push(op(2, 1, x0 + 1, cy - hh, x1 - 20, cy + hh - 1, 0));
    }
    for _ in 0..1 {
        s.ops.push(op(3, 1, x0 + 1, cy - hh, x1 - 20, cy + hh - 1, 0));
    }
    s.inlet_p = 3.0;
    s
}

// ---------------------------------------------------------------------------------------------
// 7. thrust augmentor / ejector nozzle

fn ejector_scene(bare: bool, shrouded: bool) -> Scene {
    let mut s = Scene::empty();
    s.inlet_p = 3.6;
    if bare && shrouded {
        turbojet_core(&mut s, 74, 36, false);
        turbojet_core(&mut s, 214, 36, false);
        ejector_shroud(&mut s, 214, 36);
    } else if bare {
        turbojet_core(&mut s, 144, 36, false);
    } else {
        turbojet_core(&mut s, 144, 36, false);
        ejector_shroud(&mut s, 144, 36);
    }
    s
}

/// Ejector shroud around the primary nozzle: a bell-mouth inlet that lets ambient air flow past the engine body,
/// a constant-area mixing tube (about 3x the primary nozzle area) and a short diffuser.
fn ejector_shroud(s: &mut Scene, cy: i32, x0: i32) {
    let h = 34;
    let l = 190;
    let sh = [(x0 + 190, h + 10), (x0 + 215, h), (x0 + 215 + l, h), (x0 + 255 + l, h + 6)];
    duct(s, cy, &sh, 3, &|x, t| if x < x0 + 235 && t == 0 { ALU } else { STEEL });
}

fn ejector() -> Scene {
    ejector_scene(true, true)
}

// ---------------------------------------------------------------------------------------------
// registry

pub fn all() -> Vec<(&'static str, &'static str, fn() -> Scene)> {
    vec![
        ("Turbojet with afterburner (dry vs reheat)", "Two identical turbojets: dry (top) and with reheat (bottom): a spray bar, V-gutter flame holders and an opened nozzle add about 15-20 % thrust and a much hotter, longer plume (compare them in the Heat view)", turbojet_afterburner as fn() -> Scene),
        ("High-bypass turbofan", "A large fan drives most of the air round the core in a cool annular bypass stream that merges with the hot core jet at the nozzle (Flow and Pressure views)", turbofan),
        ("Ramjet", "Flame-holding study: ram air through a spike-centrebody inlet into a can combustor with V-gutter flame holders and a convergent nozzle. The flame stays anchored on the gutters, but at the default 2 atm inlet it makes no net thrust (raise Air inlet p0 to emulate flight speed)", ramjet),
        ("Scramjet (direct-connect rig)", "Fan plenum and Laval facility nozzle feed an isolator, a thin fuel strut and a diverging combustor; expansion cells and the shock train show in the Pressure view", scramjet),
        ("Valveless pulsejet (Lockheed U-tube)", "A folded valveless pulsejet: venturi intake and tailpipe share one chamber; a strong start-up pulse, then self-sustained oscillating combustion (Pressure view)", pulsejet_u),
        ("Pulse-detonation tube", "Single-shot pulse-detonation tube: Shchelkin orifice rings accelerate the flame into a Mach-5, 25-atm front that races to the open end (a rotating-detonation engine is not modelled)", pde_tube),
        ("Ejector thrust augmentor", "The same turbojet bare (top) and inside an ejector shroud (bottom): the primary jet pumps ambient air through the shroud (Flow and Heat views)", ejector),
    ]
}

pub const NAMES: [&str; 7] = ["turbab", "turbofan", "ramjet", "scramjet", "pulseu", "pde", "ejector"];

pub fn by_name(name: &str) -> Option<Scene> {
    NAMES.iter().position(|n| *n == name).map(|i| (all()[i].2)())
}

/// Development-only variants (single engines for measuring thrust separately).
pub fn debug_by_name(name: &str) -> Option<Scene> {
    match name {
        "ab_dry" => Some(ab_single(false)),
        "ab_wet" => Some(ab_single(true)),
        "ej_bare" => Some(ejector_scene(true, false)),
        "ej_shr" => Some(ejector_scene(false, true)),
        _ => by_name(name),
    }
}
