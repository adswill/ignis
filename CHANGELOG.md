# Changelog

## 0.2.x (final verification pass)
- Materials: every element id 10-91 has its own colour, grain/shape and sparkle (matter_look.wgsl); per-liquid tints, pile lighting, labelled material board, mix_demo scene.
- Explosives: fuse chain now propagates (dynamite, TNT, C4 fire); thermite spreads slowly and breaches plates; cast propellant holds steady 5-6 atm.
- Physics: conservation diagnostic fixed (post-pass sources were unbooked); dE drift now < 0.001 % on flare, campfire, srm, rocket.
- Rendering: supersonic plume with shock diamonds, log-scale pressure view with schlieren, inferno speed view, colour-bar legends, campfire turbulence and embers, anti-aliased walls.
- UI: zoom-to-fit (Shift+F, Fit button), docked propellant designer with log-log burn chart and motor section, material thumbnails, auto-scaled plot time axis, sound toggle, ASCII status bar.
- Audio: per-material signatures (black powder, Mg, thermite, TNT, grain burn), distance-delayed booms, mute toggle (not yet wired to feed_audio).
- Known: srm/thermite/rocket QA thresholds still failing (see tools/QA_REPORT.md).
