# IGNIS physics validation (science audit, 2026-10-28)

Read-only audit of the simulator against real-world reference data. Nothing in `src/` was edited. Every number below
was measured with the **real game shaders** (`src/sim.wgsl` + `src/matter.wgsl` + `src/vis.wgsl`, snapshot taken
2026-10-28 after the physics team fused `turb` into `post`) by a standalone harness:

```bash
cd "."
cargo build --release --example physval
./target/release/examples/physval <test> [args]      # tests listed at the top of examples/physval.rs
#   tad | flame <phi> | flamesweep [tmax] | autosweep | cvexpl | nozzlesweep | jet <p0> | rocket | rocketp
#   boil [id] | plume [T_src] [g] | meltsweep [T_gas] [tmax] [u_gas] | matter <id,id,..> [open|closed] | turb
# Try a recommended constant WITHOUT editing src (shader text is read at runtime):
CONST="Q_FUEL=485;K_SOOT_RAD=0.0005" ./target/release/examples/physval flamesweep
REPL="old shader text|||new shader text" ./target/release/examples/physval autosweep
SRC_DIR=/some/snapshot/src ./target/release/examples/physval tad          # run against a snapshot
```

The harness builds its own small grids (e.g. 512×8 for 1-D flames, 16×8 for 0-D boxes) with the same bind-group
layout and the same per-step dispatch order as `gpu.rs` (gas steps, walls/matter every 4th step, alternating Margolus
pass), uploads exact gas states and reads the fields back. It parses `@workgroup_size` and the entry-point list from
the shader text, so it keeps working while kernels are fused/re-tiled.

## 0. Unit scaling used for every comparison

From `ARCHITECTURE.md` and `app.rs` (`MS_PER_UNIT`, `MPS_PER_UNIT`, `KGS_PER_UNIT`, `KN_PER_UNIT`):

| quantity | 1 sim unit = | check |
|---|---|---|
| length | 1 cell = 1 cm | |
| temperature | 300 K | |
| pressure | 1 atm | |
| velocity | sqrt(R_air·300 K) = 293 m/s | sim air sound speed sqrt(1.4)=1.183 → 347 m/s ✓ |
| time | 1 cm / 293 m/s = 34 µs | |
| density | 1.177 kg/m³ (air, 300 K, 1 atm) | |
| specific energy (e = cv·T) | R_air·300 K = 86.1 kJ/kg | Q_FUEL=360 → 31 MJ/kg (real CH4 LHV 50 MJ/kg; see §1) |
| mass flow per cell face | 1.177·293·(0.01 m × 1 m depth) = 3.45 kg/s | `KGS_PER_UNIT = 3.5` ✓ |
| gravity `GRAVITY = 0.0015` | 0.0015 cm/(34 µs)² = 13 000 m/s² | **1320 × real g** (deliberate, see §11) |
| species R | CH4 1.806, O2 0.903, N2 1.032, products 1.112 (×287 J/kg K) | molar masses 16, 32, 28, 26 g/mol ✓ (real CO2+2H2O mix 26.7 ✓) |
| γ | CH4 1.31, O2/N2 1.40, products 1.25 (constant cv per species) | |

Playback: `TPS = 32` sim time units per real second, i.e. the game runs at **1/919 of real time** at 1× speed.

## Summary scorecard

| # | Phenomenon | Verdict | Biggest fix |
|---|---|---|---|
| 1 | Adiabatic flame T (open flame, const. p) | ❌ 1733–1790 K vs real 2226 K (−440 K) | `Q_FUEL 360 → 485` |
| 2 | Laminar flame speed vs φ; turbulent S_T | ⚠️ ×20 too fast (expected: unresolved flame); φ-trend too flat; S_T/S_L 2.7–5.7 at u'/S_L 6–17 ✓ but dips at low u' | smooth `T_EDC` switch |
| 3 | Flammability limits | ❌ 7.3–12 % vs real 5–15 % | `Q_FUEL 485` fixes lean limit (5.4 %); rich gate + pyrolysis gate for the rich limit (§3) |
| 4 | Autoignition | ❌ ignites at 540 K in 50 ms; real AIT 810 K, minutes | gate + steeper Arrhenius |
| 5 | Constant-volume explosion | ⚠️ 7.5 atm (theory, sim gas) vs 8.9 atm real; rich mixtures lose most heat to soot radiation | `K_SOOT_RAD 0.004 → 0.0005` |
| 6 | Choked nozzle ṁ vs p0 | ✅ linear, Cd ≈ 0.98 vs chamber stagnation p | ❌ inlet tiles lose 25–30 % stagnation pressure |
| 7 | Shock diamonds | ✅ first cell within 0–15 % of Prandtl planar theory; ⚠️ train dies after 1–2 cells | vents/edges back-pressure 1.2–1.5 atm |
| 8 | CH4/O2 rocket | ✅ c* 1643 m/s (sim theory 1600, real ~1750 at O/F 4.7), Isp_eff 250 s @ ε 4, ṁ ∝ Pc; ❌ no flame at O/F ≤ 4 | injector flame holder |
| 9 | Thermite / black powder / APCP | ✅ flame temperatures and gas yields; ❌ every reaction front moves exactly 1 cell per wall step = 2600 m/s | per-element spread rate |
| 10 | Boiling | ⚠️ exposed surface plateaus at 373–380 K ✓, but buried water superheats to 770 K; latent heat 3.3× real | boil unexposed cells; `E_LATENT[19] 6.0 → 1.8` |
| 11 | Buoyant plume | ✅ B conserved, spread ≈ 0.11 z; ⚠️ w/B^⅓ 1.2–3.1 still accelerating (real 1.7–2.0) | — (time-scale table) |
| 12 | Wall melting | ❌ melting points use °C numbers as if K for Al/Cu/Ti/steel; at 1 atm only aluminium can ever melt (one-way wall radiation) | `M_TMELT`, gas→wall radiation |
| 13 | Heat-capacity lookup bug | ❌ elements ≥ 26 use ash's cap/conductivity in `wall()` | use `m_cap()/m_cond()` |

## Change list for the owners (ordered by impact / effort)

| # | owner | file | change | evidence |
|---|---|---|---|---|
| 1 | phys | `src/sim.wgsl` | `const Q_FUEL: f32 = 485.0;` (was 360) | §1, §3: Tad 1733 → 2231 K (real 2226), lean limit 7.3 % → 5.4 % (real 5 %) |
| 2 | phys | `src/sim.wgsl` | `const K_SOOT_RAD: f32 = 0.0005;` (was 0.004) and in `post` `room = ... * 0.02` (was `* 0.5`) | §5: rich gas no longer cools below its start T |
| 3 | phys | `src/sim.wgsl` | `M_TMELT` = 5.8, 3.11, 4.53, 6.4, 5.3, 9.5, 4.53 for ids 0–6; `M_TSOFT` = 2.6, 1.7, 2.0, 2.5, 3.3, 7.0, 2.0 | §12: °C-as-K bug |
| 4 | phys | `src/sim.wgsl` | `wall()`/`post()`: use `m_cap(id)`/`m_cond(id)` with the raw element id instead of `M_CAP[mat_of(i)]` | §13.1: elements ≥ 26 read ash's heat capacity |
| 5 | chem | `src/matter.wgsl` | per-element `E_SPREAD` + probabilistic contact ignition (`rnd < dt*E_SPREAD[e]`) | §9: every front 2600 m/s; open-air APCP 100–300 atm |
| 6 | phys | `src/sim.wgsl` | `const T_ACT: f32 = 60.0; const A_PRE: f32 = 282000.0;` and gate `if (tt > 2.7)` (was 1.5) in `post` | §4: no more 540 K autoignition; flame speed unchanged (7.25 vs 7.49 m/s) |
| 7 | phys | `src/sim.wgsl` | inlets 2/3/4 = stagnation (p0,T0) boundary; vents 7 = static-pressure outlet | §6, §7: 25–30 % inlet loss, 0.2–0.5 atm vent back-pressure |
| 8 | chem | `src/matter.wgsl` | boil unexposed liquid above `tb` (latent progress + steam burst); `E_LATENT[19]` 6.0 → 1.8 (+ table §10) | §10: buried water at 770 K |
| 9 | phys | `src/sim.wgsl` | `RAD` 4e-5 → 4e-6 (or gas back-radiation) + wall fusion latent heat | §12: steel can't melt in a 3000 K flame at 1 atm |
| 10 | phys | `src/sim.wgsl` | rich partial-oxidation heat factor + rich-limit gate in `reaction_rate` | §3: rich limit |
| 11 | phys | `src/sim.wgsl` | smooth `T_EDC` switch; injector flame holder | §2b, §8 |

All of the constant changes can be previewed without editing anything: e.g.
`CONST="Q_FUEL=485;K_SOOT_RAD=0.0005;T_ACT=60;A_PRE=282000" ./target/release/examples/physval flamesweep`.

---

## 1. Adiabatic flame temperature vs φ (methane–air, 300 K, 1 atm)

The sim releases `Q_FUEL·Δfuel` into total energy and uses constant per-species cv. Exact closed-form for the sim's
thermodynamics (`physval tad`) and measured burnt-gas T in a free 1-D flame (`physval flame 1.0`):

| φ | x_CH4 % | sim Tad,p theory (Q=360) | sim Tad,v theory | **real Tad,p** (equilibrium) | sim Tad,p with Q=485 |
|---|---|---|---|---|---|
| 0.5 | 4.99 | 1097 | 1389 | 1480 | 1373 |
| 0.6 | 5.93 | 1235 | 1573 | 1665 | 1560 |
| 0.7 | 6.85 | 1368 | 1747 | 1838 | 1738 |
| 0.8 | 7.75 | 1495 | 1913 | 1997 | — |
| 0.9 | 8.63 | 1616 | 2071 | 2134 | — |
| 1.0 | 9.50 | **1733** | 2222 | **2226** | **2231** |
| 1.1 | 10.35 | 1719 | 2202 | 2211 | — |
| 1.3 | 12.01 | 1691 | 2164 | 2057 | 2174 |
| 1.5 | 13.61 | 1664 | 2127 | 1910 | 2138 |

Measured (free flame, burnt gas 25 cells behind the front, vent outlets): **φ=1: 1786–1827 K** (theory 1733 K + a
slight 1.02–1.05 atm compression); with `CONST="Q_FUEL=485"`: **2244 K** (real 2226 K ✓).

Diagnosis: `Q_FUEL = 360` was evidently calibrated so the **constant-volume** temperature (2222 K) hits the real
**constant-pressure** value. Open flames are constant-pressure, and the sim's products have γ≈1.33 (cp/cv of the
burnt N2+products mix), so open flames come out ~490 K too cold. That makes every open flame dimmer/redder than it
should be and pushes lean mixtures below the EDC switch temperature (§3).

**Recommendation (phys, `src/sim.wgsl`):** `const Q_FUEL: f32 = 485.0;` (gives 2231 K at φ=1, lean limit fixed, §3).
Side effects measured: closed-vessel stoichiometric explosion pressure rises from 7.5 to 9.7 atm (real AICC 8.9 atm,
measured vessels 7.4–8.1 bar) — acceptable; `Q_FUEL = 470` is a compromise (Tad,p 2178 K, p_v 9.45 atm). Rocket
chambers are capped by `T_MAX` anyway. Rich side: sim stays ~230 K too hot (no CO/H2 partial oxidation) — see §3.

## 2. Laminar flame speed vs φ

1-D freely propagating flame, 512×8 cells, vent (1 atm) outlets at both ends, ignition by a 12-cell burnt-gas kernel,
speed = least-squares front speed (product-mass-fraction front) minus fresh-gas velocity ahead of it.

| φ | S_sim units | **S_sim m/s** | S/S(φ=1) sim | real S_L cm/s | real S/S(φ=1) | S/S(1) with Q=485 |
|---|---|---|---|---|---|---|
| 0.60 | — | no flame | 0 | 9 | 0.24 | 0.67 |
| 0.70 | — | no flame | 0 | 17.5 | 0.47 | 0.77 |
| 0.75 | 0.029 | 8.6 (near-limit, noisy) | 1.14 | 22 | 0.59 | 0.83 |
| 0.80 | 0.019 | 5.6 | 0.74 | 26 | 0.70 | 0.84 |
| 0.90 | 0.024 | 7.1 | 0.95 | 33 | 0.89 | 0.94 |
| 1.00 | 0.0256 | **7.5** | 1.00 | **37** | 1.00 | 1.00 (10.8 m/s) |
| 1.10 | 0.022 | 6.6 | 0.87 | 38 | 1.03 | 0.88 |
| 1.20 | 0.020 | 5.8 | 0.77 | 32 | 0.86 | 0.85 |
| 1.30 | 0.019 | 5.5 | 0.73 | 22 | 0.59 | 0.81 |
| 1.50 | — | no flame | 0 | 9 | 0.24 | 0.78 |

Real data: consensus of Vagelopoulos & Egolfopoulos 1998, Park 2011, Varea 2012 (peak 37–38 cm/s at φ≈1.05–1.1).

* **Absolute:** 7.5 m/s vs 0.37 m/s → **20× too fast.** Expected and acceptable: with `D_SPEC = 0.3` the species/heat
  diffusivity is 0.3 cm·293 m/s ≈ 0.9 m²/s (real 2×10⁻⁵), the flame is ~12 cells thick instead of 0.5 mm, and the speed
  is set by EDC + numerical diffusion. A 1 cm grid cannot resolve a real flame. Keep it, but know it: in game time
  (1/919 slow motion) a flame front crawls at 0.8 cm per game second, i.e. it *looks* ~45× slower than reality (§11).
* **Trend:** peak at φ=1.0 (real 1.05–1.1), the curve is far too flat (0.74–0.95 over φ 0.8–1.3; real 0.6–1.0),
  and it drops to zero abruptly instead of tailing. Cause: above `T_EDC` the rate is `C_EDC·D·min(Yf, YO2/4)` with no
  temperature dependence, so speed hardly depends on flame temperature.

**Recommendation (phys, `sim.wgsl reaction_rate`)** — weight the EDC branch by an effective activation term so S_L
follows Tad like a real flame:
```wgsl
w = max(w, C_EDC * (D_SPEC + nu_t / PR_T) * min(s.a.x, 0.25 * s.a.y)
           * min(exp(-30.0 * (1.0 / tt - 1.0 / 8.4)), 1.0));   // 8.4 = 2520 K reference
```
**Tested and rejected** (measured, `Q_FUEL=485; K_SOOT_RAD=0.0005` + this weighting): the lean limit moves back to
φ 0.8, the rich side still never quenches and S/S(1) stays flat (0.88–1.13 over φ 0.8–4). The flat φ-trend is a
consequence of the mixing-controlled EDC closure plus heavy numerical diffusion; a 1 cm grid cannot recover the real
curve with a one-line change, so **do not apply this**. The trend is acceptable for a game once the limits are right
(§3); the practical improvements are §1 (Q) and §3 item 3.

### 2b. Turbulent flame speed

2-D channel 512×64, φ = 1, vents at both ends, initial divergence-free random velocity field (24 modes, wavelengths
8–32 cells, decaying, no forcing), S_T = d(burnt volume)/dt per unit height minus mean fresh-gas velocity
(`physval turb`). Laminar reference in the same geometry S_L = 0.0185 units (5.4 m/s; the 2-D kernel differs from the
1-D test).

| u' set (units) | measured v_rms in fresh gas | u'/S_L | **S_T/S_L sim** | real (Damköhler/Gülder-type correlations) |
|---|---|---|---|---|
| 0 | 0 | 0 | 1.00 | 1 |
| 0.02 | 0.014 | 0.8 | 0.88 | ~1.5–1.8 |
| 0.05 | 0.049 | 2.6 | 0.77 | ~2.5–3.5 |
| 0.10 | 0.117 | 6.3 | 2.69 | ~3–7 |
| 0.20 | 0.323 | 17.5 | 5.69 | ~6–12 |

* ✅ Strong turbulence wrinkles and accelerates the flame by the right order of magnitude (lower end of the data band).
* ⚠️ Weak turbulence (u'/S_L < 3) *slows* the flame by 12–23 %. Real flames speed up. Likely local quenching where eddies
  mix burnt gas with fresh gas and drop the mixture below the hard `T_EDC = 4` switch. Raising Tad (§1) widens the
  margin above `T_EDC`; a smooth switch (`w_edc *= smoothstep(T_EDC - 0.5, T_EDC + 0.5, tt)`) instead of the hard
  `if (tt > T_EDC)` would remove the cliff (phys, `reaction_rate`).

## 3. Flammability limits

From the sweep in §2 (a flame "exists" if it propagates >30 cells away from the kernel at a steady speed):

| | lean limit | rich limit |
|---|---|---|
| **real** CH4–air (1 atm, 300 K) | **5.0 % (φ 0.50)** | **15 % (φ 1.67)** |
| sim, current | 7.3 % (φ 0.75) | 12–13.6 % (φ 1.3–1.5) |
| sim, `Q_FUEL=485` | **5.4 % (φ 0.55)** ✓ | > 30 % (φ 4) — never quenches ❌ |
| sim, `Q_FUEL=485; K_SOOT_RAD=0.0005` | 5.4 % | > 30 % ❌ |
| sim, + rich partial oxidation + rich gate (items 3, 4 below) | ≤ 5.0 % ✓ | 12–13.6 % (but φ ≥ 2.5 re-lights, see item 4) |

Why: the lean limit sits exactly where the burnt gas can no longer stay above `T_EDC = 4` (1200 K) through the
preheat zone (sim limit flame T ≈ 1430 K ≈ the real limit flame temperature ~1450–1500 K!). So once Tad is right
(§1) the lean limit is right. The current *rich* limit is an accident of soot radiation: rich mixtures pyrolyse into
soot that radiates without bound (§5); with radiation tamed, rich mixtures burn at any φ because O2-limited complete
combustion still releases the full stoichiometric heat per O2 and the EDC rate has no T-dependence.

**Recommendations:**
1. `Q_FUEL = 485` (§1) – fixes lean limit.
2. ~~EDC temperature weighting~~ — tested, does not create a rich limit and breaks the lean one (§2).
3. Rich partial oxidation (phys, `post` combustion block): real rich flames stop at CO + H2, so heat per kg O2 falls
   with φ. Cheapest form: `s.b.z += Q_FUEL * db * mix(1.0, 0.65, clamp((s.a.x / max(0.25 * s.a.y, 1e-6) - 1.0), 0.0, 1.0));`
   (local φ from the cell's own fuel/O2 ratio; 0.65 ≈ heat of CH4→CO+2H2O relative to complete combustion).
   Measured (with items 1 + §5): rich speeds now fall like real ones (S/S(1) 0.64 / 0.61 / 0.61 at φ 1.1 / 1.2 / 1.3,
   real 1.03 / 0.86 / 0.59) but flames still run to φ = 4.
4. Rich-limit gate (phys, end of `reaction_rate`): `w *= 1.0 - smoothstep(1.8, 2.1, s.a.x / max(0.25 * s.a.y, 1e-6));`
   (no reaction where the local mixture is > ~1.9× stoichiometric; diffusion flames are unaffected because they burn at
   their stoichiometric surface; a cell starting at φ 1.2–1.8 stops part-way, which also mimics partial oxidation).
   Measured with gate edges (1.6, 1.9) + items 1, 3, §5: **lean limit φ ≤ 0.5 (5 %) ✓, rich limit φ 1.3–1.5
   (12–13.6 %)** — but φ ≥ 2.5 re-ignites, because hot-fuel pyrolysis (`K_PYRO`, ungated) turns fuel into "products",
   lowering the local fuel/O2 ratio back under the gate. Also gate pyrolysis to `tt > 5.0` (was 3.0) or apply it only
   where `s.a.y < 0.02` (truly O2-free). Edges (1.8, 2.1) should put the limit at the real φ ≈ 1.67.

| φ | 0.5 | 0.6 | 0.8 | 1.0 | 1.1 | 1.2 | 1.3 | 1.5 | 1.7 | 2.0 | 2.5 | 4.0 |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| S/S(1), items 1+3+4+§5 | 0.51 | 0.67 | 0.84 | 1 (10.7 m/s) | 0.74 | 0.65 | 0.58 | 0 | 0 | 0 | 0.52 ❌ | 0.49 ❌ |
| real | ~0 (limit) | 0.24 | 0.70 | 1 | 1.03 | 0.86 | 0.59 | 0.24 | ~0 (limit) | 0 | 0 | 0 |
| Tb sim (K) | 987 | 1604 | 1916 | 2214 | 1640 | 1552 | 1527 | — | — | — | | |

## 4. Autoignition

0-D uniform stoichiometric box held at 1 atm (zero-gradient edges), initial T0, delay = time to half the temperature
rise (`physval autosweep`). Real: Petersen-type shock-tube correlation τ = 2.21e-14·exp(22659/T)·[O2]^-1.05·[CH4]^0.33
(mol/cm³, s); AIT (heated flask, minutes) 810 K.

| T0 | sim delay | **real delay** | ratio |
|---|---|---|---|
| 450 K | none (rate gated off below 1.5 = 450 K) | ∞ | ✓ |
| 540 K | 50 ms | ∞ (below AIT) | ❌ |
| 600 K | 13 ms | ∞ | ❌ |
| 690 K | 3.9 ms | ∞ | ❌ |
| 810 K | 0.97 ms | minutes (AIT) | ❌ |
| 900 K | 0.49 ms | ~16 s | ×3·10⁴ |
| 1000 K | 0.24 ms | ~1.4 s | ×6·10³ |
| 1200 K | 0.03 ms (EDC switch) | 36 ms | ×1200 |
| 1350 K | 0.03 ms | 4.9 ms | ×160 |
| 1500 K | 0.03 ms | 1.0 ms | ×33 |
| 1800 K | 0.03 ms | 0.09 ms | ×3 |

Even allowing the sim's general ×20 "flame clock" speed-up, methane–air in the sim is hypergolic far below its real
autoignition temperature: a methane leak touching a 270 °C pipe explodes within 50 ms. The global activation
temperature `T_ACT = 30` (9000 K) is ~2.7× too low (Westbrook–Dryer CH4: 24 400 K = 81 units), and the hard gate is at
450 K instead of the 810 K AIT.

**Recommendation (phys, `sim.wgsl`):**
* `const T_ACT: f32 = 60.0;` with `const A_PRE: f32 = 282000.0;` (= 1900·e⁵, keeps the rate at the flame temperature,
  T = 6, unchanged so flame speeds stay put). Measured: stoichiometric flame 7.25 m/s (was 7.49) — flames unaffected.
  Estimated delays (thermal-explosion scaling τ ∝ exp(T_ACT/T)·T²/(A·T_ACT) from the measured baseline): 540 K → ~50 min
  (was 50 ms), 810 K → ~0.5 s, 900 K → 36 ms, 1000 K → 7 ms (real 16 s, 1.4 s). Real global value `T_ACT = 81`
  (`A_PRE = 9.4e6`) would put 900 K at ~0.9 s.
* raise the gate in `post` from `if (tt > 1.5)` to `if (tt > 2.7)` (810 K AIT) — free.
* keep `T_EDC = 4` (premixed flames need it); if desired raise to 5.0 to delay "instant" burning of 1200–1500 K
  mixtures (not measured — the run was confounded with the rejected EDC weighting and dropped).

## 5. Closed-vessel (constant volume) explosion

0-D box, premix started at 1350 K (so it lights), `physval cvexpl`:

| φ | final T | final p (from 1 atm, 1350 K) | expected (sim theory, no losses) |
|---|---|---|---|
| 0.6 | 2505 K | 1.86 | — |
| 1.0 | 3094 K | 2.31 | 3270 K |
| 1.2 | **1052 K** (below the start!) | **0.78** | ~3230 K |
| 1.5 | **811 K** | **0.60** | ~3180 K |

Rich mixtures crack to soot (`K_PYRO`) and the soot radiates `K_SOOT_RAD·soot·T⁴·10` with no absorption and up to
**half the sensible heat per step** (`room = 0.5·(T−1)·ρcv`). A rich gas cloud therefore cools *below its initial
temperature* — physically impossible. With `K_SOOT_RAD=0.0005`: φ 1.2 → 1928 K/1.43 atm, φ 1.5 → 1507 K/1.10 atm
(still lossy but no longer absurd). Real stoichiometric methane–air from 300 K: AICC 2590 K / 8.9 atm. Sim theory from
300 K (exact, energy-conserving): 2222 K / 7.46 atm (Q=360) or 2889 K / 9.70 atm (Q=485). With
`Q_FUEL=485; K_SOOT_RAD=0.0005`: φ 0.6 2947 K, φ 1.0 3600 K (hits `T_MAX`), φ 1.2 1992 K, φ 1.5 1528 K — rich losses are
then dominated by pyrolysis (`K_PYRO`) turning fuel into inert "products" without heat whenever O2 is short.

**Recommendation (phys):** `const K_SOOT_RAD: f32 = 0.0005;` and cap the per-step radiative loss at
`room = max(tt - 1.0, 0.0) * rho * cvmix(s) * 0.02` (2 % per step instead of 50 %), so soot glows and cools smoke over
seconds instead of quenching flames.

## 6. Choked nozzle: mass flow vs chamber pressure

Planar converging nozzle (chamber half-height 30 → throat 15 cells), air-inlet plenum (tile 4) at p0, T0 = 300 K,
ambient outflow (`physval nozzlesweep`). Theory (choked, γ=1.4): ṁ = 0.6847·p0·A* /sqrt(R T0) in sim units.

| p_set (atm) | ṁ sim | ṁ theory(p_set, A*=16) | ratio | chamber stagnation p measured |
|---|---|---|---|---|
| 1.5 | 7.98 | 16.4 | 0.49 (not choked) | |
| 2.0 | 13.1 | 21.9 | 0.60 | |
| 3.0 | 20.7 | 32.9 | 0.63 | 2.42 (at p_set 3) |
| 4.0 | 27.3 | 43.8 | 0.62 | 3.19 |
| 6.0 | 42.4 | 65.7 | 0.65 | |
| 8.0 | 58.5 | 87.6 | 0.67 | 5.82 |
| 12.0 | 87.3 | 131.5 | 0.66 | |
| 16.0 | 115.9 | 175.3 | 0.66 | |

* **Linearity ✅**: above choking, ṁ/p_set is constant to ±3 % (0.62–0.67).
* **Discharge coefficient ✅**: with the real throat (15 gas cells — the stair-stepped wall eats one row) and the
  *measured* chamber stagnation pressure (p_set 8 → 5.82 atm): theory 0.6847·5.82·15 = 59.8 vs 58.5 → **Cd = 0.98**
  (real smooth converging nozzles 0.97–0.99).
* **Inlet tiles ❌**: the chamber only gets 72–80 % of the set inlet pressure. Inlet tiles are reset each step to gas
  *at rest* at p_set, so the inflow face is an unsteady Riemann expansion, p/p_set = (1 − (γ−1)/2·u/a)^(2γ/(γ−1));
  at M≈0.24 that is 0.71, exactly what was measured (5.58/8). The UI's "inlet pressure" is therefore not the chamber
  pressure, and high-flow feeds lose up to 30 %.

**Recommendation (phys, `post`, tiles 2/3/4):** make inlets a stagnation (p0, T0) boundary: take the velocity of the
downstream gas neighbour (`u_n`, clamp to Mach 0.9 and to the inward direction), then set
`T = T0 / (1 + 0.5(γ−1)M²)`, `p = p0·(T/T0)^(γ/(γ−1))` with that velocity, instead of `gas_state(..., p_set, 1.0)` at
rest. Same species. This makes "inlet pressure" = chamber stagnation pressure.

## 7. Shock diamonds (under-expanded planar jet)

Same nozzle, ambient vents on the far edges (`VENT=1 physval jet <p>`), axis pressure profile, first shock-cell length
from the nozzle exit to the first pressure maximum. Theory: planar Prandtl/Pack s = 2h·sqrt(Mj²−1), h = 15 cells, Mj
from the *effective* NPR (measured chamber stagnation pressure).

| p_set | NPR_eff | Mj | theory s (cells) | **sim first cell** | error | cells visible |
|---|---|---|---|---|---|---|
| 3 | 2.42 | 1.20 | 19.7 | 17 | −14 % | 2 (17, 15) |
| 4 | 3.19 | 1.40 | 29.4 | 25 | −15 % | 2–3 |
| 5 | 3.97 | 1.55 | 35.7 | 32 | −10 % | 1 |
| 6.5 | 5.14 | 1.73 | 42.2 | 42 | 0 % | 1 + terminal normal shock |

* First-cell length and its growth with pressure ratio ✅ (−15 … 0 %; real planar jets sit slightly *above* linear
  theory at high NPR, the sim does the same).
* ⚠️ The train dies after 1–2 cells and ends in a strong normal shock at x ≈ exit + 4–5 h, behind which the pressure is
  **1.2–1.5 atm**, not 1 atm, even with vent tiles. Real jets show 5–10 decaying diamonds. Two causes:
  1. **Vent tiles back-pressure** (same mechanism as §6, reversed): a vent is "air at rest, 1 atm", so gas flowing *into*
     it must be compressed to stop: p ≈ (1 + (γ−1)/2·u/a)^(2γ/(γ−1)) → 1.3–1.4 atm at M 0.25. Domain edges (cid clamp
     = zero-gradient) let the domain slowly pressurise too (1.16 atm without vents).
  2. MUSCL minmod limiter + LES viscosity damp the weaker downstream cells.

**Recommendations (phys):**
* vent tiles (7): static-pressure outlet — copy the interior neighbour's velocity and composition, set p = 1 atm
  (keep ambient air only where flow goes *into* the domain).
* domain edges: relax edge cells toward 1 atm each step (`p ← p + 0.05(1 − p)`), i.e. a cheap non-reflecting far field.
* optional sharper limiter (van Leer / MC instead of `minmod4/3`) for longer diamond trains.

## 8. CH4/O2 rocket chamber, c* and Isp trend

Layout copied from `scene.rs rocket()` (showerhead injector, 1 CH4 row per N rows, igniters behind the face,
cooled-copper liner, sin-contoured throat 10 cells, bell to area ratio 4), feed ramped like the bench, chamber values
averaged over x 60–85, c* = p0,c·A*/ṁ with ṁ measured at the throat (`physval rocket`, `physval rocketp`).

**Sim theory** (complete combustion, sim cv/R, `T_MAX` cap): Tc capped at 3600 K for O/F 2.4–5.4, γ 1.25–1.26,
c* = 1676 (O/F 2.5) → 1626 (O/F 4) → 1594 m/s (O/F 5). **Real CH4/LOX** (NASA CEA, ~10 atm): Tc peak ≈ 3500–3550 K
at O/F 3.5–3.8, c* peak ≈ 1850 m/s at O/F ≈ 3.0–3.3, ≈ 1750 m/s at O/F 4.7; best Isp runs fuel-rich (O/F 3.2–3.5).

| injector (p 12 atm) | O/F measured | Pc (atm) | Tc (K) | c* (m/s) | u_exit (m/s) | Isp_eff (s) | verdict |
|---|---|---|---|---|---|---|---|
| 1 in 2 rows CH4 | 0.91 | 8.9 | 738 | 814 | 1027 | 112 | flame-out (very rich) |
| 1 in 3 | **3.19** | 8.7 | **889** | 767 | 930 | 99 | ❌ flame-out at the *real optimum* O/F |
| 1 in 4 (game preset) | 4.68 | 8.5 | 3065 | **1643** | 2093 | **250** | ✅ c* = sim theory (1600), −6 % vs real 1750 |
| 1 in 5 | 8.4 | 8.8 | 2277 | 1350 | 1810 | 210 | lean, plausible trend |
| 1 in 6 | 12.9 | 8.9 | 1776 | 1165 | 1404 | 158 | lean, plausible |
| 1 in 8 | 17.3 | 5.6 | 1091 | 962 | 1053 | 95 | |

Chamber pressure sweep (1 in 4 rows):

| p_inj | Pc | Tc | ṁ | ṁ/Pc | c* |
|---|---|---|---|---|---|
| 4 | 3.7 | 2876 K | 12.7 | 3.43 | 855 (O/F 11.8: fuel feed starved at low ΔP) |
| 8 | 6.8 | 3124 K | 12.6 | 1.84 | 1589 |
| 12 | 8.5 | 3065 K | 15.2 | 1.78 | 1643 |
| 16 | 10.4 | 2968 K | 18.9 | 1.82 | 1607 |
| 24 | 26.1 | 2658 K | 33.6 | 1.29 | 2279 (unsteady/chugging during the averaging window) |

* ✅ ṁ ∝ Pc (1.78–1.84 over Pc 6.8–10.4 atm), c* and Isp (250 s at ε = 4, real CH4/LOX ε 4 ≈ 240–260 s) are right
  for a stoichiometric/lean engine. Chamber pressure is only ~70 % of the injector setting (inlet-tile loss, §6).
* ❌ **Engines at or below stoichiometric O/F do not hold a flame** in this injector: 1-in-3 rows (O/F 3.2) runs at
  890 K with unburnt CH4+O2 leaving the throat. Re-measured with `Q_FUEL=485; K_SOOT_RAD=0.0005`: 1-in-3 now gives
  O/F 3.99 but still only 1130 K, 1-in-4 gives O/F 5.26, 3177 K, c* 1670 m/s, Isp_eff 293 s. So it is a
  **flame-holding** limit (chamber flow outruns the ~7–11 m/s sim flame unless the mixture near the igniters is lean),
  not soot. Real engines run rich (O/F 3.2–3.5) for best Isp, so a player building a "realistic" engine gets a dud.
  Recommendations (phys): (a) let the igniter keep heating while the chamber burns (it already does) **and** make the EDC
  branch use `T_EDC` as a smooth switch (§2b) so partially mixed rich pockets near the igniter don't drop out;
  (b) cheapest robust fix: inside `post`, treat any gas cell with T > 6 (1800 K) touching an *inlet* (2/3) cell as an
  "injector flame holder" (pilot) — set `igniter = true` there. Verify with `physval rocket` (row 1-in-3 should exceed
  3000 K).
* Products are a single species with M = 26 g/mol; real CH4/O2 exhaust (CO, H2, H2O, CO2) is ~20–22 g/mol, which is why
  the sim c* tops out ~10 % low. Optional (phys): products `RS.w` 1.1115 → 1.30 and `CV.w` 4.446 → 5.2 (keeps γ≈1.25)
  raises c* by √(1.30/1.11) = 8 % — but it changes every product-gas density in the game, so treat as a later tuning.

## 9. Thermite, black powder, APCP (condensed phase)

32×16-cell blocks (checkerboard/stripe mixtures), left 2 columns preheated to 1800 K, open air, `physval matter`.

| mixture | ids | peak T sim | **real** | gas yield sim | real | front speed sim | **real burn rate** | peak p (open air) |
|---|---|---|---|---|---|---|---|---|
| gunpowder element | 57 | 2190 K | 2300–2600 K | 0.45 | 0.43 | **2600 m/s** | ~1 cm/s (pressed), m/s (loose train) | 54 atm |
| KNO3+charcoal+S mix | 13,11,12 | 2190–2635 K | 2300–2600 K | 0.45 | 0.43 | **2600 m/s** | same | 31 atm |
| thermite Fe2O3+Al | 17,15 | 3120 K | 3135 K (Fe b.p. limited) | 0 (molten iron ✓) | ~0 | **2600 m/s** | 1–10 cm/s | 7–9 atm (trapped hot air) |
| AP + Al (APCP-like) | 16,15 | 3450 K | 3300–3500 K | 0.9 | ~1 | **2600 m/s** | 0.3 cm/s @1 atm, ~1 cm/s @70 atm (n≈0.35) | **100–320 atm** |
| AP + sugar | 16,14 | 3000–3500 K | ~2600–3000 K | 0.9 | ~1 | **2600 m/s** | ~0.1–0.3 cm/s | 80–320 atm |

✅ Flame temperatures, gas yields, ignition temperatures (BP 690 K vs real 700–740 K; thermite 1350 K) and the
pressure exponents (`pow(p, 0.4)` pairs, `pow(p,0.5)` E_SELF, `N_PROP = 0.35` walls; real 0.3–0.5) are realistic.

❌ **Every condensed reaction front moves exactly one cell per wall step** (2 cells per 8 gas steps, measured for all
five mixtures): 1 cm / 0.112 units = **2600 m/s** — detonation speed for a deflagration. Cause (matter.wgsl
`matter_update`): ignition is triggered by *contact* (`hot_partner`, `tn_max - 1.0` and `gt_max` thresholds) and the
cell then jumps to flame temperature within one update (`t += (best.z - t) * min(1, dt*r*6)`), so the next cell lights
on the next pass. Because each cell then dumps `MATTER_MASS · yield` = 9 gas-mass units at 3450 K into a 1 cm cell
within ~2 wall steps, open-air APCP produces 100–300 atm "explosions" (real: a calm 0.3 cm/s burn).

**Recommendation (chem, `src/matter.wgsl`):** add a per-element spread rate `E_SPREAD` (cells per time unit) and make
contact ignition probabilistic: replace the immediate contact trigger by
```wgsl
let lit_by_contact = (hot_partner > best.x || tn_max - 1.0 > best.x) && rnd < dt * E_SPREAD[e]; // dt = wall_dt
```
(and the same for the `E_SELF` branch). Mean front speed = `E_SPREAD` cells/unit (max 8.9 = current behaviour).
Targets using the sim's own ×20 flame-speed factor (sim m/s = real m/s × 20; cells/unit = sim m/s ÷ 2.93):

| element | real reference | E_SPREAD (cells/unit) | looks like in game (1×) |
|---|---|---|---|
| flash powder (47+15/39) | ~300 m/s unconfined | 8.9 (unchanged) | instant flash ✓ |
| loose gunpowder train (57 / 13+11+12) | ~1–3 m/s | 0.15 | 5 cells per game second, visible fizzing train |
| fuse (65) | 1 cm/s | 0.01 (gameplay floor) | 1 cell / 3 s |
| APCP / rocket candy (16+15, 16+14, 13+14) | 0.3–1 cm/s × p^0.35 | 0.02 × p^0.35 | grain burns over seconds, pressure-dependent |
| thermite (17+15, 59+15) | 1–10 cm/s | 0.05 (Cu thermite 0.5) | slow white-hot pour ✓ |
| nitrocellulose (48) | ~0.5–1 m/s | 0.1 | |

Also: `E_SELF[57].z` (gunpowder flame T) 7.3 → 8.0 (2400 K).

## 10. Boiling and latent heat

20×6-cell water pool (id 19) in a steel tray on an igniter strip, gravity on (`physval boil 19`):

| time | liquid cells | mean T | min T | **max T** | remaining mass |
|---|---|---|---|---|---|
| 8 ms | 120 | 328 K | 304 | 378 | 1.00 |
| 19 ms | 118 | 397 | 355 | 525 | 0.96 |
| 31 ms | 107 | 449 | 378 | **695** | 0.91 |
| 38 ms | 73 | 469 | 380 | **774** | 0.88 |
| 50 ms | 39 | 413 | 378 | 594 | 0.70 |
| 57 ms | 19 | 397 | 380 | 428 | 0.65 |

* ✅ Cells with a free surface plateau at **377–380 K** while they evaporate (min T column) — the latent-heat plateau
  works where it is applied (real 373 K).
* ❌ Boiling only happens on *exposed* faces (`can_boil && exposed > 0.0`). Water touching the hot tray but under other
  water superheats to **770 K as a liquid** (real: nucleate boiling pins the whole bulk at 373 K). Mean pool T sits
  25–100 K above the boiling point.
* ❌ Latent heat too large: `E_LATENT[19] = 6.0` temperature units per unit mass = 1800 K-equivalent; real
  L/cp = 2257/4.18 = 540 K = **1.8** units. Water takes 3.3× too much energy to boil off relative to heating it.

**Recommendations (chem, `matter.wgsl` boiling block):**
* unexposed liquid above `tb`: hold `t = tb` and accumulate the excess as progress, `prog += (t - tb) / E_LATENT[e]`;
  at `prog ≥ 1` emit a steam burst (`flag 1.6`, or swap the cell upward as a bubble). This makes the bulk plateau at
  373 K and gives "rolling boil" bursts for free.
* `E_LATENT`: water 19 → 1.8, brine 29 → 1.8, lye 43 → 1.8, HCl 44 → 1.7, H2O2 45 → 1.5 (L/cp in 300 K units:
  ethanol 21 (838/2.44 = 343 K) → 1.15, kerosene 20 (~250/2.0) → 0.42, LOX 22 (213/1.7) → 0.42, LN2 30 (199/2.04)
  → 0.33, gasoline 31 → 0.5).

## 11. Buoyant plume and the time-scale consistency of the whole sim

12-cell-wide source held at 900 K on a ceramic floor, 192×256 domain, default `GRAVITY = 0.0015`, 80 000 steps
(2240 units = 76 ms sim), `HOLD=400 physval plume 3.0 0.0015 80000`. B = ∫ g(1 − ρ/ρ_a) w dx (buoyancy flux per unit
length). Planar (line) plume theory: w_c = C·B^(1/3) with C ≈ 1.66–2.0 (Rouse, Yih & Humphreys; Lee & Emmons),
constant with height; half-width b ≈ 0.12–0.16 z.

| height z (cells) | w_c (units) | T_c | B | **w_c / B^(1/3)** (theory 1.66–2.0) | half-width (theory 0.12–0.16 z) |
|---|---|---|---|---|---|
| 10 | 0.077 | 419 K | 2.9e-4 | 1.15 | 5 (1.2–1.6) |
| 20 | 0.119 | 371 K | 3.6e-4 | 1.67 | 5 (2.4–3.2) |
| 40 | 0.156 | 342 K | 3.9e-4 | 2.14 | 7 (4.8–6.4) |
| 60 | 0.178 | 333 K | 4.0e-4 | 2.41 | 8 (7–10) |
| 90 | 0.238 | 325 K | 4.6e-4 | 3.09 | 10 (11–14) |

* ✅ Buoyancy flux is conserved with height (2.9 → 4.6e-4, the rise is the re-heated source), the sign/structure of the
  buoyancy term (`g·(ρ − p)` = force relative to cold air at local pressure, no hydrostatic imbalance) is correct,
  methane (ρ = p/(1.806 T)) rises ✓.
* ⚠️ w_c/B^(1/3) is 1.2–3.1 and still *accelerating* with height (real line plumes reach a constant w after a few source
  widths): the plume stays laminar-ish because the 1 cm grid + Smagorinsky `CS2 = 0.06` damps the entrainment eddies.
  Spread rate is roughly right (half-width within ~30 % of 0.12–0.16 z above z = 40).
* Front rise ~0.4 × centreline speed (starting-plume cap, real ≈ 0.5–0.6) ✓.

**Time-scale consistency (the "like real life" check).** Each process has its own speed-up relative to reality; on
screen everything is additionally slowed by 919× (TPS = 32). Ideally all apparent factors would be similar:

| process | sim speed vs real | apparent on screen at 1× | comment |
|---|---|---|---|
| sound, shocks, nozzle flow | 1× | 1/919 (slow motion) | exact physics |
| laminar flame speed | 20× | 1/46 | unresolved flame (§2) |
| buoyant rise (thermals, √(gL)) | √1320 = 36× | 1/25 | `GRAVITY` = 1320 g |
| line plume velocity ((gB)^1/3) | ≈ 11× | 1/84 | |
| wall heat conduction (α steel 0.083 units = 0.25 m²/s vs 1.2e-5) | 20 000× | **22× faster than real** | walls glow/melt in game-seconds |
| condensed-phase fronts (§9) | 2600 m/s vs ~1 cm/s | **~280× faster than real** | grid-locked |
| boiling a 72 L (20×6×100 cm) pool | 57 ms sim | 52 game-seconds | latent heat 3.3× too big (§10) |

Flames vs buoyancy are within ~2× of each other (good: candle/campfire shapes look right); solids heating and the
condensed-phase spread are the outliers. **Recommendation:** keep `GRAVITY`; fix §9 (spread rate); optionally scale
`M_COND`/`E_COND` down 4× and `HT` down 2× so walls heat on the same apparent clock as flames (gameplay call for
the phys owner — melting would then take ~4× longer in game time).

## 12. Wall melting times vs material

8×8-cell wall block in a furnace of 3000 K combustion products at 1 atm (gas re-imposed every 40 steps), run for
2500 units = 85 ms sim (`physval meltsweep 10 2500 <u_gas>`).

**Melting/softening points in `sim.wgsl` (`M_TMELT`, `M_TSOFT`, 300 K units) vs real:**

| id | material | M_TMELT now | = K | **real Tm** | recommended M_TMELT | M_TSOFT now (K) | real strength loss ~50 % | recommended M_TSOFT |
|---|---|---|---|---|---|---|---|---|
| 0 | steel | 5.0 | 1500 | 1700–1790 K | **5.8** | 1.7 (510 K) | ~870 K | **2.6** |
| 1 | aluminium | 2.2 | **660** (= its m.p. in °C) | **933 K** | **3.11** | 1.0 (300 K!) | ~520 K | **1.7** |
| 2 | copper | 3.6 | **1080** (≈ 1085 °C) | **1358 K** | **4.53** | 1.6 (480 K) | ~620 K | **2.0** |
| 3 | titanium (Ti-6Al-4V) | 5.6 | **1680** (≈ 1668 °C) | **1878–1941 K** | **6.4** | 2.0 (600 K) | ~750 K | **2.5** |
| 4 | inconel 718 | 5.4 | 1620 | 1533–1609 K ✓ | 5.3 | 3.3 (990 K) ✓ | ~975 K | 3.3 |
| 5 | ceramic | 9.5 | 2850 | alumina 2345 K / zirconia 2988 K | 7.8 if alumina, keep if zirconia | 7.0 | | |
| 6 | cooled copper | 3.6 | 1080 | 1358 K | **4.53** | 1.6 | | **2.0** |

Aluminium, copper and titanium (and roughly steel) use their melting point **in °C** as if it were in 300 K units.
Same in matter.wgsl if molten metal (24) re-solidifies at `M_TMELT[orig] - 0.05` — it follows automatically.

**Measured melting (current constants):**

| material | stagnant 3000 K gas | 3000 K gas at 586 m/s | with `RAD = 4e-6` (586 m/s) |
|---|---|---|---|
| steel | never (surface 1033 K) | never (1355 K) | **83 ms** |
| aluminium | 26 ms | 15 ms | 15 ms |
| copper | never (1077 K) | 47 ms | 44 ms |
| titanium | never (1121 K) | never (1430 K) | 84 ms (first cells) |
| inconel | never (955 K) | never (1276 K) | never (1373 K, rising) |
| ceramic | never (890 K) | never (1205 K) | never (1283 K) |
| cooled copper | never (301 K) | never (301 K) | never |

* ✅ Order is right (Al < Cu < steel ≈ Ti < Inconel < ceramic; regeneratively cooled copper survives).
* ❌ At 1 atm, steel/Ti/Inconel can never melt in a 3000 K flame because wall radiation is **emission only**
  (`dq -= RAD·(T⁴−1)·exposed`): in the sim the radiative loss at 1500 K is 0.77 × the convective gain from a 3000 K, 586 m/s
  gas; in reality (h ≈ 2000 W/m²K, ε 0.8) it is ≈ 0.08. Walls only melt in the game where the gas is dense (high
  pressure chambers), which is why rockets work and torches on steel don't.
* ❌ No latent heat of fusion for walls (Al: L/cp = 397/0.9 = 440 K ≈ 1.5 units of extra heating; steel 1.1, Cu 0.9)
  — a wall melts the instant it touches `M_TMELT`.

**Recommendations (phys, `sim.wgsl`):** the `M_TMELT`/`M_TSOFT` values in the table; `const RAD: f32 = 4e-6;` **or**
better keep RAD and add back-radiation from the adjacent gas, `dq -= dt*RAD*(tw⁴ − ε_g·tg⁴)` per exposed face with
ε_g = clamp(0.1 + 20·soot/ρ, 0.1, 0.9) (sooty/large flames radiate onto walls); and a fusion plateau in `wall()`:
when `ntw ≥ M_TMELT` accumulate `w.y += (ntw − M_TMELT)/M_LFUS[m]` (M_LFUS ≈ steel 1.1, Al 1.5, Cu 0.9, Ti 1.3,
Inconel 1.0, ceramic 3.0) with `ntw = M_TMELT`, and melt when `w.y ≥ 1` (damage and fusion progress then need separate
channels — e.g. reuse `.w`, which is only used by burnables 7–9).

## 13. Code issues found on the way (not tuning — real bugs)

1. **Heat capacity/conductivity of matter elements ≥ 26 is wrong in `wall()`** (`sim.wgsl`): `mat_of()` clamps to 25,
   so `M_CAP[m]`/`M_COND[m]`/`M_COOL[m]` for ice, snow, gasoline, glass, lava, stone, copper oxide, … all read **ash's**
   values (cap 8, cond 0.1). matter.wgsl already provides `m_cap(m)` / `m_cond(m)` (`E_CAP`/`E_COND` tables, 66
   entries) but sim.wgsl doesn't use them. Fix (phys): in `wall()` and in `post()`'s gas↔wall exchange use
   `let mm = (META[i] >> 8u) & 0xFFu;` and `m_cap(mm)`, `m_cond(mm)` (and `mat_of` only for the 0–9 wall tables).
2. **Inlet and vent tiles are "gas at rest" Riemann boundaries** → 25–30 % stagnation-pressure loss at inlets and
   0.2–0.5 atm back-pressure at vents (§6, §7).
3. **Soot radiation can remove up to 50 % of a cell's sensible heat per step** (`room` factor 0.5) → rich gas cools
   below its initial temperature (§5).
4. **Condensed-phase fronts are grid-locked at 1 cell / wall step** (§9).
5. **Wall radiation is emission-only** (`dq -= RAD·(T⁴−1)·exposed`): surrounding hot gas never radiates back, so a wall
   in a 3000 K furnace equilibrates around 1000–1350 K (§12).
