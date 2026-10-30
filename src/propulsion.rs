//! Propulsion demo registry: merges the air-breathing (propulsion_air.rs) and rocket (propulsion_rocket.rs) sets.
//! The valveless pulsejet ("pulseu") is built but withheld: it flames out after the start-up pulse instead of cycling.
#![allow(dead_code)]
use crate::scene::Scene;

#[path = "propulsion_air.rs"]
mod air;
#[path = "propulsion_rocket.rs"]
mod rocket;

type Entry = (&'static str, &'static str, fn() -> Scene);

const WITHHELD: &str = "pulseu";

pub fn all() -> Vec<Entry> {
    let mut v: Vec<Entry> = Vec::new();
    for (i, e) in air::all().into_iter().enumerate() {
        if air::NAMES[i] != WITHHELD {
            v.push(e);
        }
    }
    v.extend(rocket::all());
    v
}

pub const NAMES: [&str; 6 + 12] = {
    let mut a = [""; 18];
    let mut n = 0;
    let mut i = 0;
    while i < 7 {
        // const-compatible comparison against the withheld name
        let s = air::NAMES[i].as_bytes();
        let w = WITHHELD.as_bytes();
        let mut same = s.len() == w.len();
        let mut k = 0;
        while same && k < s.len() { if s[k] != w[k] { same = false; } k += 1; }
        if !same { a[n] = air::NAMES[i]; n += 1; }
        i += 1;
    }
    let mut j = 0;
    while j < 12 { a[n] = rocket::NAMES[j]; n += 1; j += 1; }
    a
};

pub fn by_name(name: &str) -> Option<Scene> {
    let all = all();
    NAMES.iter().position(|x| *x == name).map(|i| (all[i].2)())
}
