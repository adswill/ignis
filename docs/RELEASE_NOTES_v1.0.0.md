# Ignis 1.0.0

First public release of Ignis, a research-grade 2D combustion and chemistry simulator for macOS on Apple Silicon.

<!-- Fill in the date and re-check every line marked VERIFY before running `gh release create`. -->
Release date: YYYY-MM-DD

## Downloads

| File | Description |
|---|---|
| `Ignis-1.0.0-macOS-arm64.dmg` | Disk image. Open it and drag Ignis onto Applications. |
| `Ignis-1.0.0-macOS-arm64.zip` | The same app bundle as a zip archive. |

Requirements: macOS 11 or later, Apple Silicon (M1 or later).

### First launch

This build is not signed with an Apple Developer ID or notarised. On first launch, right-click (or
Control-click) Ignis.app in Applications and choose **Open**, then confirm. Alternatively run:

```bash
xattr -dr com.apple.quarantine /Applications/Ignis.app
```

## Highlights

- **GPU gas dynamics.** Compressible multi-species solver (HLLC fluxes, MUSCL reconstruction, RK2) with a
  reduced speed of sound, per-species thermodynamics, Smagorinsky LES turbulence and buoyancy.
- **Combustion.** Arrhenius kinetics combined with Eddy Dissipation Concept mixing-limited burning; soot
  formation, oxidation and radiation.
- **Matter layer.** 56 powders, liquids and reactive solids with emergent chemistry: thermite, black powder,
  APCP, dust explosions, alkali metals in water, peroxide decomposition, acids, explosives and more.
- **Materials.** Ten wall materials with heat transfer, stress, softening, bursting, melting and thermal shock.
- **Workspace.** Field views with colour-map legends, probes and line samplers with live plots, CSV export,
  selectable units, conservation diagnostics. <!-- VERIFY -->
- **Experiments and Codex.** Guided lab exercises with measured results, and an in-app reference covering every
  element, material, reaction and concept. <!-- VERIFY -->

## Known issues

<!-- Replace with the real list from QA (tools/QA_REPORT.md). -->
- The app is unsigned; Gatekeeper requires the first-launch step above.
- Intel Macs are not supported by the release build.
- <!-- VERIFY: add any open rendering / performance issues -->

## Checksums

```text
<sha256>  Ignis-1.0.0-macOS-arm64.dmg
<sha256>  Ignis-1.0.0-macOS-arm64.zip
```

<!-- Generate with: shasum -a 256 dist/Ignis-1.0.0-macOS-arm64.* -->

## Licence

Source-available under the PolyForm Noncommercial License 1.0.0. Free for personal, educational, research and
non-profit use; commercial use is not permitted. Contact the author for commercial licensing.

## Support

To support more projects, consider donating here: https://ko-fi.com/adswill
