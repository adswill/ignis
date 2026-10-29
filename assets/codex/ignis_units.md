---
id: ignis_units
title: How Ignis measures things
category: Concepts
tags: units scale grid kelvin atm help
---
# How Ignis measures things
*Every number in the game, translated.*

## At a glance
| Property | Value |
|---|---|
| Grid | 512 × 288 cells, each about 1 cm |
| Temperature unit | 1 = 300 K (room temperature); 10 = 3000 K |
| Pressure unit | 1 = 1 atm (101.3 kPa) |
| Density unit | 1 = ambient air (≈1.2 kg/m³) |
| Time | 32 simulation time units per real second at 1× speed |

## Reading the readouts
- **Temperatures** are shown in kelvin. 273 K is 0 °C, 373 K is 100 °C, 1000 K is a dull red glow, 3000 K is a rocket flame.
- **Pressure** is in atmospheres. A car tyre is about 3 atm absolute; a rocket chamber 10-300 atm.
- The **views** (Fire, Pressure, Heat, Flow, Gases, Stress) show the same simulation through different lenses. Switch often.

## What is simulated
- The gas is a compressible fluid with four species (fuel, oxygen, inert nitrogen, products) plus soot, solved with a shock-capturing scheme (HLLC fluxes, MUSCL reconstruction, two-stage Runge-Kutta) and a turbulence model.
- Combustion is Arrhenius chemistry blended with turbulent mixing ([[eddy_dissipation_concept]]).
- Walls conduct heat, soften, get stressed by pressure, and fail. Powders and liquids move as a falling-sand cellular automaton and react with neighbours.
- Ignis is a game: numbers are tuned to *feel* right on a 1 cm grid in seconds of real time. Real values are given in each Codex entry so you can compare.

## Try this
Hover over any cell with the inspector and compare the temperature it shows with the Codex values for that material.

**See also:** [[heat_transfer]], [[eddy_dissipation_concept]], [[turbulence]]

> Reference values: Ignis ARCHITECTURE.md.
