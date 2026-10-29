---
id: thermal_shock
title: Thermal shock
category: Concepts
tags: crack ceramic glass quench stress
---
# Thermal shock
*When one side of a material gets hot faster than the other can follow.*

## At a glance
| Property | Value |
|---|---|
| Thermal stress | σ ≈ E · α · ΔT / (1 - ν) |
| Soda-lime glass expansion | ≈9 ppm/K (cracks at ΔT ≈ 50-100 K) |
| Borosilicate glass | ≈3.3 ppm/K (survives ≈150-200 K) |
| Fused silica | ≈0.5 ppm/K: survives quenching from red heat |

## Mechanism
Heated material expands. If the surface heats or cools much faster than the interior, the hot part tries to expand while the cold part holds it back, creating tension. Metals yield and stretch a little; brittle materials like ceramics and glass can't, so a crack runs through.

## Resistance
A material survives thermal shock best with high strength, high thermal conductivity (to even out gradients), and low stiffness and expansion. That's why Pyrex (borosilicate) replaced soda-lime for kitchen and lab glass, and why Space Shuttle tiles were made of extremely low-expansion silica.

## In Ignis
- [[ceramic]] walls crack if the temperature difference to a neighbour exceeds **≈660 K** (2.2 units). Heat them gradually.
- Metals don't suffer thermal shock in the game, but they soften and melt.

## Try this
Heat a ceramic block slowly with a small methane flame until it glows, then chill one side with the chill tool.

**See also:** [[ceramic]], [[glass]], [[heat_transfer]]

> Reference values: Kingery (1955), J. Am. Ceram. Soc. 38; Hasselman (1969).
