---
id: pressure_vessels
title: Pressure vessels
category: Concepts
tags: hoop stress burst tank bleve
---
# Pressure vessels
*Why tanks are round and why they fail along their length.*

## At a glance
| Property | Value |
|---|---|
| Hoop stress (thin cylinder) | σ = p · r / t |
| Axial stress | σ = p · r / (2t): half the hoop stress |
| Sphere | σ = p · r / (2t): the most efficient shape |
| Stored energy | compressed gas holds far more energy than the same vessel full of liquid |

## Hoop stress
Pressure inside a cylinder pulls its wall apart around the circumference with stress **p·r/t**. That's twice the stress along the length, so overpressured pipes and sausages split lengthwise. Spheres share the load equally in all directions and need half the wall thickness, which is why high-pressure gas tanks and nuclear containment buildings are spherical.

## Bursts
When a vessel fails, its stored energy goes into a blast wave and fragments. A **BLEVE** (boiling liquid expanding vapour explosion) happens when a tank of liquid kept above its boiling point by pressure ruptures: the liquid flashes to vapour all at once.

## In Ignis
- Walls carry a stress ratio from the pressure difference across them; above their (temperature-dependent) strength they accumulate damage and fail. See the **Stress** view.
- Hot walls are weaker: a vessel that holds 10 atm cold can fail at 5 atm when its walls are glowing.

## Try this
Build a round and a square vessel from the same steel and pump both up with air. Which fails first, and where?

**See also:** [[steel]], [[titanium]], [[shock_waves]], [[vent]]

> Reference values: Roark's Formulas for Stress and Strain; ASME Boiler and Pressure Vessel Code.
