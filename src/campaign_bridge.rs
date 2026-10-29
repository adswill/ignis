//! Adapter: turns the data-driven campaign (`campaign.rs` + assets/campaign/campaign.json) into
//! `missions::Mission` values, so the existing mission UI / scoring in app.rs can run them unchanged.
//!
//! `Mission` wants `&'static` data and plain `fn` pointers. The campaign is parsed once into a
//! `OnceLock`; strings / slices are leaked once (a few KB, lives for the whole program) and the
//! layout / solution callbacks are monomorphised per index (`layout_at::<N>`), up to `MAX_CAMPAIGN`.
//!
//! Integration (see the campaign team's report): `mod campaign; mod campaign_bridge;` in main.rs,
//! then use `campaign_bridge::all_missions()` wherever app.rs uses `MISSIONS`.
#![allow(dead_code)]

use crate::campaign::{self, Goal as CGoal, Metric as CMetric, MissionDef, PaletteItem};
use crate::elements::Tool;
use crate::gpu::Op;
use crate::missions::{Feeds, Goal, Metric, Mission, Objective, MISSIONS};
use crate::scene::Scene;
use std::sync::OnceLock;

pub const MAX_CAMPAIGN: usize = 48;

static DEFS: OnceLock<Vec<MissionDef>> = OnceLock::new();
static CAMPAIGN: OnceLock<Vec<Mission>> = OnceLock::new();
static ALL: OnceLock<Vec<&'static Mission>> = OnceLock::new();

/// The parsed campaign definitions (ids, chapters, unlocks, hints, facts...).
pub fn defs() -> &'static [MissionDef] {
    DEFS.get_or_init(|| {
        let mut v = campaign::missions();
        v.truncate(MAX_CAMPAIGN);
        v
    })
}

/// `campaign::Metric::Count` has no `missions::Metric` twin yet. Once the UI team adds
/// `Metric::Count(u32)` (cells of an id present now), change this body to `Metric::Count(k)`.
/// Until then those objectives fall back to `Used(k)` and ice-dam and foundry-pour
/// can't be completed in the app (campaign_check scores Count itself, so both pass there).
fn count_metric(k: u32) -> Metric {
    Metric::Used(k)
}

fn metric(m: CMetric) -> Metric {
    match m {
        CMetric::Thrust => Metric::Thrust,
        CMetric::Burn => Metric::Burn,
        CMetric::Isp => Metric::Isp,
        CMetric::WallHeat => Metric::WallHeat,
        CMetric::PeakTemp => Metric::PeakTemp,
        CMetric::PeakPressure => Metric::PeakPressure,
        CMetric::StructLost => Metric::StructLost,
        CMetric::MatLost(k) => Metric::MatLost(k),
        CMetric::Used(k) => Metric::Used(k),
        CMetric::UsedWalls => Metric::UsedWalls,
        CMetric::Leak => Metric::Leak,
        CMetric::Impulse => Metric::Impulse,
        CMetric::Count(k) => count_metric(k),
    }
}

fn goal(g: CGoal) -> Goal {
    match g {
        CGoal::Reach(m, v) => Goal::Reach(metric(m), v),
        CGoal::ReachBy(m, v, s) => Goal::ReachBy(metric(m), v, s),
        CGoal::Hold(m, v, s) => Goal::Hold(metric(m), v, s),
        CGoal::Keep(m, v) => Goal::Keep(metric(m), v),
        CGoal::AtMost(m, v) => Goal::AtMost(metric(m), v),
        CGoal::HoldBelow(m, v, s) => Goal::HoldBelow(metric(m), v, s),
    }
}

fn tool(p: PaletteItem) -> Tool {
    match p {
        PaletteItem::Wall(k) => Tool::Wall(k),
        PaletteItem::Part(t) => Tool::Part(t),
        PaletteItem::Gas(k) => Tool::Gas(k),
        PaletteItem::Matter(k) => Tool::Matter(k),
        PaletteItem::Erase => Tool::Erase,
    }
}

fn op(o: &campaign::OpData) -> Op {
    Op { kind: o.kind, shape: o.shape, x0: o.x0, y0: o.y0, x1: o.x1, y1: o.y1, r: o.r, tile: o.tile, mat: o.mat, dir: o.dir, _p: [0; 2] }
}

/// Convert a built campaign scene into the game's `Scene`.
pub fn to_scene(c: &campaign::CampaignScene) -> Scene {
    let mut s = Scene::empty();
    s.meta.copy_from_slice(&c.meta);
    s.ops = c.ops.iter().map(op).collect();
    s.inlet_p = c.inlet_p;
    s.o2_p = c.o2_p;
    s
}

fn layout_at<const N: usize>() -> Scene {
    match defs().get(N) {
        Some(m) => to_scene(&campaign::build_scene(m)),
        None => Scene::empty(),
    }
}

fn solution_at<const N: usize>(s: &mut Scene) {
    let Some(m) = defs().get(N) else { return };
    let mut c = campaign::CampaignScene { meta: s.meta.clone(), ops: vec![], inlet_p: s.inlet_p, o2_p: s.o2_p };
    c.apply(&m.solution);
    s.meta = c.meta;
    s.ops.extend(c.ops.iter().map(op));
}

macro_rules! fn_table {
    ($f:ident, $t:ty; $($n:literal)*) => { [$($f::<$n> as $t),*] };
}
const LAYOUTS: [fn() -> Scene; MAX_CAMPAIGN] = fn_table!(layout_at, fn() -> Scene;
    0 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20 21 22 23 24 25 26 27 28 29 30 31 32 33 34 35 36 37 38 39 40 41 42 43 44 45 46 47);
const SOLUTIONS: [fn(&mut Scene); MAX_CAMPAIGN] = fn_table!(solution_at, fn(&mut Scene);
    0 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20 21 22 23 24 25 26 27 28 29 30 31 32 33 34 35 36 37 38 39 40 41 42 43 44 45 46 47);

fn leak_str(s: &str) -> &'static str {
    Box::leak(s.to_string().into_boxed_str())
}

fn to_mission(k: usize, m: &MissionDef) -> Mission {
    let objectives: Vec<Objective> = m.objectives.iter().map(|o| Objective { text: leak_str(&o.text), goal: goal(o.goal), required: o.required }).collect();
    let budgets: Vec<(Metric, f32)> = m.budgets.iter().map(|&(mt, v)| (metric(mt), v)).collect();
    let palette: Vec<Tool> = m.palette.iter().map(|&p| tool(p)).collect();
    Mission {
        title: leak_str(&m.title),
        tagline: leak_str(&m.tagline),
        brief: leak_str(&m.briefing),
        tip: leak_str(&m.hints.join(" ")),
        layout: LAYOUTS[k],
        solution: SOLUTIONS[k],
        live: m.live,
        locked_start: m.locked_start,
        palette: Box::leak(palette.into_boxed_slice()),
        budgets: Box::leak(budgets.into_boxed_slice()),
        feeds: Feeds { fuel: m.feeds.fuel, o2: m.feeds.o2, air: m.feeds.air, fan: m.feeds.fan, adjustable: m.feeds.adjustable },
        igniter_secs: m.igniter_secs,
        duration: m.duration,
        objectives: Box::leak(objectives.into_boxed_slice()),
        view: m.view,
        gravity: m.gravity,
        solve_at: m.solve_at,
    }
}

/// Campaign missions only, in campaign order.
pub fn campaign_missions() -> &'static [Mission] {
    CAMPAIGN.get_or_init(|| defs().iter().enumerate().map(|(k, m)| to_mission(k, m)).collect())
}

/// Hand-written missions that the campaign re-does under a new name.
const SUPERSEDED: [&str; 3] = ["Fire Brigade", "Thermite", "Pressure Vessel"];

/// Campaign missions followed by the hand-written `MISSIONS` that the campaign doesn't already
/// cover (matched by title), so nothing the UI team made is lost.
pub fn all_missions() -> &'static [&'static Mission] {
    ALL.get_or_init(|| {
        let c = campaign_missions();
        let mut v: Vec<&'static Mission> = c.iter().collect();
        for m in MISSIONS.iter() {
            let covered = c.iter().any(|x| x.title.eq_ignore_ascii_case(m.title)) || SUPERSEDED.iter().any(|t| t.eq_ignore_ascii_case(m.title));
            if !covered {
                v.push(m);
            }
        }
        v
    })
}

/// Campaign metadata for mission `idx` of `all_missions()` (None for legacy missions).
pub fn def_of(idx: usize) -> Option<&'static MissionDef> {
    defs().get(idx)
}

/// Is mission `idx` of `all_missions()` unlocked, given stars per mission (same indexing)?
/// Legacy missions are always open.
pub fn unlocked(idx: usize, stars: &[u32]) -> bool {
    let Some(m) = def_of(idx) else { return true };
    campaign::is_unlocked(m, &|id| defs().iter().position(|d| d.id == id).map(|j| stars.get(j).copied().unwrap_or(0) > 0).unwrap_or(true))
}

/// Chapter title for mission `idx` (for headers in the mission select grid).
pub fn chapter_of(idx: usize) -> Option<(u32, String)> {
    let m = def_of(idx)?;
    let ch = campaign::chapters().into_iter().find(|c| c.id == m.chapter)?;
    Some((ch.id, ch.title))
}
