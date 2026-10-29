---
id: heat_transfer
title: Heat transfer
category: Concepts
tags: conduction convection radiation fourier stefan boltzmann
---
# Heat transfer
*Conduction, convection and radiation: the three ways heat moves.*

## At a glance
| Property | Value |
|---|---|
| Conduction (Fourier) | q = -k · ∇T |
| Radiation (Stefan-Boltzmann) | q = ε · σ · T⁴, σ = 5.67×10⁻⁸ W/(m²·K⁴) |
| Conductivity: copper / steel / ceramic tile / air | 401 / ≈50 / ≈0.05 / 0.026 W/(m·K) |
| Radiation at 1500 K vs 300 K | 625× more |

## Conduction
Heat flows through a solid from hot to cold at a rate set by its conductivity. Copper spreads a hot spot; ceramic keeps it local.

## Convection
Moving fluid carries heat away. Forced convection (fans, coolant) is far stronger than natural convection (buoyant plumes).

## Radiation
Every hot thing glows. Radiation grows with the **fourth power** of temperature, so it dominates in big fires and furnaces. It is how a fire heats objects metres away.

## In Ignis
- Walls and matter conduct heat to their neighbours with per-material conductivity and heat capacity; gas exchanges heat with surfaces it touches; soot radiates.

## Try this
Heat one end of a copper bar, a steel bar and a ceramic bar of the same size with the torch for the same time.

**See also:** [[copper]], [[ceramic]], [[regenerative_cooling]], [[flame_colour]]

> Reference values: Incropera & DeWitt, Fundamentals of Heat and Mass Transfer.
