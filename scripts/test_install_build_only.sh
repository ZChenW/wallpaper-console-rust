#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

./install.sh --build-only

test -x target/release/wallpaper-console-tauri
test -x target/release/wallpaper-console-rust

test -f apps/tauri-gui/frontend/dist/index.html || { echo "FAIL: frontend dist/index.html missing — did npm run build succeed?"; exit 1; }

latest_tauri_output="$(ls -td target/release/build/wallpaper-console-tauri-*/output 2>/dev/null | head -n 1)"
test -n "$latest_tauri_output" || { echo "FAIL: Tauri build output marker missing"; exit 1; }
if grep -q 'cargo:rustc-cfg=dev' "$latest_tauri_output"; then
  echo "FAIL: Tauri GUI was built with dev cfg; installed release binary would load devUrl and show a blank WebView"
  exit 1
fi

if find target/release/bundle -type f 2>/dev/null | grep -q .; then
  echo "NOTE: bundle artifacts exist from a previous build, but install verification only requires release binaries"
fi

echo "PASS: install build-only artifacts exist"
