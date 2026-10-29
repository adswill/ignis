---
id: methane_combustion
title: Methane combustion
category: Reactions
tags: ch4 burn flame reaction
---
# Methane combustion
*CH₄ + 2 O₂ → CO₂ + 2 H₂O + 802 kJ/mol*

## At a glance
| Property | Value |
|---|---|
| Heat released | 802 kJ per mol CH₄ (LHV), 50 MJ/kg |
| Stoichiometric fuel fraction | 9.5 % by volume in air |
| Flame temperature | ≈2220 K in air, ≈3050 K in O₂ |
| Real mechanism | GRI-Mech 3.0: 53 species, 325 reactions |

## In the real world
The one-line equation hides a chain reaction: radicals like H, O and OH are created, multiply and attack fuel molecules. That is why flames need a minimum temperature (radicals must be made faster than they are destroyed) and why some chemicals (halons, potassium salts) stop flames by mopping radicals up. Detailed mechanisms have hundreds of steps; games and engineering codes usually use a single 'global' step with an Arrhenius rate.

## In Ignis
- A single global step with an Arrhenius rate in temperature, capped by turbulent mixing ([[eddy_dissipation_concept]]).
- Fuel-rich hot pockets crack into [[soot]] instead, which burns later in air and glows orange.

## Try this
Make a premixed jet (fuel and air inlets in the same tube) and a non-premixed one (fuel jet into still air). Compare flame shape and colour.

**See also:** [[methane]], [[stoichiometry]], [[adiabatic_flame_temperature]], [[premixed_vs_diffusion]]

> Reference values: GRI-Mech 3.0 (Smith et al.); Glassman & Yetter, Combustion; Westbrook & Dryer (1981) global mechanisms.
