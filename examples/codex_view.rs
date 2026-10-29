//! Interactive viewer for the Ignis Codex (standalone window, no simulation).
//!   cargo run --release --example codex_view [entry_id]
//!   CODEX_SHOT=/abs/out.ppm cargo run --release --example codex_view thermite   # screenshot after a few frames, then quit
#[path = "../src/codex.rs"]
mod codex;

use eframe::egui;

struct Viewer {
    st: codex::CodexState,
    frame: u32,
    shot: Option<String>,
}

impl eframe::App for Viewer {
    fn update(&mut self, ctx: &egui::Context, _f: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            let r = codex::show(ui, &mut self.st);
            if r.use_element.is_some() || r.use_tile.is_some() {
                println!("use in sandbox: {:?}", r);
            }
        });
        self.frame += 1;
        if let Some(path) = &self.shot {
            if self.frame == 8 {
                ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(Default::default()));
            }
            let img = ctx.input(|i| i.events.iter().find_map(|e| if let egui::Event::Screenshot { image, .. } = e { Some(image.clone()) } else { None }));
            if let Some(img) = img {
                let mut out = format!("P6\n{} {}\n255\n", img.size[0], img.size[1]).into_bytes();
                for p in &img.pixels {
                    out.extend_from_slice(&[p.r(), p.g(), p.b()]);
                }
                std::fs::write(path, out).expect("write screenshot");
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            if self.frame > 200 {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
        ctx.request_repaint();
    }
}

fn main() {
    let mut st = codex::CodexState::new();
    if let Some(id) = std::env::args().nth(1) {
        st.open_entry(&id);
    }
    let shot = std::env::var("CODEX_SHOT").ok();
    let options = eframe::NativeOptions {
        renderer: eframe::Renderer::Wgpu,
        viewport: egui::ViewportBuilder::default().with_inner_size([980.0, 680.0]).with_title("Ignis Codex"),
        ..Default::default()
    };
    eframe::run_native("Ignis Codex", options, Box::new(move |_cc| Ok(Box::new(Viewer { st, frame: 0, shot })))).expect("failed to start");
}
