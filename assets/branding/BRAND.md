# Ignis brand guide

Ignis is a realistic 2D combustion and chemistry sandbox. The identity should feel hot, precise and
scientific: a real flame (blue premixed base, white-hot core, gold and ember mantle) rising from a
grid of simulation cells.

## Assets (all generated; edit `src/generate.py`, then run `./build.sh`)

| File | Use |
|---|---|
| `app_icon_1024.png`, `app_icon_512.png`, `Ignis.icns`, `AppIcon.iconset/` | macOS app icon (squircle, with built-in drop shadow, 100 px safe margin on the 1024 canvas) |
| `icon_256.png` | Window/dock icon at runtime (`ViewportBuilder::with_icon`) |
| `mark_{128,256,512,1024}.png`, `svg/mark.svg` | Flame mark alone, transparent |
| `wordmark.png`, `wordmark@2x.png`, `svg/wordmark.svg` | "IGNIS" wordmark, transparent. The second I is a lit match |
| `lockup.png`, `svg/lockup.svg` | Mark, wordmark and "COMBUSTION SANDBOX" tagline |
| `title_art.png` (1920×720) | Title-screen hero with the wordmark and tagline baked in |
| `title_bg.png` (1920×720) | The same scene without text, for drawing live egui text on top |

Small icon sizes (16 and 32 px) use `svg/app_icon_small.svg`, which drops the grid and particles so they stay legible.

## Colour

| Token | Hex | RGB | Role |
|---|---|---|---|
| Soot | `#0B0A0C` | 11,10,12 | Deepest background, icon edges |
| Char | `#141519` | 20,21,25 | Panels (close to app `PANEL_BG`) |
| Coal | `#2A1409` | 42,20,9 | Warm dark, glow falloff |
| Ember | `#C2350E` | 194,53,14 | Outer flame, danger, deep heat |
| **Flame** | `#FF8C28` | 255,140,40 | **Primary accent** (same as app `ACCENT`) |
| **Gold** | `#FFC83C` | 255,200,60 | **Secondary accent / highlights** (same as app `GOLD`) |
| Core | `#FFF3D2` | 255,243,210 | White-hot core, emphasis text on dark backgrounds |
| Blue | `#3C8CFF` | 60,140,255 | Premixed flame base. Use sparingly for "clean burn" or info states |
| Blue deep | `#1F4FD8` | 31,79,216 | Blue gradient end |
| Ash | `#8C919E` | 140,145,158 | Muted text and taglines (same as app `MUTED`) |

Heat ramp for cell art (hot to cold): `#FFE08A` `#FFB02E` `#FF7A1E` `#E2450F` `#A52A0A` `#5A1A0A` `#2B1A14`.

Wordmark gradient (top to bottom): `#FFE7A8` to Gold `#FFC83C` to Flame `#FF8C28`.

Rules:
- Always place the brand on dark (Soot/Char). The mark has no light-background version, by design.
- Accents are for fire and heat. Do not use Flame orange for neutral UI chrome.
- Glow is allowed on the wordmark (an orange blur behind it, about 0.85 opacity). Never outline it.

## Type

egui's built-in fonts (eframe `default_fonts` feature, the only fonts in the binary):

| egui family | Font | Use |
|---|---|---|
| `Proportional` | **Ubuntu-Light** | All UI text. For titles use a large size with wide letter spacing (uppercase, about 0.4 em) to echo the tagline style |
| `Monospace` | **Hack** | Numbers, readouts (T, p, mole fractions), probes, code |
| fallback | NotoEmoji-Regular, emoji-icon-font | Symbols and icons |

The wordmark itself is custom geometric lettering (paths, not a font). Never set "IGNIS" in a font as the logo;
use `wordmark.png` or `title_art.png`. In running text, write the name as "Ignis".

Suggested egui sizes: title 34–40, heading 20, body 14, small/readouts 12 (Hack).

## Voice

Short, confident, factual. "Real combustion. Real chemistry." Name things by their real science (adiabatic
flame temperature, stoichiometric, deflagration) and explain them in one plain sentence.
