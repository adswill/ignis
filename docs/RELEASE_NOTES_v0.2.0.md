# Ignis 0.2.0

Adds piston engines, a Windows build, 18 new materials and new demos.

## New

- **Piston engines.** Crank-driven pistons with compression heating, cam-timed valves, spark plugs and fuel injectors. Presets: single, twin and four-cylinder petrol engines, high-compression, lean, race, turbo and Atkinson variants, diesel, two-stroke and an opposed-piston engine.
- **Engine designer** (File menu): choose cycle, cylinders, bore, stroke, clearance, valve and spark timing, mixture, intake pressure and block material, then build and run it.
- **18 new materials:** liquid hydrogen, liquid methane, propane, isopropanol, limestone, quicklime, baking soda, gypsum, concrete, soil, polystyrene foam, boron, zirconium, carbon fibre, brass, bitumen, coolant, engine oil.
- **New demos:** propane tanks in a fire, cryogenic fuels, metal powder flash race, fire barrier test, polystyrene fire, tank leak, hot oil pots.
- **Windows (x86-64) build.**

## Install

- macOS: open the `.dmg`, drag Ignis to Applications, then right-click and Open the first time (unsigned), or run `xattr -dr com.apple.quarantine /Applications/Ignis.app`.
- Windows: extract the `.zip` and run `Ignis.exe`; choose More info, then Run anyway if SmartScreen warns (unsigned).

## Known issues

- The Windows build is cross-compiled and has had little testing; please report problems.
- Diesel engine output is weak, and the two-stroke misfires on its first cycles.
- Some demos (hot pots, propane tank burst) are slow; raise the simulation speed.
