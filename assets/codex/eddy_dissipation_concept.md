---
id: eddy_dissipation_concept
title: Eddy Dissipation Concept
category: Concepts
tags: edc turbulent combustion magnussen mixing arrhenius
---
# Eddy Dissipation Concept
*In a turbulent flame the chemistry is fast; mixing sets the burn rate.*

## At a glance
| Property | Value |
|---|---|
| Origin | Eddy Break-Up (Spalding, 1971); Eddy Dissipation Concept (Magnussen & Hjertager, 1976; Magnussen, 1981) |
| Rate | ω ∝ (ε / k) · min(Y_fuel, Y_O₂ / s) |
| Used in | most industrial CFD codes for fires and furnaces |

## Two clocks
Every flame has a **chemical** time (how quickly molecules react once mixed and hot) and a **mixing** time (how quickly turbulence brings fuel and oxygen together). In hot turbulent flames the chemistry is far faster, so the burn rate is limited by mixing.

## The model
Magnussen's idea: reactions happen in the finest turbulent eddies, where molecular mixing occurs. The reaction rate is proportional to the rate at which those eddies are renewed (the turbulence dissipation rate over kinetic energy, ε/k) times whichever reactant is scarcer. At low temperatures, or in calm gas, the **Arrhenius** chemistry rate k = A · exp(-Eₐ / RT) is the bottleneck instead, which is what makes ignition and extinction possible.

## In Ignis
- Combustion uses an Arrhenius rate blended with an EDC-style mixing rate: above **≈1200 K** the rate is ∝ (molecular + turbulent diffusivity) × min(fuel, O₂/4).
- Turbulence from the LES model therefore makes flames burn faster: stir a flame with fans and it roars.

## Try this
Run two identical burners, one in still air and one with fans creating turbulence around it. Compare the burn readout.

**See also:** [[turbulence]], [[ignition_and_autoignition]], [[premixed_vs_diffusion]], [[methane_combustion]]

> Reference values: Magnussen & Hjertager (1977), 16th Symp. (Int.) on Combustion; Magnussen (1981); Poinsot & Veynante, Theoretical and Numerical Combustion.
