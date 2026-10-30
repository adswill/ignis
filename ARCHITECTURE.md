# Ignis – architecture (authoritative; keep it current when you change a layout)

2D combustion / chemistry sandbox. Rust + wgpu (Metal) + egui (eframe). Grid **W×H = 512×288** cells (~1 cm each).
Units: ambient air rho = 1, p = 1 (1 atm), T = 1 (300 K), dx = 1. Fixed `DT = 0.028`; app runs `TPS = 32` time
units per real second (≈19 sim steps per 60 Hz frame at 1× speed, capped at `MAX_STEPS = 64`).

## Files

| File | Contents |
|---|---|
| `src/sim.wgsl` | Params/Op structs, compute bindings, material tables (`M_*`, 26 entries), gas EOS helpers, HLLC+MUSCL fluxes (`flux0/flux1`), RK2 (`rk1/rk2`), LES (`turb`), `post` (inlets, fans, igniters, buoyancy, diffusion, combustion Arrhenius+EDC, soot, matter emission absorption), `wall` (heat, stress, damage, burnables, calls `matter_update`), `wall_copy`, `commit` (state conversions), `init`, `apply_ops`, `stats` |
| `src/matter.wgsl` | Appended after sim.wgsl. Powder/liquid element tables (`E_*`), `pair_params` (reaction pairs), `matter_update` (boiling, reactions, decomposition, detonation flags), `absorb_matter_emission` (gas side), Margolus movement `matter_move_a/b` |
| `src/vis.wgsl` | Appended last (same module + bindings). `colorize` (per-view colours into U2), tight bloom `blurh/blurv`, quarter-res wide bloom `bdown/bwideh/bwidev` (into U1) |
| `src/fx.wgsl` | Separate module, own bind group. `fx` kernel: packs 4 smooth cell-res HDR textures + advects 2-phase detail UVs |
| `src/detail.wgsl` | Render-only up-res detail sim (vfx.rs, own bind group): DS× grid (default 4 → 2048×1152, env `VFX_DS=1..4`), rgba16f ping-pong: r flame luminance, g soot opacity, b cloud opacity, a haze. Advected by coarse velocity (fx layer 9) + band-limited curl-noise turbulence, re-anchored every frame to the coarse layers and clamped to them (no smoke where the physics has none). Frozen while paused. `VFX_NODETAIL=1` disables it (timing A/B only) |
| `src/render.wgsl` | Full-screen triangle `vs`/`fs`: draws the grid (camera, heat shimmer, bloom, tone map). Gas in the Fire view is sampled from fx textures A/B with bilinear filtering |
| `src/particles.wgsl` | **Not wired up** (Powder-Toy gas tracer particles + trails, view 6). Left from an earlier pass; free for the VFX agent to reuse or delete |
| `src/gpu.rs` | Buffers, pipelines, `GpuSim::frame` (dispatch), `draw`, readbacks; `FrameInput`, `Op`, `Stats`, `ProbeData` |
| `src/app.rs` + `src/ui/` | egui workspace UI (see "UI / workspace"): state, sim driving, experiments, tools, docks, probes |
| `src/missions.rs` | Mission definitions/thresholds |
| `src/elements.rs` | Element/tool catalogue for the UI (`MATS`, `ELEMENTS`, `MATTER`, `EXTRA`, `RECIPES`, `CATS`) |
| `src/scene.rs` | Tile/material constants + preset scenes |
| `src/main.rs` | `--bench` headless runner, app entry |

## GPU buffers (gpu.rs)

N = W·H. Every state buffer stores **2 × vec4<f32> per cell** (index `2i`, `2i+1`).

| Buffer | Size | Layout |
|---|---|---|
| `params` (uniform) | 80 B | `Params` below |
| `ops` | 256 × 48 B | `Op[]`, `MAX_OPS = 256` |
| `u0` | N·32 | **The gas state.** `[2i]` = species densities (x CH4-like fuel, y O2, z N2, w products CO2/H2O); `[2i+1]` = (mx, my, E total energy, soot density). Solid cells hold ambient air |
| `u1`, `u2` | N·32 each | RK stage scratch during steps. After the steps vis.wgsl reuses them: `U2[2i]` = base colour (a = smoke opacity, or remaining fraction for walls), `U2[2i+1]` = emitted light linear HDR (a = temperature); `U1[2i+1]` = tight bloom, `U1[2(2NQ+j)]` = wide bloom (quarter res, NQ = ceil(W/4)·ceil(H/4)) |
| `meta` | N·4 | u32 per cell: bits 0-7 tile, 8-15 material/element id, 16-23 dir (fans) |
| `wall` | N·32 | two halves: `[0,N)` current, `[N,2N)` next (written by `wall`/`wall_m` for tiles 1/8 ONLY, copied back by `commit`; the next half of other tiles is stale — never read it). vec4 per cell, meaning depends on tile (below) |
| `flux` | ((W+1)H + W(H+1))·32 | x-faces then y-faces, 2 vec4 per face |
| `stats_buf` | 64 B | atomic u32: 0 max p, 1 max gas T, 2 max wall T/T_melt (f32 bits), 3 burn sum (i32 ×1000), 4 wall count, 5 max stress (f32 bits), 6/7 thrust x/y (i32 ×1000) |
| `probe_buf` | 128 B MAP_READ | f32[0..8) u0 of probe cell, u32[8] meta, f32[12..16) wall, [16..24) stats copy |
| `fxparams` (uniform) | 32 B | `FxParams {dt, ph0, ph1, reset bits, src, pad×3}` |
| fx uv | N·32 | ping-pong halves of vec4 advected detail displacement (layer0 xy, layer1 zw) |
| fx textures A-D | W×H Rgba16Float | A: gas emission rgb, a = T. B: cloud premultiplied albedo rgb, a = opacity. C: r liquid, g matter, a solid mask. D: detail UV displacements |

Compute bind group (sim+matter+vis module): 0 Params, 1 U0, 2 U1, 3 U2, 4 META, 5 WALL, 6 OPS (ro), 7 FLUX, 8 STATS
— **at the 8-storage-buffer limit**; new GPU features need their own bind group (like fx/particles).
Render bind group: 0 Params, 1 U1, 2 U2, 3 META (ro storage), 4-7 fx textures A-D, 8 linear sampler, 9 FxParams.
Fx bind group: 0 Params, 1 FxParams, 2 U0, 3 META, 4 U2 (ro), 5 uv buf (rw), 6-9 storage textures A-D (write).

## Tiles (META bits 0-7)

| id | tile | solid? | notes |
|---|---|---|---|
| 0 | gas | no | |
| 1 | wall | yes | material in bits 8-15 (0-9) |
| 2 | fuel inlet | no | held at `p_fuel`, pure fuel |
| 3 | O2 inlet | no | held at `p_o2` |
| 4 | air inlet | no | held at `p_air` |
| 5 | fan | no | pushes gas along dir (0 +x, 1 +y(down), 2 −x, 3 −y(up)) with `fan_pow` |
| 6 | igniter | yes | heats neighbouring gas to `T_IGNITER = 10` when `ign_on`; heats touching solids |
| 7 | vent | no | held at ambient air 1 atm |
| 8 | matter | yes (for the gas) | powder/liquid element id 10-25 in bits 8-15 |

Wall materials (ids 0-9): 0 steel, 1 aluminium, 2 copper, 3 titanium, 4 inconel, 5 ceramic (thermal-shock), 6 cooled copper,
7 wood (pyrolyses → fuel gas, chars → ash), 8 solid propellant (Saint-Robert burn, self-oxidising), 9 explosive (detonates).
Matter elements (ids 10-25): 10 sand, 11 charcoal, 12 sulfur, 13 KNO3, 14 sugar, 15 Al powder, 16 ammonium perchlorate,
17 iron oxide, 18 ammonium nitrate, 19 water, 20 kerosene, 21 ethanol, 22 LOX, 23 nitroglycerin, 24 molten metal, 25 ash/slag.
Per-id property tables of length 26 live in sim.wgsl (`M_STR, M_TSOFT, M_TMELT, M_COND, M_CAP, M_COOL, M_SHOCK, M_COL`)
and matter.wgsl (`E_PHASE` 0 static/1 powder/2 liquid, `E_DENS, E_TINIT, E_TBOIL, E_LATENT, E_TIGN_AIR, E_TBURN_AIR`).
**Adding an element = extend every 26-array in both files + elements.rs `MATTER` + `mat_of` clamp (25) in sim.wgsl.**

## WALL vec4 per tile

- **Walls (tile 1)**: (T, damage (≥1 → fails at commit), stress ratio, remaining solid fraction for burnables 7-9).
- **Matter (tile 8)**: (T, flag, emission code, remaining fraction). Emission code = `kind·1000 + rate`
  (kind 1 vapour, 2 O2 release, 3 hot products + smoke, 4 surface burning in air), read by gas neighbours in `post`.
  Flags handled in `commit`: 1.5 consumed → gas from neighbours, 1.6 → hot product gas (4 atm), 1.7 → molten iron,
  1.8 → hot slag/ash, 2.0 → detonation (60 atm, 3450 K), 3.0 → molten metal solidifies back into a wall.
  Molten metal (24) keeps its original wall material in `.z` (`mat + 0.5`).
- Other tiles: (T=1, 0, 0, 1).

## Params (uniform, gpu.rs `Params` = WGSL `Params`)

`w, h, nops, view | dt, p_fuel, fan_pow, time | ign_on, frame, srgb, _p1 | p_o2, p_air, cam_x, cam_y | cam_w, cam_h, wall_dt, gravity`
(`cam` = visible region in cells x,y,w,h; `wall_dt = DT·WALL_EVERY`). Views: 0 Fire, 1 Pressure, 2 Heat, 3 Flow,
4 Gases, 5 Stress (6 = Fire + particle trails, unused now).

## Op (user edits, 48 B)

`kind, shape, x0, y0, x1, y1, r, tile, mat, dir, _p[2]`.
shape: 0 disc (x0,y0,r), 1 filled box, 2 hollow box (thickness r+1), 3 line (half-width r).
kind: 0 place tile (tile 8 = matter, only into empty gas), 1 erase, 2 add fuel, 3 add O2, 4 add air, 5 flame/torch
(heats gas to 9; on a wall heats surface to 4), 6 chill, 7 vacuum.

## Rust API (gpu.rs)

- `FrameInput { steps, ops, view, p_fuel, p_o2, p_air, fan_pow, gravity, cam, ign_on, time, frame, scene: Option<Vec<u32>> (new layout + gas reset), meta_only (layout, gas kept), reset_gas, probe: Option<(x,y)> }`
- `Stats { max_p, max_t, max_wall_heat, burn, walls, max_stress, thrust[2] }` (one frame late, via probe readback; app converts units: `KN_PER_UNIT`, `KGS_PER_UNIT`…)
- `ProbeData { u[8], meta, wall[4] }`
- `GpuSim::new(device, target_fmt)`, `frame(device, queue, enc, &FrameInput)`, `draw(&mut RenderPass)`, blocking `read_meta`, `read_wall`, `read_state` (headless/undo).
- app.rs calls `frame` from the egui_wgpu `CallbackTrait::prepare` and `draw` from `paint`.

## Pass order per frame (`GpuSim::frame`)

1. finish async probe/stats readback.
2. write Params, ops, meta (scene/meta_only).
3. compute pass "sim": `init` (if scene/reset_gas) → `apply_ops` (if ops) → for each of `steps`:
   `rk1, rk2, post` (flux fused into rk, LES ν fused into post), and every `WALL_EVERY = 4`th step also `wall` (tile 1),
   `wall_m` (tile 8 + `matter_update`), `commit` (copies WALL next→current for tiles 1/8, then conversions) then
   `matter_move_a|b` (alternating Margolus offset, dispatch over (W/2+1)×(H/2+1)).
4. clear stats; compute pass "vis": `stats, colorize, blurh, blurv, bdown, bwideh, bwidev`.
5. compute pass "fx": `fx` (fx.wgsl).
6. probe copy (u0/meta/wall cell + stats → probe_buf).
7. (render pass, by caller) `draw`: render.wgsl full-screen triangle.

## Ownership for the next phase (edit only what you own)

- **vfx**: `src/vis.wgsl`, `src/render.wgsl`, `src/particles.wgsl`, `src/fx.wgsl` (and any new `*.wgsl` for rendering), and ONLY the rendering parts of `src/gpu.rs` (render/particle/fx pipelines, render bind groups, `GpuSim::draw`, the vis/fx/particle dispatches at the end of `frame()`). Re-read gpu.rs immediately before every edit and keep edits minimal — the physics agent edits the compute parts of the same file.
- **chem**: `src/matter.wgsl`, `src/elements.rs` (UI element metadata: ids, names, colours, categories, blurbs, recipes), `src/scene.rs`. New matter helpers go inside matter.wgsl only. Need a hook in sim.wgsl? Write the exact request in your report instead of editing sim.wgsl.
- **phys**: `src/sim.wgsl` and ONLY the compute/simulation parts of `src/gpu.rs` (step pipelines, compute bind group, solver buffers, simulation dispatches in `frame()`, `read_*` helpers) and the headless bench code in `src/main.rs`. Re-read gpu.rs immediately before every edit; keep edits minimal.
- **ui**: `src/app.rs`, `src/missions.rs` (and new UI-only Rust modules; register them with a `mod` line in main.rs, nothing else in main.rs). Read `src/elements.rs` but don't edit it — ask the chem agent.

WGSL gotchas: `of` and other reserved words are rejected at runtime (pipeline creation panics, `cargo build` does NOT
catch shader errors — always run a short bench after a shader edit). `textureSample` needs uniform control flow — use
`textureSampleLevel(..., 0.0)` inside branches. sim/matter/vis share one module: no duplicate names across them.

## Testing

```bash
cd "." && cargo build --release
./target/release/ignis --bench <frames> <jet|rocket|vessels|flare|flame|fireball|campfire|srm|demo|chem|melt|empty> [out.ppm] [view]
#   20 sim steps/frame (STEPS=n overrides); prints stats every EVERY=n frames (default 25);
#   WCELLS="x:y,.." dumps wall/matter cells; TILES=1 tile counts; DUMP=1 CELLS="x:y,.." gas cells;
#   GRAV, FAN, IGN=0 env overrides; SHOT_STEPS for the screenshot frame. Screenshot is 3x (1536x864).
sips -s format png a.ppm --out a.png && sips -Z 1200 a.png
# real app screenshot (closes itself after SHOT_FRAMES frames, default 240):
COMBUSTOR_SHOT=/abs/path.ppm SHOT_SCENE=<scenario name prefix, e.g. camp|rocket|chem> SHOT_VIEW=<n> ./target/release/ignis
#   also SHOT_SPEED, SHOT_DRAWER=0/1, SHOT_SCREEN=title|missions, SHOT_MISSION=n [SHOT_SOLVE[=bare]] [SHOT_BRIEF], SHOT_CAT=0..6, SHOT_HELP
```
Use a scratch folder for outputs, keep GPU runs short, leave no processes running.

Expected sane bench numbers (150 frames): no `bad` cells; rocket pmax ~13-22 atm, Tmax 12 (cap), burn ~1.3; srm pmax
rising to ~15; jet/flare need ~100-300 frames before burn > 0 (feeds ramp up / methane must fill the pipe).

## Baseline performance (Apple M2 Pro, 2026-10-28, after the restart fix)

| Measure | Result |
|---|---|
| Sim step (STEPS=40, 200 frames, `empty`) | 3.80 s → **0.46 ms / step** |
| Sim step (STEPS=40, 200 frames, `rocket`) | 3.80 s → **0.46 ms / step** (scene content barely matters) |
| Per-frame overhead with STEPS=0 (stats + vis + bloom + fx, no draw) | 0.14 s / 200 → **~0.7 ms / frame** |
| Default 20 steps/frame | ~9.9 ms / frame (≈100 fps sim ceiling); at 1× speed the app needs ~19 steps/frame ≈ 8.8 ms |

Budget: at 60 Hz there are ~7 ms left per frame for rendering + UI; anything that adds per-step cost scales ×19.

## Product name (decided by the lead)
The product is called **IGNIS** (Latin for "fire"). The crate and binary are `ignis` (renamed by the integrator, wf4): use `target/release/ignis` everywhere. Env hook names (`COMBUSTOR_SHOT`, …) are unchanged for compatibility.

## Design pillars (from the user — every change must serve these)
1. **Hyper-realistic physics & chemistry.** Real-world behaviour and real numbers wherever possible (temperatures, densities, ignition points, burn rates, phase changes, pressures). When a shortcut is unavoidable, keep the qualitative behaviour faithful and document it.
2. **Visually amazing.** Cinematic, smooth, volumetric fire/smoke/explosions (never pixelated gas or fire), beautiful lighting, bloom, heat haze, liquids and materials that look great — "realistic but stylised". If something is correct but looks bad, it isn't done.

## UI direction (from the user — supersedes earlier "game-y" UI goals)
Ignis is a **professional research tool** for combustion/chemistry, not a playful game. "The current UI sucks and is too playful."
- Aesthetic: sober, precise, dense-but-calm. Neutral dark greys (e.g. #1b1c1f panels, #232428 fields, #2c2e33 borders), ONE restrained accent colour, no gradients/glows/emoji/cartoon icons, no celebratory banners or toasts. Small, crisp typography; tabular/monospace numerals; consistent 4/8 px spacing grid; thin 1 px separators.
- Layout: docked workspace like a scientific application — menu bar (File, Edit, View, Simulation, Tools, Window, Help) + compact icon toolbar; left dock = materials/elements library (searchable tree by category with property columns); right dock = Properties/Inspector (selected element/material data sheet with SI units, probe readings); bottom dock (tabbed) = time-series plots, probe table, event log/console, recipe/codex reference; central viewport with coordinate readout, scale bar (cm/m), colour-map legend with numeric range for every field view (pressure kPa/atm, temperature K/°C, velocity m/s, species mass fraction), grid/axes toggle.
- Units everywhere with user-selectable unit systems (K/°C, kPa/atm/bar, m/s). Placeable measurement probes and line samplers with live plots; CSV export of probe data and screenshots.
- Status bar: sim time, step rate, dt, CFL, % real-time, mass/energy conservation drift, GPU ms.
- Start directly in a workspace (no title screen). "Missions" become "Experiments"/"Lab exercises": a neutral list with objectives and measured results, no stars/gamification visuals.

## UI / workspace (ui agent: `src/app.rs`, `src/missions.rs`, `src/ui/*.rs`)

Docked scientific workspace, no title screen: the app starts with the "Rocket engine" setup loaded.
- `src/app.rs`: state + simulation driving (`canvas` builds `FrameInput`; egui may discard a pass, so the one-shot parts
  of an input that never reached `prepare` — scene, meta, reset, ops — are carried into the next frame via `Cb.used`),
  experiments bookkeeping (`Run`, objectives, readbacks), editing tools (shapes, fill, spray, pick, undo/redo),
  save/open (library `.cmb` format), screenshot hooks, `gas()` EOS helper.
- `src/ui/theme.rs`: palette (`PANEL #1b1c1f`, `FIELD #232428`, `BORDER #2c2e33`, one accent `ACCENT #528ee6`, semantic
  GOOD/WARN/BAD), Helvetica Neue (tabular figures) + SF Mono, 4/8 px spacing.
- `src/ui/workspace.rs`: menu bar (File/Edit/View/Simulation/Tools/Window/Help), icon toolbar (vector-drawn icons),
  left dock = materials library tree (categories from `elements::CATS`, search, columns ρ kg/m³ and characteristic T),
  right dock = Experiment panel / Selected material data sheet / Boundary conditions / Cell at cursor / Domain extrema,
  bottom dock tabs = Plots (p, T, thrust+ṁ, wall heat/stress vs sim time), Probes (table + history chart), Line
  profiles, Console (event log; `toast/log_info/log_warn` write here + status bar), Reference (`codex::show`),
  Experiments (campaign list via `campaign_bridge::all_missions()`); status bar (state, t, steps/frame, Δt, CFL,
  % real-time, Δm/ΔE drift, estimated GPU ms, frame ms); viewport header (field selector, cursor readout), axes in
  the chosen length unit (y measured up from the floor), scale bar, colour-map legends (ranges mirror vis.wgsl:
  pressure 0–5 atm, temperature 90–3000 K, speed 0–733 m/s, stress 0–120 % yield, species key, emission hue).
  Units: `ws::Units` (K/°C/°F, kPa/bar/atm/psi, m/s/km/h/ft/s, mm/cm/m), View > Units, saved in
  `~/Library/Application Support/Ignis/units.txt`. Templates (File > New from template) = app `SCENARIOS` +
  `scene::SHOWCASE` + `showcase::all()`.
- `src/ui/probes.rs`: point probes / line samplers (blocking `read_state` at 2 Hz only while instruments exist),
  diagnostics (total mass/energy, CFL, max speed, every 2 s), CSV export (File > Export probe data).
- Integrations: `settings` (Edit > Preferences, ⌘,; keymap drives all shortcuts; favourites stored there),
  `library` (File > Open from library / Save to library), `codex` (bottom Reference tab + Help > Reference manual),
  `tutorial` (Help > Guided tour; highlight rects: canvas, palette, palette:<CODE>, views, view:<name>, play, speed,
  slider:fuel, inspector, readouts; progress in `Ignis/tutorial.txt`), `audio` (created at startup unless
  COMBUSTOR_SHOT, fed every frame, volume/mute from settings), `campaign_bridge` (experiments list; progress in
  `Ignis/experiments.txt`).
- Screenshot hooks: `COMBUSTOR_SHOT=/abs.ppm` + `SHOT_FRAMES`, `SHOT_SCENE=<template key or title prefix>`,
  `SHOT_MISSION=n [SHOT_SOLVE[=bare]] [SHOT_LOG=1]`, `SHOT_VIEW`, `SHOT_SPEED`, `SHOT_TAB=plots|probes|profiles|console|
  reference|experiments`, `SHOT_WINDOW=settings|library|codex|experiments|shortcuts|about|tour`, `SHOT_PROBES=1`
  (two probes + a line sampler), `SHOT_UNITS=eng`, `SHOT_DOCKS=0`, `SHOT_TOOL=<tool key, e.g. M13>`, `COMBUSTOR_UNLOCK=1`.

### Experiments (`missions.rs` + campaign)
Experiments/lab exercises are the campaign missions (`campaign_bridge::all_missions()`) followed by the hand-written
`MISSIONS` it doesn't cover. Each is data: layout fn, known-good `solution` fn, palette, budgets, feeds, objectives
(`Goal::{Reach, ReachBy, Hold, Keep, AtMost, HoldBelow}` over `Metric`). The UI shows objectives with measured values
and met / not met / pending status (no stars); `stars` (0..3) is kept only for unlock order.
- `Metric::MatLost / StructLost` are positional; mission thrust is tared during setup (load-cell zero).

## Known bug reported by the user (must be fixed and verified by VFX owner)
- **Grey smoke "slab"**: in the real app (Campfire mission, user-built wood stack, around sim t ≈ 23 ms) the Fire view showed a flat, textured grey layer filling the whole lower ~70 % of the domain with a hard horizontal top edge — looks terrible and fake. Physics was checked: soot/products exist only in the fire plume (soot 0.003–0.018 near the fire, exactly 0 elsewhere), so this is a RENDERING artefact (smoke opacity/noise applied where soot≈0, an accumulation/trail/persistent buffer not decaying, a half-res smoke target not cleared, or density-deviation haze from the ~4 % ambient density change of a heated room). Reproduce with long app runs (SHOT_MISSION=2 SHOT_SOLVE=1 SHOT_SPEED=4 SHOT_FRAMES=1500+) and with large wood stacks; smoke must only appear where soot is non-negligible, as soft plumes that rise, spread and thin out — never as uniform layers.

## Chemistry & elements (chem agent, 2026-10-28)

**Elements**: matter ids now run 10..65 (`N_ELEM = 66` in matter.wgsl; walls stay 0-9). 40 new elements:
26 ice, 27 snow, 28 salt, 29 brine, 30 liquid N2, 31 gasoline, 32 diesel, 33 cooking oil, 34 napalm, 35 wax,
36 molten wax, 37 coal, 38 paper, 39 magnesium, 40 iron filings, 41 sodium, 42 potassium, 43 lye, 44 HCl,
45 H2O2 (90 % HTP), 46 MnO2, 47 KClO4, 48 nitrocellulose, 49 TNT, 50 C4, 51 dynamite, 52 blasting cap, 53 glass,
54 molten glass, 55 lava, 56 stone, 57 gunpowder, 58 dry ice, 59 CuO, 60 KMnO4, 61 glycerin, 62 plastic,
63 molten plastic, 64 flour, 65 fuse. Phase `E_PHASE`: 0 = **static matter** (placeable solid that does not move but
reacts: ice, wax, paper, TNT/C4/dynamite/cap, glass, stone, plastic, fuse), 1 powder, 2 liquid.

**Single source of truth**: per-element tables are generated into matter.wgsl between `// GEN-BEGIN/GEN-END` (all
`array<_, N_ELEM>`: phase, density, init T, boil/decomp T, latent, vapour gas slot, soot, volatility, air ignition /
burn T / rate, dust multiplier, suspension, viscosity, phase transition T + target + latent of fusion, burn-out
residue, self-reaction (ign T, rate, flame T, gas yield), detonation shock/T, product smoke, **E_CAP, E_COND, E_COL**).
`elements.rs`: `MATTER` (UI Element list, 56 entries), `MATTER_INFO` + `matter_info(id)` (density, melt/boil/ignite K,
detonation atm, hazard, products), `RECIPES` (24 triples, ingredient names match element names), `CATS` (+ `Cat::Metals`,
`Cat::Chemicals`).

**Hooks provided by matter.wgsl for sim.wgsl / vis.wgsl**: `N_ELEM`, `elem_of(i)` (unclamped id), `m_cap(m)`,
`m_cond(m)`, `m_col(m)`, `m_phase(m)` (fall back to `M_*` for walls 0-9). Until sim.wgsl adopts them, new elements
work chemically (matter.wgsl re-reads the real id from META) but sim/vis see them as id 25 (ash) for heat capacity,
conduction and colour.

**WALL.y for matter** (commit only acts on >= 1): 0 fresh (first update applies `E_TINIT`), 0.1-0.5 settled with
melt/freeze progress, `0.55 + id/1000` = conversion request, applied by `apply_conversion` at the start of the Margolus
movement pass (water→ice, ice→water, sand→molten glass→glass, lava↔stone, wax/plastic melt/freeze, salt+water→brine,
brine boiled dry→salt, Na/K→lye, iron→rust, thermite oxide→molten metal (z = 0.5 steel / 2.5 copper), Al→slag,
burn-out residues). Existing flags 1.5/1.6/1.7/1.8/2.0/3.0 unchanged.

**Gas mapping** (layout unchanged): fuel slot = methane-like fuel **and hydrogen** (Na/K/Mg + water, metals + acid,
Al + lye) and all fuel vapours; O2 = LOX boil-off, oxidiser decomposition, 42 % of peroxide; N2 slot = LN2 boil-off;
products = steam, CO2 (dry ice), HCl fumes, reaction products, 58 % of peroxide; soot = smoke. Emission kinds in
WALL.z: 1 vapour (slot by `E_VAP`), 2 O2, 3 hot products, 4 surface burning, **5 hydrogen, 6 peroxide O2+steam,
7 inert steam/CO2**.

**Movement**: static matter is immovable; liquids can sink under lighter powders (sodium/ice float); viscous liquids
(`E_VISC`: napalm, lava, molten glass/plastic, glycerin); fine powders (`E_SUSP`: flour, snow, charcoal, Al) hang in the
air and are carried by gentle drafts → dust clouds (airborne grains burn `E_DUST`× faster = dust explosions).

**Ignition**: contact (hottest touching matter/wall `tn_max`) and flame contact (`gt_max`) light pair mixes, energetics
and air-burning fuels; flame over a fuel pool holds its surface at the boiling point (self-sustaining pool fires).

**Scenes** (scene.rs): `Scene::by_name(name)` + `SHOWCASE: [(bench name, title, description, fn); 12]`:
kitchen (grease fire + wax-plugged water cup), sodium, flash (fuse race), thermite (Mg ribbon, plate burn-through),
silo (flour dust explosion), explosives (dynamite/TNT/C4/cap+C4), peroxide (HTP monoprop rocket), volcano, cryo, fuels (flash-point ladder: 6 fuels in cups on a heated plate under pilot flames), oxidisers (sugar + KNO3/KClO4/AP/KMnO4/AN piles on one fuse), acids.

## Requests (chem -> phys / vfx, 2026-10-28 wf4)
Matter ids now go up to 65 (`N_ELEM = 66`), but `mat_of` in sim.wgsl clamps to 25, so **every element >= 26 is simulated
and DRAWN as ash** (wrong colour/phase in the Fire view, wrong heat capacity/conduction). All hooks already exist in
matter.wgsl (same module, order-independent): `elem_of(i)`, `m_cap(m)`, `m_cond(m)`, `m_col(m)`, `m_phase(m)`.
- **phys (sim.wgsl)**: in `post` (gas-wall/matter heat exchange, ~L462) and `wall` (~L596, L613) use
  `let m = elem_of(j)` and `m_cap(m)` / `m_cond(m)` instead of `M_CAP[m]` / `M_COND[m]` for matter cells (tile 8);
  keep `mat_of` for walls (tile 1). `stats` (~L941) is fine (walls only).
- **vfx (vis.wgsl)**: in `colorize` matter branch (~L218) use `let e = elem_of(i)`; in `matter_albedo` use `m_col(e)`
  instead of `M_COL[e]` and `m_phase(e)` instead of `E_PHASE[e]` (E_PHASE is already N_ELEM long, so only the colour
  and the clamp matter). `E_COL` holds real reference colours (sRGB-ish, same convention as `M_COL`) for all 66 ids.
- elements.rs now also exports SI data sheets: `MatterInfo.{cp, hc_mj, hex_mj, l_fus_kj, l_vap_kj}`,
  `density_kg_m3()`, `phase_name()`, and `WALL_SI` / `wall_si(mat)` (kg/m3, J/kg K, W/m K, K, MPa, MJ/kg) for walls 0-9.
  UI: show these in the Properties dock.

### Requests (chem -> phys, fix round: campfire / wood / T ceiling) — sim.wgsl, please apply
Campfire diagnosis (bench `campfire`, chem): the 18 atm / 3600 K start-up spike came from the scene's 3 torch ops
(kind 5 sets gas to 2700 K at constant density = 9 atm, then the wood volatiles burned in that gas → T_MAX clamp) and
from 1-cell slits between stacked logs. scene.rs now builds round logs with open gaps, kindling igniters on the log
flanks and no torch → start-up pmax 4.3 → 2.7 atm, no 12.00 Tmax. **Still open (needs sim.wgsl)**: once the logs
pyrolyse, local pmax 7-15 atm persists in the gaps under the bed. Measured cell (259,276) at frame 299: rho 1.02 of
almost pure pyrolysis gas (fuel 0.69, products 0.32, O2 0) at ~1220 K → 6.5 atm, with open gas neighbours. Cause:
wood mass release `K_WOOD·exp(-12/T)` ≈ 0.6 mass/face/time at the 1380 K wood surface we see = ~200 kg/m²s physical
(real wood 0.01-0.02 kg/m²s); the narrow gaps cannot vent it, the gas pushes the air out (no O2 → no flame in the bed)
and burns as a detached shell above. Requested changes in `post` / `wall` (wood, m == 7):
1. **Pressure-limited release** (volatiles percolate through porous char; they cannot pressurise a pocket):
   `dmass *= clamp((1.15 - p_cell) / 0.15, 0.0, 1.0)` for wood, in `post` (gas side) and the matching `rem` decrement
   in `wall` (use the same p: `gp_sum / exposed`).
2. **Endothermic pyrolysis** (≈1.5 MJ/kg → ~11 energy units per gas-mass unit with Q_FUEL 360 ≈ 50 MJ/kg): subtract
   `dt·r·exposed·11 / cap` from the wood temperature in `wall` so the pyrolysing surface self-limits near 650-900 K
   (2.2-3.0) instead of 1380 K (4.6); today the surface is heated by the flame with no sink.
3. **Volatile composition**: wood volatiles are ~35-40 % CO/H2/CH4/tar and 60-65 % CO2/H2O by mass (LHV ~15 MJ/kg):
   emit `0.4·dmass` to fuel and `0.6·dmass` to products (s.a.w) instead of 100 % fuel. Stoichiometric adiabatic
   flame then ≈ 1900-2000 K and the luminous sooty flame, with soot radiation, settles at ~1200-1500 K (real campfire).
4. **Anchoring**: ignite the volatiles at the bed: when a gas cell touches wood with `tw > 2.6` (char glowing,
   ~800 K) and has O2, treat it as `tt >= T_EDC` for the reaction-rate gate (glowing char is a pilot). This keeps the
   flame attached to the logs instead of the gas leaving unburnt at 450 K and igniting 30-60 cm higher.
5. **T ceiling** (issue: "peak gas temperature 3600 K" identical in every rocket screenshot = the hard `T_MAX = 12`
   clamp): replace the clamp with dissociation — above ~2.8 (2800 K… 9.3) reduce the heat release, e.g. in the
   combustion step `s.b.z += Q_FUEL * db * clamp((T_DISS1 - tt) / (T_DISS1 - T_DISS0), 0.15, 1.0)` with
   T_DISS0 = 9.3 (2800 K), T_DISS1 = 11.7 (3500 K), and keep `T_MAX` only as a safety net at 13 (3900 K). CH4/O2 then
   peaks at ~3400-3500 K and the readout varies with O/F instead of sitting on 3600 K.
   Also: `T_IGNITER = 10` (3000 K gas at constant density) makes every igniter a 3-4 atm source and dominates `Tmax`
   in the campfire/silo benches; a hot-wire/pyrotechnic igniter at ~1500 K (5.0) still lights everything we have.
Hooks for this: none needed from chem (wood is a wall material owned by sim.wgsl).

## User feedback: rocket exhaust still doesn't look like a rocket (look-dev MUST address)
Current render (rocket scene): chamber white-hot (good), but the plume is a soft, blobby, uniformly orange-white glow, and the start-up vortex ring is drawn as huge bright orange swirls. Real rocket exhaust looks like this (use as the reference):
- A tight, straight, collimated high-velocity jet with a fairly well-defined boundary that only slowly widens and becomes turbulent far downstream. Not a puffy cloud.
- A bright chain of shock diamonds / Mach disks (where the pressure field shows them — follow the simulated pressure/temperature: re-compressed hot cells glow brighter), with darker, more transparent expansion regions between.
- Mostly TRANSLUCENT: brightness concentrated in the core and diamonds; you can partly see through the plume; outer edges faint.
- Start-up vortex rings and far-field mixed exhaust: faint, quickly cooling, smoky/hazy — never bright fire bodies. Brightness should follow temperature steeply (blackbody / band emission), and the far field is much cooler.
- Colour by propellant (see next section): methalox = translucent blue-violet with pinkish diamonds; kerolox = bright orange-yellow, sooty, smoky trail; hydrolox = nearly invisible faint blue with visible diamonds; solid motor (APCP + Al) = blinding white-orange core with thick white Al2O3 smoke.

## Flame colour by chemistry (user request: "different fire colours based on the fuel")
Real flame colours come from emission chemistry; implement as far as the data allows:
- Premixed hydrocarbon: blue (CH*, C2 Swan bands). Methane diffusion flame: blue base, yellow-orange sooty tip. Hydrogen: almost invisible (faint blue/UV OH). Kerosene/oil/diesel/wood/candle: bright orange-yellow soot incandescence + smoke. Alcohol (ethanol): faint blue, nearly transparent. Sulfur: blue. Magnesium: blinding white. Aluminium powder / thermite: white-hot + white smoke. Sodium: intense yellow (589 nm). Potassium: lilac. Copper: green-blue. Boron: green. Strontium / lithium: red/crimson. Calcium: orange-red. Barium: pale green.
- NOW (render side): matter elements are known per cell, so emissions from burning/decomposing matter (WALL.z emission codes + element id) can seed colour tracers in the VFX detail layer that are advected with the flow and decay with temperature. Use them for powders/liquids/metals.
- NEXT (physics, separate stage after this workflow): gas-phase fuel identity requires expanding the gas state with real species (H2, kerosene vapour, metal-emitter tracers). Until then gas-phase "fuel" = methane appearance (blue premixed, yellow soot tip).

## VFX / look-dev notes (vfx owner, round 1)
- fx.wgsl now packs **10** layers (L9 = coarse velocity xy in cells/time unit, z = turbulence weight from |vorticity|).
- Fire view gas: emission = coarse chromaticity (from colorize) × detail luminance (detail.wgsl), thinning tongues tint
  toward red; smoke opacity = detail cloud opacity, lit by sky + fire light with detail-gradient relief and self-shadowing.
- **Grey slab fix**: fuel gas / O2 are near-transparent in the Fire view (opacity ≤ 0.07 / 0.05), diluted combustion
  products are invisible; only near-pure (x_products ≥ 0.62), cooled (T < 1.7 = 510 K) products show as white
  condensing steam. Soot is the only dark smoke. Detail field is clamped to ≤ 2.5× the local coarse soot.
- Analytic view colour maps (vis.wgsl `colorize`, for UI legends):
  - **Pressure (1)**: q = (p − 1 atm)/1.5 atm. q < 0 (below ambient): blue = clamp(−1.2 q); 0 ≤ q ≤ 1: red = q, green = 0.28 q
    (black → dark orange at +1.5 atm); q > 1: green = q − 1, blue = q − 1.5 (→ yellow at +3 atm, → white at ≥ +3.75 atm).
  - **Heat (2)**: `heat_ramp((T − 0.3)/9.7)` with T in units of 300 K, i.e. 90 K → 3000 K linear, 8 stops evenly spaced:
    black, deep blue, blue, cyan, green, yellow, red, white (stop k at 90 + 415.7·k K). Walls: same ramp ×0.85 + 0.1.
  - **Flow (3)**: HSV, hue = direction atan2(vy, vx) (0 = +x, y down), value = |v|/2.5 cells·time⁻¹ (saturates at 2.5).
  - **Gases (4)**: fuel yellow, O2 cyan-blue, products white (mole fraction weights), soot dark grey.
  - **Stress (5)**: walls only, stress ratio 0 → 0.5 green→yellow, 0.5 → 1 yellow→red, > 1 flashing magenta.

## Physics/optimisation pass (phys, wf4, 2026-10-28)

- **`var<private>` tables are rewritten to module `const`s** when gpu.rs builds the sim+matter+vis module
  (`.replace("\nvar<private> ", "\nconst ")`). naga/Metal copied every private table into each thread's private memory
  at kernel entry (≈8 KB/thread for kernels touching `matter_update`/`absorb_matter_emission`): `wall`+matter cost
  0.94 → 0.08 ms/dispatch. **Rule: never declare a mutable `var<private>` at module scope in sim/matter/vis.wgsl**
  (the rewrite would turn it into a const); use `const` for lookup tables. Request to chem: emit `const` directly in the
  GEN block generator.
- `wall` split into `wall` (tile 1) and `wall_m` (tile 8, calls `matter_update`); shared `wall_heat()`; `wall_copy`
  removed (folded into `commit`). `stats` reduces in workgroup memory first (exact: integer max/add).
- HLLC builds only the selected region's flux (same formulas).
- Heat transfer realism: `gas_wall_flux()` (used by both the solid side and post's gas side, conservative):
  convection h = HT·sqrt(p)·T^-0.15·(1+0.5|u|) (boundary-layer scaling; equals old HT for still ambient air, ~5x the old
  h ~ rho law for 2100 K flame gas → flame impingement heats plates much faster) + gas radiation
  eps·RAD·(Tg⁴−Tw⁴), eps = 1−exp(−(30·soot + 0.02·products)); the surface's own emission escapes only through (1−eps).
  Matter cells now use `m_cap`/`m_cond` (real per-element heat capacity/conductivity) in wall_heat and post.
- Bench: `SAVE=path` writes the final u0/meta/wall raw state for bit-equivalence checks between builds.
- Timing (M2 Pro, STEPS=40×100 frames, interleaved A/B, GPU shared with other agents): rocket 0.64 → 0.39 ms/step on a
  quiet GPU (2.63 s → 1.63 s); contended runs 5.3 → 3.5 s (−33 %); empty −41 %. Per-frame fixed cost (STEPS=0) unchanged.

## VFX / look-dev notes (vfx owner, round 2)
- detail.wgsl `r` (flame luminance) is now a **persistent glow tracer**: max(advected·exp(−dt/τ), coarse source), τ = 2.2 + 6·hot
  (time units), so fronts grow licking tongues trailing downstream instead of thin outlines. Fine curl octave weakened
  (kills the "fur" fringe). render.wgsl colours the excess over the coarse emission with a blackbody hue from its own
  brightness (yellow-white roots → orange → deep red tips).
- vis.wgsl colorize: **unresolved luminous body** — hot (> ~470 K) fuel-rich sooty gas gets a lifted visual temperature
  (≤ 1950 K) × soot/fuel emissivity: a visual stand-in for sub-grid flamelets inside pyrolysis-gas blobs (campfire, pool fires).
- fx.wgsl: fuel-side luminous zone (tight bloom × fuel fraction) added to gas emission; fuel/O2 tints in the Fire view cut to
  ≤ 0.015 opacity (no more smooth "egg" haze around fuel clouds).
- render.wgsl: background grid removed (night gradient + faint large-scale mottling + grain); backdrop receives fire light
  (light falloff around fires); bloom rebalanced to tight core + weak tail (0.34 tight / 0.20 wide); walls get only 0.06 of
  the bloom (no glow bleeding through solid metal).

## REQUIRED FEATURE (user request): mixing powders and liquids
User: "there needs to be a mix option, which mixes powders and liquids — say I put sugar and saltpeter, how can I mix them to make an explosive".
Implement (chem owns matter.wgsl, phys owns sim.wgsl apply_ops, ui owns app.rs; whoever runs next implements all three parts consistently):
1. **Stir op** — new Op kind 8 = STIR (shape/radius as usual, sent every frame while the Mix tool is held). apply_ops must ignore kind 8 (no-op). In matter.wgsl `move_block`, loop over OPS (P.nops) and if the 2x2 block lies inside any STIR op's shape, randomly permute the block's four cells using the per-frame hash (only among movable matter and gas cells; never move walls/inlets), then return (skip gravity for that block this pass). Holding the tool for a second or two must turn two adjacent piles into a uniform blend. Liquids mix too; immiscible liquids re-separate by density afterwards (existing CA does this). Optional realism: stirring friction-sensitive mixtures (e.g. flash powder) has a tiny ignition chance.
2. **Composition brush** — Op kind 0 with tile 8 gains a mixture: `_p[0]` = second element id, `_p[1]` = fraction of the second element ×1000 (0 = pure). apply_ops picks per cell by hash(x,y) (optionally a third component via a third id packed in `dir` with `_p` fraction split — or keep to two components). UI offers recipe presets from elements.rs RECIPES with real proportions (by cell fraction ≈ mass fraction / density): rocket candy KNO3:sugar 65:35, black powder KNO3:charcoal:sulfur 75:15:10, thermite Fe2O3:Al ≈ 75:25, flash powder, APCP AP:Al ≈ 70:16 (+binder), ANFO AN:kerosene 94:6.
3. **UI**: a "Mix" tool in the toolbar/tools palette (shortcut M) with tooltip explaining that reactions need intimate contact; a "Compositions" section in the library listing recipe presets; the inspector shows local mixture composition (fractions of elements in a 5x5 neighbourhood) for matter cells.
4. Reaction rate already scales with the number of reactive neighbours (`npart`), so better-mixed = faster burning; verify a stirred sugar+KNO3 pile ignites and burns as a fast deflagration while unmixed side-by-side piles only burn at their interface.

## VFX / look-dev notes (vfx owner, round 3)
- render.wgsl **projected volume fill** (Fire view, gas): a 2D slice only shows a flame sheet as an outline; in 3D it wraps
  the hot body and is seen face-on. Hot sooty volumes (haze from L0.a/detail.a × soot gate `smoothstep(0.015, 0.08, soot)`)
  and soot clouds (detail cloud × soot share) receive patchy, flow-advected emission = fire light (tight+wide bloom) ×
  (1, 0.72, 0.48) × noise, added only where it exceeds the existing emission (`max(fill − 0.9·em, 0)`): fills hollow
  flame shells and puts glowing embers inside dark smoke, never brightens already-bright cores; clean exhaust (rockets,
  jets: no soot) stays translucent.
- vis.wgsl colorize: **volumetric soot body** term (`planck(flame_tk) · bb_int · (1 − e^(−320·soot)) · 3.2`, above ~800–1150 K)
  so the whole sooty hot volume self-emits; luminous front term reduced 1.8 → 1.3.
- render.wgsl tone map: `filmic()` = mix(hue-preserving ACES on luminance, per-channel ACES, w = 0.25 … 0.8 for L 1.5 … 12):
  orange/red flames keep their hue and gradation, only the brightest cores whiten (no flat clipped white). Exposure 1.05.
- Open (not render-side): campfire flames detach from the logs because pyrolysis gas leaves the bed unburnt at ~450 K and
  ignites ~1 m higher (phys/chem); the `flare` bench scene currently does not light (fuel leaks unburnt, products ≈ 0 above
  the posts at 300 frames), so flare shots show only the posts.

## REQUIRED (user request): custom propellants that burn steadily + many more materials
User: "add the option to make custom propellants, cause at the moment making custom long-burning propellants just explode"; "add more and more materials".
Root cause: every oxidiser/fuel mixture behaves like LOOSE powder — reaction spreads through the bulk by conduction and interior cells convert to 4-atm gas pockets (flag 1.6) => bang. Real rocket propellant is a CAST/PRESSED solid grain (rocket candy is melted and cast; APCP is cast in HTPB binder; black-powder motors are pressed): flame cannot enter the non-porous grain, so it burns ONLY at the exposed surface, layer by layer, with regression rate r = a·p^n (Saint-Robert/Vieille), and the grain geometry sets the thrust curve (end-burner = long steady burn; core/BATES/star = higher thrust, shorter).
Design:
1. **Packed state**: tile-8 matter cells get a "cast/pressed" flag (use META dir byte bit 0 for tile 8). Cast cells are immobile (CA treats them like fixed solids), mechanically weak (can crack/burst under over-pressure -> realistic CATO), conduct heat poorly, and react ONLY when exposed to gas (≥1 gas neighbour) at the surface rate r = a·p^n·(mixing quality) with per-composition a, n, flame temperature and gas yield (KNO3/sugar: n≈0.32, T≈1700 K; KNO3/charcoal/S pressed: n≈0.3; APCP: n≈0.35, T≈3200 K with Al; AP/sugar; Fe2O3 or copper chromite catalyst raise a by ~1.5–2x; binder (HTPB / epoxy) lowers a and raises mechanical strength). Interior cast cells NEVER react from conduction alone; when a surface cell is consumed it becomes ordinary gas (no extra pressure pocket) since its mass already left as surface-emitted gas. Burning surface emits hot products into the adjacent gas at flame temperature (existing emission code 3).
2. **Loose powder** keeps fast deflagration (porous: flame penetrates) — that's what black powder in a pile does — but unconfined piles should "whoosh" (fast burn, modest pressure) rather than detonate; confined loose powder can still blow its container.
3. **Tools**: "Cast / press" brush (new Op kind, e.g. 9) that flags matter cells in the brush as packed (and optionally melts/sinters them into a smooth grain look); "Uncast" not needed (break it). UI: Tools menu + shortcut (C if free), tooltip explaining grain geometry. Composition brushes (see mixing section) + a **Propellant designer** panel: choose oxidiser/fuel/metal/catalyst/binder percentages -> paints a cast grain of that composition; shows predicted burn rate, flame temperature, Isp-like figure, and safety notes.
4. **More materials** (real data: density, cp, k, melting/boiling/ignition T, heat of combustion, burn behaviour): binders (HTPB, epoxy, paraffin wax as hybrid fuel), catalysts (copper chromite, iron oxide already), more oxidisers (potassium perchlorate, potassium chlorate — friction-sensitive!, sodium nitrate, ammonium nitrate exists), more fuels (magnesium, titanium powder (sparks), zinc, red phosphorus, sorbitol, dextrose, PVC/plastic, rubber, cellulose/paper/cotton, coal dust, sawdust), more liquids (gasoline, diesel, acetone, methanol, hydrogen peroxide 90 %, nitric acid, hydrazine (hypergolic with N2O4/nitric acid), glycerin + potassium permanganate (hypergolic), brine, molten salt), gases where possible (hydrogen, propane — need the gas-species expansion stage), metals (lead, tin, gold?, tungsten for nozzles, graphite throats, ablative phenolic liners that char and erode realistically), insulators (fibreglass, firebrick, aerogel).
Verification: an end-burning KNO3/sugar cast grain in a steel tube with a nozzle burns for several seconds at roughly steady pressure without exploding; a core-burning grain gives a rising/progressive pressure trace; a cracked grain or a nozzle blocked causes over-pressure and case failure (CATO); loose powder in a pile whooshes.

## Integration notes (integrator, wf4)
- Crate and binary renamed to `ignis` (`target/release/ignis`); `target/release/combustor` is a local compat symlink only. Window title "Ignis". Env hooks keep the `COMBUSTOR_SHOT`/`SHOT_*` names. `tools/bundle_macos.sh` builds `dist/Ignis.app` (verified launching). QA output defaults to `scratchpad/wf4/qa`.
- Flare scene: stack shortened (top = h-26) and igniters moved onto the rim (top-1) — the 64-cell stack took ~300 frames to fill, so the flare never lit inside the QA window. Now burns from ~frame 200 (late burn ~0.06).
- QA `silo` check now uses peak pressure (dust/matter burning is not counted in the gas `burn` stat).
- `src/acoustics.rs`/`acoustics.wgsl` (physical acoustics engine, other workflow) is NOT yet wired into gpu.rs/app.rs: it needs `GpuAcoustics::encode_mic` inside the step pass and `encode_frame`/`harvest` per frame, then `AcousticAudio::feed`. Owner was still editing it at integration time.
- Perf (M2 Pro, release, GPU shared): headless jet ≈0.31 ms/step + ≈0.85 ms fixed per frame; in app (turbojet, 1x): 9 steps/frame, ~1080 steps/s, frame 8.3 ms (120 Hz), physical real-time 0.11 % (the solver is ~920x slow-motion by design; game clock 1x).

## VFX / look-dev notes (vfx owner, round 4 fix pass)
- **Moire / halftone stripes root cause** (detail.wgsl): the re-anchoring low-pass was a 4-tap diagonal box at +-0.5 cell whose
  frequency response is -1 at exactly one coarse cell, so `h += (c - hl)·k1` amplified grid-frequency ripple every frame (wavy
  stripes around every flame/smoke edge). Now a separable (1,2,1) tent (response >= 0) plus a B-spline anchor (no bilinear kinks).
- Emission is temperature-driven: hot-gas term = Planck × bb_int(flame_tk) × 0.30 × products (was a flat product-mass fill);
  luminous diffusion-front hue/brightness from local T; soot body ×(T/1600 K)² (bright cores, dull red tips); unresolved
  luminous-body lift now ramps with T (cool ~600-800 K pyrolysis core stays dark under its smoke).
- Smoke: opacity 1-exp(-70·max(soot-0.0006,0)) (wood soot 0.003-0.02 → visible grey-brown, trace soot clear), albedo 0.23;
  fire light on smoke self-shadowed (×1.1, tau×2.2); projected-volume fill gated by gas temperature (not by smoke opacity).
- Bloom is occluded by walls (tile 1): tight blur stops marching at the first wall cell; quarter-res wide blur uses block
  transmission (a of the downsample). Walls are lit by tight bloom + 0.55 wide (no chamber light on the outside of steel).
- render.wgsl: log highlight compression (C1 at L=1) before filmic; flow-noise LOD from advected-coordinate stretch (FBIAS);
  heat shimmer uses rotated, time-advected noise; solid/liquid/powder coverage from B-spline mask (smoother diagonals);
  no per-cell albedo jitter on walls (fire and analytic views).
- Open (physics): the campfire still produces a large sooty pyrolysis-gas body that burns only at its outline (renders as
  a dark sooty mass wrapped in flame sheets, not a compact flame with a thin plume).

## Performance team status (2026-10-29)
- Already in the tree and verified this round: sparse active-tile stepping (`act` kernel + calm flags in AUX; rk1/rk2/post/wall/wall_m/commit skip calm
  tiles with a one-tile halo), wall ping-pong (no `wall_copy`), workgroup-local `stats` reduction, turb fused into `post`, unchecked sim module,
  const lookup tables. `IGNIS_DENSE=1` disables sparse stepping (A/B switch).
- Bench, 100 frames x 20 steps, shared GPU: rocket dense 2.56 s -> sparse 0.6-0.95 s (about 0.3-0.45 ms/step incl. fixed cost);
  empty 2.63 s -> 0.42 s. STEPS=0 fixed per-frame cost: 2.0 ms/frame. A 1x frame (19 steps) is ~6-8 ms, so 60 fps holds with headroom.
  Rocket/jet stats unchanged (rocket intact, 0 bad walls, thrust sign and magnitude as before).
- Not done: CFL-adaptive dt (P6, high physics risk; DT stays 0.028), f16 storage (P10). Suggestion for UI team: the dyn_cap controller can now
  allow a higher MAX_STEPS since a step costs ~0.3 ms; shift+scroll brush resize already exists in app.rs (brush_scroll).

## UI team notes (wf5)
- Shift+scroll over the viewport changes the brush radius (0..24 cells; accepts either scroll axis because macOS maps shift+scroll to horizontal; 8 pt of scroll per step). Cmd+scroll / pinch zooms, plain scroll pans when zoomed. Brush also on `[` / `]` via keymap actions BrushSmaller/BrushLarger.
- Window icon is set from `assets/branding/icon_256.png` (main.rs, `with_icon`); title "Ignis".

## VFX / look-dev notes (look-dev team, round 1/3)
- render.wgsl white oxide smoke (APCP/Al/Mg tracer, `CTX.a`): particles lag the gas, so their incandescence is driven by
  max(haze, jet speed from fx layer 9) instead of the (expanded, cooled) static gas temperature. Fast cores of solid-motor
  plumes now read warm white-orange with the base smoke lighting suppressed (`wl*(1-0.7hw)`), no longer a flat grey slab.
- Status: shock diamonds / Mach disks (vis.wgsl `band`, pressure+T+Mach driven), propellant colour tracers (fx L10 -> detail
  `CSRC/CDST` -> render) are in place from earlier waves. Still open: campfire flames render as bright rims around dark
  sooty interiors (physics-side hollow flame bodies); the rocket scene's start-up phase is chaotic at 1 cm resolution.
- User request relayed: Shift+scroll resizes the paint brush (already implemented in app.rs, `brush_scroll`).

## Known bug (user screenshot, rocket scene): over-exposed render
After the real-chemistry calibration (Q_FUEL 485, T_ACT 60, T_MAX 30) CH4/O2 flames reach T ≈ 13.5–14 (≈4000 K). The Fire view then saturates: the entire frame (chamber, plume, far field) renders as a flat blown-out pink/orange wash with large purple haze patches, hiding walls and structure. Emission/tonemap/bloom must be re-balanced for the new temperature range: exposure by local luminance (auto-exposure or compress above ~3000 K), bloom radius/intensity capped, haze (purple) only where gas is genuinely hot AND dense, walls and nozzle edges always legible. Check with the rocket (bench 400+ frames), srm, jet and demo scenes.
(Fixed separately: the demo rocket scene itself — injector-face igniter, titanium jacket 12 cells, cooled copper liner on every wall cell within 2.5 cells of gas — so it no longer bursts at start-up.)

## VFX / look-dev notes (look-dev team, round 2/3)
- render.wgsl projected-volume fill: hot gate lowered (haze 0.10-0.60 instead of 0.35-0.95), fill x2.6 with a deeper orange (1, 0.55, 0.26): flame bodies are filled through the interior (the "hollow neon shell" is gone from campfire/fireball); dark patches remain only where gas is cool soot.
- White oxide smoke (CTX.a): 5-tap blur (+-2 cells) of the tracer, max opacity 0.82 and x0.5..1 by incandescence, hotter orange tint. Still open: SRM core is a cream slab with a cut-off where the tracer ends (needs a tracer fade in detail.wgsl CDST), rocket colour by propellant, jet/flare scenes.
- Shift+scroll brush resize already exists (app.rs `brush_scroll`).

## VFX / look-dev notes (look-dev team, round 3/3)
- render.wgsl projected-volume fill: colour now temperature-graded (deep red-orange for cooler haze -> yellow-orange for the hottest, via `hh`), fill modulated more strongly by flow noise (0.25 + 1.8*pch) so flame bodies are patchy/graded instead of uniform salmon; smoke occlusion of emission reduced 0.45 -> 0.25 (smaller dark "mask" patches in campfire/chem).
- Still open (physics-side): rocket plume meanders (sim momentum/dissipation), no clean shock-diamond chain in rocket scenes; flare scene weak. Shift+scroll brush resize exists in app.rs (`brush_scroll`).

## Materials & chemistry (chem2, 2026-10-29)
- **Looks**: `src/matter_look.wgsl` (appended after vis.wgsl in gpu.rs) owns `matter_look(e,x,y)`; vis.wgsl `matter_albedo` just calls it. Per-element table `LK_STYLE/LK_C2/LK_PAR` (colour = `E_COL`), 21 styles: 0 fine dust, 1 granule, 2 crystal, 3 prill, 4 flake, 5 turning/ribbon, 6 chunk, 7 fibre, 8 smooth/waxy, 9 liquid, 10 coal facets, 11 woody, 12 brushed metal, 13 rock, 14 mixed grains, 15 ice, 16 paper, 17 glass, 18 cord, 19 brick, 20 aerogel. Overlays: cast (smooth sintered grain, layered), wet, charred. Pure function of the cell coordinate: crisp at 1 cell, grain detail when zoomed.
- **Elements**: ids 10..91 (`N_ELEM = 92`). New 66-91: HTPB, epoxy, copper chromite, KClO3 (friction-sensitive), NaNO3, Ti, Zn, red P, sorbitol, dextrose, rubber, cotton, coal dust, sawdust, acetone, methanol, HNO3, hydrazine, N2O4 (hypergolic pair), tungsten, graphite, ablative phenolic (87, chars to 88), fibreglass, firebrick, aerogel. elements.rs remains the single source (`MATTER`, `MATTER_INFO`, `RECIPES`).
- **Cast / pressed grains**: META dir bit 0 on a tile-8 cell (`CAST_BIT = 0x10000`) = packed grain: immobile, conducts poorly, reacts only at gas-exposed surface cells via `cast_burn(ox, fu)` (r = a p^n, per-composition flame T / gas yield; catalyst raises a, binder lowers it), `cast_ign`. Op kind 9 = cast/press shape; Op kind 0 tile 8 with `dir` bit 0 places cast cells; `_p[0]`/`_p[1]` = second component id / fraction x1000. Over-pressure cracks grains (clears the bit -> loose) = CATO. Loose powder still deflagrates (whoosh).
- **Scenes** (`scene::MATERIAL_SCENES`, resolved by `Scene::by_name`, usable as bench names and SHOT_SCENE): `material_board` (every element in a labelled tray), `motor_end`, `motor_core`, `motor_cato`, `motor_loose`.
- **Verified** (bench, 20 steps/frame, 0 bad cells): motor_end burns ~8-10 s sim time (grain 1400 cells -> 0), pmax 25-37 atm steady, case intact; motor_core consumed ~3.5 s, pressure falls off as the bore opens; motor_cato pmax 47-56 atm, wall stress 0.82 and rising; motor_loose pmax 7-10 atm (whoosh, no detonation).

## Acoustics integration (acoustics team, wf6) - ONE audio system
`src/audio.rs` = device (cpal) + mixer + room reverb + limiter and the `Audio` API; `src/acoustics.rs`+`acoustics.wgsl` compute the sound from the solver's fields (child module `crate::audio::acoustics`, no extra `mod` line needed). `Audio::update(&AudioSnapshot)` stays as a crossfaded fallback when no GPU frames arrive.
**Handle:** `acoustics::Link` wraps all GPU state; gpu.rs only needs three plain closure hooks (no dependency on audio/acoustics, so examples that `#[path]`-include gpu.rs keep compiling). The patch below was applied to a COPY of the tree and verified end-to-end through the real `GpuSim` (headless `--bench` + synth gave the same spectra as the replica renderer; cost ~0.2 ms GPU/frame). `Link::pre/step/post` are no-ops when `set_enabled(false)`.
**gpu.rs (perf team):**
```rust
pub type PreHook  = Box<dyn FnMut(&wgpu::Queue, u32, bool) + Send + Sync>;
pub type StepHook = Box<dyn for<'a, 'b> FnMut(&'a mut wgpu::ComputePass<'b>, &'a wgpu::BindGroup) + Send + Sync>;
pub type PostHook = Box<dyn FnMut(&wgpu::Device, &mut wgpu::CommandEncoder) + Send + Sync>;
// GpuSim fields (init all three to None in new()):
pub pre_hook: Option<PreHook>, pub step_hook: Option<StepHook>, pub post_hook: Option<PostHook>,
// impl GpuSim:
pub fn buffers(&self) -> (&wgpu::Buffer, &wgpu::Buffer, &wgpu::Buffer) { (&self.u0, &self.meta, &self.wall) }
// frame(): right after `let (gx, gy) = ...;`
if let Some(h) = self.pre_hook.as_mut() { h(queue, inp.steps, inp.scene.is_some() || inp.reset_gas); }
// frame(): inside `for _ in 0..inp.steps {`, as the LAST statement (after the `if walls_now { ...p_move... }` block)
if let Some(h) = self.step_hook.as_mut() { h(&mut pass, &self.cbg); }
// frame(): after the "vis" compute pass block closes, before `self.vfx.frame(...)`
if let Some(h) = self.post_hook.as_mut() { h(device, enc); }
```
**app.rs / ui (UI team):**
```rust
// app.rs ~l.526, right after `let mut sim = GpuSim::new(..)` (make it `mut`), only if audio is wanted:
let link = {
    let (u0, meta, wall) = sim.buffers();
    crate::audio::acoustics::Link::new(&rs.device, u0, meta, wall, crate::gpu::W, crate::gpu::H)
};
{ let l = link.clone(); sim.pre_hook  = Some(Box::new(move |q, steps, reset| l.pre(q, steps, reset))); }
{ let l = link.clone(); sim.step_hook = Some(Box::new(move |p, bg| l.step(p, bg))); }
{ let l = link.clone(); sim.post_hook = Some(Box::new(move |d, e| l.post(d, e))); }
// store `acoustics: Option<crate::audio::acoustics::Link>` in the app struct (None when shot / no audio device)
// app.rs, just before `let input = FrameInput {..}` (~l.1800), every frame:
if let Some(l) = &self.acoustics {
    l.set_frame(crate::audio::acoustics::Listener {
        pos: [cam[0] + cam[2] * 0.5, cam[1] + cam[3] * 0.5], cam, speed: self.speed, real_dt: dt_real,
        sim_time: self.sim_time, paused: self.paused, fan_pow: fan,
    });
}
// ui/workspace.rs feed_audio(): after set_muted:
if let Some(l) = &self.acoustics { l.set_enabled(!s.muted); a.feed(l.take_frames()); }
```
Keep `a.update(&snap)` as is (fallback). Optional: settings sliders -> `a.set_mix(acoustics::Mix{..})` (layers: combustion, jet, crackle, microphone, structure, machinery, lfe) and `a.set_reverb(wet)`. `Audio::fed` counts analysed frames (diagnostics).
**Validation (examples/acoustics_render.rs; python in scratchpad/v2/acoustics/{val,scene}.py; numpy venv at scratchpad/acoustics/venv):** `acoustics_render` now includes matter_look.wgsl in its sim module (it failed to compile after the look-dev change). Results 12 s renders: jet PSD slope f^-1.8 (1.5-5 kHz) .. f^-2.3 (5-15 kHz), hump 0.4-1.3 kHz (St 0.2 on U/D); rocket f^-2.2..-2.7 above peak, pressure skewness +0.40, dp/dt skewness +0.49 (crackle); flare peak 150-315 Hz, f^-3 roll-off; synthetic wood fire: excess kurtosis 13 (heavy-tailed), Poisson inter-pop CV 1.00; boiling: 97 % of power in 1-6 kHz (Minnaert, peak 1.9 kHz); explosion: 4 ms crack, positive phase then negative phase of similar size, rumble tail 89 % of power below 150 Hz, -14 dBFS -> -23 dBFS over 4 s. Changes: WIND_MIN_MS 120 -> 260 (campfire solver flow made a constant 100 Hz "wind" boom), loudness slope below 130 dB 0.22 -> 0.17 dB/dB (flare -31.8 -> -28.8 dBFS rms, rocket unchanged).
**Known (physics, not audio):** the 2D campfire scene's solver produces pmax up to 7 atm in the first seconds and >440 m/s flow cells through the whole run, which the jet model correctly reports as a weak jet (99-126 dB) on top of the 72 dB combustion noise; fixing the campfire initial transient would remove it.

## Integration round (v2 final)
- **Acoustics wired in**: gpu.rs has `PreHook/StepHook/PostHook`, `pre_hook/step_hook/post_hook` fields (None in `new()`), `GpuSim::buffers()`, and calls them in `frame()` as specified above. app.rs builds an `acoustics::Link` after `GpuSim::new` (not in `COMBUSTOR_SHOT` mode), installs the three closures, stores `App.acoustics`, and sends `Listener` every frame before `FrameInput`. `ui/workspace.rs::feed_audio` does `set_enabled(!muted)` + `a.feed(l.take_frames())`; `a.update(&snap)` stays as fallback. Verified: app runs 10 s without panic/validation errors, headless WAVs render (examples/acoustics_render).
- app.rs `MAX_STEPS` raised 64 -> 96 (dyn_cap controller still backs off below 52 fps).
- `mat_of()` clamp to 25 in sim.wgsl is harmless: it is only used for tile-1 walls and as `m` for matter (matter paths use `elem_of`/`m_cap`/`m_cond`/`m_col`).
- **Regression fixes**: (1) matter surface burn gate `o2_avg > 0.02` -> `0.008` (hot gas next to an igniter has O2 *density* ~0.02 even in air, so metals/dust touching igniters never burned; silo dust explosion recovered, thermite now ignites). (2) thermite scene: Mg ribbon is a pressed (dir bit 0) immobile strip; as loose powder it fell off the igniter.
- QA (tools/qa.sh, 24 scenes): 163 pass / 6 fail. Remaining failures (rocket Tmax 20.3 & thrust 170 < 250, srm case 88 % intact, thermite plate not breached in 400 frames, explosives pmax 3.99 < 5) are tuning issues; rocket/srm/thermite were already failing in the previous QA run with similar numbers.
- Perf (headless, 19 steps/frame, shared GPU): rocket 8.6 ms/frame, jet 8.1 ms/frame, empty scene 1.3 ms fixed cost -> ~0.38 ms/step in a busy scene; 1x needs ~19 steps/frame so ~115 fps headroom, 100 % real time.

## Physics critic round (v2, phys-critic)
Bench 240-400 frames, 20 steps/frame, 0 bad cells everywhere. Findings: rocket Tmax 12-13.4 (3600-4000 K, above real CH4/O2 ~3550 K) with thrust only ~-150 (QA wants >250) and a plume that vanishes ~15 cm past the nozzle; srm loses its case (walls 4110 -> 2282 by frame 239, stress 1.2-1.7, dE +442 %); campfire pmax 7-8 atm, Tmax pinned at exactly 9.00, dE +220 %, lobes spread sideways instead of a rising plume; motor_cato / motor_end / motor_loose behave as designed (end-burner 8-10 s then dies; CATO stress 1.14); explosives pmax only 1.1 atm (TNT/C4/dynamite do not detonate with a blast wave); thermite plate not breached (walls 8361 constant, pmax 2); flare Tmax 9.6 pmax 1.48 stuck (igniter-capped). No source edited.

## Audio fix round (snd)
- **Material signatures** (`audio::Signature`: Generic, BlackPowder, Magnesium, Thermite, Tnt, GrainBurn) shape the stats-driven engine: crackle rate/band/decay, steady hiss, deep pops, low body, blast crack/body, and chuffing (grain burn pulses ~11 Hz when chamber pressure < ~2 atm, steady above ~6 atm). Select with `Audio::set_signature(sig)` (non-breaking; app can pick it from the dominant burning element). Default Generic = old behaviour.
- `Audio::boom_at(strength, distance_m)`: report delayed by distance/347 m/s of game time (paused = frozen, scaled by sim speed) and attenuated with distance. `Audio::toggle_muted()` for the M hotkey.
- Offline review: `AUDIO_NO_DEVICE=1 AUDIO_SCENES_DIR=<dir> cargo run --release --example audio_demo` writes campfire, rocket, blackpowder, magnesium, thermite, tnt, graincast, explosion_far .wav. Checked: rocket RMS follows thrust ramp and shutdown; chuffing visible in graincast.
- Pause ducking and slow-motion already exist in the acoustic path (Listener.paused fade, Analyzer slow-motion scaling by `speed`).
- **Integration requests (not my files):** toolbar/status-bar speaker button + volume popover and hotkey M calling `settings.muted` toggle (workspace.rs/ui chrome); call `a.set_signature(..)` in `feed_audio`; show a "muted" state in the status bar.

## perf fix round (perf owner)
- **Conservation accounting was dead**: `post` had a stray `//ACCX` placeholder, so inlet/vent boundary flux and post-pass physical sources (igniters, fans, wall/matter exchange, soot radiation) were never booked. Now `acc_flush(...)` runs at the end of `post` (sim.wgsl). Bench drift at f300: flare, campfire, srm, rocket all |dm|,|dE| < 0.001 % (was dE +65..+505 %). No leak existed; the diagnostic was incomplete.
- Not done (files not owned by perf): steps/frame governor, time-scale presets, live V-sync and frame sparkline live in app.rs/settings.rs; detail-layer clamp/re-anchor looseness (item 1) is in detail.wgsl/vfx.rs (hi clamp = coarse*[3,2.5,2.2,2]+eps, re-anchor k1 = 1-exp(-dt/2.5)); app.rs dyn_cap starts at 24 and backs off when dt_real > 19 ms, so loaded GPUs (shared by teams) settle at ~9 steps.

## UI fix round (ui team, v2)
- View: `fit_view(meta)` in app.rs frames the occupied layout (extra room to the right for engine scenes); runs on every scene load, via View > Zoom to fit geometry / Shift+F / header "Fit" button (`App.fit_req`, handled in `shortcuts`). Header also has 1:1, +, - buttons.
- Propellant Designer is now a docked, resizable right SidePanel (`designer_window` in ui/propellant.rs, called from app.rs after the inspector) with a fixed footer (Paint / Test motor / Build), a log-log burn chart with padded axes, and a grain section + qualitative thrust-time preview.
- Inspector: while the sim runs, Domain extrema and Cell at cursor lead; the material sheet is a collapsible header.
- Charts: separate axis title row, auto time unit (us/ms/s) in Plots, legend entries degrade to value-only when narrow.
- Status bar: dm/dE/dt ASCII labels, separators, Sound on/off toggle. Experiments are all unlocked; one Start run (toolbar); state chip RUNNING/PAUSED/SETUP. Emission legend labelled "Visual / not a measurement".
- Library: procedural per-material thumbnails (`ws::thumb`), category colour tick, richer tooltips, Compositions open by default.

## VFX / look-dev notes (look owner, v2 fix round)
- vis.wgsl: new `inferno(t)` and `pres_col(p)` (duplicated in render.wgsl for the legend). **Pressure view (1)** = log2(p/1 atm): below ambient dark -> blue (to 0.25 atm), ambient near-black, above inferno up to 2^4.2 = 18 atm, plus a schlieren overlay (|grad p|/p) that outlines shocks / Mach discs. **Flow view (3)** = inferno of speed, 0..5 cells/time (hue wheel removed). Heat/gases/stress unchanged. The big circle in these views is the real expanding start-up blast wave of the sim (not a mask).
- render.wgsl: analytic views 1 and 3 draw a colour-bar legend at the right edge (uv x 0.962-0.978, y 0.60-0.94; ticks every 1/4 for flow, per octave of pressure with 1 atm in yellow). UI may overlay text labels: pressure bar bottom = 0.25 atm, top = 18 atm (log); flow 0..5. Analytic-view walls are now coverage-anti-aliased from the filtered wall mask with a bevel tint.
- Fire view: (a) overflow highlight tint + lower whitening so the rocket chamber keeps an orange gradient instead of flat cream; (b) supersonic jet band emission (vis.wgsl term (c): Mach and re-compression driven, visible at >= ~600 K) makes the collimated plume and diamonds visible; (c) flow-advected 3-octave turbulence modulation of fire emission (rolling lobes, dark gaps, deep-orange lows); (d) procedural ember sparks over hot sooty gas; (e) blast-wave shadowgraph ring from the pressure gradient; (f) brighter night backdrop gradient + stronger fire-light spill.
- Known / not render-side: the isolated "square" after the rocket nozzle is the sim's start-up Mach disc / vortex front (now visible as a thin bright line); the fireball scene is only ~40 cells wide and carries little soot so it still reads as a compact glow (needs more fuel mass/soot from sim); explosive flames and fuse look are sim/matter_look scale.

## Materials fix round (mat owner, 2026-10-29)
- **Looks**: `matter_look.wgsl` now has its own base colour table `LK_C1` (E_COL is untouched and still used for physics/heat tint), style/C2/PAR tables generated from `scratchpad/v2/mat/spec.py`. New styles 21 needle (KNO3, iron filings), 22 pressed stamped block (TNT), 23 spangle (zinc); smooth variants (putty C4, soft alkali metal Na/K, wax); liquids (style 9) take `par.w` variants 0 thin / 1 viscous / 2 fizz / 3 fume-frost / 4 gel / 5 molten, with refraction streaks, a meniscus line and depth darkening. Powders get a lit top edge, shaded flanks and depth shading (pile profile). Granule/prill/coal row offsets are hash-jittered (no periodic lattice in mixed piles). Per-element coordinate phase shift. `matter_albedo` in vis.wgsl is unchanged (calls `matter_look`).
- **Distinctness check**: `scratchpad/v2/mat/dist.py board.ppm N` lists the closest tile pairs (Lab mean colour + texture SD) from a `material_board` bench screenshot; remaining near pairs are only genuinely black materials (coal 37, rubber 76, coal dust 78, char 88) which differ in texture.
- **Scenes**: `material_board` labels trays with element codes (3x5 steel font, `Scene::glyph/label`) and builds powder heaps / flat liquids / pressed blocks; new `mix_demo` (separate vs stirred 65:35 vs black powder). Thermite scene: 2-cell plate and a KClO3+Mg starter pellet with its own igniters (plate is breached by ~frame 300).
- **elements.rs**: matter swatch colours regenerated from `LK_C1`; Steel / Titanium / Inconel / Cooled copper wall swatches separated.
- **matter.wgsl**: fuse now propagates (touch/gas ignition thresholds 2.5 / 3.4, `E_SPREAD[65]` 1.2) so explosives chains work (dynamite 60 atm at ~f40, TNT, cap -> C4 49 atm at ~f320); iron-thermite spread 0.4 (Mg 2.0), copper thermite 1.0; molten metal keeps a latent-heat reservoir in WALL.w (holds at freezing point while it drains); cast_burn a reduced (KNO3/sugar 0.12 -> 0.05, KNO3/charcoal 0.25 at 6.3, APCP 0.10/0.075) plus a pressure limiter `r /= 1+(p/45)^2` => motor_end holds 5-6 atm steadily for 900+ frames.
- **Requests to phys (sim.wgsl)**: (1) `commit` flag 2.0 uses a fixed 60 atm / 3450 K; a per-explosive brisance (WALL.z could carry it) and a detonation front speed would separate TNT / C4 / dynamite. (2) The `srm` bench scene uses wall material 8 (Saint-Robert in sim.wgsl), not cast matter, so its case burst (4110 -> 1500 walls) needs a lower burn coefficient or a pressure limiter there. (3) Tmax 11-12 in motor_end is the igniter held on (`T_IGNITER`), not the 1700 K grain flame.
- **Not done**: library thumbnails/columns/sorting (library.rs / ui), regression-rate mm/s inspector readout and designer prediction link (ui), rocket/flare/campfire gas-side items 13-16 (sim.wgsl).

## Final verification (v2)
Release build clean; qa.sh 164 pass / 5 fail (rocket Tmax 20.31 vs 20 limit and late thrust 170 vs 250; srm case/nozzle 88% intact; thermite plate not breached in 400 frames). All 24 scenes run with 0 bad cells. Bench ~0.45 ms/step, ~9 ms/frame (20 steps). App: 8 steps/frame, 0.09-0.11 % real-time. Gallery in scratchpad/v2/final. dist/Ignis.app rebuilt and launches.

## Propulsion demos (src/propulsion.rs, propulsion_air.rs, propulsion_rocket.rs)
Registry `propulsion::all()/NAMES/by_name` (bench: `ignis --bench N <name> [out.ppm] [view]`; UI group "Propulsion"). Thrust = Stats.thrust x (negative = forward for +x exhaust), steady values at frames 200-400; pmax in atm. All: 0 bad cells. Thrust stat counts walls/igniters/fans only (not load through matter/grain), and includes fan force.
- turbab: dry (top) vs reheat (bottom) turbojet; reheat plume far hotter/longer (Fire view); single-engine thrust gain about +17 %. Walls 4925 -> 4906, pmax 7.9.
- turbofan: bypass/core ~3.5:1 area ratio, shock diamonds in core jet; thrust -106..-127, pmax 6.5, walls -6.
- ramjet: spike inlet, gutter-held flame; flame-holding demo, no net thrust at 2 atm inlet (thrust ~+3..8 = inlet drag artefact). Walls intact.
- scramjet: fan plenum, Laval nozzle, isolator, strut, diverging combustor (transonic, not true supersonic); thrust -108..-143, pmax 6.6.
- pde: single-shot pulse-detonation tube with Shchelkin rings, front ~Mach 5, pmax up to ~36; walls -55 (obstacles).
- ejector: bare vs shrouded turbojet; augmentation only ~+7 %; thrust sum -140..-194.
- rk_aerospike (-270, pmax 12.1), rk_expansion (3 nozzles, -400, 12.1), rk_dualbell (-124..-136, 12.1), rk_srm_core (-80..-125 early, burns out), rk_srm_star (pmax 13-17), rk_hybrid (-130..-156, pmax 11-29), rk_monoprop (thrust_y -250..-380, pmax 12-17), rk_tanks (blowdown -70 -> -26), rk_coldgas (-86, pmax 8-10), rk_rcs (net ~0, couple), rk_expander (-240..-284, pmax 21.7), rk_srm_end (steady 4-7 atm burn, thrust stat not meaningful).
- Withheld: pulseu (valveless pulsejet) - flames out after the start-up pulse, no sustained cycle; still in propulsion_air.rs.
- Wishlist: Scene.air_p for real ram pressure, thrust stat counting load through matter/fans excluded, throttled/timed injectors.

## UX / time controls round (v0.2.0, ux team)
- Step governor (`app.rs canvas`): target 60 fps; an over-budget frame (>21 ms) shrinks the step cap in proportion to the overrun, an on-time frame (<18.5 ms) that wanted more steps grows it x1.08+0.5. `MAX_STEPS` is 192; the cap is `Settings::graphics().max_steps_per_frame` (presets 24/64/128/192, previously unwired). Per-step cost is learned (`step_cost_ms`: upper-bounded by short frames, measured by long ones); there are no GPU timestamps, so the status-bar GPU figure is an estimate and is capped at the frame time.
- Status bar: `sim : wall 1 : N` (wall seconds per simulated second, from smoothed sim ms per wall s; 1x is about 1 : 950 because TPS = 32 time units/s and 1 unit = 0.034 ms), steps/frame, dt, CFL, drift, GPU estimate, frame. The old "real-time %" is gone. Time-scale presets 0.1-4x; hotkeys `-` / `=` (Simulation group), Shift+M mutes. No solver time-scale Params plumbing existed in gpu.rs, so none was wired.
- Inspector leads with Cell at cursor (placeholder when the pointer is outside) and Domain extrema; the material sheet is collapsed and opens when the tool changes. Plots: rolling window (All/0.5/1/2/5/20/100 ms) and log10 pressure axis. Viewport header readout degrades to the zoom factor when narrow (docked designer).
- Library: category filter chips, click-to-sort on Name / density / T* (click again to reverse, third click resets), group view only when unsorted and unfiltered.
- File > Browse templates (window with honest descriptions; `SHOT_WINDOW=templates`) and the New from template menu split Propulsion into Air-breathing / Rocket nozzles / Solid motors / Liquid, hybrid and cold gas / Engine tests; material scenes (motor_*, material_board, mix_demo) are included. `honest_desc()` overrides optimistic scene blurbs (ramjet, scramjet, pde, ejector, srm). Test `template_tests::all_templates_build` builds every template.
- Shortcuts window lists shift+scroll brush size, Shift+F fit, M mix, C cast. Not done: auto-pan to follow the plume, plot scrubber, thumbnails from the real matter look, modal Preferences.

## Final verification (v0.2.0)
Clean release build; qa.sh 165 pass / 4 fail (rocket late thrust 183, srm case 88 %/nozzle 89.6 %, thermite plate not breached). All 70 scenes (core, material, 18 propulsion, 18 showcase) run 150 frames: rc 0, 0 bad cells, no panics. Rocket 300 frames 1.85 s headless, jet 2.1 s, campfire 2.0 s, material_board 4.0 s. App shots (COMBUSTOR_SHOT) work. Gallery: scratchpad/r2/final. README feature/limitations updated; docs/CHANGES_v0.2.0.md added.
