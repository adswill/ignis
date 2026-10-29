//! Compile + smoke test for src/campaign_bridge.rs against the real missions/elements/scene modules.
//!   cargo build --release --example campaign_bridge_check && ./target/release/examples/campaign_bridge_check
#![allow(dead_code, unused_imports)]
#[path = "../src/gpu.rs"]
mod gpu;
#[path = "../src/scene.rs"]
mod scene;
#[path = "../src/elements.rs"]
mod elements;
#[path = "../src/missions.rs"]
mod missions;
#[path = "../src/campaign.rs"]
mod campaign;
#[path = "../src/campaign_bridge.rs"]
mod campaign_bridge;

fn main() {
    let all = campaign_bridge::all_missions();
    let mut stars = vec![0u32; all.len()];
    for (k, m) in all.iter().enumerate() {
        let mut s = (m.layout)();
        let base = s.meta.iter().filter(|&&v| v != 0).count();
        (m.solution)(&mut s);
        let solved = s.meta.iter().filter(|&&v| v != 0).count();
        let ch = campaign_bridge::chapter_of(k).map(|c| c.1).unwrap_or_else(|| "legacy".into());
        println!("{k:2} {:<22} [{ch:<30}] unlocked {:<5} pal {:2} obj {} cells {base} -> {solved}", m.title, campaign_bridge::unlocked(k, &stars), m.palette.len(), m.objectives.len());
        for t in m.palette {
            if elements::find(*t).is_none() {
                println!("     palette tool {:?} has no elements.rs entry yet", t);
            }
        }
        stars[k] = 1;
    }
}
