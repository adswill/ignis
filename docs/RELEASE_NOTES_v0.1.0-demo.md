# Ignis 0.1.0 (demo)

First public build of Ignis, a research-grade 2D combustion and chemistry simulator for macOS (Apple Silicon).

This is an early demo. It is usable and the core simulation is solid, but it has known rough edges and some planned features are not in yet.

## What is included

- GPU compressible reacting-flow solver with turbulence, buoyancy, methane combustion, soot and smoke.
- Powders, liquids and solids (50+) with emergent chemistry, a Mix tool, and wall materials that heat, burst and melt.
- Docked scientific workspace: materials library, inspector, plots, probes, legends, an in-app encyclopedia.
- Example setups: turbojet, rocket engine, solid motor, flare, campfire, chemistry bench, pressure vessels, demolition.
- Procedural sound.

## Install

1. Download the `.dmg`, open it and drag Ignis to Applications.
2. First launch: right-click the app, choose Open, confirm. (The app is not signed with an Apple Developer ID.) Or run `xattr -dr com.apple.quarantine /Applications/Ignis.app`.

## Known issues

- Some visual effects are still being tuned (very hot flames can look over-exposed).
- Heavy scenes can run slower than real time.
- Only Apple Silicon is supported.

Checksums: see `SHA256SUMS.txt`.

Licence: PolyForm Noncommercial 1.0.0. To support more projects, consider donating here: https://ko-fi.com/adswill
