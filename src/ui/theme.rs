//! Ignis visual theme: sober neutral dark greys, one accent colour, small crisp type with tabular figures,
//! a 4/8 px spacing grid and 1 px separators (ARCHITECTURE.md "UI direction").
use eframe::egui::{self, Color32, CornerRadius, FontFamily, FontId, Stroke, Vec2};
use std::sync::Arc;

pub const VIEWPORT_BG: Color32 = Color32::from_rgb(13, 14, 16);
pub const BAR: Color32 = Color32::from_rgb(22, 23, 26);
pub const PANEL: Color32 = Color32::from_rgb(27, 28, 31);
pub const FIELD: Color32 = Color32::from_rgb(35, 36, 40);
pub const BORDER: Color32 = Color32::from_rgb(44, 46, 51);
pub const HOVER: Color32 = Color32::from_rgb(46, 48, 54);
pub const TEXT: Color32 = Color32::from_rgb(212, 214, 219);
pub const STRONG: Color32 = Color32::from_rgb(238, 239, 242);
pub const MUTED: Color32 = Color32::from_rgb(138, 142, 152);
pub const FAINT: Color32 = Color32::from_rgb(92, 96, 106);
pub const ACCENT: Color32 = Color32::from_rgb(82, 142, 230);
pub const ACCENT_DIM: Color32 = Color32::from_rgb(34, 58, 96);
pub const GOOD: Color32 = Color32::from_rgb(104, 176, 122);
pub const WARN: Color32 = Color32::from_rgb(212, 162, 76);
pub const BAD: Color32 = Color32::from_rgb(218, 98, 88);

pub fn fam(name: &str) -> FontFamily {
    FontFamily::Name(name.into())
}

/// 11 px regular (labels, captions).
pub fn small_font() -> FontId {
    FontId::proportional(11.0)
}

/// 11 px medium weight (section headers).
pub fn head_font() -> FontId {
    FontId::new(11.0, fam("semibold"))
}

/// 11 px monospace (coordinates, console).
pub fn mono_font() -> FontId {
    FontId::monospace(11.0)
}

pub fn setup_fonts(ctx: &egui::Context) {
    let mut f = egui::FontDefinitions::default();
    let neue = std::fs::read("/System/Library/Fonts/HelveticaNeue.ttc").ok();
    let mono = std::fs::read("/System/Library/Fonts/SFNSMono.ttf").ok().or_else(|| std::fs::read("/System/Library/Fonts/Menlo.ttc").ok());
    let mut add = |name: &str, bytes: &Option<Vec<u8>>, index: u32| -> bool {
        let Some(b) = bytes else { return false };
        let mut d = egui::FontData::from_owned(b.clone());
        d.index = index;
        d.tweak.y_offset_factor = 0.02;
        f.font_data.insert(name.into(), Arc::new(d));
        true
    };
    // Helvetica Neue: tabular lining figures by default, so live numbers don't jitter
    let body = add("ui", &neue, 0);
    let semi = add("ui-medium", &neue, 10);
    let mono_ok = add("ui-mono", &mono, 0);
    let base = f.families[&FontFamily::Proportional].clone();
    if body {
        f.families.get_mut(&FontFamily::Proportional).unwrap().insert(0, "ui".into());
    }
    if mono_ok {
        f.families.get_mut(&FontFamily::Monospace).unwrap().insert(0, "ui-mono".into());
    }
    let mut list = if semi { vec!["ui-medium".to_string()] } else { vec![] };
    list.extend(base.iter().cloned());
    f.families.insert(fam("semibold"), list);
    ctx.set_fonts(f);
}

pub fn apply(ctx: &egui::Context) {
    let mut v = egui::Visuals::dark();
    v.panel_fill = PANEL;
    v.window_fill = PANEL;
    v.window_stroke = Stroke::new(1.0_f32, BORDER);
    v.window_corner_radius = CornerRadius::same(4);
    v.menu_corner_radius = CornerRadius::same(4);
    v.window_shadow = egui::epaint::Shadow { offset: [0, 4], blur: 16, spread: 0, color: Color32::from_black_alpha(110) };
    v.popup_shadow = egui::epaint::Shadow { offset: [0, 3], blur: 10, spread: 0, color: Color32::from_black_alpha(110) };
    v.extreme_bg_color = Color32::from_rgb(19, 20, 23);
    v.faint_bg_color = Color32::from_rgb(31, 32, 36);
    v.code_bg_color = FIELD;
    v.selection.bg_fill = ACCENT_DIM;
    v.selection.stroke = Stroke::new(1.0_f32, ACCENT);
    v.hyperlink_color = ACCENT;
    v.override_text_color = None;
    let r = CornerRadius::same(3);
    v.widgets.noninteractive.bg_fill = PANEL;
    v.widgets.noninteractive.weak_bg_fill = PANEL;
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, BORDER);
    v.widgets.noninteractive.fg_stroke = Stroke::new(1.0_f32, TEXT);
    v.widgets.noninteractive.corner_radius = r;
    v.widgets.inactive.bg_fill = FIELD;
    v.widgets.inactive.weak_bg_fill = FIELD;
    v.widgets.inactive.bg_stroke = Stroke::new(1.0_f32, BORDER);
    v.widgets.inactive.fg_stroke = Stroke::new(1.0_f32, TEXT);
    v.widgets.inactive.corner_radius = r;
    v.widgets.hovered.bg_fill = HOVER;
    v.widgets.hovered.weak_bg_fill = HOVER;
    v.widgets.hovered.bg_stroke = Stroke::new(1.0_f32, Color32::from_rgb(70, 74, 82));
    v.widgets.hovered.fg_stroke = Stroke::new(1.0_f32, STRONG);
    v.widgets.hovered.corner_radius = r;
    v.widgets.hovered.expansion = 0.0;
    v.widgets.active.bg_fill = ACCENT_DIM;
    v.widgets.active.weak_bg_fill = ACCENT_DIM;
    v.widgets.active.bg_stroke = Stroke::new(1.0_f32, ACCENT);
    v.widgets.active.fg_stroke = Stroke::new(1.0_f32, STRONG);
    v.widgets.active.corner_radius = r;
    v.widgets.active.expansion = 0.0;
    v.widgets.open.bg_fill = HOVER;
    v.widgets.open.weak_bg_fill = HOVER;
    v.widgets.open.bg_stroke = Stroke::new(1.0_f32, BORDER);
    v.widgets.open.corner_radius = r;
    v.slider_trailing_fill = true;
    v.striped = true;
    v.interact_cursor = Some(egui::CursorIcon::Default);
    ctx.set_visuals(v);
    ctx.style_mut(|s| {
        s.spacing.item_spacing = Vec2::new(8.0, 4.0);
        s.spacing.button_padding = Vec2::new(8.0, 3.0);
        s.spacing.interact_size = Vec2::new(24.0, 20.0);
        s.spacing.slider_width = 120.0;
        s.spacing.window_margin = egui::Margin::same(8);
        s.spacing.menu_margin = egui::Margin::same(4);
        s.spacing.indent = 12.0;
        s.spacing.icon_width = 12.0;
        s.spacing.icon_spacing = 4.0;
        s.spacing.combo_height = 300.0;
        s.spacing.scroll = egui::style::ScrollStyle::thin();
        s.interaction.tooltip_delay = 0.35;
        use egui::TextStyle::*;
        s.text_styles.insert(Body, FontId::proportional(12.0));
        s.text_styles.insert(Button, FontId::proportional(12.0));
        s.text_styles.insert(Small, FontId::proportional(10.5));
        s.text_styles.insert(Heading, FontId::new(14.0, fam("semibold")));
        s.text_styles.insert(Monospace, FontId::monospace(11.0));
    });
}

/// Panel frame with an 8 px inner margin.
pub fn panel_frame() -> egui::Frame {
    egui::Frame::NONE.fill(PANEL).inner_margin(egui::Margin::same(8))
}

/// Section header: small medium-weight caps text with a hairline under it.
pub fn section(ui: &mut egui::Ui, title: &str) {
    ui.add_space(6.0);
    ui.label(egui::RichText::new(title.to_uppercase()).font(head_font()).color(MUTED));
    let r = ui.available_rect_before_wrap();
    ui.painter().hline(r.x_range(), r.top() + 1.0, Stroke::new(1.0_f32, BORDER));
    ui.add_space(5.0);
}

/// A value label in the monospace (tabular) font.
pub fn num(s: impl Into<String>) -> egui::RichText {
    egui::RichText::new(s).monospace().color(STRONG)
}

pub fn muted(s: impl Into<String>) -> egui::RichText {
    egui::RichText::new(s).color(MUTED)
}

pub fn faint(s: impl Into<String>) -> egui::RichText {
    egui::RichText::new(s).color(FAINT).size(10.5)
}
