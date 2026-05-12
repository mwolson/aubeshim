#!/bin/bash

set -euo pipefail

for cmd in cargo; do
    if ! command -v "$cmd" >/dev/null 2>&1; then
        echo "Error: '$cmd' is required but not found in PATH." >&2
        exit 1
    fi
done

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

BIN_DIR="$HOME/.local/bin"

echo "Building aubeshim (release)..."
cargo build --release

mkdir -p "$BIN_DIR"
cp "$SCRIPT_DIR/target/release/aubeshim" "$BIN_DIR/aubeshim"
chmod +x "$BIN_DIR/aubeshim"
echo "  Installed $BIN_DIR/aubeshim"

echo ""
echo "Installing npm and pnpm shims..."
exec "$BIN_DIR/aubeshim" install --force
