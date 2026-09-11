#!/usr/bin/env bash
# Activate a cached per-output palette without running matugen.
# Usage: activate-palette.sh <output-name>
set -euo pipefail

OUTPUT="${1:-}"
if [[ -z "$OUTPUT" ]]; then
  echo "usage: activate-palette.sh <output>" >&2
  exit 2
fi

PALETTE_ROOT="${XDG_CACHE_HOME:-$HOME/.cache}/wallpaper-console-rust/theme-palettes"
SRC="$PALETTE_ROOT/$OUTPUT"

if [[ ! -d "$SRC" ]]; then
  echo "activate-palette: no cached palette for $OUTPUT ($SRC)" >&2
  exit 1
fi

install_file() {
  local from="$1" to="$2"
  [[ -f "$from" ]] || return 0
  mkdir -p "$(dirname "$to")"
  cp -f "$from" "$to"
}

install_file "$SRC/waybar-colors.css" "$HOME/.config/waybar/colors.css"
install_file "$SRC/kitty-matugen.conf" "$HOME/.config/kitty/themes/matugen.conf"
install_file "$SRC/gtk-3.0-gtk.css" "$HOME/.config/gtk-3.0/gtk.css"
install_file "$SRC/gtk-4.0-gtk.css" "$HOME/.config/gtk-4.0/gtk.css"
install_file "$SRC/fuzzel_theme.ini" "$HOME/.config/fuzzel/fuzzel_theme.ini"

ln -sfn "$OUTPUT" "$PALETTE_ROOT/active"

# Best-effort live reload for kitty (ignore failures).
if command -v kitten >/dev/null 2>&1 && [[ -f "$HOME/.config/kitty/themes/matugen.conf" ]]; then
  kitten @ set-colors -a -c "$HOME/.config/kitty/themes/matugen.conf" 2>/dev/null || true
fi

# Optional waybar signal if a colors include is watched via USR2.
if command -v killall >/dev/null 2>&1; then
  killall -USR2 waybar 2>/dev/null || true
fi

echo "activate-palette: activated $OUTPUT"
