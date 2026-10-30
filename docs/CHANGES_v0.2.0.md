# Ignis v0.2.0 - what is new

## Solver and physics
- Conservation accounting fixed: post-pass sources (igniters, fans, wall/matter exchange, soot radiation) are now booked; mass/energy drift < 0.001 % on flare, campfire, srm, rocket.
- Wall model: heat transfer, bursting, melting, regenerative cooling retained; latent-heat reservoir for molten metal.
- Step governor (target 60 fps, up to 192 steps/frame) with learned per-step cost.

## Explosives and propellants
- Fuse chains propagate: dynamite, TNT, C4 and cap-to-C4 chains fire; thermite spreads and ignites from a KClO3+Mg starter.
- Cast propellant burns steadily (5-6 atm for 900+ frames) via a pressure limiter and re-tuned burn coefficients (KNO3/sugar, KNO3/charcoal, APCP).
- Propellant designer: docked panel, log-log burn chart, grain section, thrust preview, Test motor / Build.

## Propulsion demos (18)
turbab, turbofan, ramjet, scramjet, pde, ejector; rk_aerospike, rk_expansion, rk_dualbell, rk_srm_core, rk_srm_star, rk_hybrid, rk_monoprop, rk_tanks, rk_coldgas, rk_rcs, rk_expander, rk_srm_end. Plus 18 showcase scenes (candy, cannon, boiler, bunsen, forge, mine, ...). Run with `ignis --bench N <name>`.

## Materials
- Every element has its own colour, texture style and sparkle; liquids have six variants (thin, viscous, fizz, fume, gel, molten); labelled `material_board` and `mix_demo` scenes.

## Rendering
- Supersonic plumes with shock diamonds, log pressure view with schlieren and legends, inferno speed view, turbulence-modulated fire, embers, blast-wave shadowgraph, anti-aliased walls.

## UI
- Zoom to fit (Shift+F), library filter chips and sorting, per-material thumbnails, template browser, time-scale presets (- / =), status bar with wall:sim ratio, steps/frame, CFL, drift; auto-scaled plots with rolling window and log pressure axis.

## Audio
- Acoustic synthesis from the solver (jet, combustion, crackle, structure, LFE), per-material signatures, distance-delayed booms, mute (M / Shift+M).

## Performance (headless, shared GPU)
- ~6-8 ms per frame at 20 steps (rocket 1.85 s / 300 frames, jet 2.11 s, campfire 1.97 s, material_board 4.0 s); about 0.3-0.4 ms/step.

## QA
tools/qa.sh: 165 pass / 4 fail (rocket late thrust 183 < 250; srm case 88 % and nozzle 89.6 % intact; thermite plate not breached). All 70 scenes run with 0 bad cells and no panics.
