---
id: saint_roberts_law
title: Saint-Robert's law
category: Concepts
tags: burn rate vieille pressure exponent solid rocket
---
# Saint-Robert's law
*Solid propellant burns faster under pressure: r = a · pⁿ*

## At a glance
| Property | Value |
|---|---|
| Law | r = a · pⁿ (Saint-Robert 1860s, Vieille 1893) |
| Typical exponent n | 0.3-0.5 for APCP; ≈0.35 in Ignis |
| Typical burn rate | ≈0.5-2 cm/s at 7 MPa |
| Stability | requires n < 1 |

## The law
A solid propellant surface recedes at rate **r** that depends on chamber pressure: higher pressure pushes the flame closer to the surface, heating it faster. The exponent **n** is the key design number.

## Why n < 1 matters
Chamber pressure settles where gas made by the burning surface equals gas leaving through the nozzle:

**pₒ = (a · ρ · c* · A_burn / A_throat)^(1/(1-n))**

If n is small, a small change in burning area or throat size changes pressure only a little: stable. As n approaches 1 the exponent 1/(1-n) explodes: a tiny crack in the grain, or a throat partly blocked by debris, sends pressure soaring until the case bursts. Propellant chemists add burn-rate modifiers (like the iron oxide in the Shuttle boosters) to tune a and n.

## In Ignis
- [[solid_propellant]] walls and AP mixes burn at **r ∝ p^0.35**. A smaller throat gives a much higher chamber pressure.

## Try this
Build a solid motor with a 6-cell throat, then shrink it to 3, then 2 cells. Record chamber pressure each time. When does the case fail?

**See also:** [[solid_propellant]], [[choked_flow]], [[apcp_mix]], [[pressure_vessels]]

> Reference values: Sutton & Biblarz, Rocket Propulsion Elements; Vieille (1893).
