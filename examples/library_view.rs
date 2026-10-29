//! Standalone window showing the design-library browser (design library team).
//!   cargo run --release --example library_view                       # browse ~/Library/Application Support/Ignis/designs
//!   IGNIS_LIBRARY_DIR=<dir> LIB_SHOT=/abs/out.png [LIB_SELECT=<id>] cargo run --release --example library_view
//!     -> screenshot after ~40 frames, then quit.
#![allow(dead_code)]

#[path = "../src/library.rs"]
mod library;

use eframe::egui;

struct App {
    browser: library::LibraryBrowser,
    open: bool,
    frame: u32,
    shot: Option<String>,
    log: Vec<String>,
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _f: &mut eframe::Frame) {
        ctx.set_visuals(egui::Visuals::dark());
        self.frame += 1;
        if self.frame == 2 {
            if let Ok(id) = std::env::var("LIB_SELECT") {
                self.browser.select(&id);
            }
        }
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("Ignis - library test harness");
            for l in &self.log {
                ui.label(l);
            }
        });
        let mut open = self.open;
        if let Some(a) = self.browser.window(ctx, &mut open) {
            self.log.push(format!("action: {a:?}"));
        }
        self.open = open;
        if let Some(path) = &self.shot {
            if self.frame == 40 {
                ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(Default::default()));
            }
            let shot = ctx.input(|i| {
                i.raw.events.iter().find_map(|e| if let egui::Event::Screenshot { image, .. } = e { Some(image.clone()) } else { None })
            });
            if let Some(img) = shot {
                let rgba = library::RgbaImage { w: img.size[0] as u32, h: img.size[1] as u32, data: img.as_raw().to_vec() };
                std::fs::write(path, library::encode_png(&rgba)).unwrap();
                println!("screenshot -> {path}");
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            if self.frame > 400 {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
        ctx.request_repaint();
    }
}

fn main() -> eframe::Result<()> {
    let opts = eframe::NativeOptions { viewport: egui::ViewportBuilder::default().with_inner_size([900.0, 680.0]), ..Default::default() };
    eframe::run_native(
        "Ignis library",
        opts,
        Box::new(|_cc| Ok(Box::new(App { browser: library::LibraryBrowser::default(), open: true, frame: 0, shot: std::env::var("LIB_SHOT").ok(), log: vec![] }))),
    )
}
