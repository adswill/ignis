---
id: stoichiometry
title: Stoichiometry
category: Concepts
tags: mixture ratio equivalence ratio afr o/f lean rich
---
# Stoichiometry
*Exactly enough oxygen to burn all the fuel, and no more.*

## At a glance
| Property | Value |
|---|---|
| Methane | CH₄ + 2 O₂ → CO₂ + 2 H₂O; 9.5 % in air; air/fuel 17.2 by mass |
| Gasoline | air/fuel ≈14.7 by mass |
| Hydrogen | 29.5 % in air; air/fuel 34.3 by mass |
| Methane / oxygen | O/F = 4.0 by mass |

## Balancing the books
Every carbon atom wants two oxygen atoms (→ CO₂) and every two hydrogen atoms want one (→ H₂O). Counting atoms gives the **stoichiometric** ratio. Because air is only 21 % oxygen, you need about 4.76 volumes of air per volume of O₂ required.

## Lean and rich
The **equivalence ratio** φ = (fuel/oxidiser)actual ÷ (fuel/oxidiser)stoichiometric.
- **φ < 1, lean**: spare oxygen, cooler flame, clean exhaust. Gas turbines run very lean overall.
- **φ = 1**: hottest flame (in fact the peak is slightly rich, φ ≈ 1.05, because of dissociation).
- **φ > 1, rich**: unburnt fuel, CO and [[soot]]. Rockets run slightly rich: lighter exhaust molecules give better [[specific_impulse]].

## In Ignis
- Every fire in Ignis is limited by the fuel/oxygen ratio. Rich clouds burn only at their edges as air mixes in.
- Watch the **Gases** view: green fuel, blue oxygen, the flame lives where they meet.

## Try this
Set up the rocket scene and sweep the O2 feed pressure from low to high. Find the setting with the highest chamber temperature.

**See also:** [[flammability_limits]], [[adiabatic_flame_temperature]], [[methane_combustion]], [[premixed_vs_diffusion]]

> Reference values: Turns, An Introduction to Combustion; Glassman & Yetter, Combustion.
