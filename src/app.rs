//! Ignis workspace: application state, simulation driving, experiments bookkeeping and editing tools.
//! The docked UI (menu bar, toolbar, library, inspector, bottom dock, status bar, viewport overlays)
//! lives in `ui/workspace.rs` (child module `ws`), the visual theme in `ui/theme.rs`.
use crate::elements::{self, Cat, Element, Tool, CATS, MATS};
use crate::gpu::{FrameInput, GpuSim, Op, ProbeData, Stats, DT, H, W};
use crate::missions::{Goal, Metric, Mission, Objective};
use crate::scene::{self, Scene};
use eframe::egui::{self, Align2, Color32, CornerRadius, FontId, Pos2, Rect, Sense, Stroke, StrokeKind, Vec2};
use eframe::egui_wgpu;
use std::collections::VecDeque;
use std::sync::Arc;

#[path = "ui/theme.rs"]
mod theme;
#[path = "ui/workspace.rs"]
mod ws;
#[path = "ui/probes.rs"]
mod probes;
#[path = "ui/propellant.rs"]
mod prop;
use theme::*;

/// Simulated time units per real second at 1x speed (one "game second").
const TPS: f32 = 32.0;
const MAX_STEPS: u32 = 192;
/// Physical scale used for readouts: 1 cell = 1 cm, 1 time unit = 34 us, 1 m engine depth.
const MS_PER_UNIT: f32 = 0.034;
const MPS_PER_UNIT: f32 = 293.0;
const KN_PER_UNIT: f32 = 1.013;
const KGS_PER_UNIT: f32 = 3.5;

/// All experiments: the data-driven campaign followed by the hand-written missions it doesn't cover.
fn missions_list() -> &'static [&'static Mission] {
    crate::campaign_bridge::all_missions()
}

#[derive(Clone, Copy, PartialEq)]
enum Shape {
    Brush,
    Line,
    Box,
    Frame,
    /// flood-fill the connected region of identical cells under the cursor
    Fill,
    /// scatter single grains / puffs around the cursor
    Spray,
}

/// (shape, button label, key, tooltip)
const SHAPES: [(Shape, &str, &str, &str); 6] = [
    (Shape::Brush, "Brush", "B", "Paint freehand"),
    (Shape::Line, "Line", "L", "Drag a straight line"),
    (Shape::Box, "Box", "X", "Drag a filled box"),
    (Shape::Frame, "Frame", "F", "Drag a hollow box (wall thickness = size + 1)"),
    (Shape::Fill, "Fill", "G", "Flood-fill the connected area under the cursor (an empty room, a wall, a puddle...)"),
    (Shape::Spray, "Spray", "S", "Scatter loose grains or droplets around the cursor: great for mixing powders or making dust"),
];

fn tool_key(t: Tool) -> String {
    match t {
        Tool::Wall(m) => format!("W{m}"),
        Tool::Part(p) => format!("P{p}"),
        Tool::Gas(g) => format!("G{g}"),
        Tool::Matter(m) => format!("M{m}"),
        Tool::Erase => "E".into(),
    }
}

fn parse_tool(s: &str) -> Option<Tool> {
    let (k, n) = s.split_at(1.min(s.len()));
    let n: Option<u32> = n.parse().ok();
    let t = match (k, n) {
        ("W", Some(n)) => Tool::Wall(n),
        ("P", Some(n)) => Tool::Part(n),
        ("G", Some(n)) => Tool::Gas(n),
        ("M", Some(n)) => Tool::Matter(n),
        ("E", _) => Tool::Erase,
        _ => return None,
    };
    elements::find(t).map(|e| e.tool)
}

fn data_dir() -> Option<std::path::PathBuf> {
    crate::settings::data_dir()
}


const SCENARIOS: [(&str, &str, fn() -> Scene); 11] = [
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
    ("Blank canvas", "Empty box of still air at 1 atm", Scene::empty),
];

struct StrokeState {
    right: bool,
    erase: bool,
    start: (i32, i32),
    last: (i32, i32),
}

/// Global time series for the Plots dock (sampled 10x per sim-second of wall clock, 600 samples).
#[derive(Default)]
struct History {
    /// sim time, ms
    t: VecDeque<f32>,
    thrust: VecDeque<f32>,
    p: VecDeque<f32>,
    temp: VecDeque<f32>,
    burn: VecDeque<f32>,
    wall_heat: VecDeque<f32>,
    stress: VecDeque<f32>,
}

impl History {
    fn clear(&mut self) {
        for q in [&mut self.t, &mut self.thrust, &mut self.p, &mut self.temp, &mut self.burn, &mut self.wall_heat, &mut self.stress] {
            q.clear();
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Phase {
    Build,
    Test,
    Done,
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum St {
    Pending,
    Done,
    Failed,
}

#[derive(Clone, Copy)]
struct ObjState {
    st: St,
    progress: f32,
    timer: f32,
    gap: f32,
    best: f32,
}

impl ObjState {
    fn new() -> Self {
        ObjState { st: St::Pending, progress: 0.0, timer: 0.0, gap: 0.0, best: 0.0 }
    }
}

#[derive(Clone, Copy)]
struct Counts {
    /// wall cells per material
    mat: [u32; 10],
    /// non-burnable wall cells
    structural: u32,
    /// cells per material / matter id that the player placed (not part of the starting layout)
    used: [u32; 128],
    used_walls: u32,
}

impl Default for Counts {
    fn default() -> Self {
        Counts { mat: [0; 10], structural: 0, used: [0; 128], used_walls: 0 }
    }
}

fn count(layout: &[u32], initial: &[u32]) -> Counts {
    let mut c = Counts::default();
    for (i, &v) in layout.iter().enumerate() {
        if v & 0xff == scene::MATTER {
            let id = ((v >> 8) & 0xff).min(127) as usize;
            if initial[i] != v {
                c.used[id] += 1;
            }
            continue;
        }
        if v & 0xff != scene::WALL {
            continue;
        }
        let m = ((v >> 8) & 0xff).min(9) as usize;
        c.mat[m] += 1;
        if m < 7 {
            c.structural += 1;
        }
        if initial[i] != v {
            c.used[m] += 1;
            if m < 7 {
                c.used_walls += 1;
            }
        }
    }
    c
}

/// Cells of one wall material (id < 10) or matter element (id >= 10).
fn count_id(layout: &[u32], id: u32) -> u32 {
    let want = if id < 10 { scene::WALL | (id << 8) } else { scene::MATTER | (id << 8) };
    layout.iter().filter(|&&v| v & 0xffff == want).count() as u32
}

/// Can gas starting at the initial explosive charge reach the edge of the world or a vent?
fn leaks(layout: &[u32], initial: &[u32]) -> bool {
    let (w, h) = (W as usize, H as usize);
    let passable = |v: u32| {
        let t = v & 0xff;
        !(t == scene::WALL && ((v >> 8) & 0xff) < 7 || t == scene::IGNITER)
    };
    let mut seen = vec![false; w * h];
    let mut stack: Vec<usize> = vec![];
    for (i, &v) in initial.iter().enumerate() {
        if v & 0xff == scene::WALL && (v >> 8) & 0xff == scene::EXPLOSIVE {
            seen[i] = true;
            stack.push(i);
        }
    }
    if stack.is_empty() {
        return false;
    }
    while let Some(i) = stack.pop() {
        let (x, y) = (i % w, i / w);
        if x == 0 || y == 0 || x == w - 1 || y == h - 1 || layout[i] & 0xff == scene::VENT {
            return true;
        }
        for j in [i - 1, i + 1, i - w, i + w] {
            if !seen[j] && passable(layout[j]) {
                seen[j] = true;
                stack.push(j);
            }
        }
    }
    false
}

struct Run {
    idx: usize,
    phase: Phase,
    initial: Vec<u32>,
    prefill: Vec<Op>,
    inlet_p: f32,
    o2_p: f32,
    test_layout: Option<Vec<u32>>,
    counts: Counts,
    counts_start: Counts,
    leak: bool,
    /// cells of the test-start layout (per wall material / structural) that are no longer there
    lost_mat: [u32; 10],
    lost_struct: u32,
    /// solution to apply part-way through a live mission (SHOT_SOLVE testing hook)
    solve_pending: bool,
    /// static force on the structure measured while building (hydrostatic load on floors etc.);
    /// subtracted from the thrust during the test, like zeroing a load cell
    tare: Vec2,
    /// live missions: cells the player actually painted, per material / element id (measured per stroke,
    /// so condensation, melting or reactions don't count as "used")
    live_used: [u32; 128],
    stroke_base: Option<(u32, u32)>,
    /// matter cells per element id that are burning / reacting (WALL.z emission kind 3 or 4)
    burning: [u32; 128],
    t: f32,
    thrust: f32,
    burn: f32,
    impulse: f32,
    obj: Vec<ObjState>,
    passed: bool,
    stars: u32,
    new_best: bool,
    fail_reason: String,
    done_at: f64,
    show_brief: bool,
    show_result: bool,
    log_t: f32,
}

impl Run {
    fn m(&self) -> &'static Mission {
        missions_list()[self.idx]
    }

    fn value(&self, m: Metric, stats: &Stats) -> f32 {
        match m {
            Metric::Thrust => self.thrust,
            Metric::Burn => self.burn,
            Metric::Isp => {
                if self.burn > 0.05 {
                    self.thrust * 1000.0 / (self.burn * 5.0 * 9.81)
                } else {
                    0.0
                }
            }
            Metric::WallHeat => stats.max_wall_heat * 100.0,
            Metric::PeakTemp => stats.max_t * 300.0,
            Metric::PeakPressure => stats.max_p,
            Metric::StructLost => self.lost_struct as f32,
            Metric::MatLost(k) => self.lost_mat[(k as usize).min(9)] as f32,
            // after a test starts, report what was placed at the start (charges get used up)
            Metric::Used(k) if self.m().live => self.live_used[(k as usize).min(127)] as f32,
            Metric::Used(k) => self.placed().used[(k as usize).min(127)] as f32,
            Metric::UsedWalls => self.placed().used_walls as f32,
            Metric::Leak => self.leak as u32 as f32,
            Metric::Impulse => self.impulse,
            Metric::Burning(k) => self.burning[(k as usize).min(127)] as f32,
        }
    }

    fn placed(&self) -> &Counts {
        if self.phase != Phase::Build && !self.m().live {
            &self.counts_start
        } else {
            &self.counts
        }
    }

    /// Budgets that are currently exceeded.
    fn over_budget(&self) -> Option<String> {
        for &(m, v) in self.m().budgets {
            let have = self.value(m, &Stats::default());
            if have > v {
                return Some(format!("{} over budget: {:.0} / {:.0}", metric_name(m), have, v));
            }
        }
        None
    }
}

fn metric_name(m: Metric) -> String {
    match m {
        Metric::Used(k) | Metric::MatLost(k) if k < 10 => MATS[k as usize].name.to_string(),
        Metric::Used(k) | Metric::MatLost(k) => elements::matter_name(k).to_string(),
        Metric::UsedWalls => "Walls".into(),
        _ => format!("{m:?}"),
    }
}

pub struct App {
    run: Option<Run>,
    /// per experiment: 0 not passed, 1..3 passed (bonus objectives met); used for unlock order only
    stars: Vec<u32>,
    tool: Tool,
    prev_tool: Tool,
    recent: VecDeque<Tool>,
    favs: Vec<Tool>,
    settings: crate::settings::Settings,
    show_settings: bool,
    applied_scale: f32,
    units: ws::Units,
    /// extra edit ops (flood fills) sent a few hundred per frame
    op_queue: VecDeque<Op>,
    fill_request: Option<((i32, i32), bool)>,
    picker: bool,
    rng: u32,
    /// the scene has engine parts (inlets, fans, propellant): show thrust prominently
    engine_scene: bool,
    /// sandbox load-cell zero (Zero button)
    tare: Vec2,
    search: String,
    shape: Shape,
    brush: i32,
    fan_dir: u32,
    view: u32,
    paused: bool,
    speed: f32,
    step_once: bool,
    p_fuel: f32,
    p_o2: f32,
    p_air: f32,
    fan_pow: f32,
    ign_on: bool,
    gravity_on: bool,
    pending_scene: Option<Scene>,
    pending_meta: Option<Vec<u32>>,
    reset_gas: bool,
    undo: Vec<Vec<u32>>,
    redo: Vec<Vec<u32>>,
    zoom: f32,
    fit_req: bool,
    center: Vec2,
    stroke: Option<StrokeState>,
    hover: Option<(i32, i32)>,
    last_probe_cell: (i32, i32),
    step_acc: f32,
    /// accumulated shift+scroll distance (points) for brush-size changes
    brush_scroll: f32,
    /// self-tuning cap on sim steps per frame (keeps the frame rate smooth; the sim slows instead)
    dyn_cap: f32,
    last_capped: bool,
    frame: u32,
    sim_time: f32,
    rt_factor: f32,
    /// Library: 0 category, 1 name, 2 density, 3 T*; descending flag; category filter chip.
    lib_sort: u8,
    lib_desc: bool,
    lib_filter: Option<Cat>,
    /// Plots: rolling window in ms (0 = whole run) and log10 pressure axis.
    plot_win: f32,
    plot_log_p: bool,
    insp_tool: Option<Tool>,
    /// Learned wall-clock cost of one solver step (ms), from frames that ran longer than vsync.
    step_cost_ms: f32,
    /// Smoothed simulated milliseconds advanced per wall-clock second (for the 1 : N readout).
    sim_rate: f32,
    steps_last: u32,
    frame_ms: f32,
    hist: History,
    hist_timer: f32,
    dock_left: bool,
    dock_right: bool,
    dock_bottom: bool,
    bottom_tab: ws::BottomTab,
    log: VecDeque<ws::LogEntry>,
    status_msg: Option<(String, f64, ws::Level)>,
    readback_in: i32,
    readback_timer: f32,
    shot_init: bool,
    shot_taken: bool,
    scene_name: String,
    show_grid: bool,
    show_legend: bool,
    show_about: bool,
    show_templates: bool,
    show_engine: bool,
    engine_p: crate::engine::EngineParams,
    show_shortcuts: bool,
    show_experiments: bool,
    probes: probes::ProbeSet,
    place: Option<probes::Place>,
    diag: probes::Diagnostics,
    codex: crate::codex::CodexState,
    show_codex: bool,
    /// which modal windows were open last frame (bit per window); opening one closes the others
    modal_mask: u8,
    /// Preferences window tab: 0 settings, 1 units
    pref_tab: u8,
    /// inlet / fan tiles present in the domain: fuel, O2, air, fan
    inlets: [bool; 4],
    library: crate::library::LibraryBrowser,
    show_library: bool,
    tutorial: crate::tutorial::Tutorial,
    tut_rects: std::collections::HashMap<String, Rect>,
    audio: Option<crate::audio::Audio>,
    pub(crate) acoustics: Option<crate::audio::acoustics::Link>,
    audio_reset: bool,
    hist_raw_thrust: Vec2,
    pin_cell: bool,
    /// active composition brush (Compositions / Propellant Designer)
    comp: Option<prop::Brush>,
    show_designer: bool,
    designer: prop::Designer,
    /// inspector: local mixture around the cursor cell
    mix_sample: Option<prop::MixSample>,
    inflight: Option<(Arc<std::sync::atomic::AtomicBool>, Arc<FrameInput>)>,
}

#[derive(Clone)]
struct Cb {
    input: Arc<FrameInput>,
    /// set once the GPU consumed this input (egui may discard a whole frame, e.g. on its sizing pass)
    used: Arc<std::sync::atomic::AtomicBool>,
}

impl egui_wgpu::CallbackTrait for Cb {
    fn prepare(
        &self,
        device: &eframe::wgpu::Device,
        queue: &eframe::wgpu::Queue,
        _sd: &egui_wgpu::ScreenDescriptor,
        enc: &mut eframe::wgpu::CommandEncoder,
        res: &mut egui_wgpu::CallbackResources,
    ) -> Vec<eframe::wgpu::CommandBuffer> {
        if let Some(sim) = res.get_mut::<GpuSim>() {
            sim.frame(device, queue, enc, &self.input);
            self.used.store(true, std::sync::atomic::Ordering::Release);
        }
        vec![]
    }

    fn paint(&self, _info: egui::PaintCallbackInfo, rp: &mut eframe::wgpu::RenderPass<'static>, res: &egui_wgpu::CallbackResources) {
        if let Some(sim) = res.get::<GpuSim>() {
            sim.draw(rp);
        }
    }
}

fn progress_path() -> Option<std::path::PathBuf> {
    crate::settings::data_dir().map(|d| d.join("experiments.txt"))
}

/// Mission order of the first release, used to read old space-separated progress files.
const OLD_ORDER: [&str; 9] = ["First Light", "Campfire", "Blast Door", "Pressure Vessel", "Hot Section", "Liftoff", "Solid Motor", "Thermite", "Fire Brigade"];

/// Stars per mission. Stored as `Title=stars` lines so missions can be reordered or added.
fn load_progress() -> Vec<u32> {
    let ms = missions_list();
    let mut v = vec![0; ms.len()];
    let Some(s) = progress_path().and_then(|p| std::fs::read_to_string(p).ok()) else { return v };
    let mut set = |title: &str, n: u32| {
        if let Some(k) = ms.iter().position(|m| m.title.eq_ignore_ascii_case(title.trim())) {
            v[k] = v[k].max(n.min(3));
        }
    };
    if s.contains('=') {
        for line in s.lines() {
            if let Some((t, n)) = line.split_once('=') {
                set(t, n.trim().parse().unwrap_or(0));
            }
        }
    } else {
        for (k, w) in s.split_whitespace().filter_map(|w| w.parse::<u32>().ok()).enumerate() {
            if let Some(t) = OLD_ORDER.get(k) {
                set(t, w);
            }
        }
    }
    v
}

fn save_progress(v: &[u32]) {
    if std::env::var("COMBUSTOR_SHOT").is_ok() {
        return;
    }
    if let Some(p) = progress_path() {
        if let Some(d) = p.parent() {
            let _ = std::fs::create_dir_all(d);
        }
        let s: Vec<String> = missions_list().iter().zip(v).map(|(m, x)| format!("{}={x}", m.title)).collect();
        let _ = std::fs::write(p, s.join("\n"));
    }
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let rs = cc.wgpu_render_state.as_ref().expect("wgpu backend");
        let mut sim = GpuSim::new(&rs.device, rs.target_format);
        let shot_mode = std::env::var("COMBUSTOR_SHOT").is_ok();
        let acoustics = if shot_mode {
            None
        } else {
            let link = {
                let (u0, meta, wall) = sim.buffers();
                crate::audio::acoustics::Link::new(&rs.device, u0, meta, wall, crate::gpu::W, crate::gpu::H)
            };
            { let l = link.clone(); sim.pre_hook = Some(Box::new(move |q, steps, reset| l.pre(q, steps, reset))); }
            { let l = link.clone(); sim.step_hook = Some(Box::new(move |p, bg| l.step(p, bg))); }
            { let l = link.clone(); sim.post_hook = Some(Box::new(move |d, e| l.post(d, e))); }
            Some(link)
        };
        rs.renderer.write().callback_resources.insert(sim);
        theme::setup_fonts(&cc.egui_ctx);
        theme::apply(&cc.egui_ctx);
        let settings = crate::settings::load();
        let favs: Vec<Tool> = settings.favs.iter().filter_map(|k| parse_tool(k)).collect();
        let shot = std::env::var("COMBUSTOR_SHOT").is_ok();
        let audio = if shot { None } else { crate::audio::Audio::new() };
        let mut codex = crate::codex::CodexState::new();
        codex.bold_family = Some(egui::FontFamily::Name("semibold".into()));
        codex.heading_family = Some(egui::FontFamily::Name("semibold".into()));
        let mut app = App {
            run: None,
            stars: load_progress(),
            tool: Tool::Wall(0),
            prev_tool: Tool::Erase,
            recent: VecDeque::new(),
            favs,
            speed: settings.start_speed,
            settings,
            show_settings: false,
            applied_scale: 0.0,
            units: ws::Units::load(),
            op_queue: VecDeque::new(),
            fill_request: None,
            picker: false,
            rng: 0x1234_5678,
            engine_scene: true,
            tare: Vec2::ZERO,
            search: String::new(),
            shape: Shape::Brush,
            brush: 1,
            fan_dir: 0,
            view: 0,
            paused: false,
            step_once: false,
            p_fuel: 1.0,
            p_o2: 1.0,
            p_air: 2.0,
            fan_pow: 1.0,
            acoustics,
            ign_on: true,
            gravity_on: true,
            pending_scene: None,
            pending_meta: None,
            reset_gas: false,
            undo: vec![],
            redo: vec![],
            zoom: 1.0,
            fit_req: false,
            center: Vec2::new(W as f32 / 2.0, H as f32 / 2.0),
            stroke: None,
            hover: None,
            last_probe_cell: (0, 0),
            step_acc: 0.0,
            brush_scroll: 0.0,
            dyn_cap: 24.0,
            last_capped: false,
            frame: 0,
            sim_time: 0.0,
            rt_factor: 1.0,
            lib_sort: 0,
            lib_desc: false,
            lib_filter: None,
            plot_win: 0.0,
            plot_log_p: false,
            insp_tool: None,
            step_cost_ms: 0.4,
            sim_rate: 0.0,
            steps_last: 0,
            frame_ms: 16.7,
            hist: History::default(),
            hist_timer: 0.0,
            dock_left: true,
            dock_right: true,
            dock_bottom: true,
            bottom_tab: ws::BottomTab::Plots,
            log: VecDeque::new(),
            status_msg: None,
            readback_in: -1,
            readback_timer: 0.0,
            shot_init: false,
            shot_taken: false,
            scene_name: String::new(),
            show_grid: false,
            show_legend: true,
            show_about: false,
            show_templates: false,
            show_engine: false,
            engine_p: crate::engine::EngineParams::otto(),
            show_shortcuts: false,
            show_experiments: false,
            probes: probes::ProbeSet::default(),
            place: None,
            diag: probes::Diagnostics::default(),
            codex,
            show_codex: false,
            modal_mask: 0,
            pref_tab: 0,
            inlets: [false; 4],
            library: crate::library::LibraryBrowser::default(),
            show_library: false,
            tutorial: crate::tutorial::Tutorial::new(),
            tut_rects: Default::default(),
            audio,
            audio_reset: true,
            hist_raw_thrust: Vec2::ZERO,
            inflight: None,
            pin_cell: false,
            comp: None,
            show_designer: false,
            designer: prop::Designer::default(),
            mix_sample: None,
        };
        // default experiment: a methane/oxygen rocket chamber with a probe in the throat region
        if let Some(t) = data_dir().and_then(|d| std::fs::read_to_string(d.join("tutorial.txt")).ok()) {
            app.tutorial.load_string(&t);
        }
        app.start_sandbox_named("Rocket engine", Scene::rocket());
        app.log_info("Ignis workspace ready. Default experiment: Rocket engine (CH4/O2, 12 atm feed).");
        app
    }

    fn load_scene(&mut self, sc: Scene) {
        let sc_meta = sc.meta.clone();
        self.inlets = [false; 4];
        for &v in sc.meta.iter() {
            let t = v & 0xff;
            if (2..=5).contains(&t) {
                self.inlets[(t - 2) as usize] = true;
            }
        }
        self.engine_scene = sc.meta.iter().any(|&v| {
            let t = v & 0xff;
            (2..=5).contains(&t) || (t == scene::WALL && (v >> 8) & 0xff == scene::PROPELLANT)
        });
        self.tare = Vec2::ZERO;
        self.p_fuel = sc.inlet_p;
        self.p_o2 = sc.o2_p;
        self.p_air = sc.p_air;
        self.pending_scene = Some(sc);
        self.sim_time = 0.0;
        self.undo.clear();
        self.redo.clear();
        self.hist.clear();
        self.probes.reset_series();
        self.diag = probes::Diagnostics::default();
        self.audio_reset = true;
        self.fit_view(&sc_meta);
    }

    /// Frame the occupied part of the layout (with room for a plume to the right) in the viewport.
    fn fit_view(&mut self, meta: &[u32]) {
        let (mut x0, mut x1, mut y0, mut y1) = (i32::MAX, i32::MIN, i32::MAX, i32::MIN);
        for (i, &v) in meta.iter().enumerate() {
            if v & 0xff != 0 {
                let (x, y) = ((i % W as usize) as i32, (i / W as usize) as i32);
                x0 = x0.min(x);
                x1 = x1.max(x);
                y0 = y0.min(y);
                y1 = y1.max(y);
            }
        }
        if x0 > x1 || x1 - x0 < 8 {
            self.zoom = 1.0;
            self.center = Vec2::new(W as f32 / 2.0, H as f32 / 2.0);
            return;
        }
        let (bw, bh) = ((x1 - x0 + 1) as f32, (y1 - y0 + 1) as f32);
        let right = if self.engine_scene { 0.6 } else { 0.15 };
        let (fw, fh) = (bw * (1.0 + 0.1 + right), bh * 1.35);
        self.zoom = (W as f32 / fw).min(H as f32 / fh).clamp(1.0, 6.0);
        self.center = Vec2::new(x0 as f32 + bw * (right - 0.1) / 2.0 + bw / 2.0, y0 as f32 + bh / 2.0);
    }

    fn start_sandbox_named(&mut self, name: &str, sc: Scene) {
        self.start_sandbox(sc);
        self.scene_name = name.to_string();
        self.log_info(&format!("Loaded experiment setup: {name}"));
        self.tutorial.on_event(crate::tutorial::TutorialEvent::SceneLoaded(name.to_string()));
    }

    fn start_sandbox(&mut self, sc: Scene) {
        self.run = None;
        self.scene_name = "Untitled".into();
        self.gravity_on = true;
        self.ign_on = true;
        self.fan_pow = 1.0;
        self.p_air = 2.0;
        self.paused = false;
        self.load_scene(sc);
    }

    fn start_mission(&mut self, idx: usize, solve: bool) {
        let m: &'static Mission = missions_list()[idx];
        let mut sc = (m.layout)();
        let initial = sc.meta.clone();
        let prefill = sc.ops.clone();
        if solve && !m.live {
            (m.solution)(&mut sc);
        }
        let (inlet_p, o2_p) = (sc.inlet_p, sc.o2_p);
        let counts = count(&sc.meta, &initial);
        self.load_scene(sc);
        self.p_fuel = m.feeds.fuel;
        self.p_o2 = m.feeds.o2;
        self.p_air = m.feeds.air;
        self.fan_pow = m.feeds.fan;
        self.gravity_on = m.gravity;
        self.view = m.view;
        self.paused = false;
        self.speed = 1.0;
        self.scene_name = crate::missions::lab_title(m.title).to_string();
        self.bottom_tab = ws::BottomTab::Experiment;
        self.dock_bottom = true;
        self.tool = m.palette.first().copied().unwrap_or(Tool::Erase);
        let mut run = Run {
            idx,
            phase: if m.live { Phase::Test } else { Phase::Build },
            initial,
            prefill,
            inlet_p,
            o2_p,
            test_layout: None,
            counts,
            counts_start: counts,
            leak: false,
            lost_mat: [0; 10],
            lost_struct: 0,
            solve_pending: false,
            tare: Vec2::ZERO,
            live_used: [0; 128],
            stroke_base: None,
            burning: [0; 128],
            t: 0.0,
            thrust: 0.0,
            burn: 0.0,
            impulse: 0.0,
            obj: m.objectives.iter().map(|_| ObjState::new()).collect(),
            passed: false,
            stars: 0,
            new_best: false,
            fail_reason: String::new(),
            done_at: 0.0,
            show_brief: !solve,
            show_result: false,
            log_t: 0.0,
        };
        if m.live {
            run.test_layout = Some(run.initial.clone());
            run.solve_pending = solve;
        }
        self.run = Some(run);
        self.readback_in = 2;
    }

    fn begin_test(&mut self, frame: &eframe::Frame) {
        let Some(layout) = self.current_meta(frame) else { return };
        let Some(run) = self.run.as_mut() else { return };
        run.counts = count(&layout, &run.initial);
        if let Some(why) = run.over_budget() {
            self.log_warn(&why);
            return;
        }
        run.counts_start = run.counts;
        run.leak = leaks(&layout, &run.initial);
        run.test_layout = Some(layout.clone());
        run.lost_mat = [0; 10];
        run.lost_struct = 0;
        run.t = 0.0;
        run.thrust = 0.0;
        run.burn = 0.0;
        run.impulse = 0.0;
        run.log_t = 0.0;
        run.obj = run.m().objectives.iter().map(|_| ObjState::new()).collect();
        run.phase = Phase::Test;
        run.show_result = false;
        let prefill = run.prefill.clone();
        let (inlet_p, o2_p) = (run.inlet_p, run.o2_p);
        // start-of-test static checks (budget objectives)
        let stats = Stats::default();
        for (k, o) in run.m().objectives.iter().enumerate() {
            if let Goal::AtMost(m, v) = o.goal {
                let x = run.value(m, &stats);
                run.obj[k].st = if x <= v { St::Done } else { St::Failed };
                run.obj[k].progress = 1.0;
            }
        }
        self.pending_scene = Some(Scene { meta: layout, inlet_p, o2_p, ops: prefill, p_air: self.p_air, engine: vec![] });
        self.sim_time = 0.0;
        self.paused = false;
        self.stroke = None;
        self.log_info("Test run started. Geometry is locked until the run ends.");
        self.readback_timer = 0.0;
    }

    fn back_to_build(&mut self) {
        let Some(run) = self.run.as_mut() else { return };
        if run.m().live {
            let idx = run.idx;
            self.start_mission(idx, false);
            if let Some(r) = self.run.as_mut() {
                r.show_brief = false;
            }
            return;
        }
        if let Some(l) = run.test_layout.take() {
            self.pending_meta = Some(l);
        }
        self.reset_gas = true;
        run.phase = Phase::Build;
        run.show_result = false;
        run.t = 0.0;
        run.obj = run.m().objectives.iter().map(|_| ObjState::new()).collect();
        self.sim_time = 0.0;
        self.readback_in = 2;
    }

    /// End the running test and score it.
    fn finish_test(&mut self, now: f64) {
        let Some(run) = self.run.as_mut() else { return };
        if run.phase != Phase::Test {
            return;
        }
        let m = run.m();
        for (k, o) in m.objectives.iter().enumerate() {
            if run.obj[k].st == St::Pending {
                run.obj[k].st = if matches!(o.goal, Goal::Keep(..) | Goal::AtMost(..)) { St::Done } else { St::Failed };
            }
        }
        run.passed = m.objectives.iter().zip(&run.obj).all(|(o, s)| !o.required || s.st == St::Done);
        let bonus = m.objectives.iter().zip(&run.obj).filter(|(o, s)| !o.required && s.st == St::Done).count() as u32;
        run.stars = if run.passed { (1 + bonus).min(3) } else { 0 };
        run.fail_reason = m
            .objectives
            .iter()
            .zip(&run.obj)
            .find(|(o, s)| o.required && s.st != St::Done)
            .map(|(o, _)| o.label())
            .unwrap_or_default();
        run.phase = Phase::Done;
        run.done_at = now;
        run.show_result = true;
        let msg = if run.passed { format!("Run complete: PASS ({} of {} objectives met, t = {:.1} s)", run.obj.iter().filter(|o| o.st == St::Done).count(), run.obj.len(), run.t) } else { format!("Run complete: FAIL ({})", run.fail_reason) };
        let passed = run.passed;
        run.new_best = run.stars > self.stars[run.idx];
        if run.new_best {
            self.stars[run.idx] = run.stars;
            if std::env::var("COMBUSTOR_SHOT").is_err() {
                save_progress(&self.stars);
            }
        }
        if passed {
            self.log_info(&msg);
        } else {
            self.log_warn(&msg);
        }
    }

    /// Per-frame mission logic: clocks, smoothing, objectives.
    fn tick_mission(&mut self, stats: &Stats, dt_game: f32, now: f64) {
        let Some(run) = self.run.as_mut() else { return };
        let raw = Vec2::new(stats.thrust[0], stats.thrust[1]);
        let a = 1.0 - (-dt_game / 0.35).exp();
        if run.phase == Phase::Build && dt_game > 0.0 {
            let k = 1.0 - (-dt_game / 1.0).exp();
            run.tare += (raw - run.tare) * k;
        }
        let thrust_now = (raw - run.tare).length() * KN_PER_UNIT;
        let burn_now = stats.burn * KGS_PER_UNIT;
        run.thrust += (thrust_now - run.thrust) * a;
        run.burn += (burn_now - run.burn) * a;
        if run.phase != Phase::Test || dt_game <= 0.0 {
            return;
        }
        run.t += dt_game;
        run.impulse += thrust_now * dt_game;
        let m = run.m();
        let t = run.t;
        let mut newly_done: Vec<String> = vec![];
        for (k, o) in m.objectives.iter().enumerate() {
            let st = run.obj[k];
            if st.st != St::Pending {
                continue;
            }
            let mut s = st;
            match o.goal {
                Goal::Reach(mt, v) | Goal::ReachBy(mt, v, _) => {
                    let x = run.value(mt, stats);
                    s.best = s.best.max(x);
                    s.progress = (s.best / v).clamp(0.0, 1.0);
                    if x >= v {
                        s.st = St::Done;
                    } else if let Goal::ReachBy(_, _, by) = o.goal {
                        if t > by {
                            s.st = St::Failed;
                        }
                    }
                }
                Goal::Hold(mt, v, secs) => {
                    let x = run.value(mt, stats);
                    if x >= v {
                        s.timer += dt_game;
                        s.gap = 0.0;
                    } else {
                        s.gap += dt_game;
                        if s.gap > 0.6 {
                            s.timer = 0.0;
                        }
                    }
                    s.best = s.best.max(x);
                    s.progress = (s.timer / secs).clamp(0.0, 1.0);
                    if s.timer >= secs {
                        s.st = St::Done;
                    }
                }
                Goal::Keep(mt, v) => {
                    let x = run.value(mt, stats);
                    s.best = s.best.max(x);
                    s.progress = (t / m.duration).clamp(0.0, 1.0);
                    if x > v {
                        s.st = St::Failed;
                    }
                }
                Goal::AtMost(mt, v) => {
                    // live missions: a budget that must never be exceeded
                    let x = run.value(mt, stats);
                    s.best = s.best.max(x);
                    if x > v {
                        s.st = St::Failed;
                    }
                }
                Goal::HoldBelow(mt, v, secs) => {
                    let x = run.value(mt, stats);
                    s.best = s.best.max(x);
                    if s.best > v * 3.0 && x <= v {
                        s.timer += dt_game;
                    } else if x > v {
                        s.timer = 0.0;
                    }
                    s.progress = (s.timer / secs).clamp(0.0, 1.0);
                    if s.timer >= secs {
                        s.st = St::Done;
                    }
                }
            }
            if s.st == St::Done {
                newly_done.push(o.label());
            }
            if s.st == St::Failed && o.required {
                run.fail_reason = o.label();
            }
            run.obj[k] = s;
        }
        let required_failed = m.objectives.iter().zip(&run.obj).any(|(o, s)| o.required && s.st == St::Failed);
        let all_resolved = run.obj.iter().all(|s| s.st != St::Pending);
        // once every goal that can still be "achieved" is settled, the remaining keep-it-safe goals count as met
        let has_active = m.objectives.iter().any(|o| !matches!(o.goal, Goal::Keep(..) | Goal::AtMost(..)));
        let active_resolved = m.objectives.iter().zip(&run.obj).all(|(o, s)| matches!(o.goal, Goal::Keep(..) | Goal::AtMost(..)) || s.st != St::Pending);
        let end = required_failed || all_resolved || (has_active && active_resolved) || t >= m.duration;
        if std::env::var("SHOT_LOG").is_ok() && t >= run.log_t {
            run.log_t += 0.5;
            let isp = run.value(Metric::Isp, stats);
            let sts: Vec<String> = run.obj.iter().map(|s| format!("{:?}{:.2}", s.st, s.progress)).collect();
            println!(
                "t={t:5.1} thrust={:6.1} burn={:5.2} isp={isp:5.1} heat={:3.0}% pT={:4.0}K pP={:5.1} walls={} lost={} matlost0/3/7={}/{}/{} leak={} imp={:.0} | {}",
                run.thrust,
                run.burn,
                stats.max_wall_heat * 100.0,
                stats.max_t * 300.0,
                stats.max_p,
                run.counts.structural,
                run.value(Metric::StructLost, stats),
                run.value(Metric::MatLost(0), stats),
                run.value(Metric::MatLost(3), stats),
                run.value(Metric::MatLost(7), stats),
                run.leak,
                run.impulse,
                sts.join(" ")
            );
        }
        for d in newly_done {
            self.log_info(&format!("Objective met: {d}"));
        }
        if end {
            self.finish_test(now);
        }
    }

    fn cam(&self) -> [f32; 4] {
        let w = W as f32 / self.zoom;
        let h = H as f32 / self.zoom;
        let cx = self.center.x.clamp(w / 2.0, W as f32 - w / 2.0);
        let cy = self.center.y.clamp(h / 2.0, H as f32 - h / 2.0);
        [cx - w / 2.0, cy - h / 2.0, w, h]
    }

    fn snapshot(&mut self, frame: &eframe::Frame) {
        if let Some(m) = self.current_meta(frame) {
            self.undo.push(m);
            if self.undo.len() > 40 {
                self.undo.remove(0);
            }
            self.redo.clear();
        }
    }

    fn current_meta(&self, frame: &eframe::Frame) -> Option<Vec<u32>> {
        let rs = frame.wgpu_render_state()?;
        let r = rs.renderer.read();
        let sim = r.callback_resources.get::<GpuSim>()?;
        Some(sim.read_meta(&rs.device, &rs.queue))
    }

    fn can_edit_layout(&self) -> bool {
        match &self.run {
            None => true,
            Some(r) => r.phase == Phase::Build,
        }
    }

    fn do_undo(&mut self, frame: &eframe::Frame) {
        if !self.can_edit_layout() {
            return;
        }
        if let Some(prev) = self.undo.pop() {
            if let Some(cur) = self.current_meta(frame) {
                self.redo.push(cur);
            }
            self.pending_meta = Some(prev);
            self.readback_in = 2;
            self.toast("Undone");
        }
    }

    fn do_redo(&mut self, frame: &eframe::Frame) {
        if !self.can_edit_layout() {
            return;
        }
        if let Some(next) = self.redo.pop() {
            if let Some(cur) = self.current_meta(frame) {
                self.undo.push(cur);
            }
            self.pending_meta = Some(next);
            self.readback_in = 2;
            self.toast("Redone");
        }
    }

    fn toast(&mut self, s: &str) {
        self.log_push(ws::Level::Info, s, true);
    }

    fn log_info(&mut self, s: &str) {
        self.log_push(ws::Level::Info, s, false);
    }

    fn log_warn(&mut self, s: &str) {
        self.log_push(ws::Level::Warn, s, true);
    }

    fn log_push(&mut self, level: ws::Level, s: &str, status: bool) {
        self.log.push_back(ws::LogEntry { t_ms: self.sim_time * MS_PER_UNIT, level, text: s.to_string() });
        while self.log.len() > 500 {
            self.log.pop_front();
        }
        if status || level != ws::Level::Info {
            self.status_msg = Some((s.to_string(), 0.0, level));
        }
    }

    fn design_params(&self) -> [f32; 4] {
        [self.p_fuel, self.p_o2, self.p_air, self.fan_pow]
    }

    /// File > Save as: an Ignis design file (.cmb, library format with metadata).
    fn save(&mut self, frame: &eframe::Frame) {
        if self.run.is_some() {
            self.log_warn("Saving is disabled while an experiment is loaded");
            return;
        }
        let Some(meta) = self.current_meta(frame) else { return };
        let Some(path) = rfd::FileDialog::new().add_filter("Ignis design", &["cmb"]).set_file_name(format!("{}.cmb", self.scene_name)).save_file() else {
            return;
        };
        let info = crate::library::DesignInfo { name: self.scene_name.clone(), author: crate::library::default_author(), created: crate::library::now_unix(), modified: crate::library::now_unix(), source: "user".into(), ..Default::default() };
        let d = crate::library::Design::new(W, H, meta, self.design_params(), info);
        match std::fs::write(&path, crate::library::encode_cmb(&d)) {
            Ok(_) => self.toast(&format!("Saved {}", path.display())),
            Err(e) => self.log_warn(&format!("Save failed: {e}")),
        }
    }

    fn open(&mut self) {
        let Some(path) = rfd::FileDialog::new().add_filter("Ignis design", &["cmb"]).pick_file() else { return };
        let Ok(b) = std::fs::read(&path) else {
            self.log_warn("Could not read file");
            return;
        };
        match crate::library::decode_cmb(&b) {
            Ok(d) => {
                let name = if d.info.name.is_empty() { path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default() } else { d.info.name.clone() };
                self.load_design(&name, d);
            }
            Err(e) => self.log_warn(&format!("Not an Ignis design: {e}")),
        }
    }

    fn load_design(&mut self, name: &str, d: crate::library::Design) {
        if d.w != W || d.h != H || d.meta.len() != (W * H) as usize {
            self.log_warn(&format!("Design grid {}x{} does not match {}x{}", d.w, d.h, W, H));
            return;
        }
        let mut sc = Scene::empty();
        sc.inlet_p = d.params[0];
        sc.o2_p = d.params[1];
        sc.meta = d.meta;
        sc.ops = d.ops.iter().map(|w| bytemuck::cast::<[u32; 12], Op>(*w)).collect();
        self.start_sandbox_named(name, sc);
        self.p_air = d.params[2];
        self.fan_pow = d.params[3];
    }

    fn tool_op(&self, shape: u32, a: (i32, i32), b: (i32, i32), erase: bool) -> Op {
        let mut op = Op { kind: 0, shape, x0: a.0, y0: a.1, x1: b.0, y1: b.1, r: self.brush, tile: 0, mat: 0, dir: 0, _p: [0; 2] };
        if erase {
            op.kind = 1;
            return op;
        }
        match self.tool {
            Tool::Wall(m) => {
                op.tile = scene::WALL;
                op.mat = m;
            }
            Tool::Part(t) => {
                op.tile = t;
                op.dir = self.fan_dir;
            }
            Tool::Gas(k) => {
                op.kind = k;
                op.r = self.brush + 1;
            }
            Tool::Matter(id) => {
                op.tile = scene::MATTER;
                op.mat = id;
            }
            Tool::Erase => op.kind = 1,
        }
        op
    }

    fn allowed(&self, t: Tool) -> bool {
        match &self.run {
            None => true,
            Some(r) => (t == Tool::Erase && !r.m().live) || r.m().palette.contains(&t),
        }
    }

    /// Thrust shown to the player: missions use their own tare, the sandbox the Zero button's.
    fn shown_thrust(&self, stats: &Stats) -> Vec2 {
        let raw = Vec2::new(stats.thrust[0], stats.thrust[1]);
        match &self.run {
            Some(r) => raw - r.tare,
            None => raw - self.tare,
        }
    }

    fn pick(&mut self, t: Tool) {
        if matches!(t, Tool::Part(2..=5) | Tool::Wall(8)) {
            self.engine_scene = true;
        }
        if let Tool::Part(k @ 2..=5) = t {
            self.inlets[(k - 2) as usize] = true;
        }
        if self.comp.as_ref().map(|b| Tool::Matter(b.base()) != t).unwrap_or(false) {
            self.comp = None;
        }
        if t != self.tool {
            self.prev_tool = self.tool;
            self.tool = t;
        }
        self.recent.retain(|x| *x != t);
        self.recent.push_front(t);
        self.recent.truncate(12);
        if let Some(e) = elements::find(t) {
            self.tutorial.on_event(crate::tutorial::TutorialEvent::ElementSelected(e.code.to_string()));
        }
    }

    fn toggle_fav(&mut self, t: Tool) {
        if let Some(k) = self.favs.iter().position(|x| *x == t) {
            self.favs.remove(k);
            if let Some(e) = elements::find(t) {
                self.toast(&format!("Removed {} from favourites", e.name));
            }
        } else {
            self.favs.push(t);
            if let Some(e) = elements::find(t) {
                let n = self.favs.len();
                let key = if n <= 9 { format!(" (Shift+{n})") } else { String::new() };
                self.toast(&format!("{} added to favourites{key}", e.name));
            }
        }
        self.settings.favs = self.favs.iter().map(|t| tool_key(*t)).collect();
        let _ = crate::settings::save(&self.settings);
    }

    fn rand(&mut self) -> f32 {
        let mut x = self.rng;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.rng = x;
        (x & 0xFFFFFF) as f32 / 16777216.0
    }

    /// SHOT_SOLVE hook for live missions: once the clock passes `solve_at`, paint the known-good
    /// solution's cells (the difference between the solved and the bare layout) into the running sim.
    fn apply_live_solution(&mut self) {
        let Some(run) = self.run.as_mut() else { return };
        if !run.solve_pending || run.t < run.m().solve_at {
            return;
        }
        run.solve_pending = false;
        let m = run.m();
        let base = (m.layout)();
        let mut solved = (m.layout)();
        (m.solution)(&mut solved);
        let w = W as usize;
        let mut ops: Vec<Op> = vec![];
        for y in 0..H as usize {
            let mut x = 0;
            while x < w {
                let i = y * w + x;
                let v = solved.meta[i];
                if v == base.meta[i] {
                    x += 1;
                    continue;
                }
                let mut x1 = x;
                while x1 + 1 < w && solved.meta[y * w + x1 + 1] == v && base.meta[y * w + x1 + 1] != v {
                    x1 += 1;
                }
                let tile = v & 0xff;
                let (mat, dir) = ((v >> 8) & 0xff, (v >> 16) & 0xff);
                let kind = if tile == 0 { 1 } else { 0 };
                ops.push(Op { kind, shape: 1, x0: x as i32, y0: y as i32, x1: x1 as i32, y1: y as i32, r: 0, tile, mat, dir, _p: [0; 2] });
                x = x1 + 1;
            }
        }
        ops.extend(solved.ops.iter().skip(base.ops.len()).copied());
        self.op_queue.extend(ops);
        self.readback_in = 6;
    }

    /// Eyedropper: switch to the tool that made the hovered cell.
    fn eyedrop(&mut self, pd: &ProbeData) {
        let tile = pd.meta & 0xff;
        let id = (pd.meta >> 8) & 0xff;
        let t = match tile {
            0 => {
                self.toast("Just gas here: nothing to pick");
                return;
            }
            t if t == scene::WALL => Tool::Wall(id),
            t if t == scene::MATTER => Tool::Matter(id),
            t => {
                if t == scene::FAN {
                    self.fan_dir = (pd.meta >> 16) & 0xff;
                }
                Tool::Part(t)
            }
        };
        match elements::find(t) {
            Some(e) if self.allowed(t) => {
                self.pick(t);
                self.toast(&format!("Picked {}", e.name));
            }
            Some(e) => self.log_warn(&format!("{} is not part of this experiment's materials list", e.name)),
            None => self.toast("Can't pick that"),
        }
        self.picker = false;
    }

    /// Flood-fill the region of identical cells around `at` with the current tool (or erase it).
    fn flood_fill(&mut self, frame: &eframe::Frame, at: (i32, i32), erase: bool) {
        let Some(layout) = self.current_meta(frame) else { return };
        let (w, h) = (W as i32, H as i32);
        if at.0 < 0 || at.1 < 0 || at.0 >= w || at.1 >= h {
            return;
        }
        let key = |v: u32| v & 0xffff;
        let target = key(layout[(at.1 * w + at.0) as usize]);
        let erase = erase || self.tool == Tool::Erase;
        if erase && target == 0 {
            return;
        }
        let gas_tool = matches!(self.tool, Tool::Gas(_));
        if !erase && gas_tool && target & 0xff != 0 {
            self.toast("Gas tools only fill open space");
            return;
        }
        if !erase && !gas_tool {
            let op = self.tool_op(0, at, at, false);
            if target == key(op.tile | (op.mat << 8)) {
                return;
            }
        }
        let mut seen = vec![false; (w * h) as usize];
        let mut stack = vec![(at.0, at.1)];
        seen[(at.1 * w + at.0) as usize] = true;
        let mut rows: Vec<Vec<i32>> = vec![vec![]; h as usize];
        let mut n = 0usize;
        while let Some((x, y)) = stack.pop() {
            rows[y as usize].push(x);
            n += 1;
            for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                let (nx, ny) = (x + dx, y + dy);
                if nx < 0 || ny < 0 || nx >= w || ny >= h {
                    continue;
                }
                let j = (ny * w + nx) as usize;
                if !seen[j] && key(layout[j]) == target {
                    seen[j] = true;
                    stack.push((nx, ny));
                }
            }
        }
        let mut ops = vec![];
        let replace = !erase && !gas_tool && target != 0;
        for (y, xs) in rows.iter_mut().enumerate() {
            if xs.is_empty() {
                continue;
            }
            xs.sort_unstable();
            let mut a = xs[0];
            let mut b = xs[0];
            let flush = |a: i32, b: i32, ops: &mut Vec<Op>| {
                let y = y as i32;
                if replace || erase {
                    let mut e = self.tool_op(1, (a, y), (b, y), true);
                    e.r = 0;
                    ops.push(e);
                }
                if !erase {
                    let mut o = self.tool_op(1, (a, y), (b, y), false);
                    o.shape = 1;
                    o.r = 0;
                    ops.push(o);
                }
            };
            for &x in xs.iter().skip(1) {
                if x == b + 1 {
                    b = x;
                } else {
                    flush(a, b, &mut ops);
                    a = x;
                    b = x;
                }
            }
            flush(a, b, &mut ops);
        }
        self.op_queue.extend(ops);
        let what = if erase { "Cleared" } else { "Filled" };
        self.toast(&format!("{what} {n} cells"));
    }

    fn shortcuts(&mut self, ctx: &egui::Context, frame: &eframe::Frame) {
        use crate::settings::Action as A;
        if ctx.input(|i| i.key_pressed(egui::Key::F) && i.modifiers.shift && !i.modifiers.command) && !ctx.wants_keyboard_input() {
            self.fit_req = true;
        }
        if self.fit_req {
            self.fit_req = false;
            if let Some(m) = self.current_meta(frame) {
                self.fit_view(&m);
            }
        }
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            if self.place.is_some() {
                self.place = None;
            } else if self.picker {
                self.picker = false;
            } else if !self.search.is_empty() {
                self.search.clear();
                ctx.memory_mut(|m| m.surrender_focus(egui::Id::new("search")));
            } else if self.show_settings {
                self.show_settings = false;
            }
        }
        if ctx.wants_keyboard_input() || crate::settings::is_capturing(ctx) {
            return;
        }
        let acts = ctx.input(|i| self.settings.pressed_actions(i));
        for a in acts {
            if let Some(k) = a.fav_slot() {
                if let Some(t) = self.favs.get(k).copied() {
                    if self.allowed(t) {
                        self.pick(t);
                    }
                }
                continue;
            }
            if let Some(v) = a.view_index() {
                self.set_view(v);
                continue;
            }
            match a {
                A::Pause => self.paused = !self.paused,
                A::Step => {
                    self.paused = true;
                    self.step_once = true;
                }
                A::StartTest => {
                    if let Some(r) = &self.run {
                        if r.phase == Phase::Build {
                            self.begin_test(frame);
                        }
                    }
                }
                A::SpeedSlower | A::SpeedFaster => {
                    let presets = [0.1f32, 0.25, 0.5, 1.0, 2.0, 4.0];
                    let i = presets.iter().position(|p| (*p - self.speed).abs() < 1e-3).unwrap_or(3);
                    let j = if a == A::SpeedFaster { (i + 1).min(presets.len() - 1) } else { i.saturating_sub(1) };
                    self.speed = presets[j];
                    self.toast(&format!("Time scale {}x", self.speed));
                }
                A::Mute => {
                    self.settings.muted = !self.settings.muted;
                    self.toast(if self.settings.muted { "Sound off" } else { "Sound on" });
                }
                A::ToggleIgniters if self.run.is_none() => self.ign_on = !self.ign_on,
                A::ResetZoom => self.zoom = 1.0,
                A::ShapeBrush => self.shape = Shape::Brush,
                A::ShapeLine => self.shape = Shape::Line,
                A::ShapeBox => self.shape = Shape::Box,
                A::ShapeFrame => self.shape = Shape::Frame,
                A::ShapeFill => self.shape = Shape::Fill,
                A::ShapeSpray => self.shape = Shape::Spray,
                A::BrushSmaller => self.brush = (self.brush - 1).max(0),
                A::BrushLarger => self.brush = (self.brush + 1).min(24),
                A::RotateFan => self.fan_dir = (self.fan_dir + 1) % 4,
                A::Eraser => self.pick(Tool::Erase),
                A::MixTool => self.pick(Tool::Gas(prop::OP_STIR)),
                A::CastTool => self.pick(Tool::Gas(prop::OP_CAST)),
                A::Designer => self.show_designer = !self.show_designer,
                A::PrevTool => {
                    let p = self.prev_tool;
                    if self.allowed(p) {
                        self.pick(p);
                    }
                }
                A::Picker => self.picker = !self.picker,
                A::Book => {
                    self.dock_bottom = true;
                    self.bottom_tab = ws::BottomTab::Reference;
                }
                A::Search => ctx.memory_mut(|m| m.request_focus(egui::Id::new("search"))),
                A::Drawer => self.dock_right = !self.dock_right,
                A::Help => self.show_shortcuts = !self.show_shortcuts,
                A::Undo => self.do_undo(frame),
                A::Redo => self.do_redo(frame),
                A::Save => self.save(frame),
                A::Open if self.run.is_none() => self.open(),
                _ => {}
            }
        }
    }

    fn set_view(&mut self, v: u32) {
        if self.view != v {
            self.view = v;
            self.tutorial.on_event(crate::tutorial::TutorialEvent::ViewChanged(v));
        }
    }

    /// Read the layout back from the GPU for mission bookkeeping and enforce locked cells.
    fn mission_readback(&mut self, frame: &eframe::Frame) {
        let Some(layout) = self.current_meta(frame) else { return };
        let stroking = self.stroke.is_some() || !self.op_queue.is_empty();
        let Some(run) = self.run.as_mut() else { return };
        if !stroking {
            if let Some((id, base)) = run.stroke_base.take() {
                let now = count_id(&layout, id);
                let k = (id as usize).min(127);
                run.live_used[k] += now.saturating_sub(base);
            }
        }
        let m = run.m();
        if run.phase == Phase::Build && m.locked_start && !m.live {
            let mut fixed = layout.clone();
            let mut n = 0;
            for (i, &v) in run.initial.iter().enumerate() {
                if v != 0 && fixed[i] != v {
                    fixed[i] = v;
                    n += 1;
                }
            }
            if n > 0 {
                run.counts = count(&fixed, &run.initial);
                self.pending_meta = Some(fixed);
                self.log_warn("The experiment's fixture geometry is locked");
                return;
            }
        }
        run.counts = count(&layout, &run.initial);
        if run.phase == Phase::Test {
            if let Some(start) = &run.test_layout {
                let mut lm = [0u32; 10];
                let mut ls = 0;
                for (a, b) in start.iter().zip(layout.iter()) {
                    if a & 0xff == scene::WALL && (a & 0xffff) != (b & 0xffff) {
                        let k = ((a >> 8) & 0xff).min(9) as usize;
                        lm[k] += 1;
                        if k < 7 {
                            ls += 1;
                        }
                    }
                }
                run.lost_mat = lm;
                run.lost_struct = ls;
            }
        }
        let wants_burning = m.objectives.iter().any(|o| match o.goal {
            Goal::Reach(Metric::Burning(_), _) | Goal::HoldBelow(Metric::Burning(_), _, _) | Goal::Keep(Metric::Burning(_), _) => true,
            _ => false,
        });
        if wants_burning {
            if let Some(rs) = frame.wgpu_render_state() {
                let r = rs.renderer.read();
                if let Some(sim) = r.callback_resources.get::<GpuSim>() {
                    let wall = sim.read_wall(&rs.device, &rs.queue);
                    let mut b = [0u32; 128];
                    for (i, &v) in layout.iter().enumerate() {
                        if v & 0xff == scene::MATTER {
                            let kind = (wall.get(i * 4 + 2).copied().unwrap_or(0.0) / 1000.0) as u32;
                            if kind == 3 || kind == 4 {
                                b[((v >> 8) & 0xff).min(127) as usize] += 1;
                            }
                        }
                    }
                    run.burning = b;
                }
            }
        }
        if m.objectives.iter().any(|o| matches!(o.goal, Goal::Keep(Metric::Leak, _))) {
            run.leak = leaks(&layout, &run.initial);
        }
    }
}

fn words(s: &str) -> Vec<String> {
    s.to_lowercase().split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).map(|w| w.to_string()).collect()
}

/// Does the element's name appear as whole words in the text ("salt" does not match "saltpeter")?
fn mentions(text: &str, e: &Element) -> bool {
    let t = words(text);
    let n = words(e.name);
    if n.is_empty() || t.len() < n.len() {
        return false;
    }
    t.windows(n.len()).any(|w| w == n.as_slice())
}

/// Recipes from elements.rs whose ingredients mention this element.
fn recipes_with(e: &Element) -> Vec<(&'static str, &'static str, &'static str)> {
    elements::RECIPES.iter().filter(|r| mentions(r.1, e)).map(|r| (r.0, r.1, r.2)).collect()
}


impl App {
    fn canvas(&mut self, ctx: &egui::Context, stats: &Stats, probe: Option<ProbeData>, dt_real: f32) -> (bool, u32) {
        let mut ops: Vec<Op> = vec![];
        let mut want_snapshot = false;
        let mut stroke_ended = false;
        let mut steps_out = 0;
        let play = true;
        let placing = self.place.is_some();
        let editing = !placing && self.run.as_ref().map(|r| r.phase == Phase::Build || (r.m().live && r.phase == Phase::Test)).unwrap_or(true);
        egui::CentralPanel::default().frame(egui::Frame::NONE.fill(VIEWPORT_BG)).show(ctx, |ui| {
            let full = ui.available_rect_before_wrap();
            // viewport header strip (view tabs, coordinates) and margins for axes / legend
            let header = Rect::from_min_size(full.min, Vec2::new(full.width(), 26.0));
            let avail = Rect::from_min_max(full.min + Vec2::new(30.0, 34.0), full.max - Vec2::new(if self.show_legend { 96.0 } else { 12.0 }, 26.0));
            let aspect = W as f32 / H as f32;
            let mut size = avail.size();
            let fit = size.x / size.y > aspect;
            if fit {
                size.x = size.y * aspect;
            } else {
                size.y = size.x / aspect;
            }
            let rect = Rect::from_center_size(avail.center(), size);
            let resp = ui.allocate_rect(rect.intersect(avail), if play { Sense::click_and_drag() } else { Sense::hover() });
            let cam = self.cam();
            let to_cell = |p: Pos2| -> (f32, f32) { (cam[0] + (p.x - rect.left()) / rect.width() * cam[2], cam[1] + (p.y - rect.top()) / rect.height() * cam[3]) };
            let to_screen = |cx: f32, cy: f32| -> Pos2 { Pos2::new(rect.left() + (cx - cam[0]) / cam[2] * rect.width(), rect.top() + (cy - cam[1]) / cam[3] * rect.height()) };
            let cell_px = rect.width() / cam[2];

            // zoom & pan
            let (scroll, zoom_d, mods, ptr) = ctx.input(|i| (i.smooth_scroll_delta, i.zoom_delta(), i.modifiers, i.pointer.hover_pos()));
            if play && resp.hovered() {
                if let Some(p) = ptr {
                    let mut z = zoom_d;
                    if mods.command && scroll.y != 0.0 {
                        z *= (scroll.y * 0.004).exp();
                    }
                    if mods.shift && !mods.command && (scroll.x != 0.0 || scroll.y != 0.0) {
                        // Shift+scroll resizes the brush. macOS turns shift+scroll into horizontal
                        // scrolling, so take whichever axis moved more.
                        let d = if scroll.y.abs() >= scroll.x.abs() { scroll.y } else { scroll.x };
                        self.brush_scroll += d;
                        let step = 8.0;
                        while self.brush_scroll >= step {
                            self.brush = (self.brush + 1).min(24);
                            self.brush_scroll -= step;
                        }
                        while self.brush_scroll <= -step {
                            self.brush = (self.brush - 1).max(0);
                            self.brush_scroll += step;
                        }
                    } else if (z - 1.0).abs() > 1e-4 {
                        let before = to_cell(p);
                        self.zoom = (self.zoom * z).clamp(1.0, 12.0);
                        let c2 = self.cam();
                        let after = (c2[0] + (p.x - rect.left()) / rect.width() * c2[2], c2[1] + (p.y - rect.top()) / rect.height() * c2[3]);
                        self.center.x = c2[0] + c2[2] / 2.0 + before.0 - after.0;
                        self.center.y = c2[1] + c2[3] / 2.0 + before.1 - after.1;
                    } else if !mods.command && !mods.shift && (scroll.x != 0.0 || scroll.y != 0.0) && self.zoom > 1.0 {
                        self.center -= scroll / cell_px;
                    }
                }
            }
            let c = self.cam();
            self.center = Vec2::new(c[0] + c[2] / 2.0, c[1] + c[3] / 2.0);
            if resp.dragged_by(egui::PointerButton::Middle) {
                self.center -= resp.drag_delta() / cell_px;
            }

            // painting
            let hover_cell = ptr.filter(|p| rect.contains(*p) && play).map(|p| {
                let (x, y) = to_cell(p);
                (x.floor() as i32, y.floor() as i32)
            });
            self.hover = if resp.hovered() { hover_cell } else { None };
            if let Some(hc) = self.hover {
                self.last_probe_cell = hc;
            }
            let (pdown, sdown, ppress, spress) = ctx.input(|i| {
                (
                    i.pointer.button_down(egui::PointerButton::Primary),
                    i.pointer.button_down(egui::PointerButton::Secondary),
                    i.pointer.button_pressed(egui::PointerButton::Primary),
                    i.pointer.button_pressed(egui::PointerButton::Secondary),
                )
            });
            let is_gas = matches!(self.tool, Tool::Gas(_));
            let alt = mods.alt;
            let picking = (self.picker || alt) && !placing;
            if placing && resp.hovered() && ppress {
                if let Some(hc) = hover_cell {
                    self.place_click(hc);
                }
            } else if picking && resp.hovered() && ppress {
                if let Some(pd) = probe {
                    self.eyedrop(&pd);
                }
            } else if editing && self.stroke.is_none() && resp.hovered() && (ppress || spress) {
                if let Some(hc) = hover_cell {
                    let erase = spress || self.tool == Tool::Erase;
                    let live = self.run.as_ref().map(|r| r.m().live).unwrap_or(false);
                    if live && erase {
                        // live missions: no erasing the scene
                    } else if self.shape == Shape::Fill {
                        if !is_gas || erase {
                            want_snapshot = true;
                        }
                        self.fill_request = Some((hc, erase));
                    } else {
                        if !is_gas || erase {
                            want_snapshot = true;
                        }
                        self.stroke = Some(StrokeState { right: spress && !ppress, erase, start: hc, last: hc });
                    }
                }
            }
            if !editing {
                self.stroke = None;
            }
            let cur = ptr.map(|p| {
                let (x, y) = to_cell(p);
                (x.floor() as i32, y.floor() as i32)
            });
            let spray_r = (self.brush * 2 + 3) as f32;
            let spray_pts: Vec<(i32, i32)> = (0..(2 + self.brush).min(16))
                .map(|_| {
                    let a = self.rand() * std::f32::consts::TAU;
                    let d = self.rand().sqrt() * spray_r;
                    ((a.cos() * d).round() as i32, (a.sin() * d).round() as i32)
                })
                .collect();
            let spraying = self.shape == Shape::Spray && !is_gas;
            if let Some(st) = &mut self.stroke {
                let held = if st.right { sdown } else { pdown };
                let freehand = st.erase || is_gas || self.shape == Shape::Brush || self.shape == Shape::Spray;
                let cur = cur.unwrap_or(st.last);
                if freehand {
                    if spraying && !st.erase {
                        for (dx, dy) in spray_pts.iter() {
                            let q = (cur.0 + dx, cur.1 + dy);
                            ops.push(Op { kind: 0, shape: 0, x0: q.0, y0: q.1, x1: q.0, y1: q.1, r: 0, tile: 0, mat: 0, dir: 0, _p: [0; 2] });
                        }
                    } else if is_gas && !st.erase {
                        ops.push(Op { kind: 0, shape: 0, x0: cur.0, y0: cur.1, x1: cur.0, y1: cur.1, r: 0, tile: 0, mat: 0, dir: 0, _p: [0; 2] });
                    } else {
                        ops.push(Op { kind: 0, shape: 3, x0: st.last.0, y0: st.last.1, x1: cur.0, y1: cur.1, r: 0, tile: 0, mat: 0, dir: 0, _p: [0; 2] });
                    }
                }
                st.last = cur;
                if !held && !freehand {
                    let shape = match self.shape {
                        Shape::Line => 3,
                        Shape::Box => 1,
                        _ => 2,
                    };
                    ops.push(Op { kind: 0, shape, x0: st.start.0, y0: st.start.1, x1: st.last.0, y1: st.last.1, r: 0, tile: 0, mat: 0, dir: 0, _p: [0; 2] });
                }
                let erase = st.erase;
                let (start, last) = (st.start, st.last);
                for o in ops.iter_mut() {
                    let mut full = self.tool_op(o.shape, (o.x0, o.y0), (o.x1, o.y1), erase);
                    if is_gas && !erase {
                        full.shape = 0;
                    }
                    if spraying && !erase {
                        full.shape = 0;
                        full.r = 0;
                    }
                    *o = full;
                }
                // composition brush: blend + optional cast op (minor components as single-cell ops, queued in order)
                if !erase {
                    if let (Some(b), Tool::Matter(_)) = (&self.comp, self.tool) {
                        let ex = prop::expand(b, &ops);
                        ops.clear();
                        self.op_queue.extend(ex);
                    }
                }
                if !held {
                    self.stroke = None;
                    stroke_ended = true;
                } else if !freehand {
                    let p = ui.painter_at(rect);
                    let a = to_screen(start.0 as f32 + 0.5, start.1 as f32 + 0.5);
                    let b = to_screen(last.0 as f32 + 0.5, last.1 as f32 + 0.5);
                    match self.shape {
                        Shape::Line => {
                            p.line_segment([a, b], Stroke::new(((self.brush * 2 + 1) as f32 * cell_px).max(1.5), ACCENT.gamma_multiply(0.6)));
                        }
                        _ => {
                            let r = Rect::from_two_pos(a, b).expand(cell_px * 0.5);
                            p.rect_stroke(r, CornerRadius::ZERO, Stroke::new(1.5_f32, ACCENT), StrokeKind::Middle);
                            if self.shape == Shape::Box {
                                p.rect_filled(r, CornerRadius::ZERO, ACCENT.gamma_multiply(0.15));
                            }
                        }
                    }
                    let (w, h) = ((last.0 - start.0).abs() + 1, (last.1 - start.1).abs() + 1);
                    p.text(b + Vec2::new(12.0, 12.0), Align2::LEFT_TOP, format!("{w} × {h} cm"), mono_font(), Color32::WHITE);
                }
            }

            // gpu frame
            let steps = if self.paused {
                if self.step_once {
                    self.step_once = false;
                    (TPS / 60.0 / DT).round() as u32
                } else {
                    0
                }
            } else {
                self.step_acc += self.speed * TPS * dt_real / DT;
                let want = self.step_acc.floor();
                self.step_acc -= want;
                let cap: u32 = std::env::var("SHOT_MAXSTEPS").ok().and_then(|v| v.parse().ok()).unwrap_or_else(|| self.settings.graphics().max_steps_per_frame.min(MAX_STEPS));
                // Frame-budget governor (target 60 fps): when a frame overruns, shrink the step cap in proportion to
                // the overrun; when it is on time and the sim wanted more steps than the cap allowed, grow the cap
                // geometrically. Slow frames never request more work (the shortfall is dropped, not banked).
                let dt_ms = dt_real * 1000.0;
                if self.steps_last > 2 {
                    // A frame can never cost less than its GPU work, so a short frame bounds the step cost from
                    // above; a clearly over-budget frame (GPU-bound) measures it.
                    let sample = ((dt_ms - 2.0) / self.steps_last as f32).clamp(0.05, 3.0);
                    if dt_ms > 21.0 {
                        self.step_cost_ms += (sample - self.step_cost_ms) * 0.05;
                    } else if sample < self.step_cost_ms {
                        self.step_cost_ms += (sample - self.step_cost_ms) * 0.2;
                    }
                }
                if dt_ms > 21.0 {
                    let k = (17.0 / dt_ms).clamp(0.55, 0.95);
                    self.dyn_cap = (self.dyn_cap * k).max(2.0);
                } else if self.last_capped && dt_ms < 18.5 {
                    self.dyn_cap = (self.dyn_cap * 1.08 + 0.5).min(cap as f32);
                }
                let budget = (self.dyn_cap as u32).clamp(1, cap);
                self.last_capped = want as u32 > budget;
                if self.last_capped {
                    // don't bank the shortfall: the sim simply runs slower than real time this frame
                    self.step_acc = 0.0;
                }
                let s = (want as u32).min(budget);
                self.rt_factor = self.rt_factor * 0.95 + 0.05 * if want > 0.0 { s as f32 / want } else { 1.0 };
                s
            };
            steps_out = steps;
            if !self.paused && dt_real > 0.0 {
                let sim_ms = steps as f32 * DT * MS_PER_UNIT;
                self.sim_rate += (sim_ms / dt_real - self.sim_rate) * 0.05;
            } else if self.paused {
                self.sim_rate = 0.0;
            }
            self.sim_time += steps as f32 * DT;
            let sc = self.pending_scene.take();
            if let Some(s) = sc.as_ref() {
                crate::gpu::upload_engine(s.engine.clone());
            }
            let mut all_ops = sc.as_ref().map(|s| s.ops.clone()).unwrap_or_default();
            all_ops.extend(ops.iter().copied());
            // queued fills: as many as fit in this frame's op buffer
            let room = crate::gpu::MAX_OPS.saturating_sub(all_ops.len() + 4);
            for _ in 0..room {
                match self.op_queue.pop_front() {
                    Some(o) => all_ops.push(o),
                    None => break,
                }
            }
            // start sequence: feed pressures ramp up, like a real engine start
            let mut ramp = {
                let t = (self.sim_time / 14.0).clamp(0.0, 1.0);
                t * t * (3.0 - 2.0 * t)
            };
            let mut ign = self.ign_on;
            let mut fan = self.fan_pow;
            if let Some(r) = &self.run {
                match r.phase {
                    Phase::Build => {
                        ramp = 0.0;
                        ign = false;
                        fan = 0.0;
                    }
                    _ => ign = r.t < r.m().igniter_secs,
                }
            }
            if let Some(l) = &self.acoustics {
                l.set_frame(crate::audio::acoustics::Listener {
                    pos: [cam[0] + cam[2] * 0.5, cam[1] + cam[3] * 0.5],
                    cam,
                    speed: self.speed,
                    real_dt: dt_real,
                    sim_time: self.sim_time,
                    paused: self.paused,
                    fan_pow: fan,
                });
            }
            let input = FrameInput {
                steps,
                ops: all_ops,
                view: self.view,
                p_fuel: 1.0 + (self.p_fuel - 1.0) * ramp,
                p_o2: 1.0 + (self.p_o2 - 1.0) * ramp,
                p_air: 1.0 + (self.p_air - 1.0) * ramp,
                fan_pow: fan,
                gravity: if self.gravity_on { crate::gpu::GRAVITY } else { 0.0 },
                cam,
                ign_on: ign,
                time: ctx.input(|i| i.time) as f32,
                frame: self.frame,
                scene: sc.map(|s| s.meta),
                meta_only: self.pending_meta.take(),
                reset_gas: std::mem::take(&mut self.reset_gas),
                probe: Some(self.last_probe_cell),
            };
            self.frame = self.frame.wrapping_add(1);
            let mut input = input;
            // the previous frame's input never reached the GPU (discarded egui pass): carry its one-shot parts over
            if let Some((used, prev)) = self.inflight.take() {
                if !used.load(std::sync::atomic::Ordering::Acquire) {
                    if input.scene.is_none() {
                        input.scene = prev.scene.clone();
                    }
                    if input.meta_only.is_none() && input.scene.is_none() {
                        input.meta_only = prev.meta_only.clone();
                    }
                    input.reset_gas |= prev.reset_gas;
                    let mut ops = prev.ops.clone();
                    ops.extend(input.ops.drain(..));
                    let keep = crate::gpu::MAX_OPS - 4;
                    if ops.len() > keep {
                        for o in ops.drain(keep..).rev() {
                            self.op_queue.push_front(o);
                        }
                    }
                    input.ops = ops;
                }
            }
            let input = Arc::new(input);
            let used = Arc::new(std::sync::atomic::AtomicBool::new(false));
            self.inflight = Some((used.clone(), input.clone()));
            ui.painter().add(egui_wgpu::Callback::new_paint_callback(rect, Cb { input, used }));

            let painter = ui.painter_at(rect);
            // brush cursor
            if let (Some(p), true) = (ptr, resp.hovered() || self.stroke.is_some()) {
                if rect.contains(p) {
                    let (cx, cy) = to_cell(p);
                    let centre = to_screen(cx.floor() + 0.5, cy.floor() + 0.5);
                    if placing {
                        let s = Stroke::new(1.0_f32, ACCENT);
                        painter.line_segment([centre - Vec2::new(10.0, 0.0), centre + Vec2::new(10.0, 0.0)], s);
                        painter.line_segment([centre - Vec2::new(0.0, 10.0), centre + Vec2::new(0.0, 10.0)], s);
                        painter.circle_stroke(centre, 4.0, s);
                        if let Some(a) = self.place_anchor() {
                            let pa = to_screen(a.0 as f32 + 0.5, a.1 as f32 + 0.5);
                            painter.line_segment([pa, centre], Stroke::new(1.0_f32, ACCENT));
                        }
                    } else if picking {
                        let s = Stroke::new(1.0_f32, ACCENT);
                        painter.circle_stroke(centre, 9.0, s);
                        painter.line_segment([centre - Vec2::new(14.0, 0.0), centre - Vec2::new(4.0, 0.0)], s);
                        painter.line_segment([centre + Vec2::new(4.0, 0.0), centre + Vec2::new(14.0, 0.0)], s);
                        painter.line_segment([centre - Vec2::new(0.0, 14.0), centre - Vec2::new(0.0, 4.0)], s);
                        painter.line_segment([centre + Vec2::new(0.0, 4.0), centre + Vec2::new(0.0, 14.0)], s);
                        painter.text(p + Vec2::new(16.0, 14.0), Align2::LEFT_TOP, "pick", small_font(), ACCENT);
                    } else if editing {
                        let r = (if is_gas { self.brush + 1 } else { self.brush }) as f32 + 0.5;
                        let col = if self.tool == Tool::Erase || sdown { BAD } else { Color32::from_white_alpha(190) };
                        match self.shape {
                            Shape::Fill => {
                                painter.circle_filled(centre, 3.0, col);
                                painter.text(p + Vec2::new(14.0, 12.0), Align2::LEFT_TOP, if sdown || self.tool == Tool::Erase { "clear region" } else { "fill region" }, small_font(), col);
                            }
                            Shape::Spray if !is_gas => {
                                let rr = (spray_r + 0.5) * cell_px;
                                for k in 0..24 {
                                    let a = k as f32 / 24.0 * std::f32::consts::TAU;
                                    painter.circle_filled(centre + Vec2::new(a.cos(), a.sin()) * rr, 1.0, col);
                                }
                            }
                            _ => {
                                painter.circle_stroke(centre, r * cell_px, Stroke::new(1.0_f32, col));
                            }
                        }
                        if matches!(self.tool, Tool::Part(t) if t == scene::FAN) {
                            let d = [Vec2::new(1.0, 0.0), Vec2::new(0.0, 1.0), Vec2::new(-1.0, 0.0), Vec2::new(0.0, -1.0)][self.fan_dir as usize];
                            painter.arrow(centre, d * (r * cell_px + 14.0), Stroke::new(2.0_f32, ACCENT));
                        }
                    } else {
                        painter.text(p + Vec2::new(14.0, 14.0), Align2::LEFT_TOP, "geometry locked during run", small_font(), Color32::from_white_alpha(170));
                    }
                }
            }
            self.viewport_overlay(ui, full, header, rect, cam, stats, probe);
            self.tut_rects.insert("canvas".into(), rect);
        });
        if stroke_ended {
            self.readback_in = 2;
        }
        (want_snapshot, steps_out)
    }

    /// Screenshot / test hooks (see ARCHITECTURE.md "UI / workspace").
    fn handle_shot_env(&mut self, ctx: &egui::Context) {
        if self.shot_init || std::env::var("COMBUSTOR_SHOT").is_err() {
            return;
        }
        self.shot_init = true;
        let env = |k: &str| std::env::var(k).ok();
        if let Some(v) = env("SHOT_SPEED") {
            self.speed = v.parse().unwrap_or(1.0);
        }
        if let Some(v) = env("SHOT_DOCKS") {
            let on = v != "0";
            self.dock_left = on;
            self.dock_right = on;
            self.dock_bottom = on;
        }
        if let Some(n) = env("SHOT_MISSION") {
            let ms = missions_list();
            let i = n.parse::<usize>().unwrap_or(1).clamp(1, ms.len()) - 1;
            let solve = env("SHOT_SOLVE").is_some();
            self.start_mission(i, solve && env("SHOT_SOLVE").as_deref() != Some("bare"));
            if solve && !ms[i].live {
                self.readback_in = -120; // begin the test ~2 s later (lets the thrust tare settle)
            }
        } else if let Some(name) = env("SHOT_SCENE").map(|s| s.to_lowercase()).filter(|s| !s.is_empty()) {
            if let Some((t, f)) = ws::templates().into_iter().find(|t| t.key == name || t.title.to_lowercase().starts_with(&name)).map(|t| (t.title, t.build)) {
                self.start_sandbox_named(t, f());
            }
        }
        if let Some(v) = env("SHOT_VIEW") {
            self.view = v.parse().unwrap_or(0);
        }
        if let Some(v) = env("SHOT_TAB") {
            if let Some(t) = ws::BottomTab::from_name(&v) {
                self.bottom_tab = t;
            }
        }
        if let Some(v) = env("SHOT_UNITS") {
            self.units = if v == "eng" { ws::Units::engineering() } else { ws::Units::default() };
        }
        if let Some(v) = env("SHOT_TOOL") {
            if let Some(t) = parse_tool(&v) {
                self.tool = t;
            }
        }
        if env("SHOT_PROBES").is_some() {
            self.demo_probes();
        }
        match env("SHOT_WINDOW").as_deref() {
            Some("settings") => self.show_settings = true,
            Some("library") => self.show_library = true,
            Some("codex") => {
                self.show_codex = true;
                self.codex.open_element(13);
            }
            Some("experiments") => self.show_experiments = true,
            Some("shortcuts") => self.show_shortcuts = true,
            Some("about") => self.show_about = true,
            Some("templates") => self.show_templates = true,
            Some("tour") => self.tutorial.start(0),
            Some("designer") => self.show_designer = true,
            Some("engine") => self.show_engine = true,
            _ => {}
        }
        if let Some(v) = env("SHOT_PRESET") {
            self.designer.load_preset(v.parse().unwrap_or(0));
        }
        if let Some(v) = env("SHOT_COMP").map(|s| s.to_lowercase()) {
            if let Some(k) = prop::COMPOSITIONS.iter().position(|c| c.name.to_lowercase().starts_with(&v)) {
                self.use_composition(k, prop::COMPOSITIONS[k].cast);
            }
        }
        if let Some(v) = env("SHOT_MOTOR") {
            let pr = self.designer.predict();
            let mass: Vec<(u32, f32)> = pr.mass.iter().map(|(i, f, _)| (*i, *f)).collect();
            if let Some(b) = prop::Brush::from_mass("Test motor grain", &mass, true) {
                self.build_test_motor(&b, v.parse().unwrap_or(0), self.designer.throat);
            }
        }
        let _ = ctx;
    }

    fn handle_shot_capture(&mut self, ctx: &egui::Context) {
        let Ok(path) = std::env::var("COMBUSTOR_SHOT") else { return };
        let at: u32 = std::env::var("SHOT_FRAMES").ok().and_then(|v| v.parse().ok()).unwrap_or(240);
        // SHOT_SOLVE runs: shoot ~1 s after the mission result is in (SHOT_FRAMES is then only a cap)
        let done = std::env::var("SHOT_SOLVE").is_ok() && self.run.as_ref().map(|r| r.phase == Phase::Done && (ctx.input(|i| i.time) - r.done_at) > 1.0).unwrap_or(false);
        if self.frame == at || (done && !self.shot_taken) {
            self.shot_taken = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(Default::default()));
        }
        let shot = ctx.input(|i| i.events.iter().find_map(|e| if let egui::Event::Screenshot { image, .. } = e { Some(image.clone()) } else { None }));
        if let Some(img) = shot {
            let mut bytes = format!("P6\n{} {}\n255\n", img.size[0], img.size[1]).into_bytes();
            for p in &img.pixels {
                bytes.extend_from_slice(&[p.r(), p.g(), p.b()]);
            }
            let _ = std::fs::write(&path, bytes);
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }
}


impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        if self.frame == 0 {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title("Ignis".into()));
        }
        self.handle_shot_env(ctx);
        self.handle_shot_capture(ctx);
        if (self.settings.ui_scale - self.applied_scale).abs() > 1e-3 {
            self.applied_scale = self.settings.ui_scale;
            ctx.set_zoom_factor(self.settings.ui_scale);
        }
        self.shortcuts(ctx, frame);
        let (stats, probe): (Stats, Option<ProbeData>) =
            frame.wgpu_render_state().and_then(|rs| rs.renderer.read().callback_resources.get::<GpuSim>().map(|s| (s.stats, s.probe))).unwrap_or_default();
        let dt_real = ctx.input(|i| i.stable_dt).min(0.05);
        self.frame_ms += (dt_real * 1000.0 - self.frame_ms) * 0.05;
        let now = ctx.input(|i| i.time);
        self.tut_rects.clear();

        // docked workspace (order matters: outer panels first)
        self.menu_bar(ctx, frame);
        self.toolbar(ctx, frame);
        self.status_bar(ctx, &stats);
        if self.dock_left {
            self.library_dock(ctx);
        }
        if self.dock_right {
            self.inspector_dock(ctx, frame, &stats, probe);
        }
        self.designer_window(ctx);
        if self.dock_bottom {
            self.bottom_dock(ctx, frame, &stats);
        }
        let (want_snapshot, steps) = self.canvas(ctx, &stats, probe, dt_real);
        self.steps_last = steps;
        if want_snapshot {
            let live_id = match (self.run.as_ref().map(|r| r.m().live), self.tool) {
                (Some(true), Tool::Matter(k)) | (Some(true), Tool::Wall(k)) => Some(k),
                _ => None,
            };
            if let (Some(id), Some(m)) = (live_id, self.current_meta(frame)) {
                if let Some(r) = self.run.as_mut() {
                    r.stroke_base = Some((id, count_id(&m, id)));
                }
            }
            self.snapshot(frame);
        }
        if let Some((at, erase)) = self.fill_request.take() {
            self.flood_fill(frame, at, erase);
            if self.run.is_some() {
                self.readback_in = 6;
            }
        }

        // experiments: bookkeeping and objectives
        if self.run.is_some() {
            // dev: SHOT_SOLVE starts the test automatically
            if self.readback_in < -1 {
                self.readback_in += 1;
                if self.readback_in == -1 {
                    self.begin_test(frame);
                }
            }
            self.readback_timer += dt_real;
            let testing = self.run.as_ref().map(|r| r.phase == Phase::Test).unwrap_or(false);
            let period = if testing { 0.25 } else { 0.6 };
            if self.readback_in > 0 {
                self.readback_in -= 1;
                if self.readback_in == 0 {
                    self.readback_in = -1;
                    self.readback_timer = period;
                }
            }
            if self.readback_timer >= period {
                self.readback_timer = 0.0;
                self.mission_readback(frame);
            }
            self.tick_mission(&stats, steps as f32 * DT / TPS, now);
            self.apply_live_solution();
        }

        // probes, line samplers and conservation diagnostics (blocking readback, a few times a second)
        self.sample_probes(frame, dt_real, steps);
        self.prop_tick(frame, probe, now);

        // floating windows
        self.windows(ctx, frame);

        // global time series for the Plots dock
        self.hist_timer += dt_real;
        if self.hist_timer > 0.1 && steps > 0 {
            self.hist_timer = 0.0;
            let th = self.shown_thrust(&stats).length() * KN_PER_UNIT;
            let t = self.sim_time * MS_PER_UNIT;
            let h = &mut self.hist;
            let push = |q: &mut VecDeque<f32>, v: f32| {
                q.push_back(v);
                if q.len() > 600 {
                    q.pop_front();
                }
            };
            push(&mut h.t, t);
            push(&mut h.thrust, th);
            push(&mut h.p, stats.max_p);
            push(&mut h.temp, stats.max_t * 300.0);
            push(&mut h.burn, stats.burn * KGS_PER_UNIT);
            push(&mut h.wall_heat, stats.max_wall_heat * 100.0);
            push(&mut h.stress, stats.max_stress * 100.0);
        }

        // audio + tutorial feeds
        self.feed_audio(&stats, dt_real);
        self.feed_tutorial(ctx, &stats, dt_real);

        if self.paused && self.stroke.is_none() {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        } else {
            ctx.request_repaint();
        }
    }
}

/// Gas state from the conserved variables (per-species heat capacities, matches sim.wgsl).
struct Gas {
    rho: f32,
    p: f32,
    t: f32,
    speed: f32,
    mach: f32,
}

fn gas(u: &[f32; 8]) -> Gas {
    let rho = (u[0] + u[1] + u[2] + u[3]).max(1e-3);
    let (vx, vy) = (u[4] / rho, u[5] / rho);
    let cv = (u[0] * 5.827 + u[1] * 2.258 + u[2] * 2.580 + u[3] * 4.446).max(1e-4);
    let r = u[0] * 1.80625 + u[1] * 0.903125 + u[2] * 1.032142857 + u[3] * 1.111538;
    let ke = 0.5 * rho * (vx * vx + vy * vy);
    let e = (u[6] - ke).max(0.0);
    let p = r / cv * e;
    let t = e / cv;
    let c = ((1.0 + r / cv) * p / rho).sqrt().max(1e-3);
    let speed = (vx * vx + vy * vy).sqrt();
    Gas { rho, p, t, speed, mach: speed / c }
}

