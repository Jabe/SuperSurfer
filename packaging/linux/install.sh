#!/usr/bin/env bash
set -euo pipefail

HERE="$(cd "$(dirname "$0")" && pwd)"

BIN_DIR="${XDG_BIN_HOME:-$HOME/.local/bin}"
APP_DIR="${XDG_DATA_HOME:-$HOME/.local/share}/applications"
BIN_PATH="$BIN_DIR/supersurfer"
DESKTOP_PATH="$APP_DIR/supersurfer.desktop"

mkdir -p "$BIN_DIR" "$APP_DIR"

install -m 0755 "$HERE/supersurfer" "$BIN_PATH"

# Point the desktop entry at the installed binary.
sed "s|__EXEC__|$BIN_PATH|g" "$HERE/supersurfer.desktop" > "$DESKTOP_PATH"

if command -v update-desktop-database >/dev/null 2>&1; then
  update-desktop-database "$APP_DIR" >/dev/null 2>&1 || true
fi

echo "Installed supersurfer to $BIN_PATH"
echo "Installed desktop entry to $DESKTOP_PATH"
echo
echo "Make sure $BIN_DIR is on your PATH, then run:"
echo "  supersurfer init"
echo "  supersurfer register   # set as default browser"
echo "  supersurfer doctor"
