---
id: cooled_copper
title: Cooled copper
category: Metals & Ceramics
element: 6
tags: regenerative cooling rocket chamber channel
---
# Cooled copper
*A copper wall with coolant flowing through it: how rocket chambers survive 3500 K.*

## At a glance
| Property | Value |
|---|---|
| Rocket chamber gas temperature | 3300-3600 K (LOX/kerosene or LOX/methane) |
| Liner melting point | ≈1080 °C (copper alloy) |
| Throat heat flux | order of 100 MW/m² in high-pressure engines |
| SSME main chamber | 390 milled coolant channels, liquid hydrogen coolant |

## In the real world
No material survives a rocket's combustion gases by itself. Engines instead run cold propellant through hundreds of channels milled into a thin copper-alloy liner before injecting it. Copper's huge conductivity pulls heat through the liner into the coolant, so the gas-side wall stays below ~800 K while gas inches away is above 3000 K. The propellant arrives at the injector pre-heated, so the energy is not wasted.

**Uses:** Liquid-rocket combustion chambers and nozzles (SSME/RS-25, Raptor, RD-170, Merlin).

**Hazards:** If coolant flow stops even for a fraction of a second the liner melts through (a burn-through).

## In Ignis
- Behaves like [[copper]] but with built-in active cooling that constantly pulls the wall back toward 300 K.
- Strength **3.0**. Survives a full rocket flame as long as the local heat flux is not extreme; the hottest spot is usually the nozzle throat.
- Cooling only works on the cooled cells: put plain steel next to the flame and it will still burn through.

## Try this
Open the rocket scene, replace the chamber liner with plain [[copper]] and fire it. Then switch back to cooled copper. Watch the max wall-heat readout.

**See also:** [[regenerative_cooling]], [[copper]], [[choked_flow]], [[specific_impulse]]

> Reference values: NASA SP-125 (Huzel & Huang, Design of Liquid Propellant Rocket Engines); Sutton & Biblarz, Rocket Propulsion Elements.
