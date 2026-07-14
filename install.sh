#!/usr/bin/env bash
# install.sh — build release Rust binaries and install Wallpaper Console.
#
# Usage:
#   ./install.sh              # build + install to ~/.local/bin
#   ./install.sh --build-only # build only, don't install
#   ./install.sh --prefix /usr/local  # custom install prefix
#   ./install.sh --uninstall          # remove files installed by this script
#
# Installs:
#   $PREFIX/bin/wallpaper-console-gui-rust  Tauri GUI launcher
#   $PREFIX/bin/wallpaper-console-rust      Rust CLI helper
#
# Does NOT touch or replace:
#   wallpaper-console        (Bash)
#   wallpaper-console-gui    (Python GTK)

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"

# ── Colour helpers ────────────────────────────────────────────────────────
RED='\033[0;31m'; GREEN='\033[0;32m'; YELLOW='\033[1;33m'; NC='\033[0m'
info()  { printf "${GREEN}[INFO]${NC} %s\n" "$*"; }
warn()  { printf "${YELLOW}[WARN]${NC} %s\n" "$*" >&2; }
err()   { printf "${RED}[ERROR]${NC} %s\n" "$*" >&2; exit 1; }

PREFIX="${PREFIX:-$HOME/.local}"
BUILD_ONLY=false
UNINSTALL=false

while [[ $# -gt 0 ]]; do
  case "$1" in
    --build-only) BUILD_ONLY=true ;;
    --uninstall) UNINSTALL=true ;;
    --prefix)
      if [[ $# -lt 2 || "$2" == --* ]]; then
        err "--prefix requires a value (e.g. --prefix /usr/local)"
      fi
      PREFIX="$2"; shift ;;
    --prefix=*) PREFIX="${1#*=}" ;;
    *)
      err "Unknown option: $1

Usage: $0 [--build-only] [--prefix DIR] [--uninstall]

  --build-only  Build only, don't install
  --prefix DIR  Install prefix (default: ~/.local)
  --uninstall   Remove files installed by this script" ;;
  esac
  shift
done

BIN_DIR="$PREFIX/bin"
LIBEXEC_DIR="$PREFIX/lib/wallpaper-console-rust"
APP_ID="wallpaper-console-gui-rust"
DESKTOP_DIR="$PREFIX/share/applications"
ICON_DIR="$PREFIX/share/icons/hicolor/128x128/apps"
DESKTOP_FILE="$DESKTOP_DIR/$APP_ID.desktop"
ICON_FILE="$ICON_DIR/$APP_ID.png"
GUI_BIN_FILE="$LIBEXEC_DIR/wallpaper-console-gui-rust"

if $UNINSTALL; then
  info "Uninstalling from $PREFIX..."
  rm -f "$BIN_DIR/wallpaper-console-rust"
  rm -f "$BIN_DIR/wallpaper-console-gui-rust"
  rm -f "$GUI_BIN_FILE"
  rmdir "$LIBEXEC_DIR" 2>/dev/null || true
  rm -f "$DESKTOP_FILE"
  rm -f "$ICON_FILE"
  info "Removed GUI launcher, Rust CLI helper, desktop entry, and icon."
  exit 0
fi

# ── Prerequisites check ───────────────────────────────────────────────────
check_cmd() { command -v "$1" >/dev/null 2>&1 || err "$1 is required but not found. Install it first."; }

if [[ "${WCR_INSTALL_SKIP_BUILD:-}" != "1" ]]; then
  info "Checking prerequisites..."
  check_cmd cargo
  check_cmd cargo-tauri
  check_cmd node
  check_cmd npm
fi

# ── Build Tauri GUI ────────────────────────────────────────────────────────
if [[ "${WCR_INSTALL_SKIP_BUILD:-}" == "1" ]]; then
  TAURI_BIN="${WCR_INSTALL_TAURI_BIN:?WCR_INSTALL_TAURI_BIN is required when WCR_INSTALL_SKIP_BUILD=1}"
  CLI_BIN="${WCR_INSTALL_CLI_BIN:?WCR_INSTALL_CLI_BIN is required when WCR_INSTALL_SKIP_BUILD=1}"
  FRONTEND_DIST="${WCR_INSTALL_FRONTEND_DIST:?WCR_INSTALL_FRONTEND_DIST is required when WCR_INSTALL_SKIP_BUILD=1}"
  info "Skipping build; using provided test artifacts."
else
  info "Installing frontend dependencies..."
  cd "$SCRIPT_DIR/apps/tauri-gui/frontend"
  npm ci
  info "Building Tauri GUI binary..."
  cd "$SCRIPT_DIR/apps/tauri-gui/src-tauri"
  cargo clean --package tauri --package wallpaper-console-tauri
  cargo tauri build --no-bundle --ci --features production
  info "Building Rust CLI helper..."
  cd "$SCRIPT_DIR"
  cargo build --package wc-cli --release
  TAURI_BIN="$(realpath "$SCRIPT_DIR/target/release/wallpaper-console-tauri")"
  CLI_BIN="$(realpath "$SCRIPT_DIR/target/release/wallpaper-console-rust")"
  FRONTEND_DIST="$SCRIPT_DIR/apps/tauri-gui/frontend/dist"
fi
info "Tauri GUI built: $TAURI_BIN"
info "Rust CLI helper built: $CLI_BIN"

# ── Verify GUI build artifacts ─────────────────────────────────────────────
info "Verifying GUI build artifacts..."

if [[ ! -f "$FRONTEND_DIST/index.html" ]]; then
  err "Frontend dist/index.html missing. Did npm run build succeed?"
fi

if [[ ! -x "$TAURI_BIN" ]]; then
  err "Tauri GUI binary not found or not executable: $TAURI_BIN"
fi

if [[ ! -x "$CLI_BIN" ]]; then
  err "Rust CLI helper binary not found or not executable: $CLI_BIN"
fi

info "GUI build artifacts verified."

# ── Install ────────────────────────────────────────────────────────────────
if $BUILD_ONLY; then
  info "Build-only mode — skipping install."
  info ""
  info "Built artifacts:"
  info "  Tauri GUI:     $TAURI_BIN"
  info "  Rust CLI:      $CLI_BIN"
  exit 0
fi

info "Installing to $BIN_DIR..."
mkdir -p "$BIN_DIR" "$LIBEXEC_DIR"

# Install Tauri GUI behind a launcher wrapper. Keep WebKitGTK's accelerated
# DMABUF path by default; an explicit compatibility switch remains available
# for Wayland setups that otherwise render a blank window.
cp "$TAURI_BIN" "$GUI_BIN_FILE"
chmod +x "$GUI_BIN_FILE"
cat > "$BIN_DIR/wallpaper-console-gui-rust" <<EOF_GUI_WRAPPER
#!/usr/bin/env sh
if [ "\${WCR_WEBKIT_DISABLE_DMABUF_RENDERER:-0}" = "1" ] && [ -z "\${WEBKIT_DISABLE_DMABUF_RENDERER+x}" ]; then
  export WEBKIT_DISABLE_DMABUF_RENDERER=1
fi
exec "$GUI_BIN_FILE" "\$@"
EOF_GUI_WRAPPER
chmod +x "$BIN_DIR/wallpaper-console-gui-rust"
info "  Installed: $BIN_DIR/wallpaper-console-gui-rust"
info "  Installed: $GUI_BIN_FILE"

# Install Rust CLI helper for compositor startup hooks such as restore.
cp "$CLI_BIN" "$BIN_DIR/wallpaper-console-rust"
chmod +x "$BIN_DIR/wallpaper-console-rust"
info "  Installed: $BIN_DIR/wallpaper-console-rust"

# Install desktop launcher and icon for Linux desktop environments.
mkdir -p "$DESKTOP_DIR" "$ICON_DIR"
cp "$SCRIPT_DIR/apps/tauri-gui/src-tauri/icons/128x128.png" "$ICON_FILE"
cat > "$DESKTOP_FILE" <<EOF_DESKTOP
[Desktop Entry]
Type=Application
Name=Wallpaper Console
Comment=Manage wallpapers with the Rust Tauri GUI
Exec=$BIN_DIR/wallpaper-console-gui-rust
Icon=$APP_ID
Terminal=false
Categories=Utility;Graphics;
EOF_DESKTOP
chmod 0644 "$DESKTOP_FILE"
info "  Installed: $DESKTOP_FILE"
info "  Installed: $ICON_FILE"

# ── Post-install ───────────────────────────────────────────────────────────
info ""
info "=============================================="
info " Installation complete"
info "=============================================="
info ""
info "Installed command:"
info "  wallpaper-console-gui-rust      Tauri GUI"
info "  wallpaper-console-rust restore-at-login  Restore saved display wallpapers at login when enabled"
info ""
info "The original Python command is untouched:"
info "  wallpaper-console            (Bash)"
info "  wallpaper-console-gui        (Python GTK)"

# Check PATH
if [[ ":$PATH:" != *":$BIN_DIR:"* ]]; then
  warn ""
  warn "$BIN_DIR is not in your PATH."
  warn "Add this to your shell profile:"
  warn ""
  warn "  export PATH=\"$BIN_DIR:\$PATH\""
fi

info ""
info "To run the Tauri GUI:"
info "  $BIN_DIR/wallpaper-console-gui-rust"
info "  Blank-window compatibility: WCR_WEBKIT_DISABLE_DMABUF_RENDERER=1 $BIN_DIR/wallpaper-console-gui-rust"
info ""
info "For niri startup restore:"
info "  $BIN_DIR/wallpaper-console-rust config-set restore_on_login on"
info "  spawn-at-startup \"$BIN_DIR/wallpaper-console-rust\" \"restore-at-login\""
info ""
info "For a niri launch binding:"
info "  spawn \"$BIN_DIR/wallpaper-console-gui-rust\""
info ""
info "Rollback:"
info "  ./install.sh --prefix \"$PREFIX\" --uninstall"
