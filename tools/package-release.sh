#!/bin/bash
# Packages a self-contained Release Cinma.app bundle (and optional DMG).
# By default checks for libmpv.2.dylib in CINMA_BUNDLE_LIBMPV or Homebrew
# and, when BUNDLE_LIBMPV=1 is set, copies + rewrites @loader_path dylibs
# into Contents/Frameworks/ (ADR 0015).
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
DERIVED="$ROOT/build/xcode"
APP="$DERIVED/Build/Products/Release/Cinma.app"
OUT_DIR="${OUT_DIR:-$ROOT/dist}"
ENTITLEMENTS="$ROOT/app/Cinma/Cinma.entitlements"
SIGN_ID="${CODESIGN_IDENTITY:--}"
BUNDLE_LIBMPV="${BUNDLE_LIBMPV:-0}"

echo "Building Release Cinma.app…"
xcodebuild -project "$ROOT/app/Cinma.xcodeproj" -scheme Cinma -configuration Release \
  -destination 'platform=macOS,arch=arm64' -derivedDataPath "$DERIVED" build -quiet

mkdir -p "$OUT_DIR"
DEST_APP="$OUT_DIR/Cinma.app"
rm -rf "$DEST_APP"
ditto "$APP" "$DEST_APP"

if [ "$BUNDLE_LIBMPV" = "1" ]; then
  MPV_SRC="${CINMA_BUNDLE_LIBMPV:-/opt/homebrew/opt/mpv/lib/libmpv.2.dylib}"
  if [ ! -f "$MPV_SRC" ]; then
    echo "error: libmpv not found at $MPV_SRC" >&2
    exit 1
  fi
  FW_DIR="$DEST_APP/Contents/Frameworks"
  mkdir -p "$FW_DIR"
  echo "Bundling libmpv from $MPV_SRC into Contents/Frameworks…"
  cp -L "$MPV_SRC" "$FW_DIR/libmpv.2.dylib"
  chmod u+w "$FW_DIR/libmpv.2.dylib"
  install_name_tool -id "@loader_path/libmpv.2.dylib" "$FW_DIR/libmpv.2.dylib"

  # Copy first-order non-system dependencies recursively so bundle is relocatable.
  queue=("$FW_DIR/libmpv.2.dylib")
  seen=""
  while [ "${#queue[@]}" -gt 0 ]; do
    cur="${queue[0]}"
    queue=("${queue[@]:1}")
    deps="$(otool -L "$cur" | awk 'NR>1 {print $1}' | grep -E '^(/opt/homebrew|/usr/local)' || true)"
    for dep in $deps; do
      base="$(basename "$dep")"
      if [ ! -f "$FW_DIR/$base" ] && [ -f "$dep" ]; then
        cp -L "$dep" "$FW_DIR/$base"
        chmod u+w "$FW_DIR/$base"
        install_name_tool -id "@loader_path/$base" "$FW_DIR/$base"
        queue+=("$FW_DIR/$base")
      fi
      install_name_tool -change "$dep" "@loader_path/$base" "$cur"
    done
  done
  for dylib in "$FW_DIR"/*.dylib; do
    codesign --force --sign "$SIGN_ID" --timestamp=none "$dylib"
  done
fi

echo "Signing $DEST_APP (identity: $SIGN_ID)…"
RUNTIME_OPT=""
if [ "$SIGN_ID" != "-" ]; then
  RUNTIME_OPT="--options runtime"
fi
# shellcheck disable=SC2086
codesign --force --deep $RUNTIME_OPT --entitlements "$ENTITLEMENTS" \
  --sign "$SIGN_ID" --timestamp=none "$DEST_APP"
codesign --verify --deep --strict "$DEST_APP"
echo "Packaged: $DEST_APP"
