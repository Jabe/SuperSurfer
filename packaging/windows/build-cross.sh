#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$ROOT"
export CARGO_TARGET_DIR="$ROOT/target"

TARGET="${CARGO_WINDOWS_TARGET:-x86_64-pc-windows-gnu}"

if ! command -v zig >/dev/null; then
  echo "zig is required for cross-compiling to Windows from macOS/Linux."
  echo "Add zig to mise.toml or run: mise use zig"
  exit 1
fi

if ! command -v cargo-zigbuild >/dev/null; then
  echo "Installing cargo-zigbuild..."
  cargo install cargo-zigbuild --locked
fi

if ! rustup target list --installed | grep -qx "$TARGET"; then
  rustup target add "$TARGET"
fi

echo "Cross-compiling for $TARGET..."
cargo zigbuild --release --target "$TARGET"

DIST="$ROOT/dist"
EXE="$ROOT/target/$TARGET/release/supersurfer.exe"
OUT="$DIST/supersurfer.exe"

mkdir -p "$DIST"
cp -f "$EXE" "$OUT"

echo "Built $OUT"
echo "Copy to a Windows machine, then run: supersurfer.exe init --register"
echo "Then Settings -> Apps -> Default apps -> SuperSurfer -> Set default"
