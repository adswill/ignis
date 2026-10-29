#!/usr/bin/env bash
# Build Ignis and assemble a macOS .app bundle (and optionally a .dmg).
#
#   tools/bundle_macos.sh            # cargo build --release + dist/Ignis.app
#   tools/bundle_macos.sh --dmg      # ... plus dist/Ignis-<version>.dmg
#   tools/bundle_macos.sh --no-build # reuse the existing target/release binary
#   tools/bundle_macos.sh --open     # launch the bundle when done
#
# The cargo binary is `ignis`; it is copied into the bundle as
# Contents/MacOS/Ignis. The icon comes from assets/branding/Ignis.icns (rebuilt by
# assets/branding/build.sh if missing). The bundle is ad-hoc code-signed so it
# launches locally on Apple Silicon; distribution needs a Developer ID + notarisation.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

DO_BUILD=1; DO_DMG=0; DO_OPEN=0
for a in "$@"; do
  case "$a" in
    --dmg) DO_DMG=1 ;;
    --no-build) DO_BUILD=0 ;;
    --open) DO_OPEN=1 ;;
    -h|--help) sed -n '2,13p' "$0"; exit 0 ;;
    *) echo "unknown option: $a" >&2; exit 2 ;;
  esac
done

APP_NAME="Ignis"
BUNDLE_ID="com.ignis.sandbox"
BIN_SRC="$ROOT/target/release/ignis"
VERSION="$(sed -n 's/^version *= *"\(.*\)"/\1/p' Cargo.toml | head -1)"
VERSION="${VERSION:-0.1.0}"
BUILD_NUM="$(date +%Y%m%d%H%M)"
MIN_MACOS="11.0"
ICNS="$ROOT/assets/branding/Ignis.icns"
DIST="$ROOT/dist"
APP="$DIST/$APP_NAME.app"

if [[ $DO_BUILD -eq 1 ]]; then
  echo "==> cargo build --release"
  cargo build --release
fi
[[ -x "$BIN_SRC" ]] || { echo "missing $BIN_SRC (build first)" >&2; exit 1; }

if [[ ! -f "$ICNS" ]]; then
  echo "==> icon missing, regenerating branding"
  "$ROOT/assets/branding/build.sh"
fi

echo "==> assembling $APP"
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp "$BIN_SRC" "$APP/Contents/MacOS/$APP_NAME"
chmod +x "$APP/Contents/MacOS/$APP_NAME"
cp "$ICNS" "$APP/Contents/Resources/$APP_NAME.icns"
# ship the brand art too (handy for a future runtime loader / about box)
mkdir -p "$APP/Contents/Resources/branding"
for f in title_art.png title_bg.png wordmark.png lockup.png icon_256.png; do
  [[ -f "$ROOT/assets/branding/$f" ]] && cp "$ROOT/assets/branding/$f" "$APP/Contents/Resources/branding/"
done

cat > "$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleDevelopmentRegion</key>     <string>en</string>
  <key>CFBundleName</key>                  <string>$APP_NAME</string>
  <key>CFBundleDisplayName</key>           <string>$APP_NAME</string>
  <key>CFBundleExecutable</key>            <string>$APP_NAME</string>
  <key>CFBundleIdentifier</key>            <string>$BUNDLE_ID</string>
  <key>CFBundleIconFile</key>              <string>$APP_NAME</string>
  <key>CFBundleInfoDictionaryVersion</key> <string>6.0</string>
  <key>CFBundlePackageType</key>           <string>APPL</string>
  <key>CFBundleShortVersionString</key>    <string>$VERSION</string>
  <key>CFBundleVersion</key>               <string>$BUILD_NUM</string>
  <key>LSApplicationCategoryType</key>     <string>public.app-category.simulation-games</string>
  <key>LSMinimumSystemVersion</key>        <string>$MIN_MACOS</string>
  <key>NSHighResolutionCapable</key>       <true/>
  <key>NSSupportsAutomaticGraphicsSwitching</key> <true/>
  <key>NSRequiresAquaSystemAppearance</key> <false/>
  <key>NSHumanReadableCopyright</key>      <string>Ignis — combustion &amp; chemistry sandbox</string>
</dict>
</plist>
PLIST
printf 'APPL????' > "$APP/Contents/PkgInfo"

plutil -lint "$APP/Contents/Info.plist" >/dev/null
# ad-hoc signature so Gatekeeper/AMFI accept the renamed binary locally
codesign --force --deep --sign - "$APP" >/dev/null 2>&1 || echo "warning: codesign failed (bundle still usable locally)"
touch "$APP"   # nudge Finder/LaunchServices to refresh the icon
echo "==> built $APP ($(du -sh "$APP" | cut -f1))"

if [[ $DO_DMG -eq 1 ]]; then
  DMG="$DIST/$APP_NAME-$VERSION.dmg"
  STAGE="$(mktemp -d "${TMPDIR:-/tmp}/ignis-dmg.XXXXXX")"
  cp -R "$APP" "$STAGE/"
  ln -s /Applications "$STAGE/Applications"
  rm -f "$DMG"
  hdiutil create -volname "$APP_NAME" -srcfolder "$STAGE" -ov -format UDZO "$DMG" 2>&1 >/dev/null | grep -v deprecated >&2 || true
  rm -rf "$STAGE"
  [[ -f "$DMG" ]] || { echo "hdiutil failed to create $DMG" >&2; exit 1; }
  echo "==> built $DMG ($(du -sh "$DMG" | cut -f1))"
fi

if [[ $DO_OPEN -eq 1 ]]; then
  open "$APP"
fi
