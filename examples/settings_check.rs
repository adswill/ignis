//! Compile / smoke check for src/settings.rs (settings & accessibility team).
//! cargo build --release --example settings_check && ./target/release/examples/settings_check
//! cargo test --release --example settings_check
#[path = "../src/settings.rs"]
mod settings;

use eframe::egui;

fn main() {
    let mut s = settings::Settings::default();
    let json = s.to_json();
    let back = settings::Settings::from_json(&json).expect("parse");
    assert_eq!(s, back, "round trip");
    println!("{json}");
    println!("graphics(High) = {:?}", s.graphics());
    s.battery_saver = true;
    println!("graphics(battery) = {:?}, fps {}", s.graphics(), s.effective_fps_limit(true));
    for m in settings::PaletteMode::ALL {
        let p = settings::palette_params(m);
        println!("{:<14} heat(0.5)={:?} floats={}", m.id(), p.heat_at(0.5), p.to_floats().len());
    }
    // headless UI pass over every tab, to catch panics in show()
    let ctx = egui::Context::default();
    for tab in 0..5 {
        settings::open_tab(&ctx, tab);
        for _ in 0..2 {
            let _ = ctx.run(egui::RawInput::default(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    settings::show(ui, &mut s);
                });
            });
        }
    }
    let mut open = true;
    let _ = ctx.run(egui::RawInput::default(), |ctx| {
        settings::show_window(ctx, &mut open, &mut s);
    });
    println!("capturing: {}", settings::is_capturing(&ctx));
    // save/load to scratch path
    let p = std::env::temp_dir().join("ignis_settings_check.json");
    settings::save_to(&s, &p).expect("save");
    let txt = std::fs::read_to_string(&p).unwrap();
    assert_eq!(settings::Settings::from_json(&txt).unwrap(), s);
    let _ = std::fs::remove_file(&p);
    println!("OK");
}
