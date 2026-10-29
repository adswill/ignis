#!/usr/bin/env bash
# Build Ignis release archives for GitHub Releases (macOS, Apple Silicon).
#
#   tools/release_macos.sh               # build, bundle, zip + dmg
#   tools/release_macos.sh --no-build    # reuse the existing target/release binary
#   BIN=combustor tools/release_macos.sh # override the cargo binary name
#   VERSION=1.0.0 tools/release_macos.sh # override the version (default: Cargo.toml)
#
# Produces:
#   dist/Ignis.app
#   dist/Ignis-<version>-macOS-arm64.zip   (ditto -c -k --keepParent)
#   dist/Ignis-<version>-macOS-arm64.dmg   (hdiutil UDZO, with an Applications symlink)
#   dist/SHA256SUMS.txt
# and prints (does NOT run) the matching `gh release create` command.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

DO_BUILD=1
for a in "$@"; do
  case "$a" in
    --no-build) DO_BUILD=0 ;;
    -h|--help) sed -n '2,15p' "$0"; exit 0 ;;
    *) echo "unknown option: $a" >&2; exit 2 ;;
  esac
done

APP_NAME="Ignis"
BUNDLE_ID="${BUNDLE_ID:-com.ignis.sandbox}"
MIN_MACOS="11.0"
ARCH="arm64"
REPO="${REPO:-adswill/ignis}"
DIST="$ROOT/dist"
APP="$DIST/$APP_NAME.app"

VERSION="${VERSION:-$(sed -n 's/^version *= *"\(.*\)"/\1/p' Cargo.toml | head -1)}"
VERSION="${VERSION:-0.0.0}"
TAG="v$VERSION"

if [[ "$(uname -m)" != "arm64" ]]; then
  echo "warning: host is $(uname -m); archives will still be named $ARCH" >&2
fi

# --- 1. build ---------------------------------------------------------------
if [[ $DO_BUILD -eq 1 ]]; then
  echo "==> cargo build --release"
  cargo build --release
fi

# Binary name: BIN (default ignis), falling back to combustor (pre-rename crate).
BIN="${BIN:-ignis}"
if [[ ! -x "$ROOT/target/release/$BIN" ]]; then
  if [[ -x "$ROOT/target/release/combustor" ]]; then
    echo "note: target/release/$BIN not found, using target/release/combustor"
    BIN="combustor"
  else
    echo "error: no release binary found (target/release/$BIN or combustor)" >&2
    exit 1
  fi
fi
BIN_SRC="$ROOT/target/release/$BIN"

# --- 2. bundle --------------------------------------------------------------
BUNDLER="$ROOT/tools/bundle_macos.sh"
USED_BUNDLER=0
if [[ -x "$BUNDLER" ]] && grep -Eq "target/release/${BIN}\"|target/release/\\\$\{?BIN" "$BUNDLER" 2>/dev/null; then
  echo "==> bundling with tools/bundle_macos.sh"
  "$BUNDLER" --no-build
  USED_BUNDLER=1
elif [[ -x "$BUNDLER" ]]; then
  echo "note: tools/bundle_macos.sh does not reference target/release/$BIN; assembling the bundle here"
fi

if [[ $USED_BUNDLER -eq 0 ]]; then
  echo "==> assembling $APP"
  ICNS="$(ls "$ROOT"/assets/branding/*.icns 2>/dev/null | head -1 || true)"
  rm -rf "$APP"
  mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
  cp "$BIN_SRC" "$APP/Contents/MacOS/$APP_NAME"
  chmod +x "$APP/Contents/MacOS/$APP_NAME"
  ICON_KEY=""
  if [[ -n "$ICNS" ]]; then
    cp "$ICNS" "$APP/Contents/Resources/$APP_NAME.icns"
    ICON_KEY="<key>CFBundleIconFile</key><string>$APP_NAME</string>"
  else
    echo "warning: no assets/branding/*.icns found; bundle has no icon" >&2
  fi
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
  $ICON_KEY
  <key>CFBundleInfoDictionaryVersion</key> <string>6.0</string>
  <key>CFBundlePackageType</key>           <string>APPL</string>
  <key>CFBundleShortVersionString</key>    <string>$VERSION</string>
  <key>CFBundleVersion</key>               <string>$(date +%Y%m%d%H%M)</string>
  <key>LSApplicationCategoryType</key>     <string>public.app-category.education</string>
  <key>LSMinimumSystemVersion</key>        <string>$MIN_MACOS</string>
  <key>NSHighResolutionCapable</key>       <true/>
  <key>NSHumanReadableCopyright</key>      <string>Copyright (c) 2026 adswill. PolyForm Noncommercial 1.0.0.</string>
</dict>
</plist>
PLIST
  printf 'APPL????' > "$APP/Contents/PkgInfo"
  plutil -lint "$APP/Contents/Info.plist" >/dev/null
  codesign --force --deep --sign - "$APP" >/dev/null 2>&1 || echo "warning: ad-hoc codesign failed" >&2
fi

[[ -d "$APP" ]] || { echo "error: $APP was not produced" >&2; exit 1; }
# Ship the licence inside the bundle (PolyForm requires recipients get the terms + Required Notice).
[[ -f "$ROOT/LICENSE" ]] && cp "$ROOT/LICENSE" "$APP/Contents/Resources/LICENSE.txt"
# Re-sign after touching the bundle contents.
codesign --force --deep --sign - "$APP" >/dev/null 2>&1 || true

# --- 3. archives ------------------------------------------------------------
BASE="$APP_NAME-$VERSION-macOS-$ARCH"
ZIP="$DIST/$BASE.zip"
DMG="$DIST/$BASE.dmg"

echo "==> $ZIP"
rm -f "$ZIP"
ditto -c -k --sequesterRsrc --keepParent "$APP" "$ZIP"

echo "==> $DMG"
STAGE="$(mktemp -d "${TMPDIR:-/tmp}/ignis-dmg.XXXXXX")"
trap 'rm -rf "$STAGE"' EXIT
cp -R "$APP" "$STAGE/"
ln -s /Applications "$STAGE/Applications"
[[ -f "$ROOT/LICENSE" ]] && cp "$ROOT/LICENSE" "$STAGE/LICENSE.txt"
rm -f "$DMG"
hdiutil create -volname "$APP_NAME $VERSION" -srcfolder "$STAGE" -ov -format UDZO "$DMG" >/dev/null
[[ -f "$DMG" ]] || { echo "error: hdiutil failed to create $DMG" >&2; exit 1; }

( cd "$DIST" && shasum -a 256 "$BASE.zip" "$BASE.dmg" > SHA256SUMS.txt )

echo
echo "==> done"
ls -lh "$ZIP" "$DMG" "$DIST/SHA256SUMS.txt" | awk '{print "    " $5 "  " $9}'
cat "$DIST/SHA256SUMS.txt" | sed 's/^/    /'

NOTES="docs/RELEASE_NOTES_v$VERSION.md"
[[ -f "$ROOT/$NOTES" ]] || NOTES="docs/RELEASE_NOTES_v1.0.0.md"

echo
echo "To publish (not run by this script):"
echo
cat <<CMD
  gh release create $TAG \\
    "dist/$BASE.dmg" \\
    "dist/$BASE.zip" \\
    "dist/SHA256SUMS.txt" \\
    --repo $REPO \\
    --title "Ignis $VERSION" \\
    --notes-file "$NOTES"
CMD
