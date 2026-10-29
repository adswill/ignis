//! Throwaway check (branding team): the embedded brand PNGs decode with the
//! eframe dependencies the crate already has (no new crates needed).
fn main() {
    let icon = eframe::icon_data::from_png_bytes(include_bytes!("../assets/branding/icon_256.png")).expect("icon");
    assert_eq!((icon.width, icon.height), (256, 256));
    let art = eframe::icon_data::from_png_bytes(include_bytes!("../assets/branding/title_art.png")).expect("title");
    let img = eframe::egui::ColorImage::from_rgba_unmultiplied([art.width as usize, art.height as usize], &art.rgba);
    let _vp = eframe::egui::ViewportBuilder::default().with_title("Ignis").with_icon(std::sync::Arc::new(icon));
    println!("ok: title art {:?}", img.size);
}
