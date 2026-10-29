---
id: specific_impulse
title: Specific impulse & thrust
category: Concepts
tags: isp rocket efficiency exhaust velocity
---
# Specific impulse & thrust
*How much push you get per kilogram of propellant.*

## At a glance
| Property | Value |
|---|---|
| Definition | Isp = F / (ṁ · g₀), in seconds; vₑ ≈ Isp · 9.81 m/s |
| Black powder | ≈80 s |
| Rocket candy | ≈110-130 s |
| Hydrogen peroxide monoprop | ≈160 s |
| Solid APCP (SRB) | ≈242 s SL / 268 s vac |
| Kerosene/LOX (Merlin 1D) | ≈282 s SL / 311 s vac |
| Methane/LOX (Raptor 2) | ≈327 s SL / ≈350 s vac |
| Hydrogen/LOX (RS-25) | ≈366 s SL / 452 s vac |

## Why it matters
The rocket equation, Δv = vₑ · ln(m₀/m₁), makes exhaust velocity the most precious number in rocketry: modest Isp gains save huge masses of propellant.

## What sets it
Exhaust velocity grows with **chamber temperature** and falls with the **molecular mass** of the exhaust: v ∝ √(T / M). Hydrogen-oxygen wins because its exhaust is light water vapour and unburnt hydrogen. A bigger [[choked_flow]] expansion ratio extracts more of the energy, especially in vacuum.

## In Ignis
- The thrust readout measures the momentum and pressure leaving your engine. Divide by propellant flow to compare designs.

## Try this
Build the same engine and run it with air and then with oxygen. Which gives more thrust per unit of fuel?

**See also:** [[choked_flow]], [[stoichiometry]], [[solid_propellant]], [[cooled_copper]]

> Reference values: Sutton & Biblarz, Rocket Propulsion Elements; SpaceX and NASA published engine data.
