#!/usr/bin/env bash
# Regenerates every Ignis brand asset from source.
#   assets/branding/src/generate.py  -> assets/branding/svg/*.svg
#   rsvg-convert                      -> assets/branding/*.png
#   iconutil                          -> assets/branding/Ignis.icns
# Requires: python3 (stdlib only), rsvg-convert (brew install librsvg), iconutil (Xcode CLT / macOS).
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
cd "$HERE"
command -v rsvg-convert >/dev/null || { echo "rsvg-convert missing: brew install librsvg" >&2; exit 1; }

python3 -I src/generate.py

r() { rsvg-convert "$@"; }   # r -w W [-h H] in.svg -o out.png

# app icon + window icon
r -w 1024 -h 1024 svg/app_icon.svg -o app_icon_1024.png
r -w 512  -h 512  svg/app_icon.svg -o app_icon_512.png
r -w 256  -h 256  svg/app_icon.svg -o icon_256.png

# flame mark
for s in 128 256 512 1024; do r -w $s -h $s svg/mark.svg -o "mark_${s}.png"; done

# wordmark + lockup (transparent)
r -z 2 svg/wordmark.svg -o wordmark.png
r -z 4 svg/wordmark.svg -o wordmark@2x.png
r -z 2 svg/lockup.svg -o lockup.png

# title art (with and without text)
r -w 1920 -h 720 svg/title_art.svg -o title_art.png
r -w 1920 -h 720 svg/title_bg.svg  -o title_bg.png

# .iconset -> .icns  (16/32 px use the simplified small icon for legibility)
SET="$HERE/AppIcon.iconset"
rm -rf "$SET"; mkdir -p "$SET"
r -w 16   -h 16   svg/app_icon_small.svg -o "$SET/icon_16x16.png"
r -w 32   -h 32   svg/app_icon_small.svg -o "$SET/icon_16x16@2x.png"
r -w 32   -h 32   svg/app_icon_small.svg -o "$SET/icon_32x32.png"
r -w 64   -h 64   svg/app_icon_small.svg -o "$SET/icon_32x32@2x.png"
r -w 128  -h 128  svg/app_icon.svg       -o "$SET/icon_128x128.png"
r -w 256  -h 256  svg/app_icon.svg       -o "$SET/icon_128x128@2x.png"
r -w 256  -h 256  svg/app_icon.svg       -o "$SET/icon_256x256.png"
r -w 512  -h 512  svg/app_icon.svg       -o "$SET/icon_256x256@2x.png"
r -w 512  -h 512  svg/app_icon.svg       -o "$SET/icon_512x512.png"
cp app_icon_1024.png "$SET/icon_512x512@2x.png"
iconutil -c icns "$SET" -o Ignis.icns
echo "branding assets rebuilt in $HERE"
