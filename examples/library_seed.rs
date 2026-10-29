//! Design-library seeder + checker (design library team).
//!
//!   cargo run --release --example library_seed                    # seed ~/Library/Application Support/Ignis/designs
//!   cargo run --release --example library_seed -- --dir <path>    # seed another folder
//!   cargo run --release --example library_seed -- --force         # overwrite existing preset files
//!   cargo run --release --example library_seed -- --selftest      # codec round-trip tests only
//!   cargo run --release --example library_seed -- --sheet out.png # also write a contact sheet of all thumbnails
//!
//! Renders every preset in scene.rs (classic SCENARIOS list + SHOWCASE) and showcase.rs into .cmb + .png,
//! with colours taken from elements.rs, then re-reads everything and verifies it.
#![allow(dead_code, unused_imports)]

/// Minimal stand-in for crate::gpu (scene.rs / showcase.rs only need Op, W, H).
mod gpu {
    pub const W: u32 = 512;
    pub const H: u32 = 288;
    #[repr(C)]
    #[derive(Clone, Copy, Debug, Default, PartialEq)]
    pub struct Op {
        pub kind: u32,
        pub shape: u32,
        pub x0: i32,
        pub y0: i32,
        pub x1: i32,
        pub y1: i32,
        pub r: i32,
        pub tile: u32,
        pub mat: u32,
        pub dir: u32,
        pub _p: [u32; 2],
    }
}
#[path = "../src/elements.rs"]
mod elements;
#[path = "../src/library.rs"]
mod library;
#[path = "../src/scene.rs"]
mod scene;
#[path = "../src/showcase.rs"]
mod showcase;

use library::*;
use scene::Scene;

fn op_words(o: &gpu::Op) -> [u32; 12] {
    [o.kind, o.shape, o.x0 as u32, o.y0 as u32, o.x1 as u32, o.y1 as u32, o.r as u32, o.tile, o.mat, o.dir, o._p[0], o._p[1]]
}

/// Palette from elements.rs (same code the app should use, see the integration notes).
fn palette() -> Palette {
    let mut p = Palette::default();
    for (i, m) in elements::MATS.iter().enumerate() {
        p.set_wall(i as u32, [m.color.r(), m.color.g(), m.color.b()]);
    }
    for e in elements::all() {
        let c = [e.color.r(), e.color.g(), e.color.b()];
        match e.tool {
            elements::Tool::Matter(id) => p.set_matter(id, c, e.cat == elements::Cat::Liquids),
            elements::Tool::Part(t) => p.set_tile(t, c),
            _ => {}
        }
    }
    p
}

fn tags_for(title: &str, desc: &str, source: &str) -> Vec<String> {
    let s = format!("{title} {desc}").to_lowercase();
    let rules: [(&str, &[&str]); 10] = [
        ("rocket", &["rocket", "motor", "nozzle", "thrust"]),
        ("engine", &["jet", "engine", "turbo", "compressor", "pulse"]),
        ("explosives", &["explos", "detonat", "blast", "nitro", "anfo", "c4", "tnt", "dynamite", "cannon", "demolition"]),
        ("fire", &["fire", "flame", "burn", "candle", "flare", "fireball", "burner"]),
        ("metal", &["thermite", "molten", "melt", "forge", "lava", "iron", "steel", "alumin"]),
        ("gas", &["gas", "methane", "vapour", "cloud", "dust", "firedamp", "hydrogen"]),
        ("water", &["water", "steam", "boiler", "sprinkler", "lake", "pond"]),
        ("chemistry", &["chem", "acid", "powder", "peroxide", "sodium", "potassium", "catalyst", "permanganate"]),
        ("pressure", &["pressure", "vessel", "boiler", "vault", "atm"]),
        ("cryogenic", &["cryo", "liquid nitrogen", "dry ice", "liquid oxygen", "oxyliquit"]),
    ];
    let mut v = vec![source.to_string()];
    for (tag, keys) in rules {
        if keys.iter().any(|k| s.contains(k)) {
            v.push(tag.to_string());
        }
    }
    v
}

fn design(title: &str, desc: &str, source: &str, sc: Scene, created: u64) -> Design {
    let info = DesignInfo {
        name: title.to_string(),
        author: "Ignis".into(),
        description: desc.to_string(),
        tags: tags_for(title, desc, source),
        created,
        modified: created,
        source: source.into(),
    };
    let mut d = Design::new(gpu::W, gpu::H, sc.meta, [sc.inlet_p, sc.o2_p, 2.0, 1.0], info);
    d.ops = sc.ops.iter().map(op_words).collect();
    d
}

fn slug(s: &str) -> String {
    let mut o = String::new();
    for c in s.chars() {
        if c.is_ascii_alphanumeric() {
            o.push(c.to_ascii_lowercase());
        } else if !o.ends_with('-') && !o.is_empty() {
            o.push('-');
        }
    }
    o.trim_matches('-').to_string()
}

fn items() -> Vec<(String, Design)> {
    // Classic presets (same titles/descriptions as app.rs SCENARIOS) + a melt test.
    let classic: [(&str, &str, fn() -> Scene); 11] = [
        ("Turbojet", "Fan compressor, fuel injectors, flame holder and nozzle", Scene::turbojet),
        ("Rocket engine", "Methane + oxygen at 12 atm through a bell nozzle", Scene::rocket),
        ("Solid rocket motor", "Steel case, hollow propellant grain and nozzle, lit at the head end", Scene::solid_rocket),
        ("Campfire", "A pile of logs: wood pyrolyses, burns with smoky flames and chars away", Scene::campfire),
        ("Demolition", "An explosive charge against a titanium vault", Scene::demolition),
        ("Chemistry bench", "Powders and liquids: black powder, thermite, a kerosene pool over water, nitroglycerin...", Scene::chem_lab),
        ("Pressure vessels", "Aluminium vs steel vs titanium: which survives the blast?", Scene::vessels),
        ("Gas flare", "A pipe leaking methane, lit at the lip: a sooty, flickering buoyant flame", Scene::flare),
        ("Gas-filled room", "The room is full of lean methane/air, lit at the left: a flame front sweeps across", Scene::flame_test),
        ("Fuel cloud", "A rich methane cloud with an igniter: it burns from the outside in and rises as a fireball", Scene::fireball),
        ("Melt test", "Metal walls next to a hot flame: which one melts first?", Scene::melt_test),
    ];
    let base = 1_793_145_600u64; // 2026-10-28
    let mut v: Vec<(String, Design)> = Vec::new();
    let push = |id: String, d: Design, v: &mut Vec<(String, Design)>| {
        let mut id = id;
        if v.iter().any(|(i, _)| *i == id) {
            id.push_str("-deluxe");
        }
        v.push((id, d));
    };
    // classic presets get the newest timestamps so they lead the default "Newest" sort
    let mut t = base + 7200;
    for (title, desc, f) in classic {
        push(slug(title), design(title, desc, "preset", f(), t), &mut v);
        t -= 60;
    }
    let mut t = base;
    for (bench, title, desc, f) in scene::SHOWCASE.iter() {
        let _ = bench;
        push(slug(title), design(title, desc, "showcase", f(), t), &mut v);
        t += 60;
    }
    for (title, desc, f) in showcase::all() {
        push(slug(title), design(title, desc, "showcase", f(), t), &mut v);
        t += 60;
    }
    v
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if let Err(e) = selftest() {
        eprintln!("SELFTEST FAILED: {e}");
        std::process::exit(1);
    }
    println!("selftest ok");
    if let Some(f) = args.iter().position(|a| a == "--decode").and_then(|i| args.get(i + 1)) {
        let img = decode_png(&std::fs::read(f).expect("read")).expect("decode");
        let sum: u64 = img.data.iter().map(|&b| b as u64).sum();
        println!("decoded {f}: {}x{} sum {sum}", img.w, img.h);
        return;
    }
    if args.iter().any(|a| a == "--selftest") {
        return;
    }
    let dir = args.iter().position(|a| a == "--dir").and_then(|i| args.get(i + 1)).map(std::path::PathBuf::from).unwrap_or_else(library_dir);
    let force = args.iter().any(|a| a == "--force");
    let sheet = args.iter().position(|a| a == "--sheet").and_then(|i| args.get(i + 1)).cloned();

    let mut lib = Library::at(&dir);
    lib.palette = palette();
    let items = items();
    let t0 = std::time::Instant::now();
    let n = lib.seed(&items, force).expect("seed");
    println!("seeded {n} of {} presets into {} in {:.2}s", items.len(), dir.display(), t0.elapsed().as_secs_f32());

    // verify
    let mut bad = 0;
    for (id, d) in &items {
        match lib.load(id) {
            Ok(back) if force || n == items.len() => {
                if back != *d {
                    println!("MISMATCH {id}");
                    bad += 1;
                }
            }
            Ok(_) => {}
            Err(e) => {
                println!("LOAD FAIL {id}: {e}");
                bad += 1;
            }
        }
        if let Err(e) = lib.thumbnail(id) {
            println!("THUMB FAIL {id}: {e}");
            bad += 1;
        }
    }
    let list = lib.list();
    println!("library lists {} designs, {} tags: {:?}", list.len(), Library::all_tags(&list).len(), Library::all_tags(&list));
    for e in list.iter().take(40) {
        println!("  {:<28} {:<28} {:>7} B  [{}]", e.id, e.info.name, e.bytes, e.info.tags.join(", "));
    }

    // import/export/delete round trip in a temp sub-library
    let tmp = std::env::temp_dir().join(format!("ignis-lib-test-{}", std::process::id()));
    let tl = Library::at(&tmp);
    if let Some((id, _)) = items.first() {
        let out = tmp.with_extension("export.cmb");
        std::fs::create_dir_all(&tmp).unwrap();
        lib.export(id, &out).expect("export");
        let nid = tl.import(&out).expect("import");
        let a = lib.load(id).unwrap();
        let b = tl.load(&nid).unwrap();
        assert_eq!(a.meta, b.meta);
        assert_eq!(a.info.name, b.info.name);
        tl.update_info(&nid, DesignInfo { name: "Renamed".into(), tags: vec!["x".into()], ..Default::default() }).unwrap();
        assert_eq!(tl.load(&nid).unwrap().info.name, "Renamed");
        assert_eq!(tl.load(&nid).unwrap().meta, a.meta);
        let dup = tl.duplicate(&nid).unwrap();
        let sid = tl.save("Brief API save", &a.meta, [1.0, 2.0, 3.0, 4.0], None).unwrap();
        assert_eq!(tl.load(&sid).unwrap().params, [1.0, 2.0, 3.0, 4.0]);
        assert_eq!(tl.list().len(), 3);
        tl.delete(&dup).unwrap();
        assert_eq!(tl.list().len(), 2);
        assert!(tmp.join(".trash").read_dir().unwrap().count() >= 1);
        // an old-format CMB1 file (exact length, no chunk) imports too
        let raw = std::fs::read(lib.cmb_path(id)).unwrap();
        let old = std::env::temp_dir().join(format!("old_format-{}.cmb", std::process::id()));
        std::fs::write(&old, &raw[..28 + (gpu::W * gpu::H * 4) as usize]).unwrap();
        let oid = tl.import(&old).unwrap();
        assert!(tl.load(&oid).unwrap().info.name.starts_with("old format"));
        println!("import/export/update/duplicate/delete ok");
        let _ = std::fs::remove_dir_all(&tmp); // our own temp test folder
        let _ = std::fs::remove_file(&out);
        let _ = std::fs::remove_file(&old);
    }

    if let Some(path) = sheet {
        let cols = 6u32;
        let rows = (items.len() as u32).div_ceil(cols);
        let (tw, th) = (THUMB_W + 4, THUMB_H + 4);
        let mut img = RgbaImage::new(cols * tw, rows * th);
        for (k, (id, _)) in items.iter().enumerate() {
            let t = lib.thumbnail(id).unwrap();
            let (ox, oy) = ((k as u32 % cols) * tw + 2, (k as u32 / cols) * th + 2);
            for y in 0..t.h {
                for x in 0..t.w {
                    let s = ((y * t.w + x) * 4) as usize;
                    let o = (((oy + y) * img.w + ox + x) * 4) as usize;
                    img.data[o..o + 4].copy_from_slice(&t.data[s..s + 4]);
                }
            }
        }
        std::fs::write(&path, encode_png(&img)).unwrap();
        println!("contact sheet -> {path}");
    }
    if bad > 0 {
        println!("{bad} problems");
        std::process::exit(1);
    }
    println!("all ok");
}
