#!/usr/bin/env bash
# Poll focused output and swap precomputed palettes (no matugen).
# Designed for niri spawn-at-startup; exits cleanly on SIGTERM/SIGINT.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
POLL_MS="${WCR_FOCUS_FOLLOW_POLL_MS:-200}"
LAST=""

cleanup() {
  exit 0
}
trap cleanup TERM INT

focused_output() {
  if command -v niri >/dev/null 2>&1; then
    niri msg -j focused-output 2>/dev/null \
      | python3 -c 'import json,sys; print(json.load(sys.stdin).get("name") or "")' 2>/dev/null \
      && return 0
  fi
  if command -v hyprctl >/dev/null 2>&1; then
    hyprctl -j monitors 2>/dev/null \
      | python3 -c 'import json,sys
ms=json.load(sys.stdin)
print(next((m.get("name") for m in ms if m.get("focused")), "") or "")' 2>/dev/null \
      && return 0
  fi
  if command -v swaymsg >/dev/null 2>&1; then
    swaymsg -t get_outputs 2>/dev/null \
      | python3 -c 'import json,sys
ms=json.load(sys.stdin)
print(next((m.get("name") for m in ms if m.get("focused")), "") or "")' 2>/dev/null \
      && return 0
  fi
  echo ""
}

# Convert poll interval to seconds for sleep (bash integer ms).
sleep_secs="$(python3 -c "print(max(0.05, int('${POLL_MS}')/1000.0))")"

while true; do
  current="$(focused_output | head -n1 | tr -d '\r')"
  if [[ -n "$current" && "$current" != "$LAST" ]]; then
    if "$SCRIPT_DIR/activate-palette.sh" "$current" 2>/dev/null; then
      LAST="$current"
    fi
  fi
  sleep "$sleep_secs"
done
