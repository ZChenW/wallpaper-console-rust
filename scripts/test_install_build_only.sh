#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

./install.sh --build-only

test -x target/release/wallpaper-console-rust
test -x target/release/wallpaper-console-tauri

if find target/release/bundle -type f 2>/dev/null | grep -q .; then
  echo "NOTE: bundle artifacts exist from a previous build, but install verification only requires release binaries"
fi

echo "PASS: install build-only artifacts exist"
