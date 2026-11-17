#!/bin/sh
# Builds the Rust core as a static library and regenerates the UniFFI Swift
# bindings into app/Generated. Called by the Xcode "Build Rust core" phase and
# usable standalone:  tools/build-core.sh [Debug|Release]
set -eu

CONFIGURATION="${1:-Debug}"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT="$ROOT/app/Generated"
TARGET="aarch64-apple-darwin"

# Xcode runs scripts with a minimal PATH.
export PATH="$HOME/.cargo/bin:/opt/homebrew/opt/rustup/bin:/opt/homebrew/bin:/usr/local/bin:$PATH"
# Must match MACOSX_DEPLOYMENT_TARGET in app/Config/Cinma.xcconfig.
export MACOSX_DEPLOYMENT_TARGET="${MACOSX_DEPLOYMENT_TARGET:-15.0}"

case "$CONFIGURATION" in
  Release) PROFILE_FLAG="--release"; PROFILE_DIR="release" ;;
  *)       PROFILE_FLAG="";          PROFILE_DIR="debug" ;;
esac

cd "$ROOT"
# shellcheck disable=SC2086
cargo build -p cinma-ffi --target "$TARGET" $PROFILE_FLAG
BUILT="$ROOT/target/$TARGET/$PROFILE_DIR"

GEN="$(mktemp -d)"
trap 'rm -rf "$GEN"' EXIT
cargo run -q -p uniffi-bindgen -- generate \
  --library "$BUILT/libcinma_ffi.dylib" --language swift --out-dir "$GEN"

mkdir -p "$OUT/include" "$OUT/lib"
# Copy only on change so Xcode doesn't recompile Swift needlessly.
copy_if_changed() { cmp -s "$1" "$2" || cp "$1" "$2"; }
copy_if_changed "$GEN/cinma_ffi.swift"         "$OUT/cinma_ffi.swift"
copy_if_changed "$GEN/cinma_ffiFFI.h"          "$OUT/include/cinma_ffiFFI.h"
copy_if_changed "$GEN/cinma_ffiFFI.modulemap"  "$OUT/include/module.modulemap"
copy_if_changed "$BUILT/libcinma_ffi.a"        "$OUT/lib/libcinma_ffi.a"
