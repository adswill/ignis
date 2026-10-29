//! Ready-made showcase scenes (showcase team). Built only through the public `Scene` API
//! (`Scene::empty`, `put`, `meta`/`inlet_p`/`o2_p`/`ops`) and the element ids in matter.wgsl.
//! Grid is 512 x 288 cells of ~1 cm (y grows downwards); tile/material constants come from scene.rs.
#![allow(dead_code)]

use crate::gpu::{Op, H, W};
use crate::scene::{Scene, AIR_IN, FUEL_IN, IGNITER, INCONEL, MATTER, O2_IN, STEEL, VENT, WALL, WOOD};

// wall materials (ids 0-9)
const ALU: u32 = 1;
const COPPER: u32 = 2;
const TITANIUM: u32 = 3;
const CERAMIC: u32 = 5;
const COOLED_CU: u32 = 6;
const EXPLOSIVE: u32 = 9;

// matter elements (ids from matter.wgsl)
const SAND: u32 = 10;
const CHARCOAL: u32 = 11;
const SULFUR: u32 = 12;
const KNO3: u32 = 13;
const SUGAR: u32 = 14;
const AL_POWDER: u32 = 15;
const RUST: u32 = 17;
const AN: u32 = 18;
const WATER: u32 = 19;
const KEROSENE: u32 = 20;
const LOX: u32 = 22;
const NITRO: u32 = 23;
const GASOLINE: u32 = 31;
const DIESEL: u32 = 32;
const WAX: u32 = 35;
const COAL: u32 = 37;
const PAPER: u32 = 38;
const GLASS: u32 = 53;
const STONE: u32 = 56;
const C4: u32 = 50;
const CAP: u32 = 52;
const FUSE: u32 = 65;

/// Floor line (top surface row of the steel floor).
const FL: i32 = H as i32 - 7;

// ---------------------------------------------------------------------------------------------
// small drawing helpers (Scene's own helpers are private to scene.rs)

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

fn clear(s: &mut Scene, x0: i32, y0: i32, x1: i32, y1: i32) {
    fill(s, x0, y0, x1, y1, 0, 0);
}

/// Box outline of thickness `t`; `open_top` leaves the top edge out (a tray / beaker).
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

/// Intimately mixed powders: a deterministic scatter of `mats` with the given weights.
fn mix(s: &mut Scene, x0: i32, y0: i32, x1: i32, y1: i32, mats: &[(u32, u32)]) {
    let total: u32 = mats.iter().map(|m| m.1).sum();
    for x in x0..=x1 {
        for y in y0..=y1 {
            let mut h = (x as u32).wrapping_mul(0x9E37_79B1) ^ (y as u32).wrapping_mul(0x85EB_CA77);
            h ^= h >> 15;
            h = h.wrapping_mul(0x2C1B_3C6D);
            h ^= h >> 12;
            let mut r = h % total;
            for &(m, w) in mats {
                if r < w {
                    s.put(x, y, MATTER, m, 0);
                    break;
                }
                r -= w;
            }
        }
    }
}

fn disc(s: &mut Scene, cx: i32, cy: i32, r: i32, tile: u32, mat: u32) {
    for x in cx - r..=cx + r {
        for y in cy - r..=cy + r {
            if (x - cx) * (x - cx) + (y - cy) * (y - cy) <= r * r {
                s.put(x, y, tile, mat, 0);
            }
        }
    }
}

fn op(kind: u32, shape: u32, x0: i32, y0: i32, x1: i32, y1: i32, r: i32) -> Op {
    Op { kind, shape, x0, y0, x1, y1, r, tile: 0, mat: 0, dir: 0, _p: [0; 2] }
}

/// Pre-fill a box with a near-stoichiometric fuel/air mix (each add-fuel op adds ~5 % fuel by mass).
fn premix(s: &mut Scene, x0: i32, y0: i32, x1: i32, y1: i32, times: u32) {
    for _ in 0..times {
        s.ops.push(op(2, 1, x0, y0, x1, y1, 0));
    }
}

fn torch(s: &mut Scene, x: i32, y: i32, r: i32) {
    s.ops.push(op(5, 0, x, y, x, y, r));
}

const BLACK_POWDER: [(u32, u32); 3] = [(KNO3, 75), (CHARCOAL, 15), (SULFUR, 10)];

// ---------------------------------------------------------------------------------------------
// scenes

/// Solid rocket motor (cored cast grain) in a steel case, nozzle down, clamped in a test stand.
pub fn candy_rocket() -> Scene {
    let mut s = Scene::empty();
    floor(&mut s);
    let cx = 256;
    let (top, grain_bot) = (40, 150);
    let r = 14; // inner radius of the case
    // case walls + nose cap
    fill(&mut s, cx - r - 5, top, cx - r - 1, grain_bot + 6, WALL, STEEL);
    fill(&mut s, cx + r + 1, top, cx + r + 5, grain_bot + 6, WALL, STEEL);
    for k in 0..12 {
        fill(&mut s, cx - r - 3 + k, top - 1 - k, cx + r + 3 - k, top - 1 - k, WALL, STEEL);
    }
    // water-jacketed test-stand liner (cooled copper) so the case survives a full burn
    fill(&mut s, cx - r, top, cx - r + 1, grain_bot, WALL, COOLED_CU);
    fill(&mut s, cx + r - 1, top, cx + r, grain_bot, WALL, COOLED_CU);
    // BATES-like grain with a central core (bore) so it burns from the inside out
    // cast propellant grain (rocket candy is a cast KNO3/sugar solid; the cast-grain material
    // gives the proper pressure-dependent surface burn instead of a loose powder deflagration)
    fill(&mut s, cx - r + 2, top, cx + r - 2, grain_bot, WALL, crate::scene::PROPELLANT);
    clear(&mut s, cx - 3, top + 8, cx + 3, grain_bot);
    // ceramic converging-diverging nozzle below the grain
    for y in grain_bot + 1..grain_bot + 40 {
        let d = y - grain_bot;
        let h = if d < 14 { r - d * (r - 8) / 14 } else { 8 + (d - 14) * 8 / 25 };
        fill(&mut s, cx - r - 5, y, cx - h - 1, y, WALL, INCONEL);
        fill(&mut s, cx + h + 1, y, cx + r + 5, y, WALL, INCONEL);
    }
    // igniter up in the core (electric match), fed through the nozzle
    for y in (top + 10..grain_bot).step_by(20) {
        s.put(cx, y, IGNITER, 0, 0);
    }
    // launch stand: two steel rails and a blast deflector plate on the floor
    for side in [-1, 1] {
        let x = cx + side * (r + 12);
        fill(&mut s, x, top - 20, x + side * 2, FL, WALL, STEEL);
        fill(&mut s, x.min(x + side * 6), grain_bot - 30, x.max(x + side * 6) - 0, grain_bot - 28, WALL, STEEL);
    }
    for k in 0..30 {
        s.put(cx - 30 + k, FL - k / 6, WALL, STEEL, 0);
        s.put(cx + 30 - k, FL - k / 6, WALL, STEEL, 0);
    }
    s
}

/// Black-powder cannon: powder charge at the breech, a sand "ball" rammed down the barrel, a fuse in
/// the touch hole and a wooden palisade downrange.
pub fn cannon() -> Scene {
    let mut s = Scene::empty();
    floor(&mut s);
    let cy = FL - 30;
    let (bx0, bx1, bore) = (40, 200, 7);
    // steel barrel, thick at the breech
    for x in bx0..=bx1 {
        let t = if x < bx0 + 50 { 7 } else { 4 };
        fill(&mut s, x, cy - bore - t, x, cy - bore - 1, WALL, STEEL);
        fill(&mut s, x, cy + bore + 1, x, cy + bore + t, WALL, STEEL);
    }
    fill(&mut s, bx0 - 6, cy - bore - 7, bx0 - 1, cy + bore + 7, WALL, STEEL);
    // carriage
    fill(&mut s, bx0 + 10, cy + bore + 8, bx0 + 60, FL, WALL, WOOD);
    // the charge
    mix(&mut s, bx0, cy - bore, bx0 + 26, cy + bore, &BLACK_POWDER);
    // a wooden tampion plugs the muzzle: it holds the pressure until it shears out
    fill(&mut s, bx1 - 6, cy - bore, bx1 - 4, cy + bore, WALL, WOOD);
    // friction primer in the breech plug (the touch hole would vent most of the gas)
    s.put(bx0 - 1, cy, IGNITER, 0, 0);
    // wooden palisade
    for k in 0..4 {
        let x = 232 + k * 26;
        fill(&mut s, x, FL - 80, x + 6, FL, WALL, WOOD);
    }
    s
}

/// Thermite on a thin steel plate over a water tank: molten iron burns through and hits the water.
pub fn thermite_steam() -> Scene {
    let mut s = Scene::empty();
    floor(&mut s);
    let (tx0, tx1) = (150, 362);
    // open steel tank of water
    hollow(&mut s, tx0, FL - 70, tx1, FL, 4, WALL, STEEL, true);
    fill(&mut s, tx0 + 4, FL - 50, tx1 - 4, FL - 4, MATTER, WATER);
    // 2-cell steel plate on posts above the water
    let py = FL - 110;
    fill(&mut s, 200, py, 312, py + 1, WALL, STEEL);
    fill(&mut s, 200, py + 2, 203, FL - 71, WALL, STEEL);
    fill(&mut s, 309, py + 2, 312, FL - 71, WALL, STEEL);
    // thermite heap (rust : aluminium about 3 : 1 by volume)
    for k in 0..18 {
        let w = 34 - k * 3 / 2;
        mix(&mut s, 256 - w, py - 1 - k, 256 + w, py - 1 - k, &[(RUST, 3), (AL_POWDER, 1)]);
    }
    // magnesium-ribbon starter: a strip of Mg up the heap from an igniter
    for k in 0..16 {
        s.put(216 + k, py - 1 - k / 2, MATTER, 39, 0);
        s.put(216 + k, py - 2 - k / 2, MATTER, 39, 0);
    }
    s.put(215, py - 1, IGNITER, 0, 0);
    s
}

/// Kerosene pool fire in a room: the hot smoke layer collects under the ceiling until the wax fusible
/// plugs of the sprinkler main melt and let the water rain down.
pub fn sprinkler() -> Scene {
    let mut s = Scene::empty();
    floor(&mut s);
    // fuel tray
    hollow(&mut s, 150, FL - 14, 362, FL, 3, WALL, STEEL, true);
    fill(&mut s, 153, FL - 9, 359, FL - 3, MATTER, KEROSENE);
    for x in [160, 210, 256, 300, 350] {
        s.put(x, FL - 10, IGNITER, 0, 0);
        torch(&mut s, x, FL - 13, 4);
    }
    // sprinkler main near the ceiling: a long steel pipe full of water
    let (py0, py1) = (FL - 126, FL - 110);
    // a low hood/ceiling with side skirts; open below so air can feed the fire
    fill(&mut s, 60, py0 - 4, 452, py0 - 1, WALL, STEEL);
    fill(&mut s, 60, py0 - 4, 64, FL - 60, WALL, STEEL);
    fill(&mut s, 448, py0 - 4, 452, FL - 60, WALL, STEEL);
    hollow(&mut s, 120, py0, 392, py1, 3, WALL, STEEL, false);
    fill(&mut s, 123, py0 + 3, 389, py1 - 3, MATTER, WATER);
    // sprinkler heads: wax plugs (fusible links) in the pipe bottom with a deflector below
    for x in [170, 230, 290, 350] {
        fill(&mut s, x - 2, py1 - 2, x + 2, py1, MATTER, WAX);
        fill(&mut s, x - 5, py1 + 6, x + 5, py1 + 6, WALL, STEEL);
        s.put(x - 5, py1 + 5, WALL, STEEL, 0);
        s.put(x + 5, py1 + 5, WALL, STEEL, 0);
    }
    s
}

/// Two identical boilers on gas burners: the left one is sealed, the right one has a safety valve.
pub fn boiler() -> Scene {
    let mut s = Scene::empty();
    floor(&mut s);
    for (k, cx) in [150, 362].into_iter().enumerate() {
        let (x0, x1, y0, y1) = (cx - 40, cx + 40, FL - 90, FL - 22);
        // thin riveted shell: steel 2 cells, aluminium seam panel on top (weakest point)
        hollow(&mut s, x0, y0, x1, y1, 2, WALL, STEEL, false);
        fill(&mut s, cx - 20, y0, cx + 20, y0 + 1, WALL, ALU);
        fill(&mut s, x0 + 2, y1 - 14, x1 - 2, y1 - 2, MATTER, WATER);
        // legs
        // lattice legs (gaps let air in under the shell)
        for y in y1 + 1..=FL {
            if (y - y1).rem_euclid(4) < 2 {
                fill(&mut s, x0, y, x0 + 3, y, WALL, STEEL);
                fill(&mut s, x1 - 3, y, x1, y, WALL, STEEL);
            }
        }
        // premix ribbon burner under the shell (~1 fuel : 9 air)
        for x in x0 + 8..=x1 - 8 {
            s.put(x, FL, if x % 10 == 0 { FUEL_IN } else { AIR_IN }, 0, 0);
        }
        s.put(x0 + 8, FL - 1, IGNITER, 0, 0);
        s.put(x1 - 8, FL - 1, IGNITER, 0, 0);
        // pre-heat the water a little with torches on the shell bottom so it boils sooner
        s.ops.push(op(5, 1, x0, y1 - 1, x1, y1, 0));
        if k == 1 {
            // safety valve: a riser pipe ending in a vent
            fill(&mut s, cx + 30, y0 - 20, cx + 31, y0 - 1, WALL, STEEL);
            fill(&mut s, cx + 35, y0 - 20, cx + 36, y0 - 1, WALL, STEEL);
            clear(&mut s, cx + 32, y0, cx + 34, y0 + 1);
            fill(&mut s, cx + 32, y0 - 20, cx + 34, y0 - 20, VENT, 0);
        }
    }
    s.inlet_p = 2.0;
    s
}

/// Bunsen burners: the left one draws air in through its collar (premixed blue flame), the right
/// one has the air hole closed (yellow, sooty diffusion flame).
pub fn bunsen() -> Scene {
    let mut s = Scene::empty();
    floor(&mut s);
    for (k, cx) in [170, 342].into_iter().enumerate() {
        let (top, bot) = (FL - 48, FL - 8);
        // barrel
        fill(&mut s, cx - 6, top, cx - 4, bot, WALL, STEEL);
        fill(&mut s, cx + 4, top, cx + 6, bot, WALL, STEEL);
        // base
        fill(&mut s, cx - 20, bot + 1, cx + 20, FL, WALL, STEEL);
        // gas jet at the bottom
        for x in cx - 1..=cx + 1 {
            s.put(x, bot, FUEL_IN, 0, 0);
        }
        fill(&mut s, cx - 3, bot, cx - 2, bot, WALL, STEEL);
        fill(&mut s, cx + 2, bot, cx + 3, bot, WALL, STEEL);
        if k == 0 {
            // open collar: air inlets in both sides of the barrel just above the jet
            for y in bot - 8..bot - 3 {
                fill(&mut s, cx - 6, y, cx - 4, y, AIR_IN, 0);
                fill(&mut s, cx + 4, y, cx + 6, y, AIR_IN, 0);
            }
        }
        // pilot just above the mouth
        s.put(cx - 2, top - 5, IGNITER, 0, 0);
        s.put(cx + 2, top - 5, IGNITER, 0, 0);
    }
    s.inlet_p = 1.6;
    s
}

/// A row of candles (wax body, paper wick) lit by a match flame, in a draughty glass lantern + open air.
pub fn candles() -> Scene {
    let mut s = Scene::empty();
    floor(&mut s);
    // a stone shelf
    fill(&mut s, 40, FL - 4, 470, FL, MATTER, STONE);
    for (k, x) in (70..=440).step_by(74).enumerate() {
        let hgt = 30 + (k as i32 % 3) * 14;
        let y_top = FL - 4 - hgt;
        fill(&mut s, x - 5, y_top, x + 5, FL - 5, MATTER, WAX);
        // wick: a thin paper strand down into the wax
        fill(&mut s, x, y_top - 3, x, y_top + 6, MATTER, PAPER);
        // the wick's wax-vapour feed (a candle flame burns wax vapour drawn up the wick)
        s.put(x, y_top - 4, FUEL_IN, 0, 0);
        s.put(x + 1, y_top - 5, IGNITER, 0, 0);
    }
    // a glass lantern chimney around the middle candle
    let x = 70 + 74 * 2;
    fill(&mut s, x - 18, FL - 130, x - 17, FL - 30, MATTER, GLASS);
    fill(&mut s, x + 17, FL - 130, x + 18, FL - 30, MATTER, GLASS);
    s.inlet_p = 1.03;
    s
}

/// Campfire in a stone ring: a split-log teepee over a paper-and-kindling tinder bed, lit by a match.
pub fn campfire_deluxe() -> Scene {
    let mut s = Scene::empty();
    floor(&mut s);
    let cx = 256;
    // stone ring
    for x in [cx - 70, cx + 52] {
        fill(&mut s, x, FL - 14, x + 18, FL, MATTER, STONE);
    }
    // tinder: paper and charcoal
    mix(&mut s, cx - 24, FL - 5, cx + 24, FL, &[(PAPER, 2), (CHARCOAL, 1)]);
    // teepee: two leaning wood poles each side + cross logs
    for k in 0..60 {
        let y = FL - 6 - k;
        let dx = 40 - k * 2 / 3;
        for t in 0..4 {
            s.put(cx - dx + t, y, WALL, WOOD, 0);
            s.put(cx + dx - t, y, WALL, WOOD, 0);
        }
        if k % 20 == 10 {
            let d2 = dx - 4;
            fill(&mut s, cx - d2, y - 2, cx + d2, y, WALL, WOOD);
        }
    }
    // a coal bucket beside it
    hollow(&mut s, 400, FL - 30, 440, FL, 2, WALL, STEEL, true);
    fill(&mut s, 402, FL - 20, 438, FL - 2, MATTER, COAL);
    // the match: tinder igniter + torch on the bottom of the poles and the lowest cross log
    s.put(cx, FL - 6, IGNITER, 0, 0);
    s.put(cx - 37, FL - 7, IGNITER, 0, 0);
    s.put(cx + 37, FL - 7, IGNITER, 0, 0);
    s.ops.push(op(5, 1, cx - 42, FL - 16, cx + 42, FL - 6, 0));
    s
}

/// Oxyliquit: charcoal soaked in liquid oxygen in a paper cartridge, behind a titanium blast wall.
pub fn oxyliquit() -> Scene {
    let mut s = Scene::empty();
    floor(&mut s);
    // cartridge in a sand pit
    fill(&mut s, 60, FL - 20, 200, FL, MATTER, SAND);
    hollow(&mut s, 110, FL - 40, 150, FL - 6, 1, MATTER, PAPER, true);
    mix(&mut s, 111, FL - 34, 149, FL - 7, &[(CHARCOAL, 1), (LOX, 1)]);
    // fuse to a firing point behind the wall
    fill(&mut s, 151, FL - 21, 330, FL - 21, MATTER, FUSE);
    fill(&mut s, 151, FL - 25, 152, FL - 22, MATTER, FUSE);
    s.put(331, FL - 21, IGNITER, 0, 0);
    // titanium blast wall and a wooden shed to show the blast
    fill(&mut s, 250, FL - 120, 258, FL - 22, WALL, TITANIUM);
    fill(&mut s, 250, FL - 20, 258, FL, WALL, TITANIUM);
    hollow(&mut s, 30, FL - 110, 70, FL - 21, 2, WALL, WOOD, false);
    s
}

/// Nitroglycerin chain: one vial is heated by an igniter; its shock wave sets off the others.
pub fn nitro_chain() -> Scene {
    let mut s = Scene::empty();
    floor(&mut s);
    for (k, x) in [120, 160, 200, 240, 280, 330, 390].into_iter().enumerate() {
        let shelf = FL - 40 - (k as i32 % 2) * 12;
        fill(&mut s, x - 12, shelf, x + 12, shelf + 2, WALL, WOOD);
        fill(&mut s, x - 1, shelf + 3, x + 1, FL, WALL, WOOD);
        // glass vial with NG
        hollow(&mut s, x - 5, shelf - 14, x + 5, shelf - 1, 1, MATTER, GLASS, true);
        fill(&mut s, x - 4, shelf - 10, x + 4, shelf - 2, MATTER, NITRO);
        if k == 0 {
            s.put(x, shelf - 11, IGNITER, 0, 0);
        }
    }
    s
}

/// ANFO in two boreholes in rock: left fired with a plain hot wire (it only smoulders), right through
/// a blasting cap + C4 booster (the way miners actually do it).
pub fn anfo() -> Scene {
    let mut s = Scene::empty();
    floor(&mut s);
    // bedrock
    fill(&mut s, 0, FL - 90, W as i32 - 1, FL, MATTER, STONE);
    for (k, cx) in [140, 372].into_iter().enumerate() {
        let (top, bot) = (FL - 90, FL - 15);
        clear(&mut s, cx - 8, top, cx + 8, bot);
        mix(&mut s, cx - 8, top + 30, cx + 8, bot, &[(AN, 94), (DIESEL, 6)]);
        // sand stemming on top
        fill(&mut s, cx - 8, top + 12, cx + 8, top + 29, MATTER, SAND);
        if k == 1 {
            fill(&mut s, cx - 4, bot - 10, cx + 4, bot - 2, MATTER, C4);
            fill(&mut s, cx - 1, bot - 13, cx + 1, bot - 11, MATTER, CAP);
        }
        // electric firing: left a hot bridge wire straight in the ANFO, right the detonator in the cap
        if k == 1 {
            s.put(cx, bot - 14, IGNITER, 0, 0);
        } else {
            s.put(cx, top + 34, IGNITER, 0, 0);
        }
    }
    s
}

/// Flame arrestor (Davy-lamp principle): two pipes of fuel/air lit at one end; the lower pipe has a
/// copper mesh that soaks the heat out of the flame and stops it.
pub fn flame_arrestor() -> Scene {
    let mut s = Scene::empty();
    let (x0, x1) = (40, 300);
    for (k, cy) in [72, 216].into_iter().enumerate() {
        let half = 22;
        fill(&mut s, x0, cy - half - 3, x1, cy - half - 1, WALL, CERAMIC);
        fill(&mut s, x0, cy + half + 1, x1, cy + half + 3, WALL, CERAMIC);
        fill(&mut s, x0 - 3, cy - half - 3, x0 - 1, cy + half + 3, WALL, CERAMIC);
        // open far end
        premix(&mut s, x0, cy - half, x1, cy + half, 1);
        for y in cy - 6..=cy + 6 {
            s.put(x0, y, IGNITER, 0, 0);
        }
        if k == 1 {
            // mesh: 1-cell copper wires with 1-cell gaps, several layers deep
            // plate pack: thin cooled-copper plates with 1-cell slots (crimped-ribbon arrestor)
            for x in 140..160 {
                for y in cy - half..=cy + half {
                    if (y - cy).rem_euclid(2) == 0 {
                        s.put(x, y, WALL, COOLED_CU, 0);
                    }
                }
            }
        }
    }
    s
}

/// De Laval nozzles: identical methane/oxygen chambers, top one converging only (under-expanded,
/// shock diamonds), bottom one converging-diverging (supersonic, straight plume).
pub fn nozzles() -> Scene {
    let mut s = Scene::empty();
    for (k, cy) in [72, 216].into_iter().enumerate() {
        let (xf, xc, xt) = (40, 110, 130);
        let xe = if k == 0 { 132 } else { 180 };
        let half_at = |x: i32| -> i32 {
            if x < xc {
                20
            } else if x < xt {
                20 - 15 * (x - xc) / (xt - xc)
            } else {
                5 + (x - xt) * 10 / 50
            }
        };
        for x in xf..=xe {
            let h = half_at(x);
            for t in 0..6 {
                let m = if t < 2 { COOLED_CU } else { STEEL };
                s.put(x, cy - h - 1 - t, WALL, m, 0);
                s.put(x, cy + h + t, WALL, m, 0);
            }
        }
        fill(&mut s, xf - 4, cy - 27, xf - 1, cy + 26, WALL, STEEL);
        for y in cy - 20..cy + 20 {
            let t = if (y - cy).rem_euclid(4) == 0 { FUEL_IN } else { O2_IN };
            s.put(xf, y, t, 0, 0);
        }
        for x in xf + 3..xf + 6 {
            s.put(x, cy - 21, IGNITER, 0, 0);
            s.put(x, cy + 20, IGNITER, 0, 0);
        }
    }
    s.inlet_p = 10.0;
    s.o2_p = 10.0;
    s
}

/// Valveless pulse-jet: a fuel jet feeds a combustion chamber with a short intake and a long tailpipe;
/// each explosion pushes gas out both ends, the tailpipe's rebound sucks fresh air back in.
pub fn pulsejet() -> Scene {
    let mut s = Scene::empty();
    let cy = 144;
    // intake (x 40..90, half 6) -> chamber (90..170, half 22) -> tailpipe (180..470, half 9)
    let half_at = |x: i32| -> i32 {
        if x < 90 {
            6
        } else if x < 100 {
            6 + (x - 90) * 16 / 10
        } else if x < 165 {
            22
        } else if x < 185 {
            22 - (x - 165) * 13 / 20
        } else {
            9
        }
    };
    for x in 40..=470 {
        let h = half_at(x);
        for t in 0..4 {
            s.put(x, cy - h - 1 - t, WALL, INCONEL, 0);
            s.put(x, cy + h + t, WALL, INCONEL, 0);
        }
    }
    // fuel injectors in the chamber walls, a spark plug behind them
    for x in 100..112 {
        s.put(x, cy - 23, FUEL_IN, 0, 0);
        s.put(x, cy + 22, FUEL_IN, 0, 0);
    }
    for x in 120..123 {
        s.put(x, cy - 23, IGNITER, 0, 0);
    }
    // blower at the intake end
    for y in cy - 6..cy + 6 {
        s.put(40, y, AIR_IN, 0, 0);
    }
    premix(&mut s, 90, cy - 22, 165, cy + 22, 1);
    s.inlet_p = 2.5;
    s
}

/// Gas forge: a ceramic-lined box with a blown premix burner in the back wall; aluminium ingots
/// on the ceramic floor melt (933 K) and the metal runs out through the tap hole into an ingot mould.
pub fn forge() -> Scene {
    let mut s = Scene::empty();
    floor(&mut s);
    let (x0, x1, y0, y1) = (90, 270, FL - 100, FL - 30);
    // stand
    fill(&mut s, x0 + 10, y1 + 1, x0 + 16, FL, WALL, STEEL);
    fill(&mut s, x1 - 16, y1 + 1, x1 - 10, FL, WALL, STEEL);
    // refractory box, floor sloping to the tap hole on the right
    hollow(&mut s, x0, y0, x1, y1, 6, WALL, CERAMIC, false);
    for x in x0 + 6..x1 - 6 {
        let h = (x1 - 6 - x) / 20;
        fill(&mut s, x, y1 - 6 - h, x, y1 - 6, WALL, CERAMIC);
    }
    // flue in the roof
    clear(&mut s, x0 + 30, y0, x0 + 40, y0 + 5);
    // burner: blown premix through the back (left) wall, ~1 fuel : 9 air
    for y in y0 + 20..y0 + 44 {
        let t = if y % 10 == 0 { FUEL_IN } else { AIR_IN };
        s.put(x0 + 5, y, t, 0, 0);
    }
    for y in [y0 + 18, y0 + 46] {
        s.put(x0 + 7, y, IGNITER, 0, 0);
    }
    // aluminium ingots on the floor
    for k in 0..4 {
        let x = x0 + 40 + k * 30;
        let fl = y1 - 7 - (x1 - 6 - x) / 20;
        fill(&mut s, x, fl - 10, x + 18, fl, WALL, ALU);
    }
    // tap hole and a ceramic ingot mould below it
    clear(&mut s, x1 - 5, y1 - 8, x1, y1 - 7);
    hollow(&mut s, x1 + 8, FL - 24, x1 + 70, FL, 3, WALL, CERAMIC, true);
    s.inlet_p = 2.0;
    s
}

/// Garage vapour explosion: a gasoline spill on the floor of a closed wooden shed evaporates, the
/// heavy vapour spreads along the floor and reaches a water heater's pilot flame.
pub fn vapour() -> Scene {
    let mut s = Scene::empty();
    floor(&mut s);
    let (x0, x1, y0) = (60, 450, FL - 130);
    hollow(&mut s, x0, y0, x1, FL, 4, WALL, WOOD, false);
    // spill
    fill(&mut s, 100, FL - 3, 300, FL, MATTER, GASOLINE);
    // jerry can
    hollow(&mut s, 90, FL - 30, 110, FL - 3, 2, WALL, STEEL, false);
    fill(&mut s, 92, FL - 24, 108, FL - 5, MATTER, GASOLINE);
    // water heater with a pilot at floor level
    hollow(&mut s, 340, FL - 100, 390, FL - 8, 3, WALL, STEEL, false);
    fill(&mut s, 343, FL - 90, 387, FL - 11, MATTER, WATER);
    s.put(330, FL - 6, IGNITER, 0, 0);
    s.put(330, FL - 7, FUEL_IN, 0, 0);
    s.inlet_p = 1.05;
    // the warm afternoon: a little evaporation already happened
    s.ops.push(op(2, 1, 100, FL - 14, 300, FL - 4, 0));
    s
}

/// Ribbon burner under three metal bars: aluminium (933 K) melts first, copper (1358 K) later,
/// steel (1700 K+) only glows.
pub fn melt_race() -> Scene {
    let mut s = Scene::empty();
    floor(&mut s);
    // three bars on ceramic pedestals over one long ribbon burner
    fill(&mut s, 60, FL - 2, 452, FL, WALL, CERAMIC);
    for x in 70..=442 {
        if x % 10 == 0 {
            s.put(x, FL - 3, FUEL_IN, 0, 0);
        } else {
            s.put(x, FL - 3, AIR_IN, 0, 0);
        }
    }
    for x in (66..=446).step_by(38) {
        s.put(x, FL - 5, IGNITER, 0, 0);
    }
    for (k, m) in [ALU, COPPER, STEEL].into_iter().enumerate() {
        let cx = 130 + k as i32 * 126;
        fill(&mut s, cx - 4, FL - 25, cx + 4, FL - 4, WALL, CERAMIC);
        fill(&mut s, cx - 20, FL - 28, cx + 20, FL - 26, WALL, CERAMIC);
        fill(&mut s, cx - 14, FL - 40, cx + 14, FL - 29, WALL, m);
    }
    // ceramic hood
    fill(&mut s, 40, FL - 120, 472, FL - 117, WALL, CERAMIC);
    s.inlet_p = 2.0;
    s
}

/// Coal-dust mine gallery: a methane pocket at the face is lit, the blast lifts settled coal dust
/// off the floor and a secondary dust explosion races down the tunnel.
pub fn mine() -> Scene {
    let mut s = Scene::empty();
    let (top, bot) = (100, 190);
    fill(&mut s, 0, 0, W as i32 - 1, top, MATTER, STONE);
    fill(&mut s, 0, bot, W as i32 - 1, H as i32 - 1, MATTER, STONE);
    fill(&mut s, 0, top, 20, bot, MATTER, STONE);
    // wooden pit props
    for x in (80..500).step_by(70) {
        fill(&mut s, x, top + 1, x + 4, bot - 1, WALL, WOOD);
        clear(&mut s, x, top + 10, x + 4, bot - 6);
        fill(&mut s, x - 8, top + 1, x + 12, top + 4, WALL, WOOD);
    }
    // coal dust layer on the floor + a thin suspended haze
    fill(&mut s, 21, bot - 4, W as i32 - 1, bot - 1, MATTER, COAL);
    for x in (30..500).step_by(4) {
        for y in (top + 12..bot - 12).step_by(5) {
            s.put(x + (y % 3), y, MATTER, COAL, 0);
        }
    }
    // firedamp pocket at the face (left), lit by a spark
    premix(&mut s, 21, top + 1, 260, bot - 5, 1);
    s.put(22, top + 30, IGNITER, 0, 0);
    s
}

/// Registry for the UI / bench: (title, one-line description, constructor).
pub fn all() -> Vec<(&'static str, &'static str, fn() -> Scene)> {
    vec![
        ("Rocket motor static fire", "A cored solid-propellant grain burns from the inside out on a nozzle-down test stand", candy_rocket as fn() -> Scene),
        ("Black-powder cannon", "A saltpeter/charcoal/sulfur charge: flash, muzzle blast and a scorched palisade", cannon),
        ("Thermite steam blast", "Molten iron burns through a steel plate into a water tank", thermite_steam),
        ("Sprinkler vs pool fire", "Wax fusible plugs melt over a kerosene fire and let the water main rain down", sprinkler),
        ("Boiler explosion", "Two boilers on burners: the sealed one over-pressures, the one with a safety valve vents", boiler),
        ("Bunsen burners", "Air collar open: blue premixed flame. Closed: yellow, sooty diffusion flame", bunsen),
        ("Candles", "Wax candles with paper wicks: melt pool, vapour and a gentle buoyant flame", candles),
        ("Campfire", "A log teepee over paper and charcoal tinder in a stone ring", campfire_deluxe),
        ("Oxyliquit", "Charcoal soaked in liquid oxygen, fired by fuse behind a titanium blast wall", oxyliquit),
        ("Nitroglycerin chain", "One vial heats until it detonates; the shock sets off the others", nitro_chain),
        ("ANFO boreholes", "Hot wire vs blasting cap + C4 booster: ANFO only detonates with a strong kick", anfo),
        ("Flame arrestor", "A copper mesh quenches a flame racing down a fuel/air pipe (Davy lamp)", flame_arrestor),
        ("De Laval nozzles", "Converging vs converging-diverging nozzle on identical CH4/O2 chambers", nozzles),
        ("Pulse combustor", "Blower-fed combustion chamber with a long tailpipe: watch the Pressure view pulse", pulsejet),
        ("Gas forge", "A blown premix burner melts aluminium ingots; the melt runs out of the tap hole", forge),
        ("Gasoline vapour", "A spill in a closed shed creeps to the water-heater pilot light", vapour),
        ("Melting race", "Aluminium, copper and steel in crucibles over one burner: who melts first?", melt_race),
        ("Firedamp explosion", "Methane in a mine gallery: the flame races past the pit props, igniting coal dust", mine),
    ]
}

/// Bench short names, parallel to `all()`.
pub const NAMES: [&str; 18] = [
    "candy", "cannon", "thermsteam", "sprinkler", "boiler", "bunsen", "candles", "campfire2", "oxyliquit", "nitro", "anfo",
    "arrestor", "nozzles", "pulsejet", "forge", "vapour", "meltrace", "mine",
];

pub fn by_name(name: &str) -> Option<Scene> {
    NAMES.iter().position(|n| *n == name).map(|i| (all()[i].2)())
}
