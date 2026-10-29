---
id: adiabatic_flame_temperature
title: Adiabatic flame temperature
category: Concepts
tags: flame temperature dissociation
---
# Adiabatic flame temperature
*How hot a flame gets if none of its heat escapes.*

## At a glance
| Property | Value |
|---|---|
| Methane-air | ≈2220 K |
| Propane-air | ≈2270 K |
| Hydrogen-air | ≈2380 K |
| Acetylene-air | ≈2500 K |
| Methane-oxygen | ≈3050 K |
| Hydrogen-oxygen | ≈3080 K |
| Acetylene-oxygen | ≈3480 K |

## The energy balance
All the chemical energy released goes into heating the products. More inert gas to heat (nitrogen in air) means a cooler flame, which is why pure-oxygen flames are about 800 K hotter.

## Dissociation sets a ceiling
Above roughly 2500 K, CO₂ and H₂O start splitting back into CO, H₂, OH, H and O, absorbing energy. So oxy-fuel flames never get as hot as the simple calculation suggests, and rocket chambers top out around 3500-3600 K. Higher pressure suppresses dissociation slightly, which is one reason rocket engines run at high chamber pressure.

Real flames are cooler still because they radiate heat away (especially sooty ones) and lose heat to walls.

## In Ignis
- Fuel-air flames peak near **2200 K**; oxygen flames and energetic mixes go to the temperature cap (**≈3600 K**).
- Sooty flames radiate and run cooler.

## Try this
Use the **Heat** view to compare a methane-air flame with a methane-oxygen flame and an [[apcp_mix]] flame.

**See also:** [[stoichiometry]], [[oxygen]], [[soot]], [[flame_colour]]

> Reference values: NASA CEA (Gordon & McBride) equilibrium calculations; Turns, An Introduction to Combustion.
