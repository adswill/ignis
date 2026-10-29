//! Compositions, composition brush, Cast / press tool and the Propellant Designer.
//!
//! * `COMPOSITIONS`: preset blends with real mass proportions (left dock "Compositions" group).
//! * `Brush`: the active composition brush (element ids + cell fractions, optional cast flag). Painting with it
//!   sends op kind 0 / tile 8 with the mixture encoded as documented in ARCHITECTURE.md ("UI: composition brush"):
//!   `mat` = base element, `_p[0]` = second element id, `_p[1]` = cell fraction of the second element x1000.
//!   Further components (and, when the solver has no native mixture support, every non-base component) are placed
//!   first as single-cell ops at hash-selected cells; the main op then fills the remaining empty cells.
//! * Cast / press: op kind `OP_CAST` (9) over the brush shape; sent after the matter ops when the brush is cast.
//! * Propellant Designer: oxidiser / fuel / metal / catalyst / binder percentages -> oxygen-balance estimate of
//!   flame temperature, burn-rate law r = a p^n, ideal Isp, density and safety notes; paints a cast grain or builds
//!   a test motor.
use super::theme::*;
use super::ws::{kv, sheet, swatch};
use super::*;
use egui::RichText;

pub const OP_STIR: u32 = 8;
pub const OP_CAST: u32 = 9;

/// The solver decodes the 2-component mixture brush (`o._p0` / `o._p1` used in a shader).
pub fn native_mixture() -> bool {
    const SIM: &str = include_str!("../sim.wgsl");
    const MAT: &str = include_str!("../matter.wgsl");
    SIM.contains("o._p0") || MAT.contains("o._p0") || SIM.contains("op._p0") || MAT.contains("op._p0")
}

/// The solver implements the Cast / press op (kind 9).
pub fn native_cast() -> bool {
    const SIM: &str = include_str!("../sim.wgsl");
    const MAT: &str = include_str!("../matter.wgsl");
    let k = |s: &str| s.contains("kind == 9u") || s.contains("OP_CAST") || s.contains("KIND_CAST");
    k(SIM) || k(MAT)
}

/// Matter element id by (case-insensitive) name; first candidate that exists wins.
pub fn elem_id(names: &[&str]) -> Option<u32> {
    for n in names {
        let n = n.to_lowercase();
        if let Some(e) = elements::MATTER.iter().find(|e| e.name.to_lowercase() == n) {
            if let Tool::Matter(id) = e.tool {
                return Some(id);
            }
        }
    }
    // elements added later (other catalogue lists)
    for n in names {
        let n = n.to_lowercase();
        if let Some(e) = elements::all().find(|e| matches!(e.tool, Tool::Matter(_)) && e.name.to_lowercase() == n) {
            if let Tool::Matter(id) = e.tool {
                return Some(id);
            }
        }
    }
    None
}

fn elem_color(id: u32) -> Color32 {
    elements::find(Tool::Matter(id)).map(|e| e.color).unwrap_or(Color32::GRAY)
}

/// Density relative to water (g/cm³).
fn elem_density(id: u32, fallback: f32) -> f32 {
    elements::matter_info(id).map(|i| i.density).filter(|d| *d > 0.05).unwrap_or(fallback)
}

// =====================================================================================================
// presets
// =====================================================================================================

pub struct Composition {
    pub name: &'static str,
    /// (candidate element names, mass %)
    pub parts: &'static [(&'static [&'static str], f32)],
    /// a propellant that is cast / pressed into a grain (rather than used loose)
    pub cast: bool,
    pub note: &'static str,
    /// designer preset to open (index into DESIGNER_PRESETS) for propellants
    pub designer: Option<usize>,
}

const KNO3: &[&str] = &["Saltpeter", "Potassium nitrate"];
const SUGAR: &[&str] = &["Sugar", "Sucrose"];
const CHAR: &[&str] = &["Charcoal"];
const SULF: &[&str] = &["Sulfur"];
const AP: &[&str] = &["Ammonium perchlorate"];
const AL: &[&str] = &["Aluminium powder", "Aluminum powder"];
const BINDER: &[&str] = &["HTPB", "HTPB binder", "Epoxy", "Plastic"];
const RUST: &[&str] = &["Rust", "Iron oxide"];
const MG: &[&str] = &["Magnesium", "Magnesium powder"];
const CUO: &[&str] = &["Copper oxide"];
const KCLO4: &[&str] = &["Potassium perchlorate"];
const AN: &[&str] = &["Ammonium nitrate"];
const DIESEL: &[&str] = &["Diesel", "Kerosene"];
const FE: &[&str] = &["Iron filings"];
const KMNO4: &[&str] = &["Potassium permanganate"];
const GLYC: &[&str] = &["Glycerin"];
const H2O2: &[&str] = &["Hydrogen peroxide"];
const MNO2: &[&str] = &["Manganese dioxide"];
const SORB: &[&str] = &["Sorbitol"];

pub const COMPOSITIONS: &[Composition] = &[
    Composition { name: "Black powder", parts: &[(KNO3, 75.0), (CHAR, 15.0), (SULF, 10.0)], cast: false, note: "Classic 75:15:10. Loose it deflagrates through its pores (whoosh, or a bang when confined); pressed it burns layer by layer.", designer: Some(2) },
    Composition { name: "Rocket candy (KNSU)", parts: &[(KNO3, 65.0), (SUGAR, 35.0)], cast: true, note: "65:35 saltpeter / sucrose, melt-cast. r = 8.3 p^0.32 mm/s (p in MPa), about 1700 K, smoky K2CO3 exhaust.", designer: Some(0) },
    Composition { name: "KNSB (sorbitol)", parts: &[(KNO3, 65.0), (SORB, 35.0)], cast: true, note: "Sorbitol variant of rocket candy: easier to cast, slower at low pressure.", designer: Some(1) },
    Composition { name: "APCP", parts: &[(AP, 70.0), (AL, 16.0), (BINDER, 14.0)], cast: true, note: "Ammonium perchlorate composite: 70 % AP, 16 % Al, 14 % HTPB binder. About 3300 K, r = 3.6 p^0.35.", designer: Some(3) },
    Composition { name: "Thermite", parts: &[(RUST, 75.0), (AL, 25.0)], cast: false, note: "Fe2O3 + 2 Al, stoichiometric 75:25. Needs ~1500 K to start (use magnesium); makes molten iron.", designer: None },
    Composition { name: "Magnesium thermite", parts: &[(RUST, 69.0), (MG, 31.0)], cast: false, note: "Fe2O3 + 3 Mg (69:31). Lights more easily than aluminium thermite.", designer: None },
    Composition { name: "Copper thermite", parts: &[(CUO, 81.5), (AL, 18.5)], cast: false, note: "3 CuO + 2 Al (81.5:18.5). Very fast, almost explosive; sprays molten copper.", designer: None },
    Composition { name: "Flash powder", parts: &[(KCLO4, 70.0), (AL, 30.0)], cast: false, note: "70:30 perchlorate / aluminium. Explodes even unconfined. Not a propellant.", designer: None },
    Composition { name: "ANFO", parts: &[(AN, 94.0), (DIESEL, 6.0)], cast: false, note: "94:6 ammonium nitrate / fuel oil. Only smoulders in a flame; needs a booster charge to detonate.", designer: None },
    Composition { name: "Sparkler mix", parts: &[(KNO3, 60.0), (FE, 40.0)], cast: false, note: "Oxidiser with coarse iron: the iron burns as orange sparks.", designer: None },
    Composition { name: "Permanganate + glycerin", parts: &[(KMNO4, 75.0), (GLYC, 25.0)], cast: false, note: "Hypergolic pair: smokes for a few seconds, then bursts into a purple-tinged flame.", designer: None },
    Composition { name: "Catalysed peroxide", parts: &[(H2O2, 95.0), (MNO2, 5.0)], cast: false, note: "90 % peroxide on a MnO2 catalyst: decomposes into ~1000 K steam and oxygen.", designer: None },
];

/// Resolve a preset into (element id, mass fraction, density); None if an ingredient is missing.
fn resolve(c: &Composition) -> Option<Vec<(u32, f32)>> {
    let mut v = vec![];
    for (names, pct) in c.parts {
        let id = elem_id(names)?;
        v.push((id, *pct / 100.0));
    }
    Some(v)
}

// =====================================================================================================
// brush
// =====================================================================================================

#[derive(Clone, Debug)]
pub struct Brush {
    pub name: String,
    /// (element id, cell fraction), sorted by fraction, largest first; sums to 1
    pub parts: Vec<(u32, f32)>,
    /// mass fractions for display
    pub mass: Vec<(u32, f32)>,
    pub cast: bool,
}

impl Brush {
    /// From mass fractions: cell fraction ∝ mass fraction / density (cells have a fixed volume).
    pub fn from_mass(name: &str, mass: &[(u32, f32)], cast: bool) -> Option<Brush> {
        let mut agg: Vec<(u32, f32)> = vec![];
        for (id, w) in mass.iter().filter(|p| p.1 > 1e-4) {
            match agg.iter_mut().find(|a| a.0 == *id) {
                Some(a) => a.1 += *w,
                None => agg.push((*id, *w)),
            }
        }
        if agg.is_empty() {
            return None;
        }
        let msum: f32 = agg.iter().map(|p| p.1).sum();
        let mass: Vec<(u32, f32)> = agg.iter().map(|(i, w)| (*i, *w / msum)).collect();
        let mut parts: Vec<(u32, f32)> = mass.iter().map(|(id, w)| (*id, *w / elem_density(*id, 1.5))).collect();
        let s: f32 = parts.iter().map(|p| p.1).sum();
        for p in parts.iter_mut() {
            p.1 /= s;
        }
        parts.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        Some(Brush { name: name.to_string(), parts, mass, cast })
    }

    pub fn base(&self) -> u32 {
        self.parts[0].0
    }
}

fn hash01(x: i32, y: i32, salt: u32) -> f32 {
    let mut h = (x as u32).wrapping_mul(0x8da6_b343) ^ (y as u32).wrapping_mul(0xd816_3841) ^ salt.wrapping_mul(0xcb1a_b31f);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2c1b_3c6d);
    h ^= h >> 12;
    h = h.wrapping_mul(0x297a_2d39);
    h ^= h >> 15;
    (h >> 8) as f32 / (1u32 << 24) as f32
}

/// Cells covered by an op shape (mirrors `in_shape` in sim.wgsl), clipped to the grid.
fn shape_cells(o: &Op) -> Vec<(i32, i32)> {
    let (w, h) = (W as i32, H as i32);
    let mut v = vec![];
    let (lx, hx, ly, hy) = match o.shape {
        0 => (o.x0 - o.r, o.x0 + o.r, o.y0 - o.r, o.y0 + o.r),
        1 | 2 => (o.x0.min(o.x1), o.x0.max(o.x1), o.y0.min(o.y1), o.y0.max(o.y1)),
        _ => (o.x0.min(o.x1) - o.r - 1, o.x0.max(o.x1) + o.r + 1, o.y0.min(o.y1) - o.r - 1, o.y0.max(o.y1) + o.r + 1),
    };
    for y in ly.max(0)..=hy.min(h - 1) {
        for x in lx.max(0)..=hx.min(w - 1) {
            let inside = match o.shape {
                0 => {
                    let (dx, dy) = (x - o.x0, y - o.y0);
                    dx * dx + dy * dy <= o.r * o.r + o.r / 2
                }
                1 => true,
                2 => {
                    let t = o.r + 1;
                    let (l, r, b, u) = (o.x0.min(o.x1), o.x0.max(o.x1), o.y0.min(o.y1), o.y0.max(o.y1));
                    x < l + t || x > r - t || y < b + t || y > u - t
                }
                _ => {
                    let (ax, ay, bx, by) = (o.x0 as f32, o.y0 as f32, o.x1 as f32, o.y1 as f32);
                    let (abx, aby) = (bx - ax, by - ay);
                    let t = (((x as f32 - ax) * abx + (y as f32 - ay) * aby) / (abx * abx + aby * aby).max(1e-6)).clamp(0.0, 1.0);
                    let (px, py) = (ax + abx * t - x as f32, ay + aby * t - y as f32);
                    (px * px + py * py).sqrt() <= o.r as f32 + 0.5
                }
            };
            if inside {
                v.push((x, y));
            }
        }
    }
    v
}

/// Expand matter-placing ops (kind 0, tile 8) into a composition: minor components as single-cell ops at
/// hash-chosen cells first, then the main op (native 2-component mixture when the solver supports it).
pub fn expand(b: &Brush, ops: &[Op]) -> Vec<Op> {
    let native = native_mixture();
    let n_main = if native { 2.min(b.parts.len()) } else { 1 };
    let mut out = vec![];
    for o in ops {
        if !(o.kind == 0 && o.tile == scene::MATTER) {
            out.push(*o);
            continue;
        }
        // minor components occupy [0, minor) of the per-cell hash
        let minor: f32 = b.parts[n_main..].iter().map(|p| p.1).sum();
        let single = o.shape == 0 && o.r == 0;
        if single {
            // spray grain: choose the component directly
            let u = hash01(o.x0, o.y0, 0x51ed);
            let mut acc = 0.0;
            let mut id = b.base();
            for p in b.parts.iter().rev() {
                acc += p.1;
                if u < acc {
                    id = p.0;
                    break;
                }
            }
            out.push(Op { mat: id, ..*o });
            continue;
        }
        if minor > 1e-4 {
            for (x, y) in shape_cells(o) {
                let u = hash01(x, y, 0x51ed);
                if u >= minor {
                    continue;
                }
                let mut acc = 0.0;
                for p in &b.parts[n_main..] {
                    acc += p.1;
                    if u < acc {
                        out.push(Op { kind: 0, shape: 0, x0: x, y0: y, x1: x, y1: y, r: 0, tile: scene::MATTER, mat: p.0, dir: 0, _p: [0; 2] });
                        break;
                    }
                }
            }
        }
        let mut m = Op { mat: b.base(), ..*o };
        if n_main == 2 {
            let (a, c) = (b.parts[0].1, b.parts[1].1);
            m._p = [b.parts[1].0, ((c / (a + c)) * 1000.0).round() as u32];
        }
        out.push(m);
    }
    if b.cast {
        let cast: Vec<Op> = ops.iter().filter(|o| o.kind == 0 && o.tile == scene::MATTER).map(|o| Op { kind: OP_CAST, tile: 0, mat: 0, _p: [0; 2], ..*o }).collect();
        out.extend(cast);
    }
    out
}

// =====================================================================================================
// designer model
// =====================================================================================================

#[derive(Clone, Copy, PartialEq)]
pub enum Role {
    Oxidiser,
    Fuel,
    Metal,
    Catalyst,
    Binder,
}

pub struct Ingr {
    pub label: &'static str,
    pub names: &'static [&'static str],
    pub role: Role,
    /// g O2 released per g (oxidisers)
    pub o_supply: f32,
    /// g O2 needed to burn 1 g (fuels, metals, binders)
    pub o_demand: f32,
    /// oxidiser: flame-temperature rise at the optimum (K); metal: K per unit mass fraction
    pub dt: f32,
    /// oxidiser: burn-rate coefficient a (mm/s at 1 MPa); others: rate multiplier (metal: per unit fraction slope)
    pub a: f32,
    /// oxidiser: pressure exponent n; others: shift of n
    pub n: f32,
    /// oxidiser: mean molar mass of the products (g/mol)
    pub m: f32,
    /// density fallback (g/cm³) if the element has no data sheet
    pub rho: f32,
}

const fn ig(label: &'static str, names: &'static [&'static str], role: Role, o_supply: f32, o_demand: f32, dt: f32, a: f32, n: f32, m: f32, rho: f32) -> Ingr {
    Ingr { label, names, role, o_supply, o_demand, dt, a, n, m, rho }
}

/// Published reference data: Nakka (KNSU/KNSB burn rates), Sutton & Biblarz (AP composites), oxygen balances
/// from the decomposition stoichiometry. The figures are estimates for the simulator, not formulation advice.
pub const INGREDIENTS: &[Ingr] = &[
    // oxidisers
    ig("Potassium nitrate", KNO3, Role::Oxidiser, 0.396, 0.0, 1450.0, 8.26, 0.319, 42.0, 2.11),
    ig("Ammonium perchlorate", AP, Role::Oxidiser, 0.340, 0.0, 2850.0, 3.60, 0.350, 25.0, 1.95),
    ig("Potassium perchlorate", KCLO4, Role::Oxidiser, 0.462, 0.0, 2500.0, 6.00, 0.400, 36.0, 2.52),
    ig("Ammonium nitrate", AN, Role::Oxidiser, 0.200, 0.0, 1350.0, 1.60, 0.550, 22.0, 1.72),
    ig("Sodium nitrate", &["Sodium nitrate"], Role::Oxidiser, 0.470, 0.0, 1400.0, 5.00, 0.350, 40.0, 2.26),
    ig("Potassium chlorate", &["Potassium chlorate"], Role::Oxidiser, 0.392, 0.0, 2200.0, 12.0, 0.400, 38.0, 2.32),
    // fuels
    ig("Sucrose", SUGAR, Role::Fuel, 0.0, 1.123, 0.0, 1.00, 0.00, 0.0, 1.59),
    ig("Sorbitol", SORB, Role::Fuel, 0.0, 1.143, 0.0, 1.00, 0.15, 0.0, 1.49),
    ig("Dextrose", &["Dextrose", "Glucose"], Role::Fuel, 0.0, 1.066, 0.0, 0.95, 0.02, 0.0, 1.54),
    ig("Charcoal", CHAR, Role::Fuel, 0.0, 2.400, 0.0, 1.35, -0.02, 0.0, 1.30),
    ig("Sulfur", SULF, Role::Fuel, 0.0, 1.000, 0.0, 1.15, 0.00, 0.0, 2.07),
    ig("Coal", &["Coal"], Role::Fuel, 0.0, 2.500, 0.0, 0.80, 0.00, 0.0, 1.35),
    ig("Flour", &["Flour"], Role::Fuel, 0.0, 1.200, 0.0, 0.80, 0.00, 0.0, 1.50),
    // metals
    ig("Aluminium", AL, Role::Metal, 0.0, 0.889, 3500.0, 1.00, 0.00, 0.0, 2.70),
    ig("Magnesium", MG, Role::Metal, 0.0, 0.658, 2800.0, 2.00, 0.00, 0.0, 1.74),
    ig("Titanium", &["Titanium powder"], Role::Metal, 0.0, 0.668, 2000.0, 0.50, 0.00, 0.0, 4.51),
    ig("Iron filings", FE, Role::Metal, 0.0, 0.430, 500.0, 0.20, 0.00, 0.0, 7.87),
    ig("Zinc", &["Zinc", "Zinc powder"], Role::Metal, 0.0, 0.245, 600.0, 0.30, 0.00, 0.0, 7.14),
    // catalysts / burn-rate modifiers (a = saturated multiplier at >= 1 %)
    ig("Iron oxide", RUST, Role::Catalyst, 0.0, 0.0, 0.0, 1.70, -0.03, 0.0, 5.24),
    ig("Copper chromite", &["Copper chromite"], Role::Catalyst, 0.0, 0.0, 0.0, 1.90, -0.04, 0.0, 5.40),
    ig("Copper oxide", CUO, Role::Catalyst, 0.0, 0.0, 0.0, 1.40, -0.02, 0.0, 6.31),
    ig("Manganese dioxide", MNO2, Role::Catalyst, 0.0, 0.0, 0.0, 1.30, 0.00, 0.0, 5.03),
    // binders
    ig("HTPB", &["HTPB", "HTPB binder"], Role::Binder, 0.0, 3.250, 0.0, 1.00, 0.00, 0.0, 0.93),
    ig("Epoxy", &["Epoxy"], Role::Binder, 0.0, 2.600, 0.0, 1.00, 0.02, 0.0, 1.20),
    ig("Paraffin wax", &["Wax", "Paraffin"], Role::Binder, 0.0, 3.420, 0.0, 1.00, 0.00, 0.0, 0.90),
    ig("Polyethylene", &["Plastic", "Polyethylene"], Role::Binder, 0.0, 3.420, 0.0, 1.00, 0.00, 0.0, 0.95),
];

fn ingr_of(role: Role) -> Vec<(usize, &'static Ingr, Option<u32>)> {
    INGREDIENTS.iter().enumerate().filter(|(_, i)| i.role == role).map(|(k, i)| (k, i, elem_id(i.names))).filter(|(_, _, id)| id.is_some()).collect()
}

fn ingr_index(label: &str) -> usize {
    INGREDIENTS.iter().position(|i| i.label == label).unwrap_or(0)
}

#[derive(Clone)]
pub struct Designer {
    /// ingredient index per slot (oxidiser, fuel, metal, catalyst, binder); metal..binder may be None
    pub sel: [Option<usize>; 5],
    /// mass % per slot (not necessarily normalised)
    pub pct: [f32; 5],
    pub cast: bool,
    pub preset: usize,
    /// test-motor geometry: 0 end-burner, 1 core-burner (BATES-like)
    pub grain: usize,
    pub throat: i32,
}

/// (name, [ox, fuel, metal, catalyst, binder] labels, pct)
pub const DESIGNER_PRESETS: &[(&str, [&str; 5], [f32; 5])] = &[
    ("KNSU rocket candy 65/35", ["Potassium nitrate", "Sucrose", "", "", ""], [65.0, 35.0, 0.0, 0.0, 0.0]),
    ("KNSB 65/35", ["Potassium nitrate", "Sorbitol", "", "", ""], [65.0, 35.0, 0.0, 0.0, 0.0]),
    ("Pressed black powder 75/15/10", ["Potassium nitrate", "Charcoal", "", "", ""], [75.0, 15.0, 0.0, 0.0, 0.0]),
    ("APCP 70/16/14 (HTPB)", ["Ammonium perchlorate", "", "Aluminium", "", "HTPB"], [70.0, 0.0, 16.0, 0.0, 14.0]),
    ("APCP + 1 % iron oxide", ["Ammonium perchlorate", "", "Aluminium", "Iron oxide", "HTPB"], [69.0, 0.0, 16.0, 1.0, 14.0]),
    ("KNSU + 1 % iron oxide", ["Potassium nitrate", "Sucrose", "", "Iron oxide", ""], [64.5, 34.5, 0.0, 1.0, 0.0]),
    ("AN / Mg / HTPB (slow gas generator)", ["Ammonium nitrate", "", "Magnesium", "", "HTPB"], [75.0, 0.0, 10.0, 0.0, 15.0]),
];

impl Default for Designer {
    fn default() -> Self {
        let mut d = Designer { sel: [None; 5], pct: [0.0; 5], cast: true, preset: 0, grain: 0, throat: 6 };
        d.load_preset(0);
        d
    }
}

pub struct Prediction {
    pub mass: Vec<(u32, f32, &'static str)>,
    pub phi: f32,
    pub ob: f32,
    pub t_flame: f32,
    pub a: f32,
    pub n: f32,
    pub m: f32,
    pub isp: f32,
    pub rho: f32,
    pub notes: Vec<(Level, String)>,
    pub missing: Vec<&'static str>,
}

#[derive(Clone, Copy, PartialEq)]
pub enum Level {
    Info,
    Caution,
    Danger,
}

impl Designer {
    pub fn load_preset(&mut self, k: usize) {
        let (_, labels, pct) = DESIGNER_PRESETS[k.min(DESIGNER_PRESETS.len() - 1)];
        self.preset = k;
        // when a binder named in the preset is missing, fall back to any available binder
        for s in 0..5 {
            self.sel[s] = if labels[s].is_empty() { None } else { Some(ingr_index(labels[s])) };
            if let Some(i) = self.sel[s] {
                if elem_id(INGREDIENTS[i].names).is_none() {
                    let role = INGREDIENTS[i].role;
                    self.sel[s] = ingr_of(role).first().map(|x| x.0);
                }
            }
        }
        self.pct = pct;
    }

    pub fn predict(&self) -> Prediction {
        let mut notes = vec![];
        let mut missing = vec![];
        let tot: f32 = (0..5).filter(|s| self.sel[*s].is_some()).map(|s| self.pct[s].max(0.0)).sum::<f32>().max(1e-3);
        let w = |s: usize| if self.sel[s].is_some() { self.pct[s].max(0.0) / tot } else { 0.0 };
        let ing = |s: usize| self.sel[s].map(|i| &INGREDIENTS[i]);
        let mut mass = vec![];
        for s in 0..5 {
            if let Some(i) = ing(s) {
                if w(s) > 1e-4 {
                    match elem_id(i.names) {
                        Some(id) => mass.push((id, w(s), i.label)),
                        None => missing.push(i.label),
                    }
                }
            }
        }
        let ox = ing(0).unwrap_or(&INGREDIENTS[0]);
        let (wo, wf, wm, wc, wb) = (w(0), w(1), w(2), w(3), w(4));
        let supply = wo * ox.o_supply;
        let demand = wf * ing(1).map(|i| i.o_demand).unwrap_or(0.0) + wb * ing(4).map(|i| i.o_demand).unwrap_or(0.0) + 0.5 * wm * ing(2).map(|i| i.o_demand).unwrap_or(0.0);
        let full_demand = demand + 0.5 * wm * ing(2).map(|i| i.o_demand).unwrap_or(0.0);
        let phi = demand / supply.max(1e-4);
        let ob = (supply - full_demand) * 100.0;
        // combustion efficiency vs. equivalence: propellants run fuel-rich, optimum phi ~ 1.8 (CO/H2 products)
        let g = if supply < 1e-4 || demand < 1e-4 { 0.0 } else { (-((phi / 1.8).ln()).powi(2) / (2.0 * 0.5 * 0.5)).exp() };
        let metal_dt = ing(2).map(|i| i.dt).unwrap_or(0.0) * wm.min(0.25) * g.sqrt();
        let inert = (wc * 2.5).min(0.5);
        let t_flame = (300.0 + (ox.dt * g + metal_dt) * (1.0 - inert)).min(3750.0);
        // burn-rate law
        let fuel_mul = ing(1).map(|i| i.a).unwrap_or(1.0);
        let metal_mul = 1.0 + ing(2).map(|i| i.a).unwrap_or(0.0) * wm;
        let cat_mul = ing(3).map(|i| 1.0 + (i.a - 1.0) * (wc / 0.01).min(1.0)).unwrap_or(1.0);
        let nominal_binder = if ox.label == "Ammonium perchlorate" || ox.label == "Ammonium nitrate" { 0.13 } else { 0.0 };
        let binder_mul = (-(wb - nominal_binder) * if wb > nominal_binder { 4.0 } else { 1.5 }).exp();
        let a = ox.a * fuel_mul * metal_mul * cat_mul * binder_mul * g.max(0.05).sqrt();
        let n = (ox.n + ing(1).map(|i| i.n * (wf / 0.3).min(1.0)).unwrap_or(0.0) + ing(3).map(|i| i.n * (wc / 0.01).min(1.0)).unwrap_or(0.0)).clamp(0.1, 0.95);
        let m = ox.m + 25.0 * wm;
        let isp = if t_flame > 400.0 { 23.0 * (t_flame / m).sqrt() } else { 0.0 };
        let mut inv = 0.0;
        for s in 0..5 {
            if let Some(i) = ing(s) {
                let d = elem_id(i.names).map(|id| elem_density(id, i.rho)).unwrap_or(i.rho);
                inv += w(s) / d;
            }
        }
        let rho = if inv > 0.0 { 1.0 / inv } else { 0.0 };

        // safety / quality notes
        use Level::*;
        if ox.label == "Potassium chlorate" {
            notes.push((Danger, "Chlorate mixtures are friction- and shock-sensitive; never combine with sulfur or ammonium salts.".into()));
        }
        if (ox.label == "Potassium perchlorate" || ox.label == "Potassium chlorate") && wm >= 0.2 && wb < 0.05 {
            notes.push((Danger, "This is a flash composition: it explodes unconfined. It is not a propellant.".into()));
        }
        if !self.cast {
            notes.push((Caution, "Loose powder: flame penetrates the pores and the whole mass reacts at once (flash or explosion). Cast or press it for a steady burn.".into()));
        }
        if n > 0.7 {
            notes.push((Danger, format!("Pressure exponent n = {n:.2} > 0.7: chamber pressure is unstable and can run away (CATO).")));
        } else if n > 0.55 {
            notes.push((Caution, format!("High pressure exponent n = {n:.2}: thrust is sensitive to throat erosion and grain defects.")));
        }
        if self.cast && nominal_binder > 0.0 && wb < 0.08 {
            notes.push((Caution, "Too little binder for a composite: brittle, porous grain. Cracks add burning surface and over-pressurise the case.".into()));
        }
        if wb > 0.25 {
            notes.push((Caution, "Binder-rich: hard to ignite, low flame temperature, smoky exhaust.".into()));
        }
        if supply > 1e-4 && phi < 1.0 {
            notes.push((Caution, "Oxidiser-rich: hot oxidising exhaust attacks the nozzle and lowers performance.".into()));
        }
        if phi > 3.5 {
            notes.push((Caution, "Very fuel-rich: incomplete combustion and sooty exhaust; may not sustain burning.".into()));
        }
        if wm > 0.25 {
            notes.push((Caution, "Metal loading above 25 %: hard ignition, slag in the chamber, two-phase flow losses.".into()));
        }
        if wc > 0.03 {
            notes.push((Info, "Catalyst above 2-3 % adds little and dilutes the propellant.".into()));
        }
        if ox.label == "Ammonium nitrate" {
            notes.push((Info, "Ammonium nitrate is hygroscopic and slow-burning; its 32 °C phase change cracks grains (use phase-stabilised AN).".into()));
        }
        if ox.label == "Potassium nitrate" && (ing(1).map(|i| i.label == "Sucrose" || i.label == "Sorbitol" || i.label == "Dextrose").unwrap_or(false)) {
            notes.push((Info, "Sugar propellants are melt-cast near 185 °C; sucrose caramelises above ~200 °C.".into()));
        }
        if ox.label == "Ammonium perchlorate" && ing(2).map(|i| i.label == "Magnesium").unwrap_or(false) {
            notes.push((Caution, "Magnesium with ammonium perchlorate degrades in humid storage (forms magnesium perchlorate).".into()));
        }
        if t_flame < 1200.0 {
            notes.push((Caution, "Predicted flame temperature is low: the composition may not sustain combustion.".into()));
        }
        Prediction { mass, phi, ob, t_flame, a, n, m, isp, rho, notes, missing }
    }
}

// =====================================================================================================
// mixture sampling for the inspector
// =====================================================================================================

#[derive(Default, Clone)]
pub struct MixSample {
    pub cell: (i32, i32),
    /// (element id, count) in the 5×5 neighbourhood, most common first
    pub counts: Vec<(u32, u32)>,
    pub matter: u32,
    pub cast: u32,
    pub at: f64,
}

pub fn sample_mix(meta: &[u32], c: (i32, i32)) -> MixSample {
    let mut s = MixSample { cell: c, ..Default::default() };
    for dy in -2..=2 {
        for dx in -2..=2 {
            let (x, y) = (c.0 + dx, c.1 + dy);
            if x < 0 || y < 0 || x >= W as i32 || y >= H as i32 {
                continue;
            }
            let m = meta[(y as u32 * W + x as u32) as usize];
            if m & 0xff != scene::MATTER {
                continue;
            }
            let id = (m >> 8) & 0xff;
            s.matter += 1;
            if (m >> 16) & 1 != 0 {
                s.cast += 1;
            }
            match s.counts.iter_mut().find(|p| p.0 == id) {
                Some(p) => p.1 += 1,
                None => s.counts.push((id, 1)),
            }
        }
    }
    s.counts.sort_by(|a, b| b.1.cmp(&a.1));
    s
}

// =====================================================================================================
// UI
// =====================================================================================================

fn level_color(l: Level) -> Color32 {
    match l {
        Level::Info => MUTED,
        Level::Caution => WARN,
        Level::Danger => BAD,
    }
}

/// Horizontal stacked bar of fractions.
fn stack_bar(ui: &mut egui::Ui, parts: &[(Color32, f32)], h: f32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), h), Sense::hover());
    let p = ui.painter();
    p.rect_filled(rect, CornerRadius::ZERO, FIELD);
    let mut x0 = rect.left();
    for (c, f) in parts {
        let w = rect.width() * f.clamp(0.0, 1.0);
        p.rect_filled(Rect::from_min_size(Pos2::new(x0, rect.top()), Vec2::new(w, h)), CornerRadius::ZERO, *c);
        x0 += w;
    }
    p.rect_stroke(rect, CornerRadius::ZERO, Stroke::new(1.0_f32, BORDER), StrokeKind::Inside);
}

impl App {
    /// Called every frame: refreshes the inspector's mixture sample (blocking META readback, <= 3 Hz, only while
    /// the cursor is over matter).
    pub(super) fn prop_tick(&mut self, frame: &eframe::Frame, probe: Option<ProbeData>, now: f64) {
        let Some(pd) = probe else { return };
        if pd.meta & 0xff != scene::MATTER || (self.hover.is_none() && !self.pin_cell) {
            return;
        }
        let c = self.last_probe_cell;
        let fresh = self.mix_sample.as_ref().map(|m| m.cell == c && now - m.at < 0.35).unwrap_or(false);
        if fresh {
            return;
        }
        if let Some(meta) = self.current_meta(frame) {
            let mut s = sample_mix(&meta, c);
            s.at = now;
            self.mix_sample = Some(s);
        }
    }

    /// Inspector block for a matter cell: local composition (5×5 cells) and packing.
    pub(super) fn mix_sheet(&self, ui: &mut egui::Ui) {
        let Some(s) = self.mix_sample.as_ref().filter(|s| s.cell == self.last_probe_cell && s.matter > 0) else { return };
        ui.add_space(4.0);
        ui.label(faint("LOCAL COMPOSITION  (5 × 5 cells, by volume)"));
        let parts: Vec<(Color32, f32)> = s.counts.iter().map(|(id, n)| (elem_color(*id), *n as f32 / s.matter as f32)).collect();
        stack_bar(ui, &parts, 6.0);
        ui.add_space(2.0);
        egui::Grid::new("mixg").num_columns(3).spacing([10.0, 2.0]).show(ui, |ui| {
            for (id, n) in s.counts.iter().take(6) {
                ui.horizontal(|ui| {
                    swatch(ui, elem_color(*id), 8.0);
                    ui.label(muted(elements::matter_name(*id)));
                });
                let vol = *n as f32 / s.matter as f32;
                ui.label(num(format!("{:5.1} %", vol * 100.0)));
                // mass fraction from densities
                let msum: f32 = s.counts.iter().map(|(i, k)| *k as f32 * elem_density(*i, 1.5)).sum();
                ui.label(RichText::new(format!("{:5.1} % wt", *n as f32 * elem_density(*id, 1.5) / msum.max(1e-6) * 100.0)).monospace().color(FAINT).size(10.5));
                ui.end_row();
            }
        });
        let mixed = if s.counts.len() <= 1 { "single component".to_string() } else { format!("{}-component blend", s.counts.len()) };
        let packing = if s.cast * 2 > s.matter { "cast / pressed grain (burns at the surface only)" } else { "loose (porous, flame penetrates)" };
        sheet(ui, "mixk", |ui| {
            kv(ui, "Mixture", mixed);
            kv(ui, "Packing", packing);
        });
    }

    /// "Compositions" group in the materials library.
    pub(super) fn compositions_group(&mut self, ui: &mut egui::Ui, q: &str) {
        let items: Vec<(usize, &Composition, Vec<(u32, f32)>)> = COMPOSITIONS
            .iter()
            .enumerate()
            .filter(|(_, c)| q.is_empty() || c.name.to_lowercase().contains(q) || c.note.to_lowercase().contains(q))
            .filter_map(|(k, c)| resolve(c).map(|v| (k, c, v)))
            .collect();
        if items.is_empty() || self.run.is_some() {
            return;
        }
        let mut job = egui::text::LayoutJob::default();
        job.append("Compositions", 0.0, egui::TextFormat { font_id: head_font(), color: TEXT, ..Default::default() });
        job.append(&format!("   {}", items.len()), 0.0, egui::TextFormat { font_id: small_font(), color: FAINT, ..Default::default() });
        egui::CollapsingHeader::new(job).id_salt(("libcat", "comp")).default_open(false).open(if q.is_empty() { None } else { Some(true) }).show(ui, |ui| {
            for (k, c, v) in items {
                let w = ui.available_width();
                let (rect, resp) = ui.allocate_exact_size(Vec2::new(w, 20.0), Sense::click());
                let sel = self.comp.as_ref().map(|b| b.name == c.name).unwrap_or(false) && matches!(self.tool, Tool::Matter(_));
                let p = ui.painter();
                if sel {
                    p.rect_filled(rect, CornerRadius::same(2), ACCENT_DIM);
                    p.rect_filled(Rect::from_min_size(rect.min, Vec2::new(2.0, rect.height())), CornerRadius::ZERO, ACCENT);
                } else if resp.hovered() {
                    p.rect_filled(rect, CornerRadius::same(2), HOVER);
                }
                // proportional swatch
                let sw = Rect::from_center_size(Pos2::new(rect.left() + 10.0, rect.center().y), Vec2::new(9.0, 9.0));
                let mut y0 = sw.top();
                for (id, f) in &v {
                    let hgt = sw.height() * f;
                    p.rect_filled(Rect::from_min_size(Pos2::new(sw.left(), y0), Vec2::new(sw.width(), hgt)), CornerRadius::ZERO, elem_color(*id));
                    y0 += hgt;
                }
                let tc = if sel { STRONG } else { TEXT };
                p.text(Pos2::new(rect.left() + 20.0, rect.center().y), Align2::LEFT_CENTER, c.name, FontId::proportional(12.0), tc);
                let ratio: Vec<String> = c.parts.iter().map(|(_, p)| format!("{p:.0}")).collect();
                p.text(Pos2::new(rect.right() - 2.0, rect.center().y), Align2::RIGHT_CENTER, ratio.join(":"), FontId::monospace(10.5), MUTED);
                let tip = {
                    let names: Vec<String> = v.iter().map(|(id, f)| format!("{:>5.1} %  {}", f * 100.0, elements::matter_name(*id))).collect();
                    format!("{}\n{}\n\nMass fractions:\n{}\n\nClick to paint this blend{}.", c.name, c.note, names.join("\n"), if c.cast { " (cast grain)" } else { "" })
                };
                let resp = resp.on_hover_text(tip);
                if resp.clicked() {
                    self.use_composition(k, c.cast);
                }
                resp.context_menu(|ui| {
                    if ui.button("Paint as loose powder").clicked() {
                        self.use_composition(k, false);
                        ui.close_menu();
                    }
                    if ui.button("Paint as cast / pressed grain").clicked() {
                        self.use_composition(k, true);
                        ui.close_menu();
                    }
                    if let Some(d) = c.designer {
                        if ui.button("Open in Propellant Designer").clicked() {
                            self.designer.load_preset(d);
                            self.show_designer = true;
                            ui.close_menu();
                        }
                    }
                });
            }
            ui.add_space(4.0);
        });
    }

    pub(super) fn use_composition(&mut self, k: usize, cast: bool) {
        let c = &COMPOSITIONS[k];
        let Some(v) = resolve(c) else { return };
        if let Some(b) = Brush::from_mass(c.name, &v, cast) {
            self.set_brush(b);
        }
    }

    pub(super) fn set_brush(&mut self, b: Brush) {
        let base = b.base();
        let name = b.name.clone();
        let cast = b.cast;
        self.pick(Tool::Matter(base));
        self.comp = Some(b);
        self.place = None;
        self.picker = false;
        if self.shape == Shape::Fill {
            self.shape = Shape::Brush;
        }
        self.log_info(&format!("Composition brush: {name}{}", if cast { " (cast / pressed)" } else { " (loose)" }));
    }

    /// Selected-material block when a composition brush is active.
    pub(super) fn comp_sheet(&mut self, ui: &mut egui::Ui) -> bool {
        let Some(b) = self.comp.clone() else { return false };
        if self.tool != Tool::Matter(b.base()) {
            return false;
        }
        ui.horizontal(|ui| {
            let (r, _) = ui.allocate_exact_size(Vec2::splat(14.0), Sense::hover());
            let mut y0 = r.top();
            for (id, f) in &b.parts {
                let h = r.height() * f;
                ui.painter().rect_filled(Rect::from_min_size(Pos2::new(r.left(), y0), Vec2::new(r.width(), h)), CornerRadius::ZERO, elem_color(*id));
                y0 += h;
            }
            ui.label(RichText::new(&b.name).font(FontId::new(13.0, fam("semibold"))).color(STRONG));
            ui.label(faint("composition"));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.small_button("Clear").on_hover_text("Paint the single base element instead").clicked() {
                    self.comp = None;
                }
            });
        });
        let parts: Vec<(Color32, f32)> = b.mass.iter().map(|(id, f)| (elem_color(*id), *f)).collect();
        stack_bar(ui, &parts, 6.0);
        sheet(ui, "compk", |ui| {
            for (id, f) in &b.mass {
                kv(ui, elements::matter_name(*id), format!("{:.1} % wt", f * 100.0));
            }
        });
        let mut cast = b.cast;
        if ui.checkbox(&mut cast, "Cast / pressed grain").on_hover_text("Cast grains burn only at their exposed surface, layer by layer (r = a·pⁿ). Loose powder burns through its pores almost at once.").changed() {
            if let Some(bb) = self.comp.as_mut() {
                bb.cast = cast;
            }
        }
        if !native_mixture() {
            ui.label(faint("Blend placed cell by cell (solver mixture brush not available)."));
        }
        true
    }

    // ------------------------------------------------------------------------------------- designer window
    pub(super) fn designer_window(&mut self, ctx: &egui::Context) {
        if !self.show_designer {
            return;
        }
        let mut open = true;
        let scr = ctx.screen_rect();
        egui::Window::new("Propellant Designer")
            .open(&mut open)
            .collapsible(true)
            .resizable(false)
            .default_width(430.0)
            .default_pos(Pos2::new(scr.right() - 760.0, 84.0))
            .show(ctx, |ui| {
                ui.set_width(430.0);
                self.designer_ui(ui);
            });
        self.show_designer = open;
    }

    fn designer_ui(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(muted("Preset"));
            let cur = DESIGNER_PRESETS[self.designer.preset.min(DESIGNER_PRESETS.len() - 1)].0;
            egui::ComboBox::from_id_salt("dpreset").width(290.0).selected_text(cur).show_ui(ui, |ui| {
                for (k, (n, _, _)) in DESIGNER_PRESETS.iter().enumerate() {
                    if ui.selectable_label(self.designer.preset == k, *n).clicked() {
                        self.designer.load_preset(k);
                    }
                }
            });
        });
        ui.add_space(6.0);
        ui.label(faint("FORMULATION  (mass %)"));
        let slots = [(Role::Oxidiser, "Oxidiser", false), (Role::Fuel, "Fuel", true), (Role::Metal, "Metal fuel", true), (Role::Catalyst, "Catalyst", true), (Role::Binder, "Binder", true)];
        egui::Grid::new("dslots").num_columns(3).spacing([8.0, 4.0]).show(ui, |ui| {
            for (s, (role, label, optional)) in slots.iter().enumerate() {
                ui.label(muted(*label));
                let avail = ingr_of(*role);
                let cur = self.designer.sel[s].map(|i| INGREDIENTS[i].label).unwrap_or("none");
                egui::ComboBox::from_id_salt(("dsel", s)).width(170.0).selected_text(cur).show_ui(ui, |ui| {
                    if *optional && ui.selectable_label(self.designer.sel[s].is_none(), "none").clicked() {
                        self.designer.sel[s] = None;
                    }
                    for (k, i, _) in &avail {
                        if ui.selectable_label(self.designer.sel[s] == Some(*k), i.label).clicked() {
                            self.designer.sel[s] = Some(*k);
                            if self.designer.pct[s] <= 0.0 {
                                self.designer.pct[s] = match role {
                                    Role::Catalyst => 1.0,
                                    Role::Metal => 10.0,
                                    Role::Binder => 14.0,
                                    _ => 30.0,
                                };
                            }
                        }
                    }
                });
                ui.add_enabled(self.designer.sel[s].is_some(), egui::DragValue::new(&mut self.designer.pct[s]).range(0.0..=100.0).speed(0.25).fixed_decimals(1).suffix(" %"));
                ui.end_row();
            }
        });
        let tot: f32 = (0..5).filter(|s| self.designer.sel[*s].is_some()).map(|s| self.designer.pct[s]).sum();
        ui.horizontal(|ui| {
            ui.label(muted("Total"));
            ui.label(RichText::new(format!("{tot:.1} %")).monospace().color(if (tot - 100.0).abs() < 0.05 { TEXT } else { WARN }));
            if (tot - 100.0).abs() >= 0.05 && tot > 0.0 && ui.small_button("Normalise to 100 %").clicked() {
                for s in 0..5 {
                    self.designer.pct[s] *= 100.0 / tot;
                }
            }
        });
        let pr = self.designer.predict();
        if !pr.missing.is_empty() {
            ui.label(RichText::new(format!("Not available in the materials library: {}", pr.missing.join(", "))).color(WARN).size(11.0));
        }
        let parts: Vec<(Color32, f32)> = pr.mass.iter().map(|(id, f, _)| (elem_color(*id), *f)).collect();
        stack_bar(ui, &parts, 6.0);
        ui.add_space(8.0);
        ui.label(faint("PREDICTED PERFORMANCE  (oxygen-balance estimate)"));
        let u = self.units;
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                ui.set_width(200.0);
                sheet(ui, "dpred", |ui| {
                    kv(ui, "Flame temperature", u.ft(pr.t_flame));
                    kv(ui, "Burn-rate law", format!("r = {:.2}·p^{:.3}", pr.a, pr.n));
                    kv(ui, "  r at 1 atm", format!("{:.2} mm/s", pr.a * 0.101325f32.powf(pr.n)));
                    kv(ui, "  r at 3.5 MPa", format!("{:.2} mm/s", pr.a * 3.5f32.powf(pr.n)));
                    kv(ui, "  r at 7 MPa", format!("{:.2} mm/s", pr.a * 7.0f32.powf(pr.n)));
                    kv(ui, "Ideal Isp (7 MPa → 1 atm)", format!("{:.0} s", pr.isp));
                    kv(ui, "Density", format!("{:.0} kg/m³", pr.rho * 1000.0));
                    kv(ui, "Equivalence φ", format!("{:.2}", pr.phi));
                    kv(ui, "Oxygen balance", format!("{:+.1} %", pr.ob));
                    kv(ui, "Product M", format!("{:.1} g/mol", pr.m));
                });
            });
            ui.add_space(6.0);
            burn_chart(ui, Vec2::new(200.0, 160.0), pr.a, pr.n);
        });
        ui.add_space(6.0);
        if !pr.notes.is_empty() {
            ui.label(faint("SAFETY AND QUALITY NOTES"));
            for (l, s) in &pr.notes {
                ui.horizontal_top(|ui| {
                    let (r, _) = ui.allocate_exact_size(Vec2::new(8.0, 14.0), Sense::hover());
                    ui.painter().rect_filled(Rect::from_center_size(r.center(), Vec2::new(3.0, 10.0)), CornerRadius::ZERO, level_color(*l));
                    ui.add(egui::Label::new(RichText::new(s).color(TEXT).size(11.0)).wrap());
                });
            }
        }
        ui.label(RichText::new("Estimates calibrated to published data (Nakka; Sutton & Biblarz). For the simulator only; not formulation guidance.").color(FAINT).size(10.0));
        ui.add_space(6.0);
        ui.separator();
        ui.horizontal(|ui| {
            ui.checkbox(&mut self.designer.cast, "Cast / pressed").on_hover_text("Cast grains burn only at the exposed surface. Loose blends burn through their pores (fast, explosive when confined).");
            let edit_ok = self.can_edit_layout();
            if ui.add_enabled(edit_ok && !pr.mass.is_empty(), egui::Button::new(RichText::new(if self.designer.cast { "Paint cast grain" } else { "Paint loose blend" }).color(STRONG)).fill(ACCENT_DIM).stroke(Stroke::new(1.0_f32, ACCENT))).on_hover_text("Select a composition brush with this formulation (box shape)").clicked() {
                let mass: Vec<(u32, f32)> = pr.mass.iter().map(|(i, f, _)| (*i, *f)).collect();
                let name = format!("Custom: {}", pr.mass.iter().map(|(_, f, l)| format!("{l} {:.0}", f * 100.0)).collect::<Vec<_>>().join(" / "));
                if let Some(b) = Brush::from_mass(&name, &mass, self.designer.cast) {
                    self.set_brush(b);
                    self.shape = Shape::Box;
                }
            }
        });
        ui.horizontal(|ui| {
            ui.label(muted("Test motor"));
            egui::ComboBox::from_id_salt("dgrain").width(130.0).selected_text(["End-burner", "Core-burner"][self.designer.grain.min(1)]).show_ui(ui, |ui| {
                ui.selectable_value(&mut self.designer.grain, 0, "End-burner").on_hover_text("Burns from one face only: long, steady, low thrust");
                ui.selectable_value(&mut self.designer.grain, 1, "Core-burner").on_hover_text("Central channel: large burning area, higher and progressive thrust");
            });
            ui.label(muted("throat"));
            ui.add(egui::DragValue::new(&mut self.designer.throat).range(2..=24).suffix(" cm"));
            if ui.add_enabled(self.run.is_none() && !pr.mass.is_empty(), egui::Button::new("Build")).on_hover_text("New setup: steel case, this grain (cast), nozzle and an igniter").clicked() {
                let mass: Vec<(u32, f32)> = pr.mass.iter().map(|(i, f, _)| (*i, *f)).collect();
                if let Some(b) = Brush::from_mass("Test motor grain", &mass, true) {
                    self.build_test_motor(&b, self.designer.grain, self.designer.throat);
                }
            }
        });
    }

    /// New setup with a static-test motor: steel case (head end closed), cast grain, converging-diverging nozzle,
    /// igniter at the free grain face.
    pub(super) fn build_test_motor(&mut self, b: &Brush, grain: usize, throat: i32) {
        self.start_sandbox_named("Static test motor", Scene::empty());
        let st = |x0: i32, y0: i32, x1: i32, y1: i32, shape: u32, r: i32| Op { kind: 0, shape, x0, y0, x1, y1, r, tile: scene::WALL, mat: 0, dir: 0, _p: [0; 2] };
        let er = |x0: i32, y0: i32, x1: i32, y1: i32| Op { kind: 1, shape: 1, x0, y0, x1, y1, r: 0, tile: 0, mat: 0, dir: 0, _p: [0; 2] };
        let (x0, x1) = (120, 300);
        let (yc, half) = (H as i32 - 70, 18);
        let (ya, yb) = (yc - half, yc + half);
        let mut ops = vec![];
        // case (hollow box, 3 cells thick) and test stand floor
        ops.push(st(x0, ya - 3, x1, yb + 3, 2, 2));
        let th = (throat / 2).max(1);
        // nozzle: open the aft closure at the throat, then a diverging cone
        ops.push(er(x1 - 2, yc - th, x1, yc + th - 1 + throat % 2));
        ops.push(st(x1 + 1, yc - th - 1, x1 + 34, yc - th - 1 - 14, 3, 1));
        ops.push(st(x1 + 1, yc + th + throat % 2, x1 + 34, yc + th + throat % 2 + 14, 3, 1));
        // convergent section inside the case
        ops.push(st(x1 - 16, ya, x1 - 3, yc - th - 1, 3, 1));
        ops.push(st(x1 - 16, yb, x1 - 3, yc + th + throat % 2, 3, 1));
        // grain
        let gx1 = if grain == 0 { x1 - 70 } else { x1 - 24 };
        let g = Op { kind: 0, shape: 1, x0: x0 + 3, y0: ya, x1: gx1, y1: yb, r: 0, tile: scene::MATTER, mat: b.base(), dir: 0, _p: [0; 2] };
        let mut gops = expand(b, &[g]);
        ops.append(&mut gops);
        let ig = |x: i32, y: i32| Op { kind: 0, shape: 0, x0: x, y0: y, x1: x, y1: y, r: 0, tile: scene::IGNITER, mat: 0, dir: 0, _p: [0; 2] };
        if grain == 1 {
            // central channel (core) through the grain, igniter wire along it
            ops.push(er(x0 + 12, yc - 3, gx1, yc + 3));
            ops.push(ig(x0 + 30, yc));
            ops.push(ig(x0 + 70, yc));
            ops.push(ig(gx1 - 8, yc));
        } else {
            ops.push(ig(gx1 + 1, yc - 6));
            ops.push(ig(gx1 + 1, yc + 6));
        }
        self.op_queue.extend(ops);
        self.ign_on = true;
        self.engine_scene = true;
        self.paused = false;
        self.log_info(&format!("Static test motor: {} grain, throat {} cm, {}", ["end-burning", "core-burning"][grain.min(1)], throat, b.mass.iter().map(|(i, f)| format!("{} {:.0} %", elements::matter_name(*i), f * 100.0)).collect::<Vec<_>>().join(", ")));
    }
}

/// log-log burn rate chart r(p), p = 0.1 .. 10 MPa
fn burn_chart(ui: &mut egui::Ui, size: Vec2, a: f32, n: f32) {
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    let p = ui.painter();
    p.rect_filled(rect, CornerRadius::ZERO, FIELD);
    p.rect_stroke(rect, CornerRadius::ZERO, Stroke::new(1.0_f32, BORDER), StrokeKind::Inside);
    let plot = Rect::from_min_max(rect.min + Vec2::new(30.0, 18.0), rect.max - Vec2::new(8.0, 20.0));
    let (lp0, lp1) = (-1.0f32, 1.0f32); // log10 MPa
    let (lr0, lr1) = (-0.5f32, 2.0f32); // log10 mm/s
    let tx = |lp: f32| plot.left() + (lp - lp0) / (lp1 - lp0) * plot.width();
    let ty = |lr: f32| plot.bottom() - (lr - lr0) / (lr1 - lr0) * plot.height();
    for d in [-1.0f32, 0.0, 1.0] {
        let x = tx(d);
        p.vline(x, plot.y_range(), Stroke::new(1.0_f32, BORDER));
        p.text(Pos2::new(x, plot.bottom() + 3.0), Align2::CENTER_TOP, format!("{}", 10f32.powf(d)), FontId::monospace(9.5), FAINT);
    }
    for d in [0.0f32, 1.0, 2.0] {
        let y = ty(d);
        p.hline(plot.x_range(), y, Stroke::new(1.0_f32, BORDER));
        p.text(Pos2::new(plot.left() - 3.0, y), Align2::RIGHT_CENTER, format!("{}", 10f32.powf(d)), FontId::monospace(9.5), FAINT);
    }
    p.text(rect.left_top() + Vec2::new(6.0, 4.0), Align2::LEFT_TOP, "burn rate  mm/s", small_font(), MUTED);
    p.text(Pos2::new(plot.right(), rect.bottom() - 2.0), Align2::RIGHT_BOTTOM, "p  MPa", small_font(), FAINT);
    let pts: Vec<Pos2> = (0..=40)
        .map(|k| {
            let lp = lp0 + (lp1 - lp0) * k as f32 / 40.0;
            let lr = (a.max(1e-4) * 10f32.powf(lp).powf(n)).log10().clamp(lr0, lr1);
            Pos2::new(tx(lp), ty(lr))
        })
        .collect();
    p.add(egui::Shape::line(pts, Stroke::new(1.5_f32, ACCENT)));
}
