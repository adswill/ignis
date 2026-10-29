//! Compile + logic check for src/tutorial.rs (not part of the game).
#[path = "../src/tutorial.rs"]
#[allow(dead_code)]
mod tutorial;
use tutorial::{Stat, Tutorial, TutorialEvent as E};

fn main() {
    let mut t = Tutorial::new();
    assert_eq!(t.lessons().len(), 3);
    // Lesson 0: First fire
    t.start(0);
    t.on_event(E::Acknowledge);
    t.on_event(E::ElementSelected("FUEL".into()));
    for _ in 0..10 { t.on_event(E::Painted { code: "FUEL".into(), cells: 15 }); }
    t.on_event(E::ElementSelected("TRCH".into()));
    t.on_event(E::Ignited);
    for _ in 0..400 { t.on_event(E::Tick(1.0 / 60.0)); }
    t.on_event(E::ViewChanged(2));
    assert_eq!(t.current().unwrap().title, "That's fire", "{:?}", t.progress());
    t.on_event(E::Acknowledge);
    assert!(t.current().is_none() && t.is_lesson_complete(0));
    t.on_event(E::Acknowledge);
    assert!(!t.is_active());
    // Lesson 1: Mixing powders
    t.start(1);
    t.on_event(E::Acknowledge);
    t.on_event(E::ElementSelected("KNO3".into()));
    t.on_event(E::Painted { code: "KNO3".into(), cells: 90 });
    t.on_event(E::ElementSelected("CHAR".into()));
    t.on_event(E::Painted { code: "CHAR".into(), cells: 40 });
    t.on_event(E::ElementSelected("TRCH".into()));
    t.on_event(E::Stat(Stat::Burn, 0.5));
    t.on_event(E::PauseState(true));
    t.on_event(E::Acknowledge);
    assert!(t.is_lesson_complete(1), "{:?}", t.progress());
    // Lesson 2: Engines
    t.start(2);
    t.on_event(E::PauseState(false));
    t.on_event(E::SceneLoaded("Turbojet".into()));
    for _ in 0..300 { t.on_event(E::Tick(1.0 / 60.0)); }
    t.on_event(E::Stat(Stat::FuelFeed, 15.0));
    t.on_event(E::Stat(Stat::Thrust, 1.2));
    t.on_event(E::ViewChanged(5));
    t.on_event(E::Stat(Stat::WallStress, 0.5));
    t.on_event(E::Acknowledge);
    assert!(t.is_lesson_complete(2), "{:?}", t.progress());
    assert_eq!(t.save_string(), "1,1,1");
    // Overlay draws headlessly without panicking.
    let ctx = eframe::egui::Context::default();
    let mut t2 = Tutorial::new();
    t2.start(0);
    let mut rects = std::collections::HashMap::new();
    rects.insert("canvas".to_string(), eframe::egui::Rect::from_min_size([0.0, 40.0].into(), [900.0, 600.0].into()));
    rects.insert("palette".to_string(), eframe::egui::Rect::from_min_size([1000.0, 100.0].into(), [200.0, 400.0].into()));
    for i in 0..3 {
        let _ = ctx.run(eframe::egui::RawInput { screen_rect: Some(eframe::egui::Rect::from_min_size(Default::default(), [1280.0, 800.0].into())), ..Default::default() }, |ctx| {
            let _ = t2.draw_overlay(ctx, &rects);
        });
        if i == 0 { t2.on_event(E::Acknowledge); }
    }
    println!("tutorial_check OK: 3 lessons, {} / {} / {} steps", t.lessons()[0].steps.len(), t.lessons()[1].steps.len(), t.lessons()[2].steps.len());
}
