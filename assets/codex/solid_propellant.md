---
id: solid_propellant
title: Solid propellant (APCP)
category: Burnable Solids
element: 8
tags: apcp rocket motor grain booster
---
# Solid propellant (APCP)
*Fuel and oxidiser mixed into rubber: a rocket motor you can pour.*

## At a glance
| Property | Value |
|---|---|
| Shuttle SRB composition | 69.6 % ammonium perchlorate, 16 % aluminium, 12 % PBAN binder, 2 % epoxy, 0.4 % iron oxide |
| Density | ≈1750-1800 kg/m³ |
| Flame temperature | ≈3200-3500 K |
| Specific impulse | ≈242 s sea level, ≈268 s vacuum (SRB) |
| Burn rate | ≈1 cm/s at ≈7 MPa |
| Pressure exponent n | ≈0.3-0.5 |

## In the real world
Ammonium perchlorate composite propellant (APCP) carries its own oxidiser, so it burns in a sealed casing with no air. It only burns at its exposed surface, and the surface recedes at a rate set by pressure (see [[saint_roberts_law]]). Engineers shape the hole down the middle (star, finocyl, slots) to control how the burning area, and so the thrust, changes over the burn. Once lit it cannot be throttled or stopped.

**Uses:** Space boosters (Shuttle SRB, SLS, Ariane 5/6), missiles, model rocket motors (APCP reloads).

**Hazards:** Burns violently with no air needed; a cracked grain exposes extra area and can over-pressurise the case.

## In Ignis
- Lights at **≈750 K** at exposed surfaces and burns into **≈3000 K** exhaust that is fuel-rich (it afterburns in air).
- Burn rate follows Saint-Robert's law **r ∝ p^0.35**: confine it and it burns faster, so a too-small nozzle runs away.
- Buried cells are heated through and burn slower than exposed ones.

## Recipes
- Mix [[ammonium_perchlorate]] with [[aluminium_powder]] or [[sugar]] powders for a loose APCP-like mix: see [[apcp_mix]].

## Try this
Make a motor: a steel tube with a solid propellant grain and a narrow throat. Try a throat 2 cells wide, then 4 cells. The narrow one has much higher chamber pressure, and might burst.

**See also:** [[saint_roberts_law]], [[choked_flow]], [[specific_impulse]], [[apcp_mix]], [[ammonium_perchlorate]]

> Reference values: Sutton & Biblarz, Rocket Propulsion Elements; NASA Space Shuttle SRB press kit.
