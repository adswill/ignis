# IGNIS performance profile (Apple M2 Pro, wgpu 24 / Metal, grid 512x288, 2026-10-28)

How these numbers were taken:

- `examples/gpu_profile.rs`: the game's own `GpuSim` (src/gpu.rs, src/vfx.rs and every `.wgsl` pulled in with `#[path]`
  and `include_str!`, so it always measures the current sources). Run it with
  `cargo build --release --example gpu_profile && ./target/release/examples/gpu_profile`.
  - **A. Real `GpuSim`, pipelined**: frames submitted back to back like the app does it. Wall time / frames is the
    steady-state GPU cost, since CPU encoding (0.4–0.9 ms) is far below GPU time. Render passes are timed with pass
    timestamps.
  - **B. Per-kernel replica**: the same WGSL modules, kernel lists and dispatch order (parsed from gpu.rs/vfx.rs), with
    every dispatch in its own timestamped compute pass. Metal on Apple GPUs can only sample timestamps at pass
    boundaries: `TIMESTAMP_QUERY` and `INSIDE_ENCODERS` are available, `INSIDE_PASSES` is not. Encoder-level
    `write_timestamp` gives unusable values, so it is off by default (`TS_ENC=1` turns it back on). Splitting into
    ~110 passes adds about 20–25 % to the total (replica 20.3 ms against the real 16.4 ms frame). Use the % column for
    ranking. The "real ms" column below is rescaled to the real frame.
  - **B2. Isolated micro-bench + shader experiments**: a kernel dispatched N times inside one pass. Variants of the WGSL
    are built at runtime by string replacement, so a candidate optimisation can be measured before anyone edits a
    shader.
  - **C. Calibration**: an empty pass, an empty full-grid dispatch, and a 64 B/cell copy (memory roof) used as the
    yardstick.
- `tools/profile.sh`: drives all of the above, plus the headless `--bench` at STEPS=0/19/40 and the **real app**
  through the `COMBUSTOR_SHOT` hook (two runs, SHOT_FRAMES 60 and 600, so the difference is the frame time). It also
  collects `top` (CPU, Energy Impact), `footprint` (memory), `ioreg` (GPU utilisation), a 3 s `sample` CPU call graph,
  optionally `sudo powermetrics` (POWERMETRICS=1), and an Instruments Metal System Trace (XCTRACE=1).

**Caveat:** seven other agents were building and running GPU benches during this session. GPU "Device Utilization"
was at 94–100 % before most runs, and contended runs came out 2–5x slower. The numbers below come from the
quiet windows (the calibration copy hit 213 GB/s, against 45 GB/s when contended). The ratios and experiment
deltas were checked across contended runs and stay stable. Re-run `tools/profile.sh` on an idle machine before you
treat any absolute number as final. The script prints the GPU utilisation before it starts.

## 1. Headline

| | ms / frame | note |
|---|---|---|
| `GpuSim::frame` at **1x speed (19 sim steps)** | **16.0 – 17.0** | rocket / chem / demo scenes. Scene content barely matters (see wall) |
| of which fixed per-frame (stats, colorize, bloom, fx, sparks) | 0.9 – 1.1 | measured with STEPS=0 |
| **per sim step** | **0.80 – 0.84** | was 0.46 in the ARCHITECTURE.md baseline: +80 % since matter/chemistry landed |
| render pass `draw()` 1536x864 / 2880x1620 | 0.81 / 1.95 | composite + 32768 spark instances, scales with pixels |
| CPU: encode `GpuSim::frame` | 0.4 – 0.9 | ~110 dispatches/frame |
| CPU: egui UI + tessellation + egui_wgpu buffers (app, `sample`) | ≈ 0.5 | about 0.5 % of main-thread time. The main thread is **99 % blocked in `CAMetalLayer nextDrawable`** (GPU-bound) |
| process memory | phys_footprint 228 MB (peak 248), GPU buffers ≈ 63 MB | IOSurface 59 MB (swapchain), IOAccelerator 14 MB |

**The game is entirely GPU-bound, and at 1x speed it cannot hold 60 fps.** One frame needs about 16.5 ms of compute
plus about 1.5–2 ms of composite at retina canvas size, which is over the 16.7 ms budget. Because the step accumulator
in app.rs asks for `speed * TPS * dt_real / DT` steps, a slow frame asks for more steps on the next one. The steady
state is `s = 1.143·speed·(2.9 + 0.8·s)` ms:

- speed 1x: about 39 steps/frame, 34 ms per frame, so **~30 fps** (vsync rounds it down to 30).
- speed 2x: the loop gain is 1.83 > 1, so it **runs away to MAX_STEPS = 64**. Frames take ~55 ms (**~18 fps**) and the
  sim actually runs at only ~1x.
- Under GPU contention the real app measured **7.5 fps (132 ms/frame)**: 64 steps per frame at a contended
  ~2 ms/step. That is the same runaway.

To get 60 fps at 1x with margin, the cost per step has to come down to about **0.45 ms**, and the step loop needs a
GPU-time budget (see P0 and P9).

## 1b. Status update 19:16: phys already started the optimisation pass

While this profile was being written, gpu.rs/sim.wgsl changed. The step list is now
`rk1, rk2, post, wall, wall_m, commit`: `turb` is folded into `post`, `matter_update` moved into its own `wall_m`
kernel, and `wall_copy` is gone. rk/post use 16x8 groups. The profiler picks this up on its own: it parses the kernel
lists and `if k >= N` from gpu.rs. Re-profiled on the new code (rocket and empty, contended; per-dispatch **minimum**
in brackets, which is the closest thing to an idle-GPU figure):

| kernel | dispatches/frame | ms/dispatch median (min) | share of compute |
|---|---|---|---|
| `rk2` | 19 | 0.46 (0.239) | 27 % |
| `rk1` | 19 | 0.44 (0.233) | 25 % |
| `post` (now includes turb) | 19 | 0.34 (0.164) | 19 % |
| **`wall_m`** (matter_update) | 4.75 | 1.01 (**0.53**) | **15 %**: still launched over all 147 k cells; the empty scene costs the same (0.51) |
| `wall` (solid only) | 4.75 | 0.12 (0.070) | 1.8 %, was 1.15 ms: the split works (**−94 %**) |
| `commit` / `matter_move_a+b` | 4.75 / 4.75 | 0.07 / 0.13 (min) | 2 % / 3 % |
| vis + bloom + stats | 7 | ~0.9 total (min) | 4.5 % |
| fx + sparks | 3 | ~0.37 total (min) | 2 % |

Estimate for an idle GPU on this code: about **0.70–0.75 ms per step**, down from 0.80–0.84. Turb-in-post costs the
same as the old turb+post (0.164 against 0.152 ms min): the fusion removed a dispatch but none of the work. What
remains on P1 is **sparse launch of `wall_m`** over tiles that contain matter (indirect dispatch). An empty or
gas-only scene should then pay ~0 instead of 2.5 ms per frame. P0, P4–P13 below are still open.

## 2. Ranked hotspots (code as of 18:18, before the wall split; rocket, 1x = 19 steps/frame, quiet run)

`ms/dispatch` is the replica's per-pass time. `real ms/frame` is that time × dispatches/frame × 0.81 (the replica-to-real
ratio). The % column is the share of the GPU frame (compute + render at 2880x1620).

| # | kernel | dispatches / frame | ms / dispatch | real ms / frame | % of frame | x copy-roof¹ | bound by |
|---|---|---|---|---|---|---|---|
| 1 | `wall` (sim.wgsl, calls `matter_update`) | 4.75 | 1.147 | **4.41** | **24 %** | 26x | occupancy / register pressure from the inlined `matter_update`. Pays the same with **zero** matter cells (empty scene 1.1–1.4 ms) |
| 2 | `rk2` (MUSCL+HLLC flux + RK stage 2) | 19 | 0.246 | **3.78** | **21 %** | 5.6x | ALU (2 HLLC faces + reconstruction per cell), LDS barriers |
| 3 | `rk1` | 19 | 0.226 | **3.48** | **19 %** | 5.1x | same as rk2 |
| 4 | `post` (diffusion, heat exchange, combustion, soot) | 19 | 0.108 | **1.66** | **9 %** | 2.4x | 4-neighbour gather + divergent branches + `exp()`; `absorb_matter_emission` inlined |
| 5 | `turb` (Smagorinsky ν) | 19 | 0.044 | 0.67 | 3.7 % | 1.0x | memory: re-reads 4 neighbours of U2; separate pass only to write ν |
| 6 | `commit` | 4.75 | 0.091 | 0.35 | 1.9 % | 2.1x | full-grid launch for a handful of failing walls |
| 7 | `matter_move_a/b` | 4.75 | 0.13 | 0.50 | 2.7 % | 3x | Margolus 2x2 blocks, hash RNG, many conditionals |
| 8 | `wall_copy` | 4.75 | 0.069 | 0.27 | 1.5 % | 1.6x | pure copy of WALL next→current (redundant: ping-pong instead) |
| 9 | `stats` | 1 | 0.275 | 0.22 | 1.2 % | 6x | ~440 k **global atomics on 8 addresses** (3 per gas cell), fully serialised |
| 10 | `colorize` | 1 | 0.235 | 0.19 | 1.0 % | 5x | per-view branching, writes 64 B/cell |
| 11 | `fx` (fx.wgsl) | 1 | 0.210 | 0.17 | 0.9 % | 5x | semi-Lagrangian uv + 4x `bloom_wide` bilinear taps, 8 output layers |
| 12 | bloom `blurh`+`blurv` | 2 | 0.155 | 0.25 | 1.4 % | 3.5x | 13 taps each, reading `.rgb` out of a 32 B/cell interleaved layout (half of every cache line wasted) |
| 13 | wide bloom `bdown`+`bwideh`+`bwidev` | 3 | 0.128 | 0.31 | 1.7 % | 3x | **dispatched over the full 512x288 grid for a 128x72 job**: 15/16 of the threads return at once |
| 14 | sparks `spark_update`+`spark_spawn` | 2 | 0.123 | 0.20 | 1.1 % | – | updates all 32768 slots even when none are alive |
| 15 | render `draw()` @ 2880x1620 | 1 | 1.95 | 1.95 | 10.6 % | – | fragment ALU: 4–9 bilinear layer taps/pixel + noise + ACES; spark VS for 32768 instances × 6 vertices |
| 16 | `copy OUT→layers` (8x W×H rgba16f) | 1 | 0.022 | 0.02 | 0.1 % | – | – |

¹ 64 B/cell copy = 0.044 ms (≈213 GB/s effective). A kernel close to 1x is memory-bound. Far above 1x means ALU,
latency or occupancy-bound.

By stage: flux+RK **44 %**, wall/commit/matter (every 4th step) **30 %**, turb+post **13 %**, render 11 %,
stats/colorize/bloom 5 %, fx+sparks 2 %.

### Shader experiments (B2: built at runtime from the current WGSL, both variants timed in interleaved rounds)

| experiment | kernel | baseline → variant | saving |
|---|---|---|---|
| remove the `matter_update(...)` call from `wall` (gas/solid path unchanged) | `wall` | 0.50 → 0.07 ms (rocket), 1.32 → 0.05 ms (empty) | **86–96 %** |
| remove `absorb_matter_emission` from `post` | `post` | ±30 % (noise) | inconclusive. Re-run on an idle GPU |

So the wall kernel's cost does not come from walls. It is the occupancy penalty of carrying `matter_update` (more than
400 lines, an 8-neighbour scan and about 30 live accumulators) inside a kernel that is launched on all 147 k cells.
Gas cells pay it too, even though they only copy 32 B.

## 3. Why each hotspot is slow

1. **`wall` (24 %)**: register allocation is sized for the largest path (`matter_update` → pair_params, neighbour
   counters, thermite/ANFO/acid logic). On Apple GPUs this cuts the number of resident SIMD groups to a fraction of
   normal, so the latency of even the trivial `META` load → branch → `WALL` load/store path of gas cells goes unhidden.
   The work itself is tiny: in the rocket scene ~2.9 k of 147 k cells are walls. Every gas cell also copies
   `WALL[i]` into `WALL[n+i]`, and then `wall_copy` copies it back: 2 × 4.7 MB of traffic that does nothing.
2. **`rk1`/`rk2` (40 %)**: these are already LDS-tiled (20x20 halo for a 16x16 tile, fused flux + update). Each cell
   runs `to_prim` on a 1.56x halo, then 2 MUSCL reconstructions (minmod on 4 cells) and 2 HLLC solves (with several
   `sqrt` and divisions), and the tile's edge threads do a third. That is about 5x the memory roof, so the kernels are
   ALU and latency-bound. LDS is `vec4<f32>` × 2 × 240 + 240 u32, about 8.6 KB per group. There are 4 barriers. The
   halo-load loop (400 loads per 256 threads) leaves part of the threads idle on the second pass. Every cell is solved,
   including solid cells and large regions of quiescent ambient air.
3. **`post` (9 %)**: a 4-neighbour loop with tile-type branches (walls, matter, igniters, burnables), plus
   `reaction_rate`, `exp(-T_PYRO/T)`, `exp(-T_SOOT_OX/T)` and soot radiation `T⁴` for every gas cell. Neighbour
   reads of `FLUX[j].x` (ν) and of `U2` are 32 B-strided.
4. **`turb` (3.7 %)**: a whole pass whose output is one float per cell, read back by `post` straight away. ν goes
   into `FLUX`, a 9 MB buffer that is now used only for ν, at 32 B/cell stride.
5. **`stats` (1.2 %)**: `atomicMax`/`atomicAdd` per cell on 8 shared words, all serialised on the same cache line.
6. **Bloom**: quarter-res kernels launched on the full grid, 13-tap blurs reading `.rgb` from vec4 pairs, and
   blurh→blurv going through global memory.
7. **`commit`, `wall_copy`, `matter_move`**: full-grid launches that touch only the few cells that changed. Dispatch
   count per frame is about 110 kernels, which is fine for CPU (0.4–0.9 ms encode). On the GPU each launch costs about
   0.005 ms fixed, so the count is not the problem. Full-grid work per launch is.
8. **Render (11 % at retina)**: per-pixel cost is fine (about 0.42 ns/px), but it scales with the canvas. The spark
   draw always issues 32768 × 6 vertices.
9. **CPU / frame pacing**: CPU is not a bottleneck (egui + app is about 0.5 ms/frame). The real problems: (a) the
   step accumulator runs away when the GPU is over budget; (b) `snapshot()` on every stroke start calls
   `GpuSim::read_meta`, which does `device.poll(Wait)`, so the whole queued GPU frame (17–35 ms) stalls on the CPU and
   you see a hitch whenever you start drawing; (c) when paused the app still repaints at 10 Hz with all vis/fx passes,
   which is fine.

## 4. Prioritised optimisation plan

Expected gains are against the 1x frame (16.5 ms compute + ~2 ms render). "Owner" is the team that owns the file
today.

| P | change | owner | expected gain | risk |
|---|---|---|---|---|
| **P0** | **Fix frame pacing**: cap `steps` per frame by a GPU-time budget. Measure the last frame's GPU time from egui's `dt_real` or a timestamp query, and allow `steps ≤ (14 ms − fixed)/per_step`. When the cap bites, let `rt_factor` show the slowdown instead of letting the accumulator run away to `MAX_STEPS` | ui (app.rs) | 18 fps → 30+ fps at 2x right away; no more runaway | low |
| **P1** (half done 19:16) | **Split `wall`**: (a) a light `wall` for tiles 1/6 only, and (b) `matter_update` as its own kernel `matter_step`. Launch both only over **active tiles**: a 16x16 tile bitmask, rebuilt by `apply_ops`/`commit`/`matter_move` with `atomicOr`, compacted into an indirect-dispatch list (`dispatch_workgroups_indirect`). For the first step, a separate pipeline alone, with an early return for non-matter cells, keeps the heavy register set off the gas cells | phys (sim.wgsl, gpu.rs) + chem (matter.wgsl) | measured: wall 1.15 → ~0.07 ms/dispatch, **−4.0 ms/frame (−24 %)** | low: same math |
| **P2** (done 19:16, wall_copy removed) | **Ping-pong WALL halves instead of `wall_copy`**: a `wall_half` bit in Params selects read/write halves, and gas cells no longer copy | phys | −0.3 ms/frame and 2 × 4.7 MB less traffic per wall step | low |
| **P3** (turb merged 19:16, LDS tiling still open) | **Fuse `turb` into `post`** with an LDS tile: load U2 (tile + 2 halo) once, compute ν for tile + 1 halo in LDS, then run the diffusion/combustion stencil out of LDS. `FLUX` shrinks to N × f32 or goes away | phys | −0.7 ms (turb pass) and −30–40 % of post ≈ **−1.2 ms/frame** | medium |
| **P4** | **Active-region (sparse) dispatch for rk1/rk2/post**: a per-tile "quiescent" flag (max \|Δρ\|, \|u\|, \|Δp\|, \|ΔT\| over tile + halo below ε for k steps, never next to inlets/fans/igniters/matter). Quiet tiles skip rk/post; neighbours of active tiles are always woken (dilate the mask by one tile each step, so pressure waves cannot outrun it). Use indirect dispatch over the active list | phys | sandbox scenes are mostly still air: **−30–70 % of rk+post** (−3 to −6 ms) in typical builds; 0 in a fully turbulent scene | medium: must be conservative for acoustics |
| **P5** | **rk1/rk2 kernel work**: (a) cache primitives as `vec4<f16>` in LDS (`SHADER_F16`, if wgpu/Metal exposes it) to halve LDS and cut registers; (b) split the halo load evenly (400 loads / 256 threads); (c) early-exit tiles that are fully solid; (d) compute wave speeds once per face (Davis estimates share `sqrt(γp/ρ)` between the two faces of a cell, so precompute `c` in the halo load); (e) try a 32x8 tile, which has a smaller halo ratio | phys | −15–25 % of rk ≈ **−1.0 to −1.8 ms** | low/medium |
| **P6** | **Bigger or adaptive time step**: `DT = 0.028` is fixed, but CFL ≈ (V_MAX + c_max)·dt ≈ 0.39 even at the T and speed caps. A CFL-adaptive dt (max wave speed is already reduced in `stats`; reduce it per step on the GPU instead) or DT ≈ 0.045 with chemistry sub-cycling in `post` cuts steps/frame from 19 to ~12, which scales **every** per-step kernel by 0.63 | phys | **−35 % of all per-step work (−5 ms)**, the biggest single lever | high: physics validation (flame speed, detonations, stiffness) |
| **P7** | **`stats`**: workgroup reduction in shared memory (or subgroup ops) → 1 atomic per workgroup per counter | phys | −0.2 ms | low |
| **P8** | **Vis/bloom**: dispatch `bdown/bwideh/bwidev` over the quarter-res grid (`ceil(W/4)/16`); fuse `blurh+blurv` (LDS tile 16 + 2×6 halo); store glow in a packed `rgba16f` texture or a separate tightly packed buffer instead of every other vec4 of U2/U1; merge `colorize` into `fx` (fx already reads U2 / U1 bloom) | vfx | −0.5 ms | low |
| **P9** | **Render**: render the composite at no more than 2x cell density (512x288 × ~3, i.e. about 1536x864) into an offscreen target and upscale with a sharp filter, or use dynamic resolution when GPU time > budget; sparks: keep a live count (atomic in `spark_update`) and `draw_indirect` only live instances | vfx | −1 to −1.5 ms at retina | low |
| **P10** | **Half-precision state storage** (U1/U2 RK scratch, fx uv, colour buffers as packed f16 via `pack2x16float`): halves memory traffic for the memory-bound kernels (turb, wall_copy, commit, colorize, blur). U0 has to stay f32 (conservation, small mass fractions) | phys/vfx | −0.5 to −1 ms, 15 MB less GPU memory | medium (accuracy of RK stage 1) |
| **P11** | **No blocking readbacks on the UI thread**: keep a CPU mirror of META (the app already knows every op it sends) or use async `map_async` for undo snapshots and flood fill | ui | removes 17–35 ms hitches at stroke start | low |
| **P12** | **`commit`/`matter_move` sparse**: run `commit` only when `stats` reports a damaged wall (flag word in STATS); run `matter_move` only over tiles that contain matter (reuse the P1 tile mask) | phys/chem | −0.5 ms | low |
| **P13** | **Energy / idle**: when paused or on the title/missions screens, skip `stats`/`colorize`/bloom/fx when nothing changed (dirty flag), and drop to `request_repaint_after(100 ms)` there as well | ui/vfx | GPU idle when paused or in menus | low |

**Cumulative estimate (P0–P3, P5, P7, P8, P11, P12; no algorithmic risk):** compute 16.5 → ~9 ms per 1x frame, per
step 0.80 → ~0.45 ms. Add the retina render (P9): **~10.5 ms per frame = 60 fps at 1x with ~6 ms headroom**, and the
step loop stays stable up to about 1.9x speed. **With P4 (sparse) and/or P6 (adaptive dt):** typical sandbox frames
drop to 5–7 ms. That makes room for a 2x larger grid (1024x576 needs about 4x the per-step work) or for 2–3x speed at 60 fps.

Suggested order: P0 (one-line-level change, stops the runaway now) → P1+P2 (largest measured win) → P7/P8/P12
(quick) → P3 → P5 → P11 → P9 → P4 → P6/P10 (after physics validation).

## 5. Memory

| resource | MiB | note |
|---|---|---|
| fx `OUT` staging (8 layers × W×H × rgba16f + 256² noise) | 10.6 | copied every frame into the `layers` texture array (another 10.1 MiB). Writing storage textures directly would save 10 MiB and a copy |
| `FLUX` | 9.0 | now holds only ν (`FLUX[i].x`): **1.1 MiB would do** |
| `PPREV` (particles.wgsl binding 8, new) | ≤ 9.0 | size in vfx.rs not checked; the replica allocates N·64 B |
| U0, U1, U2, WALL, fx UVB | 4.5 each | |
| sparks, META, noise, ops | 2.1 | |
| **GPU total** | **≈ 63** | phys_footprint of the app 228 MB (peak 248 MB): swapchain IOSurfaces 59 MB, IOAccelerator 14 MB, fonts and egui |

Memory is not a constraint (16 GB unified). The traffic per step matters more. rk1, rk2, turb and post move about
330 B/cell, which is ~50 MB per step and ~0.9 GB per 1x frame (~55 GB/s at 60 fps, a quarter of the measured
copy bandwidth). Wall steps add 2 × 4.7 MB of WALL copies. The heavy kernels are not memory-bound, which is why
f16 storage (P10) ranks low.

## 6. Energy impact (estimate)

- CPU: `top` shows 2–4 % CPU (Energy Impact score 2–4) for the app, because the main thread sleeps in `nextDrawable`.
- GPU: `ioreg` "Device Utilization" sits at **~100 %** whenever the sim runs: the sim is GPU-bound at every speed
  setting because of the step-accumulator runaway. On an M2 Pro (19-core GPU) a saturated GPU draws roughly
  **12–20 W**. That means fans on a 14" laptop and about 3–4 h of battery. Measure it exactly with
  `POWERMETRICS=1 tools/profile.sh`, which needs sudo, or with `sudo powermetrics --samplers gpu_power -i 1000 -n 5`
  while the app runs.
- After P0+P1–P3+P8: at 60 fps the GPU is ~55–60 % busy, so roughly −40 % GPU energy. Pausing already drops the
  repaint rate to 10 Hz. P13 brings the paused and menu GPU load close to zero.

## 7. Re-running

```bash
cd "."
tools/profile.sh                                   # gpu + bench + app (app opens the window ~90 s)
ONLY=gpu SCENES=rocket,empty,campfire tools/profile.sh
DISPATCH=mirror ./target/release/examples/gpu_profile      # legacy: 16x16 grid for every kernel (default derives grids from @workgroup_size)
MICRO=0 ./target/release/examples/gpu_profile              # skip micro-bench / experiments
XCTRACE=1 ONLY=bench tools/profile.sh                      # Instruments Metal System Trace (occupancy, per-encoder timeline)
```

To test an optimisation before editing a shader, add a `(description, from, to, kernels)` entry to the `experiments`
list in `examples/gpu_profile.rs`. The profiler builds the variant from the current WGSL and times it in interleaved
rounds against the original.
