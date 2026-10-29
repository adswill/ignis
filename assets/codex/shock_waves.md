---
id: shock_waves
title: Shock waves & blast
category: Concepts
tags: mach overpressure blast wave sonic
---
# Shock waves & blast
*A pressure jump that travels faster than sound.*

## At a glance
| Property | Value |
|---|---|
| Speed of sound in air | 343 m/s at 20 °C (∝ √T) |
| Normal shock at Mach 2 | pressure ×4.5, temperature ×1.69 |
| Overpressure: eardrum rupture | ≈0.35 atm |
| Overpressure: most buildings destroyed | ≈1 atm (100 kPa) |
| Blast scaling | Hopkinson-Cranz: Z = R / W^(1/3) |

## Why shocks form
Sound waves are small pressure ripples. A large pressure pulse moves faster at its high-pressure peak (the gas there is hotter and moving forward), so the back catches up with the front and it steepens into a near-instant jump: a shock. Across it, pressure, density and temperature leap (the Rankine-Hugoniot relations).

## Blast waves
An explosion makes a spherical shock followed by a negative-pressure phase that sucks debris back. The damage scales with **scaled distance** Z = R / W^(1/3): doubling the distance is like dividing the charge by eight. Reflected shocks off walls can be 2-8× the incident pressure, so explosions in corners and rooms are far more destructive.

## In Ignis
- The solver is shock-capturing: blasts, reflections and Mach stems emerge naturally. Watch them in the **Pressure** view.
- Shocks above an explosive's threshold set it off: this is how chains of charges propagate.

## Try this
Detonate a charge near a wall and one in open space. Compare the reflected pressure in the **Pressure** view.

**See also:** [[deflagration_vs_detonation]], [[pressure_vessels]], [[choked_flow]], [[tamping]]

> Reference values: Kinney & Graham, Explosive Shocks in Air; Anderson, Modern Compressible Flow.
