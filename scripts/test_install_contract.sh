#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

fake_gui="$tmp/wallpaper-console-tauri"
fake_cli="$tmp/wallpaper-console-rust"
fake_dist="$tmp/dist"
prefix="$tmp/prefix"

mkdir -p "$fake_dist"
printf '#!/bin/sh\nprintf "gui %%s dmabuf=%%s\\n" "$*" "${WEBKIT_DISABLE_DMABUF_RENDERER-unset}"\n' >"$fake_gui"
printf '#!/bin/sh\necho cli "$@"\n' >"$fake_cli"
chmod +x "$fake_gui" "$fake_cli"
printf '<!doctype html><div id="root"></div>\n' >"$fake_dist/index.html"

WCR_INSTALL_SKIP_BUILD=1 \
WCR_INSTALL_TAURI_BIN="$fake_gui" \
WCR_INSTALL_CLI_BIN="$fake_cli" \
WCR_INSTALL_FRONTEND_DIST="$fake_dist" \
  "$ROOT/install.sh" --prefix "$prefix"

gui_wrapper="$prefix/bin/wallpaper-console-gui-rust"
gui_bin="$prefix/lib/wallpaper-console-rust/wallpaper-console-gui-rust"
cli_bin="$prefix/bin/wallpaper-console-rust"
desktop="$prefix/share/applications/wallpaper-console-gui-rust.desktop"

test -x "$gui_wrapper"
test -x "$gui_bin"
test -x "$cli_bin"
test -f "$desktop"

grep -q "$gui_bin" "$gui_wrapper"
grep -q "Exec=$gui_wrapper" "$desktop"

env -u WEBKIT_DISABLE_DMABUF_RENDERER "$gui_wrapper" --version \
  | grep -q 'gui --version dmabuf=unset'
env -u WEBKIT_DISABLE_DMABUF_RENDERER \
  WCR_WEBKIT_DISABLE_DMABUF_RENDERER=1 "$gui_wrapper" --version \
  | grep -q 'gui --version dmabuf=1'
WEBKIT_DISABLE_DMABUF_RENDERER=external "$gui_wrapper" --version \
  | grep -q 'gui --version dmabuf=external'
"$cli_bin" restore | grep -q 'cli restore'

"$ROOT/install.sh" --prefix "$prefix" --uninstall
test ! -e "$gui_wrapper"
test ! -e "$gui_bin"
test ! -e "$cli_bin"
test ! -e "$desktop"

echo "PASS: install contract"
