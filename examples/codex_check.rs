//! Compile + content check for the Ignis Codex (src/codex.rs + assets/codex/*.md).
//!   cargo run --release --example codex_check            # validate entries, links, element coverage, headless UI pass
//!   cargo run --release --example codex_check -- thermite  # also print one entry's body
#[path = "../src/codex.rs"]
mod codex;

use eframe::egui;

fn main() {
    let es = codex::entries();
    let mut errors = 0;
    println!("{} codex entries", es.len());

    // ids unique, categories known
    for (i, e) in es.iter().enumerate() {
        if es[..i].iter().any(|o| o.id == e.id) {
            println!("ERROR duplicate id {}", e.id);
            errors += 1;
        }
        if !codex::CATEGORIES.contains(&e.category) {
            println!("ERROR {} has unknown category {}", e.id, e.category);
            errors += 1;
        }
        if e.body_md.starts_with("---") {
            println!("ERROR {} front matter not stripped", e.id);
            errors += 1;
        }
    }
    // links resolve
    for e in &es {
        let mut rest = e.body_md;
        while let Some(s) = rest.find("[[") {
            let Some(end) = rest[s..].find("]]") else { break };
            let inner = &rest[s + 2..s + end];
            let target = inner.split('|').next().unwrap().trim();
            if codex::find(target).is_none() {
                println!("ERROR {} links to missing [[{}]]", e.id, target);
                errors += 1;
            }
            rest = &rest[s + end + 2..];
        }
    }
    // every wall material (0-9) and matter element (10-65) has an entry
    for id in 0..66u32 {
        if codex::for_element(id).is_none() {
            println!("ERROR no entry for element {id}");
            errors += 1;
        }
    }
    for tile in [2u32, 3, 4, 5, 6, 7] {
        if codex::for_tile(tile).is_none() {
            println!("ERROR no entry for tile {tile}");
            errors += 1;
        }
    }
    for (c, n) in codex::CATEGORIES.iter().map(|c| (c, es.iter().filter(|e| e.category == *c).count())) {
        println!("  {c:<18} {n}");
    }
    let s = codex::search("thermite");
    println!("search 'thermite' -> {:?}", s.iter().take(4).map(|e| e.id).collect::<Vec<_>>());
    let s = codex::search("rocket nozzle");
    println!("search 'rocket nozzle' -> {:?}", s.iter().take(4).map(|e| e.id).collect::<Vec<_>>());
    if let Some((id, title, fact)) = codex::random_fact(7) {
        println!("random fact: [{id}] {title}: {fact}");
    }

    // headless UI pass: render the welcome page and every entry in a narrow and a wide layout
    let ctx = egui::Context::default();
    let mut st = codex::CodexState::new();
    let mut frames = 0;
    for size in [egui::vec2(380.0, 700.0), egui::vec2(1000.0, 700.0)] {
        for idx in 0..=es.len() {
            if idx == 0 {
                st.selected = None;
            } else {
                st.open_entry(es[idx - 1].id);
            }
            for _ in 0..2 {
                let input = egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
                    ..Default::default()
                };
                let _ = ctx.run(input, |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        let _ = codex::show(ui, &mut st);
                    });
                });
                frames += 1;
            }
        }
    }
    st.open = true;
    let _ = ctx.run(egui::RawInput::default(), |ctx| {
        let _ = codex::show_window(ctx, &mut st);
    });
    println!("headless UI frames rendered: {frames}");

    // glyph coverage with egui's default fonts (the app may add more fonts; this is the worst case)
    let mut missing: std::collections::BTreeMap<char, Vec<&str>> = Default::default();
    ctx.fonts(|f| {
        let fid = egui::FontId::proportional(14.0);
        for e in &es {
            for c in e.body_md.chars().chain(e.title.chars()) {
                if !c.is_whitespace() && !f.has_glyph(&fid, c) {
                    let v = missing.entry(c).or_default();
                    if !v.contains(&e.id) { v.push(e.id); }
                }
            }
        }
    });
    for (c, ids) in &missing {
        println!("missing glyph {c:?} U+{:04X} in {} entries (e.g. {:?})", *c as u32, ids.len(), &ids[..ids.len().min(3)]);
    }

    if let Some(id) = std::env::args().nth(1) {
        match codex::find(&id) {
            Some(e) => println!("\n==== {} [{}] ====\n{}", e.title, e.category, e.body_md),
            None => println!("no entry {id}"),
        }
    }
    if errors > 0 {
        println!("{errors} errors");
        std::process::exit(1);
    }
    println!("OK");
}
