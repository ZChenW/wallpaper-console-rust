#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

./install.sh --build-only

test ! -e target/release/wallpaper-console-rust || echo "NOTE: legacy CLI artifact exists from a previous build"
test -x target/release/wallpaper-console-tauri

test -f apps/tauri-gui/frontend/dist/index.html || { echo "FAIL: frontend dist/index.html missing — did npm run build succeed?"; exit 1; }

if find target/release/bundle -type f 2>/dev/null | grep -q .; then
  echo "NOTE: bundle artifacts exist from a previous build, but install verification only requires release binaries"
fi

echo "PASS: install build-only artifacts exist"
