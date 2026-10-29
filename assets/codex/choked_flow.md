---
id: choked_flow
title: Choked flow & nozzles
category: Concepts
tags: de laval nozzle throat mach supersonic rocket
---
# Choked flow & nozzles
*Why a rocket nozzle narrows, then widens.*

## At a glance
| Property | Value |
|---|---|
| Critical pressure ratio (γ = 1.4) | p*/p₀ = 0.528: above ≈1.9 atm into 1 atm the throat chokes |
| Throat Mach number | exactly 1 when choked |
| Choked mass flow | ṁ ∝ A* · p₀ / √T₀ (independent of downstream pressure) |
| Typical exit Mach (rocket) | ≈3-5 |

## Choking
Push gas through a narrowing passage and it speeds up. At the narrowest point (the **throat**) it can reach the speed of sound but no faster: once the upstream pressure is about twice the downstream pressure, the throat is **choked**, and the flow rate depends only on throat area and chamber conditions. Lowering the outside pressure further can't pull more gas through, because pressure signals can't travel upstream through sonic flow.

## The de Laval nozzle
Gustaf de Laval found (1888) that beyond the throat a supersonic flow *speeds up as the passage widens*. A converging-diverging nozzle turns the chamber's thermal energy into a fast, cold exhaust jet. Thrust is

**F = ṁ · vₑ + (pₑ - pₐ) · Aₑ**

A nozzle is **ideally expanded** when exit pressure equals ambient. **Over-expanded** nozzles (sea-level, pₑ < pₐ) show shock diamonds and can separate; **under-expanded** ones (vacuum) leave energy in a ballooning plume. That's why upper-stage nozzles are huge.

## In Ignis
- Narrow throats in your engines really choke: chamber pressure rises until the mass flow out matches the flow in.
- Over-expanded exhaust can show shock-diamond patterns in the **Pressure** view.

## Try this
Build a chamber with a straight 4-cell exit, then reshape it into a converging-diverging nozzle with the same throat. Compare thrust.

**See also:** [[specific_impulse]], [[shock_waves]], [[regenerative_cooling]], [[saint_roberts_law]]

> Reference values: Anderson, Modern Compressible Flow; Sutton & Biblarz, Rocket Propulsion Elements.
