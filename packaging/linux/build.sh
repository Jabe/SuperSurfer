#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$ROOT"
export CARGO_TARGET_DIR="$ROOT/target"

echo "Building release binary..."
cargo build --release

DIST="$ROOT/dist/linux"
rm -rf "$DIST"
mkdir -p "$DIST"

cp target/release/supersurfer "$DIST/supersurfer"
chmod +x "$DIST/supersurfer"
cp packaging/linux/supersurfer.desktop "$DIST/supersurfer.desktop"
cp packaging/linux/install.sh "$DIST/install.sh"
chmod +x "$DIST/install.sh"

ARCH="$(uname -m)"
TARBALL="$ROOT/dist/supersurfer-linux-${ARCH}.tar.gz"
tar -czf "$TARBALL" -C "$ROOT/dist" linux

echo "Built $DIST"
echo "Tarball: $TARBALL"
echo "Install: cd dist/linux && ./install.sh"
