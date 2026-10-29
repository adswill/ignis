//! IGNIS campaign: 5 chapters of missions loaded from `assets/campaign/campaign.json`.
//!
//! Self-contained on purpose (no crate-internal imports, no dependencies): a tiny JSON parser,
//! plain data types that mirror `missions.rs` (`Metric`, `Goal`, objectives, feeds, palette), and a
//! scene builder that turns a mission's primitive placements into a meta layout (`tile | mat << 8 |
//! dir << 16`, exactly what `Scene::put` writes) plus gas pre-fill ops (fields of `gpu::Op`).
//!
//! The JSON is embedded at compile time; `load_from_str` / `load_file` let modders load their own.
//!
//! Units in objectives are display units, the same ones the mission card shows:
//! thrust kN, burn kg/s, isp s, wall_heat % of melting, peak_temp K, peak_pressure atm,
//! impulse kN*s, cell counts for lost / used / count.

#![allow(dead_code)]

pub const W: i32 = 512;
pub const H: i32 = 288;

/// Embedded campaign file (path is relative to this source file).
pub const CAMPAIGN_JSON: &str = include_str!("../assets/campaign/campaign.json");

// ------------------------------------------------------------------ ids

pub const T_GAS: u32 = 0;
pub const T_WALL: u32 = 1;
pub const T_FUEL_IN: u32 = 2;
pub const T_O2_IN: u32 = 3;
pub const T_AIR_IN: u32 = 4;
pub const T_FAN: u32 = 5;
pub const T_IGNITER: u32 = 6;
pub const T_VENT: u32 = 7;
pub const T_MATTER: u32 = 8;

/// Element / material names by id (walls 0-9, matter 10-65; must match matter.wgsl).
pub const ELEMENT_NAMES: [&str; 66] = [
    "steel", "aluminium", "copper", "titanium", "inconel", "ceramic", "cooled_copper", "wood", "propellant", "explosive",
    "sand", "charcoal", "sulfur", "saltpeter", "sugar", "al_powder", "ammonium_perchlorate", "rust", "ammonium_nitrate", "water",
    "kerosene", "ethanol", "lox", "nitroglycerin", "molten_metal", "ash", "ice", "snow", "salt", "brine",
    "liquid_nitrogen", "gasoline", "diesel", "cooking_oil", "napalm", "wax", "molten_wax", "coal", "paper", "magnesium",
    "iron_filings", "sodium", "potassium", "lye", "hydrochloric_acid", "hydrogen_peroxide", "manganese_dioxide", "potassium_perchlorate", "nitrocellulose", "tnt",
    "c4", "dynamite", "blasting_cap", "glass", "molten_glass", "lava", "stone", "gunpowder", "dry_ice", "copper_oxide",
    "potassium_permanganate", "glycerin", "plastic", "molten_plastic", "flour", "fuse",
];

/// A few friendly aliases accepted in the JSON.
const ALIASES: [(&str, u32); 14] = [
    ("aluminum", 1), ("al", 1), ("ti", 3), ("cu", 2), ("kno3", 13), ("alpw", 15), ("ap", 16), ("an", 18),
    ("lox", 22), ("ng", 23), ("ln2", 30), ("htp", 45), ("mno2", 46), ("mg", 39),
];

pub fn element_id(name: &str) -> Option<u32> {
    let n = name.trim().to_ascii_lowercase().replace([' ', '-'], "_");
    if let Ok(v) = n.parse::<u32>() {
        return Some(v);
    }
    ELEMENT_NAMES.iter().position(|e| *e == n).map(|i| i as u32).or_else(|| ALIASES.iter().find(|a| a.0 == n).map(|a| a.1))
}

pub fn element_name(id: u32) -> &'static str {
    ELEMENT_NAMES.get(id as usize).copied().unwrap_or("?")
}

pub fn tile_id(name: &str) -> Option<u32> {
    Some(match name.trim().to_ascii_lowercase().as_str() {
        "gas" | "air" | "empty" | "clear" => T_GAS,
        "wall" => T_WALL,
        "fuel_in" | "fuel_inlet" => T_FUEL_IN,
        "o2_in" | "o2_inlet" | "oxygen_inlet" => T_O2_IN,
        "air_in" | "air_inlet" => T_AIR_IN,
        "fan" => T_FAN,
        "igniter" => T_IGNITER,
        "vent" => T_VENT,
        "matter" => T_MATTER,
        _ => return None,
    })
}

// ------------------------------------------------------------------ data types (mirror missions.rs)

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Metric {
    Thrust,
    Burn,
    Isp,
    WallHeat,
    PeakTemp,
    PeakPressure,
    /// structural (ids 0-6) wall cells destroyed since the test began
    StructLost,
    /// wall cells of material 0-9 destroyed since the test began
    MatLost(u32),
    /// cells of an id the player has placed
    Used(u32),
    UsedWalls,
    Leak,
    Impulse,
    /// cells of an element id present right now (phase-change goals: ice made, water left...).
    /// Not in missions.rs yet: see the integration notes.
    Count(u32),
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Goal {
    Reach(Metric, f32),
    ReachBy(Metric, f32, f32),
    Hold(Metric, f32, f32),
    Keep(Metric, f32),
    AtMost(Metric, f32),
    HoldBelow(Metric, f32, f32),
}

impl Goal {
    pub fn metric(&self) -> Metric {
        match *self {
            Goal::Reach(m, _) | Goal::ReachBy(m, _, _) | Goal::Hold(m, _, _) | Goal::Keep(m, _) | Goal::AtMost(m, _) | Goal::HoldBelow(m, _, _) => m,
        }
    }
    pub fn value(&self) -> f32 {
        match *self {
            Goal::Reach(_, v) | Goal::ReachBy(_, v, _) | Goal::Hold(_, v, _) | Goal::Keep(_, v) | Goal::AtMost(_, v) | Goal::HoldBelow(_, v, _) => v,
        }
    }
    pub fn secs(&self) -> f32 {
        match *self {
            Goal::ReachBy(_, _, s) | Goal::Hold(_, _, s) | Goal::HoldBelow(_, _, s) => s,
            _ => 0.0,
        }
    }
}

#[derive(Clone, Debug)]
pub struct ObjectiveDef {
    /// `{v}` / `{s}` placeholders, like missions.rs
    pub text: String,
    pub goal: Goal,
    /// required objectives decide pass/fail; each met star objective adds a star (1 + up to 2)
    pub required: bool,
}

impl ObjectiveDef {
    pub fn label(&self) -> String {
        let (v, s) = (self.goal.value(), self.goal.secs());
        let fv = if v.fract().abs() < 1e-3 { format!("{v:.0}") } else { format!("{v:.1}") };
        self.text.replace("{v}", &fv).replace("{s}", &format!("{s:.0}"))
    }
}

/// Palette entries, same meaning as `elements::Tool`.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum PaletteItem {
    Wall(u32),
    /// machine tile id (fan 5, igniter 6, vent 7, fuel inlet 2, O2 inlet 3, air inlet 4)
    Part(u32),
    /// gas tool = op kind (2 fuel, 3 O2, 4 air, 5 torch, 6 chill, 7 vacuum)
    Gas(u32),
    Matter(u32),
    Erase,
}

#[derive(Clone, Copy, Debug)]
pub struct Feeds {
    pub fuel: f32,
    pub o2: f32,
    pub air: f32,
    pub fan: f32,
    pub adjustable: bool,
}

impl Default for Feeds {
    fn default() -> Self {
        Feeds { fuel: 1.0, o2: 1.0, air: 1.0, fan: 0.0, adjustable: false }
    }
}

/// One cell value to paint: tile, material / element id, fan direction.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Cell {
    pub tile: u32,
    pub mat: u32,
    pub dir: u32,
}

impl Cell {
    pub fn meta(&self) -> u32 {
        self.tile | (self.mat << 8) | (self.dir << 16)
    }
}

/// Plain copy of `gpu::Op` (gas edits applied when the scene loads / the test starts).
#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub struct OpData {
    /// 2 add fuel, 3 add O2, 4 add air, 5 torch, 6 chill, 7 vacuum (0 place, 1 erase also valid)
    pub kind: u32,
    /// 0 disc (x0,y0,r), 1 filled box, 2 hollow box, 3 line
    pub shape: u32,
    pub x0: i32,
    pub y0: i32,
    pub x1: i32,
    pub y1: i32,
    pub r: i32,
    pub tile: u32,
    pub mat: u32,
    pub dir: u32,
}

/// Placement primitives. Coordinates are cells, inclusive; later primitives overwrite earlier ones,
/// exactly like successive `Scene::put` calls.
#[derive(Clone, Debug)]
pub enum Prim {
    /// steel (or `cell`) floor over the bottom 6 rows (y 282..287); first free row is y = 281
    Floor { cell: Cell },
    Rect { x0: i32, y0: i32, x1: i32, y1: i32, cell: Cell },
    /// box filled in vertical stripes: columns where `(x - x0) % period < on` (log piles, grates)
    Stripes { x0: i32, y0: i32, x1: i32, y1: i32, period: i32, on: i32, cell: Cell },
    /// box outline `t` thick; `open_top` leaves the top side open (a tray / tub / jar without lid)
    Hollow { x0: i32, y0: i32, x1: i32, y1: i32, t: i32, open_top: bool, cell: Cell },
    Disc { x: i32, y: i32, r: i32, cell: Cell },
    /// thick line, half-width `w` (0 = 1 cell)
    Line { x0: i32, y0: i32, x1: i32, y1: i32, w: i32, cell: Cell },
    /// intimately mixed powders: cells cycle through `mats` (matter tile)
    Mix { x0: i32, y0: i32, x1: i32, y1: i32, mats: Vec<u32> },
    /// triangular pile standing on row `base`: half-width `w` at the bottom, `h` rows tall
    Heap { x: i32, base: i32, w: i32, h: i32, mats: Vec<u32> },
    /// sparse random grains (dust clouds): roughly `density` (0..1) of the free cells in the box
    Scatter { x0: i32, y0: i32, x1: i32, y1: i32, density: f32, seed: u32, mats: Vec<u32>, only_empty: bool },
    /// gas edit (fuel / O2 / air cloud, torch heat, chill, vacuum)
    Gas(OpData),
}

#[derive(Clone, Debug)]
pub struct ChapterDef {
    pub id: u32,
    pub title: String,
    pub blurb: String,
}

#[derive(Clone, Debug)]
pub struct MissionDef {
    /// stable string id (used for progress saves and unlocks)
    pub id: String,
    pub chapter: u32,
    /// position in the campaign (1-based, global)
    pub order: u32,
    pub title: String,
    pub tagline: String,
    /// 2-3 punchy sentences with real-world framing
    pub briefing: String,
    pub hints: Vec<String>,
    /// mission ids that must be passed (>= 1 star) before this unlocks; empty = open from the start
    pub requires: Vec<String>,
    /// no build phase: the clock runs from the start and the player acts live
    pub live: bool,
    pub locked_start: bool,
    pub gravity: bool,
    pub view: u32,
    pub duration: f32,
    /// igniters stay on this many seconds into the test (1e9 = always)
    pub igniter_secs: f32,
    pub feeds: Feeds,
    pub palette: Vec<PaletteItem>,
    pub budgets: Vec<(Metric, f32)>,
    pub objectives: Vec<ObjectiveDef>,
    /// short descriptions of the 3 stars (derived from the objectives if the JSON omits them)
    pub stars: Vec<String>,
    pub layout: Vec<Prim>,
    /// a known-good solution, applied on top of the layout (testing / SHOT_SOLVE / "show me")
    pub solution: Vec<Prim>,
    /// live missions: seconds into the run at which the solution is applied
    pub solve_at: f32,
    /// real-world fact shown after the mission
    pub fact: String,
}

#[derive(Clone, Debug)]
pub struct Campaign {
    pub chapters: Vec<ChapterDef>,
    pub missions: Vec<MissionDef>,
}

/// A built layout: meta per cell (W*H, row-major) + gas ops to apply on load.
#[derive(Clone, Debug)]
pub struct CampaignScene {
    pub meta: Vec<u32>,
    pub ops: Vec<OpData>,
    /// feed pressures (copy of the mission's feeds, for `Scene::inlet_p / o2_p`)
    pub inlet_p: f32,
    pub o2_p: f32,
}

impl CampaignScene {
    pub fn empty() -> Self {
        CampaignScene { meta: vec![0; (W * H) as usize], ops: vec![], inlet_p: 1.0, o2_p: 1.0 }
    }
    pub fn put(&mut self, x: i32, y: i32, c: Cell) {
        if x >= 0 && y >= 0 && x < W && y < H {
            self.meta[(y * W + x) as usize] = c.meta();
        }
    }
    pub fn get(&self, x: i32, y: i32) -> u32 {
        if x >= 0 && y >= 0 && x < W && y < H {
            self.meta[(y * W + x) as usize]
        } else {
            0
        }
    }
    pub fn apply(&mut self, prims: &[Prim]) {
        for p in prims {
            self.apply_one(p);
        }
    }
    fn apply_one(&mut self, p: &Prim) {
        let mc = |m: u32| Cell { tile: T_MATTER, mat: m, dir: 0 };
        match p {
            Prim::Floor { cell } => {
                for x in 0..W {
                    for y in H - 6..H {
                        self.put(x, y, *cell);
                    }
                }
            }
            Prim::Rect { x0, y0, x1, y1, cell } => {
                for x in *x0.min(x1)..=*x0.max(x1) {
                    for y in *y0.min(y1)..=*y0.max(y1) {
                        self.put(x, y, *cell);
                    }
                }
            }
            Prim::Stripes { x0, y0, x1, y1, period, on, cell } => {
                for x in *x0..=*x1 {
                    if (x - x0).rem_euclid((*period).max(1)) >= *on {
                        continue;
                    }
                    for y in *y0..=*y1 {
                        self.put(x, y, *cell);
                    }
                }
            }
            Prim::Hollow { x0, y0, x1, y1, t, open_top, cell } => {
                for x in *x0..=*x1 {
                    for y in *y0..=*y1 {
                        let edge = x < x0 + t || x > x1 - t || y > y1 - t || (!open_top && y < y0 + t);
                        if edge {
                            self.put(x, y, *cell);
                        }
                    }
                }
            }
            Prim::Disc { x, y, r, cell } => {
                for dy in -r..=*r {
                    for dx in -r..=*r {
                        if dx * dx + dy * dy <= r * r {
                            self.put(x + dx, y + dy, *cell);
                        }
                    }
                }
            }
            Prim::Line { x0, y0, x1, y1, w, cell } => {
                let n = (x1 - x0).abs().max((y1 - y0).abs()).max(1);
                for k in 0..=n {
                    let t = k as f32 / n as f32;
                    let cx = (*x0 as f32 + (x1 - x0) as f32 * t).round() as i32;
                    let cy = (*y0 as f32 + (y1 - y0) as f32 * t).round() as i32;
                    for dy in -w..=*w {
                        for dx in -w..=*w {
                            self.put(cx + dx, cy + dy, *cell);
                        }
                    }
                }
            }
            Prim::Mix { x0, y0, x1, y1, mats } => {
                if mats.is_empty() {
                    return;
                }
                for x in *x0..=*x1 {
                    for y in *y0..=*y1 {
                        let k = ((x * 7 + y * 3) as usize + (x * y).unsigned_abs() as usize % 5) % mats.len();
                        self.put(x, y, mc(mats[k]));
                    }
                }
            }
            Prim::Heap { x, base, w, h, mats } => {
                if mats.is_empty() || *h <= 0 {
                    return;
                }
                for k in 0..*h {
                    let hw = w - (w * k) / h;
                    let y = base - k;
                    for xx in x - hw..=x + hw {
                        let j = ((xx * 7 + y * 3) as usize + (xx * y).unsigned_abs() as usize % 5) % mats.len();
                        self.put(xx, y, mc(mats[j]));
                    }
                }
            }
            Prim::Scatter { x0, y0, x1, y1, density, seed, mats, only_empty } => {
                if mats.is_empty() {
                    return;
                }
                let mut h: u32 = seed.wrapping_mul(2654435761).wrapping_add(12345);
                let thr = (density.clamp(0.0, 1.0) * 4294967295.0) as u32;
                let mut k = 0usize;
                for y in *y0..=*y1 {
                    for x in *x0..=*x1 {
                        h = h.wrapping_mul(1664525).wrapping_add(1013904223);
                        if h < thr && (!only_empty || self.get(x, y) == 0) {
                            self.put(x, y, mc(mats[k % mats.len()]));
                            k += 1;
                        }
                    }
                }
            }
            Prim::Gas(op) => self.ops.push(*op),
        }
    }
}

/// The starting layout of a mission (what the player sees at the start; locked cells).
pub fn build_scene(m: &MissionDef) -> CampaignScene {
    let mut s = CampaignScene::empty();
    s.inlet_p = m.feeds.fuel;
    s.o2_p = m.feeds.o2;
    s.apply(&m.layout);
    s
}

/// The layout with the known-good solution applied on top.
pub fn build_solution(m: &MissionDef) -> CampaignScene {
    let mut s = build_scene(m);
    s.apply(&m.solution);
    s
}

/// The embedded campaign. Panics with a readable message if the bundled JSON is malformed
/// (it is checked by `examples/campaign_check.rs`).
pub fn campaign() -> Campaign {
    load_from_str(CAMPAIGN_JSON).unwrap_or_else(|e| panic!("assets/campaign/campaign.json: {e}"))
}

pub fn missions() -> Vec<MissionDef> {
    campaign().missions
}

pub fn chapters() -> Vec<ChapterDef> {
    campaign().chapters
}

/// Load a campaign from a file on disk (modding / hot reload).
pub fn load_file(path: &std::path::Path) -> Result<Campaign, String> {
    let s = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    load_from_str(&s)
}

/// Is a mission available? `passed(id)` should say whether that mission has >= 1 star.
pub fn is_unlocked(m: &MissionDef, passed: &dyn Fn(&str) -> bool) -> bool {
    m.requires.iter().all(|r| passed(r))
}

/// Stars for a finished run: 0 if any required objective failed, else 1 + met star objectives (max 3).
pub fn stars_for(m: &MissionDef, met: &[bool]) -> u32 {
    let mut req_ok = true;
    let mut bonus = 0;
    for (o, &ok) in m.objectives.iter().zip(met) {
        if o.required {
            req_ok &= ok;
        } else if ok {
            bonus += 1;
        }
    }
    if req_ok { (1 + bonus).min(3) } else { 0 }
}

// ------------------------------------------------------------------ JSON-lite parser

#[derive(Clone, Debug, PartialEq)]
pub enum Json {
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
    Arr(Vec<Json>),
    Obj(Vec<(String, Json)>),
}

impl Json {
    pub fn get(&self, k: &str) -> Option<&Json> {
        match self {
            Json::Obj(v) => v.iter().find(|(a, _)| a == k).map(|(_, b)| b),
            _ => None,
        }
    }
    pub fn as_f(&self) -> Option<f64> {
        match self {
            Json::Num(n) => Some(*n),
            _ => None,
        }
    }
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Json::Str(s) => Some(s),
            _ => None,
        }
    }
    pub fn as_arr(&self) -> &[Json] {
        match self {
            Json::Arr(v) => v,
            _ => &[],
        }
    }
}

struct Parser<'a> {
    b: &'a [u8],
    i: usize,
}

impl<'a> Parser<'a> {
    fn err<T>(&self, msg: &str) -> Result<T, String> {
        let line = self.b[..self.i.min(self.b.len())].iter().filter(|&&c| c == b'\n').count() + 1;
        Err(format!("JSON error at line {line}: {msg}"))
    }
    fn ws(&mut self) {
        while self.i < self.b.len() {
            match self.b[self.i] {
                b' ' | b'\t' | b'\n' | b'\r' => self.i += 1,
                // allow // comments (JSON-lite)
                b'/' if self.b.get(self.i + 1) == Some(&b'/') => {
                    while self.i < self.b.len() && self.b[self.i] != b'\n' {
                        self.i += 1;
                    }
                }
                _ => break,
            }
        }
    }
    fn value(&mut self) -> Result<Json, String> {
        self.ws();
        let Some(&c) = self.b.get(self.i) else { return self.err("unexpected end") };
        match c {
            b'{' => {
                self.i += 1;
                let mut v = vec![];
                loop {
                    self.ws();
                    if self.b.get(self.i) == Some(&b'}') {
                        self.i += 1;
                        break;
                    }
                    let k = match self.value()? {
                        Json::Str(s) => s,
                        _ => return self.err("object key must be a string"),
                    };
                    self.ws();
                    if self.b.get(self.i) != Some(&b':') {
                        return self.err("expected ':'");
                    }
                    self.i += 1;
                    let x = self.value()?;
                    v.push((k, x));
                    self.ws();
                    match self.b.get(self.i) {
                        Some(b',') => self.i += 1,
                        Some(b'}') => {}
                        _ => return self.err("expected ',' or '}'"),
                    }
                }
                Ok(Json::Obj(v))
            }
            b'[' => {
                self.i += 1;
                let mut v = vec![];
                loop {
                    self.ws();
                    if self.b.get(self.i) == Some(&b']') {
                        self.i += 1;
                        break;
                    }
                    v.push(self.value()?);
                    self.ws();
                    match self.b.get(self.i) {
                        Some(b',') => self.i += 1,
                        Some(b']') => {}
                        _ => return self.err("expected ',' or ']'"),
                    }
                }
                Ok(Json::Arr(v))
            }
            b'"' => {
                self.i += 1;
                let mut s = Vec::new();
                loop {
                    let Some(&c) = self.b.get(self.i) else { return self.err("unterminated string") };
                    self.i += 1;
                    match c {
                        b'"' => break,
                        b'\\' => {
                            let Some(&e) = self.b.get(self.i) else { return self.err("bad escape") };
                            self.i += 1;
                            match e {
                                b'n' => s.push(b'\n'),
                                b't' => s.push(b'\t'),
                                b'u' => {
                                    let hex = std::str::from_utf8(self.b.get(self.i..self.i + 4).unwrap_or(b"0000")).unwrap_or("0000");
                                    let cp = u32::from_str_radix(hex, 16).unwrap_or(63);
                                    self.i += 4;
                                    let ch = char::from_u32(cp).unwrap_or('?');
                                    let mut buf = [0u8; 4];
                                    s.extend_from_slice(ch.encode_utf8(&mut buf).as_bytes());
                                }
                                other => s.push(other),
                            }
                        }
                        _ => s.push(c),
                    }
                }
                String::from_utf8(s).map(Json::Str).or_else(|_| self.err("invalid utf-8"))
            }
            b't' if self.b[self.i..].starts_with(b"true") => {
                self.i += 4;
                Ok(Json::Bool(true))
            }
            b'f' if self.b[self.i..].starts_with(b"false") => {
                self.i += 5;
                Ok(Json::Bool(false))
            }
            b'n' if self.b[self.i..].starts_with(b"null") => {
                self.i += 4;
                Ok(Json::Null)
            }
            _ => {
                let st = self.i;
                while self.i < self.b.len() && matches!(self.b[self.i], b'-' | b'+' | b'.' | b'e' | b'E' | b'0'..=b'9') {
                    self.i += 1;
                }
                let t = std::str::from_utf8(&self.b[st..self.i]).unwrap_or("");
                match t.parse::<f64>() {
                    Ok(n) if st != self.i => Ok(Json::Num(n)),
                    _ => self.err(&format!("unexpected character '{}'", c as char)),
                }
            }
        }
    }
}

pub fn parse_json(s: &str) -> Result<Json, String> {
    let mut p = Parser { b: s.as_bytes(), i: 0 };
    let v = p.value()?;
    p.ws();
    if p.i < p.b.len() {
        return p.err("trailing characters");
    }
    Ok(v)
}

// ------------------------------------------------------------------ JSON -> data

fn num(j: &Json, k: &str, d: f64) -> f64 {
    j.get(k).and_then(|v| v.as_f()).unwrap_or(d)
}
fn int(j: &Json, k: &str, d: i32) -> i32 {
    num(j, k, d as f64).round() as i32
}
fn boolean(j: &Json, k: &str, d: bool) -> bool {
    match j.get(k) {
        Some(Json::Bool(b)) => *b,
        _ => d,
    }
}
fn string(j: &Json, k: &str) -> String {
    j.get(k).and_then(|v| v.as_str()).unwrap_or("").to_string()
}

fn elem_of(j: &Json, ctx: &str) -> Result<u32, String> {
    match j {
        Json::Num(n) => Ok(*n as u32),
        Json::Str(s) => element_id(s).ok_or_else(|| format!("{ctx}: unknown element '{s}'")),
        _ => Err(format!("{ctx}: element must be a name or id")),
    }
}

fn mats_of(j: &Json, ctx: &str) -> Result<Vec<u32>, String> {
    match j.get("mats") {
        Some(Json::Arr(v)) => v.iter().map(|x| elem_of(x, ctx)).collect(),
        Some(x) => Ok(vec![elem_of(x, ctx)?]),
        None => match j.get("mat") {
            Some(x) => Ok(vec![elem_of(x, ctx)?]),
            None => Err(format!("{ctx}: needs 'mats'")),
        },
    }
}

/// `"tile"` (default: "matter" if mat >= 10, else "wall") + `"mat"` + `"dir"`.
fn cell_of(j: &Json, ctx: &str) -> Result<Cell, String> {
    let mat = match j.get("mat") {
        Some(m) => elem_of(m, ctx)?,
        None => 0,
    };
    let tile = match j.get("tile").and_then(|t| t.as_str()) {
        Some(t) => tile_id(t).ok_or_else(|| format!("{ctx}: unknown tile '{t}'"))?,
        None => {
            if j.get("mat").is_none() {
                return Err(format!("{ctx}: needs 'tile' or 'mat'"));
            }
            if mat >= 10 { T_MATTER } else { T_WALL }
        }
    };
    let dir = match j.get("dir") {
        Some(Json::Str(d)) => match d.as_str() {
            "right" | "+x" => 0,
            "down" | "+y" => 1,
            "left" | "-x" => 2,
            "up" | "-y" => 3,
            _ => return Err(format!("{ctx}: bad dir '{d}'")),
        },
        Some(Json::Num(n)) => *n as u32,
        _ => 0,
    };
    let mat = if tile == T_WALL || tile == T_MATTER { mat } else { 0 };
    Ok(Cell { tile, mat, dir })
}

fn gas_kind(s: &str) -> Option<u32> {
    Some(match s {
        "place" => 0,
        "erase" => 1,
        "fuel" => 2,
        "o2" | "oxygen" => 3,
        "air" => 4,
        "torch" | "flame" => 5,
        "chill" => 6,
        "vacuum" => 7,
        _ => return None,
    })
}

fn prim_of(j: &Json, ctx: &str) -> Result<Vec<Prim>, String> {
    let op = j.get("op").and_then(|v| v.as_str()).ok_or_else(|| format!("{ctx}: primitive needs 'op'"))?;
    let (x0, y0, x1, y1) = (int(j, "x0", 0), int(j, "y0", 0), int(j, "x1", int(j, "x0", 0)), int(j, "y1", int(j, "y0", 0)));
    let ctx = &format!("{ctx} ({op})");
    Ok(vec![match op {
        "floor" => Prim::Floor { cell: if j.get("mat").is_some() { cell_of(j, ctx)? } else { Cell { tile: T_WALL, mat: 0, dir: 0 } } },
        "rect" | "put" => {
            let (x0, y0) = (int(j, "x0", int(j, "x", 0)), int(j, "y0", int(j, "y", 0)));
            Prim::Rect { x0, y0, x1: int(j, "x1", x0), y1: int(j, "y1", y0), cell: cell_of(j, ctx)? }
        }
        "stripes" => Prim::Stripes { x0, y0, x1, y1, period: int(j, "period", 4), on: int(j, "on", 2), cell: cell_of(j, ctx)? },
        "clear" => Prim::Rect { x0, y0, x1, y1, cell: Cell { tile: 0, mat: 0, dir: 0 } },
        "hollow" => Prim::Hollow { x0, y0, x1, y1, t: int(j, "t", 2), open_top: boolean(j, "open_top", false), cell: cell_of(j, ctx)? },
        "disc" => Prim::Disc { x: int(j, "x", 0), y: int(j, "y", 0), r: int(j, "r", 1), cell: cell_of(j, ctx)? },
        "line" => Prim::Line { x0, y0, x1, y1, w: int(j, "w", 0), cell: cell_of(j, ctx)? },
        "mix" => Prim::Mix { x0, y0, x1, y1, mats: mats_of(j, ctx)? },
        "heap" => Prim::Heap { x: int(j, "x", 0), base: int(j, "base", H - 7), w: int(j, "w", 10), h: int(j, "h", 5), mats: mats_of(j, ctx)? },
        "scatter" => Prim::Scatter {
            x0,
            y0,
            x1,
            y1,
            density: num(j, "density", 0.05) as f32,
            seed: num(j, "seed", 1.0) as u32,
            mats: mats_of(j, ctx)?,
            only_empty: boolean(j, "only_empty", true),
        },
        "gas" => {
            let kind_s = j.get("kind").and_then(|v| v.as_str()).unwrap_or("fuel");
            let kind = gas_kind(kind_s).ok_or_else(|| format!("{ctx}: unknown gas kind '{kind_s}'"))?;
            let shape_s = j.get("shape").and_then(|v| v.as_str()).unwrap_or(if j.get("r").is_some() && j.get("x1").is_none() { "disc" } else { "box" });
            let shape = match shape_s {
                "disc" => 0,
                "box" | "rect" => 1,
                "hollow" => 2,
                "line" => 3,
                _ => return Err(format!("{ctx}: unknown gas shape '{shape_s}'")),
            };
            let (gx0, gy0) = if shape == 0 { (int(j, "x", x0), int(j, "y", y0)) } else { (x0, y0) };
            let (gx1, gy1) = if shape == 0 { (gx0, gy0) } else { (x1, y1) };
            let repeat = int(j, "repeat", 1).max(1);
            let o = OpData { kind, shape, x0: gx0, y0: gy0, x1: gx1, y1: gy1, r: int(j, "r", 0), tile: 0, mat: 0, dir: 0 };
            // repeated ops stack (e.g. three fuel puffs make a rich cloud)
            return Ok(vec![Prim::Gas(o); repeat as usize]);
        }
        _ => return Err(format!("{ctx}: unknown primitive")),
    }])
}

fn prims_of(j: Option<&Json>, ctx: &str) -> Result<Vec<Prim>, String> {
    let mut out = vec![];
    for (k, p) in j.map(|v| v.as_arr()).unwrap_or(&[]).iter().enumerate() {
        out.extend(prim_of(p, &format!("{ctx}[{k}]"))?);
    }
    Ok(out)
}

pub fn metric_of(s: &str) -> Result<Metric, String> {
    let (a, b) = s.split_once(':').unwrap_or((s, ""));
    let id = || element_id(b).ok_or_else(|| format!("metric '{s}': unknown element '{b}'"));
    Ok(match a {
        "thrust" => Metric::Thrust,
        "burn" => Metric::Burn,
        "isp" => Metric::Isp,
        "wall_heat" => Metric::WallHeat,
        "peak_temp" | "max_t" => Metric::PeakTemp,
        "peak_pressure" | "max_p" => Metric::PeakPressure,
        "struct_lost" | "walls_lost" => Metric::StructLost,
        "lost" => {
            let k = id()?;
            if k > 9 {
                return Err(format!("metric '{s}': 'lost' only works for wall materials 0-9"));
            }
            Metric::MatLost(k)
        }
        "used" => Metric::Used(id()?),
        "used_walls" => Metric::UsedWalls,
        "leak" => Metric::Leak,
        "impulse" => Metric::Impulse,
        "count" => Metric::Count(id()?),
        _ => return Err(format!("unknown metric '{s}'")),
    })
}

pub fn metric_key(m: Metric) -> String {
    match m {
        Metric::Thrust => "thrust".into(),
        Metric::Burn => "burn".into(),
        Metric::Isp => "isp".into(),
        Metric::WallHeat => "wall_heat".into(),
        Metric::PeakTemp => "peak_temp".into(),
        Metric::PeakPressure => "peak_pressure".into(),
        Metric::StructLost => "struct_lost".into(),
        Metric::MatLost(k) => format!("lost:{}", element_name(k)),
        Metric::Used(k) => format!("used:{}", element_name(k)),
        Metric::UsedWalls => "used_walls".into(),
        Metric::Leak => "leak".into(),
        Metric::Impulse => "impulse".into(),
        Metric::Count(k) => format!("count:{}", element_name(k)),
    }
}

fn goal_of(j: &Json, ctx: &str) -> Result<Goal, String> {
    let g = j.get("goal").and_then(|v| v.as_str()).ok_or_else(|| format!("{ctx}: needs 'goal'"))?;
    let m = metric_of(j.get("metric").and_then(|v| v.as_str()).ok_or_else(|| format!("{ctx}: needs 'metric'"))?).map_err(|e| format!("{ctx}: {e}"))?;
    let v = j.get("value").and_then(|v| v.as_f()).ok_or_else(|| format!("{ctx}: needs 'value'"))? as f32;
    let s = num(j, "secs", 0.0) as f32;
    Ok(match g {
        "reach" => Goal::Reach(m, v),
        "reach_by" => Goal::ReachBy(m, v, s),
        "hold" => Goal::Hold(m, v, s),
        "keep" => Goal::Keep(m, v),
        "at_most" => Goal::AtMost(m, v),
        "hold_below" => Goal::HoldBelow(m, v, s),
        _ => return Err(format!("{ctx}: unknown goal '{g}'")),
    })
}

pub fn palette_item(s: &str) -> Result<PaletteItem, String> {
    let (a, b) = s.split_once(':').unwrap_or((s, ""));
    Ok(match a {
        "erase" => PaletteItem::Erase,
        "wall" => PaletteItem::Wall(element_id(b).filter(|&k| k < 10).ok_or_else(|| format!("palette '{s}': not a wall material"))?),
        "matter" => PaletteItem::Matter(element_id(b).filter(|&k| k >= 10).ok_or_else(|| format!("palette '{s}': not a matter element"))?),
        "part" => PaletteItem::Part(match tile_id(b) {
            Some(t) if (2..=7).contains(&t) => t,
            _ => return Err(format!("palette '{s}': not a machine")),
        }),
        "gas" | "tool" => PaletteItem::Gas(match gas_kind(b) {
            Some(k) if k >= 2 => k,
            _ => return Err(format!("palette '{s}': not a gas tool")),
        }),
        _ => return Err(format!("palette '{s}': use wall:/matter:/part:/gas:/erase")),
    })
}

pub fn load_from_str(src: &str) -> Result<Campaign, String> {
    let root = parse_json(src)?;
    let mut chapters = vec![];
    for c in root.get("chapters").map(|v| v.as_arr()).unwrap_or(&[]) {
        chapters.push(ChapterDef { id: int(c, "id", 0) as u32, title: string(c, "title"), blurb: string(c, "blurb") });
    }
    let defaults = root.get("defaults").cloned().unwrap_or(Json::Obj(vec![]));
    let mut missions = vec![];
    for (k, mj) in root.get("missions").map(|v| v.as_arr()).unwrap_or(&[]).iter().enumerate() {
        let id = string(mj, "id");
        let ctx = format!("mission {} '{}'", k + 1, id);
        if id.is_empty() {
            return Err(format!("{ctx}: needs an 'id'"));
        }
        let pick = |key: &str| mj.get(key).or_else(|| defaults.get(key));
        let f = |key: &str, d: f64| pick(key).and_then(|v| v.as_f()).unwrap_or(d);
        let b = |key: &str, d: bool| match pick(key) {
            Some(Json::Bool(x)) => *x,
            _ => d,
        };
        let feeds = match mj.get("feeds") {
            Some(fj) => Feeds {
                fuel: num(fj, "fuel", 1.0) as f32,
                o2: num(fj, "o2", 1.0) as f32,
                air: num(fj, "air", 1.0) as f32,
                fan: num(fj, "fan", 0.0) as f32,
                adjustable: boolean(fj, "adjustable", false),
            },
            None => Feeds::default(),
        };
        let mut palette = vec![];
        for p in mj.get("palette").map(|v| v.as_arr()).unwrap_or(&[]) {
            palette.push(palette_item(p.as_str().unwrap_or("")).map_err(|e| format!("{ctx}: {e}"))?);
        }
        let mut budgets = vec![];
        for bj in mj.get("budgets").map(|v| v.as_arr()).unwrap_or(&[]) {
            let m = metric_of(bj.get("metric").and_then(|v| v.as_str()).unwrap_or("")).map_err(|e| format!("{ctx}: budget: {e}"))?;
            budgets.push((m, num(bj, "max", 0.0) as f32));
        }
        let mut objectives = vec![];
        for (n, oj) in mj.get("objectives").map(|v| v.as_arr()).unwrap_or(&[]).iter().enumerate() {
            let octx = format!("{ctx} objective {}", n + 1);
            objectives.push(ObjectiveDef {
                text: string(oj, "text"),
                goal: goal_of(oj, &octx)?,
                required: match oj.get("kind").and_then(|v| v.as_str()) {
                    Some("required") | None => boolean(oj, "required", true),
                    Some("star") | Some("bonus") => false,
                    Some(x) => return Err(format!("{octx}: kind must be 'required' or 'star', got '{x}'")),
                },
            });
        }
        if !objectives.iter().any(|o| o.required) {
            return Err(format!("{ctx}: needs at least one required objective"));
        }
        let mut stars: Vec<String> = mj.get("stars").map(|v| v.as_arr()).unwrap_or(&[]).iter().filter_map(|s| s.as_str().map(String::from)).collect();
        if stars.is_empty() {
            stars.push("Complete the mission".into());
            for o in objectives.iter().filter(|o| !o.required) {
                stars.push(o.label());
            }
        }
        let requires = match mj.get("requires") {
            Some(Json::Arr(v)) => v.iter().filter_map(|x| x.as_str().map(String::from)).collect(),
            Some(Json::Str(s)) => vec![s.clone()],
            _ => vec![],
        };
        let igniter_secs = match pick("igniter_secs") {
            Some(Json::Num(n)) => *n as f32,
            _ => 1e9,
        };
        let hints = mj.get("hints").map(|v| v.as_arr()).unwrap_or(&[]).iter().filter_map(|s| s.as_str().map(String::from)).collect();
        missions.push(MissionDef {
            order: k as u32 + 1,
            chapter: int(mj, "chapter", 1) as u32,
            title: string(mj, "title"),
            tagline: string(mj, "tagline"),
            briefing: string(mj, "briefing"),
            hints,
            requires,
            live: b("live", false),
            locked_start: b("locked_start", true),
            gravity: b("gravity", true),
            view: f("view", 0.0) as u32,
            duration: f("duration", 30.0) as f32,
            igniter_secs,
            feeds,
            palette,
            budgets,
            objectives,
            stars,
            layout: prims_of(mj.get("layout"), &format!("{ctx} layout"))?,
            solution: prims_of(mj.get("solution"), &format!("{ctx} solution"))?,
            solve_at: f("solve_at", 0.0) as f32,
            fact: string(mj, "fact"),
            id,
        });
    }
    // sanity: unique ids, requires point at earlier missions
    for (k, m) in missions.iter().enumerate() {
        if missions[..k].iter().any(|o| o.id == m.id) {
            return Err(format!("duplicate mission id '{}'", m.id));
        }
        for r in &m.requires {
            if !missions[..k].iter().any(|o| &o.id == r) {
                return Err(format!("mission '{}' requires '{}', which is not an earlier mission", m.id, r));
            }
        }
    }
    Ok(Campaign { chapters, missions })
}
