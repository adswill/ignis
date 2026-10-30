# Ignis engine / game split plan

Status: **design only, not executed.** Written 2026-10-28 against a tree that other agents were editing at the same time,
so line counts and exact item lists will drift. The structure below is what matters. Re-run the inventory commands in
§0 just before you execute.

Goal: turn the single `combustor` crate into a Cargo workspace with two crates:

- `crates/ignis_engine` (library). GPU simulation, shaders, scene model, element registry, save format, audio, headless bench and readback.
  It has no egui, eframe or rfd dependency.
- `crates/ignis` (package `ignis`, binary `ignis`, plus a lib target so examples can use it). The app/UI, experiments, campaign, tutorial, codex,
  design library, settings and showcase scenes.

Principles:

1. **Every step compiles** (`cargo build --workspace --release` plus a short bench). A step is never "move everything, then fix".
2. **Shim first, rewrite later.** While a module is moving, the game keeps `crate::gpu::…` paths working through a root-level
   `use ignis_engine::gpu;` re-export. A later step updates import paths mechanically, and the shims come out last.
3. **No behaviour change** during the split. Shader text, pass order, numbers and file formats stay byte-for-byte identical until
   the split is finished and verified. The data-driven element tables (§6) are follow-up work.
4. Assets stay at the workspace root (`assets/`). Crates reach them through `env!("CARGO_MANIFEST_DIR")`, never through bare `../`
   chains.

---

## 0. Preconditions and inventory (do these first, 15 min)

- **Freeze:** no other agent may edit `src/`, `examples/` or `tools/` while steps 1-9 run. The whole thing takes about half a day.
  If a freeze is impossible, run each step in a single burst and re-check the inventory between steps.
- **Backup:** there is no git repository. Before step 1, run
  `tar czf backups/pre_split_$(date +%Y%m%d%H%M).tgz Cargo.toml Cargo.lock src examples tools assets/campaign ARCHITECTURE.md`.
  Strongly recommended as well: `git init && git add -A && git commit -m "pre-split"`, so each step can be one commit.
- **Rename coordination:** another agent is renaming the package `combustor` → `ignis` (binary `ignis`). Do that rename
  **before** step 1, or fold it into step 1 as described there. Never do both at once in two agents.
- **Green baseline:** run `cargo build --release` and `QA_FAST=1 tools/qa.sh`, and save the report as the baseline (§9).
- **Inventory** (re-run; the tables below were made from these):
  ```bash
  cd "."
  for f in src/*.rs src/ui/*.rs; do echo "$f: $(grep -oE 'crate::[a-z_]+|super::[a-z_]+' $f | sort -u | tr '\n' ' ')"; done
  grep -rn 'include_str\|include_bytes\|#\[path' src examples
  grep -rn 'combustor\|src/' tools/*.sh tools/*.py
  grep -n '^mod \|^pub mod ' src/main.rs src/app.rs
  ```
  Known in-flux state at the time of writing: `src/main.rs` declares only `app, elements, missions, gpu, scene`, while
  `app.rs` already uses `crate::{audio, campaign_bridge, codex, library, settings, tutorial}`. `app.rs` also has
  `#[path = "ui/workspace.rs"] mod ws;`, but `src/ui/workspace.rs` did not exist yet. Expect the ui agent to finish this
  wiring first. `showcase.rs` is referenced only by `examples/library_seed.rs`.

### Current dependency graph (crate-internal)

```
gpu.rs ──(#[path] mod vfx)── vfx.rs           engine core, wgpu only
scene.rs        → gpu (Op, W, H)              engine
elements.rs     → scene, egui::Color32        engine (after Color32 removal)
audio.rs        → (cpal only)                 engine (feature "audio")
library.rs      → std + egui/rfd (browser submodule only)
                  split: format → engine, Library dir + browser → game
showcase.rs     → gpu, scene                  game
missions.rs     → elements, gpu, scene        game
campaign.rs     → (none; include_str campaign.json)    game
campaign_bridge → campaign, elements, gpu, missions, scene   game
codex.rs, tutorial.rs, settings.rs → egui     game
app.rs (+ ui/theme, ui/workspace, ui/probes) → everything        game
main.rs: --bench headless runner + eframe entry  split: bench core → engine, CLI → game
```

Because the engine side never depends on the game side, the split has no cycles to break. The only cross-cutting
problems are `egui::Color32` in `elements.rs`, the egui browser inside `library.rs`, and the `fn gas()` decoder that lives in
`app.rs` but belongs to the readback API.

---

## 1. Target layout

```
combustor game/                      (later: rename folder to "ignis"; not part of this plan)
├── Cargo.toml                       virtual workspace manifest (below)
├── Cargo.lock                       unchanged location
├── target/                          unchanged location → target/release/ignis
├── assets/                          unchanged (branding, campaign, codex)
├── tools/                           unchanged location; paths updated (§7)
├── docs/, designs/, dist/, backups/, old_cpu/   untouched
└── crates/
    ├── ignis_engine/
    │   ├── Cargo.toml
    │   ├── shaders/
    │   │   ├── sim.wgsl  matter.wgsl  vis.wgsl          compute module (concatenated in this order)
    │   │   ├── fx.wgsl  particles.wgsl  detail.wgsl  render.wgsl
    │   │   └── (later) elements_gen.wgsl               generated, §6
    │   ├── data/                                       (later) elements.ron / materials.ron, §6
    │   ├── examples/      gpu_profile.rs physval.rs showcase-free bench examples, audio_demo.rs
    │   └── src/
    │       ├── lib.rs           module tree + re-exports + ENGINE_VERSION
    │       ├── consts.rs        W, H, N, DT, GRAVITY, MAX_OPS, WALL_EVERY, RK_TX/TY (from gpu.rs)
    │       ├── shaders.rs       include strategy (§3)
    │       ├── gpu/
    │       │   ├── mod.rs       pub use of everything below; GpuSim struct + new/frame/draw façade
    │       │   ├── types.rs     Params (pub(crate)), Op, FrameInput, Stats, ProbeData
    │       │   ├── context.rs   GpuContext (device/queue acquisition: headless + "bring your own device")
    │       │   ├── buffers.rs   SimBuffers (params, ops, u0/u1/u2, meta, wall, flux, stats, probe) + storage()
    │       │   ├── pipelines.rs SimPipelines (shader modules, bind group layouts, compute pipelines)
    │       │   ├── schedule.rs  frame scheduling: the per-frame pass order (init/ops/steps/wall/matter/vis/fx/probe)
    │       │   ├── readback.rs  async probe/stats state machine + blocking read_meta/read_wall/read_state
    │       │   └── vfx.rs       Vfx (fx/particles/detail/render pipelines + draw)  ← src/vfx.rs
    │       ├── scene/
    │       │   ├── mod.rs       Scene, tile + material constants, put(), by_name()
    │       │   └── presets.rs   rocket/turbojet/... + SHOWCASE (bench & regression fixtures)
    │       ├── registry.rs      ← elements.rs (Tool, Cat, Element, MatInfo, MatterInfo, WallSi, tables, lookups), Rgb8
    │       ├── save/
    │       │   ├── mod.rs       Design, DesignInfo, encode/decode, FORMAT_VERSION, migrations
    │       │   ├── codec.rs     zlib, crc32, png encode/decode (from library.rs)
    │       │   └── thumb.rs     RgbaImage, Palette, render_cells/render_thumb/render_design_thumb
    │       ├── probe.rs         GasState decode (← app.rs `fn gas`), CellSample, unit constants
    │       ├── bench.rs         headless bench API (← main.rs `headless`), PPM writer
    │       └── audio.rs         ← src/audio.rs   (#[cfg(feature = "audio")])
    └── ignis/
        ├── Cargo.toml           package "ignis", [[bin]] name = "ignis", lib name "ignis"
        ├── examples/            campaign_check, campaign_bridge_check, codex_check, codex_view, library_seed,
        │                        library_view, settings_check, showcase_test, tutorial_check, branding
        └── src/
            ├── main.rs          CLI: --bench → ignis_engine::bench, --list-scenes, else eframe::run_native
            ├── lib.rs           pub mod app; missions; campaign; campaign_bridge; codex; library; settings; showcase; tutorial
            ├── app.rs           (+ app/ or ui/ children kept via the existing #[path] attributes)
            ├── ui/              theme.rs workspace.rs probes.rs
            ├── missions.rs  campaign.rs  campaign_bridge.rs  tutorial.rs  codex.rs  settings.rs  showcase.rs
            ├── library.rs       Library (dir mgmt, seed, import/export) + browser submodule; uses ignis_engine::save
            └── color.rs         `fn c32(Rgb8) -> egui::Color32` + `trait ToColor32`
```

### Root `Cargo.toml` (virtual manifest)

```toml
[workspace]
resolver = "2"
members = ["crates/ignis_engine", "crates/ignis"]
default-members = ["crates/ignis"]          # plain `cargo build --release` still produces target/release/ignis

[workspace.package]
version = "0.2.0"                           # tools/bundle_macos.sh reads the first ^version line, which is this one
edition = "2021"

[workspace.dependencies]
wgpu = "24.0"                               # MUST equal eframe 0.31's egui-wgpu wgpu (24.0.5 in Cargo.lock)
bytemuck = { version = "1", features = ["derive"] }
pollster = "0.4"
cpal = "0.18"
eframe = { version = "0.31", default-features = false, features = ["wgpu", "default_fonts"] }
rfd = "0.15"
ignis_engine = { path = "crates/ignis_engine" }

[profile.release]
opt-level = 3
lto = "thin"
debug = false
```

### `crates/ignis_engine/Cargo.toml`

```toml
[package]
name = "ignis_engine"
version.workspace = true
edition.workspace = true

[features]
default = ["audio"]
audio = ["dep:cpal"]

[dependencies]
wgpu.workspace = true
bytemuck.workspace = true
pollster.workspace = true
cpal = { workspace = true, optional = true }
```

### `crates/ignis/Cargo.toml`

```toml
[package]
name = "ignis"
version.workspace = true
edition.workspace = true

[lib]
name = "ignis"
path = "src/lib.rs"

[[bin]]
name = "ignis"
path = "src/main.rs"

[dependencies]
ignis_engine.workspace = true
eframe.workspace = true
rfd.workspace = true
pollster.workspace = true      # only while main.rs still builds its own headless device; drop after step 7
```

**wgpu version unification is the #1 build risk.** The engine names `wgpu::Device` and the app passes in
`egui_wgpu::RenderState::device`. Both must resolve to the *same* wgpu crate. Check after step 2:
`cargo tree -d -e normal | grep -i wgpu` should print nothing (no duplicate). The engine re-exports `pub use wgpu;`, so
examples and the game can write `ignis_engine::wgpu` and never name a second version. When eframe is bumped, bump
`workspace.dependencies.wgpu` in the same commit.

---

## 2. Public engine API (what the game is allowed to use)

Keep it small. Everything not listed here is `pub(crate)`. Signatures match today's code, so the move is a path change,
not a redesign. Items marked *(new)* are thin wrappers that are added after the move (step 8).

```rust
// ignis_engine/src/lib.rs
pub use wgpu;
pub const ENGINE_VERSION: &str = env!("CARGO_PKG_VERSION");

pub mod consts {                    // re-exported as ignis_engine::{W, H, DT, ...} and as gpu::{W,H,DT,GRAVITY,MAX_OPS}
    pub const W: u32 = 512;  pub const H: u32 = 288;  pub const N: usize = (W * H) as usize;
    pub const DT: f32 = 0.028;  pub const GRAVITY: f32 = 0.0015;  pub const MAX_OPS: usize = 256;
    pub const WALL_EVERY: u32 = 4;                 // was private; gpu_profile needs it (stops source-text parsing)
}

pub mod gpu {
    #[repr(C)] #[derive(Clone, Copy, Pod, Zeroable, Debug, Default, PartialEq)]   // + Debug/Default/PartialEq (new)
    pub struct Op { pub kind: u32, pub shape: u32, pub x0: i32, pub y0: i32, pub x1: i32, pub y1: i32,
                    pub r: i32, pub tile: u32, pub mat: u32, pub dir: u32, pub _p: [u32; 2] }
    impl Op {                                                                      // (new) constructors, optional
        pub fn disc(kind: u32, x: i32, y: i32, r: i32, tile: u32, mat: u32) -> Op;
        pub fn rect(kind: u32, x0: i32, y0: i32, x1: i32, y1: i32, tile: u32, mat: u32) -> Op;
        pub fn line(kind: u32, x0: i32, y0: i32, x1: i32, y1: i32, half_w: i32, tile: u32, mat: u32) -> Op;
    }
    pub mod op_kind { pub const PLACE: u32 = 0; pub const ERASE: u32 = 1; /* ... 7 VACUUM */ }   // (new)

    #[derive(Clone, Default)]                                                      // + Default (new)
    pub struct FrameInput { pub steps: u32, pub ops: Vec<Op>, pub view: u32, pub p_fuel: f32, pub p_o2: f32,
        pub p_air: f32, pub fan_pow: f32, pub gravity: f32, pub cam: [f32; 4], pub ign_on: bool, pub time: f32,
        pub frame: u32, pub scene: Option<Vec<u32>>, pub meta_only: Option<Vec<u32>>, pub reset_gas: bool,
        pub probe: Option<(i32, i32)> }
    #[derive(Clone, Copy, Default, Debug)] pub struct Stats { pub max_p: f32, pub max_t: f32, pub max_wall_heat: f32,
        pub burn: f32, pub walls: u32, pub max_stress: f32, pub thrust: [f32; 2] }
    #[derive(Clone, Copy, Default, Debug)] pub struct ProbeData { pub u: [f32; 8], pub meta: u32, pub wall: [f32; 4] }

    pub struct GpuSim { /* all fields private except the two below (kept pub for compatibility) */
        pub probe: Option<ProbeData>, pub stats: Stats }
    impl GpuSim {
        pub fn new(device: &wgpu::Device, target: wgpu::TextureFormat) -> Self;
        pub fn frame(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, enc: &mut wgpu::CommandEncoder, inp: &FrameInput);
        pub fn draw(&self, rp: &mut wgpu::RenderPass<'_>);
        pub fn read_meta(&self, device: &wgpu::Device, queue: &wgpu::Queue) -> Vec<u32>;
        pub fn read_wall(&self, device: &wgpu::Device, queue: &wgpu::Queue) -> Vec<f32>;
        pub fn read_state(&self, device: &wgpu::Device, queue: &wgpu::Queue) -> (Vec<f32>, Vec<u32>);
        pub fn stats(&self) -> Stats;                    // (new) accessor; field stays pub until the game stops using it
        pub fn probe(&self) -> Option<ProbeData>;        // (new)
        pub fn step_count(&self) -> u32;                 // (new) for the status bar / conservation tracking
    }
    pub const STEP_KERNELS: &[&str];  pub const VIS_KERNELS: &[&str];   // (new) used by gpu_profile instead of text parsing
    pub mod vfx { pub const LAYERS: u32; pub const NOISE: u32; }        // (new) pub for gpu_profile

    pub struct GpuContext { pub device: wgpu::Device, pub queue: wgpu::Queue, pub adapter_info: wgpu::AdapterInfo }
    impl GpuContext { pub fn headless() -> Result<GpuContext, String>; }   // (new) replaces 4 copies of adapter setup
}

pub mod scene {
    pub const WALL: u32; FUEL_IN; O2_IN; AIR_IN; FAN; IGNITER; VENT; MATTER;          // tiles
    pub const STEEL: u32; INCONEL; COOLED_CU; WOOD; PROPELLANT; EXPLOSIVE;            // wall materials
    pub struct Scene { pub meta: Vec<u32>, pub inlet_p: f32, pub o2_p: f32, pub ops: Vec<Op> }
    impl Scene { pub fn empty() -> Self; pub fn put(&mut self, x: i32, y: i32, tile: u32, mat: u32, dir: u32);
                 pub fn by_name(name: &str) -> Option<Scene>; /* + every preset fn, unchanged */ }
    pub const SHOWCASE: [(&str, &str, &str, fn() -> Scene); 12];
    pub const BENCH_SCENES: &[(&str, fn() -> Scene)];   // (new) the "jet"/"rocket"/... table now inlined in main.rs
    pub fn meta_pack(tile: u32, mat: u32, dir: u32) -> u32;  pub fn meta_unpack(m: u32) -> (u32, u32, u32);   // (new)
}

pub mod registry {                                       // was elements.rs; also re-exported as ignis_engine::elements
    #[derive(Clone, Copy, PartialEq, Eq, Debug)] pub struct Rgb8(pub u8, pub u8, pub u8);   // replaces egui::Color32
    pub enum Tool { Wall(u32), Part(u32), Gas(u32), Matter(u32), Erase }
    pub enum Cat { Structure, Burnable, Powders, Liquids, Machines, Gases, Tools, Metals, Chemicals }
    pub struct MatInfo { /* unchanged, color: Rgb8 */ }  pub struct Element { /* unchanged, color: Rgb8 */ }
    pub struct MatterInfo { /* unchanged */ }  pub struct WallSi { /* unchanged */ }
    pub const CATS; MATS; ELEMENTS; EXTRA; MATTER; RECIPES; MATTER_INFO; WALL_SI;   // unchanged lengths
    pub fn all() -> impl Iterator<Item = &'static Element>;  pub fn find(tool: Tool) -> Option<&'static Element>;
    pub fn matter_info(id: u32) -> Option<&'static MatterInfo>;  pub fn matter_name(id: u32) -> &'static str;
    pub fn wall_si(mat: u32) -> Option<&'static WallSi>;
    pub const N_ELEM: u32;                                // (new) mirrors matter.wgsl, checked by a unit test
}

pub mod save {
    pub const MAGIC: &[u8; 4] = b"CMB1";  pub const FORMAT_VERSION: u32;      // chunk version, currently 1
    pub struct DesignInfo { /* unchanged */ }  pub struct Design { /* unchanged */ }
    impl Design { pub fn new(w: u32, h: u32, meta: Vec<u32>, params: [f32; 4], info: DesignInfo) -> Self; }
    pub fn encode_cmb(d: &Design) -> Vec<u8>;  pub fn decode_cmb(b: &[u8]) -> Result<Design, String>;
    pub struct RgbaImage { .. }  pub struct Palette { .. }
    pub fn render_cells(w: u32, h: u32, meta: &[u32], pal: &Palette) -> RgbaImage;
    pub fn render_thumb(..) -> RgbaImage;  pub fn render_design_thumb(d: &Design, pal: &Palette) -> RgbaImage;
    pub fn encode_png(img: &RgbaImage) -> Vec<u8>;  pub fn decode_png(b: &[u8]) -> Result<RgbaImage, String>;
    pub fn zlib_compress(..); pub fn zlib_decompress(..); pub fn crc32(..);   // pub because tests/examples use them
    pub const GRID_W: u32; GRID_H; THUMB_W; THUMB_H;
}

pub mod probe {                                          // (new home of app.rs `fn gas` + unit constants)
    pub struct GasState { pub rho: f32, pub p: f32, pub t: f32, pub vel: [f32; 2], pub y: [f32; 4], pub soot: f32 }
    pub fn gas(u: &[f32; 8]) -> GasState;
    pub const MS_PER_UNIT: f32; MPS_PER_UNIT; KN_PER_UNIT; KGS_PER_UNIT; K_PER_UNIT_T; KPA_PER_UNIT_P;
}

pub mod bench {
    pub struct BenchConfig { pub frames: u32, pub steps: u32, pub every: u32, pub view: u32, pub shot: Option<PathBuf>,
        pub shot_steps: Option<u32>, pub gravity: f32, pub fan: f32, pub ign: bool, pub ramp_frames: u32,
        pub p_fuel: Option<f32>, pub p_o2: Option<f32>, pub dump: DumpOpts }
    impl BenchConfig { pub fn from_env(frames: u32, view: u32, shot: Option<PathBuf>) -> Self; }  // STEPS, EVERY, GRAV, FAN, IGN, ...
    pub struct BenchSample { pub frame: u32, pub t: f32, pub pmax: f32, pub tmax: f32, pub vmax: f32, pub bad: u32, pub stats: Stats }
    pub struct BenchReport { pub samples: Vec<BenchSample>, pub wall_secs: f64 }
    pub fn run(ctx: &GpuContext, scene: &Scene, cfg: &BenchConfig, out: &mut dyn std::io::Write) -> BenchReport;
    pub fn write_ppm(path: &Path, w: u32, h: u32, rgba: &[u8]) -> std::io::Result<()>;
}
// The printed lines must stay byte-identical: tools/qa_run.py parses them.

#[cfg(feature = "audio")]
pub mod audio { pub struct AudioSnapshot { .. }  pub struct Audio;
    impl Audio { pub fn new() -> Option<Audio>; pub fn update(&mut self, s: &AudioSnapshot); pub fn boom(&mut self, strength: f32);
                 pub fn set_volume(&mut self, v: f32); pub fn volume(&self) -> f32; pub fn set_muted(&mut self, m: bool); pub fn muted(&self) -> bool; }
    pub fn render_offline(secs: f32, sr: u32) -> Vec<f32>;  pub fn demo_cli(); }

pub mod shaders { pub const SIM: &str; MATTER; VIS; FX; PARTICLES; DETAIL; RENDER;  pub fn compute_source() -> String; }
```

Stability rules for the engine API:

- Treat changes to `Op`, `Params` and the WGSL `Op`/`Params` structs as **ABI changes**. Bump `FORMAT_VERSION` only when the save layout changes.
- New `FrameInput` fields must have a `Default`. The game builds `FrameInput { .., ..Default::default() }`.
- No egui types in engine signatures. Colors are `Rgb8`/`[u8; 4]`, images are `RgbaImage`.
- The egui integration (`egui_wgpu::CallbackTrait` that calls `frame`/`draw`, plus the `callback_resources` storage of `GpuSim`)
  stays in the game. `GpuSim` must remain `Send + Sync + 'static`. Add a `static_assertions`-style compile check in the engine:
  `const _: fn() = || { fn ok<T: Send + Sync + 'static>() {} ok::<GpuSim>(); };`

---

## 3. Shader include strategy

Today: `gpu.rs` uses `include_str!("sim.wgsl")` and the rest by relative path, and `vfx.rs` does the same for fx/particles/detail/render.
`examples/gpu_profile.rs` uses `include_str!("../src/*.wgsl")` and `examples/physval.rs` reads `$SRC_DIR` (default is an absolute
path into `src/`) at runtime.

Target: `crates/ignis_engine/src/shaders.rs`

```rust
macro_rules! shader { ($f:literal) => { include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/shaders/", $f)) }; }
pub const SIM: &str = shader!("sim.wgsl");
pub const MATTER: &str = shader!("matter.wgsl");
pub const VIS: &str = shader!("vis.wgsl");
pub const FX: &str = shader!("fx.wgsl");
pub const PARTICLES: &str = shader!("particles.wgsl");
pub const DETAIL: &str = shader!("detail.wgsl");
pub const RENDER: &str = shader!("render.wgsl");
/// sim + matter + vis share one module and one binding set (order matters; no duplicate names across them).
pub fn compute_source() -> String { format!("{SIM}\n{MATTER}\n{VIS}") }   // exactly the format! gpu.rs uses today
/// Dev-only: IGNIS_SHADER_DIR=/path loads shaders from disk at GpuSim::new (hot iteration without recompiling Rust).
pub fn load(name: &str, embedded: &'static str) -> std::borrow::Cow<'static, str>;
```

- `gpu/pipelines.rs` and `gpu/vfx.rs` reference only `crate::shaders::*`. No other file contains `include_str!` of a shader.
- `include_str!` already makes cargo rebuild on shader edits, so no `build.rs` is needed for the move. A `build.rs` appears only with §6 phase A.
- Keep `COMBUSTOR_CHECKED` (unchecked sim module unless set) working; add `IGNIS_CHECKED` as an alias.
- Add a cheap CI-style test that needs no GPU: `#[test] fn wgsl_parses()`. It runs `naga` front-end parsing on `compute_source()`
  and each other shader. naga is already in the tree through wgpu, so add `naga = { version = "24", features = ["wgsl-in"] }` as a
  dev-dependency matching wgpu's naga. This catches the "reserved word / pipeline panic at runtime" class of bugs that
  `cargo build` misses today.

---

## 4. Ordered move list (file / item → destination)

E = `crates/ignis_engine`, G = `crates/ignis`. "Shim" = a line in G's crate root that keeps old `crate::x` paths valid.

| # | Current | Destination | Visibility / API changes | Import changes |
|---|---|---|---|---|
| 1 | `src/sim.wgsl`, `matter.wgsl`, `vis.wgsl` | `E/shaders/` | — | `include_str!` → `crate::shaders::{SIM,MATTER,VIS}` |
| 2 | `src/fx.wgsl`, `particles.wgsl`, `detail.wgsl`, `render.wgsl` | `E/shaders/` | — | vfx.rs `include_str!` → `crate::shaders::*` |
| 3 | `src/gpu.rs` | `E/src/gpu/mod.rs` (split later, step 8) | `WALL_EVERY` → `pub`; add `Default` to `FrameInput`; `Debug/Default/PartialEq` to `Op` | `use eframe::wgpu` → `use wgpu`; `#[path = "vfx.rs"] mod vfx` → `mod vfx;` (file moves next to it) |
| 4 | `src/vfx.rs` | `E/src/gpu/vfx.rs` | `LAYERS`, `NOISE` → `pub` | `crate::gpu::{DT,H,W}` → `crate::consts::*` or `super::*` |
| 5 | constants in gpu.rs (`W,H,DT,GRAVITY,MAX_OPS,WALL_EVERY,RK_TX,RK_TY`) | `E/src/consts.rs`, re-exported from `gpu` | RK_* stay `pub(crate)` | none (re-export) |
| 6 | `src/scene.rs` | `E/src/scene/mod.rs` + `presets.rs` | none (already pub) | `crate::gpu::{Op,H,W}` unchanged inside E |
| 7 | `src/elements.rs` | `E/src/registry.rs` (+ `pub use registry as elements;`) | `color: Color32` → `color: Rgb8` in `MatInfo`, `Element`; `const fn m/mt` build `Rgb8` | `use eframe::egui::Color32` removed; `crate::scene` unchanged |
| 8 | `src/library.rs` lines ~1-1200 (format, codec, png, thumbs, `GRID_*`, `THUMB_*`) | `E/src/save/{mod,codec,thumb}.rs` | all currently-`pub` stay pub; `Rd`, `BitW`, `BitR`, `Huff` → `pub(crate)` | — |
| 9 | `src/library.rs` `library_dir`, `Library`, `Entry`, `now_unix`, `format_date`, `parse_tags`, `pub mod browser` | `G/src/library.rs` | — | add `pub use ignis_engine::save::*;` at top of G's library.rs so `crate::library::{Design, encode_cmb, ..}` keeps working |
| 10 | `src/audio.rs` | `E/src/audio.rs` behind feature `audio` | none | none (std + cpal only) |
| 11 | `fn gas` + `MS_PER_UNIT`, `MPS_PER_UNIT`, `KN_PER_UNIT`, `KGS_PER_UNIT` in `app.rs` | `E/src/probe.rs` | `Gas` → `pub struct GasState` (keep `pub type Gas = GasState` in app) | `ui/probes.rs`: `use super::{gas, ...}` → `use ignis_engine::probe::{gas, ...}` |
| 12 | `main.rs::headless` (≈200 lines) | `E/src/bench.rs` (`BenchConfig::from_env`, `run`) | — | G main.rs: `"--bench" => ignis::cli::bench(&args)`, which maps names via `scene::BENCH_SCENES` + `Scene::by_name` |
| 13 | `main.rs` eframe entry (`run_native`, viewport, icon) | `G/src/main.rs` | — | `mod x;` lines → `use ignis::{app, ..}` (lib target) |
| 14 | `src/app.rs`, `src/ui/{theme,workspace,probes}.rs` | `G/src/` (same relative layout, so the `#[path = "ui/…"]` attributes keep working) | — | via shims first (step 3), then `crate::gpu` → `ignis_engine::gpu` etc. (step 9) |
| 15 | `src/missions.rs`, `campaign.rs`, `campaign_bridge.rs`, `tutorial.rs`, `settings.rs`, `codex.rs`, `showcase.rs` | `G/src/` | — | `campaign.rs`, `codex.rs`: asset paths (risk R2) |
| 16 | `examples/gpu_profile.rs`, `physval.rs`, `audio_demo.rs` | `E/examples/` | — | drop `#[path]`; use `ignis_engine::{gpu, scene, shaders, consts}`; physval `SRC_DIR` default → `concat!(env!("CARGO_MANIFEST_DIR"), "/shaders")` |
| 17 | `examples/showcase_test.rs`, `campaign_check.rs`, `campaign_bridge_check.rs`, `codex_check.rs`, `codex_view.rs`, `library_seed.rs`, `library_view.rs`, `settings_check.rs`, `tutorial_check.rs`, `branding.rs` | `G/examples/` | — | drop `#[path]` + the fake `mod gpu` in library_seed; use `ignis::…` / `ignis_engine::…`; branding.rs `include_bytes!("../assets/…")` → `concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/…")` |
| 18 | `Cargo.toml` (package) | root virtual manifest + 2 crate manifests | — | — |
| 19 | `tools/qa.sh`, `qa_run.py`, `profile.sh`, `bundle_macos.sh` | stay | binary + source paths (R4-R7) | — |
| 20 | `ARCHITECTURE.md` | stays at root | update "Files", "Rust API", "Testing" sections | — |

### Risk points (check each explicitly)

- **R1 shader `include_str!`**: gpu.rs:209 (`format!("{}\n{}\n{}", sim, matter, vis)`) and vfx.rs:182/217/307/310. After moving,
  `grep -rn 'include_str!("' crates/ignis_engine/src | grep wgsl` must return nothing outside `shaders.rs`.
- **R2 asset `include_str!`**: `campaign.rs:20` `include_str!("../assets/campaign/campaign.json")` and **every line** of the codex
  table in `codex.rs` (`include_str!("../assets/codex/<name>.md")`, about 120 lines). From `crates/ignis/src/` the relative path would be
  `../../../assets/…`. Use instead
  `macro_rules! asset { ($p:literal) => { include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/", $p)) } }`
  and a one-shot `sed -i '' 's#include_str!("../assets/\([^"]*\)")#asset!("\1")#' crates/ignis/src/codex.rs crates/ignis/src/campaign.rs`.
  Also grep for `include_bytes!` (window icon / title art in main.rs or app.rs).
- **R3 examples `#[path = "../src/…"]`**: 10 of 13 examples compile game sources directly. The `#[path]` targets break the moment a file
  moves. Fix order: in step 2, move examples into the crates and switch them to crate imports at the same time as the files they include.
  `gpu_profile.rs` also **parses source text** of gpu.rs/vfx.rs (`"steps: ["`, `"vis: ["`, `"const WALL_EVERY: u32"`,
  `"if k >= "`, `"const LAYERS: u32"`, `"const NOISE: u32"`). After step 8 splits gpu.rs, those strings live in other files. Replace them
  with the pub constants from §2 (`consts::WALL_EVERY`, `gpu::STEP_KERNELS`, `gpu::VIS_KERNELS`, `gpu::vfx::{LAYERS, NOISE}`).
  Interim, before step 8: repoint the two `include_str!`s to `../src/gpu/mod.rs` and `../src/gpu/vfx.rs`.
- **R4 `tools/qa.sh`**: checks `-x "$ROOT/target/release/combustor"`. `cargo build --release` at the root still works (default-members).
  Change the binary name to `ignis` (or `${IGNIS_BIN:-ignis}`). If the rename agent already did this, there's nothing to do.
- **R5 `tools/qa_run.py`**: line ~286 reads `src/main.rs` to discover bench scene names from the `"name" => scene::Scene::…` match arms
  (and scene.rs `by_name`). Both files move. Preferred fix: add `ignis --list-scenes` (prints one name per line from
  `BENCH_SCENES` + `SHOWCASE`) and have qa_run.py call it. Fallback: point it at `crates/ignis_engine/src/scene/mod.rs` and parse
  `BENCH_SCENES`. Lines ~313/390/440 hard-code `target/release/combustor`. The bench stdout format must not change (qa_run parses it).
- **R6 `tools/profile.sh`**: `BIN=…/combustor` → `ignis`; `PROF=target/release/examples/gpu_profile` is unchanged (examples of any member
  land in the shared `target/release/examples/`). Its build line must become `cargo build --release -p ignis_engine --example gpu_profile`.
- **R7 `tools/bundle_macos.sh`**: `BIN_SRC=…/combustor` → `…/ignis`. `VERSION` is read with `sed -n 's/^version *= *"\(.*\)"/\1/p' Cargo.toml`.
  This still works because `[workspace.package] version = "0.2.0"` sits in the root manifest. Do not write it as
  `version.workspace` at the root. Icon/branding paths are root-relative and unchanged.
- **R8 wgpu duplication**: see §1. Symptom: "expected `wgpu::Device`, found `wgpu::Device`" errors in app.rs.
- **R9 egui `Color32` in registry**: `Color32::from_rgb` is `const fn`, and so is `Rgb8(..)`. Usage sites to convert:
  `codex.rs` (~6), `library.rs` (~6, the browser palette), `ui/theme.rs` (~4), `app.rs`/`ui/workspace.rs` (`e.color`, `m.color`).
  Add `G/src/color.rs`: `pub trait ToColor32 { fn c32(self) -> Color32; } impl ToColor32 for Rgb8 { … }` and replace `x.color`
  with `x.color.c32()` (compile errors list every site).
- **R10 `#[cfg(test)]` modules** (`settings.rs`, `codex.rs`, `library.rs`): they use `super::*` and move with their files. `cargo test --workspace`
  must pass. `library.rs` tests that exercise the codec move to `E/src/save/`.
- **R11 runtime paths**: `~/Library/Application Support/Combustor/` (settings, progress, library), env hooks `COMBUSTOR_SHOT`,
  `COMBUSTOR_UNLOCK`, `COMBUSTOR_CHECKED`, `SHOT_*`. Keep them unchanged in this plan (tools depend on them). An `IGNIS_*` alias
  plus a data-dir migration is a separate task.
- **R12 window title** `"Combustor"` in main.rs is user-visible. Change it to `"Ignis"` (allowed by ARCHITECTURE.md "Product name").

---

## 5. Step-by-step execution (compiles after every step)

Each step ends with: `cargo build --workspace --release && ./target/release/<bin> --bench 60 rocket` (no `bad` cells, pmax
in range). Effort is for one agent working mechanically.

**Step 1: workspace skeleton, nothing moved (20 min).**
1. `mkdir -p crates/ignis/src crates/ignis_engine/{src,shaders,examples}`.
2. `git mv`-equivalent (`mv`): `src/` → `crates/ignis/src/`, `examples/` → `crates/ignis/examples/`. The root `Cargo.toml` becomes the
   virtual manifest from §1, and the old `[package]`+`[dependencies]` move into `crates/ignis/Cargo.toml` as package `ignis` (bin `ignis`;
   this is where the rename lands if it hasn't happened yet). Leave the `[lib]` section out for now.
3. `crates/ignis_engine/src/lib.rs` = `//! Ignis simulation engine.` (empty). Add it to members.
4. Fix R2 now (codex/campaign asset macro), because the relative depth changed. Examples: `#[path = "../src/x.rs"]` still resolves
   (examples/ and src/ moved together). Fix `branding.rs` `include_bytes!` and `physval` `SRC_DIR` default.
5. Update tools R4/R5(path only: `crates/ignis/src/main.rs`)/R6/R7 to binary `ignis`.
   ✔ Verify: `cargo build --workspace --release`, `cargo build --release --examples`, `QA_FAST=1 QA_SCENES="rocket campfire" tools/qa.sh`.

**Step 2: move GPU core + shaders into the engine (45 min).**
1. `mv crates/ignis/src/{sim,matter,vis,fx,particles,detail,render}.wgsl crates/ignis_engine/shaders/`.
2. `mv crates/ignis/src/gpu.rs crates/ignis_engine/src/gpu/mod.rs`, `mv crates/ignis/src/vfx.rs crates/ignis_engine/src/gpu/vfx.rs`.
3. Write `shaders.rs` (§3) and `consts.rs` (move consts, `pub use crate::consts::*;` in gpu/mod.rs). Replace the 5 `include_str!` sites.
   Change `use eframe::wgpu` to `use wgpu` (2 files). Delete the `#[path = "vfx.rs"]` attribute. Make `WALL_EVERY`, `vfx::LAYERS` and `vfx::NOISE` `pub`.
4. Engine `lib.rs`: `pub use wgpu; pub mod consts; pub mod shaders; pub mod gpu;`. Engine Cargo.toml deps per §1.
5. Game: add `ignis_engine` dep. In `main.rs`, replace `mod gpu;` with `pub use ignis_engine::gpu;` (shim, so every `crate::gpu::X` keeps working).
6. Examples that `#[path]` gpu.rs (`gpu_profile`, `showcase_test`, `campaign_check`, `campaign_bridge_check`): replace
   `#[path="../src/gpu.rs"] mod gpu;` with `use ignis_engine::gpu;`. Move `gpu_profile.rs` and `physval.rs` to `E/examples/` and repoint their
   `include_str!`s to `ignis_engine::shaders::*` (and the gpu/vfx source-text includes to the new paths, R3).
   ✔ Verify: `cargo tree -d | grep wgpu` is empty (R8). Run benches rocket/campfire/chem. Take an app screenshot (§9) to prove that the
   fx/render path and the egui callback still draw.

**Step 3: scene + registry into the engine (40 min).**
1. `mv scene.rs` → `E/src/scene/mod.rs` (split presets into `presets.rs` later, or now with `pub use presets::*` if trivial).
2. `mv elements.rs` → `E/src/registry.rs`. Introduce `Rgb8`. Swap `Color32` for `Rgb8` in the struct defs and `const fn`s.
   `lib.rs`: `pub mod scene; pub mod registry; pub use registry as elements;`.
3. Game shims in main.rs: `pub use ignis_engine::{scene, elements};`. Add `G/src/color.rs` (`ToColor32`) and fix every compile
   error at a `.color` use site (R9).
4. `#[path]` examples that include scene.rs/elements.rs switch to `use ignis_engine::{scene, elements}`. `library_seed.rs` drops its fake `mod gpu`.
   ✔ Verify: build, run bench on all 12 bench scenes plus 2 SHOWCASE names, and check the app screenshot of the element browser (colors identical).

**Step 4: save format out of library.rs (45 min).**
1. Create `E/src/save/{mod,codec,thumb}.rs` by cutting the std-only top part of `library.rs` (everything above `library_dir()`/`Library`).
   `Palette` may need default colors from the registry: use `crate::registry` there, not egui.
2. G `library.rs` keeps `library_dir`, `Library`, `Entry`, date/tag helpers and `pub mod browser`, and starts with `pub use ignis_engine::save::*;`.
3. Move codec unit tests to the engine. Add the round-trip test `decode_cmb(encode_cmb(d)) == d` plus a **golden file test**: copy one
   real `.cmb` from `designs/` into `E/tests/data/` and assert it decodes (protects old saves).
   ✔ Verify: `cargo test --workspace`, open/save a design in the app (⌘S/⌘O), and check that library thumbnails render.

**Step 5: audio into the engine (15 min).**
`mv audio.rs` → `E/src/audio.rs`, `#[cfg(feature="audio")] pub mod audio;`, game shim `pub use ignis_engine::audio;`. Move `audio_demo.rs` to `E/examples/`
(`required-features = ["audio"]`). ✔ Verify: `cargo run --release -p ignis_engine --example audio_demo` renders offline without crashing.

**Step 6: probe/readback decode into the engine (20 min).**
Move `fn gas`, `struct Gas` and the unit constants from app.rs to `E/src/probe.rs` (`GasState`). In app.rs keep
`use ignis_engine::probe::{gas, GasState as Gas, MS_PER_UNIT, …};` so `super::gas` in `ui/probes.rs` still resolves.
✔ Verify: build, then an app screenshot with `SHOT_DRAWER=1` (inspector readouts show the same numbers as the baseline).

**Step 7: headless bench API (45 min).**
1. Add `gpu::GpuContext::headless()` (the adapter/device code from `main.rs::headless`).
2. Move the body of `headless()` into `bench::run`. Env parsing goes into `BenchConfig::from_env`, and the PPM writer moves too. **Keep every
   `println!` format string identical.** Add `scene::BENCH_SCENES` (the `"jet" => turbojet, …` table) and make
   `Scene::by_name` fall back to it.
3. G `main.rs`: `--bench` → `ignis_engine::bench::run(&ctx, &scene, &cfg, &mut stdout())`; add `--list-scenes`; update qa_run.py (R5).
4. Examples `showcase_test`, `campaign_check` switch their own device setup to `GpuContext::headless()` (optional).
   ✔ Verify: run full `tools/qa.sh` (not fast) and diff the report against the baseline. Numbers must match exactly for deterministic
   scenes, and the step must not be marked failed on GPU nondeterminism.

**Step 8: split gpu/mod.rs internally (60 min, engine-only).**
Cut `gpu/mod.rs` into `types.rs`, `context.rs`, `buffers.rs` (`SimBuffers` + `storage()`), `pipelines.rs` (`SimPipelines::new(device, target, &SimBuffers)`:
shader modules, bind group layouts, step/vis/move/init/ops/stats pipelines), `schedule.rs` (`fn encode_frame(&mut GpuSim, …)`: the
pass order exactly as ARCHITECTURE.md "Pass order per frame"), and `readback.rs` (`ProbeState`, async probe map, `read_*`). `GpuSim`
keeps the same public methods and holds `buffers: SimBuffers, pipes: SimPipelines, readback: Readback, vfx: Vfx`. Add
`pub const STEP_KERNELS / VIS_KERNELS` and switch `gpu_profile.rs` from text parsing to them (R3). Add the `Send + Sync` compile check.
✔ Verify: run benches, run `gpu_profile` once with small `FRAMES` (not a heavy benchmark), and check the per-step time is within noise of the baseline.

**Step 9: remove shims, real import paths, lib target (40 min).**
1. Add `[lib]` to G and create `lib.rs` with the game `pub mod …` lines (moved from main.rs). `main.rs` becomes `use ignis::app;` + CLI.
2. Mechanical rewrite in G (macOS sed):
   ```bash
   cd crates/ignis/src
   sed -i '' -E 's/crate::(gpu|scene|elements|audio)::/ignis_engine::\1::/g; s/use crate::(gpu|scene|elements|audio)\b/use ignis_engine::\1/g' $(grep -rlE 'crate::(gpu|scene|elements|audio)' .)
   ```
   Then remove the `pub use ignis_engine::…` shims from main.rs/lib.rs. Do it the other way round too: anything that fails to resolve was
   using a shim you missed.
3. Examples in G: replace the remaining `#[path]` includes of game modules with `use ignis::{campaign, codex, library, settings, tutorial, showcase, missions, campaign_bridge};`.
   Some game modules may need `pub` items that were private, which is fine inside the game crate: make them `pub`, never `pub(crate)`, if an example uses them.
4. `grep -rn '#\[path' crates/*/examples` must print nothing, and `grep -rn 'crate::\(gpu\|scene\|elements\|audio\)' crates/ignis` must also print nothing.
   ✔ Verify: run the full checklist (§9).

**Step 10: docs (20 min).** Update ARCHITECTURE.md "Files" (new paths), "Rust API" (`ignis_engine::…`) and "Testing"
(`./target/release/ignis --bench …`, `cargo run -p ignis_engine --example gpu_profile`). Add a "Crate boundary" rule to the ownership
section: the phys/chem/vfx agents own `crates/ignis_engine`, the ui agent owns `crates/ignis`, and an API change needs an entry in §2 of this file.

**Total for steps 0-10: about 6.5 h of focused agent time.** The longest risk-heavy steps are 2 and 7.

---

## 6. Follow-up: element tables as data (not part of the split; do after step 10)

Today, adding an element means editing every `array<_, N_ELEM>` in matter.wgsl (generated between `// GEN-BEGIN/GEN-END`), the
`M_*` arrays in sim.wgsl, `MATTER`/`MATTER_INFO` in elements.rs, and the `mat_of` clamp. Goal: **a new element is one data entry and
needs no WGSL edit.**

**Data file.** `crates/ignis_engine/data/elements.ron` (or `.json` if we'd rather not add serde/ron; `campaign.rs` already has a JSON parser
that could move to `E/src/json.rs`). One record per id that holds all fields of today's tables:
```ron
(id: 19, code: "WATR", name: "Water", cat: Liquids, rgb: (40, 90, 200),
 phase: Liquid, density: 1000.0, t_init: 1.0, t_boil: 1.243, latent: ..., vap_slot: Products, cap: ..., cond: ...,
 ign_air: None, transition: Some((t: 0.91, to: "ICE", latent_fusion: ...)), blurb: "...", fact: "...", tags: "...")
```
Walls (0-9) go in `materials.ron` with `M_STR, M_TSOFT, M_TMELT, M_COND, M_CAP, M_COOL, M_SHOCK, M_COL` + `WallSi`. Reaction pairs
(`pair_params`) go in `reactions.ron`. Cross-references use element **codes**, not ids, and ids are assigned by load order with an explicit `id`
override to keep saves stable.

**Phase A: build-time codegen (zero runtime risk, about 1 day).** `E/build.rs` parses the data files and writes
`$OUT_DIR/elements_gen.wgsl` (the exact `const E_*: array<…, N_ELEM> = …;` text that sits between the GEN markers today) and
`$OUT_DIR/registry_gen.rs` (`MATTER`, `MATTER_INFO`, `WALL_SI`, `N_ELEM`). `shaders::compute_source()` becomes
`format!("{SIM}\n{ELEM_GEN}\n{MATTER}\n{VIS}")`. Delete the GEN block from matter.wgsl. Verify with a byte diff that the generated
WGSL equals the old GEN block, then bench.

**Phase B: runtime GPU arrays (new elements without recompiling, about 2 days).**
- The compute bind group is **at the 8-storage-buffer limit** (ARCHITECTURE.md), so the tables go in a **uniform buffer**
  (does not count against storage limits): `struct ElemProps { a: vec4<f32>, b: vec4<f32>, …(8 vec4) }`,
  `@group(0) @binding(9) var<uniform> ELEM: array<ElemProps, 256>;`. That is 256 × 128 B = 32 KiB, under the 64 KiB default
  `max_uniform_buffer_binding_size`. 256 matches the 8-bit material field in META. Pair reactions go into a second uniform
  `array<vec4<u32>, 1024>` (lookup by `(a,b)` hash or a 64×64 index table packed as u8) or a texture if they outgrow it.
- Accessors: `fn e_dens(id: u32) -> f32 { return ELEM[id].a.y; }` and so on. Phase A already routed all code through `m_cap/m_cond/m_col/m_phase/elem_of`-style
  helpers, so only the helper bodies change.
- `GpuSim::new(device, target)` gains `GpuSim::with_registry(device, target, &Registry)`. `Registry::load_default()` reads the embedded
  RON, and `Registry::load(path)` takes a user mod directory. `queue.write_buffer` uploads the table at init and on hot-reload.
- Performance gate: constant arrays vs uniform loads differ per GPU. Run the `rocket`/`chem` bench (STEPS=40, 200 frames) before
  and after. Per-step time must stay ≤ +3 % (baseline 0.46 ms/step on M2 Pro), or keep the hottest few fields as constants.
- The UI registry (`registry::all()`) reads the same `Registry`, so names, colors, categories and blurbs come from one file.

---

## 7. Versioned save format (engine `save`)

Today: `CMB1` magic, header, `IGMD` chunk with `CHUNK_VERSION = 1`, meta as raw u32 tile/mat/dir, params `[f32; 4]`, zlib, PNG thumb.

Plan (do as part of step 4 or right after it; it is additive and old files keep loading):
- `pub const FORMAT_VERSION: u32 = 2;`. Writers emit v2 and readers accept v1 and v2. `decode_cmb` dispatches on the chunk version, and
  `fn migrate_v1(d: DesignV1) -> Design` upgrades in memory.
- v2 adds the chunks `ELEM` (id → element **code** table at save time, so reordering ids in the data files remaps cleanly on load),
  `ENGV` (engine version string, informational) and optionally `GAS0` (u0 snapshot, zlib, for "save with gas state").
  Unknown chunks are skipped (length-prefixed), so later versions stay readable by older builds where possible.
- Grid size stays in the header. On load, a mismatch with `consts::{W,H}` returns `Err` (or later, a resample).
- Tests: golden v1 file decodes, v2 round-trips, an id remap test (swap two ids in a test registry, load, compare codes).

---

## 8. Ownership after the split

| Crate / path | Owner (from ARCHITECTURE.md roles) |
|---|---|
| `crates/ignis_engine/shaders/sim.wgsl`, `gpu/{buffers,pipelines,schedule,readback,context}.rs`, `bench.rs` | phys |
| `crates/ignis_engine/shaders/matter.wgsl`, `registry.rs`, `scene/`, `data/` | chem |
| `crates/ignis_engine/shaders/{vis,fx,particles,detail,render}.wgsl`, `gpu/vfx.rs` | vfx |
| `crates/ignis_engine/src/{save,probe,audio}`, `lib.rs`, the public API in §2 | lead (API changes need sign-off) |
| `crates/ignis/**` | ui |

---

## 9. Verification checklist (run after steps 2, 3, 4, 7, 9 in full; after the others in short form)

Short form: items 1, 2 and 4 with `rocket` only.

1. `cargo build --workspace --release` with no errors. Warnings count ≤ baseline.
2. `cargo build --workspace --release --examples` and `cargo test --workspace` (codec round trip, golden save, wgsl parse test, settings/codex tests).
3. `cargo tree -d | grep -i wgpu` prints nothing.
4. Bench smoke: `for s in jet rocket vessels flare flame fireball campfire srm demo chem melt empty; do ./target/release/ignis --bench 150 $s; done`
   shows no `bad` cells, rocket pmax ~13-22 atm, Tmax 12, burn ~1.3, and srm pmax rising to ~15 (ARCHITECTURE.md expectations).
5. `tools/qa.sh` (full) produces a report whose pass/fail table matches the baseline report from step 0.
6. Bench screenshot: `./target/release/ignis --bench 150 rocket <scratch>/r.ppm 0`, then `sips -s format png`, then compare visually to the baseline image.
7. App screenshot: `COMBUSTOR_SHOT=<scratch>/app.ppm SHOT_SCENE=camp SHOT_FRAMES=240 ./target/release/ignis`, plus `SHOT_DRAWER=1` and
   `SHOT_SCREEN=missions`. The window opens, closes itself, and the image matches the baseline (UI, element colors, fire).
8. Save/load: save a design from the app, reopen it, and check that a pre-split `designs/*.cmb` still loads.
9. Bundle: `tools/bundle_macos.sh --no-build`, then `dist/Ignis.app/Contents/MacOS/Ignis` exists, `plutil -lint` passes, the Info.plist version is `0.2.0`,
   and `open dist/Ignis.app` launches (close it afterwards).
10. Profile tool: `ONLY=gpu FRAMES=20 tools/profile.sh` runs (short; it's a smoke test, not a benchmark).
11. Hygiene: `grep -rn '#\[path' crates/*/examples`, `grep -rn 'include_str!("\.\./' crates`, and `grep -rn combustor tools/*.sh tools/*.py`
    (only env var names like `COMBUSTOR_SHOT` may remain) are all empty. `crates/ignis_engine` has no `egui|eframe|rfd` in its Cargo.toml or sources.
12. No leftover processes (`pgrep -f target/release/ignis` is empty).

## 10. Effort summary

| Step | What | Effort | Risk |
|---|---|---|---|
| 0 | freeze, backup, baseline, inventory | 15 min (+ qa run time) | low |
| 1 | workspace skeleton, move src/examples into crates/ignis, asset macro, tool paths | 20 min | medium (R2, R4-R7) |
| 2 | GPU core + shaders → engine, shaders.rs, wgpu dep | 45 min | **high** (R1, R3, R8) |
| 3 | scene + registry → engine, Rgb8 | 40 min | medium (R9) |
| 4 | save format split out of library.rs, golden test | 45 min | medium |
| 5 | audio → engine (feature) | 15 min | low |
| 6 | probe decode → engine | 20 min | low |
| 7 | bench API + GpuContext + --list-scenes, qa_run.py | 45 min | **high** (R5, output format) |
| 8 | gpu/mod.rs internal split, gpu_profile constants | 60 min | medium |
| 9 | remove shims, lib target, examples to crate imports | 40 min | medium |
| 10 | ARCHITECTURE.md + ownership | 20 min | low |
| — | **Split total** | **≈ 6.5 h** | |
| 6A | element data → build-time WGSL/Rust codegen | ≈ 1 day | medium |
| 6B | runtime uniform element tables + Registry loading | ≈ 2 days | high (perf gate) |
| 7 | save format v2 (ELEM/ENGV chunks, migration) | ≈ 0.5 day | low |
