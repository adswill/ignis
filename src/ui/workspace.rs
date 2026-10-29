//! The Ignis workspace: menu bar, icon toolbar, materials library (left dock), Properties / Inspector
//! (right dock), tabbed bottom dock (plots, probes, profiles, console, reference, experiments), status bar,
//! viewport overlays (axes, scale bar, colour-map legend, probes) and floating windows.
use super::probes::{Place, Sample};
use super::theme::*;
use super::*;
use crate::library::BrowserAction;
use egui::{Mesh, RichText, Shape as EShape};

// =====================================================================================================
// units
// =====================================================================================================

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum TempU {
    K,
    C,
    F,
}
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum PressU {
    Atm,
    KPa,
    Bar,
    Psi,
}
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum VelU {
    Ms,
    Kmh,
    Fts,
}
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum LenU {
    Mm,
    Cm,
    M,
}

/// User-selectable display units. Internally everything stays in sim units / SI.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Units {
    pub t: TempU,
    pub p: PressU,
    pub v: VelU,
    pub l: LenU,
}

impl Default for Units {
    fn default() -> Self {
        Units { t: TempU::K, p: PressU::KPa, v: VelU::Ms, l: LenU::Cm }
    }
}

impl Units {
    pub fn engineering() -> Self {
        Units { t: TempU::C, p: PressU::Bar, v: VelU::Ms, l: LenU::Mm }
    }
    fn path() -> Option<std::path::PathBuf> {
        data_dir().map(|d| d.join("units.txt"))
    }
    pub fn load() -> Self {
        let mut u = Units::default();
        let Some(s) = Self::path().and_then(|p| std::fs::read_to_string(p).ok()) else { return u };
        for w in s.split_whitespace() {
            match w {
                "K" => u.t = TempU::K,
                "C" => u.t = TempU::C,
                "F" => u.t = TempU::F,
                "atm" => u.p = PressU::Atm,
                "kPa" => u.p = PressU::KPa,
                "bar" => u.p = PressU::Bar,
                "psi" => u.p = PressU::Psi,
                "m/s" => u.v = VelU::Ms,
                "km/h" => u.v = VelU::Kmh,
                "ft/s" => u.v = VelU::Fts,
                "mm" => u.l = LenU::Mm,
                "cm" => u.l = LenU::Cm,
                "m" => u.l = LenU::M,
                _ => {}
            }
        }
        u
    }
    pub fn save(&self) {
        if std::env::var("COMBUSTOR_SHOT").is_ok() {
            return;
        }
        if let Some(p) = Self::path() {
            let _ = std::fs::create_dir_all(p.parent().unwrap());
            let _ = std::fs::write(p, format!("{} {} {} {}", self.t_unit(), self.p_unit(), self.v_unit(), self.l_unit()));
        }
    }
    pub fn t_unit(&self) -> &'static str {
        match self.t {
            TempU::K => "K",
            TempU::C => "°C",
            TempU::F => "°F",
        }
    }
    pub fn p_unit(&self) -> &'static str {
        match self.p {
            PressU::Atm => "atm",
            PressU::KPa => "kPa",
            PressU::Bar => "bar",
            PressU::Psi => "psi",
        }
    }
    pub fn v_unit(&self) -> &'static str {
        match self.v {
            VelU::Ms => "m/s",
            VelU::Kmh => "km/h",
            VelU::Fts => "ft/s",
        }
    }
    pub fn l_unit(&self) -> &'static str {
        match self.l {
            LenU::Mm => "mm",
            LenU::Cm => "cm",
            LenU::M => "m",
        }
    }
    /// kelvin -> display
    pub fn t(&self, k: f32) -> f32 {
        match self.t {
            TempU::K => k,
            TempU::C => k - 273.15,
            TempU::F => (k - 273.15) * 1.8 + 32.0,
        }
    }
    /// display -> kelvin
    pub fn t_inv(&self, v: f32) -> f32 {
        match self.t {
            TempU::K => v,
            TempU::C => v + 273.15,
            TempU::F => (v - 32.0) / 1.8 + 273.15,
        }
    }
    /// atm -> display
    pub fn p(&self, atm: f32) -> f32 {
        atm * self.p_scale()
    }
    pub fn p_scale(&self) -> f32 {
        match self.p {
            PressU::Atm => 1.0,
            PressU::KPa => 101.325,
            PressU::Bar => 1.01325,
            PressU::Psi => 14.6959,
        }
    }
    /// m/s -> display
    pub fn v(&self, ms: f32) -> f32 {
        ms * match self.v {
            VelU::Ms => 1.0,
            VelU::Kmh => 3.6,
            VelU::Fts => 3.28084,
        }
    }
    /// cm -> display
    pub fn l(&self, cm: f32) -> f32 {
        cm * match self.l {
            LenU::Mm => 10.0,
            LenU::Cm => 1.0,
            LenU::M => 0.01,
        }
    }
    pub fn ft(&self, k: f32) -> String {
        if !k.is_finite() {
            return "—".into();
        }
        format!("{:.0} {}", self.t(k), self.t_unit())
    }
    pub fn fp(&self, atm: f32) -> String {
        if !atm.is_finite() {
            return "—".into();
        }
        let v = self.p(atm);
        let d = if v.abs() >= 100.0 { 0 } else if v.abs() >= 10.0 { 1 } else { 2 };
        format!("{v:.d$} {}", self.p_unit())
    }
    pub fn fv(&self, ms: f32) -> String {
        if !ms.is_finite() {
            return "—".into();
        }
        let v = self.v(ms);
        let d = if v.abs() >= 100.0 { 0 } else { 1 };
        format!("{v:.d$} {}", self.v_unit())
    }
    pub fn fl(&self, cm: f32) -> String {
        let v = self.l(cm);
        let d = match self.l {
            LenU::M => 2,
            _ => 0,
        };
        format!("{v:.d$} {}", self.l_unit())
    }
}

// =====================================================================================================
// small shared types
// =====================================================================================================

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum BottomTab {
    Plots,
    Probes,
    Profiles,
    Console,
    Reference,
    Experiment,
}

impl BottomTab {
    const ALL: [(BottomTab, &'static str); 6] = [
        (BottomTab::Plots, "Plots"),
        (BottomTab::Probes, "Probes"),
        (BottomTab::Profiles, "Line profiles"),
        (BottomTab::Console, "Console"),
        (BottomTab::Reference, "Reference"),
        (BottomTab::Experiment, "Experiments"),
    ];
    pub fn from_name(s: &str) -> Option<BottomTab> {
        let s = s.to_lowercase();
        Self::ALL.iter().find(|(_, n)| n.to_lowercase().split(' ').any(|w| w.starts_with(&s))).map(|t| t.0)
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Level {
    Info,
    Warn,
}

pub struct LogEntry {
    pub t_ms: f32,
    pub level: Level,
    pub text: String,
}

/// A loadable setup for File > New from template.
pub struct Template {
    pub key: String,
    pub title: &'static str,
    pub desc: &'static str,
    pub group: &'static str,
    pub build: fn() -> Scene,
}

pub fn templates() -> Vec<Template> {
    let mut v = vec![];
    for (t, d, f) in SCENARIOS.iter() {
        let group = match *t {
            "Turbojet" | "Rocket engine" | "Solid rocket motor" => "Propulsion",
            "Blank canvas" => "General",
            "Demolition" | "Pressure vessels" => "Structures & blast",
            "Chemistry bench" => "Chemistry",
            _ => "Flames & fires",
        };
        v.push(Template { key: t.to_lowercase(), title: t, desc: d, group, build: *f });
    }
    for s in scene::SHOWCASE.iter() {
        v.push(Template { key: s.0.to_string(), title: s.1, desc: s.2, group: "Chemistry", build: s.3 });
    }
    for (k, (t, d, f)) in crate::showcase::all().into_iter().enumerate() {
        let group = match k {
            0 | 12 | 13 => "Propulsion",
            1 | 4 | 8 | 9 | 10 => "Structures & blast",
            5 | 6 | 7 | 11 | 15 | 17 => "Flames & fires",
            _ => "Materials & heat transfer",
        };
        v.push(Template { key: crate::showcase::NAMES.get(k).map(|s| s.to_string()).unwrap_or_default(), title: t, desc: d, group, build: f });
    }
    v
}

const VIEW_NAMES: [(&str, &str); 6] = [
    ("Render", "Physically based render: incandescence, chemiluminescence, soot and smoke"),
    ("Pressure", "Static pressure field"),
    ("Temperature", "Gas and solid temperature"),
    ("Velocity", "Flow speed (brightness) and direction (hue)"),
    ("Species", "Composition: fuel, oxygen, combustion products, soot"),
    ("Stress", "Wall stress as a fraction of yield strength"),
];

/// Approximate real densities (kg/m³) of the wall materials 0-9, for the library columns.
const WALL_RHO: [f32; 10] = [7850.0, 2700.0, 8960.0, 4510.0, 8440.0, 3950.0, 8960.0, 600.0, 1770.0, 1650.0];

/// (density kg/m³, characteristic temperature K + its kind) for the library columns / data sheets.
fn props(t: Tool) -> (Option<f32>, Option<(f32, &'static str)>) {
    match t {
        Tool::Wall(m) => {
            let mi = &MATS[(m as usize).min(9)];
            let tk = match m {
                7 => (600.0, "pyrolysis"),
                8 => (750.0, "ignition"),
                9 => (600.0, "ignition"),
                _ => (mi.t_melt * 300.0, "melting"),
            };
            (Some(WALL_RHO[(m as usize).min(9)]), Some(tk))
        }
        Tool::Matter(id) => match elements::matter_info(id) {
            Some(i) => {
                let tk = i.ignite_k.map(|v| (v, "ignition")).or(i.melt_k.map(|v| (v, "melting"))).or(i.boil_k.map(|v| (v, "boiling")));
                (Some(i.density * 1000.0), tk)
            }
            None => (None, None),
        },
        Tool::Gas(2) => (Some(0.657), Some((868.0, "autoignition"))),
        Tool::Gas(3) => (Some(1.429), Some((90.2, "boiling"))),
        Tool::Gas(4) => (Some(1.204), None),
        _ => (None, None),
    }
}

// =====================================================================================================
// colour maps (mirror vis.wgsl so legends match the field views)
// =====================================================================================================

fn lerp3(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t]
}

fn heat_ramp(tv: f32) -> [f32; 3] {
    let st = [[0.0, 0.0, 0.0], [0.05, 0.0, 0.4], [0.0, 0.3, 1.0], [0.0, 0.9, 0.8], [0.2, 0.95, 0.1], [1.0, 0.95, 0.0], [1.0, 0.2, 0.0], [1.0, 1.0, 1.0]];
    let f = tv.clamp(0.0, 1.0) * 7.0;
    let k = (f.floor() as usize).min(6);
    lerp3(st[k], st[k + 1], f - k as f32)
}

fn stress_ramp(r: f32) -> [f32; 3] {
    let k = r.clamp(0.0, 1.2);
    if k < 0.5 {
        lerp3([0.10, 0.45, 0.20], [0.85, 0.85, 0.15], k / 0.5)
    } else if k < 1.0 {
        lerp3([0.85, 0.85, 0.15], [1.0, 0.15, 0.05], (k - 0.5) / 0.5)
    } else {
        [1.0, 0.3, 0.6]
    }
}

fn pressure_ramp(p: f32) -> [f32; 3] {
    let q = (p - 1.0) / 1.5;
    let mut b = [q.clamp(0.0, 1.0), (q * 0.28).clamp(0.0, 1.0), (-q * 1.2).clamp(0.0, 1.0)];
    if q > 1.0 {
        b[1] = (q - 1.0).min(1.0);
        b[2] = (q - 1.5).clamp(0.0, 1.0);
    }
    b
}

fn glow_ramp(tk: f32) -> [f32; 3] {
    let st = [(800.0, [0.05, 0.0, 0.0]), (1000.0, [0.55, 0.06, 0.0]), (1500.0, [1.0, 0.42, 0.05]), (2000.0, [1.0, 0.68, 0.25]), (3000.0, [1.0, 0.92, 0.72])];
    if tk <= st[0].0 {
        return st[0].1;
    }
    for w in st.windows(2) {
        if tk <= w[1].0 {
            return lerp3(w[0].1, w[1].1, (tk - w[0].0) / (w[1].0 - w[0].0));
        }
    }
    st[4].1
}

fn c3(c: [f32; 3]) -> Color32 {
    let g = |v: f32| (v.clamp(0.0, 1.0) * 255.0) as u8;
    Color32::from_rgb(g(c[0]), g(c[1]), g(c[2]))
}

/// Ticks at "nice" values covering [lo, hi].
fn nice_ticks(lo: f32, hi: f32, n: usize) -> Vec<f32> {
    if !(hi > lo) || !lo.is_finite() || !hi.is_finite() {
        return vec![];
    }
    let raw = (hi - lo) / n.max(1) as f32;
    let mag = 10f32.powf(raw.log10().floor());
    let step = [1.0, 2.0, 2.5, 5.0, 10.0].iter().map(|m| m * mag).find(|s| *s >= raw).unwrap_or(10.0 * mag);
    let mut v = vec![];
    let mut x = (lo / step).ceil() * step;
    while x <= hi + step * 1e-3 {
        v.push(if x.abs() < step * 1e-4 { 0.0 } else { x });
        x += step;
    }
    v
}

fn tick_label(v: f32, step: f32) -> String {
    let d = if step >= 1.0 { 0 } else if step >= 0.1 { 1 } else if step >= 0.01 { 2 } else { 3 };
    if v.abs() >= 10000.0 {
        format!("{:.0}k", v / 1000.0)
    } else {
        format!("{v:.d$}")
    }
}

// =====================================================================================================
// vector icons for the toolbar (drawn, no emoji / bitmap glyphs)
// =====================================================================================================

#[derive(Clone, Copy, PartialEq)]
enum Icon {
    Play,
    Pause,
    Step,
    Reset,
    Undo,
    Redo,
    Brush,
    Line,
    Box,
    Frame,
    Fill,
    Spray,
    Pick,
    Erase,
    Mix,
    Cast,
    Probe,
    Sampler,
    Grid,
    New,
    Open,
    Save,
}

fn draw_icon(p: &egui::Painter, r: Rect, icon: Icon, c: Color32) {
    let s = Stroke::new(1.3_f32, c);
    let cx = r.center();
    let h = 6.0;
    let pt = |x: f32, y: f32| cx + Vec2::new(x, y);
    match icon {
        Icon::Play => {
            p.add(EShape::convex_polygon(vec![pt(-4.0, -5.5), pt(5.5, 0.0), pt(-4.0, 5.5)], c, Stroke::NONE));
        }
        Icon::Pause => {
            p.rect_filled(Rect::from_min_size(pt(-4.5, -5.5), Vec2::new(3.0, 11.0)), CornerRadius::ZERO, c);
            p.rect_filled(Rect::from_min_size(pt(1.5, -5.5), Vec2::new(3.0, 11.0)), CornerRadius::ZERO, c);
        }
        Icon::Step => {
            p.add(EShape::convex_polygon(vec![pt(-5.0, -5.0), pt(2.5, 0.0), pt(-5.0, 5.0)], c, Stroke::NONE));
            p.rect_filled(Rect::from_min_size(pt(3.0, -5.0), Vec2::new(2.2, 10.0)), CornerRadius::ZERO, c);
        }
        Icon::Reset => {
            let pts: Vec<Pos2> = (0..=20).map(|k| {
                let a = -0.6 + k as f32 / 20.0 * 5.0;
                pt(a.cos() * 5.0, a.sin() * 5.0)
            }).collect();
            let end = pts[0];
            p.add(EShape::line(pts, s));
            p.add(EShape::convex_polygon(vec![end + Vec2::new(-1.0, -4.0), end + Vec2::new(3.0, 0.5), end + Vec2::new(-2.5, 1.5)], c, Stroke::NONE));
        }
        Icon::Undo | Icon::Redo => {
            let sg = if icon == Icon::Undo { 1.0 } else { -1.0 };
            let pts: Vec<Pos2> = (0..=14).map(|k| {
                let a = std::f32::consts::PI * (k as f32 / 14.0);
                pt(sg * -a.cos() * 4.5 + sg * 1.0, -a.sin() * 4.5 + 3.0)
            }).collect();
            p.add(EShape::line(pts, s));
            let tip = pt(sg * -3.5, 3.0);
            p.line_segment([tip, tip + Vec2::new(sg * -2.5, -3.0)], s);
            p.line_segment([tip, tip + Vec2::new(sg * 3.0, -2.0)], s);
        }
        Icon::Brush => {
            p.circle_stroke(cx, 5.0, s);
            p.circle_filled(cx, 1.6, c);
        }
        Icon::Line => {
            p.line_segment([pt(-5.0, 5.0), pt(5.0, -5.0)], Stroke::new(1.8_f32, c));
        }
        Icon::Box => {
            p.rect_filled(Rect::from_center_size(cx, Vec2::splat(10.0)), CornerRadius::ZERO, c.gamma_multiply(0.85));
        }
        Icon::Frame => {
            p.rect_stroke(Rect::from_center_size(cx, Vec2::splat(10.0)), CornerRadius::ZERO, Stroke::new(2.0_f32, c), StrokeKind::Inside);
        }
        Icon::Fill => {
            p.rect_stroke(Rect::from_center_size(cx, Vec2::splat(11.0)), CornerRadius::ZERO, Stroke::new(1.0_f32, c.gamma_multiply(0.6)), StrokeKind::Inside);
            for (x, y) in [(-2.5, -2.5), (2.5, -2.5), (-2.5, 2.5), (2.5, 2.5)] {
                p.rect_filled(Rect::from_center_size(pt(x, y), Vec2::splat(3.0)), CornerRadius::ZERO, c);
            }
        }
        Icon::Spray => {
            for (x, y) in [(-4.0, -3.0), (0.0, -5.0), (4.0, -2.0), (-2.0, 1.0), (2.5, 2.5), (-4.5, 4.0), (1.0, 5.5), (5.0, 5.0)] {
                p.circle_filled(pt(x, y), 1.1, c);
            }
        }
        Icon::Pick => {
            p.circle_stroke(cx, 4.0, s);
            for d in [Vec2::X, -Vec2::X, Vec2::Y, -Vec2::Y] {
                p.line_segment([cx + d * 2.0, cx + d * h], s);
            }
        }
        Icon::Erase => {
            p.rect_stroke(Rect::from_center_size(cx, Vec2::new(11.0, 7.0)), CornerRadius::same(1), s, StrokeKind::Inside);
            p.line_segment([pt(-1.0, -3.5), pt(-1.0, 3.5)], s);
        }
        Icon::Mix => {
            // stirring rod in a beaker
            p.add(EShape::line(vec![pt(-5.0, -4.0), pt(-5.0, 5.5), pt(5.0, 5.5), pt(5.0, -4.0)], s));
            p.line_segment([pt(-1.5, 3.0), pt(3.5, -6.5)], s);
            p.circle_filled(pt(-2.5, 3.0), 0.9, c);
            p.circle_filled(pt(1.5, 2.0), 0.9, c);
        }
        Icon::Cast => {
            // grain block with a pressing ram
            p.rect_filled(Rect::from_center_size(pt(0.0, 3.0), Vec2::new(11.0, 5.0)), CornerRadius::ZERO, c);
            p.line_segment([pt(0.0, -6.0), pt(0.0, -1.5)], s);
            p.line_segment([pt(-4.0, -1.5), pt(4.0, -1.5)], s);
        }
        Icon::Probe => {
            p.line_segment([pt(-6.0, 0.0), pt(6.0, 0.0)], s);
            p.line_segment([pt(0.0, -6.0), pt(0.0, 6.0)], s);
            p.circle_stroke(cx, 3.0, s);
        }
        Icon::Sampler => {
            p.line_segment([pt(-5.5, 4.0), pt(5.5, -4.0)], s);
            for (x, y) in [(-5.5, 4.0), (5.5, -4.0)] {
                p.circle_filled(pt(x, y), 1.8, c);
            }
            for k in 1..4 {
                let f = k as f32 / 4.0;
                p.circle_filled(pt(-5.5 + 11.0 * f, 4.0 - 8.0 * f), 0.9, c);
            }
        }
        Icon::Grid => {
            let rr = Rect::from_center_size(cx, Vec2::splat(11.0));
            p.rect_stroke(rr, CornerRadius::ZERO, Stroke::new(1.0_f32, c), StrokeKind::Inside);
            p.line_segment([pt(-5.5, 0.0), pt(5.5, 0.0)], Stroke::new(1.0_f32, c));
            p.line_segment([pt(0.0, -5.5), pt(0.0, 5.5)], Stroke::new(1.0_f32, c));
        }
        Icon::New => {
            p.rect_stroke(Rect::from_center_size(cx, Vec2::new(9.0, 12.0)), CornerRadius::ZERO, s, StrokeKind::Inside);
        }
        Icon::Open => {
            p.add(EShape::line(vec![pt(-6.0, 4.5), pt(-6.0, -4.5), pt(-2.0, -4.5), pt(-0.5, -3.0), pt(5.5, -3.0), pt(5.5, 4.5), pt(-6.0, 4.5)], s));
        }
        Icon::Save => {
            p.rect_stroke(Rect::from_center_size(cx, Vec2::splat(11.0)), CornerRadius::same(1), s, StrokeKind::Inside);
            p.rect_filled(Rect::from_min_size(pt(-3.0, -5.5), Vec2::new(6.0, 3.5)), CornerRadius::ZERO, c);
        }
    }
}

fn icon_button(ui: &mut egui::Ui, icon: Icon, selected: bool, enabled: bool, tip: &str) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(26.0, 24.0), if enabled { Sense::click() } else { Sense::hover() });
    let p = ui.painter();
    if selected {
        p.rect_filled(rect, CornerRadius::same(3), ACCENT_DIM);
        p.rect_stroke(rect, CornerRadius::same(3), Stroke::new(1.0_f32, ACCENT.gamma_multiply(0.7)), StrokeKind::Inside);
    } else if resp.hovered() && enabled {
        p.rect_filled(rect, CornerRadius::same(3), HOVER);
    }
    let col = if !enabled {
        FAINT
    } else if selected {
        STRONG
    } else {
        TEXT
    };
    draw_icon(p, rect, icon, col);
    resp.on_hover_text(tip)
}

fn vsep(ui: &mut egui::Ui) {
    let (r, _) = ui.allocate_exact_size(Vec2::new(9.0, 20.0), Sense::hover());
    ui.painter().vline(r.center().x, r.y_range(), Stroke::new(1.0_f32, BORDER));
}

/// Key/value row in a data sheet grid.
pub(super) fn kv(ui: &mut egui::Ui, k: &str, v: impl Into<String>) {
    ui.label(muted(k));
    ui.label(num(v));
    ui.end_row();
}

pub(super) fn sheet(ui: &mut egui::Ui, id: &str, f: impl FnOnce(&mut egui::Ui)) {
    egui::Grid::new(id).num_columns(2).spacing([12.0, 3.0]).min_col_width(96.0).striped(false).show(ui, f);
}

pub(super) fn swatch(ui: &mut egui::Ui, c: Color32, size: f32) {
    let (r, _) = ui.allocate_exact_size(Vec2::splat(size), Sense::hover());
    ui.painter().rect_filled(r, CornerRadius::same(1), c);
    ui.painter().rect_stroke(r, CornerRadius::same(1), Stroke::new(1.0_f32, Color32::from_white_alpha(28)), StrokeKind::Inside);
}

/// A compact time-series / profile chart with axes, ticks and an inline legend.
struct Series<'a> {
    name: String,
    color: Color32,
    pts: &'a [[f32; 2]],
}

fn chart(ui: &mut egui::Ui, size: Vec2, title: &str, x_label: &str, y_unit: &str, series: &[Series<'_>], y_floor: Option<f32>) {
    let (rect, resp) = ui.allocate_exact_size(size, Sense::hover());
    let p = ui.painter_at(rect);
    p.rect_filled(rect, CornerRadius::same(2), Color32::from_rgb(20, 21, 24));
    p.rect_stroke(rect, CornerRadius::same(2), Stroke::new(1.0_f32, BORDER), StrokeKind::Inside);
    let tg = p.layout_no_wrap(title.to_string(), head_font(), TEXT);
    let tw = tg.size().x;
    p.galley(rect.min + Vec2::new(8.0, 6.0), tg, TEXT);
    if !y_unit.is_empty() {
        p.text(rect.min + Vec2::new(14.0 + tw, 6.0), Align2::LEFT_TOP, format!("[{y_unit}]"), small_font(), FAINT);
    }
    let plot = Rect::from_min_max(rect.min + Vec2::new(48.0, 28.0), rect.max - Vec2::new(10.0, 20.0));
    let (mut x0, mut x1, mut y0, mut y1) = (f32::MAX, f32::MIN, f32::MAX, f32::MIN);
    for s in series {
        for q in s.pts {
            if q[1].is_finite() {
                x0 = x0.min(q[0]);
                x1 = x1.max(q[0]);
                y0 = y0.min(q[1]);
                y1 = y1.max(q[1]);
            }
        }
    }
    if x0 > x1 {
        p.text(plot.center(), Align2::CENTER_CENTER, "no data", small_font(), FAINT);
        return;
    }
    if let Some(f) = y_floor {
        y0 = y0.min(f);
    }
    if x1 - x0 < 1e-6 {
        x1 = x0 + 1.0;
    }
    if y1 - y0 < 1e-3 * y1.abs().max(1.0) {
        let c = (y0 + y1) * 0.5;
        let h = (c.abs() * 0.1).max(0.5);
        y0 = if y_floor.is_some() && c >= 0.0 { (c - h).max(0.0) } else { c - h };
        y1 = c + h;
    }
    let pad = ((y1 - y0) * 0.08).max(1e-4);
    y0 -= if y_floor.is_some() && y0 >= 0.0 { 0.0 } else { pad };
    y1 += pad;
    let tx = |x: f32| plot.left() + (x - x0) / (x1 - x0) * plot.width();
    let ty = |y: f32| plot.bottom() - (y - y0) / (y1 - y0) * plot.height();
    let yt = nice_ticks(y0, y1, 4);
    let ystep = if yt.len() > 1 { yt[1] - yt[0] } else { 1.0 };
    for v in &yt {
        let y = ty(*v);
        p.hline(plot.x_range(), y, Stroke::new(1.0_f32, Color32::from_rgb(34, 36, 40)));
        p.text(Pos2::new(plot.left() - 5.0, y), Align2::RIGHT_CENTER, tick_label(*v, ystep), small_font(), MUTED);
    }
    let xt = nice_ticks(x0, x1, ((plot.width() / 70.0) as usize).max(2));
    let xstep = if xt.len() > 1 { xt[1] - xt[0] } else { 1.0 };
    for v in &xt {
        let x = tx(*v);
        p.vline(x, plot.y_range(), Stroke::new(1.0_f32, Color32::from_rgb(30, 32, 36)));
        p.text(Pos2::new(x, plot.bottom() + 3.0), Align2::CENTER_TOP, tick_label(*v, xstep), small_font(), MUTED);
    }
    p.rect_stroke(plot, CornerRadius::ZERO, Stroke::new(1.0_f32, BORDER), StrokeKind::Outside);
    p.text(Pos2::new(plot.right(), rect.bottom() - 2.0), Align2::RIGHT_BOTTOM, x_label, small_font(), FAINT);
    let mut lx = rect.right() - 10.0;
    for s in series.iter().rev() {
        let last = s.pts.iter().rev().find(|q| q[1].is_finite()).map(|q| q[1]);
        let txt = match last {
            Some(v) => format!("{}  {}", s.name, tick_label(v, ystep / 10.0)),
            None => s.name.clone(),
        };
        let g = p.layout_no_wrap(txt, small_font(), TEXT);
        lx -= g.size().x;
        p.galley(Pos2::new(lx, rect.top() + 6.0), g, TEXT);
        p.rect_filled(Rect::from_min_size(Pos2::new(lx - 12.0, rect.top() + 11.0), Vec2::new(8.0, 2.0)), CornerRadius::ZERO, s.color);
        lx -= 22.0;
        let mut pts: Vec<Pos2> = Vec::with_capacity(s.pts.len());
        let flush = |pts: &mut Vec<Pos2>| {
            if pts.len() > 1 {
                p.add(EShape::line(std::mem::take(pts), Stroke::new(1.4_f32, s.color)));
            }
            pts.clear();
        };
        for q in s.pts {
            if q[1].is_finite() {
                pts.push(Pos2::new(tx(q[0]), ty(q[1].clamp(y0, y1))));
            } else {
                flush(&mut pts);
            }
        }
        flush(&mut pts);
    }
    // hover cursor with values
    if let Some(h) = resp.hover_pos().filter(|h| plot.contains(*h)) {
        p.vline(h.x, plot.y_range(), Stroke::new(1.0_f32, Color32::from_white_alpha(50)));
        let xv = x0 + (h.x - plot.left()) / plot.width() * (x1 - x0);
        let mut lines = vec![format!("{} {}", tick_label(xv, xstep / 10.0), x_label)];
        for s in series {
            if let Some(q) = s.pts.iter().min_by(|a, b| (a[0] - xv).abs().total_cmp(&(b[0] - xv).abs())) {
                lines.push(format!("{}: {}", s.name, tick_label(q[1], ystep / 100.0)));
            }
        }
        let g = p.layout_no_wrap(lines.join("\n"), small_font(), STRONG);
        let at = Pos2::new((h.x + 10.0).min(plot.right() - g.size().x - 8.0), plot.top() + 6.0);
        p.rect_filled(Rect::from_min_size(at - Vec2::splat(4.0), g.size() + Vec2::splat(8.0)), CornerRadius::same(2), Color32::from_black_alpha(200));
        p.galley(at, g, STRONG);
    }
}

/// Two quantities with different units: left axis for `a`, right axis for `b`, each in its series colour.
fn chart_dual(ui: &mut egui::Ui, size: Vec2, title: &str, x_label: &str, a: (&str, &str, Color32, &[[f32; 2]]), b: (&str, &str, Color32, &[[f32; 2]])) {
    let (rect, resp) = ui.allocate_exact_size(size, Sense::hover());
    let p = ui.painter_at(rect);
    p.rect_filled(rect, CornerRadius::same(2), Color32::from_rgb(20, 21, 24));
    p.rect_stroke(rect, CornerRadius::same(2), Stroke::new(1.0_f32, BORDER), StrokeKind::Inside);
    p.text(rect.min + Vec2::new(8.0, 6.0), Align2::LEFT_TOP, title, head_font(), TEXT);
    let plot = Rect::from_min_max(rect.min + Vec2::new(48.0, 28.0), rect.max - Vec2::new(48.0, 20.0));
    let mut x0 = f32::MAX;
    let mut x1 = f32::MIN;
    let range = |pts: &[[f32; 2]], x0: &mut f32, x1: &mut f32| -> Option<(f32, f32)> {
        let (mut lo, mut hi) = (0.0f32, f32::MIN);
        for q in pts.iter().filter(|q| q[1].is_finite()) {
            *x0 = x0.min(q[0]);
            *x1 = x1.max(q[0]);
            lo = lo.min(q[1]);
            hi = hi.max(q[1]);
        }
        if hi == f32::MIN {
            return None;
        }
        if hi - lo < 1e-3 * hi.abs().max(1.0) {
            hi = lo + hi.abs().max(1.0) * 0.2 + 1e-3;
        }
        Some((lo, hi + (hi - lo) * 0.08))
    };
    let ra = range(a.3, &mut x0, &mut x1);
    let rb = range(b.3, &mut x0, &mut x1);
    if x0 > x1 {
        p.text(plot.center(), Align2::CENTER_CENTER, "no data", small_font(), FAINT);
        return;
    }
    if x1 - x0 < 1e-6 {
        x1 = x0 + 1.0;
    }
    let tx = |x: f32| plot.left() + (x - x0) / (x1 - x0) * plot.width();
    let xt = nice_ticks(x0, x1, ((plot.width() / 70.0) as usize).max(2));
    let xstep = if xt.len() > 1 { xt[1] - xt[0] } else { 1.0 };
    for v in &xt {
        let x = tx(*v);
        p.vline(x, plot.y_range(), Stroke::new(1.0_f32, Color32::from_rgb(30, 32, 36)));
        p.text(Pos2::new(x, plot.bottom() + 3.0), Align2::CENTER_TOP, tick_label(*v, xstep), small_font(), MUTED);
    }
    p.text(Pos2::new(plot.right(), rect.bottom() - 2.0), Align2::RIGHT_BOTTOM, x_label, small_font(), FAINT);
    let mut legend_x = rect.right() - 10.0;
    let mut hover_lines = Vec::new();
    let hx = resp.hover_pos().filter(|h| plot.contains(*h)).map(|h| x0 + (h.x - plot.left()) / plot.width() * (x1 - x0));
    if let Some(xv) = hx {
        hover_lines.push(format!("{} {}", tick_label(xv, xstep / 10.0), x_label));
    }
    for (side, (name, unit, col, pts), r) in [(0, a, ra), (1, b, rb)] {
        let Some((y0, y1)) = r else { continue };
        let ty = |y: f32| plot.bottom() - (y - y0) / (y1 - y0) * plot.height();
        let yt = nice_ticks(y0, y1, 4);
        let ystep = if yt.len() > 1 { yt[1] - yt[0] } else { 1.0 };
        let dim = col.gamma_multiply(0.85);
        for v in &yt {
            let y = ty(*v);
            if side == 0 {
                p.hline(plot.x_range(), y, Stroke::new(1.0_f32, Color32::from_rgb(34, 36, 40)));
                p.text(Pos2::new(plot.left() - 5.0, y), Align2::RIGHT_CENTER, tick_label(*v, ystep), small_font(), dim);
            } else {
                p.text(Pos2::new(plot.right() + 5.0, y), Align2::LEFT_CENTER, tick_label(*v, ystep), small_font(), dim);
            }
        }
        let pts_s: Vec<Pos2> = pts.iter().filter(|q| q[1].is_finite()).map(|q| Pos2::new(tx(q[0]), ty(q[1].clamp(y0, y1)))).collect();
        if pts_s.len() > 1 {
            p.add(EShape::line(pts_s, Stroke::new(1.4_f32, col)));
        }
        let last = pts.iter().rev().find(|q| q[1].is_finite()).map(|q| q[1]);
        let txt = match last {
            Some(v) => format!("{name}  {} {unit}", tick_label(v, ystep / 10.0)),
            None => name.to_string(),
        };
        let g = p.layout_no_wrap(txt, small_font(), TEXT);
        legend_x -= g.size().x;
        p.galley(Pos2::new(legend_x, rect.top() + 6.0), g, TEXT);
        p.rect_filled(Rect::from_min_size(Pos2::new(legend_x - 12.0, rect.top() + 11.0), Vec2::new(8.0, 2.0)), CornerRadius::ZERO, col);
        legend_x -= 22.0;
        if let Some(xv) = hx {
            if let Some(q) = pts.iter().min_by(|a, b| (a[0] - xv).abs().total_cmp(&(b[0] - xv).abs())) {
                hover_lines.push(format!("{name}: {} {unit}", tick_label(q[1], ystep / 100.0)));
            }
        }
    }
    p.rect_stroke(plot, CornerRadius::ZERO, Stroke::new(1.0_f32, BORDER), StrokeKind::Outside);
    if let Some(h) = resp.hover_pos().filter(|h| plot.contains(*h)) {
        p.vline(h.x, plot.y_range(), Stroke::new(1.0_f32, Color32::from_white_alpha(50)));
        let g = p.layout_no_wrap(hover_lines.join("\n"), small_font(), STRONG);
        let at = Pos2::new((h.x + 10.0).min(plot.right() - g.size().x - 8.0), plot.top() + 6.0);
        p.rect_filled(Rect::from_min_size(at - Vec2::splat(4.0), g.size() + Vec2::splat(8.0)), CornerRadius::same(2), Color32::from_black_alpha(200));
        p.galley(at, g, STRONG);
    }
}

/// `campaign_bridge::chapter_of` re-parses the campaign JSON on every call (it was ~35 ms per frame with
/// the experiments list open); the chapter table is cached here once.
fn chapter_of_cached(idx: usize) -> Option<(u32, String)> {
    static CH: std::sync::OnceLock<Vec<(u32, String)>> = std::sync::OnceLock::new();
    let chs = CH.get_or_init(|| crate::campaign::chapters().into_iter().map(|c| (c.id, c.title)).collect());
    let m = crate::campaign_bridge::def_of(idx)?;
    chs.iter().find(|c| c.0 == m.chapter).cloned()
}

/// Typical physical properties of the wall materials (k W/(m·K), cp J/(kg·K), yield / strength MPa),
/// in the order of `elements::MATS`.
const MAT_SI: [(f32, f32, f32); 10] = [
    (45.0, 490.0, 250.0),  // steel (mild)
    (205.0, 900.0, 276.0), // aluminium 6061-T6
    (400.0, 385.0, 70.0),  // copper (annealed)
    (6.7, 526.0, 880.0),   // Ti-6Al-4V
    (11.4, 435.0, 1030.0), // Inconel 718
    (30.0, 880.0, 300.0),  // alumina (flexural)
    (320.0, 390.0, 280.0), // CuCrZr cooled liner
    (0.12, 1700.0, 40.0),  // softwood (compressive)
    (0.4, 1250.0, 1.0),    // AP/HTPB propellant (tensile)
    (0.25, 1100.0, 10.0),  // cast explosive
];

/// Key/value row whose value is coloured by a fraction of a limit: amber above 80 %, red above 100 %.
fn kv_limit(ui: &mut egui::Ui, k: &str, frac: f32, v: String, what: &str) {
    ui.label(muted(k));
    let (c, mark) = if frac >= 1.0 { (BAD, "⚠ ") } else if frac >= 0.8 { (WARN, "") } else { (TEXT, "") };
    let r = ui.label(num(format!("{mark}{v}")).color(c));
    if frac >= 0.8 {
        r.on_hover_text(format!("{:.0} % of {what}. Amber above 80 %, red above 100 %.", frac * 100.0));
    }
    ui.end_row();
}

// =====================================================================================================
// the workspace
// =====================================================================================================

impl App {
    fn mission_active(&self) -> Option<&'static Mission> {
        self.run.as_ref().map(|r| r.m())
    }

    fn feeds_locked(&self) -> bool {
        self.mission_active().map(|m| !m.feeds.adjustable).unwrap_or(false)
    }

    fn load_template(&mut self, t: &Template) {
        self.start_sandbox_named(t.title, (t.build)());
    }

    // ------------------------------------------------------------------------------------- menu bar
    pub(super) fn menu_bar(&mut self, ctx: &egui::Context, frame: &eframe::Frame) {
        egui::TopBottomPanel::top("menu").exact_height(24.0).frame(egui::Frame::NONE.fill(BAR).inner_margin(egui::Margin::symmetric(6, 0))).show(ctx, |ui| {
            egui::menu::bar(ui, |ui| {
                ui.spacing_mut().button_padding = Vec2::new(7.0, 2.0);
                ui.label(RichText::new("Ignis").font(head_font()).color(STRONG));
                ui.add_space(6.0);
                ui.menu_button("File", |ui| self.menu_file(ui, frame));
                ui.menu_button("Edit", |ui| self.menu_edit(ui, frame));
                ui.menu_button("View", |ui| self.menu_view(ui));
                ui.menu_button("Simulation", |ui| self.menu_sim(ui, frame));
                ui.menu_button("Tools", |ui| self.menu_tools(ui));
                ui.menu_button("Window", |ui| self.menu_window(ui));
                ui.menu_button("Help", |ui| self.menu_help(ui));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let kind = if self.run.is_some() { "Experiment" } else { "Setup" };
                    ui.label(RichText::new(&self.scene_name).color(TEXT));
                    ui.label(muted(format!("{kind}:")));
                });
            });
        });
    }

    fn menu_file(&mut self, ui: &mut egui::Ui, frame: &eframe::Frame) {
        ui.set_min_width(220.0);
        if ui.button("New blank setup").clicked() {
            self.start_sandbox_named("Blank canvas", Scene::empty());
            ui.close_menu();
        }
        ui.menu_button("New from template", |ui| {
            ui.set_min_width(260.0);
            let ts = templates();
            for g in ["Propulsion", "Flames & fires", "Structures & blast", "Chemistry", "Materials & heat transfer", "General"] {
                let items: Vec<&Template> = ts.iter().filter(|t| t.group == g).collect();
                if items.is_empty() {
                    continue;
                }
                ui.menu_button(g, |ui| {
                    ui.set_min_width(280.0);
                    for t in items {
                        if ui.button(t.title).on_hover_text(t.desc).clicked() {
                            self.load_template(t);
                            ui.close_menu();
                        }
                    }
                });
            }
        });
        ui.separator();
        if ui.add_enabled(self.run.is_none(), egui::Button::new("Open design file...").shortcut_text("⌘O")).clicked() {
            ui.close_menu();
            self.open();
        }
        if ui.button("Open from library...").clicked() {
            self.library.allow_save = self.run.is_none();
            self.show_library = true;
            ui.close_menu();
        }
        ui.separator();
        if ui.add_enabled(self.run.is_none(), egui::Button::new("Save as design file...").shortcut_text("⌘S")).clicked() {
            ui.close_menu();
            self.save(frame);
        }
        if ui.add_enabled(self.run.is_none(), egui::Button::new("Save to library...")).clicked() {
            self.library.allow_save = true;
            self.library.begin_save(&self.scene_name.clone());
            self.show_library = true;
            ui.close_menu();
        }
        ui.separator();
        if ui.button("Export probe data (CSV)...").clicked() {
            ui.close_menu();
            self.export_probes_csv();
        }
        if ui.button("Export viewport image (PNG)...").clicked() {
            ui.close_menu();
            if let Some(p) = rfd::FileDialog::new().add_filter("PNG image", &["png"]).set_file_name("ignis_view.png").save_file() {
                ui.ctx().data_mut(|d| d.insert_temp(egui::Id::new("export_png"), p.to_string_lossy().to_string()));
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::Screenshot(Default::default()));
            }
        }
        ui.separator();
        if ui.add(egui::Button::new("Quit Ignis").shortcut_text("⌘Q")).clicked() {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }

    fn menu_edit(&mut self, ui: &mut egui::Ui, frame: &eframe::Frame) {
        ui.set_min_width(220.0);
        let edit = self.can_edit_layout();
        if ui.add_enabled(edit && !self.undo.is_empty(), egui::Button::new("Undo").shortcut_text("⌘Z")).clicked() {
            self.do_undo(frame);
            ui.close_menu();
        }
        if ui.add_enabled(edit && !self.redo.is_empty(), egui::Button::new("Redo").shortcut_text("⇧⌘Z")).clicked() {
            self.do_redo(frame);
            ui.close_menu();
        }
        ui.separator();
        if ui.add(egui::Button::new("Pick material from viewport").shortcut_text("P")).clicked() {
            self.picker = true;
            ui.close_menu();
        }
        if ui.add_enabled(self.run.is_none(), egui::Button::new("Clear all geometry")).clicked() {
            self.snapshot(frame);
            self.pending_meta = Some(vec![0; (W * H) as usize]);
            self.reset_gas = true;
            self.log_info("Geometry cleared");
            ui.close_menu();
        }
        ui.separator();
        if ui.add(egui::Button::new("Preferences...").shortcut_text("⌘,")).clicked() {
            self.show_settings = true;
            ui.close_menu();
        }
    }

    fn menu_view(&mut self, ui: &mut egui::Ui) {
        ui.set_min_width(230.0);
        for (k, (n, d)) in VIEW_NAMES.iter().enumerate() {
            let key = format!("{}", k + 1);
            if ui.add(egui::Button::new(*n).selected(self.view == k as u32).shortcut_text(key)).on_hover_text(*d).clicked() {
                self.set_view(k as u32);
                ui.close_menu();
            }
        }
        ui.separator();
        ui.checkbox(&mut self.show_grid, "Grid lines");
        ui.checkbox(&mut self.show_legend, "Colour-map legend");
        ui.separator();
        if ui.add(egui::Button::new("Zoom to fit").shortcut_text("0")).clicked() {
            self.zoom = 1.0;
            ui.close_menu();
        }
        if ui.button("Zoom in").clicked() {
            self.zoom = (self.zoom * 1.5).min(12.0);
        }
        if ui.button("Zoom out").clicked() {
            self.zoom = (self.zoom / 1.5).max(1.0);
        }
        ui.separator();
        ui.menu_button("Units", |ui| self.units_menu(ui));
    }

    fn units_menu(&mut self, ui: &mut egui::Ui) {
        ui.set_min_width(200.0);
        let before = self.units;
        if ui.button("SI (K, kPa, m/s, cm)").clicked() {
            self.units = Units::default();
        }
        if ui.button("Engineering (°C, bar, m/s, mm)").clicked() {
            self.units = Units::engineering();
        }
        ui.separator();
        ui.label(faint("TEMPERATURE"));
        ui.horizontal(|ui| {
            ui.radio_value(&mut self.units.t, TempU::K, "K");
            ui.radio_value(&mut self.units.t, TempU::C, "°C");
            ui.radio_value(&mut self.units.t, TempU::F, "°F");
        });
        ui.label(faint("PRESSURE"));
        ui.horizontal(|ui| {
            ui.radio_value(&mut self.units.p, PressU::KPa, "kPa");
            ui.radio_value(&mut self.units.p, PressU::Bar, "bar");
            ui.radio_value(&mut self.units.p, PressU::Atm, "atm");
            ui.radio_value(&mut self.units.p, PressU::Psi, "psi");
        });
        ui.label(faint("VELOCITY"));
        ui.horizontal(|ui| {
            ui.radio_value(&mut self.units.v, VelU::Ms, "m/s");
            ui.radio_value(&mut self.units.v, VelU::Kmh, "km/h");
            ui.radio_value(&mut self.units.v, VelU::Fts, "ft/s");
        });
        ui.label(faint("LENGTH"));
        ui.horizontal(|ui| {
            ui.radio_value(&mut self.units.l, LenU::Mm, "mm");
            ui.radio_value(&mut self.units.l, LenU::Cm, "cm");
            ui.radio_value(&mut self.units.l, LenU::M, "m");
        });
        if self.units != before {
            self.units.save();
        }
    }

    fn menu_sim(&mut self, ui: &mut egui::Ui, frame: &eframe::Frame) {
        ui.set_min_width(230.0);
        if ui.add(egui::Button::new(if self.paused { "Run" } else { "Pause" }).shortcut_text("Space")).clicked() {
            self.paused = !self.paused;
            ui.close_menu();
        }
        if ui.add(egui::Button::new("Advance one frame").shortcut_text(".")).clicked() {
            self.paused = true;
            self.step_once = true;
        }
        ui.menu_button("Time scale", |ui| {
            for s in [0.1f32, 0.25, 0.5, 1.0, 2.0, 4.0] {
                if ui.add(egui::Button::new(format!("{s}×")).selected((self.speed - s).abs() < 1e-3)).clicked() {
                    self.speed = s;
                    ui.close_menu();
                }
            }
        });
        ui.separator();
        if ui.button("Reset flow field (keep geometry)").clicked() {
            self.reset_gas = true;
            self.sim_time = 0.0;
            self.hist.clear();
            self.probes.reset_series();
            self.diag = probes::Diagnostics::default();
            self.log_info("Flow field reset to ambient (1 atm, 300 K)");
            ui.close_menu();
        }
        if ui.button("Reload setup").clicked() {
            ui.close_menu();
            self.reload_setup();
        }
        ui.separator();
        ui.add_enabled_ui(self.run.is_none(), |ui| {
            ui.checkbox(&mut self.ign_on, "Igniters energised");
            ui.checkbox(&mut self.gravity_on, "Gravity (9.81 m/s²)");
        });
        if let Some(r) = &self.run {
            ui.separator();
            match r.phase {
                Phase::Build => {
                    if ui.add(egui::Button::new("Start experiment run").shortcut_text("Enter")).clicked() {
                        self.begin_test(frame);
                        ui.close_menu();
                    }
                }
                _ => {
                    if ui.button("Return to setup").clicked() {
                        self.back_to_build();
                        ui.close_menu();
                    }
                }
            }
        }
    }

    fn reload_setup(&mut self) {
        if let Some(r) = &self.run {
            let idx = r.idx;
            self.start_mission(idx, false);
            return;
        }
        let name = self.scene_name.clone();
        if let Some(t) = templates().into_iter().find(|t| t.title == name) {
            self.load_template(&t);
        } else {
            self.reset_gas = true;
            self.sim_time = 0.0;
        }
    }

    fn menu_tools(&mut self, ui: &mut egui::Ui) {
        ui.set_min_width(230.0);
        for (s, n, k, tip) in SHAPES.iter() {
            if ui.add(egui::Button::new(*n).selected(self.shape == *s).shortcut_text(*k)).on_hover_text(*tip).clicked() {
                self.shape = *s;
                self.place = None;
                ui.close_menu();
            }
        }
        if ui.add(egui::Button::new("Eraser").selected(self.tool == Tool::Erase).shortcut_text("E")).clicked() {
            self.pick(Tool::Erase);
            ui.close_menu();
        }
        if ui
            .add(egui::Button::new("Mix / stir").selected(self.tool == Tool::Gas(8)).shortcut_text("M"))
            .on_hover_text("Stir powders and liquids under the brush into an intimate mixture (needed for black powder, rocket candy, thermite...)")
            .clicked()
        {
            self.pick(Tool::Gas(8));
            ui.close_menu();
        }
        if ui
            .add(egui::Button::new("Cast / press").selected(self.tool == Tool::Gas(super::prop::OP_CAST)).shortcut_text("C"))
            .on_hover_text("Pack powders under the brush into a solid grain that burns only at its surface (steady propellant burn instead of a flash)")
            .clicked()
        {
            self.pick(Tool::Gas(super::prop::OP_CAST));
            ui.close_menu();
        }
        if ui.add(egui::Button::new("Propellant designer...").selected(self.show_designer).shortcut_text("D")).clicked() {
            self.show_designer = !self.show_designer;
            ui.close_menu();
        }
        ui.separator();
        if ui.button("Place point probe").clicked() {
            self.place = Some(Place::Probe);
            ui.close_menu();
        }
        if ui.button("Place line sampler").clicked() {
            self.place = Some(Place::Line(None));
            ui.close_menu();
        }
        if ui.add_enabled(!self.probes.is_empty(), egui::Button::new("Remove all probes")).clicked() {
            self.probes.probes.clear();
            self.probes.lines.clear();
            ui.close_menu();
        }
        ui.separator();
        if ui.button("Zero load cell (thrust tare)").clicked() {
            ui.close_menu();
            let raw = self.diag_thrust_raw();
            self.tare = raw;
            self.log_info("Thrust load cell zeroed");
        }
    }

    fn diag_thrust_raw(&self) -> Vec2 {
        self.hist_raw_thrust
    }

    fn menu_window(&mut self, ui: &mut egui::Ui) {
        ui.set_min_width(220.0);
        ui.checkbox(&mut self.dock_left, "Materials library");
        ui.checkbox(&mut self.dock_right, "Properties / Inspector");
        ui.checkbox(&mut self.dock_bottom, "Bottom dock");
        ui.separator();
        for (t, n) in BottomTab::ALL {
            if ui.button(n).clicked() {
                self.dock_bottom = true;
                self.bottom_tab = t;
                ui.close_menu();
            }
        }
        ui.separator();
        if ui.button("Experiments...").clicked() {
            self.show_experiments = true;
            ui.close_menu();
        }
        if ui.button("Design library...").clicked() {
            self.show_library = true;
            ui.close_menu();
        }
        if ui.button("Propellant designer").clicked() {
            self.show_designer = true;
            ui.close_menu();
        }
        if ui.button("Reference manual...").clicked() {
            self.show_codex = true;
            ui.close_menu();
        }
    }

    fn menu_help(&mut self, ui: &mut egui::Ui) {
        ui.set_min_width(240.0);
        ui.menu_button("Guided tour", |ui| {
            ui.set_min_width(260.0);
            let names: Vec<(usize, &str, &str)> = self.tutorial.lessons().iter().enumerate().map(|(k, l)| (k, l.name, l.blurb)).collect();
            for (k, n, b) in names {
                let done = if self.tutorial.is_lesson_complete(k) { "  (completed)" } else { "" };
                if ui.button(format!("{}. {n}{done}", k + 1)).on_hover_text(b).clicked() {
                    self.start_lesson(k);
                    ui.close_menu();
                }
            }
        });
        if ui.add(egui::Button::new("Keyboard shortcuts").shortcut_text("H")).clicked() {
            self.show_shortcuts = true;
            ui.close_menu();
        }
        if ui.button("Reference manual").clicked() {
            self.show_codex = true;
            ui.close_menu();
        }
        ui.separator();
        if ui.button("About Ignis").clicked() {
            self.show_about = true;
            ui.close_menu();
        }
    }

    fn start_lesson(&mut self, k: usize) {
        if let Some(sc) = self.tutorial.lessons().get(k).and_then(|l| l.scene) {
            let sc = sc.to_lowercase();
            if let Some(t) = templates().into_iter().find(|t| t.title.to_lowercase().starts_with(&sc)) {
                self.load_template(&t);
            }
        }
        self.tutorial.start(k);
    }

    // ------------------------------------------------------------------------------------- toolbar
    pub(super) fn toolbar(&mut self, ctx: &egui::Context, frame: &eframe::Frame) {
        egui::TopBottomPanel::top("toolbar").exact_height(32.0).frame(egui::Frame::NONE.fill(PANEL).inner_margin(egui::Margin::symmetric(8, 4)).stroke(Stroke::new(1.0_f32, BORDER))).show(ctx, |ui| {
            ui.horizontal_centered(|ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                if icon_button(ui, Icon::New, false, true, "New blank setup").clicked() {
                    self.start_sandbox_named("Blank canvas", Scene::empty());
                }
                if icon_button(ui, Icon::Open, false, self.run.is_none(), "Open design (⌘O)").clicked() {
                    self.open();
                }
                if icon_button(ui, Icon::Save, false, self.run.is_none(), "Save design (⌘S)").clicked() {
                    self.save(frame);
                }
                vsep(ui);
                let edit = self.can_edit_layout();
                if icon_button(ui, Icon::Undo, false, edit && !self.undo.is_empty(), "Undo (⌘Z)").clicked() {
                    self.do_undo(frame);
                }
                if icon_button(ui, Icon::Redo, false, edit && !self.redo.is_empty(), "Redo (⇧⌘Z)").clicked() {
                    self.do_redo(frame);
                }
                vsep(ui);
                let r = icon_button(ui, if self.paused { Icon::Play } else { Icon::Pause }, false, true, if self.paused { "Run (Space)" } else { "Pause (Space)" });
                if r.clicked() {
                    self.paused = !self.paused;
                }
                self.tut_rects.insert("play".into(), r.rect);
                if icon_button(ui, Icon::Step, false, true, "Advance one frame (.)").clicked() {
                    self.paused = true;
                    self.step_once = true;
                }
                if icon_button(ui, Icon::Reset, false, true, "Reload setup").clicked() {
                    self.reload_setup();
                }
                ui.add_space(4.0);
                let sp = egui::ComboBox::from_id_salt("speed").width(58.0).selected_text(format!("{}×", self.speed)).show_ui(ui, |ui| {
                    for s in [0.1f32, 0.25, 0.5, 1.0, 2.0, 4.0] {
                        ui.selectable_value(&mut self.speed, s, format!("{s}×"));
                    }
                });
                sp.response.clone().on_hover_text("Time scale (1× = 32 solver time units per second)");
                self.tut_rects.insert("speed".into(), sp.response.rect);
                vsep(ui);
                let edit_ok = self.can_edit_layout() || self.mission_active().map(|m| m.live).unwrap_or(false);
                for (s, icon) in [(Shape::Brush, Icon::Brush), (Shape::Line, Icon::Line), (Shape::Box, Icon::Box), (Shape::Frame, Icon::Frame), (Shape::Fill, Icon::Fill), (Shape::Spray, Icon::Spray)] {
                    let (_, n, k, tip) = SHAPES.iter().find(|x| x.0 == s).copied().unwrap();
                    if icon_button(ui, icon, self.shape == s && self.place.is_none() && !self.picker, edit_ok, &format!("{n} ({k}): {tip}")).clicked() {
                        self.shape = s;
                        self.place = None;
                        self.picker = false;
                    }
                }
                if icon_button(ui, Icon::Erase, self.tool == Tool::Erase, edit_ok, "Eraser (E); right-drag also erases").clicked() {
                    self.pick(Tool::Erase);
                }
                if icon_button(ui, Icon::Mix, self.tool == Tool::Gas(8), edit_ok, "Mix / stir (M): stir powders and liquids under the brush into an intimate blend. Reactions between grains need contact, so a stirred mixture burns far faster than piles side by side").clicked() {
                    self.pick(Tool::Gas(8));
                }
                if icon_button(ui, Icon::Cast, self.tool == Tool::Gas(super::prop::OP_CAST), edit_ok, "Cast / press (C): pack the powder under the brush into a solid grain that burns only at its exposed surface").clicked() {
                    self.pick(Tool::Gas(super::prop::OP_CAST));
                }
                if icon_button(ui, Icon::Pick, self.picker, true, "Pick material from viewport (P or Alt-click)").clicked() {
                    self.picker = !self.picker;
                    self.place = None;
                }
                ui.add_space(6.0);
                ui.label(muted("Size"));
                ui.add(egui::DragValue::new(&mut self.brush).range(0..=24).speed(0.1).custom_formatter(|v, _| format!("{} cm", 2 * v as i32 + 1))).on_hover_text("Brush diameter ([ and ])");
                vsep(ui);
                if icon_button(ui, Icon::Probe, self.place == Some(Place::Probe), true, "Place a point probe: click a cell").clicked() {
                    self.place = if self.place == Some(Place::Probe) { None } else { Some(Place::Probe) };
                }
                if icon_button(ui, Icon::Sampler, matches!(self.place, Some(Place::Line(_))), true, "Place a line sampler: click start and end").clicked() {
                    self.place = if matches!(self.place, Some(Place::Line(_))) { None } else { Some(Place::Line(None)) };
                }
                if icon_button(ui, Icon::Grid, self.show_grid, true, "Grid lines").clicked() {
                    self.show_grid = !self.show_grid;
                }
                vsep(ui);
                // run-control for experiments
                if let Some(r) = &self.run {
                    match r.phase {
                        Phase::Build => {
                            if ui.add(egui::Button::new(RichText::new("Start run").color(STRONG)).fill(ACCENT_DIM).stroke(Stroke::new(1.0_f32, ACCENT))).on_hover_text("Lock the geometry and run the experiment (Enter)").clicked() {
                                self.begin_test(frame);
                            }
                        }
                        Phase::Test => {
                            ui.label(RichText::new(format!("Run in progress  t = {:.1} s", r.t)).color(TEXT));
                            if !r.m().live && ui.button("Abort").clicked() {
                                self.back_to_build();
                            }
                        }
                        Phase::Done => {
                            let (t, c) = if r.passed { ("PASS", GOOD) } else { ("FAIL", BAD) };
                            ui.label(RichText::new(t).font(head_font()).color(c));
                            if ui.button("Return to setup").clicked() {
                                self.back_to_build();
                            }
                        }
                    }
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    // current material
                    if ui.add(egui::Button::new("Propellant designer").selected(self.show_designer)).on_hover_text("Formulate a propellant, predict its burn rate and flame temperature, paint a cast grain (D)").clicked() {
                        self.show_designer = !self.show_designer;
                    }
                    vsep(ui);
                    let comp = self.comp.as_ref().filter(|b| self.tool == Tool::Matter(b.base())).map(|b| (b.name.clone(), b.cast));
                    if let Some((n, cast)) = comp {
                        ui.label(RichText::new(format!("{n}{}", if cast { "  (cast)" } else { "" })).color(STRONG));
                        ui.label(muted("Composition"));
                    } else if self.tool == Tool::Gas(super::prop::OP_CAST) {
                        ui.label(RichText::new("Cast / press").color(STRONG));
                        ui.label(muted("Tool"));
                    } else if let Some(e) = elements::find(self.tool) {
                        ui.label(RichText::new(e.name).color(STRONG));
                        swatch(ui, e.color, 12.0);
                        ui.label(muted("Material"));
                    }
                });
            });
        });
    }

    // ------------------------------------------------------------------------------------- status bar
    pub(super) fn status_bar(&mut self, ctx: &egui::Context, stats: &Stats) {
        let now = ctx.input(|i| i.time);
        egui::TopBottomPanel::bottom("status").exact_height(22.0).frame(egui::Frame::NONE.fill(BAR).inner_margin(egui::Margin::symmetric(8, 0)).stroke(Stroke::new(1.0_f32, BORDER))).show(ctx, |ui| {
            ui.horizontal_centered(|ui| {
                ui.spacing_mut().item_spacing.x = 14.0;
                let (state, c) = if self.paused {
                    ("Paused", WARN)
                } else if self.run.as_ref().map(|r| r.phase == Phase::Test).unwrap_or(false) {
                    ("Running (experiment)", GOOD)
                } else {
                    ("Running", GOOD)
                };
                let (r, _) = ui.allocate_exact_size(Vec2::new(8.0, 8.0), Sense::hover());
                ui.painter().circle_filled(r.center(), 3.5, c);
                ui.add_space(-10.0);
                ui.label(RichText::new(state).color(TEXT).size(11.0));
                let st = |ui: &mut egui::Ui, k: &str, v: String, tip: &str| {
                    ui.label(RichText::new(format!("{k} ")).color(FAINT).size(11.0)).on_hover_text(tip);
                    ui.add_space(-14.0);
                    ui.label(RichText::new(v).monospace().color(TEXT).size(11.0)).on_hover_text(tip);
                };
                st(ui, "t", format!("{:.3} ms", self.sim_time * MS_PER_UNIT), "Simulated physical time since the setup was loaded");
                let rate = self.steps_last as f32 / (self.frame_ms / 1000.0).max(1e-3);
                st(ui, "steps", format!("{} /frame  {:.0} /s", self.steps_last, rate), "Solver steps per displayed frame and per wall-clock second");
                st(ui, "Δt", format!("{:.3} µs", DT * MS_PER_UNIT * 1000.0), "Fixed solver time step (1 cell = 1 cm)");
                let cfl_c = if self.diag.cfl > 0.9 { BAD } else if self.diag.cfl > 0.6 { WARN } else { TEXT };
                ui.label(RichText::new("CFL ").color(FAINT).size(11.0));
                ui.add_space(-14.0);
                ui.label(RichText::new(if self.diag.valid { format!("{:.2}", self.diag.cfl) } else { "—".into() }).monospace().color(cfl_c).size(11.0)).on_hover_text("Courant number max(|u| + c)·Δt/Δx over the domain (stable below ~0.9)");
                let rt = if self.paused { 0.0 } else { TPS * self.speed * self.rt_factor * MS_PER_UNIT / 10.0 };
                st(ui, "real-time", format!("{rt:.3} %"), "Simulated time per wall-clock time (combustion runs in microseconds: ~0.1 % at 1×)");
                if self.diag.valid {
                    st(ui, "Δm", format!("{:+.3} %", self.diag.mass_drift()), "Total gas mass relative to the start (open boundaries, inlets and reactions change it physically)");
                    st(ui, "ΔE", format!("{:+.3} %", self.diag.energy_drift()), "Total gas energy relative to the start (includes heat release and losses)");
                }
                let gpu = self.steps_last as f32 * 0.46 + 0.7;
                st(ui, "GPU", format!("≈{gpu:.1} ms"), "Estimated GPU time per frame (0.46 ms per solver step + 0.7 ms render, M2 Pro baseline)");
                st(ui, "frame", format!("{:.1} ms", self.frame_ms), "Wall-clock frame time");
                let _ = stats;
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if let Some((msg, t0, lvl)) = &mut self.status_msg {
                        if *t0 == 0.0 {
                            *t0 = now;
                        }
                        let age = now - *t0;
                        if age > 6.0 {
                            self.status_msg = None;
                        } else {
                            let c = if *lvl == Level::Warn { WARN } else { MUTED };
                            ui.label(RichText::new(msg.as_str()).color(c).size(11.0));
                        }
                    }
                });
            });
        });
    }

    // ------------------------------------------------------------------------------------- left dock: materials library
    pub(super) fn library_dock(&mut self, ctx: &egui::Context) {
        let resp = egui::SidePanel::left("materials").resizable(true).default_width(272.0).width_range(220.0..=440.0).frame(panel_frame().stroke(Stroke::new(1.0_f32, BORDER))).show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("MATERIALS LIBRARY").font(head_font()).color(MUTED));
            });
            ui.add_space(4.0);
            let r = ui.add(egui::TextEdit::singleline(&mut self.search).id(egui::Id::new("search")).hint_text("Search name, code, property...").desired_width(f32::INFINITY));
            if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                let q = self.search.to_lowercase();
                if let Some(e) = elements::all().find(|e| e.name.to_lowercase().contains(&q) || e.code.to_lowercase() == q) {
                    if self.allowed(e.tool) {
                        self.pick(e.tool);
                    }
                }
            }
            ui.add_space(4.0);
            // column header
            let w = ui.available_width();
            let (hr, hresp) = ui.allocate_exact_size(Vec2::new(w, 16.0), Sense::hover());
            hresp.on_hover_text("ρ: density.  T*: characteristic transition temperature (melting point for solids, ignition / decomposition onset for reactive materials).");
            let p = ui.painter();
            p.text(hr.left_center() + Vec2::new(18.0, 0.0), Align2::LEFT_CENTER, "Name", small_font(), FAINT);
            p.text(Pos2::new(hr.right() - 56.0, hr.center().y), Align2::RIGHT_CENTER, "ρ kg/m³", small_font(), FAINT);
            p.text(Pos2::new(hr.right() - 2.0, hr.center().y), Align2::RIGHT_CENTER, format!("T* {}", self.units.t_unit()), small_font(), FAINT);
            p.hline(hr.x_range(), hr.bottom(), Stroke::new(1.0_f32, BORDER));
            egui::ScrollArea::vertical().auto_shrink([false, false]).id_salt("libscroll").show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                let q = self.search.trim().to_lowercase();
                let hit = |e: &Element| q.is_empty() || e.name.to_lowercase().contains(&q) || e.code.to_lowercase().contains(&q) || e.tags.to_lowercase().contains(&q) || e.blurb.to_lowercase().contains(&q);
                if let Some(m) = self.mission_active() {
                    let items: Vec<&'static Element> = m.palette.iter().filter_map(|t| elements::find(*t)).filter(|e| hit(e)).collect();
                    self.lib_group(ui, "Experiment materials", "exp", &items, true);
                }
                if !self.favs.is_empty() {
                    let items: Vec<&'static Element> = self.favs.iter().filter_map(|t| elements::find(*t)).filter(|e| hit(e)).collect();
                    self.lib_group(ui, "Favourites", "fav", &items, self.run.is_none());
                }
                self.compositions_group(ui, &q);
                let cur_cat = elements::find(self.tool).map(|e| e.cat);
                for (cat, name, _) in CATS.iter() {
                    let items: Vec<&'static Element> = elements::all().filter(|e| e.cat == *cat && hit(e)).collect();
                    let open = self.run.is_none() && (cur_cat == Some(*cat) || *cat == Cat::Structure);
                    self.lib_group(ui, name, name, &items, open);
                }
            });
        });
        self.tut_rects.insert("palette".into(), resp.response.rect);
    }

    fn lib_group(&mut self, ui: &mut egui::Ui, title: &str, id: &str, items: &[&'static Element], default_open: bool) {
        if items.is_empty() {
            return;
        }
        let searching = !self.search.trim().is_empty();
        let mut job = egui::text::LayoutJob::default();
        job.append(title, 0.0, egui::TextFormat { font_id: head_font(), color: TEXT, ..Default::default() });
        job.append(&format!("   {}", items.len()), 0.0, egui::TextFormat { font_id: small_font(), color: FAINT, ..Default::default() });
        let hdr = egui::CollapsingHeader::new(job)
            .id_salt(("libcat", id))
            .default_open(default_open)
            .open(if searching { Some(true) } else { None })
            .show(ui, |ui| {
                for e in items {
                    self.lib_row(ui, e);
                }
                ui.add_space(4.0);
            });
        let _ = hdr;
    }

    fn lib_row(&mut self, ui: &mut egui::Ui, e: &'static Element) {
        let w = ui.available_width();
        let (rect, resp) = ui.allocate_exact_size(Vec2::new(w, 20.0), Sense::click());
        let allowed = self.allowed(e.tool);
        let sel = self.tool == e.tool;
        let p = ui.painter();
        if sel {
            p.rect_filled(rect, CornerRadius::same(2), ACCENT_DIM);
            p.rect_filled(Rect::from_min_size(rect.min, Vec2::new(2.0, rect.height())), CornerRadius::ZERO, ACCENT);
        } else if resp.hovered() {
            p.rect_filled(rect, CornerRadius::same(2), HOVER);
        }
        let sw = Rect::from_center_size(Pos2::new(rect.left() + 10.0, rect.center().y), Vec2::splat(9.0));
        p.rect_filled(sw, CornerRadius::same(1), if allowed { e.color } else { e.color.gamma_multiply(0.3) });
        let tc = if !allowed { FAINT } else if sel { STRONG } else { TEXT };
        let name = p.layout_no_wrap(e.name.to_string(), FontId::proportional(12.0), tc);
        let nw = name.size().x;
        p.galley(Pos2::new(rect.left() + 20.0, rect.center().y - name.size().y / 2.0), name, tc);
        if rect.width() > 250.0 {
            p.text(Pos2::new(rect.left() + 26.0 + nw, rect.center().y + 0.5), Align2::LEFT_CENTER, e.code, FontId::monospace(9.5), FAINT);
        }
        let (rho, tk) = props(e.tool);
        let col = if allowed { MUTED } else { FAINT };
        if let Some(r) = rho {
            let s = if r < 10.0 { format!("{r:.2}") } else { format!("{r:.0}") };
            p.text(Pos2::new(rect.right() - 56.0, rect.center().y), Align2::RIGHT_CENTER, s, FontId::monospace(10.5), col);
        }
        if let Some((t, _)) = tk {
            p.text(Pos2::new(rect.right() - 2.0, rect.center().y), Align2::RIGHT_CENTER, format!("{:.0}", self.units.t(t)), FontId::monospace(10.5), col);
        }
        self.tut_rects.insert(format!("palette:{}", e.code), rect);
        if resp.clicked() && allowed {
            self.pick(e.tool);
            self.place = None;
            self.picker = false;
        }
        let tip = {
            let tk = tk.map(|(t, k)| format!("\n{}: {}", k, self.units.ft(t))).unwrap_or_default();
            format!("{} ({})\n{}{}", e.name, e.code, e.blurb, tk)
        };
        let resp = resp.on_hover_text(tip);
        resp.context_menu(|ui| {
            let fav = self.favs.contains(&e.tool);
            if ui.button(if fav { "Remove from favourites" } else { "Add to favourites" }).clicked() {
                self.toggle_fav(e.tool);
                ui.close_menu();
            }
            if ui.button("Open reference entry").clicked() {
                self.open_reference(e.tool);
                ui.close_menu();
            }
        });
    }

    fn open_reference(&mut self, t: Tool) {
        let ok = match t {
            Tool::Wall(m) | Tool::Matter(m) => self.codex.open_element(m),
            Tool::Part(p) => self.codex.open_tile(p),
            _ => false,
        };
        if !ok {
            if let Some(e) = elements::find(t) {
                self.codex.query = e.name.to_string();
            }
        }
        self.dock_bottom = true;
        self.bottom_tab = BottomTab::Reference;
    }

    // ------------------------------------------------------------------------------------- right dock: properties / inspector
    pub(super) fn inspector_dock(&mut self, ctx: &egui::Context, frame: &eframe::Frame, stats: &Stats, probe: Option<ProbeData>) {
        let resp = egui::SidePanel::right("inspector").resizable(true).default_width(300.0).width_range(250.0..=480.0).frame(panel_frame().stroke(Stroke::new(1.0_f32, BORDER))).show(ctx, |ui| {
            egui::ScrollArea::vertical().auto_shrink([false, false]).id_salt("insp").show(ui, |ui| {
                if self.run.is_some() {
                    self.experiment_panel(ui, frame, stats);
                }
                self.material_sheet(ui);
                self.boundary_panel(ui);
                let r = ui.scope(|ui| self.cell_sheet(ui, probe)).response.rect;
                self.tut_rects.insert("inspector".into(), r);
                let r = ui.scope(|ui| self.domain_sheet(ui, stats)).response.rect;
                self.tut_rects.insert("readouts".into(), r);
            });
        });
        let _ = resp;
    }

    fn material_sheet(&mut self, ui: &mut egui::Ui) {
        section(ui, "Selected material");
        if self.comp_sheet(ui) {
            return;
        }
        if self.tool == Tool::Gas(super::prop::OP_CAST) {
            ui.label(RichText::new("Cast / press").font(FontId::new(13.0, fam("semibold"))).color(STRONG));
            ui.label(muted("Packs powders and liquids under the brush into a solid, non-porous grain. A cast grain burns only at its exposed surface, layer by layer, at r = a·pⁿ; its geometry (end-burner, core, star) sets the thrust curve. Loose powder burns through its pores almost at once."));
            if !super::prop::native_cast() {
                ui.label(RichText::new("The solver build in use does not implement op kind 9 yet: cast ops are sent but have no effect.").color(WARN).size(11.0));
            }
            return;
        }
        let Some(e) = elements::find(self.tool) else { return };
        ui.horizontal(|ui| {
            swatch(ui, e.color, 14.0);
            ui.label(RichText::new(e.name).font(FontId::new(13.0, fam("semibold"))).color(STRONG));
            ui.label(faint(e.code));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.small_button("Reference").on_hover_text("Open the reference entry").clicked() {
                    self.open_reference(e.tool);
                }
            });
        });
        let cat = CATS.iter().find(|c| c.0 == e.cat).map(|c| c.1).unwrap_or("");
        ui.add_space(2.0);
        let u = self.units;
        sheet(ui, "matsheet", |ui| {
            kv(ui, "Category", cat);
            let (rho, _) = props(e.tool);
            match e.tool {
                Tool::Wall(m) => {
                    let mi = &MATS[(m as usize).min(9)];
                    kv(ui, "Phase", "solid (fixed)");
                    if let Some(r) = rho {
                        kv(ui, "Density", format!("{r:.0} kg/m³"));
                    }
                    if mi.burns() {
                        let (t, k) = match m {
                            7 => (600.0, "Pyrolysis onset"),
                            8 => (750.0, "Ignition"),
                            _ => (600.0, "Ignition"),
                        };
                        kv(ui, k, u.ft(t));
                        if m == 9 {
                            kv(ui, "Shock initiation", u.fp(8.0));
                        }
                    } else {
                        kv(ui, "Softening onset", u.ft(mi.t_soft * 300.0));
                        kv(ui, "Melting point", u.ft(mi.t_melt * 300.0));
                    }
                    let (k_si, cp_si, sy) = MAT_SI[(m as usize).min(9)];
                    kv(ui, "Thermal conductivity", format!("{k_si} W/(m·K)"));
                    kv(ui, "Specific heat", format!("{cp_si:.0} J/(kg·K)"));
                    kv(ui, if m >= 7 { "Strength (typ.)" } else { "Yield strength (typ.)" }, format!("{sy} MPa"));
                }
                Tool::Matter(id) => {
                    if let Some(i) = elements::matter_info(id) {
                        kv(ui, "Phase", ["solid (static)", "powder (granular)", "liquid"][(i.phase as usize).min(2)]);
                        kv(ui, "Density", format!("{:.0} kg/m³", i.density * 1000.0));
                        kv(ui, "Melting point", i.melt_k.map(|v| u.ft(v)).unwrap_or("—".into()));
                        kv(ui, "Boil / decomposition", i.boil_k.map(|v| u.ft(v)).unwrap_or("—".into()));
                        kv(ui, "Ignition", i.ignite_k.map(|v| u.ft(v)).unwrap_or("—".into()));
                        if let Some(d) = i.detonate_atm {
                            kv(ui, "Shock initiation", u.fp(d));
                        }
                        if !i.gives.is_empty() {
                            kv(ui, "Products", i.gives);
                        }
                        if !i.hazard.is_empty() {
                            kv(ui, "Hazard", i.hazard);
                        }
                    }
                }
                Tool::Gas(k) => {
                    kv(ui, "Phase", "gas (source brush)");
                    if let Some(r) = rho {
                        kv(ui, "Density at STP", format!("{r:.3} kg/m³"));
                    }
                    if k == 2 {
                        kv(ui, "Flammability", "5 – 15 % vol in air");
                        kv(ui, "Autoignition", u.ft(868.0));
                        kv(ui, "Adiabatic flame", u.ft(2226.0));
                    }
                }
                Tool::Part(t) => {
                    kv(ui, "Type", "boundary / device");
                    if t == scene::FAN {
                        kv(ui, "Direction", ["+x (right)", "−y (down)", "−x (left)", "+y (up)"][self.fan_dir as usize % 4]);
                    }
                    if t == scene::IGNITER {
                        kv(ui, "Surface temperature", u.ft(3000.0));
                    }
                }
                Tool::Erase => kv(ui, "Action", "remove cells"),
            }
        });
        if let Tool::Wall(m) = e.tool {
            let mi = &MATS[(m as usize).min(9)];
            egui::CollapsingHeader::new(RichText::new("Solver parameters").color(MUTED).size(11.0)).id_salt("solver_params").show(ui, |ui| {
                sheet(ui, "solversheet", |ui| {
                    ui.label(muted("Cell burst pressure")).on_hover_text("Pressure difference across a single 1 cm wall cell at which the solver breaks it. Stress readouts are a fraction of this.");
                    ui.label(num(u.fp(mi.strength)));
                    ui.end_row();
                    ui.label(muted("Conductivity coeff.")).on_hover_text("Dimensionless wall-to-wall conduction coefficient used by the GPU solver (steel = 1).");
                    ui.label(num(format!("{:.3}", mi.cond)));
                    ui.end_row();
                    ui.label(muted("Heat capacity coeff.")).on_hover_text("Dimensionless per-cell thermal inertia used by the GPU solver.");
                    ui.label(num(format!("{:.2}", mi.cap)));
                    ui.end_row();
                    if let Some(sh) = mi.shock {
                        ui.label(muted("Thermal shock limit")).on_hover_text("Largest temperature change per solver step before the cell cracks.");
                        ui.label(num(format!("{:.0} K/step", sh * 300.0)));
                        ui.end_row();
                    }
                });
            });
        }
        ui.add_space(3.0);
        ui.label(RichText::new(e.blurb).color(MUTED).size(11.0));
        let rec = recipes_with(e);
        if !rec.is_empty() {
            ui.add_space(3.0);
            ui.label(faint("REACTIONS"));
            for (n, ing, res) in rec.iter().take(4) {
                ui.label(RichText::new(format!("{n}: {ing} → {res}")).color(MUTED).size(10.5));
            }
        }
    }

    fn boundary_panel(&mut self, ui: &mut egui::Ui) {
        section(ui, "Boundary conditions");
        let locked = self.feeds_locked();
        let pu = self.units.p_unit();
        let ps = self.units.p_scale();
        ui.add_enabled_ui(!locked, |ui| {
            sheet(ui, "bc", |ui| {
                let row = |ui: &mut egui::Ui, k: &str, v: &mut f32, lo: f32, hi: f32, id: &str| -> egui::Response {
                    ui.label(muted(k));
                    let mut d = *v * ps;
                    let r = ui.add(egui::DragValue::new(&mut d).range(lo * ps..=hi * ps).speed(0.02 * ps).max_decimals(if ps > 10.0 { 1 } else { 2 }).suffix(format!(" {pu}")));
                    *v = d / ps;
                    ui.end_row();
                    let _ = id;
                    r
                };
                let [fuel, o2, air, fan] = self.inlets;
                if fuel {
                    let r = row(ui, "Fuel inlet (CH₄) p₀", &mut self.p_fuel, 1.0, 30.0, "fuel");
                    self.tut_rects.insert("slider:fuel".into(), r.rect);
                }
                if o2 {
                    row(ui, "Oxygen inlet p₀", &mut self.p_o2, 1.0, 30.0, "o2");
                }
                if air {
                    row(ui, "Air inlet p₀", &mut self.p_air, 1.0, 10.0, "air");
                }
                if fan {
                    ui.label(muted("Fan power"));
                    ui.add(egui::DragValue::new(&mut self.fan_pow).range(0.0..=3.0).speed(0.01).max_decimals(2));
                    ui.end_row();
                }
                if !(fuel || o2 || air || fan) {
                    ui.label(faint("No inlets in domain"));
                    ui.end_row();
                }
            });
        });
        ui.add_enabled_ui(self.run.is_none(), |ui| {
            ui.horizontal(|ui| {
                ui.checkbox(&mut self.ign_on, "Igniters");
                ui.checkbox(&mut self.gravity_on, "Gravity");
                if ui.small_button("Zero load cell").on_hover_text("Tare the thrust measurement").clicked() {
                    self.tare = self.hist_raw_thrust;
                }
            });
        });
        if locked {
            ui.label(faint("Inlet conditions are fixed by the experiment."));
        }
    }

    fn cell_sheet(&mut self, ui: &mut egui::Ui, probe: Option<ProbeData>) {
        let (x, y) = self.last_probe_cell;
        let Some(pd) = probe.filter(|_| self.hover.is_some() || self.pin_cell) else {
            return;
        };
        section(ui, "Cell at cursor");
        let u = self.units;
        let yc = H as i32 - 1 - y;
        ui.label(RichText::new(format!("x = {}   y = {}", u.fl(x as f32 + 0.5), u.fl(yc as f32 + 0.5))).monospace().color(MUTED).size(11.0));
        let tile = pd.meta & 0xff;
        if tile == scene::WALL {
            let m = &MATS[((pd.meta >> 8) & 0xff).min(9) as usize];
            let [tw, dmg, stress, rem] = pd.wall;
            sheet(ui, "cellw", |ui| {
                kv(ui, "Material", format!("{} (solid)", m.name));
                kv(ui, "Temperature", u.ft(tw * 300.0));
                if m.burns() {
                    kv(ui, "Remaining", format!("{:.1} %", rem.clamp(0.0, 1.0) * 100.0));
                } else {
                    kv(ui, "T / T melt", format!("{:.1} %", tw / m.t_melt * 100.0));
                    let sf = if tw <= m.t_soft { 1.0 } else { 1.0 - 0.9 * ((tw - m.t_soft) / (m.t_melt - m.t_soft)).min(1.0).powi(2) };
                    kv(ui, "Strength retained", format!("{:.1} %", sf * 100.0));
                    kv(ui, "Damage", format!("{:.1} %", dmg.min(1.0) * 100.0));
                }
                kv(ui, "Stress / yield", format!("{:.1} %", stress * 100.0));
            });
            return;
        }
        if tile == scene::MATTER {
            let id = (pd.meta >> 8) & 0xff;
            let [tw, _, code, rem] = pd.wall;
            let state = if id != 24 && code >= 1000.0 {
                match (code / 1000.0) as u32 {
                    1 => "evaporating",
                    2 => "releasing O₂",
                    3 => "reacting (hot products)",
                    4 => "burning in air",
                    5 => "evolving H₂",
                    6 => "decomposing (O₂ + steam)",
                    7 => "releasing inert gas",
                    _ => "outgassing",
                }
            } else {
                "inert"
            };
            sheet(ui, "cellm", |ui| {
                kv(ui, "Material", elements::matter_name(id));
                kv(ui, "Temperature", u.ft(tw * 300.0));
                kv(ui, "Remaining", format!("{:.1} %", rem.clamp(0.0, 1.0) * 100.0));
                kv(ui, "State", state);
            });
            self.mix_sheet(ui);
            return;
        }
        if tile == scene::IGNITER {
            sheet(ui, "celli", |ui| {
                kv(ui, "Device", "Igniter");
                kv(ui, "State", if self.ign_on { "energised (≈3000 K)" } else { "off" });
            });
            return;
        }
        let uu = &pd.u;
        let g = gas(uu);
        let rho = g.rho;
        let label = match tile {
            2 => "Fuel inlet",
            3 => "Oxygen inlet",
            4 => "Air inlet",
            5 => "Fan",
            7 => "Vent (1 atm)",
            _ => "Gas",
        };
        let fr = [uu[0].max(0.0) / rho, uu[1].max(0.0) / rho, uu[2].max(0.0) / rho, uu[3].max(0.0) / rho, uu[7].max(0.0) / rho];
        let (vx, vy) = (uu[4] / rho * MPS_PER_UNIT, -uu[5] / rho * MPS_PER_UNIT);
        sheet(ui, "cellg", |ui| {
            kv(ui, "Region", label);
            kv(ui, "Pressure", u.fp(g.p));
            kv(ui, "Temperature", u.ft(g.t * 300.0));
            kv(ui, "Density", format!("{:.3} kg/m³", rho * 1.2));
            kv(ui, "Velocity |u|", u.fv(g.speed * MPS_PER_UNIT));
            kv(ui, "  (uₓ, u_y)", format!("{:.0}, {:.0}", u.v(vx), u.v(vy)));
            kv(ui, "Mach number", format!("{:.3}", g.mach));
        });
        ui.add_space(4.0);
        ui.label(faint("MASS FRACTIONS"));
        let cols = [Color32::from_rgb(220, 196, 60), Color32::from_rgb(80, 160, 230), Color32::from_rgb(120, 124, 136), Color32::from_rgb(214, 216, 222), Color32::from_rgb(90, 80, 72)];
        let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 6.0), Sense::hover());
        let mut x0 = rect.left();
        for k in 0..5 {
            let w = (rect.right() - x0).min(rect.width() * fr[k].clamp(0.0, 1.0));
            ui.painter().rect_filled(Rect::from_min_size(Pos2::new(x0, rect.top()), Vec2::new(w, rect.height())), CornerRadius::ZERO, cols[k]);
            x0 += w;
        }
        ui.add_space(2.0);
        egui::Grid::new("yfr").num_columns(4).spacing([10.0, 2.0]).show(ui, |ui| {
            for (k, n) in ["CH₄", "O₂", "N₂", "CO₂+H₂O", "soot"].iter().enumerate() {
                ui.horizontal(|ui| {
                    swatch(ui, cols[k], 8.0);
                    ui.label(muted(*n));
                });
                ui.label(num(format!("{:.4}", fr[k])));
                if k % 2 == 1 {
                    ui.end_row();
                }
            }
        });
        if fr[0] > 0.002 && fr[1] > 0.002 {
            let phi = (fr[0] / fr[1]) / 0.25;
            let regime = if phi > 1.7 {
                "above rich limit"
            } else if phi < 0.5 {
                "below lean limit"
            } else if g.t < 3.0 {
                "flammable, unignited"
            } else {
                "flammable, reacting"
            };
            ui.label(RichText::new(format!("Equivalence ratio φ = {phi:.2}  ({regime})")).color(MUTED).size(11.0));
        }
    }

    fn domain_sheet(&mut self, ui: &mut egui::Ui, stats: &Stats) {
        section(ui, "Domain extrema");
        let u = self.units;
        let thrust = self.shown_thrust(stats).length() * KN_PER_UNIT;
        sheet(ui, "dom", |ui| {
            kv(ui, "Peak pressure", u.fp(stats.max_p));
            kv(ui, "Peak gas temperature", u.ft(stats.max_t * 300.0));
            kv(ui, "Fuel consumption", format!("{:.3} kg/s", stats.burn * KGS_PER_UNIT));
            if self.engine_scene {
                kv(ui, "Net thrust", format!("{thrust:.2} kN"));
            }
            kv_limit(ui, "Hottest wall", stats.max_wall_heat, format!("{:.1} % of T melt", stats.max_wall_heat * 100.0), "melting point");
            kv_limit(ui, "Peak wall stress", stats.max_stress, format!("{:.1} % of yield", stats.max_stress * 100.0), "yield strength");
            kv(ui, "Solid cells", format!("{}", stats.walls));
            if self.diag.valid {
                kv(ui, "Max flow speed", u.fv(self.diag.vmax));
            }
        });
    }

    fn experiment_panel(&mut self, ui: &mut egui::Ui, frame: &eframe::Frame, stats: &Stats) {
        let Some(run) = &self.run else { return };
        let m = run.m();
        section(ui, "Experiment");
        ui.label(RichText::new(format!("Exp {:02} · {}", run.idx + 1, crate::missions::lab_title(m.title))).font(FontId::new(13.0, fam("semibold"))).color(STRONG));
        if let Some((_, ch)) = chapter_of_cached(run.idx) {
            ui.label(faint(crate::missions::lab_chapter(&ch)));
        }
        ui.add_space(2.0);
        ui.label(RichText::new(m.brief).color(MUTED).size(11.0));
        ui.add_space(4.0);
        let phase = match run.phase {
            Phase::Build => "Setup: build the apparatus, then start the run".to_string(),
            Phase::Test => format!("Run in progress: protocol clock {:.1} / {:.0} s  ·  t_sim {:.3} ms", run.t, m.duration, run.t * TPS * MS_PER_UNIT),
            Phase::Done => format!("Run complete: {}", if run.passed { "PASS" } else { "FAIL" }),
        };
        ui.label(RichText::new(phase).color(TEXT).size(11.0)).on_hover_text(format!("Experiment durations are on a protocol clock of {TPS:.0} solver time units per second ({:.3} ms of simulated time per protocol second), so a run stays watchable. Physical time is t_sim.", TPS * MS_PER_UNIT));
        if let Some(b) = run.over_budget() {
            ui.label(RichText::new(b).color(WARN).size(11.0));
        }
        ui.add_space(4.0);
        self.objective_list(ui, stats);
        ui.add_space(4.0);
        let phase = run.phase;
        let live = m.live;
        ui.horizontal(|ui| match phase {
            Phase::Build => {
                if ui.add(egui::Button::new(RichText::new("Start run").color(STRONG)).fill(ACCENT_DIM).stroke(Stroke::new(1.0_f32, ACCENT))).clicked() {
                    self.begin_test(frame);
                }
            }
            Phase::Test => {
                if !live && ui.button("Abort run").clicked() {
                    self.back_to_build();
                }
                if live && ui.button("Restart").clicked() {
                    self.back_to_build();
                }
            }
            Phase::Done => {
                if ui.button("Return to setup").clicked() {
                    self.back_to_build();
                }
            }
        });
        if !m.tip.is_empty() {
            egui::CollapsingHeader::new(RichText::new("Notes").color(MUTED).size(11.0)).id_salt("exp-notes").show(ui, |ui| {
                ui.label(RichText::new(m.tip).color(MUTED).size(11.0));
            });
        }
    }

    /// Narrow (side panel) form: one block per objective.
    fn objective_list(&self, ui: &mut egui::Ui, stats: &Stats) {
        let Some(run) = &self.run else { return };
        let m = run.m();
        for (k, o) in m.objectives.iter().enumerate() {
            let s = run.obj.get(k);
            let st = s.map(|s| s.st).unwrap_or(St::Pending);
            let (t, c) = match (st, run.phase) {
                (St::Done, _) => ("Achieved".to_string(), GOOD),
                (St::Failed, _) => ("Not achieved".to_string(), BAD),
                (St::Pending, Phase::Test) => {
                    let pr = s.map(|s| s.progress).unwrap_or(0.0);
                    (if pr > 0.0 { format!("{:.0} %", pr * 100.0) } else { "Pending".into() }, MUTED)
                }
                _ => ("—".into(), FAINT),
            };
            ui.horizontal(|ui| {
                let (r, _) = ui.allocate_exact_size(Vec2::new(3.0, 30.0), Sense::hover());
                ui.painter().rect_filled(r, CornerRadius::ZERO, c);
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 1.0;
                    let opt = if o.required { "" } else { "  optional" };
                    ui.horizontal(|ui| {
                        ui.add(egui::Label::new(RichText::new(o.label()).color(TEXT).size(11.5)).wrap());
                    });
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(t).font(FontId::new(10.0, fam("semibold"))).color(c));
                        ui.label(num(self.live_text(o, stats).unwrap_or_default()).size(11.0));
                        ui.label(faint(opt));
                    });
                });
            });
        }
    }

    fn objective_table(&self, ui: &mut egui::Ui, stats: &Stats, id: &str) {
        let Some(run) = &self.run else { return };
        let m = run.m();
        egui::Grid::new(id).num_columns(3).spacing([12.0, 3.0]).min_col_width(80.0).striped(true).show(ui, |ui| {
            ui.label(faint("OBJECTIVE"));
            ui.label(faint("MEASURED"));
            ui.label(faint("STATUS"));
            ui.end_row();
            for (k, o) in m.objectives.iter().enumerate() {
                let s = run.obj.get(k);
                let st = s.map(|s| s.st).unwrap_or(St::Pending);
                let req = if o.required { "" } else { " (optional)" };
                let full = format!("{}{req}", o.label());
                let ow = (ui.available_width() * 0.55).clamp(140.0, 300.0);
                ui.allocate_ui_with_layout(Vec2::new(ow, 16.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    ui.set_max_width(ow);
                    ui.add(egui::Label::new(RichText::new(&full).color(TEXT).size(11.0)).wrap());
                });
                ui.label(num(self.live_text(o, stats).unwrap_or("—".into())).size(11.0));
                let (t, c) = match (st, run.phase) {
                    (St::Done, _) => ("met", GOOD),
                    (St::Failed, _) => ("not met", BAD),
                    (St::Pending, Phase::Test) => {
                        let pr = s.map(|s| s.progress).unwrap_or(0.0);
                        if pr > 0.0 && pr < 1.0 {
                            ui.label(RichText::new(format!("{:.0} %", pr * 100.0)).color(MUTED).size(11.0));
                            ui.end_row();
                            continue;
                        }
                        ("pending", MUTED)
                    }
                    _ => ("—", FAINT),
                };
                ui.label(RichText::new(t).color(c).size(11.0));
                ui.end_row();
            }
        });
    }

    fn live_text(&self, o: &Objective, stats: &Stats) -> Option<String> {
        let run = self.run.as_ref()?;
        let u = self.units;
        let fmt = |m: Metric, x: f32| match m {
            Metric::Leak => if x > 0.5 { "leaks".to_string() } else { "sealed".to_string() },
            Metric::Burn => format!("{x:.2} kg/s"),
            Metric::PeakTemp => u.ft(x),
            Metric::PeakPressure => u.fp(x),
            Metric::Thrust => format!("{x:.1} kN"),
            _ => format!("{x:.0} {}", m.unit()),
        };
        match o.goal {
            Goal::AtMost(m, _) => Some(fmt(m, run.value(m, stats))),
            Goal::Hold(m, _, _) | Goal::Reach(m, _) | Goal::ReachBy(m, _, _) | Goal::Keep(m, _) | Goal::HoldBelow(m, _, _) => {
                if run.phase == Phase::Build && !matches!(m, Metric::Leak) {
                    return None;
                }
                Some(fmt(m, run.value(m, stats)))
            }
        }
    }

    // ------------------------------------------------------------------------------------- bottom dock
    pub(super) fn bottom_dock(&mut self, ctx: &egui::Context, frame: &eframe::Frame, stats: &Stats) {
        egui::TopBottomPanel::bottom("dock").resizable(true).default_height(228.0).height_range(120.0..=520.0).frame(egui::Frame::NONE.fill(PANEL).stroke(Stroke::new(1.0_f32, BORDER))).show(ctx, |ui| {
            // tab strip
            let (strip, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 26.0), Sense::hover());
            ui.painter().rect_filled(strip, CornerRadius::ZERO, BAR);
            ui.painter().hline(strip.x_range(), strip.bottom() - 0.5, Stroke::new(1.0_f32, BORDER));
            let mut x = strip.left() + 8.0;
            for (t, n) in BottomTab::ALL {
                let g = ui.painter().layout_no_wrap(n.to_string(), FontId::proportional(11.5), TEXT);
                let r = Rect::from_min_size(Pos2::new(x, strip.top()), Vec2::new(g.size().x + 20.0, strip.height()));
                let resp = ui.interact(r, egui::Id::new(("tab", n)), Sense::click());
                let sel = self.bottom_tab == t;
                if sel {
                    ui.painter().rect_filled(r, CornerRadius::ZERO, PANEL);
                    ui.painter().hline(r.x_range(), r.top() + 0.5, Stroke::new(2.0_f32, ACCENT));
                } else if resp.hovered() {
                    ui.painter().rect_filled(r, CornerRadius::ZERO, HOVER);
                }
                ui.painter().galley(Pos2::new(r.left() + 10.0, r.center().y - g.size().y / 2.0), g, if sel { STRONG } else { MUTED });
                if resp.clicked() {
                    self.bottom_tab = t;
                }
                x = r.right();
            }
            let full = ui.available_rect_before_wrap();
            ui.allocate_rect(full, Sense::hover());
            let body = full.shrink2(Vec2::new(8.0, 6.0));
            let mut cu = ui.new_child(egui::UiBuilder::new().max_rect(body).layout(egui::Layout::top_down(egui::Align::Min)));
            cu.set_clip_rect(body.expand(2.0));
            match self.bottom_tab {
                BottomTab::Plots => self.tab_plots(&mut cu),
                BottomTab::Probes => self.tab_probes(&mut cu),
                BottomTab::Profiles => self.tab_profiles(&mut cu),
                BottomTab::Console => self.tab_console(&mut cu),
                BottomTab::Reference if self.show_codex => {
                    cu.add_space(12.0);
                    cu.label(muted("The reference manual is open in its own window."));
                    if cu.button("Dock it here").clicked() {
                        self.show_codex = false;
                        self.codex.open = false;
                    }
                }
                BottomTab::Reference => {
                    let r = crate::codex::show(&mut cu, &mut self.codex);
                    self.codex_response(r);
                }
                BottomTab::Experiment => self.tab_experiments(&mut cu, frame, stats),
            }
        });
    }

    fn codex_response(&mut self, r: crate::codex::CodexResponse) {
        if let Some(id) = r.use_element {
            let t = if id < 10 { Tool::Wall(id) } else { Tool::Matter(id) };
            if self.allowed(t) {
                self.pick(t);
            }
        }
        if let Some(t) = r.use_tile {
            let t = Tool::Part(t);
            if self.allowed(t) {
                self.pick(t);
            }
        }
    }

    fn tab_plots(&mut self, ui: &mut egui::Ui) {
        let u = self.units;
        let t: Vec<f32> = self.hist.t.iter().copied().collect();
        let mk = |q: &VecDeque<f32>, f: &dyn Fn(f32) -> f32| -> Vec<[f32; 2]> { t.iter().zip(q.iter()).map(|(a, b)| [*a, f(*b)]).collect() };
        let p = mk(&self.hist.p, &|v| u.p(v));
        let tt = mk(&self.hist.temp, &|v| u.t(v));
        let th = mk(&self.hist.thrust, &|v| v);
        let br = mk(&self.hist.burn, &|v| v);
        let wh = mk(&self.hist.wall_heat, &|v| v);
        let ss = mk(&self.hist.stress, &|v| v);
        let avail = ui.available_size();
        let n = if avail.x > 1100.0 { 4 } else { 3 };
        let w = (avail.x - 8.0 * (n as f32 - 1.0)) / n as f32;
        let size = Vec2::new(w, avail.y.max(100.0));
        ui.horizontal(|ui| {
            chart(ui, size, "Peak pressure", "t, ms", u.p_unit(), &[Series { name: "p max".into(), color: Color32::from_rgb(110, 160, 235), pts: &p }], Some(0.0));
            chart(ui, size, "Peak gas temperature", "t, ms", u.t_unit(), &[Series { name: "T max".into(), color: Color32::from_rgb(230, 150, 70), pts: &tt }], None);
            if self.engine_scene {
                chart_dual(ui, size, "Thrust / fuel flow", "t, ms", ("F", "kN", Color32::from_rgb(200, 200, 205), &th), ("ṁ", "kg/s", Color32::from_rgb(120, 190, 120), &br));
            } else {
                chart(ui, size, "Fuel consumption", "t, ms", "kg/s", &[Series { name: "ṁ".into(), color: Color32::from_rgb(120, 190, 120), pts: &br }], Some(0.0));
            }
            if n == 4 {
                chart(ui, size, "Structure", "t, ms", "%", &[Series { name: "T/Tmelt".into(), color: Color32::from_rgb(220, 110, 90), pts: &wh }, Series { name: "σ/σy".into(), color: Color32::from_rgb(200, 190, 90), pts: &ss }], Some(0.0));
            }
        });
    }

    fn quantity_combo(&self, ui: &mut egui::Ui, id: &str, q: &mut usize) {
        let names = self.quantity_names();
        egui::ComboBox::from_id_salt(id).width(150.0).selected_text(names[*q].clone()).show_ui(ui, |ui| {
            for (k, n) in names.iter().enumerate() {
                ui.selectable_value(q, k, n.clone());
            }
        });
    }

    fn quantity_names(&self) -> Vec<String> {
        let u = self.units;
        vec![
            format!("Pressure ({})", u.p_unit()),
            format!("Temperature ({})", u.t_unit()),
            format!("Velocity |u| ({})", u.v_unit()),
            "Density (kg/m³)".into(),
            "Mach number".into(),
            "Y CH₄".into(),
            "Y O₂".into(),
            "Y products".into(),
            "Y soot".into(),
        ]
    }

    fn quantity(&self, s: &Sample, q: usize) -> f32 {
        let u = self.units;
        match q {
            0 => u.p(s.p_atm),
            1 => u.t(s.t_k),
            2 => u.v(s.speed),
            3 => s.rho,
            4 => s.mach,
            5 => s.y_fuel,
            6 => s.y_o2,
            7 => s.y_prod,
            _ => s.soot,
        }
    }

    fn tab_probes(&mut self, ui: &mut egui::Ui) {
        let mut q: usize = ui.ctx().data(|d| d.get_temp(egui::Id::new("probe_q"))).unwrap_or(1);
        ui.horizontal(|ui| {
            if ui.button("Add probe").on_hover_text("Click a cell in the viewport").clicked() {
                self.place = Some(Place::Probe);
            }
            if ui.button("Add line sampler").clicked() {
                self.place = Some(Place::Line(None));
            }
            if ui.add_enabled(!self.probes.is_empty(), egui::Button::new("Export CSV...")).clicked() {
                self.export_probes_csv();
            }
            if ui.add_enabled(!self.probes.probes.is_empty(), egui::Button::new("Clear series")).clicked() {
                self.probes.reset_series();
            }
            ui.add_space(12.0);
            ui.label(muted("Plot"));
            self.quantity_combo(ui, "probe_qc", &mut q);
            if self.place.is_some() {
                ui.label(RichText::new("Placement active: click in the viewport (Esc cancels)").color(ACCENT).size(11.0));
            }
        });
        ui.ctx().data_mut(|d| d.insert_temp(egui::Id::new("probe_q"), q));
        ui.add_space(4.0);
        if self.probes.probes.is_empty() {
            ui.add_space(20.0);
            ui.vertical_centered(|ui| {
                ui.label(muted("No probes placed. Use Tools > Place point probe, or the probe button in the toolbar."));
            });
            return;
        }
        let avail = ui.available_size();
        let table_w = (avail.x * 0.52).clamp(420.0, 720.0);
        let u = self.units;
        let mut remove = None;
        ui.horizontal_top(|ui| {
            ui.allocate_ui(Vec2::new(table_w, avail.y), |ui| {
                egui::ScrollArea::vertical().id_salt("ptab").auto_shrink([false, false]).show(ui, |ui| {
                    egui::Grid::new("probetable").num_columns(10).striped(true).spacing([10.0, 3.0]).show(ui, |ui| {
                        for h in ["", "ID", &format!("x {}", u.l_unit()), &format!("y {}", u.l_unit()), &format!("p {}", u.p_unit()), &format!("T {}", u.t_unit()), &format!("|u| {}", u.v_unit()), "Y CH₄", "Y O₂", ""] {
                            let r = ui.label(faint(h));
                            if h.starts_with("Y ") {
                                r.on_hover_text(format!("Mass fraction of {} (kg per kg of gas)", &h[2..]));
                            }
                        }
                        ui.end_row();
                        for pr in &self.probes.probes {
                            swatch(ui, pr.color, 9.0);
                            ui.label(num(format!("P{}", pr.id)));
                            ui.label(num(format!("{:.0}", u.l(pr.x as f32 + 0.5))));
                            ui.label(num(format!("{:.0}", u.l((H as i32 - 1 - pr.y) as f32 + 0.5))));
                            let s = pr.series.back().copied().unwrap_or_default();
                            let f = |v: f32, d: usize| if v.is_finite() { format!("{v:.d$}") } else { "—".into() };
                            ui.label(num(f(u.p(s.p_atm), if u.p_scale() > 10.0 { 1 } else { 3 })));
                            ui.label(num(f(u.t(s.t_k), 0)));
                            ui.label(num(f(u.v(s.speed), 1)));
                            ui.label(num(f(s.y_fuel, 4)));
                            ui.label(num(f(s.y_o2, 4)));
                            if ui.small_button("Remove").clicked() {
                                remove = Some(pr.id);
                            }
                            ui.end_row();
                        }
                    });
                });
            });
            ui.add_space(8.0);
            let names = self.quantity_names();
            let data: Vec<(String, Color32, Vec<[f32; 2]>)> = self.probes.probes.iter().map(|p| (format!("P{}", p.id), p.color, p.series.iter().map(|s| [s.t_ms, self.quantity(s, q)]).collect())).collect();
            let series: Vec<Series> = data.iter().map(|(n, c, v)| Series { name: n.clone(), color: *c, pts: v }).collect();
            let size = Vec2::new(ui.available_width(), avail.y);
            chart(ui, size, &format!("Probe history: {}", names[q]), "t, ms", "", &series, None);
        });
        if let Some(id) = remove {
            self.probes.probes.retain(|p| p.id != id);
        }
    }

    fn tab_profiles(&mut self, ui: &mut egui::Ui) {
        let mut q: usize = ui.ctx().data(|d| d.get_temp(egui::Id::new("line_q"))).unwrap_or(1);
        let mut remove = None;
        ui.horizontal(|ui| {
            if ui.button("Add line sampler").clicked() {
                self.place = Some(Place::Line(None));
            }
            if ui.add_enabled(!self.probes.is_empty(), egui::Button::new("Export CSV...")).clicked() {
                self.export_probes_csv();
            }
            ui.add_space(12.0);
            ui.label(muted("Quantity"));
            self.quantity_combo(ui, "line_qc", &mut q);
            for l in &self.probes.lines {
                ui.add_space(8.0);
                swatch(ui, l.color, 9.0);
                ui.label(num(format!("L{}  {}", l.id, self.units.fl(l.length_cm()))));
                if ui.small_button("Remove").clicked() {
                    remove = Some(l.id);
                }
            }
        });
        ui.ctx().data_mut(|d| d.insert_temp(egui::Id::new("line_q"), q));
        if let Some(id) = remove {
            self.probes.lines.retain(|l| l.id != id);
        }
        if self.probes.lines.is_empty() {
            ui.add_space(20.0);
            ui.vertical_centered(|ui| ui.label(muted("No line samplers. Tools > Place line sampler, then click two points in the viewport.")));
            return;
        }
        let u = self.units;
        let names = self.quantity_names();
        let data: Vec<(String, Color32, Vec<[f32; 2]>)> = self.probes.lines.iter().map(|l| (format!("L{}", l.id), l.color, l.profile.iter().map(|(d, s)| [u.l(*d), self.quantity(s, q)]).collect())).collect();
        let series: Vec<Series> = data.iter().map(|(n, c, v)| Series { name: n.clone(), color: *c, pts: v }).collect();
        let size = ui.available_size();
        chart(ui, size, &format!("Profile along sampler: {}", names[q]), &format!("s, {}", u.l_unit()), "", &series, None);
    }

    fn tab_console(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(muted(format!("{} messages", self.log.len())));
            if ui.small_button("Clear").clicked() {
                self.log.clear();
            }
            if ui.small_button("Copy all").clicked() {
                let s: Vec<String> = self.log.iter().map(|e| format!("{:10.3} ms  {}  {}", e.t_ms, if e.level == Level::Warn { "WARN" } else { "INFO" }, e.text)).collect();
                ui.ctx().copy_text(s.join("\n"));
            }
        });
        ui.add_space(2.0);
        egui::Frame::NONE.fill(Color32::from_rgb(19, 20, 23)).stroke(Stroke::new(1.0_f32, BORDER)).inner_margin(egui::Margin::same(6)).show(ui, |ui| {
            egui::ScrollArea::vertical().id_salt("console").auto_shrink([false, false]).stick_to_bottom(true).show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 1.0;
                for e in &self.log {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(format!("{:>10.3} ms", e.t_ms)).monospace().color(FAINT));
                        let (lv, c) = if e.level == Level::Warn { ("WARN", WARN) } else { ("INFO", MUTED) };
                        ui.label(RichText::new(lv).monospace().color(c));
                        ui.label(RichText::new(&e.text).monospace().color(TEXT));
                    });
                }
            });
        });
    }

    fn tab_experiments(&mut self, ui: &mut egui::Ui, frame: &eframe::Frame, stats: &Stats) {
        let w = ui.available_width();
        ui.horizontal_top(|ui| {
            let list_w = if self.run.is_some() { (w * 0.5).max(420.0) } else { w };
            ui.allocate_ui(Vec2::new(list_w, ui.available_height()), |ui| self.experiment_list(ui, "explist_dock"));
            if self.run.is_some() {
                ui.add_space(12.0);
                ui.vertical(|ui| {
                    ui.label(RichText::new("CURRENT RUN").font(head_font()).color(MUTED));
                    self.objective_table(ui, stats, "objtab_dock");
                    let _ = frame;
                });
            }
        });
    }

    fn experiment_list(&mut self, ui: &mut egui::Ui, id: &str) {
        let ms = missions_list();
        let mut load = None;
        egui::ScrollArea::vertical().id_salt(id).auto_shrink([false, false]).show(ui, |ui| {
            egui::Grid::new((id, "g")).num_columns(5).striped(true).spacing([12.0, 4.0]).show(ui, |ui| {
                for h in ["#", "EXPERIMENT", "SUMMARY", "STATUS", ""] {
                    ui.label(faint(h));
                }
                ui.end_row();
                let mut last_ch = String::new();
                for (k, m) in ms.iter().enumerate() {
                    let ch = chapter_of_cached(k).map(|c| c.1).unwrap_or_else(|| "Additional exercises".into());
                    if ch != last_ch {
                        ui.label("");
                        ui.label(RichText::new(crate::missions::lab_chapter(&ch).to_uppercase()).font(head_font()).color(MUTED));
                        ui.end_row();
                        last_ch = ch;
                    }
                    let open = self.settings.unlock_all || crate::campaign_bridge::unlocked(k, &self.stars) || std::env::var("COMBUSTOR_UNLOCK").is_ok();
                    let cur = self.run.as_ref().map(|r| r.idx == k).unwrap_or(false);
                    ui.label(num(format!("{:02}", k + 1)).color(MUTED));
                    let t = RichText::new(crate::missions::lab_title(m.title)).color(if open { STRONG } else { FAINT });
                    ui.label(if cur { t.underline() } else { t });
                    ui.allocate_ui_with_layout(Vec2::new(260.0, 16.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                        ui.set_width(260.0);
                        ui.add(egui::Label::new(RichText::new(m.tagline).color(MUTED).size(11.0)).truncate()).on_hover_text(m.tagline);
                    });
                    let req = crate::campaign_bridge::def_of(k).and_then(|d| d.requires.first().cloned()).and_then(|id| (0..ms.len()).find(|&j| crate::campaign_bridge::def_of(j).map(|d| d.id == id).unwrap_or(false)));
                    let locked_txt;
                    let (res, c) = match self.stars.get(k).copied().unwrap_or(0) {
                        0 if !open => {
                            locked_txt = match req {
                                Some(j) => format!("Locked (requires {:02})", j + 1),
                                None => "Locked".into(),
                            };
                            (locked_txt.as_str(), FAINT)
                        }
                        0 => ("Not run", MUTED),
                        1 => ("Passed", GOOD),
                        n => (if n >= 3 { "Passed, all optional" } else { "Passed, +optional" }, GOOD),
                    };
                    ui.label(RichText::new(res).color(c).size(11.0));
                    if open {
                        if ui.add(egui::Button::new(if cur { "Reload" } else { "Load" }).small()).clicked() {
                            load = Some(k);
                        }
                    } else {
                        ui.label("");
                    }
                    ui.end_row();
                }
            });
        });
        if let Some(k) = load {
            self.start_mission(k, false);
            self.log_info(&format!("Loaded experiment {:02}: {}", k + 1, crate::missions::lab_title(ms[k].title)));
        }
    }

    // ------------------------------------------------------------------------------------- viewport overlay
    pub(super) fn viewport_overlay(&mut self, ui: &mut egui::Ui, full: Rect, header: Rect, rect: Rect, cam: [f32; 4], stats: &Stats, probe: Option<ProbeData>) {
        let u = self.units;
        let cell_px = rect.width() / cam[2];
        let to_screen = |cx: f32, cy: f32| Pos2::new(rect.left() + (cx - cam[0]) / cam[2] * rect.width(), rect.top() + (cy - cam[1]) / cam[3] * rect.height());
        let fp = ui.painter_at(full);
        // header: field selector + readout
        fp.rect_filled(header, CornerRadius::ZERO, BAR);
        fp.hline(header.x_range(), header.bottom() - 0.5, Stroke::new(1.0_f32, BORDER));
        let mut x = header.left() + 8.0;
        fp.text(Pos2::new(x, header.center().y), Align2::LEFT_CENTER, "FIELD", FontId::new(10.0, fam("semibold")), MUTED);
        x += 36.0;
        let mut view_rect = Rect::NOTHING;
        for (k, (n, d)) in VIEW_NAMES.iter().enumerate() {
            let g = fp.layout_no_wrap(n.to_string(), FontId::proportional(11.5), TEXT);
            let r = Rect::from_min_size(Pos2::new(x, header.top() + 4.0), Vec2::new(g.size().x + 16.0, header.height() - 8.0));
            let resp = ui.interact(r, egui::Id::new(("view", k)), Sense::click()).on_hover_text(format!("{d}  ({})", k + 1));
            let sel = self.view == k as u32;
            if sel {
                fp.rect_filled(r, CornerRadius::same(2), ACCENT_DIM);
                fp.rect_stroke(r, CornerRadius::same(2), Stroke::new(1.0_f32, ACCENT.gamma_multiply(0.7)), StrokeKind::Inside);
            } else if resp.hovered() {
                fp.rect_filled(r, CornerRadius::same(2), HOVER);
            }
            fp.galley(Pos2::new(r.left() + 8.0, r.center().y - g.size().y / 2.0), g, if sel { STRONG } else { MUTED });
            if resp.clicked() {
                self.set_view(k as u32);
            }
            let key = ["fire", "pressure", "heat", "flow", "gases", "stress"][k];
            self.tut_rects.insert(format!("view:{key}"), r);
            view_rect = view_rect.union(r);
            x = r.right() + 2.0;
        }
        self.tut_rects.insert("views".into(), view_rect);
        // cursor readout
        let readout = match (self.hover, probe) {
            (Some((cx, cy)), Some(pd)) => {
                let yc = H as i32 - 1 - cy;
                let pos = format!("x {:>7}  y {:>7}", u.fl(cx as f32 + 0.5), u.fl(yc as f32 + 0.5));
                let tile = pd.meta & 0xff;
                let v = if tile == scene::WALL || tile == scene::MATTER {
                    let name = if tile == scene::WALL { MATS[((pd.meta >> 8) & 0xff).min(9) as usize].name } else { elements::matter_name((pd.meta >> 8) & 0xff) };
                    format!("{name}   T {}", u.ft(pd.wall[0] * 300.0))
                } else {
                    let g = gas(&pd.u);
                    format!("p {}   T {}   |u| {}", u.fp(g.p), u.ft(g.t * 300.0), u.fv(g.speed * MPS_PER_UNIT))
                };
                format!("{pos}    {v}")
            }
            _ => format!("domain {} × {}   zoom {:.1}×", u.fl(W as f32), u.fl(H as f32), self.zoom),
        };
        fp.text(Pos2::new(header.right() - 10.0, header.center().y), Align2::RIGHT_CENTER, readout, mono_font(), TEXT);

        // frame + axes
        fp.rect_stroke(rect, CornerRadius::ZERO, Stroke::new(1.0_f32, BORDER), StrokeKind::Outside);
        let target_px = 64.0;
        let step_cm = [1.0f32, 2.0, 5.0, 10.0, 20.0, 25.0, 50.0, 100.0, 200.0].iter().copied().find(|s| s * cell_px >= target_px).unwrap_or(200.0);
        let gstroke = Stroke::new(1.0_f32, Color32::from_white_alpha(22));
        let x0 = (cam[0] / step_cm).ceil() * step_cm;
        let mut xv = x0;
        while xv <= cam[0] + cam[2] + 0.01 {
            let sx = to_screen(xv, 0.0).x;
            fp.vline(sx, rect.bottom()..=rect.bottom() + 4.0, Stroke::new(1.0_f32, FAINT));
            fp.text(Pos2::new(sx, rect.bottom() + 6.0), Align2::CENTER_TOP, tick_label(u.l(xv), u.l(step_cm)), small_font(), MUTED);
            if self.show_grid {
                fp.vline(sx, rect.y_range(), gstroke);
            }
            xv += step_cm;
        }
        // y axis measured upwards from the floor
        let ybot = H as f32;
        let mut yv = 0.0f32;
        while yv <= H as f32 {
            let cy = ybot - yv;
            if cy >= cam[1] - 0.01 && cy <= cam[1] + cam[3] + 0.01 {
                let sy = to_screen(0.0, cy).y;
                fp.hline(rect.left() - 4.0..=rect.left(), sy, Stroke::new(1.0_f32, FAINT));
                fp.text(Pos2::new(rect.left() - 6.0, sy), Align2::RIGHT_CENTER, tick_label(u.l(yv), u.l(step_cm)), small_font(), MUTED);
                if self.show_grid {
                    fp.hline(rect.x_range(), sy, gstroke);
                }
            }
            yv += step_cm;
        }
        fp.text(Pos2::new(rect.right(), rect.bottom() + 6.0 + 12.0), Align2::RIGHT_TOP, format!("x, {}", u.l_unit()), small_font(), FAINT);
        fp.text(Pos2::new(full.left() + 4.0, rect.top() - 4.0), Align2::LEFT_BOTTOM, format!("y, {}", u.l_unit()), small_font(), FAINT);

        let p = ui.painter_at(rect);
        // scale bar
        let bar_cm = [5.0f32, 10.0, 20.0, 25.0, 50.0, 100.0].iter().copied().find(|s| s * cell_px >= 90.0).unwrap_or(100.0);
        let bl = Pos2::new(rect.left() + 14.0, rect.bottom() - 14.0);
        let br = bl + Vec2::new(bar_cm * cell_px, 0.0);
        let sh = Stroke::new(3.0_f32, Color32::from_black_alpha(140));
        p.line_segment([bl, br], sh);
        p.line_segment([bl, br], Stroke::new(1.0_f32, Color32::from_white_alpha(220)));
        for e in [bl, br] {
            p.line_segment([e - Vec2::new(0.0, 4.0), e + Vec2::new(0.0, 1.0)], Stroke::new(1.0_f32, Color32::from_white_alpha(220)));
        }
        let lbl = if bar_cm >= 100.0 { format!("{:.0} m", bar_cm / 100.0) } else { format!("{bar_cm:.0} cm") };
        p.text(Pos2::new((bl.x + br.x) / 2.0, bl.y - 5.0), Align2::CENTER_BOTTOM, lbl, small_font(), Color32::from_white_alpha(220));

        // state labels (top-left inside viewport)
        let mut tags: Vec<(String, Color32)> = vec![];
        if self.paused {
            tags.push(("PAUSED".into(), WARN));
        }
        if let Some(r) = &self.run {
            match r.phase {
                Phase::Build => tags.push(("SETUP".into(), MUTED)),
                Phase::Test => tags.push((format!("RUN  t = {:.1} s", r.t), GOOD)),
                Phase::Done => tags.push((if r.passed { "RUN COMPLETE: PASS".into() } else { "RUN COMPLETE: FAIL".into() }, if r.passed { GOOD } else { BAD })),
            }
        }
        if let Some(pl) = self.place {
            tags.push((match pl {
                Place::Probe => "PLACE PROBE: click a cell".into(),
                Place::Line(None) => "LINE SAMPLER: click the start point".into(),
                Place::Line(Some(_)) => "LINE SAMPLER: click the end point".into(),
            }, ACCENT));
        }
        let mut tx = rect.left() + 10.0;
        for (t, c) in tags {
            let g = p.layout_no_wrap(t, head_font(), c);
            let r = Rect::from_min_size(Pos2::new(tx, rect.top() + 10.0), g.size() + Vec2::new(12.0, 6.0));
            p.rect_filled(r, CornerRadius::same(2), Color32::from_black_alpha(170));
            p.rect_stroke(r, CornerRadius::same(2), Stroke::new(1.0_f32, c.gamma_multiply(0.5)), StrokeKind::Inside);
            p.galley(r.min + Vec2::new(6.0, 3.0), g, c);
            tx = r.right() + 6.0;
        }

        // probes and samplers
        for l in &self.probes.lines {
            let a = to_screen(l.a.0 as f32 + 0.5, l.a.1 as f32 + 0.5);
            let b = to_screen(l.b.0 as f32 + 0.5, l.b.1 as f32 + 0.5);
            p.line_segment([a, b], Stroke::new(3.0_f32, Color32::from_black_alpha(120)));
            p.line_segment([a, b], Stroke::new(1.0_f32, l.color));
            for e in [a, b] {
                p.circle_filled(e, 2.5, l.color);
            }
            p.text(b + Vec2::new(6.0, -6.0), Align2::LEFT_BOTTOM, format!("L{}", l.id), small_font(), l.color);
        }
        for pr in &self.probes.probes {
            let c = to_screen(pr.x as f32 + 0.5, pr.y as f32 + 0.5);
            let s = Stroke::new(1.0_f32, pr.color);
            p.circle_stroke(c, 4.0, Stroke::new(3.0_f32, Color32::from_black_alpha(120)));
            p.circle_stroke(c, 4.0, s);
            for d in [Vec2::X, -Vec2::X, Vec2::Y, -Vec2::Y] {
                p.line_segment([c + d * 6.0, c + d * 10.0], s);
            }
            p.text(c + Vec2::new(8.0, -8.0), Align2::LEFT_BOTTOM, format!("P{}", pr.id), small_font(), pr.color);
        }

        // colour-map legend (right margin)
        if self.show_legend {
            let lr = Rect::from_min_max(Pos2::new(rect.right() + 14.0, rect.top() + 4.0), Pos2::new(full.right() - 4.0, rect.bottom()));
            self.legend(&fp, lr, stats);
        }
    }

    fn legend(&self, p: &egui::Painter, r: Rect, stats: &Stats) {
        let u = self.units;
        let bar = Rect::from_min_size(r.min + Vec2::new(0.0, 18.0), Vec2::new(10.0, (r.height() - 40.0).clamp(80.0, 300.0)));
        let grad = |f: &dyn Fn(f32) -> [f32; 3]| {
            let mut m = Mesh::default();
            let n = 48;
            for k in 0..=n {
                let t = k as f32 / n as f32;
                let y = bar.bottom() - t * bar.height();
                let c = c3(f(t));
                m.colored_vertex(Pos2::new(bar.left(), y), c);
                m.colored_vertex(Pos2::new(bar.right(), y), c);
                if k > 0 {
                    let i = (k * 2) as u32;
                    m.add_triangle(i - 2, i - 1, i);
                    m.add_triangle(i - 1, i, i + 1);
                }
            }
            p.add(EShape::mesh(m));
            p.rect_stroke(bar, CornerRadius::ZERO, Stroke::new(1.0_f32, BORDER), StrokeKind::Outside);
        };
        // ticks: values in display units at fractions along the bar
        let ticks = |vals: &[(f32, String)]| {
            for (f, s) in vals {
                let y = bar.bottom() - f.clamp(0.0, 1.0) * bar.height();
                p.hline(bar.right()..=bar.right() + 3.0, y, Stroke::new(1.0_f32, MUTED));
                p.text(Pos2::new(bar.right() + 5.0, y), Align2::LEFT_CENTER, s, small_font(), TEXT);
            }
        };
        let title = |t: &str, unit: &str| {
            p.text(r.min, Align2::LEFT_TOP, t, head_font(), MUTED);
            p.text(Pos2::new(r.left(), bar.bottom() + 6.0), Align2::LEFT_TOP, unit, small_font(), FAINT);
        };
        let marker = |f: f32| {
            let y = bar.bottom() - f.clamp(0.0, 1.0) * bar.height();
            p.add(EShape::convex_polygon(vec![Pos2::new(bar.left() - 1.0, y), Pos2::new(bar.left() - 6.0, y - 3.5), Pos2::new(bar.left() - 6.0, y + 3.5)], STRONG, Stroke::NONE));
        };
        match self.view {
            1 => {
                // pressure 0 .. 5 atm (diverging about 1 atm)
                let pmax = 5.0f32;
                title("Pressure", u.p_unit());
                grad(&|t| pressure_ramp(t * pmax));
                let vals: Vec<(f32, String)> = nice_ticks(0.0, u.p(pmax), 5).into_iter().map(|v| (v / u.p(pmax), tick_label(v, u.p(1.0)))).collect();
                ticks(&vals);
                marker(stats.max_p / pmax);
            }
            2 => {
                let (lo, hi) = (90.0f32, 3000.0f32);
                title("Temperature", u.t_unit());
                grad(&|t| heat_ramp(t));
                let (a, b) = (u.t(lo), u.t(hi));
                let vals: Vec<(f32, String)> = nice_ticks(a, b, 6).into_iter().map(|v| ((u.t_inv(v) - lo) / (hi - lo), tick_label(v, 100.0))).collect();
                ticks(&vals);
                marker((stats.max_t * 300.0 - lo) / (hi - lo));
            }
            3 => {
                let vmax = 2.5 * MPS_PER_UNIT;
                title("Speed |u|", u.v_unit());
                grad(&|t| [t, t, t]);
                let vals: Vec<(f32, String)> = nice_ticks(0.0, u.v(vmax), 5).into_iter().map(|v| (v / u.v(vmax), tick_label(v, 10.0))).collect();
                ticks(&vals);
                // direction wheel
                let c = Pos2::new(bar.center().x + 14.0, bar.bottom() + 40.0);
                for k in 0..36 {
                    let a0 = k as f32 / 36.0 * std::f32::consts::TAU;
                    let a1 = (k + 1) as f32 / 36.0 * std::f32::consts::TAU;
                    let h = k as f32 / 36.0;
                    let hc = {
                        let f = |n: f32| ((h + n).fract() * 6.0 - 3.0).abs() - 1.0;
                        [f(1.0).clamp(0.0, 1.0) * 0.85 + 0.15, f(2.0 / 3.0).clamp(0.0, 1.0) * 0.85 + 0.15, f(1.0 / 3.0).clamp(0.0, 1.0) * 0.85 + 0.15]
                    };
                    // screen y is down: hue angle atan2(vy_down, vx)
                    p.add(EShape::convex_polygon(vec![c, c + Vec2::new(a0.cos(), a0.sin()) * 12.0, c + Vec2::new(a1.cos(), a1.sin()) * 12.0], c3(hc), Stroke::NONE));
                }
                p.text(c + Vec2::new(0.0, 15.0), Align2::CENTER_TOP, "direction", small_font(), FAINT);
            }
            4 => {
                title("Species", "mole fraction");
                let items = [([1.0, 0.9, 0.1], "CH₄ (fuel)"), ([0.1, 0.75, 1.0], "O₂ (excess)"), ([0.92, 0.94, 0.97], "CO₂ + H₂O"), ([0.10, 0.095, 0.09], "soot")];
                for (k, (c, n)) in items.iter().enumerate() {
                    let y = r.top() + 24.0 + k as f32 * 18.0;
                    let sw = Rect::from_min_size(Pos2::new(r.left(), y), Vec2::splat(10.0));
                    p.rect_filled(sw, CornerRadius::ZERO, c3(*c));
                    p.rect_stroke(sw, CornerRadius::ZERO, Stroke::new(1.0_f32, BORDER), StrokeKind::Outside);
                    p.text(Pos2::new(r.left() + 15.0, y + 5.0), Align2::LEFT_CENTER, *n, small_font(), TEXT);
                }
                p.text(Pos2::new(r.left(), r.top() + 100.0), Align2::LEFT_TOP, "additive mix,\nbrightness ∝ fraction", small_font(), FAINT);
            }
            5 => {
                title("Wall stress", "% of yield");
                grad(&|t| stress_ramp(t * 1.2));
                let vals: Vec<(f32, String)> = [0.0f32, 25.0, 50.0, 75.0, 100.0, 120.0].iter().map(|v| (v / 120.0, format!("{v:.0}"))).collect();
                ticks(&vals);
                marker(stats.max_stress / 1.2);
            }
            _ => {
                let (lo, hi) = (800.0f32, 3000.0f32);
                title("Emission", u.t_unit());
                grad(&|t| glow_ramp(lo + t * (hi - lo)));
                let vals: Vec<(f32, String)> = [1000.0f32, 1500.0, 2000.0, 2500.0, 3000.0].iter().map(|k| ((k - lo) / (hi - lo), format!("{:.0}", u.t(*k)))).collect();
                ticks(&vals);
                p.text(Pos2::new(r.left(), bar.bottom() + 20.0), Align2::LEFT_TOP, "blackbody hue\n(qualitative)", small_font(), FAINT);
            }
        }
    }

    // ------------------------------------------------------------------------------------- floating windows
    pub(super) fn windows(&mut self, ctx: &egui::Context, frame: &eframe::Frame) {
        if ctx.input(|i| i.modifiers.command && i.key_pressed(egui::Key::Comma)) {
            self.show_settings = true;
        }
        // one modal at a time: whichever was opened last wins
        let mask = (self.show_settings as u8) | (self.show_library as u8) << 1 | (self.show_codex as u8) << 2 | (self.show_experiments as u8) << 3 | (self.show_shortcuts as u8) << 4 | (self.show_about as u8) << 5;
        let newly = mask & !self.modal_mask;
        let mask = if newly != 0 && mask != newly { 1u8 << (7 - newly.leading_zeros()) } else { mask };
        self.show_settings = mask & 1 != 0;
        self.show_library = mask & 2 != 0;
        self.show_codex = mask & 4 != 0;
        self.show_experiments = mask & 8 != 0;
        self.show_shortcuts = mask & 16 != 0;
        self.show_about = mask & 32 != 0;
        if !self.show_codex {
            self.codex.open = false;
        }
        if mask != 0 && !self.tutorial.is_active() {
            let scr = ctx.screen_rect();
            let r = egui::Area::new(egui::Id::new("modal_scrim")).order(egui::Order::Middle).fixed_pos(scr.min).interactable(true).show(ctx, |ui| {
                let (r, resp) = ui.allocate_exact_size(scr.size(), Sense::click());
                ui.painter().rect_filled(r, CornerRadius::ZERO, Color32::from_black_alpha(105));
                resp
            });
            if r.inner.clicked() {
                self.show_settings = false;
                self.show_library = false;
                self.show_codex = false;
                self.codex.open = false;
                self.show_experiments = false;
                self.show_shortcuts = false;
                self.show_about = false;
            }
        }
        if self.show_settings {
            let mut open = true;
            let mut changed = false;
            let h = (ctx.screen_rect().height() - 160.0).clamp(300.0, 580.0);
            egui::Window::new("Preferences").open(&mut open).collapsible(false).resizable(false).fixed_size([680.0, h]).anchor(Align2::CENTER_CENTER, Vec2::ZERO).show(ctx, |ui| {
                ui.horizontal_top(|ui| {
                    ui.vertical(|ui| {
                        ui.set_width(140.0);
                        for (k, n) in [(0u8, "Simulation & display"), (1u8, "Units")] {
                            if ui.add_sized([140.0, 22.0], egui::SelectableLabel::new(self.pref_tab == k, n)).clicked() {
                                self.pref_tab = k;
                            }
                        }
                    });
                    ui.separator();
                    ui.vertical(|ui| {
                        egui::ScrollArea::vertical().id_salt("prefs").auto_shrink([false, false]).show(ui, |ui| {
                            ui.set_max_width(ui.available_width() - 12.0);
                            if self.pref_tab == 0 {
                                changed = crate::settings::show(ui, &mut self.settings);
                            } else {
                                ui.label(RichText::new("Display units").font(head_font()).color(STRONG));
                                ui.label(muted("Applies to every readout, plot, legend and export. The solver always works in SI."));
                                ui.add_space(6.0);
                                self.units_menu(ui);
                            }
                        });
                    });
                });
            });
            if changed {
                let _ = crate::settings::save(&self.settings);
            }
            self.show_settings = open;
        }
        if self.show_library {
            let mut open = true;
            if let Some(a) = self.library.window(ctx, &mut open) {
                self.library_action(a, frame);
            }
            self.show_library = open;
        }
        if self.show_codex {
            self.codex.open = true;
            let r = crate::codex::show_window(ctx, &mut self.codex);
            self.codex_response(r);
            self.show_codex = self.codex.open;
        }
        if self.show_experiments {
            let mut open = true;
            egui::Window::new("Experiments / Lab exercises").open(&mut open).default_size([720.0, 520.0]).collapsible(false).show(ctx, |ui| {
                ui.label(muted("Guided lab exercises. Each defines an apparatus, the materials available, inlet conditions and measurable objectives."));
                ui.add_space(6.0);
                self.experiment_list(ui, "explist_win");
            });
            self.show_experiments = open && self.run.as_ref().map(|_| true).unwrap_or(true);
        }
        if self.show_shortcuts {
            let mut open = true;
            egui::Window::new("Keyboard shortcuts").open(&mut open).default_width(420.0).collapsible(false).resizable(false).show(ctx, |ui| {
                egui::ScrollArea::vertical().max_height(460.0).show(ui, |ui| {
                    egui::Grid::new("keys").num_columns(2).striped(true).spacing([24.0, 3.0]).show(ui, |ui| {
                        let mut group = "";
                        for a in crate::settings::Action::ALL {
                            if a.group() != group {
                                group = a.group();
                                ui.label(RichText::new(group.to_uppercase()).font(head_font()).color(MUTED));
                                ui.end_row();
                            }
                            ui.label(a.label());
                            ui.label(num(self.settings.key_label(*a)));
                            ui.end_row();
                        }
                        ui.label(RichText::new("VIEWPORT").font(head_font()).color(MUTED));
                        ui.end_row();
                        for (k, v) in [("Paint / draw", "Left drag"), ("Erase", "Right drag"), ("Pick material", "Alt + click"), ("Zoom", "Pinch or ⌘ + scroll"), ("Pan", "Scroll or middle drag"), ("Preferences", "⌘,")] {
                            ui.label(k);
                            ui.label(num(v));
                            ui.end_row();
                        }
                    });
                });
            });
            self.show_shortcuts = open;
        }
        if self.show_about {
            let mut open = true;
            egui::Window::new("About Ignis").open(&mut open).collapsible(false).resizable(false).default_width(380.0).show(ctx, |ui| {
                ui.label(RichText::new("Ignis").font(FontId::new(18.0, fam("semibold"))).color(STRONG));
                ui.label(muted("Two-dimensional reacting-flow and materials laboratory"));
                ui.add_space(6.0);
                sheet(ui, "about", |ui| {
                    kv(ui, "Grid", format!("{W} × {H} cells, Δx = 1 cm"));
                    kv(ui, "Solver", "HLLC + MUSCL, RK2, LES (GPU, wgpu/Metal)");
                    kv(ui, "Chemistry", "4-species CH₄/O₂/N₂/products, Arrhenius + EDC, soot");
                    kv(ui, "Time step", format!("{:.3} µs", DT * MS_PER_UNIT * 1000.0));
                    kv(ui, "Materials", format!("{} entries", elements::all().count()));
                    kv(ui, "Reference entries", format!("{}", crate::codex::all().len()));
                });
            });
            self.show_about = open;
        }
        for t in ["Preferences", "Design library", "Reference manual", "Experiments / Lab exercises", "Keyboard shortcuts", "About Ignis"] {
            ctx.move_to_top(egui::LayerId::new(egui::Order::Middle, egui::Id::new(t)));
        }
        self.modal_mask = (self.show_settings as u8) | (self.show_library as u8) << 1 | (self.show_codex as u8) << 2 | (self.show_experiments as u8) << 3 | (self.show_shortcuts as u8) << 4 | (self.show_about as u8) << 5;
        // guided tour overlay
        if self.tutorial.is_active() {
            if let Some(a) = self.tutorial.draw_overlay(ctx, &self.tut_rects) {
                match a {
                    crate::tutorial::OverlayAction::StartLesson(n) => self.start_lesson(n),
                    a => self.tutorial.on_event(a.into()),
                }
                if std::env::var("COMBUSTOR_SHOT").is_err() {
                    if let Some(d) = data_dir() {
                        let _ = std::fs::create_dir_all(&d);
                        let _ = std::fs::write(d.join("tutorial.txt"), self.tutorial.save_string());
                    }
                }
            }
        }
    }

    fn library_action(&mut self, a: BrowserAction, frame: &eframe::Frame) {
        match a {
            BrowserAction::Load(id) => match self.library.lib.load(&id) {
                Ok(d) => {
                    let name = d.info.name.clone();
                    self.load_design(&name, d);
                    self.show_library = false;
                }
                Err(e) => self.log_warn(&format!("Could not load design: {e}")),
            },
            BrowserAction::SaveCurrent(info) => {
                let Some(meta) = self.current_meta(frame) else { return };
                let thumb = crate::library::render_thumb(W, H, &meta, &lib_palette());
                let name = info.name.clone();
                let d = crate::library::Design::new(W, H, meta, self.design_params(), info);
                match self.library.lib.save_design(d, Some(&thumb)) {
                    Ok(_) => {
                        self.library.refresh();
                        self.toast(&format!("Saved \"{name}\" to the design library"));
                    }
                    Err(e) => self.log_warn(&format!("Library save failed: {e}")),
                }
            }
            BrowserAction::Overwrite(id) => {
                let Some(meta) = self.current_meta(frame) else { return };
                let thumb = crate::library::render_thumb(W, H, &meta, &lib_palette());
                match self.library.lib.overwrite(&id, &meta, self.design_params(), None, Some(&thumb)) {
                    Ok(_) => {
                        self.library.refresh();
                        self.toast("Design updated");
                    }
                    Err(e) => self.log_warn(&format!("Library save failed: {e}")),
                }
            }
        }
    }

    // ------------------------------------------------------------------------------------- feeds for audio / tutorial
    pub(super) fn feed_audio(&mut self, stats: &Stats, dt: f32) {
        self.hist_raw_thrust = Vec2::new(stats.thrust[0], stats.thrust[1]);
        let Some(a) = self.audio.as_mut() else { return };
        let s = &self.settings;
        a.set_volume(s.master_volume);
        a.set_muted(s.muted);
        let snap = crate::audio::AudioSnapshot {
            dt,
            paused: self.paused,
            sim_speed: self.speed,
            max_p_atm: stats.max_p,
            max_t_k: stats.max_t * 300.0,
            burn_kgs: stats.burn * KGS_PER_UNIT,
            thrust_kn: (Vec2::new(stats.thrust[0], stats.thrust[1]) - self.tare).length() * KN_PER_UNIT,
            walls: stats.walls,
            max_stress: stats.max_stress,
            max_wall_heat: stats.max_wall_heat,
            inlet: if self.engine_scene { ((self.p_fuel - 1.0).max(0.0) + (self.p_o2 - 1.0).max(0.0)) / 20.0 } else { 0.0 },
            boil: 0.0,
            explosion: 0.0,
            scene_reset: std::mem::take(&mut self.audio_reset),
        };
        a.update(&snap);
    }

    pub(super) fn feed_tutorial(&mut self, _ctx: &egui::Context, stats: &Stats, dt: f32) {
        if !self.tutorial.is_active() {
            return;
        }
        use crate::tutorial::{Stat as TS, TutorialEvent as E};
        let t = &mut self.tutorial;
        t.on_event(E::Tick(dt));
        t.on_event(E::PauseState(self.paused));
        t.on_event(E::Stat(TS::Thrust, (Vec2::new(stats.thrust[0], stats.thrust[1]) - self.tare).length() * KN_PER_UNIT));
        t.on_event(E::Stat(TS::MaxPressure, stats.max_p));
        t.on_event(E::Stat(TS::MaxTemp, stats.max_t * 300.0));
        t.on_event(E::Stat(TS::Burn, stats.burn));
        t.on_event(E::Stat(TS::WallStress, stats.max_stress));
        t.on_event(E::Stat(TS::WallHeat, stats.max_wall_heat));
        t.on_event(E::Stat(TS::FuelFeed, self.p_fuel));
        t.on_event(E::Stat(TS::Speed, self.speed));
        if stats.burn > 0.01 {
            t.on_event(E::Ignited);
        }
    }
}

/// Thumbnail colours for the design library, from the element catalogue.
fn lib_palette() -> crate::library::Palette {
    let mut p = crate::library::Palette::default();
    for e in elements::all() {
        let c = [e.color.r(), e.color.g(), e.color.b()];
        match e.tool {
            Tool::Wall(m) => p.set_wall(m, c),
            Tool::Matter(id) => p.set_matter(id, c, elements::matter_info(id).map(|i| i.phase == 2).unwrap_or(false)),
            Tool::Part(t) => p.set_tile(t, c),
            _ => {}
        }
    }
    p
}
