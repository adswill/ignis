---
id: ignition_and_autoignition
title: Ignition & autoignition
category: Concepts
tags: arrhenius activation energy thermal runaway spark mie
---
# Ignition & autoignition
*Why fuels need a push to start, and how they can start themselves.*

## At a glance
| Property | Value |
|---|---|
| Arrhenius law | k = A · exp(-Eₐ / (R·T)) |
| Rule of thumb | many reactions roughly double in speed every 10 K near room temperature |
| Autoignition, diesel / gasoline / methane | ≈210 °C / ≈250-280 °C / ≈540-600 °C |
| Minimum ignition energy, methane / hydrogen | ≈0.28 mJ / ≈0.02 mJ |

## Activation energy
Fuel and oxygen are stable together at room temperature because molecules must collide with enough energy to break bonds before they can form new, stronger ones. The fraction of collisions with that energy rises **exponentially** with temperature (Arrhenius).

## Thermal runaway
If a reacting mixture makes heat faster than it can lose it, it heats up, reacts faster, makes more heat... Semenov's theory (1928) gives the critical condition. This is how oily rags, haystacks and coal piles self-ignite, and how [[permanganate_glycerin]] lights itself.

## Ignition types
- **Piloted ignition**: a flame or spark lights the vapour (flash point, fire point).
- **Autoignition**: the whole mixture is hot enough to ignite without any flame (diesel engines, engine knock).
- **Minimum ignition energy**: the smallest spark that lights the most ignitable mixture, tiny for hydrogen.

## In Ignis
- Every reaction in Ignis has an ignition temperature and an Arrhenius-like rate. The torch, igniter and hot walls supply the activation heat.
- Self-heating mixtures (permanganate + glycerin, hot peroxide) run away on their own.

## Try this
Place a steel plate above a methane flame and slowly let it heat. When does fuel touching the hot plate light?

**See also:** [[flash_point]], [[eddy_dissipation_concept]], [[permanganate_glycerin]], [[igniter]]

> Reference values: Semenov (1928); Glassman & Yetter, Combustion; NFPA 921.
