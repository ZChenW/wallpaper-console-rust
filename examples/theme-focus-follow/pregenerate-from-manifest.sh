#!/usr/bin/env bash
# Pregenerate per-output matugen palettes from WCR_THEME_MANIFEST, then activate
# the theme-source output. Intended as post_apply_command.
set -euo pipefail

PALETTE_ROOT="${XDG_CACHE_HOME:-$HOME/.cache}/wallpaper-console-rust/theme-palettes"
MANIFEST="${WCR_THEME_MANIFEST:-}"
THEME_SOURCE="${WCR_THEME_SOURCE_OUTPUT:-}"

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

if [[ -z "$MANIFEST" || ! -f "$MANIFEST" ]]; then
  echo "pregenerate-from-manifest: WCR_THEME_MANIFEST missing or not a file" >&2
  exit 1
fi

if ! command -v matugen >/dev/null 2>&1; then
  echo "pregenerate-from-manifest: matugen not found in PATH" >&2
  exit 1
fi

if ! command -v python3 >/dev/null 2>&1; then
  echo "pregenerate-from-manifest: python3 required to parse theme-state.json" >&2
  exit 1
fi

mkdir -p "$PALETTE_ROOT"

# Known live paths produced by examples/matugen templates (after expansion).
copy_generated_into() {
  local dest="$1"
  mkdir -p "$dest"
  local pairs=(
    "$HOME/.config/waybar/colors.css:$dest/waybar-colors.css"
    "$HOME/.config/kitty/themes/matugen.conf:$dest/kitty-matugen.conf"
    "$HOME/.config/gtk-3.0/gtk.css:$dest/gtk-3.0-gtk.css"
    "$HOME/.config/gtk-4.0/gtk.css:$dest/gtk-4.0-gtk.css"
    "$HOME/.config/fuzzel/fuzzel_theme.ini:$dest/fuzzel_theme.ini"
  )
  local pair src out
  for pair in "${pairs[@]}"; do
    src="${pair%%:*}"
    out="${pair#*:}"
    if [[ -f "$src" ]]; then
      mkdir -p "$(dirname "$out")"
      cp -f "$src" "$out"
    fi
  done
}

mapfile -t ROWS < <(python3 - "$MANIFEST" <<'PY'
import json, sys
doc = json.load(open(sys.argv[1]))
for name, entry in (doc.get("outputs") or {}).items():
    still = entry.get("still") or ""
    print(f"{name}\t{still}")
PY
)

# Re-read theme_source from JSON if env empty.
if [[ -z "$THEME_SOURCE" ]]; then
  THEME_SOURCE="$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1])).get("theme_source_output") or "")' "$MANIFEST")"
fi

for row in "${ROWS[@]}"; do
  output="${row%%$'\t'*}"
  still="${row#*$'\t'}"
  [[ -z "$output" || -z "$still" || ! -f "$still" ]] && continue

  out_dir="$PALETTE_ROOT/$output"
  mkdir -p "$out_dir"
  # Prefer symlink; fall back to copy for cross-filesystem stills.
  ln -sfn "$still" "$out_dir/still.jpg" 2>/dev/null || cp -f "$still" "$out_dir/still.jpg"

  # Generate into the user's live matugen targets, then snapshot into the cache.
  # --prefer avoids interactive multi-color prompts; -m dark matches examples/matugen.
  matugen image "$still" -m dark --prefer darkness

  copy_generated_into "$out_dir"
done

if [[ -n "$THEME_SOURCE" ]]; then
  "$SCRIPT_DIR/activate-palette.sh" "$THEME_SOURCE"
fi
