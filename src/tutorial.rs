//! Ignis interactive tutorial: a self-contained, data-driven onboarding system.
//!
//! The app owns *nothing* here except feeding events in and drawing the overlay:
//!
//! ```ignore
//! // once:            let mut tut = tutorial::Tutorial::new();  tut.start(0);
//! // when things happen:  tut.on_event(TutorialEvent::ElementSelected("FUEL".into()));
//! // every frame:     tut.on_event(TutorialEvent::Tick(dt));
//! //                  tut.on_event(TutorialEvent::Stat(Stat::Thrust, thrust_kn)); ...
//! //                  if let Some(a) = tut.draw_overlay(ctx, &rects) { tut.on_event(a.into()); }
//! ```
//!
//! Highlight target ids are plain strings that the app maps to screen rects (record the
//! `Response::rect` of the widget while laying it out). Ids used by the built-in lessons:
//!
//! | id | widget |
//! |---|---|
//! | `canvas` | the simulation view |
//! | `palette` | element browser / palette |
//! | `palette:<CODE>` | one element button, e.g. `palette:FUEL`, `palette:TRCH`, `palette:KNO3`, `palette:CHAR` (falls back to `palette`) |
//! | `views` / `view:<name>` | view selector / a single view button (`view:fire`, `view:pressure`, `view:heat`, `view:flow`, `view:gases`, `view:stress`) (falls back to `views`) |
//! | `play` | play/pause button |
//! | `speed` | sim speed control |
//! | `scenes` | scenario menu |
//! | `slider:fuel` | "Fuel feed" slider (falls back to `feeds`) |
//! | `readout:thrust`, `readout:stress` | stat readouts (fall back to `readouts`) |
//! | `inspector` | cell inspector panel |
//!
//! Any id with a `:` falls back to the part before it, so the app can start by providing only
//! coarse rects. Missing rects are fine: the card then sits bottom-centre with no arrow.

use std::collections::HashMap;

use eframe::egui::{self, Color32, CornerRadius, Id, LayerId, Order, Pos2, Rect, Stroke, StrokeKind, Vec2};

// ---------------------------------------------------------------------------------------------
// Data model
// ---------------------------------------------------------------------------------------------

/// Simulation / UI quantities the app reports each frame via [`TutorialEvent::Stat`].
/// Units are whatever the app displays (documented per variant).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Stat {
    /// Engine thrust, kN.
    Thrust,
    /// Peak pressure, atm.
    MaxPressure,
    /// Peak gas temperature, K.
    MaxTemp,
    /// Burn rate (gpu `Stats::burn`, any monotone unit; > ~0.01 means "something is burning").
    Burn,
    /// Peak wall stress ratio (0..1+, 1 = yield).
    WallStress,
    /// Peak wall temperature / melting temperature (0..1+).
    WallHeat,
    /// The "Fuel feed" slider value, atm.
    FuelFeed,
    /// Simulation speed multiplier (1 = real time).
    Speed,
}

/// A completion condition for one step. Plain data: the tutorial evaluates it against the
/// events the app sends, the app never needs to evaluate it itself.
#[derive(Clone, Debug, PartialEq)]
pub enum Condition {
    /// The user picked the element / tool with this code (elements.rs `code`, e.g. "FUEL", "TRCH").
    ElementSelected(String),
    /// At least n cells painted (any element) since the step started.
    PaintedCells(u32),
    /// At least n cells painted with this element code since the step started.
    PaintedWith(String, u32),
    /// Something ignited (torch/igniter touched fuel, or the app saw burn start).
    Ignited,
    /// The user switched to view n (0 Fire, 1 Pressure, 2 Heat, 3 Flow, 4 Gases, 5 Stress).
    ViewChanged(u32),
    /// The simulation is paused.
    Paused,
    /// The simulation is running.
    Running,
    /// A scenario whose name starts with this (case-insensitive) was loaded.
    SceneLoaded(String),
    /// The reported stat went above the value.
    StatAbove(Stat, f32),
    /// The reported stat went below the value.
    StatBelow(Stat, f32),
    /// Wait this many seconds (of `Tick` time) after the step started: "watch it burn".
    Wait(f32),
    /// The user pressed "Next" on the card.
    Acknowledge,
    /// Every sub-condition has been satisfied at some point during the step (order free).
    All(Vec<Condition>),
    /// Any sub-condition satisfied.
    Any(Vec<Condition>),
}

/// One tutorial step.
#[derive(Clone, Debug)]
pub struct Step {
    pub title: &'static str,
    pub body: &'static str,
    /// Short "do this" line shown in accent colour.
    pub hint: &'static str,
    /// Highlight target id (see module docs), or "" for none.
    pub target: &'static str,
    pub done_when: Condition,
}

/// A lesson = a named sequence of steps.
#[derive(Clone, Debug)]
pub struct Lesson {
    pub name: &'static str,
    pub blurb: &'static str,
    /// Scenario the app may load when the lesson starts (prefix of an app SCENARIOS name), if any.
    pub scene: Option<&'static str>,
    pub steps: Vec<Step>,
}

/// Events the app feeds in. Cheap to send redundantly (e.g. Stat every frame).
#[derive(Clone, Debug, PartialEq)]
pub enum TutorialEvent {
    ElementSelected(String),
    /// `cells` newly painted with element `code` this frame/stroke.
    Painted { code: String, cells: u32 },
    Ignited,
    ViewChanged(u32),
    /// Current pause state (true = paused).
    PauseState(bool),
    SceneLoaded(String),
    Stat(Stat, f32),
    /// Real seconds elapsed (send once per frame).
    Tick(f32),
    /// "Next" pressed on the card (or Enter).
    Acknowledge,
    /// Go back one step.
    Back,
    /// Skip the current step.
    SkipStep,
    /// Close the tutorial (lesson progress kept so it can be resumed).
    Close,
}

/// What the user clicked on the overlay. Feed it back with `tut.on_event(action.into())`,
/// except `LoadScene`, which asks the app to load a scenario (then send `SceneLoaded`).
#[derive(Clone, Debug, PartialEq)]
pub enum OverlayAction {
    Next,
    Back,
    SkipStep,
    Close,
    /// Start lesson n (from the "lesson complete" card).
    StartLesson(usize),
}

impl From<OverlayAction> for TutorialEvent {
    fn from(a: OverlayAction) -> Self {
        match a {
            OverlayAction::Next => TutorialEvent::Acknowledge,
            OverlayAction::Back => TutorialEvent::Back,
            OverlayAction::SkipStep => TutorialEvent::SkipStep,
            // StartLesson is handled by the app via Tutorial::start; Close is the safe default.
            OverlayAction::Close | OverlayAction::StartLesson(_) => TutorialEvent::Close,
        }
    }
}

/// Progress through the active lesson.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Progress {
    pub lesson: usize,
    /// 0-based index of the current step (== steps when the lesson is complete).
    pub step: usize,
    pub steps: usize,
    /// 0..1 over the whole lesson, including partial progress inside the current step.
    pub fraction: f32,
}

// ---------------------------------------------------------------------------------------------
// Tutorial state machine
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Debug, Default)]
struct StepState {
    elapsed: f32,
    painted_total: u32,
    painted: HashMap<String, u32>,
    /// Latched satisfaction per leaf condition (index in DFS order) for All/Any.
    latched: Vec<bool>,
    acknowledged: bool,
}

pub struct Tutorial {
    lessons: Vec<Lesson>,
    lesson: usize,
    step: usize,
    active: bool,
    st: StepState,
    // live context that persists across steps
    selected: Option<String>,
    view: u32,
    paused: bool,
    scene: Option<String>,
    stats: HashMap<Stat, f32>,
    ignited_recent: bool,
    /// Seconds left on the "step complete" flash.
    flash: f32,
    completed: Vec<bool>,
}

impl Default for Tutorial {
    fn default() -> Self {
        Self::new()
    }
}

impl Tutorial {
    pub fn new() -> Self {
        Self::with_lessons(builtin_lessons())
    }

    pub fn with_lessons(lessons: Vec<Lesson>) -> Self {
        let n = lessons.len();
        Self {
            lessons,
            lesson: 0,
            step: 0,
            active: false,
            st: StepState::default(),
            selected: None,
            view: 0,
            paused: false,
            scene: None,
            stats: HashMap::new(),
            ignited_recent: false,
            flash: 0.0,
            completed: vec![false; n],
        }
    }

    pub fn lessons(&self) -> &[Lesson] {
        &self.lessons
    }

    /// Start (or restart) lesson `i`. If the lesson has a `scene`, the app should load it
    /// (check `lessons()[i].scene`) and then send `SceneLoaded`.
    pub fn start(&mut self, i: usize) {
        if i >= self.lessons.len() {
            return;
        }
        self.lesson = i;
        self.step = 0;
        self.active = true;
        self.flash = 0.0;
        self.reset_step();
    }

    /// Is the overlay showing (a lesson is running or just finished)?
    pub fn is_active(&self) -> bool {
        self.active
    }

    pub fn lesson_index(&self) -> usize {
        self.lesson
    }

    pub fn is_lesson_complete(&self, i: usize) -> bool {
        self.completed.get(i).copied().unwrap_or(false)
    }

    /// Current step of the active lesson, `None` if inactive or the lesson is finished.
    pub fn current(&self) -> Option<&Step> {
        if !self.active {
            return None;
        }
        self.lessons.get(self.lesson)?.steps.get(self.step)
    }

    /// The highlight id the current step wants (for apps that want to e.g. open a drawer).
    pub fn wanted_target(&self) -> Option<&'static str> {
        self.current().map(|s| s.target).filter(|t| !t.is_empty())
    }

    pub fn progress(&self) -> Progress {
        let steps = self.lessons.get(self.lesson).map(|l| l.steps.len()).unwrap_or(0);
        let partial = self.current().map(|s| self.partial(&s.done_when)).unwrap_or(0.0);
        let fraction = if steps == 0 { 1.0 } else { ((self.step as f32 + partial) / steps as f32).min(1.0) };
        Progress { lesson: self.lesson, step: self.step.min(steps), steps, fraction }
    }

    /// Compact save string ("1,0,1") of completed lessons, for the app's settings file.
    pub fn save_string(&self) -> String {
        self.completed.iter().map(|c| if *c { "1" } else { "0" }).collect::<Vec<_>>().join(",")
    }

    pub fn load_string(&mut self, s: &str) {
        for (i, v) in s.split(',').enumerate() {
            if let Some(c) = self.completed.get_mut(i) {
                *c = v.trim() == "1";
            }
        }
    }

    pub fn on_event(&mut self, ev: TutorialEvent) {
        // Update persistent context first (even while inactive, so starting mid-session works).
        match &ev {
            TutorialEvent::ElementSelected(c) => self.selected = Some(c.clone()),
            TutorialEvent::ViewChanged(v) => self.view = *v,
            TutorialEvent::PauseState(p) => self.paused = *p,
            TutorialEvent::SceneLoaded(s) => self.scene = Some(s.clone()),
            TutorialEvent::Stat(k, v) => {
                self.stats.insert(*k, *v);
            }
            TutorialEvent::Tick(dt) => self.flash = (self.flash - dt).max(0.0),
            _ => {}
        }
        if !self.active {
            return;
        }
        let steps = self.lessons[self.lesson].steps.len();
        if self.step >= steps {
            // Lesson-complete card: Next/Close dismiss it.
            if matches!(ev, TutorialEvent::Acknowledge | TutorialEvent::Close | TutorialEvent::SkipStep) {
                self.active = false;
            }
            return;
        }
        match &ev {
            TutorialEvent::Close => {
                self.active = false;
                return;
            }
            TutorialEvent::Back => {
                if self.step > 0 {
                    self.step -= 1;
                    self.reset_step();
                }
                return;
            }
            TutorialEvent::SkipStep => {
                self.advance();
                return;
            }
            TutorialEvent::Acknowledge => self.st.acknowledged = true,
            TutorialEvent::Painted { code, cells } => {
                self.st.painted_total += cells;
                *self.st.painted.entry(code.clone()).or_insert(0) += cells;
            }
            TutorialEvent::Ignited => self.ignited_recent = true,
            TutorialEvent::Tick(dt) => self.st.elapsed += dt,
            _ => {}
        }
        let cond = self.lessons[self.lesson].steps[self.step].done_when.clone();
        let mut idx = 0;
        let mut latched = std::mem::take(&mut self.st.latched);
        let done = self.eval(&cond, &mut idx, &mut latched);
        self.st.latched = latched;
        self.ignited_recent = false;
        if done {
            self.advance();
        }
    }

    fn reset_step(&mut self) {
        self.st = StepState::default();
        self.ignited_recent = false;
        // A step whose condition is already true (e.g. "select fuel" while fuel is selected)
        // would complete instantly on the next event; that is intended and feels responsive,
        // except for steps that need fresh action, which use PaintedCells/Wait/Acknowledge.
    }

    fn advance(&mut self) {
        self.step += 1;
        self.flash = 1.2;
        self.reset_step();
        if self.step >= self.lessons[self.lesson].steps.len() {
            if let Some(c) = self.completed.get_mut(self.lesson) {
                *c = true;
            }
        }
    }

    fn stat(&self, k: Stat) -> Option<f32> {
        self.stats.get(&k).copied()
    }

    /// Evaluate a condition. Leaves are latched (once true, stay true) for All/Any.
    fn eval(&self, c: &Condition, idx: &mut usize, latched: &mut Vec<bool>) -> bool {
        match c {
            Condition::All(v) => {
                let mut ok = true;
                for s in v {
                    ok &= self.eval(s, idx, latched);
                }
                ok
            }
            Condition::Any(v) => {
                let mut ok = false;
                for s in v {
                    ok |= self.eval(s, idx, latched);
                }
                ok
            }
            leaf => {
                let i = *idx;
                *idx += 1;
                if latched.len() <= i {
                    latched.resize(i + 1, false);
                }
                if !latched[i] && self.leaf(leaf) {
                    latched[i] = true;
                }
                latched[i]
            }
        }
    }

    fn leaf(&self, c: &Condition) -> bool {
        match c {
            Condition::ElementSelected(code) => self.selected.as_deref().is_some_and(|s| s.eq_ignore_ascii_case(code)),
            Condition::PaintedCells(n) => self.st.painted_total >= *n,
            Condition::PaintedWith(code, n) => {
                self.st.painted.iter().filter(|(k, _)| k.eq_ignore_ascii_case(code)).map(|(_, v)| *v).sum::<u32>() >= *n
            }
            Condition::Ignited => self.ignited_recent,
            Condition::ViewChanged(v) => self.view == *v,
            Condition::Paused => self.paused,
            Condition::Running => !self.paused,
            Condition::SceneLoaded(name) => {
                self.scene.as_deref().is_some_and(|s| s.to_ascii_lowercase().starts_with(&name.to_ascii_lowercase()))
            }
            Condition::StatAbove(k, v) => self.stat(*k).is_some_and(|x| x > *v),
            Condition::StatBelow(k, v) => self.stat(*k).is_some_and(|x| x < *v),
            Condition::Wait(s) => self.st.elapsed >= *s,
            Condition::Acknowledge => self.st.acknowledged,
            Condition::All(_) | Condition::Any(_) => false,
        }
    }

    /// Partial progress (0..1) inside a step, for the progress bar.
    fn partial(&self, c: &Condition) -> f32 {
        match c {
            Condition::PaintedCells(n) => (self.st.painted_total as f32 / (*n).max(1) as f32).min(1.0),
            Condition::PaintedWith(code, n) => {
                let got: u32 = self.st.painted.iter().filter(|(k, _)| k.eq_ignore_ascii_case(code)).map(|(_, v)| *v).sum();
                (got as f32 / (*n).max(1) as f32).min(1.0)
            }
            Condition::Wait(s) => (self.st.elapsed / s.max(0.01)).min(1.0),
            Condition::StatAbove(k, v) => self.stat(*k).map(|x| (x / v.max(1e-6)).clamp(0.0, 1.0)).unwrap_or(0.0),
            Condition::All(v) if !v.is_empty() => {
                let done = self.st.latched.iter().filter(|b| **b).count();
                let leaves = count_leaves(c).max(1);
                done as f32 / leaves as f32
            }
            _ => 0.0,
        }
    }

    /// Does the current step need the "Next" button?
    fn wants_next(&self) -> bool {
        self.current().is_some_and(|s| contains_ack(&s.done_when))
    }

    /// Checklist lines for an `All` step: (label, done).
    fn checklist(&self) -> Vec<(String, bool)> {
        let Some(s) = self.current() else { return vec![] };
        let Condition::All(v) = &s.done_when else { return vec![] };
        let mut out = vec![];
        let mut i = 0;
        for c in v {
            let n = count_leaves(c);
            let done = (i..i + n).all(|j| self.st.latched.get(j).copied().unwrap_or(false));
            i += n;
            out.push((describe(c), done));
        }
        out
    }
}

fn count_leaves(c: &Condition) -> usize {
    match c {
        Condition::All(v) | Condition::Any(v) => v.iter().map(count_leaves).sum(),
        _ => 1,
    }
}

fn contains_ack(c: &Condition) -> bool {
    match c {
        Condition::Acknowledge => true,
        Condition::All(v) | Condition::Any(v) => v.iter().any(contains_ack),
        _ => false,
    }
}

const VIEW_NAMES: [&str; 6] = ["Render", "Pressure", "Temperature", "Velocity", "Species", "Stress"];

fn element_label(code: &str) -> &str {
    match code.to_ascii_uppercase().as_str() {
        "FUEL" => "Fuel gas",
        "TRCH" => "Torch",
        "KNO3" => "Saltpeter",
        "CHAR" => "Charcoal",
        "SULF" => "Sulfur",
        _ => code,
    }
}

/// Human-readable description of a condition (used for checklists).
pub fn describe(c: &Condition) -> String {
    match c {
        Condition::ElementSelected(e) => format!("Select {}", element_label(e)),
        Condition::PaintedCells(n) => format!("Paint {n} cells"),
        Condition::PaintedWith(e, n) => format!("Paint {n} cells of {}", element_label(e)),
        Condition::Ignited => "Ignite it".into(),
        Condition::ViewChanged(v) => format!("Switch to the {} view", VIEW_NAMES.get(*v as usize).unwrap_or(&"?")),
        Condition::Paused => "Pause the simulation".into(),
        Condition::Running => "Resume the simulation".into(),
        Condition::SceneLoaded(s) => format!("Load the {s} scenario"),
        Condition::StatAbove(k, v) => format!("{} above {}", stat_label(*k), fmt_stat(*k, *v)),
        Condition::StatBelow(k, v) => format!("{} below {}", stat_label(*k), fmt_stat(*k, *v)),
        Condition::Wait(s) => format!("Watch for {s:.0} s"),
        Condition::Acknowledge => "Press Next".into(),
        Condition::All(v) => v.iter().map(describe).collect::<Vec<_>>().join(" and "),
        Condition::Any(v) => v.iter().map(describe).collect::<Vec<_>>().join(" or "),
    }
}

fn stat_label(k: Stat) -> &'static str {
    match k {
        Stat::Thrust => "Thrust",
        Stat::MaxPressure => "Peak pressure",
        Stat::MaxTemp => "Peak temperature",
        Stat::Burn => "Burn rate",
        Stat::WallStress => "Wall stress",
        Stat::WallHeat => "Wall heat",
        Stat::FuelFeed => "Fuel feed",
        Stat::Speed => "Speed",
    }
}

fn fmt_stat(k: Stat, v: f32) -> String {
    match k {
        Stat::Thrust => format!("{v:.1} kN"),
        Stat::MaxPressure | Stat::FuelFeed => format!("{v:.0} atm"),
        Stat::MaxTemp => format!("{v:.0} K"),
        Stat::WallStress | Stat::WallHeat => format!("{:.0}%", v * 100.0),
        Stat::Speed => format!("{v:.1}x"),
        Stat::Burn => format!("{v:.2}"),
    }
}

// ---------------------------------------------------------------------------------------------
// Built-in lessons
// ---------------------------------------------------------------------------------------------

fn step(title: &'static str, body: &'static str, hint: &'static str, target: &'static str, done_when: Condition) -> Step {
    Step { title, body, hint, target, done_when }
}

fn s(x: &str) -> String {
    x.to_string()
}

pub fn builtin_lessons() -> Vec<Lesson> {
    use Condition::*;
    vec![
        Lesson {
            name: "First fire",
            blurb: "Paint a cloud of fuel gas, light it and watch real combustion happen.",
            scene: Some("Blank canvas"),
            steps: vec![
                step(
                    "Welcome to Ignis",
                    "Every cell of this canvas is about a centimetre of real air at 1 atm and 300 K. \
                     Gas flows, mixes, heats up and burns by the same physics as a real flame.",
                    "Press Next to begin.",
                    "canvas",
                    Acknowledge,
                ),
                step(
                    "Pick fuel gas",
                    "Fuel gas behaves like methane, the main part of natural gas. On its own it will not burn: \
                     it needs oxygen from the air and a source of heat.",
                    "Choose Fuel gas in the element palette.",
                    "palette:FUEL",
                    ElementSelected(s("FUEL")),
                ),
                step(
                    "Paint a cloud",
                    "Drag on the canvas to release fuel. It drifts and mixes with the surrounding air. \
                     Methane burns only between about 5% and 15% in air, so the edges of the cloud will catch first.",
                    "Paint fuel onto the canvas.",
                    "canvas",
                    PaintedWith(s("FUEL"), 120),
                ),
                step(
                    "Grab the torch",
                    "The torch heats gas to about 2700 K, far above methane's autoignition temperature of ~810 K.",
                    "Select the Torch tool.",
                    "palette:TRCH",
                    ElementSelected(s("TRCH")),
                ),
                step(
                    "Light it",
                    "Touch the torch to the fuel cloud. A flame front will race through the mixed region.",
                    "Click the torch on the edge of the cloud.",
                    "canvas",
                    Any(vec![Ignited, StatAbove(Stat::Burn, 0.02)]),
                ),
                step(
                    "Watch it burn",
                    "Hot products expand and rise: buoyancy pulls in fresh air at the base, and soot glows orange \
                     where there is not enough oxygen to burn cleanly.",
                    "Just watch for a few seconds.",
                    "canvas",
                    Wait(5.0),
                ),
                step(
                    "See the heat",
                    "The Render view shows what your eyes would see. The Temperature view (key 3) shows temperature: the hottest \
                     part of a methane flame in air is about 2200 K.",
                    "Switch to the Temperature view.",
                    "view:heat",
                    ViewChanged(2),
                ),
                step(
                    "That's fire",
                    "Fuel, oxygen and heat: the fire triangle. Take any one away and the flame dies. \
                     Try Chill or Vacuum on a flame, or paint more fuel, and see what happens.",
                    "Press Next to finish the lesson.",
                    "",
                    Acknowledge,
                ),
            ],
        },
        Lesson {
            name: "Mixing powders",
            blurb: "Make black powder from saltpeter and charcoal, then light it.",
            scene: Some("Blank canvas"),
            steps: vec![
                step(
                    "Oxidiser and fuel",
                    "Black powder carries its own oxygen. Saltpeter (potassium nitrate) releases O2 when heated; \
                     charcoal is almost pure carbon fuel. Together they burn without any air at all.",
                    "Press Next.",
                    "",
                    Acknowledge,
                ),
                step(
                    "Saltpeter first",
                    "Real black powder is 75% saltpeter, 15% charcoal and 10% sulfur by mass.",
                    "Select Saltpeter (KNO3) in the palette.",
                    "palette:KNO3",
                    ElementSelected(s("KNO3")),
                ),
                step(
                    "Pour a pile",
                    "Powders fall and pile up like sand. Make a small heap on the floor.",
                    "Paint saltpeter onto the canvas.",
                    "canvas",
                    PaintedWith(s("KNO3"), 80),
                ),
                step(
                    "Add charcoal",
                    "Now add the fuel right on top so the two powders touch. Reactions only happen where grains meet.",
                    "Select Charcoal and paint it over the saltpeter.",
                    "palette:CHAR",
                    All(vec![ElementSelected(s("CHAR")), PaintedWith(s("CHAR"), 30)]),
                ),
                step(
                    "Light the mix",
                    "Saltpeter starts giving off oxygen above ~670 K. Once a grain pair reacts, its heat lights its \
                     neighbours, and the burn spreads through the pile in a flash.",
                    "Select the Torch and touch it to the pile.",
                    "palette:TRCH",
                    All(vec![ElementSelected(s("TRCH")), Any(vec![Ignited, StatAbove(Stat::Burn, 0.02)])]),
                ),
                step(
                    "Slow motion",
                    "Pause and look: the hot gas, the smoke and the leftover ash. Confined in a closed box, the same \
                     gas would build pressure until the box burst. That is how a firework shell works.",
                    "Pause the simulation (Space).",
                    "play",
                    Paused,
                ),
                step(
                    "Recipes to try",
                    "Saltpeter + sugar = rocket candy. Rust + aluminium powder = thermite. \
                     Ammonium perchlorate + aluminium = Space Shuttle booster propellant. The help card lists more.",
                    "Press Next to finish.",
                    "",
                    Acknowledge,
                ),
            ],
        },
        Lesson {
            name: "Engines",
            blurb: "Run a turbojet: feed it fuel, read the thrust, and watch the walls.",
            scene: Some("Turbojet"),
            steps: vec![
                step(
                    "Load the turbojet",
                    "A fan compresses air, injectors spray fuel, a flame holder keeps the flame lit and a nozzle \
                     turns hot, high-pressure gas into a fast jet.",
                    "Open the scenario menu and load Turbojet.",
                    "scenes",
                    SceneLoaded(s("Turbojet")),
                ),
                step(
                    "Let it spool up",
                    "Fuel takes a moment to fill the pipes and the igniter needs time to light the combustor. \
                     Make sure the simulation is running.",
                    "Watch the engine light.",
                    "canvas",
                    All(vec![Running, Wait(4.0)]),
                ),
                step(
                    "More fuel",
                    "The Fuel feed slider sets the pressure behind every fuel inlet. More fuel means more heat \
                     released in the combustor and a faster exhaust, up to the point where there is not enough air left to burn it.",
                    "Raise Fuel feed above 12 atm.",
                    "slider:fuel",
                    StatAbove(Stat::FuelFeed, 12.0),
                ),
                step(
                    "Read the thrust",
                    "Thrust is the momentum of the gas leaving the nozzle each second. Watch the readout climb as \
                     the hotter exhaust accelerates.",
                    "Wait for thrust above 0.5 kN.",
                    "readout:thrust",
                    StatAbove(Stat::Thrust, 0.5),
                ),
                step(
                    "Stress view",
                    "Pressure pushes on the walls and heat weakens them. The Stress view colours each wall cell by \
                     how close it is to yielding.",
                    "Switch to the Stress view.",
                    "view:stress",
                    ViewChanged(5),
                ),
                step(
                    "Watch the walls",
                    "Red cells are close to failure. Hot steel loses strength fast above ~800 K, which is why real \
                     combustors use nickel superalloys like Inconel and film cooling.",
                    "Watch wall stress (or press Next).",
                    "readout:stress",
                    Any(vec![StatAbove(Stat::WallStress, 0.3), Wait(8.0), Acknowledge]),
                ),
                step(
                    "Your engine now",
                    "Push the fuel feed until something breaks, rebuild the combustor in Inconel, or add an oxygen \
                     inlet for an afterburner. Every change is real physics.",
                    "Press Next to finish.",
                    "",
                    Acknowledge,
                ),
            ],
        },
    ]
}

// ---------------------------------------------------------------------------------------------
// Overlay drawing
// ---------------------------------------------------------------------------------------------

const ACCENT: Color32 = Color32::from_rgb(255, 140, 40);
const CARD_BG: Color32 = Color32::from_rgb(22, 22, 28);
const CARD_EDGE: Color32 = Color32::from_rgb(70, 62, 56);
const TEXT: Color32 = Color32::from_rgb(232, 228, 222);
const MUTED: Color32 = Color32::from_rgb(150, 146, 140);
const GOOD: Color32 = Color32::from_rgb(110, 210, 120);
const CARD_W: f32 = 330.0;

fn lookup<'a>(rects: &'a HashMap<String, Rect>, id: &str) -> Option<&'a Rect> {
    if id.is_empty() {
        return None;
    }
    rects.get(id).or_else(|| id.split_once(':').and_then(|(base, _)| rects.get(base)))
}

impl Tutorial {
    /// Draw the callout card, target highlight and arrow. Returns the button the user clicked,
    /// which the app feeds back with `on_event(action.into())` (or `start(n)` for StartLesson).
    /// Does nothing when inactive. Requests repaints for its own animation.
    pub fn draw_overlay(&self, ctx: &egui::Context, highlight_rects: &HashMap<String, Rect>) -> Option<OverlayAction> {
        if !self.active {
            return None;
        }
        let lesson = self.lessons.get(self.lesson)?;
        let screen = ctx.screen_rect();
        let t = ctx.input(|i| i.time) as f32;
        ctx.request_repaint_after(std::time::Duration::from_millis(33));
        let prog = self.progress();
        let step = self.current();
        let target = step.and_then(|s| lookup(highlight_rects, s.target)).copied();

        // ---- highlight ring around the target
        let painter = ctx.layer_painter(LayerId::new(Order::Foreground, Id::new("ignis_tut_hl")));
        let big_target = target.is_some_and(|r| r.area() > screen.area() * 0.3);
        if let Some(r) = target {
            let pulse = 0.5 + 0.5 * (t * 3.2).sin();
            let pad = 3.0 + 3.0 * pulse;
            let a = (110.0 + 120.0 * pulse) as u8;
            let col = Color32::from_rgba_unmultiplied(ACCENT.r(), ACCENT.g(), ACCENT.b(), a);
            if big_target {
                painter.rect_stroke(r.shrink(2.0), CornerRadius::same(6), Stroke::new(2.0_f32, col), StrokeKind::Inside);
            } else {
                // soft glow then crisp ring
                for k in 0..3 {
                    let g = Color32::from_rgba_unmultiplied(ACCENT.r(), ACCENT.g(), ACCENT.b(), (a as f32 * 0.18 / (k + 1) as f32) as u8);
                    painter.rect_stroke(r.expand(pad + 3.0 * (k + 1) as f32), CornerRadius::same(8), Stroke::new(3.0_f32, g), StrokeKind::Outside);
                }
                painter.rect_stroke(r.expand(pad), CornerRadius::same(6), Stroke::new(2.0_f32, col), StrokeKind::Outside);
            }
        }

        // ---- card placement
        let est_h = 210.0;
        let margin = 14.0;
        let card_pos = match target {
            Some(r) if big_target => Pos2::new(r.right() - CARD_W - margin, r.top() + margin),
            Some(r) => {
                let space_r = screen.right() - r.right();
                let space_l = r.left() - screen.left();
                let space_b = screen.bottom() - r.bottom();
                let x = if space_r >= CARD_W + 40.0 {
                    r.right() + 36.0
                } else if space_l >= CARD_W + 40.0 {
                    r.left() - 36.0 - CARD_W
                } else {
                    (r.center().x - CARD_W * 0.5).clamp(screen.left() + margin, screen.right() - CARD_W - margin)
                };
                let side = space_r >= CARD_W + 40.0 || space_l >= CARD_W + 40.0;
                let y = if side {
                    r.center().y - est_h * 0.5
                } else if space_b >= est_h + 40.0 {
                    r.bottom() + 30.0
                } else {
                    r.top() - 30.0 - est_h
                };
                Pos2::new(x, y.clamp(screen.top() + 48.0, screen.bottom() - est_h - margin))
            }
            None => Pos2::new(screen.center().x - CARD_W * 0.5, screen.bottom() - est_h - 60.0),
        };

        let mut action = None;
        let area = egui::Area::new(Id::new("ignis_tutorial_card"))
            .order(Order::Foreground)
            .fixed_pos(card_pos)
            .interactable(true)
            .show(ctx, |ui| {
                egui::Frame::NONE
                    .fill(CARD_BG)
                    .stroke(Stroke::new(1.0_f32, CARD_EDGE))
                    .corner_radius(CornerRadius::same(10))
                    .inner_margin(egui::Margin::same(14))
                    .shadow(egui::epaint::Shadow { offset: [0, 8], blur: 24, spread: 0, color: Color32::from_black_alpha(150) })
                    .show(ui, |ui| {
                        ui.set_width(CARD_W - 28.0);
                        self.card_contents(ui, lesson, step, prog, t, &mut action);
                    });
            });
        let card = area.response.rect;

        // ---- arrow from card to target
        if let (Some(r), false) = (target, big_target) {
            if !card.intersects(r.expand(6.0)) {
                let to = closest_point(r.expand(8.0), card.center());
                let from = closest_point(card, to);
                let bob = (t * 4.0).sin() * 3.0;
                let dir = (to - from).normalized();
                let to = to - dir * bob.abs();
                arrow(&painter, from, to, ACCENT);
            }
        }
        action
    }

    fn card_contents(&self, ui: &mut egui::Ui, lesson: &Lesson, step: Option<&Step>, prog: Progress, t: f32, action: &mut Option<OverlayAction>) {
        // header: lesson name + step counter + close
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(lesson.name.to_uppercase()).size(11.0).color(ACCENT).strong());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.add(egui::Button::new(egui::RichText::new("✕").size(12.0).color(MUTED)).frame(false)).on_hover_text("Close tutorial").clicked() {
                    *action = Some(OverlayAction::Close);
                }
                if prog.step < prog.steps {
                    ui.label(egui::RichText::new(format!("{} / {}", prog.step + 1, prog.steps)).size(11.0).color(MUTED));
                }
            });
        });
        // progress bar
        let (bar, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 4.0), egui::Sense::hover());
        let p = ui.painter();
        p.rect_filled(bar, CornerRadius::same(2), Color32::from_rgb(45, 43, 48));
        let mut fill = bar;
        fill.set_width(bar.width() * prog.fraction);
        p.rect_filled(fill, CornerRadius::same(2), ACCENT);
        ui.add_space(8.0);

        let Some(step) = step else {
            // lesson complete
            ui.label(egui::RichText::new("Lesson complete").size(19.0).color(GOOD).strong());
            ui.add_space(4.0);
            ui.label(egui::RichText::new(format!("You finished \"{}\".", lesson.name)).size(13.5).color(TEXT));
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                let next = self.lesson + 1;
                if next < self.lessons.len() {
                    if ui.add(egui::Button::new(egui::RichText::new(format!("Next: {}", self.lessons[next].name)).color(Color32::BLACK).strong()).fill(ACCENT)).clicked() {
                        *action = Some(OverlayAction::StartLesson(next));
                    }
                }
                if ui.button("Done").clicked() {
                    *action = Some(OverlayAction::Close);
                }
            });
            return;
        };

        if self.flash > 0.0 {
            let a = (self.flash.min(1.0) * 255.0) as u8;
            ui.label(egui::RichText::new("✔ Nice!").size(12.0).color(Color32::from_rgba_unmultiplied(GOOD.r(), GOOD.g(), GOOD.b(), a)));
        }
        ui.label(egui::RichText::new(step.title).size(18.0).color(TEXT).strong());
        ui.add_space(4.0);
        ui.label(egui::RichText::new(step.body).size(13.0).color(Color32::from_rgb(200, 196, 190)));
        ui.add_space(8.0);

        // do-this line with a pulsing dot
        ui.horizontal(|ui| {
            let (r, _) = ui.allocate_exact_size(Vec2::splat(10.0), egui::Sense::hover());
            let pulse = 0.6 + 0.4 * (t * 3.2).sin();
            ui.painter().circle_filled(r.center(), 3.0 + 1.5 * pulse, ACCENT);
            ui.label(egui::RichText::new(step.hint).size(13.0).color(ACCENT).strong());
        });

        // checklist for multi-part steps
        let checks = self.checklist();
        if checks.len() > 1 {
            ui.add_space(4.0);
            for (label, done) in checks {
                let (mark, col) = if done { ("✔", GOOD) } else { ("○", MUTED) };
                ui.label(egui::RichText::new(format!("  {mark}  {label}")).size(12.0).color(col));
            }
        }

        ui.add_space(10.0);
        ui.horizontal(|ui| {
            if prog.step > 0 && ui.add(egui::Button::new(egui::RichText::new("Back").size(12.0).color(MUTED)).frame(false)).clicked() {
                *action = Some(OverlayAction::Back);
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if self.wants_next() {
                    let b = egui::Button::new(egui::RichText::new("Next").color(Color32::BLACK).strong()).fill(ACCENT).min_size(Vec2::new(70.0, 24.0));
                    if ui.add(b).clicked() || ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        *action = Some(OverlayAction::Next);
                    }
                } else if ui.add(egui::Button::new(egui::RichText::new("Skip step").size(12.0).color(MUTED)).frame(false)).clicked() {
                    *action = Some(OverlayAction::SkipStep);
                }
            });
        });
    }
}

fn closest_point(r: Rect, p: Pos2) -> Pos2 {
    Pos2::new(p.x.clamp(r.left(), r.right()), p.y.clamp(r.top(), r.bottom()))
}

/// Curved arrow with a filled head.
fn arrow(p: &egui::Painter, from: Pos2, to: Pos2, col: Color32) {
    let d = to - from;
    let len = d.length();
    if len < 12.0 {
        return;
    }
    let n = Vec2::new(-d.y, d.x) / len;
    let ctrl = from + d * 0.5 + n * (len * 0.18).min(40.0);
    let pts: Vec<Pos2> = (0..=20)
        .map(|i| {
            let s = i as f32 / 20.0;
            let a = from.lerp(ctrl, s);
            let b = ctrl.lerp(to, s);
            a.lerp(b, s)
        })
        .collect();
    let shadow = Color32::from_black_alpha(120);
    p.add(egui::Shape::line(pts.iter().map(|q| *q + Vec2::new(0.0, 1.5)).collect(), Stroke::new(4.0_f32, shadow)));
    p.add(egui::Shape::line(pts.clone(), Stroke::new(2.5_f32, col)));
    let tip_dir = (to - pts[18]).normalized();
    let side = Vec2::new(-tip_dir.y, tip_dir.x);
    let head = vec![to, to - tip_dir * 12.0 + side * 6.5, to - tip_dir * 12.0 - side * 6.5];
    p.add(egui::Shape::convex_polygon(head, col, Stroke::NONE));
    p.circle_filled(from, 3.0, col);
}

// ---------------------------------------------------------------------------------------------
// Optional: lesson picker widget for a menu/help screen
// ---------------------------------------------------------------------------------------------

impl Tutorial {
    /// Draws a list of lessons with completion ticks; returns the index the user clicked.
    pub fn lesson_picker(&self, ui: &mut egui::Ui) -> Option<usize> {
        let mut picked = None;
        for (i, l) in self.lessons.iter().enumerate() {
            let done = self.is_lesson_complete(i);
            let r = ui
                .add(
                    egui::Button::new(
                        egui::RichText::new(format!("{}  {}", if done { "✔" } else { "▶" }, l.name)).size(14.0).color(if done { GOOD } else { TEXT }),
                    )
                    .min_size(Vec2::new(ui.available_width(), 28.0)),
                )
                .on_hover_text(l.blurb);
            if r.clicked() {
                picked = Some(i);
            }
        }
        picked
    }
}
