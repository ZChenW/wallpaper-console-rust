#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
duration="${1:-30}"
bin="${WALLPAPER_CONSOLE_GUI_RUST:-$SCRIPT_DIR/../target/release/wallpaper-console-tauri}"
stamp="$(date +%Y%m%d-%H%M%S)-$$"
out_dir="${WCR_ACCEPTANCE_DIR:-/tmp/wallpaper-console-tauri-acceptance-$stamp}"
mkdir -p "$out_dir"

report="$out_dir/report.txt"
csv="$out_dir/profile.csv"

{
  echo "Wallpaper Console Tauri manual acceptance"
  echo "timestamp=$stamp"
  echo "duration_seconds=$duration"
  echo "binary=$bin"
  echo "cwd=$(pwd)"
  echo "WAYLAND_DISPLAY=${WAYLAND_DISPLAY:-}"
  echo "XDG_SESSION_TYPE=${XDG_SESSION_TYPE:-}"
  echo
} > "$report"

if [[ ! -x "$bin" ]]; then
  echo "binary_missing=$bin" | tee -a "$report"
  exit 1
fi

frontend_dist="$SCRIPT_DIR/../apps/tauri-gui/frontend/dist/index.html"
if [[ ! -f "$frontend_dist" ]]; then
  echo "frontend_dist_missing=$frontend_dist" | tee -a "$report"
  echo "hint=run ./install.sh --build-only or cd apps/tauri-gui/src-tauri && cargo tauri build --bundles deb,rpm" | tee -a "$report"
  exit 1
fi

echo "starting_profile=1" | tee -a "$report"
set +o pipefail
WALLPAPER_CONSOLE_GUI_RUST="$bin" "$SCRIPT_DIR/profile_gui.sh" "$duration" | tee "$csv"
profile_exit=${PIPESTATUS[0]}
set -o pipefail
if [[ $profile_exit -ne 0 ]]; then
  echo "profile_failed=1" >> "$report"
fi

if command -v niri >/dev/null 2>&1; then
  echo >> "$report"
  echo "niri_windows_begin" >> "$report"
  niri msg windows >> "$report" 2>&1 || true
  echo "niri_windows_end" >> "$report"
fi

if command -v grim >/dev/null 2>&1; then
  screenshot="$out_dir/screenshot.png"
  grim "$screenshot" 2>>"$report" || true
  if [[ -f "$screenshot" ]]; then
    echo "screenshot=$screenshot" >> "$report"
  else
    echo "screenshot=not_captured" >> "$report"
  fi
else
  echo "screenshot=grim_not_installed" >> "$report"
fi

echo "profile_csv=$csv" >> "$report"
echo "report=$report"
exit $profile_exit
