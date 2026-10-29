---
id: fuel_inlet
title: Fuel inlet
category: Machines & Tools
tile: 2
tags: methane injector feed
---
# Fuel inlet
*Feeds methane at the fuel feed pressure.*

## At a glance
| Property | Value |
|---|---|
| Rocket injector pressure drop | typically ≈15-25 % of chamber pressure |
| Gas stove burner | ≈2 kPa supply pressure |

## In the real world
Injectors meter fuel into a combustor and mix it with the oxidiser. In rockets the injector pressure drop is kept high so pressure wobbles in the chamber can't feed back into the flow (combustion instability).

## In Ignis
- Held at **p_fuel** with pure [[methane]]. Combine with an [[oxygen_inlet]] or [[air_inlet]] and an [[igniter]] for a burner.

## Try this
Build a Bunsen burner: a fuel inlet at the bottom of a tube with side holes to let air in.

**See also:** [[methane]], [[stoichiometry]], [[oxygen_inlet]]

> Reference values: Huzel & Huang, NASA SP-125.
