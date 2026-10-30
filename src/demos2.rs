//! Night-shift demos built on the engine, cryogenic and construction materials (ids 92-109). Built only through the public
//! `Scene` API. Grid 512 x 288 cells of ~1 cm, y grows downwards.
#![allow(dead_code)]

use crate::gpu::{Op, H, W};
use crate::scene::{Scene, AIR_IN, FUEL_IN, IGNITER, MATTER, STEEL, VENT, WALL};

const CERAMIC: u32 = 5;
const COOLED_CU: u32 = 6;
const TITANIUM: u32 = 3;
const SAND: u32 = 10;
const AL_POWDER: u32 = 15;
const KEROSENE: u32 = 20;
const MAGNESIUM: u32 = 39;
const PAPER: u32 = 38;
const WOOD_M: u32 = 7;
const STONE: u32 = 56;
const LH2: u32 = 92;
const LCH4: u32 = 93;
const LPG: u32 = 94;
const IPA: u32 = 95;
const GYPSUM: u32 = 99;
const CONCRETE: u32 = 100;
const SOIL: u32 = 101;
const EPS: u32 = 102;
const BORON: u32 = 103;
const ZIRCONIUM: u32 = 104;
const BRASS: u32 = 106;
const BITUMEN: u32 = 107;
const COOLANT: u32 = 108;
const OIL: u32 = 109;

const FL: i32 = H as i32 - 7;

type Entry = (&'static str, &'static str, &'static str, &'static str, fn() -> Scene);

/// (bench name, template group, title, description, constructor)
pub const TABLE: [Entry; 7] = [
    ("d_bleve", "Structures & blast", "Propane tanks in a pool fire (BLEVE)", "Three sealed steel tanks half full of liquid propane over gas burners: thin walls, thick walls and a tank with a relief vent. The liquid boils, the pressure climbs and the weakest tank bursts into a fireball", bleve),
    ("d_cryofuel", "Chemistry", "Cryogenic fuels compared", "Open cups of liquid hydrogen, liquid methane and liquid propane, each with a hot wire above it: they boil off fast and the vapour burns in very different flames", cryofuel),
    ("d_metalflash", "Chemistry", "Metal powder flash race", "Aluminium, magnesium, boron and zirconium powder trickle from hoppers through a hot wire: the falling dust ignites into a sparkler-like curtain. Compare their brightness and heat", metal_flash),
    ("d_firewall", "Materials & heat transfer", "Fire barrier test", "The same burner flame on a slab of gypsum, concrete and foam, each with a paper target behind it: which one protects the paper", firewall),
    ("d_hotpots", "Materials & heat transfer", "Oil, coolant and tar over a flame", "Three steel pots with engine oil, glycol coolant and bitumen on one burner each: the coolant steams, the oil and tar give off sooty vapour", hotpots),
    ("d_foamfire", "Flames & fires", "Polystyrene foam fire", "A block of packing foam beside concrete and soil: the foam melts and burns smokily, the others only warm up", foamfire),
    ("d_lpgjet", "Flames & fires", "Propane tank with a leak", "A small leak in a warm propane tank makes a roaring jet flame that heats the tank wall", lpgjet),
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

pub fn by_name(name: &str) -> Option<Scene> {
    TABLE.iter().find(|e| e.0 == name).map(|e| (e.4)())
}

// ------------------------------------------------------------------------------------------------
// helpers

fn floor(s: &mut Scene) {
    fill(s, 0, H as i32 - 6, W as i32 - 1, H as i32 - 1, WALL, STEEL);
}

fn fill(s: &mut Scene, x0: i32, y0: i32, x1: i32, y1: i32, tile: u32, mat: u32) {
    for x in x0.min(x1)..=x0.max(x1) {
        for y in y0.min(y1)..=y0.max(y1) {
            s.put(x, y, tile, mat, 0);
        }
    }
}

fn hollow(s: &mut Scene, x0: i32, y0: i32, x1: i32, y1: i32, t: i32, tile: u32, mat: u32, open_top: bool) {
    for x in x0..=x1 {
        for y in y0..=y1 {
            let edge = x < x0 + t || x > x1 - t || y > y1 - t || (!open_top && y < y0 + t);
            if edge {
                s.put(x, y, tile, mat, 0);
            }
        }
    }
}

fn op(kind: u32, shape: u32, x0: i32, y0: i32, x1: i32, y1: i32, r: i32) -> Op {
    Op { kind, shape, x0, y0, x1, y1, r, tile: 0, mat: 0, dir: 0, _p: [0; 2] }
}

/// Preheat solids / matter in a box to `t_kelvin` (op kind 10).
fn preheat(s: &mut Scene, x0: i32, y0: i32, x1: i32, y1: i32, t_kelvin: f32) {
    let mut o = op(10, 1, x0, y0, x1, y1, 0);
    o.mat = (t_kelvin / 30.0).round() as u32;
    s.ops.push(o);
}

/// A small gas burner on the floor: a fuel jet between two air inlets, pointing up, with a pilot igniter beside it.
fn burner_up(s: &mut Scene, x: i32, y_base: i32) {
    for dx in -3..=3 {
        let t = if dx == 0 { FUEL_IN } else { AIR_IN };
        s.put(x + dx, y_base, t, 0, 0);
    }
    s.put(x - 5, y_base - 6, IGNITER, 0, 0);
    s.put(x + 5, y_base - 6, IGNITER, 0, 0);
}

// ------------------------------------------------------------------------------------------------
// scenes

fn bleve() -> Scene {
    let mut s = Scene::empty();
    floor(&mut s);
    // three tanks on short legs: (centre x, wall thickness, relief vent)
    for (cx, wall, vent) in [(130, 1, false), (255, 4, false), (380, 2, true)] {
        let (x0, x1) = (cx - 40, cx + 40);
        let (y0, y1) = (FL - 62, FL - 22);
        for lx in [x0 + 4, x1 - 4] {
            fill(&mut s, lx - 1, y1 + 1, lx + 1, FL, WALL, STEEL);
        }
        hollow(&mut s, x0, y0, x1, y1, wall, WALL, STEEL, false);
        fill(&mut s, x0 + wall, y1 - wall - 7, x1 - wall, y1 - wall, MATTER, LPG);
        for dx in [-24, 0, 24] {
            burner_up(&mut s, cx + dx, FL - 1);
        }
        if vent {
            // a 2-cell relief hole at the top
            fill(&mut s, cx - 1, y0, cx, y0 + wall - 1, 0, 0);
        }
    }
    s.inlet_p = 1.5;
    s
}

fn cryofuel() -> Scene {
    let mut s = Scene::empty();
    floor(&mut s);
    for (k, id) in [LH2, LCH4, LPG].into_iter().enumerate() {
        let cx = 110 + k as i32 * 145;
        hollow(&mut s, cx - 24, FL - 50, cx + 24, FL, 3, WALL, STEEL, true);
        fill(&mut s, cx - 21, FL - 24, cx + 21, FL - 4, MATTER, id);
        // hot wire over the cup
        for dx in -6..=6 {
            s.put(cx + dx, FL - 78, IGNITER, 0, 0);
        }
    }
    s.inlet_p = 1.5;
    s
}

fn metal_flash() -> Scene {
    let mut s = Scene::empty();
    floor(&mut s);
    for (k, id) in [AL_POWDER, MAGNESIUM, BORON, ZIRCONIUM].into_iter().enumerate() {
        let cx = 70 + k as i32 * 125;
        let ys = FL - 110;
        // a ceramic hopper shelf with a 2-cell slot; the powder trickles through and falls as dust
        fill(&mut s, cx - 30, ys, cx - 2, ys + 2, WALL, COOLED_CU);
        fill(&mut s, cx + 1, ys, cx + 29, ys + 2, WALL, COOLED_CU);
        fill(&mut s, cx - 31, ys - 16, cx - 30, ys + 2, WALL, COOLED_CU);
        fill(&mut s, cx + 29, ys - 16, cx + 30, ys + 2, WALL, COOLED_CU);
        fill(&mut s, cx - 29, ys - 14, cx + 28, ys - 1, MATTER, id);
        // hot wire below the slot: the falling dust lights as it passes
        for dy in [28, 40, 52] {
            s.put(cx - 4, ys + dy, IGNITER, 0, 0);
            s.put(cx + 3, ys + dy, IGNITER, 0, 0);
        }
        // catch tray
        hollow(&mut s, cx - 24, FL - 10, cx + 24, FL, 2, WALL, COOLED_CU, true);
    }
    s.inlet_p = 1.5;
    s
}

fn firewall() -> Scene {
    let mut s = Scene::empty();
    floor(&mut s);
    for (k, (id, thick)) in [(GYPSUM, 10), (CONCRETE, 10), (EPS, 10)].into_iter().enumerate() {
        let x0 = 40 + k as i32 * 160;
        // burner: a horizontal premix jet from a wall stub on the left, pilot beside it
        fill(&mut s, x0, FL - 80, x0 + 6, FL - 50, WALL, STEEL);
        for y in FL - 70..=FL - 58 {
            let t = if y % 6 == 0 { FUEL_IN } else { AIR_IN };
            s.put(x0 + 6, y, t, 0, 0);
        }
        s.put(x0 + 12, FL - 74, IGNITER, 0, 0);
        s.put(x0 + 12, FL - 54, IGNITER, 0, 0);
        // the barrier slab and a paper target 6 cells behind it
        let bx = x0 + 60;
        fill(&mut s, bx, FL - 90, bx + thick - 1, FL - 40, MATTER, id);
        fill(&mut s, bx + thick + 6, FL - 90, bx + thick + 7, FL - 40, MATTER, PAPER);
        // shelf under the slab
        fill(&mut s, bx - 2, FL - 39, bx + thick + 9, FL - 36, WALL, STEEL);
        fill(&mut s, bx + 3, FL - 35, bx + 5, FL, WALL, STEEL);
    }
    s.inlet_p = 1.4;
    s.p_air = 1.4;
    s
}

fn hotpots() -> Scene {
    let mut s = Scene::empty();
    floor(&mut s);
    for (k, id) in [OIL, COOLANT, BITUMEN].into_iter().enumerate() {
        let cx = 100 + k as i32 * 150;
        let (x0, x1) = (cx - 36, cx + 36);
        // pot on a stand over three floor burners
        hollow(&mut s, x0, FL - 48, x1, FL - 14, 2, WALL, STEEL, true);
        fill(&mut s, x0 + 2, FL - 32, x1 - 2, FL - 16, MATTER, id);
        // the liquids start warm: oil, coolant and tar just below their boiling points
        preheat(&mut s, x0 + 2, FL - 32, x1 - 2, FL - 16, [765.0, 592.0, 612.0][k]);
        for lx in [x0 + 6, x1 - 6] {
            fill(&mut s, lx - 1, FL - 13, lx + 1, FL, WALL, STEEL);
        }
        for dx in [-20, 0, 20] {
            burner_up(&mut s, cx + dx, FL - 1);
        }
        // a pilot above the pot rim to light the vapour
        s.put(cx, FL - 58, IGNITER, 0, 0);
    }
    s.inlet_p = 1.5;
    s.p_air = 1.5;
    s
}

fn foamfire() -> Scene {
    let mut s = Scene::empty();
    floor(&mut s);
    fill(&mut s, 60, FL - 40, 140, FL - 1, MATTER, EPS);
    fill(&mut s, 200, FL - 40, 280, FL - 1, MATTER, CONCRETE);
    fill(&mut s, 340, FL - 25, 420, FL - 1, MATTER, SOIL);
    // one hot-wire igniter beside each
    for x in [58, 198, 338] {
        for dy in 0..6 {
            s.put(x, FL - 20 - dy, IGNITER, 0, 0);
        }
    }
    s.inlet_p = 1.5;
    s
}

fn lpgjet() -> Scene {
    let mut s = Scene::empty();
    floor(&mut s);
    let (x0, x1, y0, y1) = (150, 330, FL - 90, FL - 30);
    for lx in [x0 + 12, x1 - 12] {
        fill(&mut s, lx - 2, y1 + 1, lx + 2, FL, WALL, STEEL);
    }
    hollow(&mut s, x0, y0, x1, y1, 5, WALL, STEEL, false);
    fill(&mut s, x0 + 5, y1 - 5 - 20, x1 - 5, y1 - 5, MATTER, LPG);
    // leak: a 2-cell hole in the right end, with a pilot flame waiting there
    fill(&mut s, x1 - 4, y1 - 40, x1, y1 - 39, 0, 0);
    s.put(x1 + 6, y1 - 45, IGNITER, 0, 0);
    s.put(x1 + 6, y1 - 34, IGNITER, 0, 0);
    s.inlet_p = 1.5;
    s
}
