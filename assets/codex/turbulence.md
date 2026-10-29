---
id: turbulence
title: Turbulence
category: Concepts
tags: reynolds eddy les mixing vortex
---
# Turbulence
*Chaotic swirling flow that mixes everything much faster.*

## At a glance
| Property | Value |
|---|---|
| Reynolds number | Re = ρ·v·L / μ; turbulence typically above ≈2000-4000 in pipes |
| Energy cascade | big eddies → small eddies → heat (Kolmogorov, 1941) |
| Turbulent flame speed | can be 10-100× the laminar speed |

## What it is
At low speeds fluid slides in smooth layers (laminar flow). Fast or large flows become unstable and break into eddies of every size. Big eddies stretch and break into smaller ones until viscosity turns their energy into heat at the smallest (Kolmogorov) scale.

## Why combustion loves it
Turbulence wrinkles flame fronts, enormously increasing their surface area, and mixes fuel with oxygen far faster than diffusion. Jet engines and furnaces are designed to be turbulent; gas explosions accelerate because obstacles make turbulence.

## Large-eddy simulation
No computer can resolve every eddy in a real flow. **LES** solves the big eddies directly and models the effect of the small ones as extra viscosity and diffusivity (Smagorinsky, 1963).

## In Ignis
- The fluid solver has an LES-style turbulence model: eddy viscosity adds mixing where the flow is sheared, feeding the EDC burn rate.

## Try this
Switch to the **Flow** view and watch a jet entering still air break into vortices.

**See also:** [[eddy_dissipation_concept]], [[deflagration_vs_detonation]], [[buoyancy]]

> Reference values: Pope, Turbulent Flows; Smagorinsky (1963), Monthly Weather Review.
