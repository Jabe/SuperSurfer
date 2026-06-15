#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$ROOT"
export CARGO_TARGET_DIR="${ROOT}/target"

echo "Building release binary..."
cargo build --release

APP="dist/SuperSurfer.app"
MACOS="$APP/Contents/MacOS"

rm -rf "$APP"
mkdir -p "$MACOS" "$APP/Contents/Resources"

cp packaging/macos/Info.plist "$APP/Contents/Info.plist"
cp target/release/supersurfer "$MACOS/supersurfer-bin"
chmod +x "$MACOS/supersurfer-bin"

echo "Compiling Cocoa launcher..."
swiftc packaging/macos/launcher.swift \
  -o "$MACOS/SuperSurfer" \
  -framework Cocoa \
  -framework Foundation \
  -O
chmod +x "$MACOS/SuperSurfer"

swiftc packaging/macos/set-default.swift \
  -o "$MACOS/set-default" \
  -framework AppKit \
  -framework Foundation \
  -O
chmod +x "$MACOS/set-default"

if command -v codesign >/dev/null 2>&1; then
  codesign --force --deep --sign - "$APP" 2>/dev/null || true
fi

echo "Built $APP"
echo "Install: cp -R '$APP' /Applications/"
echo "Register: /Applications/SuperSurfer.app/Contents/MacOS/SuperSurfer register"
echo "         open -n -a SuperSurfer --args register"
