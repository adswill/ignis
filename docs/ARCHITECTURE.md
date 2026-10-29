# Ignis architecture

Ignis is a 2D combustion and chemistry simulator. It is written in Rust and runs on wgpu (Metal on macOS) with an egui
(eframe) front end. This document describes the data layouts, pass order, units, id tables and shader map. Keep it
current when you change a layout.

The simulation grid is **W x H = 512 x 288** cells (about 1 cm per cell, `src/gpu.rs`).

## Units

- Ambient air: rho = 1, p = 1 (1 atm), T = 1 (300 K), dx = 1. Temperatures in shaders are multiples of 300 K.
- Fixed time step `DT = 0.028`. The app advances `TPS = 32` simulation time units per real second at 1x speed
  (roughly 19 solver steps per 60 Hz frame, capped at `MAX_STEPS = 64`, see `src/app.rs`).
- The solver uses a reduced speed of sound so that flames, buoyancy and mixing evolve at watchable speeds while pressure
  waves, compression and nozzle flow remain physical. The solver therefore runs far slower than real time by design.
- The UI converts to SI for display (`ui::workspace::Units`: K / C / F, kPa / bar / atm / psi, m/s / km/h / ft/s,
  mm / cm / m).

## Design pillars

1. **Physical fidelity.** Use real-world behaviour and real numbers wherever possible (temperatures, densities,
   ignition points, burn rates, phase changes, pressures). When a shortcut is unavoidable, keep the qualitative
   behaviour faithful and document it. Validation notes live in `tools/VALIDATION.md`.
2. **Clear, convincing visuals.** Smooth volumetric fire, smoke and explosions rather than pixelated gas, with lighting,
   bloom and heat shimmer. If something is correct but looks wrong, it is not done.
3. **A scientific tool, not a toy.** Sober, dense, precise interface: docked workspace, units everywhere, probes and
   plots, CSV export, objective-based experiments instead of gamification.

## Source map

| File | Contents |
|---|---|
| `src/sim.wgsl` | `Params` / `Op` structs, compute bindings, wall-material tables (`M_*`), gas equation of state, HLLC + MUSCL fluxes, RK2 (`rk1`, `rk2`), LES turbulence, `post` (inlets, fans, igniters, buoyancy, diffusion, combustion, soot, matter emission absorption), `wall` / `wall_m` (heat, stress, damage, burnables), `commit` (state conversions), `act` (active-tile mask), `init`, `apply_ops`, `stats` |
| `src/matter.wgsl` | Concatenated after `sim.wgsl`. Powder and liquid element tables (`E_*`, generated block between `GEN-BEGIN` / `GEN-END`), reaction pairs, `matter_update` (boiling, reactions, decomposition, detonation flags), `absorb_matter_emission` (gas side), cast-grain burning, Margolus movement `matter_move_a` / `matter_move_b` |
| `src/vis.wgsl` | Concatenated after `matter.wgsl` (same module and bindings). `colorize` (per-view colours), tight bloom (`blurh`, `blurv`), quarter-resolution wide bloom (`bdown`, `bwideh`, `bwidev`) |
| `src/matter_look.wgsl` | Concatenated last: appearance helpers for matter cells |
| `src/fx.wgsl` | Separate module with its own bind group. The `fx` kernel packs smooth cell-resolution HDR textures and advects detail UVs |
| `src/detail.wgsl` | Render-only up-resolution detail layer (driven from `src/vfx.rs`, own bind group). Flame luminance, soot opacity, cloud opacity and haze are advected by the coarse velocity plus curl-noise turbulence, re-anchored to the coarse fields every frame so no smoke appears where the physics has none. Resolution factor `VFX_DS` (1 to 4; default 4, i.e. 2048 x 1152) |
| `src/render.wgsl` | Full-screen triangle `vs` / `fs`: camera, heat shimmer, bloom, tone mapping, volume fill |
| `src/particles.wgsl` | Gas tracer particles and trails (view 6). Not wired into the pipeline at present |
| `src/acoustics.rs`, `src/acoustics.wgsl` | Acoustics engine (microphone probe, audio synthesis input). Not wired into `gpu.rs` / `app.rs` at present; exercised by `examples/acoustics_render.rs` |
| `src/gpu.rs` | Buffers, pipelines, `GpuSim::frame` (dispatch), `draw`, readbacks; `FrameInput`, `Op`, `Stats`, `ProbeData` |
| `src/vfx.rs` | Render-side pipelines for `fx.wgsl` and `detail.wgsl` |
| `src/app.rs`, `src/ui/` | egui workspace: state, simulation driving, experiments, editing tools, docks, probes |
| `src/elements.rs` | UI catalogue: `MATS`, `ELEMENTS`, `MATTER`, `MATTER_INFO` (SI data sheets), `WALL_SI`, `RECIPES`, `CATS` |
| `src/scene.rs` | Tile / material constants and preset scenes (`Scene::by_name`, `SHOWCASE`) |
| `src/missions.rs`, `src/campaign.rs`, `src/campaign_bridge.rs` | Experiments (lab exercises): layouts, reference solutions, objectives, metrics |
| `src/library.rs` | Design library and the `.cmb` file format |
| `src/codex.rs`, `src/tutorial.rs`, `src/settings.rs`, `src/showcase.rs` | Reference manual, guided tour, preferences and keymap, template scenes |
| `src/audio.rs` | Audio output (cpal) |
| `src/main.rs` | Entry point and `--bench` headless runner |

`sim.wgsl`, `matter.wgsl`, `vis.wgsl` and `matter_look.wgsl` are concatenated into one WGSL module in `gpu.rs`, so
names must be unique across them. Lookup tables are written as `var<private>` in the sources and rewritten to `const`
when the module is built (see "Shader rules").

## GPU buffers (`gpu.rs`)

N = W x H. Every state buffer stores **2 x vec4<f32> per cell** (indices `2i` and `2i+1`).

| Buffer | Size | Layout |
|---|---|---|
| `params` (uniform) | 80 B | `Params` (below) |
| `ops` | 256 x 48 B | `Op[]`, `MAX_OPS = 256` |
| `u0` | N x 32 B | **Gas state.** `[2i]` = species densities (x fuel, y O2, z N2, w products); `[2i+1]` = (mx, my, total energy E, soot density). Solid cells hold ambient air |
| `u1`, `u2` | N x 32 B each | Runge-Kutta stage scratch during steps. After the steps `vis.wgsl` reuses them: `U2[2i]` = base colour (a = smoke opacity, or remaining fraction for walls), `U2[2i+1]` = emitted light, linear HDR (a = temperature); `U1[2i+1]` = tight bloom; `U1[2(2NQ+j)]` = wide bloom (quarter resolution, NQ = ceil(W/4) x ceil(H/4)) |
| `meta` | N x 4 B | u32 per cell: bits 0-7 tile, bits 8-15 material / element id, bits 16-23 direction (fans) or flags (bit 0 on matter = cast / pressed grain) |
| `wall` | N x 32 B | Two halves: `[0, N)` current, `[N, 2N)` next. The next half is written for tiles 1 and 8 only and copied back by `commit`; the next half of other tiles is stale and must not be read. vec4 per cell, meaning depends on the tile (below) |
| `flux` | ((W+1)H + W(H+1)) x 32 B | x-faces then y-faces, 2 vec4 per face |
| `aux` | per RK tile | Sparse stepping bookkeeping: calm flags written by `post`, active flags written by `act`, solid bits (1 = has wall, 2 = has matter) |
| `stats_buf` | small | Atomic u32 slots: 0 max pressure, 1 max gas T, 2 max wall T / T_melt (f32 bits), 3 burn sum (i32 x 1000), 4 wall count, 5 max stress (f32 bits), 6 / 7 thrust x / y (i32 x 1000); slots 8-11 hold cumulative conservation accounting |
| `probe_buf` | 128 B, MAP_READ | Copy of one cell's `u0`, `meta`, `wall` plus the stats slots, read back one frame late |
| `fxparams` (uniform) | 32 B | `FxParams` |
| fx UV | N x 32 B | Ping-pong detail displacement (layer 0 xy, layer 1 zw) |
| fx textures A-D | W x H Rgba16Float | A: gas emission rgb, a = T. B: cloud premultiplied albedo rgb, a = opacity. C: r liquid, g matter, a solid mask. D: detail UV displacements |

Bind groups:

- Compute (sim + matter + vis module): 0 `Params`, 1 `U0`, 2 `U1`, 3 `U2`, 4 `META`, 5 `WALL`, 6 `OPS` (read-only),
  7 `AUX`, 8 `STATS`. This is the 8-storage-buffer-per-stage limit, so new GPU features need their own bind group
  (as `fx` and the detail layer do).
- Render: `Params`, `U1`, `U2`, `META` (read-only storage), fx textures A-D, linear sampler, `FxParams`.
- Fx: `Params`, `FxParams`, `U0`, `META`, `U2` (read-only), fx UV buffer (read-write), storage textures A-D (write).

## Tiles (META bits 0-7)

| id | tile | solid | notes |
|---|---|---|---|
| 0 | gas | no | |
| 1 | wall | yes | material in bits 8-15 (ids 0-9) |
| 2 | fuel inlet | no | held at `p_fuel`, pure fuel |
| 3 | O2 inlet | no | held at `p_o2` |
| 4 | air inlet | no | held at `p_air` |
| 5 | fan | no | pushes gas along its direction (0 +x, 1 +y (down), 2 -x, 3 -y (up)) with `fan_pow` |
| 6 | igniter | yes | heats neighbouring gas to `T_IGNITER` while `ign_on`; heats touching solids |
| 7 | vent | no | held at ambient air, 1 atm |
| 8 | matter | yes (for the gas) | powder, liquid or static solid; element id (10 and above) in bits 8-15 |

### Wall materials (ids 0-9)

| id | material | id | material |
|---|---|---|---|
| 0 | steel | 5 | ceramic (thermal shock) |
| 1 | aluminium | 6 | regeneratively cooled copper |
| 2 | copper | 7 | wood (pyrolyses to fuel gas, chars to ash) |
| 3 | titanium | 8 | solid propellant (Saint-Robert burn rate, self-oxidising) |
| 4 | Inconel | 9 | explosive (detonates) |

Per-id wall property tables (length 26) live in `sim.wgsl`: `M_STR`, `M_TSOFT`, `M_TMELT`, `M_COND`, `M_CAP`, `M_COOL`,
`M_SHOCK`, `M_COL`. SI data sheets for the UI are in `elements::WALL_SI`.

### Matter elements (ids 10-91)

`N_ELEM` in `matter.wgsl` is the length of every per-element table (currently 92). Ids 10-25 are the original set; ids
26-65 and 66-91 were added later. Ids marked (U) are placeable from the palette (`elements::MATTER`).

| ids | elements |
|---|---|
| 10-25 | sand, charcoal, sulfur, saltpeter (KNO3), sugar, aluminium powder, ammonium perchlorate, iron oxide, ammonium nitrate, water, kerosene, ethanol, liquid oxygen, nitroglycerin, molten metal, ash / slag |
| 26-45 | ice, snow, salt, brine, liquid nitrogen, gasoline, diesel, cooking oil, napalm, wax, molten wax, coal, paper, magnesium, iron filings, sodium, potassium, lye, hydrochloric acid, hydrogen peroxide (90 %) |
| 46-65 | manganese dioxide, potassium perchlorate, nitrocellulose, TNT, C4, dynamite, blasting cap, glass, molten glass, lava, stone, gunpowder, dry ice, copper oxide, potassium permanganate, glycerin, plastic, molten plastic, flour, fuse |
| 66-91 | HTPB binder, epoxy binder, copper chromite, potassium chlorate, sodium nitrate, titanium powder, zinc powder, red phosphorus, sorbitol, dextrose, rubber, cotton, coal dust, sawdust, acetone, methanol, nitric acid, hydrazine, N2O4, tungsten, graphite, phenolic ablative, phenolic char, fibreglass, firebrick, aerogel |

The palette in `elements.rs` (`MATTER`, 56 entries) exposes ids 10-65. Ids 66-91 have physics tables in `matter.wgsl`
but are not all surfaced in the palette. Check `elements.rs` before assuming an id is placeable.

Phase (`E_PHASE`): 0 static solid (placeable, immobile, still reactive: ice, wax, paper, explosives, glass, stone,
plastic, fuse), 1 powder, 2 liquid.

**Adding an element** means: extend every `N_ELEM`-length table in `matter.wgsl` (the generated block between
`GEN-BEGIN` and `GEN-END`: phase, density, initial T, boil / decomposition T, latent heat, vapour slot, soot, volatility,
air ignition / burn T and rate, dust and suspension factors, viscosity, phase-transition data, burn-out residue,
self-reaction, detonation thresholds, `E_CAP`, `E_COND`, `E_COL`), add `MATTER` and `MATTER_INFO` entries in
`elements.rs`, and add recipes if appropriate. The hooks `elem_of(i)`, `m_cap(m)`, `m_cond(m)`, `m_col(m)` and
`m_phase(m)` return wall-table values for ids below 10 and element-table values otherwise.

## WALL vec4 per tile

- **Walls (tile 1):** (T, damage (>= 1 fails at commit), stress ratio, remaining solid fraction for burnables 7-9).
- **Matter (tile 8):** (T, flag, emission code, remaining fraction).
  - Emission code = `kind * 1000 + rate`, read by neighbouring gas cells in `post`. Kinds: 1 vapour, 2 O2 release,
    3 hot products and smoke, 4 surface burning in air, 5 hydrogen, 6 peroxide (O2 + steam), 7 inert steam / CO2.
  - `.y` flags handled in `commit` (acts on values >= 1): 1.5 consumed to gas, 1.6 burst into hot product gas (4 atm),
    1.7 molten iron, 1.8 hot slag / ash, 2.0 detonation (60 atm, 3450 K), 3.0 molten metal solidifies into a wall.
    Values below 1 describe settled state and phase-change progress; `0.55 + id/1000` is a conversion request applied by
    `apply_conversion` at the start of the movement pass (water <-> ice, sand -> glass, lava <-> stone, salt + water ->
    brine, and so on). A request of 0.94 on a cast grain marks a cracked grain.
  - Molten metal keeps its original wall material in `.z` (`mat + 0.5`).
- Other tiles: (1, 0, 0, 1).

### Gas species mapping

The gas state has four species plus soot. The fuel slot x holds methane-like fuel, hydrogen, fuel vapours and pyrolysis
gas; y holds O2 (including liquid-oxygen boil-off, oxidiser decomposition and part of peroxide decomposition); z is
inert N2 (including liquid-nitrogen boil-off); w holds products (CO2, steam, HCl fumes, reaction products). Soot is
smoke.

## Params (uniform; `gpu.rs` `Params` mirrors WGSL `Params`)

```
w, h, nops, view
dt, p_fuel, fan_pow, time
ign_on, frame, srgb, sflags      (sflags bit 0: sparse active-tile stepping)
p_o2, p_air, cam_x, cam_y
cam_w, cam_h, wall_dt, gravity
```

`cam_*` is the visible region in cells; `wall_dt = DT * WALL_EVERY`. Views: 0 Fire, 1 Pressure, 2 Heat, 3 Flow,
4 Gases, 5 Stress (6 = Fire plus particle trails, unused).

## Op (user edits, 48 B)

`kind, shape, x0, y0, x1, y1, r, tile, mat, dir, _p0, _p1`

- `shape`: 0 disc (x0, y0, r), 1 filled box, 2 hollow box (thickness r + 1), 3 line (half-width r).
- `kind`: 0 place tile (tile 8 places matter, only into empty gas; `_p0` / `_p1` select a second element and its
  fraction x 1000 for mixed compositions), 1 erase, 2 add fuel, 3 add O2, 4 add air, 5 flame / torch (heats gas; on a wall
  heats the surface), 6 chill, 7 vacuum, 8 stir (consumed by the matter movement pass; ignored by `apply_ops`),
  9 cast / press (flags matter cells as a packed grain).

## Rust API (`gpu.rs`)

- `FrameInput { steps, ops, view, p_fuel, p_o2, p_air, fan_pow, gravity, cam, ign_on, time, frame, scene, meta_only, reset_gas, probe }`
  (`scene` carries a new layout plus gas reset; `meta_only` replaces the layout and keeps the gas).
- `Stats { max_p, max_t, max_wall_heat, burn, walls, max_stress, thrust[2] }` arrives one frame late through the probe
  readback; the app converts units.
- `ProbeData { u[8], meta, wall[4] }`.
- `GpuSim::new(device, target_fmt)`, `frame(device, queue, enc, &FrameInput)`, `draw(&mut RenderPass)`, and blocking
  `read_meta`, `read_wall`, `read_state` (headless runs, undo, probes).
- `app.rs` calls `frame` from the egui_wgpu `CallbackTrait::prepare` and `draw` from `paint`. Because egui may discard a
  pass, the one-shot parts of an input (scene, meta, reset, ops) that never reached `prepare` are carried into the next
  frame.

## Pass order per frame (`GpuSim::frame`)

1. Finish any pending asynchronous probe / stats readback.
2. Write `Params`, ops and meta (scene / meta-only loads). Anything that edits state outside the solver clears the calm
   flags so every tile steps once.
3. Compute pass "sim": `init` (on scene load or gas reset), then `apply_ops` (if there are edits), then for each step:
   - `act` builds the active-tile mask from the previous step's calm flags;
   - `rk1`, `rk2`, `post` (fluxes are fused into the RK kernels; LES viscosity is fused into `post`);
   - every `WALL_EVERY = 4` steps additionally `wall` (tile 1), `wall_m` (tile 8, calls `matter_update`), `commit`
     (copies `WALL` next to current for tiles 1 and 8, then applies conversions), then `matter_move_a` or `matter_move_b`
     (alternating Margolus block offset, dispatched over (W/2+1) x (H/2+1)).
4. Clear the per-frame stats slots; compute pass "vis": `stats`, `colorize`, `blurh`, `blurv`, `bdown`, `bwideh`, `bwidev`.
5. Render-side passes from `vfx.rs`: `fx` (`fx.wgsl`) and the detail layer (`detail.wgsl`).
6. Probe copy (one cell of `u0` / `meta` / `wall` plus stats into `probe_buf`).
7. The caller's render pass calls `draw` (`render.wgsl`).

Sparse stepping skips tiles that are calm (every cell changed by less than a threshold, gas only, below the chemistry
temperature floor) together with their one-tile halo. Set `IGNIS_DENSE=1` to disable it for A/B comparison.

## Physics summary

- Compressible multi-species Euler equations: HLLC fluxes with MUSCL reconstruction, two-stage Runge-Kutta, per-species
  heat capacities (mixture gamma follows composition).
- Combustion: one-step Arrhenius with a high activation temperature plus a mixing-controlled (eddy dissipation) rate;
  heat of combustion and activation constants are calibrated against methane/air references (flame temperature,
  flammability limit, laminar flame speed, autoignition temperature; see `tools/VALIDATION.md`).
- Smagorinsky large-eddy turbulence, buoyancy, wall friction, soot formation / oxidation / radiation.
- Walls: gas-wall convection and radiation (`gas_wall_flux`, conservative on both sides), conduction, softening,
  stress-driven bursting, melting, thermal shock, regenerative cooling.
- Matter: Margolus-neighbourhood cellular automaton for powders and liquids (mass-conserving swaps, density layering,
  angle of repose, viscosity), phase changes, pair reactions (rocket candy, black powder, thermite, ANFO, and others),
  dust suspension, detonation thresholds, and cast / pressed propellant grains that burn only at gas-exposed surfaces
  with a Saint-Robert-style rate `r = a p^n`.

## Shader rules

- Never declare a mutable `var<private>` at module scope in `sim.wgsl`, `matter.wgsl` or `vis.wgsl`. `gpu.rs` rewrites
  `\nvar<private> ` to `\nconst ` when it builds the module (copying private tables into per-thread memory made the
  matter kernels roughly ten times slower on Metal). Use `const` for lookup tables.
- `cargo build` does not validate WGSL. Shader errors surface at pipeline creation, so run a short bench after every
  shader edit. Reserved words (for example `of`) are rejected at runtime.
- `textureSample` needs uniform control flow; use `textureSampleLevel(..., 0.0)` inside branches.
- Do not read the `WALL` next half for tiles other than 1 and 8.

## Colour maps (analytic views, `vis.wgsl` `colorize`)

- Pressure (1): q = (p - 1 atm) / 1.5 atm. Below ambient is blue; above ambient runs black to dark orange (+1.5 atm), to
  yellow (+3 atm), to white (+3.75 atm and above).
- Heat (2): 90 K to 3000 K, linear over eight stops (black, deep blue, blue, cyan, green, yellow, red, white).
- Flow (3): hue = direction, value = speed (saturates at 2.5 cells per time unit).
- Gases (4): fuel yellow, O2 cyan-blue, products white, soot dark grey.
- Stress (5): walls only; stress ratio 0 to 0.5 green to yellow, 0.5 to 1 yellow to red, above 1 flashing magenta.

## UI and workspace

The app starts directly in a workspace with the rocket engine setup loaded.

- `src/ui/theme.rs`: palette (panel `#1b1c1f`, field `#232428`, border `#2c2e33`, one accent `#528ee6`, semantic
  good / warn / bad), Helvetica Neue and SF Mono, 4 / 8 px spacing.
- `src/ui/workspace.rs`: menu bar, icon toolbar, left dock (searchable materials library by category), right dock
  (experiment panel, material data sheet, boundary conditions, cell at cursor, domain extrema), bottom dock (plots, probes,
  line profiles, console, reference, experiments), status bar (state, time, steps per frame, dt, CFL, percent real time,
  mass / energy drift, GPU time), viewport header with field selector, cursor readout, axes, scale bar and colour-map
  legends.
- `src/ui/probes.rs`: point probes and line samplers (blocking `read_state` at 2 Hz only while instruments exist),
  diagnostics, CSV export. `src/ui/propellant.rs`: propellant designer.
- `settings`: preferences and keymap (`~/Library/Application Support/Ignis/settings.json`). `library`: saved designs
  (`~/Library/Application Support/Ignis/designs/`, override with `IGNIS_LIBRARY_DIR`). `codex`: reference manual.
  `tutorial`: guided tour. Experiment progress is stored under `~/Library/Application Support/Ignis/`.
- Shift + scroll over the viewport resizes the brush; Cmd + scroll or pinch zooms; plain scroll pans when zoomed.

### Experiments

Experiments are the campaign missions (`campaign_bridge::all_missions()`) followed by the hand-written `MISSIONS` in
`missions.rs`. Each is data: layout function, known-good solution, palette, budgets, feeds and objectives
(`Goal::{Reach, ReachBy, Hold, Keep, AtMost, HoldBelow}` over `Metric`). The UI shows measured values and met / not met /
pending status. Thrust is tared during setup.

## Testing

```bash
cargo build --release
./target/release/ignis --bench <frames> <scene> [out.ppm] [view]
```

Bench scenes: `jet`, `rocket`, `vessels`, `flare`, `flame`, `fireball`, `campfire`, `srm`, `demo`, `chem`, `melt`,
`empty`, plus every `SHOWCASE` name in `scene.rs` (`kitchen`, `sodium`, `flash`, `thermite`, `silo`, `explosives`,
`peroxide`, `volcano`, `cryo`, `fuels`, `oxidisers`, `acids`).

Bench environment variables: `STEPS=n` (solver steps per frame, default 20), `EVERY=n` (print interval, default 25),
`WCELLS="x:y,.."` (dump wall / matter cells), `TILES=1` (tile counts), `DUMP=1 CELLS="x:y,.."` (gas cells), `GRAV`, `FAN`,
`IGN=0`, `SAVE=path` (write final raw state for build-to-build comparison), `SHOT_STEPS` (steps for the screenshot frame).
The screenshot is 1536 x 864.

Convert and view a screenshot: `sips -s format png a.ppm --out a.png`.

Real-application screenshot (the app closes itself after `SHOT_FRAMES` frames, default 240):

```bash
COMBUSTOR_SHOT=/abs/path.ppm SHOT_SCENE=<template key or title prefix> SHOT_VIEW=<0..6> ./target/release/ignis
```

Further hooks: `SHOT_SPEED`, `SHOT_FRAMES`, `SHOT_MISSION=n [SHOT_SOLVE[=bare]]`, `SHOT_LOG=1`,
`SHOT_TAB=plots|probes|profiles|console|reference|experiments`,
`SHOT_WINDOW=settings|library|codex|experiments|shortcuts|about|tour`, `SHOT_PROBES=1`, `SHOT_UNITS=eng`, `SHOT_DOCKS=0`,
`SHOT_TOOL=<tool key>`, `COMBUSTOR_UNLOCK=1`. See the top of `update()` in `src/app.rs` and `src/ui/workspace.rs` for the
authoritative list. The `COMBUSTOR_*` names are historical and kept for compatibility.

Regression harness: `tools/qa.sh` builds, runs every bench scene with threshold checks and writes a report
(`QA_NO_BUILD=1`, `QA_SCENES="jet rocket"`, `QA_FAST=1`, `QA_OUT=dir`, `QA_REPORT=path` are supported; see the script header).
Example programs (`cargo run --release --example <name>`): `physval` (physical validation), `gpu_profile` (per-kernel
timing), `audio_demo`, `acoustics_render`.

Expected sane bench output (150 frames): no `bad` cells; rocket peak pressure about 13-22 atm; srm pressure rising to
about 15 atm; jet and flare need roughly 100-300 frames before the burn statistic becomes non-zero.

Keep GPU runs short and leave no processes running.

## Performance reference

Apple M2 Pro, headless bench, sparse stepping enabled: roughly 0.3-0.45 ms per solver step including fixed cost, about
2 ms of fixed per-frame cost (stats, vis, bloom, fx) when `STEPS=0`. A 1x frame is about 6-8 ms, so 60 fps holds with
headroom. Anything that adds per-step cost scales by the roughly 19 steps per frame. Use `tools/profile.sh` and
`examples/gpu_profile.rs` for measurements (`tools/PROFILE.md` holds an earlier profiling log).

## Environment variables

| Variable | Effect |
|---|---|
| `IGNIS_DENSE=1` | Disable sparse active-tile stepping |
| `IGNIS_LIBRARY_DIR` | Design library folder |
| `VFX_DS=1..4` | Detail layer resolution factor |
| `VFX_NODETAIL=1` | Disable the detail layer (timing A/B only) |
| `AUDIO_NO_DEVICE`, `AUDIO_VOL`, `AUDIO_WAV` | Audio test and debug switches |
| `COMBUSTOR_SHOT`, `SHOT_*`, `COMBUSTOR_UNLOCK` | Screenshot and test hooks (above) |
