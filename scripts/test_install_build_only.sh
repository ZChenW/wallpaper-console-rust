#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

./install.sh --build-only

test -x target/release/wallpaper-console-rust
test -x target/release/wallpaper-console-tauri
test -x target/release/wallpaper-console-web-renderer

echo "PASS: install build-only artifacts exist"
