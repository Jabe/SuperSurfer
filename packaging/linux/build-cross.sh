#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$ROOT"
export CARGO_TARGET_DIR="$ROOT/target"

TARGET="${CARGO_LINUX_TARGET:-aarch64-unknown-linux-gnu}"
ARCH="${LINUX_ARCH:-${TARGET%%-*}}"

if ! command -v zig >/dev/null; then
  echo "zig is required for cross-compiling Linux ARM from x86_64."
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

DIST="$ROOT/dist/linux-${ARCH}"
rm -rf "$DIST"
mkdir -p "$DIST"

cp "target/$TARGET/release/supersurfer" "$DIST/supersurfer"
chmod +x "$DIST/supersurfer"
cp packaging/linux/supersurfer.desktop "$DIST/supersurfer.desktop"
cp packaging/linux/install.sh "$DIST/install.sh"
chmod +x "$DIST/install.sh"

TARBALL="$ROOT/dist/supersurfer-linux-${ARCH}.tar.gz"
tar -czf "$TARBALL" -C "$ROOT/dist" "linux-${ARCH}"

echo "Built $DIST"
echo "Tarball: $TARBALL"
echo "Install: tar -xzf supersurfer-linux-${ARCH}.tar.gz && cd linux-${ARCH} && ./install.sh"
