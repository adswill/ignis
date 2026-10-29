//! Ignis player settings: graphics quality, accessibility (UI scale, colour-blind palettes, reduced
//! flashes/motion), audio, key bindings, zoom, frame pacing and battery saver.
//!
//! Self-contained: depends only on `eframe::egui` and std (no crate-internal types), with a tiny
//! built-in JSON serializer/parser. Persisted to
//! `~/Library/Application Support/Ignis/settings.json` (migrates the old
//! `Application Support/Combustor/settings.txt` on first run).
//!
//! Main entry points:
//! - [`load`] / [`save`] (and [`Settings::from_json`] / [`Settings::to_json`])
//! - [`show`] — the settings panel for any `egui::Ui`; returns `true` when something changed
//! - [`show_window`] — the same panel in a floating window
//! - [`Settings::graphics`] — effective plain knobs (preset + battery saver applied)
//! - [`Settings::palette`] — plain colour parameters for gas / heat colouring (shader-ready floats)
//! - [`Settings::pressed`] — "was this action's key chord pressed this frame?"
//! - [`is_capturing`] — true while the player is rebinding a key (skip game shortcuts then)
#![allow(dead_code)]

use eframe::egui;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Duration;

pub const SETTINGS_VERSION: u32 = 1;

// =====================================================================================
// Graphics
// =====================================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Quality {
    Low,
    Medium,
    High,
    Ultra,
    /// Hand-tuned knobs (stored in `Settings::custom_graphics`).
    Custom,
}

impl Quality {
    pub const ALL: [Quality; 5] = [Quality::Low, Quality::Medium, Quality::High, Quality::Ultra, Quality::Custom];
    pub fn id(self) -> &'static str {
        match self {
            Quality::Low => "low",
            Quality::Medium => "medium",
            Quality::High => "high",
            Quality::Ultra => "ultra",
            Quality::Custom => "custom",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Quality::Low => "Low",
            Quality::Medium => "Medium",
            Quality::High => "High",
            Quality::Ultra => "Ultra",
            Quality::Custom => "Custom",
        }
    }
    pub fn from_id(s: &str) -> Option<Quality> {
        Quality::ALL.iter().copied().find(|q| q.id() == s)
    }
    /// Knobs for a preset (Custom returns High; use `Settings::graphics` for the real values).
    pub fn knobs(self) -> GraphicsKnobs {
        match self {
            Quality::Low => GraphicsKnobs {
                particle_fraction: 0.25,
                bloom: false,
                wide_bloom: false,
                render_scale: 0.75,
                heat_haze: false,
                fx_detail: false,
                max_steps_per_frame: 16,
            },
            Quality::Medium => GraphicsKnobs {
                particle_fraction: 0.5,
                bloom: true,
                wide_bloom: false,
                render_scale: 1.0,
                heat_haze: false,
                fx_detail: true,
                max_steps_per_frame: 32,
            },
            Quality::High | Quality::Custom => GraphicsKnobs {
                particle_fraction: 0.8,
                bloom: true,
                wide_bloom: true,
                render_scale: 1.0,
                heat_haze: true,
                fx_detail: true,
                max_steps_per_frame: 48,
            },
            Quality::Ultra => GraphicsKnobs {
                particle_fraction: 1.0,
                bloom: true,
                wide_bloom: true,
                render_scale: 1.5,
                heat_haze: true,
                fx_detail: true,
                max_steps_per_frame: 64,
            },
        }
    }
}

/// Plain rendering / simulation knobs the renderer and the frame loop can read directly.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GraphicsKnobs {
    /// Fraction (0..1) of the maximum tracer / spark particle count to spawn.
    pub particle_fraction: f32,
    /// Tight bloom pass (`blurh/blurv`).
    pub bloom: bool,
    /// Quarter-res wide bloom (`bdown/bwideh/bwidev`). Ignored when `bloom` is off.
    pub wide_bloom: bool,
    /// Supersampling factor for the canvas render target (0.5..2.0, 1 = native).
    pub render_scale: f32,
    /// Heat shimmer / refraction in render.wgsl.
    pub heat_haze: bool,
    /// Advected detail textures in fx.wgsl (flame / smoke micro-structure).
    pub fx_detail: bool,
    /// Upper bound for sim steps per rendered frame (app's MAX_STEPS is 64). Lower = the sim slows
    /// down instead of the frame rate when the GPU is busy.
    pub max_steps_per_frame: u32,
}

impl GraphicsKnobs {
    fn clamp(mut self) -> Self {
        self.particle_fraction = self.particle_fraction.clamp(0.0, 1.0);
        self.render_scale = self.render_scale.clamp(0.5, 2.0);
        self.max_steps_per_frame = self.max_steps_per_frame.clamp(1, 64);
        self
    }
}

// =====================================================================================
// Colour-blind palettes
// =====================================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PaletteMode {
    /// Physically-motivated colours (blackbody heat ramp).
    Natural,
    /// Red-green safe (deuteranopia, the most common form).
    Deuteranopia,
    /// Red-green safe, tuned for reduced red sensitivity.
    Protanopia,
    /// Blue-yellow safe.
    Tritanopia,
    /// Luminance only + species hatching hints.
    Monochrome,
    /// Maximum separation, saturated colours on dark.
    HighContrast,
}

impl PaletteMode {
    pub const ALL: [PaletteMode; 6] = [
        PaletteMode::Natural,
        PaletteMode::Deuteranopia,
        PaletteMode::Protanopia,
        PaletteMode::Tritanopia,
        PaletteMode::Monochrome,
        PaletteMode::HighContrast,
    ];
    pub fn id(self) -> &'static str {
        match self {
            PaletteMode::Natural => "natural",
            PaletteMode::Deuteranopia => "deuteranopia",
            PaletteMode::Protanopia => "protanopia",
            PaletteMode::Tritanopia => "tritanopia",
            PaletteMode::Monochrome => "monochrome",
            PaletteMode::HighContrast => "high_contrast",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            PaletteMode::Natural => "Natural (blackbody)",
            PaletteMode::Deuteranopia => "Deuteranopia (red-green)",
            PaletteMode::Protanopia => "Protanopia (red-weak)",
            PaletteMode::Tritanopia => "Tritanopia (blue-yellow)",
            PaletteMode::Monochrome => "Monochrome",
            PaletteMode::HighContrast => "High contrast",
        }
    }
    pub fn from_id(s: &str) -> Option<PaletteMode> {
        PaletteMode::ALL.iter().copied().find(|m| m.id() == s)
    }
    /// Index usable as a shader switch (0..=5, in `ALL` order).
    pub fn index(self) -> u32 {
        PaletteMode::ALL.iter().position(|m| *m == self).unwrap_or(0) as u32
    }
}

/// Shader-ready colour parameters for the gas / heat views. `#[repr(C)]`, all `[f32; 4]`, so it can be
/// written straight into a uniform buffer with `bytemuck::cast_slice(&p.to_floats())` (52 floats =
/// 208 bytes, 13 vec4 rows; WGSL: `struct Palette { heat: array<vec4<f32>,6>, fuel: vec4<f32>, ... misc: vec4<f32> }`).
///
/// Heat ramp: 6 stops, `rgb` linear colour, `a` = normalised position along T (0 = ambient 300 K,
/// 1 = the T cap ≈ 3600 K). Shaders lerp between consecutive stops.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PaletteParams {
    pub heat: [[f32; 4]; 6],
    /// Species tints (linear rgb, a = opacity weight): fuel, O2, N2, products (CO2/H2O), soot.
    pub fuel: [f32; 4],
    pub oxygen: [f32; 4],
    pub nitrogen: [f32; 4],
    pub products: [f32; 4],
    pub soot: [f32; 4],
    /// Flame emission tint multiplier (rgb) and a = emission gain.
    pub flame: [f32; 4],
    /// x = mode index, y = hatch/pattern strength (0..1) to separate species without colour,
    /// z = isoline spacing in normalised T (0 = off), w = saturation multiplier.
    pub misc: [f32; 4],
}

impl PaletteParams {
    /// 13 rows × 4 floats (heat[0..6], fuel, oxygen, nitrogen, products, soot, flame, misc).
    pub fn to_floats(&self) -> [f32; 52] {
        let rows: [[f32; 4]; 13] = [
            self.heat[0], self.heat[1], self.heat[2], self.heat[3], self.heat[4], self.heat[5], self.fuel, self.oxygen, self.nitrogen,
            self.products, self.soot, self.flame, self.misc,
        ];
        let mut out = [0.0f32; 52];
        for (r, row) in rows.iter().enumerate() {
            out[r * 4..r * 4 + 4].copy_from_slice(row);
        }
        out
    }
    /// Sample the heat ramp at normalised temperature `t` (0..1) — CPU mirror of the shader lerp,
    /// used for UI previews / legends.
    pub fn heat_at(&self, t: f32) -> [f32; 3] {
        let t = t.clamp(0.0, 1.0);
        for w in self.heat.windows(2) {
            let (a, b) = (w[0], w[1]);
            if t <= b[3] {
                let f = if b[3] > a[3] { (t - a[3]) / (b[3] - a[3]) } else { 0.0 };
                return [a[0] + (b[0] - a[0]) * f, a[1] + (b[1] - a[1]) * f, a[2] + (b[2] - a[2]) * f];
            }
        }
        let l = self.heat[5];
        [l[0], l[1], l[2]]
    }
}

/// sRGB 0..255 -> linear 0..1.
fn lin(c: u8) -> f32 {
    let c = c as f32 / 255.0;
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}
fn rgb(r: u8, g: u8, b: u8, a: f32) -> [f32; 4] {
    [lin(r), lin(g), lin(b), a]
}
fn stop(r: u8, g: u8, b: u8, pos: f32) -> [f32; 4] {
    rgb(r, g, b, pos)
}
fn to_srgb_u8(v: f32) -> u8 {
    let v = v.clamp(0.0, 1.0);
    let s = if v <= 0.0031308 { v * 12.92 } else { 1.055 * v.powf(1.0 / 2.4) - 0.055 };
    (s * 255.0).round() as u8
}

pub fn palette_params(mode: PaletteMode) -> PaletteParams {
    // Okabe-Ito colours (safe for all common CVD types): orange E69F00, sky 56B4E9, green 009E73,
    // yellow F0E442, blue 0072B2, vermillion D55E00, purple CC79A7.
    let idx = mode.index() as f32;
    match mode {
        PaletteMode::Natural => PaletteParams {
            heat: [
                stop(0, 0, 0, 0.0),
                stop(120, 12, 4, 0.15),
                stop(230, 70, 10, 0.35),
                stop(255, 170, 40, 0.55),
                stop(255, 240, 200, 0.8),
                stop(210, 225, 255, 1.0),
            ],
            fuel: rgb(240, 200, 120, 0.6),
            oxygen: rgb(110, 170, 255, 0.6),
            nitrogen: rgb(150, 150, 160, 0.2),
            products: rgb(200, 205, 215, 0.5),
            soot: rgb(25, 22, 20, 1.0),
            flame: [1.0, 1.0, 1.0, 1.0],
            misc: [idx, 0.0, 0.0, 1.0],
        },
        // Inferno-like ramp (perceptually uniform, CVD-safe), Okabe-Ito species.
        PaletteMode::Deuteranopia => PaletteParams {
            heat: [
                stop(0, 0, 4, 0.0),
                stop(66, 10, 104, 0.18),
                stop(147, 38, 103, 0.38),
                stop(221, 81, 58, 0.58),
                stop(252, 165, 10, 0.8),
                stop(252, 255, 164, 1.0),
            ],
            fuel: rgb(230, 159, 0, 0.7),
            oxygen: rgb(0, 114, 178, 0.7),
            nitrogen: rgb(150, 150, 150, 0.2),
            products: rgb(240, 228, 66, 0.5),
            soot: rgb(20, 20, 20, 1.0),
            flame: [1.0, 0.95, 0.85, 1.0],
            misc: [idx, 0.15, 0.0, 1.0],
        },
        // Same idea but brighter at the hot end (protans see reds darker).
        PaletteMode::Protanopia => PaletteParams {
            heat: [
                stop(0, 0, 4, 0.0),
                stop(40, 30, 120, 0.18),
                stop(90, 80, 180, 0.38),
                stop(200, 160, 60, 0.58),
                stop(250, 220, 40, 0.8),
                stop(255, 255, 220, 1.0),
            ],
            fuel: rgb(230, 159, 0, 0.7),
            oxygen: rgb(86, 180, 233, 0.7),
            nitrogen: rgb(150, 150, 150, 0.2),
            products: rgb(204, 121, 167, 0.5),
            soot: rgb(20, 20, 20, 1.0),
            flame: [0.9, 1.0, 1.0, 1.15],
            misc: [idx, 0.15, 0.0, 1.0],
        },
        // Avoid blue/yellow confusions: dark -> red -> pink -> white ramp, teal / red species.
        PaletteMode::Tritanopia => PaletteParams {
            heat: [
                stop(0, 0, 0, 0.0),
                stop(90, 10, 20, 0.18),
                stop(190, 30, 45, 0.38),
                stop(240, 90, 110, 0.58),
                stop(255, 180, 190, 0.8),
                stop(255, 250, 250, 1.0),
            ],
            fuel: rgb(213, 94, 0, 0.7),
            oxygen: rgb(0, 158, 115, 0.7),
            nitrogen: rgb(150, 150, 150, 0.2),
            products: rgb(204, 121, 167, 0.5),
            soot: rgb(20, 20, 20, 1.0),
            flame: [1.0, 0.85, 0.9, 1.0],
            misc: [idx, 0.15, 0.0, 1.0],
        },
        PaletteMode::Monochrome => PaletteParams {
            heat: [
                stop(0, 0, 0, 0.0),
                stop(50, 50, 50, 0.18),
                stop(100, 100, 100, 0.38),
                stop(160, 160, 160, 0.58),
                stop(215, 215, 215, 0.8),
                stop(255, 255, 255, 1.0),
            ],
            fuel: rgb(230, 230, 230, 0.7),
            oxygen: rgb(120, 120, 120, 0.7),
            nitrogen: rgb(80, 80, 80, 0.2),
            products: rgb(180, 180, 180, 0.5),
            soot: rgb(10, 10, 10, 1.0),
            flame: [1.0, 1.0, 1.0, 1.0],
            misc: [idx, 1.0, 0.1, 0.0],
        },
        PaletteMode::HighContrast => PaletteParams {
            heat: [
                stop(0, 0, 0, 0.0),
                stop(0, 0, 255, 0.18),
                stop(0, 255, 255, 0.38),
                stop(255, 255, 0, 0.58),
                stop(255, 0, 255, 0.8),
                stop(255, 255, 255, 1.0),
            ],
            fuel: rgb(255, 255, 0, 0.9),
            oxygen: rgb(0, 200, 255, 0.9),
            nitrogen: rgb(120, 120, 120, 0.25),
            products: rgb(255, 0, 255, 0.7),
            soot: rgb(0, 0, 0, 1.0),
            flame: [1.0, 1.0, 1.0, 1.2],
            misc: [idx, 0.5, 0.125, 1.3],
        },
    }
}

// =====================================================================================
// Key bindings
// =====================================================================================

/// A key plus exact modifier state ("Cmd" = egui's `command`, i.e. ⌘ on macOS / Ctrl elsewhere).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Chord {
    pub key: egui::Key,
    pub cmd: bool,
    pub shift: bool,
    pub alt: bool,
}

impl Chord {
    pub const fn plain(key: egui::Key) -> Chord {
        Chord { key, cmd: false, shift: false, alt: false }
    }
    pub const fn cmd(key: egui::Key) -> Chord {
        Chord { key, cmd: true, shift: false, alt: false }
    }
    pub const fn shift(key: egui::Key) -> Chord {
        Chord { key, cmd: false, shift: true, alt: false }
    }
    pub const fn cmd_shift(key: egui::Key) -> Chord {
        Chord { key, cmd: true, shift: true, alt: false }
    }
    /// "Cmd+Shift+Z" style (stable, used in the JSON file).
    pub fn to_string_id(&self) -> String {
        let mut s = String::new();
        if self.cmd {
            s.push_str("Cmd+");
        }
        if self.alt {
            s.push_str("Alt+");
        }
        if self.shift {
            s.push_str("Shift+");
        }
        s.push_str(self.key.name());
        s
    }
    pub fn parse(s: &str) -> Option<Chord> {
        let mut c = Chord::plain(egui::Key::Space);
        let mut parts: Vec<&str> = s.split('+').map(|p| p.trim()).collect();
        // "Cmd++" style (Plus key) -> last part empty
        if s.ends_with("++") {
            parts.retain(|p| !p.is_empty());
            parts.push("+");
        }
        let key = parts.pop()?;
        for m in parts {
            match m.to_ascii_lowercase().as_str() {
                "cmd" | "ctrl" | "command" | "⌘" => c.cmd = true,
                "shift" | "⇧" => c.shift = true,
                "alt" | "option" | "opt" | "⌥" => c.alt = true,
                "" => {}
                _ => return None,
            }
        }
        c.key = egui::Key::from_name(key)?;
        Some(c)
    }
    /// Short label for buttons, macOS style ("⌘⇧Z", "Space").
    pub fn label(&self) -> String {
        let mut s = String::new();
        if self.cmd {
            s.push('⌘');
        }
        if self.alt {
            s.push('⌥');
        }
        if self.shift {
            s.push('⇧');
        }
        let k = self.key.symbol_or_name();
        if !s.is_empty() && k.chars().count() > 1 {
            s.push(' ');
        }
        s.push_str(k);
        s
    }
    /// Pressed this frame with exactly these modifiers?
    pub fn pressed(&self, i: &egui::InputState) -> bool {
        i.key_pressed(self.key) && i.modifiers.command == self.cmd && i.modifiers.shift == self.shift && i.modifiers.alt == self.alt
    }
    /// Held down right now (modifiers ignored) — for continuous actions.
    pub fn down(&self, i: &egui::InputState) -> bool {
        i.key_down(self.key)
    }
}

macro_rules! actions {
    ($( $v:ident = ($id:literal, $label:literal, $group:literal, [$($def:expr),*]) ),* $(,)?) => {
        /// Every rebindable action. Defaults match the controls in app.rs `shortcuts()`.
        #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub enum Action { $($v),* }
        impl Action {
            pub const ALL: &'static [Action] = &[$(Action::$v),*];
            /// Stable id used in settings.json.
            pub fn id(self) -> &'static str { match self { $(Action::$v => $id),* } }
            pub fn label(self) -> &'static str { match self { $(Action::$v => $label),* } }
            pub fn group(self) -> &'static str { match self { $(Action::$v => $group),* } }
            pub fn defaults(self) -> Vec<Chord> {
                use egui::Key::*;
                #[allow(unused_imports)]
                use Chord as C;
                match self { $(Action::$v => vec![$($def),*]),* }
            }
            pub fn from_id(s: &str) -> Option<Action> { Action::ALL.iter().copied().find(|a| a.id() == s) }
        }
    };
}

actions! {
    Pause        = ("pause", "Pause / resume", "Simulation", [C::plain(Space)]),
    Step         = ("step", "Step one frame", "Simulation", [C::plain(Period)]),
    StartTest    = ("start_test", "Start mission test", "Simulation", [C::plain(Enter)]),
    ToggleIgniters = ("toggle_igniters", "Igniters on / off", "Simulation", [C::plain(I)]),
    View1        = ("view_1", "View 1: Render", "Views", [C::plain(Num1)]),
    View2        = ("view_2", "View 2: Pressure", "Views", [C::plain(Num2)]),
    View3        = ("view_3", "View 3: Temperature", "Views", [C::plain(Num3)]),
    View4        = ("view_4", "View 4: Velocity", "Views", [C::plain(Num4)]),
    View5        = ("view_5", "View 5: Species", "Views", [C::plain(Num5)]),
    View6        = ("view_6", "View 6: Stress", "Views", [C::plain(Num6)]),
    ResetZoom    = ("reset_zoom", "Reset zoom", "Views", [C::plain(Num0)]),
    ShapeBrush   = ("shape_brush", "Brush", "Drawing", [C::plain(B)]),
    ShapeLine    = ("shape_line", "Line", "Drawing", [C::plain(L)]),
    ShapeBox     = ("shape_box", "Filled box", "Drawing", [C::plain(X)]),
    ShapeFrame   = ("shape_frame", "Hollow frame", "Drawing", [C::plain(F)]),
    ShapeFill    = ("shape_fill", "Flood fill", "Drawing", [C::plain(G)]),
    ShapeSpray   = ("shape_spray", "Spray", "Drawing", [C::plain(S)]),
    BrushSmaller = ("brush_smaller", "Brush smaller", "Drawing", [C::plain(OpenBracket)]),
    BrushLarger  = ("brush_larger", "Brush larger", "Drawing", [C::plain(CloseBracket)]),
    RotateFan    = ("rotate_fan", "Rotate fan direction", "Drawing", [C::plain(R)]),
    Eraser       = ("eraser", "Eraser", "Tools", [C::plain(E)]),
    MixTool      = ("mix", "Mix / stir", "Tools", [C::plain(M)]),
    CastTool     = ("cast", "Cast / press", "Tools", [C::plain(C)]),
    Designer     = ("designer", "Propellant designer", "Tools", [C::plain(D)]),
    PrevTool     = ("prev_tool", "Previous tool", "Tools", [C::plain(Q)]),
    Picker       = ("picker", "Element picker", "Tools", [C::plain(P)]),
    Book         = ("book", "Element book", "Tools", [C::plain(K)]),
    Search       = ("search", "Search elements", "Tools", [C::plain(Slash), C::cmd(F)]),
    Fav1         = ("fav_1", "Favourite 1", "Favourites", [C::shift(Num1)]),
    Fav2         = ("fav_2", "Favourite 2", "Favourites", [C::shift(Num2)]),
    Fav3         = ("fav_3", "Favourite 3", "Favourites", [C::shift(Num3)]),
    Fav4         = ("fav_4", "Favourite 4", "Favourites", [C::shift(Num4)]),
    Fav5         = ("fav_5", "Favourite 5", "Favourites", [C::shift(Num5)]),
    Fav6         = ("fav_6", "Favourite 6", "Favourites", [C::shift(Num6)]),
    Fav7         = ("fav_7", "Favourite 7", "Favourites", [C::shift(Num7)]),
    Fav8         = ("fav_8", "Favourite 8", "Favourites", [C::shift(Num8)]),
    Fav9         = ("fav_9", "Favourite 9", "Favourites", [C::shift(Num9)]),
    Drawer       = ("drawer", "Toggle side drawer", "Interface", [C::plain(Tab)]),
    Help         = ("help", "Help", "Interface", [C::plain(H), C::plain(F1)]),
    Undo         = ("undo", "Undo", "File", [C::cmd(Z)]),
    Redo         = ("redo", "Redo", "File", [C::cmd_shift(Z)]),
    Save         = ("save", "Save scene", "File", [C::cmd(S)]),
    Open         = ("open", "Open scene", "File", [C::cmd(O)]),
}

impl Action {
    /// Favourite slot 0..9 for Fav1..Fav9.
    pub fn fav_slot(self) -> Option<usize> {
        const F: [Action; 9] = [Action::Fav1, Action::Fav2, Action::Fav3, Action::Fav4, Action::Fav5, Action::Fav6, Action::Fav7, Action::Fav8, Action::Fav9];
        F.iter().position(|a| *a == self)
    }
    /// View index 0..6 for View1..View6.
    pub fn view_index(self) -> Option<u32> {
        const V: [Action; 6] = [Action::View1, Action::View2, Action::View3, Action::View4, Action::View5, Action::View6];
        V.iter().position(|a| *a == self).map(|i| i as u32)
    }
}

/// Up to two chords per action (primary + alternate).
pub type KeyMap = BTreeMap<Action, Vec<Chord>>;

pub fn default_keymap() -> KeyMap {
    Action::ALL.iter().map(|a| (*a, a.defaults())).collect()
}

// =====================================================================================
// Settings
// =====================================================================================

#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    // --- graphics
    pub quality: Quality,
    pub custom_graphics: GraphicsKnobs,
    pub fps_limit: u32, // 0 = unlimited (vsync only)
    pub vsync: bool,
    pub battery_saver: bool,
    pub pause_when_unfocused: bool,
    // --- display & accessibility
    pub ui_scale: f32,
    pub palette: PaletteMode,
    /// 0..1 scales down bright flashes (explosions, detonations, bloom spikes).
    pub flash_intensity: f32,
    /// Disable camera shake / screen kick.
    pub reduce_motion: bool,
    /// Bigger, bolder brush cursor outline.
    pub large_cursor: bool,
    /// Show text labels next to colour-only indicators (gauges, legends).
    pub label_colours: bool,
    // --- audio
    pub master_volume: f32,
    pub sfx_volume: f32,
    pub ambience_volume: f32,
    pub music_volume: f32,
    pub muted: bool,
    pub mute_when_unfocused: bool,
    // --- controls
    pub keys: KeyMap,
    pub invert_zoom: bool,
    pub zoom_speed: f32,
    // --- gameplay (mirrors the old app.rs Settings so it can be replaced entirely)
    pub start_speed: f32,
    pub hover_info: bool,
    pub tips: bool,
    pub unlock_all: bool,
    /// Favourite tools as app.rs `tool_key` strings ("W0", "P6", "G5", "M13", "E").
    pub favs: Vec<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            quality: Quality::High,
            custom_graphics: Quality::High.knobs(),
            fps_limit: 0,
            vsync: true,
            battery_saver: false,
            pause_when_unfocused: false,
            ui_scale: 1.0,
            palette: PaletteMode::Natural,
            flash_intensity: 1.0,
            reduce_motion: false,
            large_cursor: false,
            label_colours: false,
            master_volume: 0.8,
            sfx_volume: 1.0,
            ambience_volume: 0.7,
            music_volume: 0.5,
            muted: false,
            mute_when_unfocused: true,
            keys: default_keymap(),
            invert_zoom: false,
            zoom_speed: 1.0,
            start_speed: 1.0,
            hover_info: true,
            tips: true,
            unlock_all: false,
            favs: ["W0", "P6", "G5", "M13", "M11", "M19"].iter().map(|s| s.to_string()).collect(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AudioChannel {
    Sfx,
    Ambience,
    Music,
}

impl Settings {
    /// Effective graphics knobs: preset (or custom) with battery saver caps applied.
    pub fn graphics(&self) -> GraphicsKnobs {
        let mut k = if self.quality == Quality::Custom { self.custom_graphics } else { self.quality.knobs() };
        if self.battery_saver {
            k.particle_fraction = k.particle_fraction.min(0.25);
            k.bloom = false;
            k.wide_bloom = false;
            k.heat_haze = false;
            k.fx_detail = false;
            k.render_scale = k.render_scale.min(0.75);
            k.max_steps_per_frame = k.max_steps_per_frame.min(12);
        }
        k.clamp()
    }

    /// Colour parameters for the selected palette, with flash intensity folded into the flame gain.
    pub fn palette(&self) -> PaletteParams {
        let mut p = palette_params(self.palette);
        p.flame[3] *= 0.4 + 0.6 * self.flash_intensity.clamp(0.0, 1.0);
        p
    }

    /// Effective FPS cap (0 = none). Battery saver caps at 30; unfocused windows drop to 10.
    pub fn effective_fps_limit(&self, focused: bool) -> u32 {
        let mut f = self.fps_limit;
        if self.battery_saver {
            f = if f == 0 { 30 } else { f.min(30) };
        }
        if !focused && (self.battery_saver || self.pause_when_unfocused) {
            f = if f == 0 { 10 } else { f.min(10) };
        }
        f
    }

    /// Minimum frame duration for the frame limiter (None = run as fast as vsync allows).
    pub fn frame_interval(&self, focused: bool) -> Option<Duration> {
        match self.effective_fps_limit(focused) {
            0 => None,
            f => Some(Duration::from_secs_f64(1.0 / f as f64)),
        }
    }

    /// Should the sim auto-pause right now?
    pub fn should_pause(&self, focused: bool) -> bool {
        self.pause_when_unfocused && !focused
    }

    /// Final 0..1 gain for an audio channel (master × channel, mute aware).
    pub fn volume(&self, ch: AudioChannel, focused: bool) -> f32 {
        if self.muted || (self.mute_when_unfocused && !focused) {
            return 0.0;
        }
        let c = match ch {
            AudioChannel::Sfx => self.sfx_volume,
            AudioChannel::Ambience => self.ambience_volume,
            AudioChannel::Music => self.music_volume,
        };
        (self.master_volume * c).clamp(0.0, 1.0)
    }

    /// Multiplicative zoom change for a scroll of `scroll_y` points (positive = wheel up / away).
    pub fn zoom_factor(&self, scroll_y: f32) -> f32 {
        let s = if self.invert_zoom { -scroll_y } else { scroll_y };
        (s * 0.0025 * self.zoom_speed).exp()
    }

    /// Was any chord bound to `a` pressed this frame (exact modifiers)?
    pub fn pressed(&self, i: &egui::InputState, a: Action) -> bool {
        self.chords(a).iter().any(|c| c.pressed(i))
    }

    /// Every action whose chord was pressed this frame (in `Action::ALL` order).
    pub fn pressed_actions(&self, i: &egui::InputState) -> Vec<Action> {
        Action::ALL.iter().copied().filter(|a| self.pressed(i, *a)).collect()
    }

    pub fn chords(&self, a: Action) -> &[Chord] {
        self.keys.get(&a).map(|v| v.as_slice()).unwrap_or(&[])
    }

    /// Primary binding label, e.g. for tooltips: "Pause (Space)".
    pub fn key_label(&self, a: Action) -> String {
        self.chords(a).first().map(|c| c.label()).unwrap_or_else(|| "unbound".into())
    }

    /// Pairs of actions sharing the same chord.
    pub fn conflicts(&self) -> Vec<(Action, Action, Chord)> {
        let mut out = Vec::new();
        let all: Vec<(Action, Chord)> = self.keys.iter().flat_map(|(a, cs)| cs.iter().map(move |c| (*a, *c))).collect();
        for (i, (a, c)) in all.iter().enumerate() {
            for (b, d) in &all[i + 1..] {
                if a != b && c == d {
                    out.push((*a, *b, *c));
                }
            }
        }
        out
    }

    pub fn reset_graphics(&mut self) {
        let d = Settings::default();
        self.quality = d.quality;
        self.custom_graphics = d.custom_graphics;
        self.fps_limit = d.fps_limit;
        self.vsync = d.vsync;
        self.battery_saver = d.battery_saver;
        self.pause_when_unfocused = d.pause_when_unfocused;
    }
    pub fn reset_display(&mut self) {
        let d = Settings::default();
        self.ui_scale = d.ui_scale;
        self.palette = d.palette;
        self.flash_intensity = d.flash_intensity;
        self.reduce_motion = d.reduce_motion;
        self.large_cursor = d.large_cursor;
        self.label_colours = d.label_colours;
    }
    pub fn reset_audio(&mut self) {
        let d = Settings::default();
        self.master_volume = d.master_volume;
        self.sfx_volume = d.sfx_volume;
        self.ambience_volume = d.ambience_volume;
        self.music_volume = d.music_volume;
        self.muted = d.muted;
        self.mute_when_unfocused = d.mute_when_unfocused;
    }
    pub fn reset_controls(&mut self) {
        let d = Settings::default();
        self.keys = d.keys;
        self.invert_zoom = d.invert_zoom;
        self.zoom_speed = d.zoom_speed;
    }

    fn clamp(&mut self) {
        self.custom_graphics = self.custom_graphics.clamp();
        self.fps_limit = self.fps_limit.min(1000);
        self.ui_scale = self.ui_scale.clamp(0.75, 2.0);
        self.flash_intensity = self.flash_intensity.clamp(0.0, 1.0);
        for v in [&mut self.master_volume, &mut self.sfx_volume, &mut self.ambience_volume, &mut self.music_volume] {
            *v = v.clamp(0.0, 1.0);
        }
        self.zoom_speed = self.zoom_speed.clamp(0.25, 4.0);
        self.start_speed = self.start_speed.clamp(0.25, 4.0);
        for a in Action::ALL {
            self.keys.entry(*a).or_insert_with(|| a.defaults());
        }
        for v in self.keys.values_mut() {
            v.truncate(2);
        }
    }

    // ---------------------------------------------------------------- JSON
    pub fn to_json(&self) -> String {
        let g = &self.custom_graphics;
        let n = |v: f64| Json::Num(v);
        let f = |v: f32| Json::Num(((v as f64) * 10000.0).round() / 10000.0);
        let b = Json::Bool;
        let s = |v: &str| Json::Str(v.to_string());
        let keys = Json::Obj(
            self.keys.iter().map(|(a, cs)| (a.id().to_string(), Json::Arr(cs.iter().map(|c| Json::Str(c.to_string_id())).collect()))).collect(),
        );
        let root = Json::Obj(vec![
            ("version".into(), n(SETTINGS_VERSION as f64)),
            (
                "graphics".into(),
                Json::Obj(vec![
                    ("quality".into(), s(self.quality.id())),
                    (
                        "custom".into(),
                        Json::Obj(vec![
                            ("particle_fraction".into(), f(g.particle_fraction)),
                            ("bloom".into(), b(g.bloom)),
                            ("wide_bloom".into(), b(g.wide_bloom)),
                            ("render_scale".into(), f(g.render_scale)),
                            ("heat_haze".into(), b(g.heat_haze)),
                            ("fx_detail".into(), b(g.fx_detail)),
                            ("max_steps_per_frame".into(), n(g.max_steps_per_frame as f64)),
                        ]),
                    ),
                    ("fps_limit".into(), n(self.fps_limit as f64)),
                    ("vsync".into(), b(self.vsync)),
                    ("battery_saver".into(), b(self.battery_saver)),
                    ("pause_when_unfocused".into(), b(self.pause_when_unfocused)),
                ]),
            ),
            (
                "display".into(),
                Json::Obj(vec![
                    ("ui_scale".into(), f(self.ui_scale)),
                    ("palette".into(), s(self.palette.id())),
                    ("flash_intensity".into(), f(self.flash_intensity)),
                    ("reduce_motion".into(), b(self.reduce_motion)),
                    ("large_cursor".into(), b(self.large_cursor)),
                    ("label_colours".into(), b(self.label_colours)),
                ]),
            ),
            (
                "audio".into(),
                Json::Obj(vec![
                    ("master".into(), f(self.master_volume)),
                    ("sfx".into(), f(self.sfx_volume)),
                    ("ambience".into(), f(self.ambience_volume)),
                    ("music".into(), f(self.music_volume)),
                    ("muted".into(), b(self.muted)),
                    ("mute_when_unfocused".into(), b(self.mute_when_unfocused)),
                ]),
            ),
            (
                "controls".into(),
                Json::Obj(vec![("invert_zoom".into(), b(self.invert_zoom)), ("zoom_speed".into(), f(self.zoom_speed)), ("keys".into(), keys)]),
            ),
            (
                "gameplay".into(),
                Json::Obj(vec![
                    ("start_speed".into(), f(self.start_speed)),
                    ("hover_info".into(), b(self.hover_info)),
                    ("tips".into(), b(self.tips)),
                    ("unlock_all".into(), b(self.unlock_all)),
                    ("favs".into(), Json::Arr(self.favs.iter().map(|t| s(t)).collect())),
                ]),
            ),
        ]);
        let mut out = String::new();
        root.write_pretty(&mut out, 0);
        out.push('\n');
        out
    }

    /// Tolerant: unknown keys are ignored, missing / malformed values keep their defaults,
    /// out-of-range numbers are clamped. Returns Err only if the text isn't JSON at all.
    pub fn from_json(text: &str) -> Result<Settings, String> {
        let root = Json::parse(text)?;
        let mut s = Settings::default();
        let gb = |o: Option<&Json>, d: &mut bool| {
            if let Some(Json::Bool(v)) = o {
                *d = *v;
            }
        };
        let gf = |o: Option<&Json>, d: &mut f32| {
            if let Some(Json::Num(v)) = o {
                if v.is_finite() {
                    *d = *v as f32;
                }
            }
        };
        let gu = |o: Option<&Json>, d: &mut u32| {
            if let Some(Json::Num(v)) = o {
                if v.is_finite() && *v >= 0.0 {
                    *d = v.round().min(u32::MAX as f64) as u32;
                }
            }
        };
        if let Some(g) = root.get("graphics") {
            if let Some(q) = g.get("quality").and_then(|v| v.as_str()).and_then(Quality::from_id) {
                s.quality = q;
            }
            if let Some(c) = g.get("custom") {
                let k = &mut s.custom_graphics;
                gf(c.get("particle_fraction"), &mut k.particle_fraction);
                gb(c.get("bloom"), &mut k.bloom);
                gb(c.get("wide_bloom"), &mut k.wide_bloom);
                gf(c.get("render_scale"), &mut k.render_scale);
                gb(c.get("heat_haze"), &mut k.heat_haze);
                gb(c.get("fx_detail"), &mut k.fx_detail);
                gu(c.get("max_steps_per_frame"), &mut k.max_steps_per_frame);
            }
            gu(g.get("fps_limit"), &mut s.fps_limit);
            gb(g.get("vsync"), &mut s.vsync);
            gb(g.get("battery_saver"), &mut s.battery_saver);
            gb(g.get("pause_when_unfocused"), &mut s.pause_when_unfocused);
        }
        if let Some(d) = root.get("display") {
            gf(d.get("ui_scale"), &mut s.ui_scale);
            if let Some(p) = d.get("palette").and_then(|v| v.as_str()).and_then(PaletteMode::from_id) {
                s.palette = p;
            }
            gf(d.get("flash_intensity"), &mut s.flash_intensity);
            gb(d.get("reduce_motion"), &mut s.reduce_motion);
            gb(d.get("large_cursor"), &mut s.large_cursor);
            gb(d.get("label_colours"), &mut s.label_colours);
        }
        if let Some(a) = root.get("audio") {
            gf(a.get("master"), &mut s.master_volume);
            gf(a.get("sfx"), &mut s.sfx_volume);
            gf(a.get("ambience"), &mut s.ambience_volume);
            gf(a.get("music"), &mut s.music_volume);
            gb(a.get("muted"), &mut s.muted);
            gb(a.get("mute_when_unfocused"), &mut s.mute_when_unfocused);
        }
        if let Some(c) = root.get("controls") {
            gb(c.get("invert_zoom"), &mut s.invert_zoom);
            gf(c.get("zoom_speed"), &mut s.zoom_speed);
            if let Some(Json::Obj(kv)) = c.get("keys") {
                for (k, v) in kv {
                    let Some(a) = Action::from_id(k) else { continue };
                    if let Json::Arr(items) = v {
                        let chords: Vec<Chord> = items.iter().filter_map(|i| i.as_str()).filter_map(Chord::parse).collect();
                        // an explicitly empty list means "unbound"; a list of garbage keeps defaults
                        if chords.is_empty() && !items.is_empty() {
                            continue;
                        }
                        s.keys.insert(a, chords);
                    }
                }
            }
        }
        if let Some(g) = root.get("gameplay") {
            gf(g.get("start_speed"), &mut s.start_speed);
            gb(g.get("hover_info"), &mut s.hover_info);
            gb(g.get("tips"), &mut s.tips);
            gb(g.get("unlock_all"), &mut s.unlock_all);
            if let Some(Json::Arr(items)) = g.get("favs") {
                s.favs = items.iter().filter_map(|i| i.as_str()).map(|t| t.to_string()).collect();
            }
        }
        s.clamp();
        Ok(s)
    }

    /// Import the pre-Ignis `key=value` settings.txt (ui_scale, start_speed, hover_info, tips,
    /// unlock_all, favs).
    pub fn from_legacy_txt(txt: &str) -> Settings {
        let mut s = Settings::default();
        for line in txt.lines() {
            let Some((k, v)) = line.split_once('=') else { continue };
            let (k, v) = (k.trim(), v.trim());
            match k {
                "ui_scale" => s.ui_scale = v.parse().unwrap_or(1.0),
                "start_speed" => s.start_speed = v.parse().unwrap_or(1.0),
                "hover_info" => s.hover_info = v == "1",
                "tips" => s.tips = v == "1",
                "unlock_all" => s.unlock_all = v == "1",
                "favs" => s.favs = v.split(',').map(|w| w.trim().to_string()).filter(|w| !w.is_empty()).collect(),
                _ => {}
            }
        }
        s.clamp();
        s
    }
}

// =====================================================================================
// Persistence
// =====================================================================================

/// `~/Library/Application Support/Ignis`
pub fn data_dir() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    Some(PathBuf::from(home).join("Library/Application Support/Ignis"))
}

pub fn settings_path() -> Option<PathBuf> {
    data_dir().map(|d| d.join("settings.json"))
}

fn legacy_path() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    Some(PathBuf::from(home).join("Library/Application Support/Combustor/settings.txt"))
}

/// Load settings (falls back to the legacy file, then defaults). Never fails.
/// `IGNIS_SETTINGS=/path.json` overrides the location (handy for tests / screenshots).
pub fn load() -> Settings {
    let path = std::env::var_os("IGNIS_SETTINGS").map(PathBuf::from).or_else(settings_path);
    if let Some(p) = &path {
        if let Ok(txt) = std::fs::read_to_string(p) {
            match Settings::from_json(&txt) {
                Ok(s) => return s,
                Err(e) => {
                    eprintln!("ignis: settings.json unreadable ({e}); using defaults");
                    // keep the broken file for the player to inspect
                    let _ = std::fs::copy(p, p.with_extension("json.bad"));
                    return Settings::default();
                }
            }
        }
    }
    if let Some(txt) = legacy_path().and_then(|p| std::fs::read_to_string(p).ok()) {
        return Settings::from_legacy_txt(&txt);
    }
    Settings::default()
}

/// Save atomically (write tmp + rename). Skipped in screenshot/bench runs (`COMBUSTOR_SHOT`).
pub fn save(s: &Settings) -> std::io::Result<()> {
    if std::env::var_os("COMBUSTOR_SHOT").is_some() {
        return Ok(());
    }
    let path = match std::env::var_os("IGNIS_SETTINGS").map(PathBuf::from).or_else(settings_path) {
        Some(p) => p,
        None => return Err(std::io::Error::new(std::io::ErrorKind::NotFound, "no HOME")),
    };
    save_to(s, &path)
}

pub fn save_to(s: &Settings, path: &std::path::Path) -> std::io::Result<()> {
    if let Some(d) = path.parent() {
        std::fs::create_dir_all(d)?;
    }
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, s.to_json())?;
    std::fs::rename(&tmp, path)
}

// =====================================================================================
// Tiny JSON
// =====================================================================================

#[derive(Clone, Debug, PartialEq)]
pub enum Json {
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
    Arr(Vec<Json>),
    /// Insertion-ordered object.
    Obj(Vec<(String, Json)>),
}

impl Json {
    pub fn get(&self, key: &str) -> Option<&Json> {
        match self {
            Json::Obj(kv) => kv.iter().rev().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Json::Str(s) => Some(s),
            _ => None,
        }
    }

    pub fn parse(text: &str) -> Result<Json, String> {
        let mut p = Parser { s: text.as_bytes(), i: 0, depth: 0 };
        p.ws();
        let v = p.value()?;
        p.ws();
        if p.i != p.s.len() {
            return Err(format!("trailing characters at byte {}", p.i));
        }
        Ok(v)
    }

    pub fn write_pretty(&self, out: &mut String, indent: usize) {
        let pad = |out: &mut String, n: usize| {
            for _ in 0..n {
                out.push_str("  ");
            }
        };
        match self {
            Json::Null => out.push_str("null"),
            Json::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
            Json::Num(n) => {
                if !n.is_finite() {
                    out.push_str("null");
                } else if n.fract() == 0.0 && n.abs() < 1e15 {
                    out.push_str(&format!("{}", *n as i64));
                } else {
                    out.push_str(&format!("{n}"));
                }
            }
            Json::Str(s) => write_str(out, s),
            Json::Arr(items) => {
                // short scalar arrays on one line
                if items.iter().all(|i| !matches!(i, Json::Arr(_) | Json::Obj(_))) {
                    out.push('[');
                    for (k, i) in items.iter().enumerate() {
                        if k > 0 {
                            out.push_str(", ");
                        }
                        i.write_pretty(out, indent);
                    }
                    out.push(']');
                } else {
                    out.push_str("[\n");
                    for (k, i) in items.iter().enumerate() {
                        pad(out, indent + 1);
                        i.write_pretty(out, indent + 1);
                        if k + 1 < items.len() {
                            out.push(',');
                        }
                        out.push('\n');
                    }
                    pad(out, indent);
                    out.push(']');
                }
            }
            Json::Obj(kv) => {
                if kv.is_empty() {
                    out.push_str("{}");
                    return;
                }
                out.push_str("{\n");
                for (k, (key, v)) in kv.iter().enumerate() {
                    pad(out, indent + 1);
                    write_str(out, key);
                    out.push_str(": ");
                    v.write_pretty(out, indent + 1);
                    if k + 1 < kv.len() {
                        out.push(',');
                    }
                    out.push('\n');
                }
                pad(out, indent);
                out.push('}');
            }
        }
    }
}

fn write_str(out: &mut String, s: &str) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
}

struct Parser<'a> {
    s: &'a [u8],
    i: usize,
    depth: usize,
}

impl Parser<'_> {
    fn ws(&mut self) {
        while self.i < self.s.len() && matches!(self.s[self.i], b' ' | b'\t' | b'\n' | b'\r') {
            self.i += 1;
        }
    }
    fn err<T>(&self, m: &str) -> Result<T, String> {
        Err(format!("{m} at byte {}", self.i))
    }
    fn peek(&self) -> Option<u8> {
        self.s.get(self.i).copied()
    }
    fn lit(&mut self, w: &str, v: Json) -> Result<Json, String> {
        if self.s[self.i..].starts_with(w.as_bytes()) {
            self.i += w.len();
            Ok(v)
        } else {
            self.err("bad literal")
        }
    }
    fn value(&mut self) -> Result<Json, String> {
        self.depth += 1;
        if self.depth > 64 {
            return self.err("nesting too deep");
        }
        let r = match self.peek() {
            None => self.err("unexpected end"),
            Some(b'n') => self.lit("null", Json::Null),
            Some(b't') => self.lit("true", Json::Bool(true)),
            Some(b'f') => self.lit("false", Json::Bool(false)),
            Some(b'"') => self.string().map(Json::Str),
            Some(b'[') => {
                self.i += 1;
                let mut v = Vec::new();
                self.ws();
                if self.peek() == Some(b']') {
                    self.i += 1;
                } else {
                    loop {
                        self.ws();
                        v.push(self.value()?);
                        self.ws();
                        match self.peek() {
                            Some(b',') => self.i += 1,
                            Some(b']') => {
                                self.i += 1;
                                break;
                            }
                            _ => return self.err("expected , or ]"),
                        }
                    }
                }
                Ok(Json::Arr(v))
            }
            Some(b'{') => {
                self.i += 1;
                let mut kv = Vec::new();
                self.ws();
                if self.peek() == Some(b'}') {
                    self.i += 1;
                } else {
                    loop {
                        self.ws();
                        if self.peek() != Some(b'"') {
                            return self.err("expected key");
                        }
                        let k = self.string()?;
                        self.ws();
                        if self.peek() != Some(b':') {
                            return self.err("expected :");
                        }
                        self.i += 1;
                        self.ws();
                        let v = self.value()?;
                        kv.push((k, v));
                        self.ws();
                        match self.peek() {
                            Some(b',') => self.i += 1,
                            Some(b'}') => {
                                self.i += 1;
                                break;
                            }
                            _ => return self.err("expected , or }"),
                        }
                    }
                }
                Ok(Json::Obj(kv))
            }
            Some(c) if c == b'-' || c.is_ascii_digit() => {
                let st = self.i;
                self.i += 1;
                while let Some(c) = self.peek() {
                    if c.is_ascii_digit() || matches!(c, b'.' | b'e' | b'E' | b'+' | b'-') {
                        self.i += 1;
                    } else {
                        break;
                    }
                }
                let t = std::str::from_utf8(&self.s[st..self.i]).unwrap_or("");
                t.parse::<f64>().map(Json::Num).or_else(|_| self.err("bad number"))
            }
            Some(_) => self.err("unexpected character"),
        };
        self.depth -= 1;
        r
    }
    fn hex4(&mut self) -> Result<u32, String> {
        if self.i + 4 > self.s.len() {
            return self.err("short \\u escape");
        }
        let t = std::str::from_utf8(&self.s[self.i..self.i + 4]).map_err(|_| "bad \\u".to_string())?;
        let v = u32::from_str_radix(t, 16).map_err(|_| format!("bad \\u escape at byte {}", self.i))?;
        self.i += 4;
        Ok(v)
    }
    fn string(&mut self) -> Result<String, String> {
        self.i += 1; // opening quote
        let mut out: Vec<u8> = Vec::new();
        loop {
            let Some(c) = self.peek() else { return self.err("unterminated string") };
            self.i += 1;
            match c {
                b'"' => break,
                b'\\' => {
                    let Some(e) = self.peek() else { return self.err("bad escape") };
                    self.i += 1;
                    let ch = match e {
                        b'"' => '"',
                        b'\\' => '\\',
                        b'/' => '/',
                        b'b' => '\u{8}',
                        b'f' => '\u{c}',
                        b'n' => '\n',
                        b'r' => '\r',
                        b't' => '\t',
                        b'u' => {
                            let mut cp = self.hex4()?;
                            if (0xD800..0xDC00).contains(&cp) && self.s[self.i..].starts_with(b"\\u") {
                                self.i += 2;
                                let lo = self.hex4()?;
                                if (0xDC00..0xE000).contains(&lo) {
                                    cp = 0x10000 + ((cp - 0xD800) << 10) + (lo - 0xDC00);
                                }
                            }
                            char::from_u32(cp).unwrap_or('\u{fffd}')
                        }
                        _ => return self.err("bad escape"),
                    };
                    let mut buf = [0u8; 4];
                    out.extend_from_slice(ch.encode_utf8(&mut buf).as_bytes());
                }
                c => out.push(c),
            }
        }
        String::from_utf8(out).map_err(|_| "invalid utf-8 in string".to_string())
    }
}

// =====================================================================================
// UI
// =====================================================================================

#[derive(Clone, Copy, PartialEq, Eq, Default)]
enum Tab {
    #[default]
    Graphics,
    Display,
    Audio,
    Controls,
    Gameplay,
}

fn tab_id() -> egui::Id {
    egui::Id::new("ignis_settings_tab")
}
fn capture_id() -> egui::Id {
    egui::Id::new("ignis_settings_capture")
}

/// Pre-select a tab: 0 Graphics, 1 Display & access, 2 Audio, 3 Controls, 4 Gameplay
/// (e.g. open straight to Controls from the help window).
pub fn open_tab(ctx: &egui::Context, tab: usize) {
    let t = [Tab::Graphics, Tab::Display, Tab::Audio, Tab::Controls, Tab::Gameplay][tab.min(4)];
    ctx.data_mut(|d| d.insert_temp(tab_id(), t));
}

/// True while the settings panel is waiting for a key press to rebind an action. The game should
/// skip its own shortcuts (and the Escape handler) while this is true.
pub fn is_capturing(ctx: &egui::Context) -> bool {
    ctx.data(|d| d.get_temp::<(Action, usize)>(capture_id())).is_some()
}

/// The settings window: a floating `egui::Window`. Returns true when something changed.
pub fn show_window(ctx: &egui::Context, open: &mut bool, s: &mut Settings) -> bool {
    let mut changed = false;
    egui::Window::new("Settings")
        .open(open)
        .default_width(560.0)
        .min_width(360.0)
        .collapsible(false)
        .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
        .show(ctx, |ui| {
            changed = show(ui, s);
        });
    changed
}

/// The full settings panel (tabs: Graphics, Display & access, Audio, Controls, Gameplay) for any
/// `Ui`. Returns true if any setting changed this frame (call [`save`] then).
pub fn show(ui: &mut egui::Ui, s: &mut Settings) -> bool {
    let mut changed = false;
    let mut tab: Tab = ui.ctx().data(|d| d.get_temp(tab_id())).unwrap_or_default();
    ui.horizontal_wrapped(|ui| {
        for (t, name) in [
            (Tab::Graphics, "Graphics"),
            (Tab::Display, "Display & access"),
            (Tab::Audio, "Audio"),
            (Tab::Controls, "Controls"),
            (Tab::Gameplay, "Simulation"),
        ] {
            if ui.selectable_label(tab == t, name).clicked() {
                tab = t;
                ui.ctx().data_mut(|d| d.remove::<(Action, usize)>(capture_id()));
            }
        }
    });
    ui.ctx().data_mut(|d| d.insert_temp(tab_id(), tab));
    ui.separator();
    egui::ScrollArea::vertical().max_height(ui.available_height().max(240.0)).auto_shrink([false, true]).show(ui, |ui| {
        changed |= match tab {
            Tab::Graphics => graphics_tab(ui, s),
            Tab::Display => display_tab(ui, s),
            Tab::Audio => audio_tab(ui, s),
            Tab::Controls => controls_tab(ui, s),
            Tab::Gameplay => gameplay_tab(ui, s),
        };
    });
    ui.separator();
    ui.horizontal(|ui| {
        if ui.button("Reset this tab").clicked() {
            match tab {
                Tab::Graphics => s.reset_graphics(),
                Tab::Display => s.reset_display(),
                Tab::Audio => s.reset_audio(),
                Tab::Controls => s.reset_controls(),
                Tab::Gameplay => {
                    let d = Settings::default();
                    s.start_speed = d.start_speed;
                    s.hover_info = d.hover_info;
                    s.tips = d.tips;
                    s.unlock_all = d.unlock_all;
                }
            }
            changed = true;
        }
        if ui.button("Reset everything").on_hover_text("Restore all settings to their defaults (favourites are kept)").clicked() {
            let favs = std::mem::take(&mut s.favs);
            *s = Settings::default();
            s.favs = favs;
            changed = true;
        }
    });
    changed
}

fn hint(ui: &mut egui::Ui, t: &str) {
    ui.label(egui::RichText::new(t).small().weak());
}

fn graphics_tab(ui: &mut egui::Ui, s: &mut Settings) -> bool {
    let mut ch = false;
    ui.label(egui::RichText::new("Quality preset").strong());
    ui.horizontal_wrapped(|ui| {
        for q in Quality::ALL {
            if ui.selectable_label(s.quality == q, q.label()).clicked() && s.quality != q {
                if q == Quality::Custom && s.quality != Quality::Custom {
                    s.custom_graphics = s.quality.knobs();
                }
                s.quality = q;
                ch = true;
            }
        }
    });
    hint(ui, "Low suits older Macs and battery use; Ultra supersamples the canvas and runs the sim at full speed.");
    ui.add_space(6.0);

    // Knobs: editing any knob switches to Custom (starting from the current preset's values).
    let mut k = if s.quality == Quality::Custom { s.custom_graphics } else { s.quality.knobs() };
    let before = k;
    egui::Grid::new("ignis_gfx_grid").num_columns(2).spacing([18.0, 8.0]).show(ui, |ui| {
        ui.label("Particles");
        ui.add(egui::Slider::new(&mut k.particle_fraction, 0.0..=1.0).custom_formatter(|v, _| format!("{:.0}%", v * 100.0)));
        ui.end_row();
        ui.label("Render scale");
        ui.add(egui::Slider::new(&mut k.render_scale, 0.5..=2.0).step_by(0.25).suffix("×"));
        ui.end_row();
        ui.label("Bloom");
        ui.horizontal(|ui| {
            ui.checkbox(&mut k.bloom, "Glow");
            ui.add_enabled(k.bloom, egui::Checkbox::new(&mut k.wide_bloom, "Wide halo"));
        });
        ui.end_row();
        ui.label("Heat haze");
        ui.checkbox(&mut k.heat_haze, "Shimmer above hot gas");
        ui.end_row();
        ui.label("Flame detail");
        ui.checkbox(&mut k.fx_detail, "Advected flame / smoke texture");
        ui.end_row();
        ui.label("Sim speed cap");
        ui.add(egui::Slider::new(&mut k.max_steps_per_frame, 4..=64).suffix(" steps/frame"));
        ui.end_row();
    });
    if k != before {
        s.custom_graphics = k.clamp();
        s.quality = Quality::Custom;
        ch = true;
    }
    hint(ui, "The sim speed cap limits physics steps per frame: lower keeps the frame rate smooth but slow-motions big fast speeds.");

    ui.add_space(8.0);
    ui.label(egui::RichText::new("Frame rate & power").strong());
    egui::Grid::new("ignis_fps_grid").num_columns(2).spacing([18.0, 8.0]).show(ui, |ui| {
        ui.label("FPS limit");
        ui.horizontal_wrapped(|ui| {
            for (v, n) in [(0u32, "Off"), (30, "30"), (60, "60"), (120, "120")] {
                if ui.selectable_label(s.fps_limit == v, n).clicked() && s.fps_limit != v {
                    s.fps_limit = v;
                    ch = true;
                }
            }
        });
        ui.end_row();
        ui.label("V-sync");
        ch |= ui.checkbox(&mut s.vsync, "Sync to display (applies on restart)").changed();
        ui.end_row();
        ui.label("Battery saver");
        ch |= ui
            .checkbox(&mut s.battery_saver, "30 fps, no glow / haze, lighter sim")
            .on_hover_text("Overrides the preset while on; turn it off to get your settings back")
            .changed();
        ui.end_row();
        ui.label("When in background");
        ch |= ui.checkbox(&mut s.pause_when_unfocused, "Pause the simulation").changed();
        ui.end_row();
    });
    if s.battery_saver {
        let g = s.graphics();
        hint(ui, &format!("Battery saver active: {} steps/frame max, {:.0}% particles, {} fps cap.", g.max_steps_per_frame, g.particle_fraction * 100.0, s.effective_fps_limit(true)));
    }
    ch
}

fn c32(c: [f32; 3]) -> egui::Color32 {
    egui::Color32::from_rgb(to_srgb_u8(c[0]), to_srgb_u8(c[1]), to_srgb_u8(c[2]))
}

fn palette_preview(ui: &mut egui::Ui, p: &PaletteParams) {
    let w = ui.available_width().min(420.0);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(w, 18.0), egui::Sense::hover());
    let painter = ui.painter_at(rect);
    let n = 64;
    for i in 0..n {
        let t0 = i as f32 / n as f32;
        let x0 = rect.left() + rect.width() * t0;
        let x1 = rect.left() + rect.width() * (i + 1) as f32 / n as f32 + 0.5;
        painter.rect_filled(egui::Rect::from_min_max(egui::pos2(x0, rect.top()), egui::pos2(x1, rect.bottom())), 0.0, c32(p.heat_at(t0 + 0.5 / n as f32)));
    }
    ui.horizontal(|ui| {
        hint(ui, "300 K");
        ui.add_space((w - 90.0).max(0.0));
        hint(ui, "3600 K");
    });
    ui.horizontal_wrapped(|ui| {
        for (name, c) in [("Fuel", p.fuel), ("O₂", p.oxygen), ("N₂", p.nitrogen), ("CO₂/H₂O", p.products), ("Soot", p.soot)] {
            let (r, _) = ui.allocate_exact_size(egui::vec2(14.0, 14.0), egui::Sense::hover());
            ui.painter().rect_filled(r, 3.0, c32([c[0], c[1], c[2]]));
            ui.painter().rect_stroke(r, 3.0, egui::Stroke::new(1.0_f32, ui.visuals().weak_text_color()), egui::StrokeKind::Inside);
            ui.label(name);
            ui.add_space(6.0);
        }
    });
}

fn display_tab(ui: &mut egui::Ui, s: &mut Settings) -> bool {
    let mut ch = false;
    egui::Grid::new("ignis_disp_grid").num_columns(2).spacing([18.0, 8.0]).show(ui, |ui| {
        ui.label("Interface size");
        ui.horizontal_wrapped(|ui| {
            for (v, n) in [(0.85f32, "Small"), (1.0, "Normal"), (1.2, "Large"), (1.45, "Huge")] {
                if ui.selectable_label((s.ui_scale - v).abs() < 0.01, n).clicked() {
                    s.ui_scale = v;
                    ch = true;
                }
            }
        });
        ui.end_row();
        ui.label("");
        // apply only on release so the UI doesn't rescale under the mouse while dragging
        let mut v = s.ui_scale;
        let r = ui.add(egui::Slider::new(&mut v, 0.75..=2.0).step_by(0.05).suffix("×"));
        if (r.drag_stopped() || (r.changed() && !r.dragged())) && (v - s.ui_scale).abs() > 1e-4 {
            s.ui_scale = v;
            ch = true;
        }
        ui.end_row();
        ui.label("Colour palette");
        egui::ComboBox::from_id_salt("ignis_palette").selected_text(s.palette.label()).width(220.0).show_ui(ui, |ui| {
            for m in PaletteMode::ALL {
                if ui.selectable_label(s.palette == m, m.label()).clicked() && s.palette != m {
                    s.palette = m;
                    ch = true;
                }
            }
        });
        ui.end_row();
    });
    hint(ui, "Changes how temperature and gas species are coloured in the heat and gas views.");
    palette_preview(ui, &s.palette());
    ui.add_space(8.0);
    egui::Grid::new("ignis_access_grid").num_columns(2).spacing([18.0, 8.0]).show(ui, |ui| {
        ui.label("Flash brightness");
        ch |= ui
            .add(egui::Slider::new(&mut s.flash_intensity, 0.0..=1.0).custom_formatter(|v, _| format!("{:.0}%", v * 100.0)))
            .on_hover_text("Dims explosions, detonations and sudden glow spikes (photosensitivity)")
            .changed();
        ui.end_row();
        ui.label("Motion");
        ch |= ui.checkbox(&mut s.reduce_motion, "Reduce motion (no camera shake)").changed();
        ui.end_row();
        ui.label("Cursor");
        ch |= ui.checkbox(&mut s.large_cursor, "Large, high-contrast brush outline").changed();
        ui.end_row();
        ui.label("Labels");
        ch |= ui.checkbox(&mut s.label_colours, "Add text to colour-only indicators").changed();
        ui.end_row();
    });
    ch
}

fn audio_tab(ui: &mut egui::Ui, s: &mut Settings) -> bool {
    let mut ch = false;
    ch |= ui.checkbox(&mut s.muted, "Mute all sound").changed();
    ui.add_enabled_ui(!s.muted, |ui| {
        egui::Grid::new("ignis_audio_grid").num_columns(2).spacing([18.0, 8.0]).show(ui, |ui| {
            let pct = |v: f64, _: std::ops::RangeInclusive<usize>| format!("{:.0}%", v * 100.0);
            for (n, v) in [
                ("Master", &mut s.master_volume),
                ("Effects", &mut s.sfx_volume),
                ("Ambience (roar, hiss)", &mut s.ambience_volume),
                ("Music", &mut s.music_volume),
            ] {
                ui.label(n);
                ch |= ui.add(egui::Slider::new(v, 0.0..=1.0).custom_formatter(pct)).changed();
                ui.end_row();
            }
        });
    });
    ch |= ui.checkbox(&mut s.mute_when_unfocused, "Mute when Ignis is in the background").changed();
    ch
}

fn controls_tab(ui: &mut egui::Ui, s: &mut Settings) -> bool {
    let mut ch = false;
    egui::Grid::new("ignis_zoom_grid").num_columns(2).spacing([18.0, 8.0]).show(ui, |ui| {
        ui.label("Zoom");
        ch |= ui.checkbox(&mut s.invert_zoom, "Invert scroll direction").changed();
        ui.end_row();
        ui.label("Zoom speed");
        ch |= ui.add(egui::Slider::new(&mut s.zoom_speed, 0.25..=4.0).logarithmic(true).suffix("×")).changed();
        ui.end_row();
    });
    ui.add_space(6.0);
    ui.label(egui::RichText::new("Key bindings").strong());
    hint(ui, "Click a key to rebind it, then press the new key (with ⌘ / ⇧ / ⌥ if you like). Esc cancels, Backspace clears.");

    // --- capture in progress?
    let capture: Option<(Action, usize)> = ui.ctx().data(|d| d.get_temp(capture_id()));
    if let Some((a, slot)) = capture {
        let ev = ui.input(|i| {
            i.events.iter().find_map(|e| match e {
                egui::Event::Key { key, pressed: true, repeat: false, modifiers, .. } => Some((*key, *modifiers)),
                _ => None,
            })
        });
        if let Some((key, m)) = ev {
            let list = s.keys.entry(a).or_default();
            match key {
                egui::Key::Escape => {}
                egui::Key::Backspace | egui::Key::Delete => {
                    if slot < list.len() {
                        list.remove(slot);
                        ch = true;
                    }
                }
                _ => {
                    let c = Chord { key, cmd: m.command, shift: m.shift, alt: m.alt };
                    if slot < list.len() {
                        list[slot] = c;
                    } else {
                        list.push(c);
                    }
                    list.dedup();
                    ch = true;
                }
            }
            ui.ctx().data_mut(|d| d.remove::<(Action, usize)>(capture_id()));
            // swallow the key so the game doesn't also act on it this frame
            ui.input_mut(|i| i.events.retain(|e| !matches!(e, egui::Event::Key { key: k, .. } if *k == key)));
        }
    }
    let capture: Option<(Action, usize)> = ui.ctx().data(|d| d.get_temp(capture_id()));

    let conflicts = s.conflicts();
    let in_conflict = |a: Action, c: &Chord| conflicts.iter().any(|(x, y, k)| (*x == a || *y == a) && k == c);
    let mut group = "";
    let mut set_capture: Option<(Action, usize)> = None;
    egui::Grid::new("ignis_keys_grid").num_columns(3).spacing([14.0, 4.0]).striped(true).show(ui, |ui| {
        for a in Action::ALL.iter().copied() {
            if a.group() != group {
                group = a.group();
                ui.label(egui::RichText::new(group).strong().color(ui.visuals().strong_text_color()));
                ui.end_row();
            }
            ui.label(a.label());
            let chords = s.chords(a).to_vec();
            for slot in 0..2 {
                let waiting = capture == Some((a, slot));
                let text = if waiting {
                    egui::RichText::new("press a key…").italics()
                } else if let Some(c) = chords.get(slot) {
                    let t = egui::RichText::new(c.label()).monospace();
                    if in_conflict(a, c) {
                        t.color(ui.visuals().warn_fg_color)
                    } else {
                        t
                    }
                } else if slot == 0 || !chords.is_empty() {
                    egui::RichText::new(if slot == 0 { "unbound" } else { "+" }).weak()
                } else {
                    egui::RichText::new("").weak()
                };
                // slot 1 only offered once slot 0 exists
                if slot == 1 && chords.is_empty() {
                    ui.label("");
                    continue;
                }
                let r = ui.add(egui::Button::new(text).min_size(egui::vec2(96.0, 0.0)).selected(waiting));
                let r = if chords.get(slot).map(|c| in_conflict(a, c)).unwrap_or(false) {
                    r.on_hover_text("Also used by another action")
                } else {
                    r.on_hover_text(if slot == 0 { "Primary key" } else { "Alternate key" })
                };
                if r.clicked() {
                    set_capture = if waiting { None } else { Some((a, slot.min(chords.len()))) };
                    if waiting {
                        ui.ctx().data_mut(|d| d.remove::<(Action, usize)>(capture_id()));
                    }
                }
                if r.secondary_clicked() && slot < chords.len() {
                    s.keys.entry(a).or_default().remove(slot);
                    ch = true;
                }
            }
            ui.end_row();
        }
    });
    if let Some(c) = set_capture {
        ui.ctx().data_mut(|d| d.insert_temp(capture_id(), c));
    }
    if !conflicts.is_empty() {
        ui.add_space(4.0);
        ui.colored_label(ui.visuals().warn_fg_color, format!("{} key conflict(s): both actions will fire.", conflicts.len()));
        for (a, b, c) in conflicts.iter().take(4) {
            hint(ui, &format!("{}: {} / {}", c.label(), a.label(), b.label()));
        }
    }
    hint(ui, "Right-click a key to unbind it.");
    ch
}

fn gameplay_tab(ui: &mut egui::Ui, s: &mut Settings) -> bool {
    let mut ch = false;
    egui::Grid::new("ignis_play_grid").num_columns(2).spacing([18.0, 8.0]).show(ui, |ui| {
        ui.label("Start speed");
        ui.horizontal_wrapped(|ui| {
            for v in [0.25f32, 0.5, 1.0, 2.0, 4.0] {
                if ui.selectable_label((s.start_speed - v).abs() < 0.01, format!("{v}×")).clicked() {
                    s.start_speed = v;
                    ch = true;
                }
            }
        });
        ui.end_row();
        ui.label("Hover info");
        ch |= ui.checkbox(&mut s.hover_info, "Show what's under the cursor").changed();
        ui.end_row();
        ui.label("Tips");
        ch |= ui.checkbox(&mut s.tips, "One-line hint when picking an element").changed();
        ui.end_row();
        ui.label("Missions");
        ch |= ui.checkbox(&mut s.unlock_all, "Unlock every mission").changed();
        ui.end_row();
    });
    ch
}

// =====================================================================================
// Tests (run via `cargo test --release --example settings_check`)
// =====================================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_default() {
        let s = Settings::default();
        let j = s.to_json();
        let t = Settings::from_json(&j).unwrap();
        assert_eq!(s, t);
    }

    #[test]
    fn roundtrip_modified() {
        let mut s = Settings::default();
        s.quality = Quality::Custom;
        s.custom_graphics.render_scale = 1.25;
        s.custom_graphics.max_steps_per_frame = 20;
        s.palette = PaletteMode::Tritanopia;
        s.muted = true;
        s.master_volume = 0.3;
        s.keys.insert(Action::Pause, vec![Chord::parse("Cmd+Alt+P").unwrap()]);
        s.keys.insert(Action::Help, vec![]);
        s.favs = vec!["W\"weird\\".into(), "ünï".into()];
        let t = Settings::from_json(&s.to_json()).unwrap();
        assert_eq!(s, t);
    }

    #[test]
    fn tolerant_parse() {
        let t = Settings::from_json(r#"{"graphics":{"quality":"ultra","fps_limit":-5},"display":{"ui_scale":99},"zzz":[1,2,{"a":null}]}"#).unwrap();
        assert_eq!(t.quality, Quality::Ultra);
        assert_eq!(t.fps_limit, 0);
        assert_eq!(t.ui_scale, 2.0);
        assert!(Settings::from_json("{not json").is_err());
    }

    #[test]
    fn default_keys_no_conflicts() {
        assert!(Settings::default().conflicts().is_empty(), "{:?}", Settings::default().conflicts());
        for a in Action::ALL {
            for c in a.defaults() {
                assert_eq!(Chord::parse(&c.to_string_id()), Some(c), "{}", c.to_string_id());
            }
        }
    }

    #[test]
    fn json_escapes() {
        let v = Json::parse(r#"["aé😀\n", 1.5e3, -2, true, null]"#).unwrap();
        assert_eq!(v, Json::Arr(vec![Json::Str("aé😀\n".into()), Json::Num(1500.0), Json::Num(-2.0), Json::Bool(true), Json::Null]));
    }

    #[test]
    fn battery_saver_caps() {
        let mut s = Settings::default();
        s.quality = Quality::Ultra;
        s.battery_saver = true;
        let g = s.graphics();
        assert!(!g.bloom && !g.heat_haze && g.max_steps_per_frame <= 12);
        assert_eq!(s.effective_fps_limit(true), 30);
    }

    #[test]
    fn legacy_import() {
        let s = Settings::from_legacy_txt("ui_scale=1.2\nstart_speed=2\nhover_info=0\ntips=1\nunlock_all=1\nfavs=W0,M13\n");
        assert_eq!(s.ui_scale, 1.2);
        assert_eq!(s.start_speed, 2.0);
        assert!(!s.hover_info && s.unlock_all);
        assert_eq!(s.favs, vec!["W0".to_string(), "M13".to_string()]);
    }
}
