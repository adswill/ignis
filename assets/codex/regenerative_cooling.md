---
id: regenerative_cooling
title: Regenerative cooling
category: Concepts
tags: rocket chamber coolant channel film cooling ablative
---
# Regenerative cooling
*Using the cold propellant to keep the engine from melting.*

## At a glance
| Property | Value |
|---|---|
| Gas temperature | 3300-3600 K |
| Wall limit (copper alloy) | ≈700-800 K gas-side |
| First used | Robert Goddard's tests; operationally V-2 (1942), with film cooling |

## The trick
Rocket gases are hotter than the melting point of any practical wall material. **Regenerative cooling** routes cold fuel (or oxidiser) through channels in the chamber wall before injecting it. A thin liner of a high-conductivity metal ([[copper]] alloy) passes the heat quickly into the coolant, keeping the gas-side surface hundreds of kelvin below melting. The heat isn't wasted: it pre-warms the propellant.

## Other ways to survive
- **Film cooling**: inject a layer of fuel along the wall that shields it.
- **Ablative**: let a resin-composite liner char and erode away (used in short-burn engines and SRB nozzles).
- **Radiative**: thin refractory metal (niobium) glowing white-hot, radiating heat away (upper stages, Apollo service module).
- **Heat sink**: a thick block that soaks up heat for a short burn (test hardware).

## In Ignis
- [[cooled_copper]] walls are actively cooled toward 300 K. They survive full rocket flames unless the local heat flux is extreme.
- Plain [[copper]] spreads heat but doesn't remove it: it heats up and melts eventually.

## Try this
Build two identical rocket chambers, one lined with cooled copper and one with [[inconel]]. Run both and watch the **Heat** view.

**See also:** [[cooled_copper]], [[copper]], [[heat_transfer]], [[choked_flow]]

> Reference values: Huzel & Huang, NASA SP-125; Sutton & Biblarz, Rocket Propulsion Elements.
