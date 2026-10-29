//! Missions: short goal-based challenges. Each mission is plain data: a starting layout,
//! the palette and budgets the player gets, feed settings, and a list of objectives.
//! The first objectives marked `required` decide pass/fail; every bonus objective met on a
//! passing run adds a star (1 star for passing + up to 2 bonus stars).
//!
//! Tuning: all the numbers players chase are constants right below.
use crate::elements::Tool;
use crate::gpu::{Op, H, W};
use crate::scene::{Scene, MATTER, AIR_IN, EXPLOSIVE, FAN, FUEL_IN, IGNITER, INCONEL, O2_IN, PROPELLANT, STEEL, WALL, WOOD};

// ---------------------------------------------------------------- tuning
// 1 First light
pub const M1_BURN: f32 = 0.3; // kg/s of fuel burning = "it's alight"
pub const M1_FAST: f32 = 8.0; // s
pub const M1_ROAR: f32 = 0.8; // kg/s
// 2 Campfire
pub const M2_CHARRED: f32 = 10.0; // wood cells burnt away
pub const M2_FAST: f32 = 20.0; // s
pub const M2_WOOD_MAX: f32 = 1200.0;
pub const M2_WOOD_STAR: f32 = 450.0;
// 3 Blast door
pub const M3_BREACH: f32 = 30.0; // titanium cells destroyed
pub const M3_VAULT_T: i32 = 3; // vault wall thickness
pub const M3_EXPL_MAX: f32 = 400.0;
pub const M3_EXPL_2: f32 = 130.0;
pub const M3_EXPL_3: f32 = 80.0;
// 4 Pressure vessel
pub const M4_WALLS_2: f32 = 1000.0;
pub const M4_WALLS_3: f32 = 650.0;
pub const M4_TIME: f32 = 10.0;
// 5 Turbojet
pub const M5_THRUST: f32 = 40.0; // kN
pub const M5_THRUST_3: f32 = 60.0;
pub const M5_HOLD: f32 = 3.0;
pub const M5_HEAT: f32 = 80.0; // % of melting
// 6 Rocket
pub const M6_THRUST: f32 = 150.0;
pub const M6_HOLD: f32 = 10.0;
pub const M6_LOST: f32 = 30.0; // wall cells the engine may lose
pub const M6_THRUST_3: f32 = 200.0;
pub const M6_WALLS_3: f32 = 2600.0;
// 9 Thermite
pub const M9_HOLE: f32 = 8.0; // steel plate cells destroyed
pub const M9_FAST: f32 = 10.0; // s
pub const M9_AL_STAR: f32 = 250.0; // aluminium powder cells
// 10 Fire brigade
pub const M10_OUT: f32 = 2.0; // oil cells still alight: at or below this the fire counts as out
pub const M10_OUT_HOLD: f32 = 3.0;
pub const M10_LIGHT: f32 = 7.0; // s the hot plate keeps heating the oil
pub const M10_SAND_STAR: f32 = 150.0;
// Rocket candy
pub const MC_THRUST: f32 = 12.0; // kN
pub const MC_HOLD_V: f32 = 10.0;
pub const MC_HOLD: f32 = 3.0;
// Dust explosion
pub const MD_ROOF: f32 = 20.0; // aluminium roof cells blown out
pub const MD_FAST: f32 = 15.0;
pub const MD_FLOUR: f32 = 1200.0; // flour sprayed, for the bonus star
// 8 Solid motor
pub const M8_THRUST: f32 = 120.0;
pub const M8_HOLD: f32 = 2.0;
pub const M8_IMPULSE: f32 = 1500.0; // kN·s
pub const M8_PROP_MAX: f32 = 6000.0;

/// Something the game can measure live. Values are in display units.
#[derive(Clone, Copy, PartialEq, Debug)]
#[allow(dead_code)]
pub enum Metric {
    /// net thrust, kN (smoothed)
    Thrust,
    /// fuel burning, kg/s (smoothed)
    Burn,
    /// specific impulse, s: thrust / (propellant flow x g0)
    Isp,
    /// hottest wall, % of its melting point
    WallHeat,
    /// hottest gas, K
    PeakTemp,
    /// highest pressure, atm
    PeakPressure,
    /// structural (non-burnable) wall cells destroyed since the test began
    StructLost,
    /// cells of one material destroyed since the test began
    MatLost(u32),
    /// cells of one material the player has placed (ids 0-9 walls, 10+ powders / liquids)
    Used(u32),
    /// structural wall cells the player has placed
    UsedWalls,
    /// 1 if the gas around the charge can reach the outside, else 0
    Leak,
    /// total impulse, kN·s
    Impulse,
    /// cells of one powder / liquid element that are on fire right now (burning in air or reacting)
    Burning(u32),
}

impl Metric {
    pub fn unit(self) -> &'static str {
        match self {
            Metric::Thrust => "kN",
            Metric::Burn => "kg/s",
            Metric::Isp => "s",
            Metric::WallHeat => "%",
            Metric::PeakTemp => "K",
            Metric::PeakPressure => "atm",
            Metric::Impulse => "kN·s",
            Metric::Leak => "",
            Metric::Burning(_) => "cells alight",
            _ => "cells",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Goal {
    /// metric ≥ v at some moment
    Reach(Metric, f32),
    /// metric ≥ v at some moment before t seconds
    ReachBy(Metric, f32, f32),
    /// metric ≥ v continuously for s seconds (short dips are forgiven)
    Hold(Metric, f32, f32),
    /// metric ≤ v for the whole test
    Keep(Metric, f32),
    /// metric ≤ v when the test starts (budget-style; in live missions: never exceeded)
    AtMost(Metric, f32),
    /// metric ≤ v continuously for s seconds, only once it has been above v (e.g. "fire is out")
    HoldBelow(Metric, f32, f32),
}

pub struct Objective {
    /// `{v}` is replaced by the goal value, `{s}` by the seconds
    pub text: &'static str,
    pub goal: Goal,
    pub required: bool,
}

impl Objective {
    pub fn label(&self) -> String {
        let (v, s) = match self.goal {
            Goal::Reach(_, v) | Goal::Keep(_, v) | Goal::AtMost(_, v) => (v, 0.0),
            Goal::ReachBy(_, v, s) | Goal::Hold(_, v, s) | Goal::HoldBelow(_, v, s) => (v, s),
        };
        let fv = if v.fract().abs() < 1e-3 { format!("{v:.0}") } else { format!("{v:.1}") };
        if let Some(t) = self.protocol(&fv, s) {
            return t;
        }
        self.text.replace("{v}", &fv).replace("{s}", &format!("{s:.0}"))
    }

    /// Lab-protocol phrasing generated from the goal itself ("F ≥ 40 kN for 3 s").
    fn protocol(&self, fv: &str, s: f32) -> Option<String> {
        let sym = |m: Metric| -> Option<String> {
            Some(match m {
                Metric::Thrust => "F".into(),
                Metric::Burn => "ṁ_fuel".into(),
                Metric::Isp => "I_sp".into(),
                Metric::WallHeat => "T_wall,max / T_melt".into(),
                Metric::PeakTemp => "T_max".into(),
                Metric::PeakPressure => "p_max".into(),
                Metric::Impulse => "Total impulse".into(),
                Metric::StructLost => "Structural cells lost".into(),
                Metric::MatLost(k) => format!("{} removed", mat_name(k)),
                Metric::Used(k) => format!("{} used", mat_name(k)),
                Metric::UsedWalls => "Wall cells used".into(),
                Metric::Burning(k) => format!("{} burning", mat_name(k)),
                Metric::Leak => return None,
            })
        };
        let unit = |m: Metric| match m.unit() {
            "" => String::new(),
            "%" => " %".into(),
            "cells alight" => " cells".into(),
            u => format!(" {u}"),
        };
        let first_ok = self.text.contains("on fire") || self.text.starts_with("Light the") || self.text.starts_with("Get it");
        Some(match self.goal {
            Goal::Keep(Metric::Leak, _) => "Vessel remains sealed".into(),
            Goal::Keep(Metric::StructLost, v) if v < 1.0 => "No structural cells lost".into(),
            Goal::Reach(Metric::Burn, _) if first_ok => format!("Ignition achieved (ṁ_fuel ≥ {fv} kg/s)"),
            Goal::ReachBy(Metric::Burn, _, _) if self.text.starts_with("Light") => format!("t_ign ≤ {s:.0} s"),
            Goal::Reach(m, _) => format!("{} ≥ {fv}{}", sym(m)?, unit(m)),
            Goal::ReachBy(m, _, _) => format!("{} ≥ {fv}{} within t ≤ {s:.0} s", sym(m)?, unit(m)),
            Goal::Hold(m, _, _) => format!("{} ≥ {fv}{} sustained {s:.0} s", sym(m)?, unit(m)),
            Goal::Keep(m, _) => format!("{} ≤ {fv}{} throughout", sym(m)?, unit(m)),
            Goal::AtMost(m, v) if v < 0.5 => format!("{} = 0", sym(m)?),
            Goal::AtMost(m, _) => format!("{} ≤ {fv}{}", sym(m)?, unit(m)),
            Goal::HoldBelow(m, _, _) => format!("Extinguished: {} ≤ {fv} held {s:.0} s", sym(m)?),
        })
    }
}

fn mat_name(k: u32) -> String {
    if k < 10 {
        crate::elements::MATS[k as usize].name.to_string()
    } else {
        crate::elements::matter_name(k).to_string()
    }
}

/// Lab-protocol display title for an experiment (internal titles stay the save/progress keys).
pub fn lab_title(title: &str) -> &str {
    match title {
        "First Light" => "Ignition of a CH₄/air cloud",
        "Fire Needs Air" => "Oxygen-limited combustion in a closed vessel",
        "Campfire" => "Wood pyrolysis and char burnout",
        "Cutting Torch" => "Oxy-fuel cutting of steel plate",
        "Shut the Valve" => "Fuel isolation and flame extinction",
        "Forge" => "Forced-draught charcoal forge",
        "Charcoal Grill" => "Charcoal bed heat release",
        "Magnesium Flare" => "Magnesium combustion",
        "Rocket Candy" => "KNO₃/sucrose propellant static fire",
        "Rail Weld" => "Aluminothermic rail weld",
        "Signal Fire" => "Sustained open-air wood fire",
        "Dust Explosion" => "Dispersed-dust deflagration",
        "Alcohol Stove" => "Ethanol evaporative burner",
        "Pool Fire" => "Liquid pool fire",
        "Grease Fire" => "Oil-pan fire suppression",
        "Ice Dam" => "Convective melting of ice",
        "Fuel Vapour" => "Vapour-phase ignition above a liquid fuel",
        "Foundry Pour" => "Metal melting and casting",
        "Hot Section" => "Turbojet hot-section material selection",
        "Solid Motor" => "Solid-propellant motor static fire",
        "Liftoff" => "CH₄/O₂ thrust chamber design",
        "Peroxide Thruster" => "Catalytic H₂O₂ monopropellant thruster",
        "Hybrid Motor" => "Hybrid rocket motor",
        "Blast Door" => "Explosive breaching of a titanium plate",
        "Containment" => "Blast containment vessel",
        "Tower Drop" => "Explosive felling of a chimney",
        "Alkali Splash" => "Alkali metal and water reaction",
        "No Matches" => "Ignition without an open flame",
        "Thermite" => "Thermite burn-through of steel",
        "Pressure Vessel" => "Detonation containment vessel",
        "Blast" => "Blast wave propagation",
        t => t,
    }
}

/// Lab-protocol chapter heading.
pub fn lab_chapter(ch: &str) -> &str {
    match ch {
        "Fire Basics" => "Combustion fundamentals",
        "Powders & Mixtures" => "Particulate and heterogeneous combustion",
        "Liquids & Phase Changes" => "Liquid fuels and phase change",
        "Engines & Rockets" => "Propulsion",
        "Demolition & Extreme Chemistry" => "Energetic materials",
        c => c,
    }
}

#[derive(Clone, Copy)]
pub struct Feeds {
    pub fuel: f32,
    pub o2: f32,
    pub air: f32,
    pub fan: f32,
    /// player may change the feed sliders
    pub adjustable: bool,
}

pub struct Mission {
    pub title: &'static str,
    pub tagline: &'static str,
    pub brief: &'static str,
    pub tip: &'static str,
    pub layout: fn() -> Scene,
    /// a known-good solution (used for testing the numbers; applied on top of `layout`)
    pub solution: fn(&mut Scene),
    /// no build phase: the clock runs from the moment the mission loads and the player acts live
    pub live: bool,
    /// cells of the starting layout can't be erased or overwritten
    pub locked_start: bool,
    pub palette: &'static [Tool],
    /// hard limits checked before a test may start
    pub budgets: &'static [(Metric, f32)],
    pub feeds: Feeds,
    /// igniters stay on this many seconds into the test
    pub igniter_secs: f32,
    /// the test ends after this many seconds
    pub duration: f32,
    pub objectives: &'static [Objective],
    pub view: u32,
    pub gravity: bool,
    /// live missions: seconds into the run at which the test solution is applied (SHOT_SOLVE)
    pub solve_at: f32,
}

const NO_FEEDS: Feeds = Feeds { fuel: 1.0, o2: 1.0, air: 1.0, fan: 0.0, adjustable: false };
const ALWAYS: f32 = 1e9;

const METALS: [Tool; 7] = [Tool::Wall(0), Tool::Wall(1), Tool::Wall(2), Tool::Wall(3), Tool::Wall(4), Tool::Wall(5), Tool::Wall(6)];

fn op(kind: u32, shape: u32, x0: i32, y0: i32, x1: i32, y1: i32, r: i32) -> Op {
    Op { kind, shape, x0, y0, x1, y1, r, tile: 0, mat: 0, dir: 0, _p: [0; 2] }
}

fn floor(s: &mut Scene) {
    for x in 0..W as i32 {
        for y in H as i32 - 6..H as i32 {
            s.put(x, y, WALL, STEEL, 0);
        }
    }
}

fn rect(s: &mut Scene, x0: i32, y0: i32, x1: i32, y1: i32, tile: u32, mat: u32) {
    for x in x0..=x1 {
        for y in y0..=y1 {
            s.put(x, y, tile, mat, 0);
        }
    }
}

fn frame(s: &mut Scene, x0: i32, y0: i32, x1: i32, y1: i32, t: i32, mat: u32) {
    for x in x0..=x1 {
        for y in y0..=y1 {
            if x < x0 + t || x > x1 - t || y < y0 + t || y > y1 - t {
                s.put(x, y, WALL, mat, 0);
            }
        }
    }
}

// ---------------------------------------------------------------- layouts

fn l_first_light() -> Scene {
    let mut s = Scene::empty();
    floor(&mut s);
    for _ in 0..3 {
        s.ops.push(op(2, 0, 256, 190, 256, 190, 40));
    }
    s
}
fn s_first_light(s: &mut Scene) {
    for _ in 0..3 {
        s.ops.push(op(5, 0, 256, 236, 256, 236, 6));
    }
}

const CAMP_X: i32 = 256;
fn l_campfire() -> Scene {
    let mut s = Scene::empty();
    floor(&mut s);
    // a ring of fire-pit stones
    let base = H as i32 - 7;
    rect(&mut s, CAMP_X - 44, base - 4, CAMP_X - 38, base, WALL, STEEL);
    rect(&mut s, CAMP_X + 38, base - 4, CAMP_X + 44, base, WALL, STEEL);
    s
}
fn s_campfire(s: &mut Scene) {
    let base = H as i32 - 7;
    for (k, w) in [(0, 46), (1, 38), (2, 30), (3, 22)].iter() {
        let y0 = base - k * 5;
        for x in CAMP_X - w / 2..CAMP_X + w / 2 {
            if (x - CAMP_X).rem_euclid(9) < 7 {
                for y in y0 - 3..=y0 {
                    s.put(x, y, WALL, WOOD, 0);
                }
            }
        }
    }
    s.put(CAMP_X - 2, base - 5, IGNITER, 0, 0);
    s.put(CAMP_X + 2, base - 5, IGNITER, 0, 0);
}

const VAULT: (i32, i32, i32) = (300, 420, H as i32 - 110);
fn l_blast_door() -> Scene {
    let mut s = Scene::empty();
    floor(&mut s);
    let (vx0, vx1, vy0) = VAULT;
    let h = H as i32;
    for x in vx0..=vx1 {
        for y in vy0..h - 6 {
            if x < vx0 + M3_VAULT_T || x > vx1 - M3_VAULT_T || y < vy0 + M3_VAULT_T {
                s.put(x, y, WALL, 3, 0);
            }
        }
    }
    s
}
fn s_blast_door(s: &mut Scene) {
    let h = H as i32;
    let vx0 = VAULT.0;
    rect(s, vx0 - 8, h - 30, vx0 - 1, h - 7, WALL, EXPLOSIVE);
    s.put(vx0 - 9, h - 10, IGNITER, 0, 0);
}

const CHARGE: (i32, i32) = (256, 144);
fn l_vessel() -> Scene {
    let mut s = Scene::empty();
    let (cx, cy) = CHARGE;
    rect(&mut s, cx - 3, cy - 3, cx + 3, cy + 3, WALL, EXPLOSIVE);
    s.put(cx - 4, cy, IGNITER, 0, 0);
    s
}
fn s_vessel(s: &mut Scene) {
    let (cx, cy) = CHARGE;
    frame(s, cx - 30, cy - 30, cx + 30, cy + 30, 3, 3);
}

fn l_turbojet() -> Scene {
    let mut s = Scene::turbojet();
    // someone built the hot section out of aluminium...
    for v in s.meta.iter_mut() {
        if *v & 0xff == WALL && (*v >> 8) & 0xff == INCONEL {
            *v = WALL | (1 << 8);
        }
    }
    s
}
fn s_turbojet(s: &mut Scene) {
    *s = Scene::turbojet();
}

const RKT_CY: i32 = H as i32 / 2;
const RKT_FACE: i32 = 60;
fn l_rocket() -> Scene {
    let mut s = Scene::empty();
    let cy = RKT_CY;
    let x_face = RKT_FACE;
    // injector plate with showerhead injectors and igniters, nothing else
    rect(&mut s, x_face - 5, cy - 30, x_face - 1, cy + 29, WALL, STEEL);
    for y in cy - 22..cy + 22 {
        let t = if (y - cy).rem_euclid(4) == 0 { FUEL_IN } else { O2_IN };
        s.put(x_face, y, t, 0, 0);
    }
    for x in x_face + 3..x_face + 6 {
        s.put(x, cy - 23, IGNITER, 0, 0);
        s.put(x, cy + 22, IGNITER, 0, 0);
    }
    s.inlet_p = 12.0;
    s.o2_p = 12.0;
    s
}
fn s_rocket(s: &mut Scene) {
    let r = Scene::rocket();
    for (i, v) in r.meta.iter().enumerate() {
        if s.meta[i] == 0 && *v & 0xff == WALL {
            s.meta[i] = *v;
        }
    }
}

const SRM: (i32, i32, i32) = (70, 190, 26);
fn l_solid() -> Scene {
    let mut s = Scene::empty();
    let cy = (H / 2) as i32;
    let (x0, x1, ch) = SRM;
    for x in x0..=x1 {
        for t in 0..4 {
            s.put(x, cy - ch - t, WALL, STEEL, 0);
            s.put(x, cy + ch + t, WALL, STEEL, 0);
        }
    }
    rect(&mut s, x0 - 4, cy - ch - 3, x0 - 1, cy + ch + 3, WALL, STEEL);
    for y in cy - 7..=cy + 7 {
        s.put(x0, y, IGNITER, 0, 0);
    }
    s
}
fn s_solid(s: &mut Scene) {
    let r = Scene::solid_rocket();
    for (i, v) in r.meta.iter().enumerate() {
        if s.meta[i] == 0 && *v != 0 {
            s.meta[i] = *v;
        }
    }
}

const PLATE: (i32, i32, i32) = (196, 316, 160);
fn l_thermite() -> Scene {
    let mut s = Scene::empty();
    floor(&mut s);
    let (x0, x1, y) = PLATE;
    // a steel plate on two pillars, with a strongbox underneath
    rect(&mut s, x0, y, x1, y + 1, WALL, STEEL);
    rect(&mut s, x0, y + 2, x0 + 5, H as i32 - 7, WALL, STEEL);
    rect(&mut s, x1 - 5, y + 2, x1, H as i32 - 7, WALL, STEEL);
    // low lips so powder stays on the plate
    rect(&mut s, x0, y - 6, x0 + 2, y - 1, WALL, STEEL);
    rect(&mut s, x1 - 2, y - 6, x1, y - 1, WALL, STEEL);
    s
}
fn s_thermite(s: &mut Scene) {
    let (_, _, y) = PLATE;
    for x in 236..276 {
        for yy in y - 14..y {
            s.put(x, yy, MATTER, if (x + yy) % 2 == 0 { 15 } else { 17 }, 0);
        }
    }
    s.put(256, y - 15, crate::scene::IGNITER, 0, 0);
}

/// Grease fire: a deep steel pan of cooking oil on a stove, lit by its hot plate (igniters).
pub const OIL: u32 = 33;
const PAN: (i32, i32) = (196, 316);
fn l_grease() -> Scene {
    let mut s = Scene::empty();
    floor(&mut s);
    let h = H as i32;
    let (x0, x1) = PAN;
    // stove block
    rect(&mut s, x0 - 10, h - 30, x1 + 10, h - 7, WALL, STEEL);
    // pan: bottom plate and walls
    rect(&mut s, x0, h - 33, x1, h - 31, WALL, STEEL);
    rect(&mut s, x0, h - 48, x0 + 2, h - 34, WALL, STEEL);
    rect(&mut s, x1 - 2, h - 48, x1, h - 34, WALL, STEEL);
    rect(&mut s, x0 + 3, h - 40, x1 - 3, h - 34, MATTER, OIL);
    // the "hot plate": igniters just above the oil surface
    for x in (x0 + 10..x1 - 10).step_by(14) {
        s.put(x, h - 41, IGNITER, 0, 0);
    }
    s
}
fn s_grease(s: &mut Scene) {
    // the right way: slide a steel lid over the pan to starve the fire of air
    let h = H as i32;
    let (x0, x1) = PAN;
    rect(s, x0 - 1, h - 51, x1 + 1, h - 49, WALL, STEEL);
}


/// Rocket candy test stand: a steel motor case standing upright, nozzle up.
const CANDY: (i32, i32, i32) = (256, 16, 70); // centre x, inner half-width, inner height
fn l_candy() -> Scene {
    let mut s = Scene::empty();
    floor(&mut s);
    let (cx, hw, ih) = CANDY;
    let bot = H as i32 - 7;
    let top = bot - ih;
    // case walls (4 thick) and a nozzle plate with a narrow throat at the top
    rect(&mut s, cx - hw - 4, top - 4, cx - hw - 1, bot, WALL, STEEL);
    rect(&mut s, cx + hw + 1, top - 4, cx + hw + 4, bot, WALL, STEEL);
    rect(&mut s, cx - hw - 4, top - 4, cx - 5, top - 1, WALL, STEEL);
    rect(&mut s, cx + 5, top - 4, cx + hw + 4, top - 1, WALL, STEEL);
    // the expansion bell
    for k in 0..14 {
        let y = top - 5 - k;
        s.put(cx - 6 - k / 2, y, WALL, STEEL, 0);
        s.put(cx - 7 - k / 2, y, WALL, STEEL, 0);
        s.put(cx + 6 + k / 2, y, WALL, STEEL, 0);
        s.put(cx + 7 + k / 2, y, WALL, STEEL, 0);
    }
    s
}
fn s_candy(s: &mut Scene) {
    let (cx, hw, ih) = CANDY;
    let bot = H as i32 - 7;
    let fill_top = bot - ih + 16;
    for x in cx - hw..=cx + hw {
        for y in fill_top..=bot {
            if s.meta[(y * W as i32 + x) as usize] == 0 {
                s.put(x, y, MATTER, if (x + y) % 2 == 0 { 13 } else { 14 }, 0);
            }
        }
    }
    s.put(cx, fill_top - 1, IGNITER, 0, 0);
    s.put(cx - 1, fill_top - 1, IGNITER, 0, 0);
}

/// Flour mill: a sealed steel building with a thin aluminium roof and sacks of flour on the floor.
const MILL: (i32, i32, i32) = (176, 336, 196); // x0, x1, roof y
pub const FLOUR: u32 = 64;
fn l_dust() -> Scene {
    let mut s = Scene::empty();
    floor(&mut s);
    let (x0, x1, ry) = MILL;
    let bot = H as i32 - 7;
    rect(&mut s, x0, ry, x0 + 3, bot, WALL, STEEL);
    rect(&mut s, x1 - 3, ry, x1, bot, WALL, STEEL);
    rect(&mut s, x0, ry, x1, ry, WALL, 1); // thin aluminium roof panels
    // a heap of flour in the middle of the floor
    let cx = (x0 + x1) / 2;
    for x in cx - 30..=cx + 30 {
        let hgt = 10 - (x - cx).abs() / 3;
        for y in bot - hgt.max(1) + 1..=bot {
            s.put(x, y, MATTER, FLOUR, 0);
        }
    }
    s
}
fn s_dust(s: &mut Scene) {
    let (x0, x1, ry) = MILL;
    // spray a thin cloud of flour through the air, then light it
    let mut h: u32 = 12345;
    for y in ry + 6..H as i32 - 20 {
        for x in x0 + 8..x1 - 8 {
            h = h.wrapping_mul(1664525).wrapping_add(1013904223);
            if (h >> 24) < 22 && s.meta[(y * W as i32 + x) as usize] == 0 {
                s.put(x, y, MATTER, FLOUR, 0);
            }
        }
    }
    s.put((x0 + x1) / 2, ry + 40, IGNITER, 0, 0);
}

// ---------------------------------------------------------------- the missions

pub const MISSIONS: [Mission; 11] = [
    Mission {
        title: "First Light",
        tagline: "Light the fuel cloud",
        brief: "A cloud of methane is drifting over the test range. Pick the Torch and set it alight. The cloud is too rich to burn inside, so light its edge where it mixes with air, then spray oxygen in to make it roar.",
        tip: "Click the Torch in Tools, then click at the edge of the cloud (Gases view, key 5, shows where the fuel is).",
        layout: l_first_light,
        solution: s_first_light,
        live: true,
        locked_start: true,
        palette: &[Tool::Gas(5), Tool::Gas(3), Tool::Gas(2)],
        budgets: &[],
        feeds: NO_FEEDS,
        igniter_secs: ALWAYS,
        duration: 30.0,
        objectives: &[
            Objective { text: "Set the cloud on fire", goal: Goal::Reach(Metric::Burn, M1_BURN), required: true },
            Objective { text: "Light it within {s} s", goal: Goal::ReachBy(Metric::Burn, M1_BURN, M1_FAST), required: false },
            Objective { text: "Make it roar: burn {v} kg/s", goal: Goal::Reach(Metric::Burn, M1_ROAR), required: false },
        ],
        view: 0,
        gravity: true,
        solve_at: 0.0,
    },
    Mission {
        title: "Campfire",
        tagline: "Get a wood fire going",
        brief: "Stack wood between the stones and put an igniter in it. Hot wood gives off flammable gas, the gas burns and heats more wood. Build a pile that burns well enough to char away a good amount of wood.",
        tip: "Leave gaps between logs so air can get in, and stack them so flames from below heat the wood above.",
        layout: l_campfire,
        solution: s_campfire,
        live: false,
        locked_start: true,
        palette: &[Tool::Wall(7), Tool::Part(6), Tool::Wall(0)],
        budgets: &[(Metric::Used(7), M2_WOOD_MAX)],
        feeds: NO_FEEDS,
        igniter_secs: ALWAYS,
        duration: 45.0,
        objectives: &[
            Objective { text: "Burn away {v} cells of wood", goal: Goal::Reach(Metric::MatLost(7), M2_CHARRED), required: true },
            Objective { text: "Do it within {s} s", goal: Goal::ReachBy(Metric::MatLost(7), M2_CHARRED, M2_FAST), required: false },
            Objective { text: "Use at most {v} wood cells", goal: Goal::AtMost(Metric::Used(7), M2_WOOD_STAR), required: false },
        ],
        view: 0,
        gravity: true,
        solve_at: 0.0,
    },
    Mission {
        title: "Grease Fire",
        tagline: "Put out a burning pan",
        brief: "A pan of cooking oil has caught fire. Put it out! But careful: burning oil is far hotter than boiling water. Water poured on it flashes into steam instantly and throws the burning oil everywhere.",
        tip: "Firefighters say: put a lid on it. A steel plate over the pan cuts off the air. Sand works too. Never water.",
        layout: l_grease,
        solution: s_grease,
        live: true,
        locked_start: false,
        palette: &[Tool::Wall(0), Tool::Matter(10), Tool::Matter(19)],
        budgets: &[],
        feeds: NO_FEEDS,
        igniter_secs: M10_LIGHT,
        duration: 40.0,
        objectives: &[
            Objective { text: "Put the fire out for {s} s", goal: Goal::HoldBelow(Metric::Burning(OIL), M10_OUT, M10_OUT_HOLD), required: true },
            Objective { text: "Don't use any water", goal: Goal::AtMost(Metric::Used(19), 0.0), required: false },
            Objective { text: "Use at most {v} sand", goal: Goal::AtMost(Metric::Used(10), M10_SAND_STAR), required: false },
        ],
        view: 0,
        gravity: true,
        solve_at: 9.0,
    },
    Mission {
        title: "Rocket Candy",
        tagline: "Fuel a sugar rocket",
        brief: "This steel motor case stands on a test stand, nozzle up. Pour in rocket candy (saltpeter mixed with sugar), put an igniter on top of the grain and fire it. Saltpeter releases oxygen when hot, so the mix burns by itself and the hot gas roars out of the nozzle.",
        tip: "Use the Spray tool to mix the two powders, or alternate strokes. Fill the case well, but leave room above the grain: the burning surface needs space to make gas.",
        layout: l_candy,
        solution: s_candy,
        live: false,
        locked_start: true,
        palette: &[Tool::Matter(13), Tool::Matter(14), Tool::Part(6), Tool::Wall(0)],
        budgets: &[],
        feeds: NO_FEEDS,
        igniter_secs: ALWAYS,
        duration: 30.0,
        objectives: &[
            Objective { text: "Make {v} kN of thrust", goal: Goal::Reach(Metric::Thrust, MC_THRUST), required: true },
            Objective { text: "Hold {v} kN for {s} s", goal: Goal::Hold(Metric::Thrust, MC_HOLD_V, MC_HOLD), required: false },
            Objective { text: "The case survives", goal: Goal::Keep(Metric::StructLost, 0.5), required: false },
        ],
        view: 0,
        gravity: true,
        solve_at: 0.0,
    },
    Mission {
        title: "Blast Door",
        tagline: "Breach the titanium vault",
        brief: "The vault is made of solid titanium, the strongest wall there is. Place explosive against it and an igniter to set it off. Explosive is expensive: the less you use, the more stars.",
        tip: "Blasts push in every direction. Charges pressed flat against the wall, or tamped behind a steel block, put more of their energy into the vault.",
        layout: l_blast_door,
        solution: s_blast_door,
        live: false,
        locked_start: true,
        palette: &[Tool::Wall(9), Tool::Part(6), Tool::Wall(0)],
        budgets: &[(Metric::Used(9), M3_EXPL_MAX)],
        feeds: NO_FEEDS,
        igniter_secs: ALWAYS,
        duration: 8.0,
        objectives: &[
            Objective { text: "Blow out {v} cells of the vault", goal: Goal::Reach(Metric::MatLost(3), M3_BREACH), required: true },
            Objective { text: "Use at most {v} explosive cells", goal: Goal::AtMost(Metric::Used(9), M3_EXPL_2), required: false },
            Objective { text: "Use at most {v} explosive cells", goal: Goal::AtMost(Metric::Used(9), M3_EXPL_3), required: false },
        ],
        view: 0,
        gravity: true,
        solve_at: 0.0,
    },
    Mission {
        title: "Thermite",
        tagline: "Burn through steel",
        brief: "Something valuable sits under a steel plate. Mix rust and aluminium powder on top of it and light it: thermite is hard to ignite, but once it goes it makes molten iron at 2800 K that melts straight through steel.",
        tip: "Mix the two powders well (alternate strokes or paint one over the other) and pile them right on the plate. The igniter has to touch the mix.",
        layout: l_thermite,
        solution: s_thermite,
        live: false,
        locked_start: true,
        palette: &[Tool::Matter(17), Tool::Matter(15), Tool::Part(6), Tool::Matter(10)],
        budgets: &[],
        feeds: NO_FEEDS,
        igniter_secs: ALWAYS,
        duration: 40.0,
        objectives: &[
            Objective { text: "Melt {v} cells of steel", goal: Goal::Reach(Metric::MatLost(0), M9_HOLE), required: true },
            Objective { text: "Burn through within {s} s", goal: Goal::ReachBy(Metric::MatLost(0), M9_HOLE, M9_FAST), required: false },
            Objective { text: "Use at most {v} aluminium powder", goal: Goal::AtMost(Metric::Used(15), M9_AL_STAR), required: false },
        ],
        view: 0,
        gravity: true,
        solve_at: 0.0,
    },
    Mission {
        title: "Dust Explosion",
        tagline: "Why flour mills explode",
        brief: "A heap of flour barely burns: air only reaches its surface. But blow the same flour into a cloud and every grain is surrounded by air, and the whole cloud burns almost at once. Make the flour mill blow its aluminium roof off.",
        tip: "Pick Flour and the Spray tool and dust the air inside the mill, then add an igniter in the cloud. Fans blowing upwards under the heap also raise a cloud.",
        layout: l_dust,
        solution: s_dust,
        live: false,
        locked_start: true,
        palette: &[Tool::Matter(FLOUR), Tool::Part(6), Tool::Part(5), Tool::Matter(14)],
        budgets: &[],
        feeds: Feeds { fuel: 1.0, o2: 1.0, air: 1.0, fan: 1.5, adjustable: false },
        igniter_secs: ALWAYS,
        duration: 25.0,
        objectives: &[
            Objective { text: "Blow out {v} cells of the roof", goal: Goal::Reach(Metric::MatLost(1), MD_ROOF), required: true },
            Objective { text: "Do it within {s} s", goal: Goal::ReachBy(Metric::MatLost(1), MD_ROOF, MD_FAST), required: false },
            Objective { text: "Spray at most {v} flour cells", goal: Goal::AtMost(Metric::Used(FLOUR), MD_FLOUR), required: false },
        ],
        view: 0,
        gravity: true,
        solve_at: 0.0,
    },
    Mission {
        title: "Pressure Vessel",
        tagline: "Contain a detonation",
        brief: "There's a charge of high explosive in the middle of the range. Build a sealed vessel around it that survives the blast without springing a leak. Lighter vessels earn more stars.",
        tip: "Give the blast room to expand: pressure drops as the gas spreads into a bigger volume. Titanium is strongest per cell.",
        layout: l_vessel,
        solution: s_vessel,
        live: false,
        locked_start: true,
        palette: &METALS,
        budgets: &[],
        feeds: NO_FEEDS,
        igniter_secs: ALWAYS,
        duration: M4_TIME,
        objectives: &[
            Objective { text: "The vessel stays sealed", goal: Goal::Keep(Metric::Leak, 0.5), required: true },
            Objective { text: "Use at most {v} wall cells", goal: Goal::AtMost(Metric::UsedWalls, M4_WALLS_2), required: false },
            Objective { text: "Use at most {v} wall cells", goal: Goal::AtMost(Metric::UsedWalls, M4_WALLS_3), required: false },
        ],
        view: 1,
        gravity: false,
        solve_at: 0.0,
    },
    Mission {
        title: "Hot Section",
        tagline: "Fix the turbojet",
        brief: "This turbojet's hot section was built from aluminium, and it melts the moment the engine lights. Rebuild it from something that can take the heat, then make 40 kN of thrust without any wall going over 80% of its melting point.",
        tip: "Hover a material to compare melting points. Inconel and ceramic love heat; more fuel means more thrust but hotter walls.",
        layout: l_turbojet,
        solution: s_turbojet,
        live: false,
        locked_start: false,
        palette: &[Tool::Wall(0), Tool::Wall(1), Tool::Wall(3), Tool::Wall(4), Tool::Wall(5), Tool::Wall(6), Tool::Part(5), Tool::Part(2), Tool::Part(6)],
        budgets: &[],
        feeds: Feeds { fuel: 3.0, o2: 1.0, air: 2.0, fan: 1.0, adjustable: true },
        igniter_secs: ALWAYS,
        duration: 30.0,
        objectives: &[
            Objective { text: "Hold {v} kN for {s} s", goal: Goal::Hold(Metric::Thrust, M5_THRUST, M5_HOLD), required: true },
            Objective { text: "No wall above {v}% of melting", goal: Goal::Keep(Metric::WallHeat, M5_HEAT), required: true },
            Objective { text: "Hold {v} kN for {s} s", goal: Goal::Hold(Metric::Thrust, M5_THRUST_3, M5_HOLD), required: false },
            Objective { text: "No wall lost", goal: Goal::Keep(Metric::StructLost, 0.5), required: false },
        ],
        view: 0,
        gravity: true,
        solve_at: 0.0,
    },
    Mission {
        title: "Solid Motor",
        tagline: "Pack a solid rocket",
        brief: "Here's an empty steel motor case with an igniter at the head end. Cast a propellant grain inside and build a nozzle. Solid propellant burns at its exposed surface, and burns faster at higher pressure: the grain's shape is the throttle.",
        tip: "Leave a hollow bore down the middle so the whole length burns at once. A throat that's too narrow raises pressure until the case bursts.",
        layout: l_solid,
        solution: s_solid,
        live: false,
        locked_start: true,
        palette: &[Tool::Wall(8), Tool::Wall(0), Tool::Wall(3), Tool::Wall(4), Tool::Wall(5)],
        budgets: &[(Metric::Used(8), M8_PROP_MAX)],
        feeds: NO_FEEDS,
        igniter_secs: ALWAYS,
        duration: 25.0,
        objectives: &[
            Objective { text: "Hold {v} kN for {s} s", goal: Goal::Hold(Metric::Thrust, M8_THRUST, M8_HOLD), required: true },
            Objective { text: "The case survives (no wall lost)", goal: Goal::Keep(Metric::StructLost, 0.5), required: false },
            Objective { text: "Total impulse {v} kN·s", goal: Goal::Reach(Metric::Impulse, M8_IMPULSE), required: false },
        ],
        view: 0,
        gravity: false,
        solve_at: 0.0,
    },
    Mission {
        title: "Liftoff",
        tagline: "Build a rocket engine",
        brief: "You have an injector plate that sprays methane and oxygen at 12 atm. Build the combustion chamber and nozzle around it. The engine must make 150 kN for 10 seconds without falling apart.",
        tip: "A chamber, a narrow throat, then a widening bell. Rocket flames are 3500 K: line the hot side with cooled copper and back it with steel.",
        layout: l_rocket,
        solution: s_rocket,
        live: false,
        locked_start: true,
        palette: &METALS,
        budgets: &[],
        feeds: Feeds { fuel: 12.0, o2: 12.0, air: 1.0, fan: 0.0, adjustable: false },
        igniter_secs: ALWAYS,
        duration: 25.0,
        objectives: &[
            Objective { text: "Hold {v} kN for {s} s", goal: Goal::Hold(Metric::Thrust, M6_THRUST, M6_HOLD), required: true },
            Objective { text: "Lose at most {v} wall cells", goal: Goal::Keep(Metric::StructLost, M6_LOST), required: true },
            Objective { text: "Hold {v} kN for {s} s", goal: Goal::Hold(Metric::Thrust, M6_THRUST_3, 5.0), required: false },
            Objective { text: "Use at most {v} wall cells", goal: Goal::AtMost(Metric::UsedWalls, M6_WALLS_3), required: false },
        ],
        view: 0,
        gravity: false,
        solve_at: 0.0,
    },
];

/// Silence "unused" warnings for tiles only referenced by some layouts.
#[allow(dead_code)]
const _USED: [u32; 4] = [AIR_IN, FAN, PROPELLANT, EXPLOSIVE];
