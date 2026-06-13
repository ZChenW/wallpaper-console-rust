#!/usr/bin/env bash
# install.sh — build and install wallpaper-console-rust (CLI + Tauri GUI)
# side-by-side with existing Bash/Python versions.
#
# Usage:
#   ./install.sh              # build + install to ~/.local/bin
#   ./install.sh --build-only # build only, don't install
#   ./install.sh --prefix /usr/local  # custom install prefix
#   ./install.sh --uninstall          # remove files installed by this script
#
# Installs:
#   $PREFIX/bin/wallpaper-console-rust      Rust CLI
#   $PREFIX/bin/wallpaper-console-gui-rust  Tauri GUI
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
APP_ID="wallpaper-console-gui-rust"
DESKTOP_DIR="$PREFIX/share/applications"
ICON_DIR="$PREFIX/share/icons/hicolor/128x128/apps"
DESKTOP_FILE="$DESKTOP_DIR/$APP_ID.desktop"
ICON_FILE="$ICON_DIR/$APP_ID.png"

if $UNINSTALL; then
  info "Uninstalling from $PREFIX..."
  rm -f "$BIN_DIR/wallpaper-console-rust"
  rm -f "$BIN_DIR/wallpaper-console-gui-rust"
  rm -f "$DESKTOP_FILE"
  rm -f "$ICON_FILE"
  info "Removed Rust CLI/GUI launchers, desktop entry, and icon installed by this script."
  exit 0
fi

# ── Prerequisites check ───────────────────────────────────────────────────
check_cmd() { command -v "$1" >/dev/null 2>&1 || err "$1 is required but not found. Install it first."; }

info "Checking prerequisites..."
check_cmd cargo
check_cmd node
check_cmd npm

# ── Build Rust CLI ─────────────────────────────────────────────────────────
info "Building Rust CLI (release)..."
cd "$SCRIPT_DIR"
cargo build -p wc-cli --release
RUST_CLI="$(realpath target/release/wallpaper-console-rust)"
info "Rust CLI built: $RUST_CLI"

# ── Build Tauri GUI ────────────────────────────────────────────────────────
info "Building Tauri GUI..."
cd "$SCRIPT_DIR/apps/tauri-gui/src-tauri"
cargo tauri build --bundles deb,rpm
TAURI_BIN="$(realpath "$SCRIPT_DIR/target/release/wallpaper-console-tauri")"
info "Tauri GUI built: $TAURI_BIN"

# ── Verify binaries ────────────────────────────────────────────────────────
info "Verifying binaries..."

# Smoke test with temp config — never touches real ~/.config/wallpaper-console
tmp_config="$(mktemp -d)"
cleanup_tmp_config() { rm -rf "$tmp_config"; }
trap cleanup_tmp_config EXIT
XDG_CONFIG_HOME="$tmp_config" "$RUST_CLI" status >/dev/null 2>&1 \
  || warn "Rust CLI smoke test failed (may be ok if no config exists)"
"$RUST_CLI" --version >/dev/null 2>&1 || warn "Rust CLI --version failed"
cleanup_tmp_config
trap - EXIT

if [[ ! -x "$TAURI_BIN" ]]; then
  err "Tauri GUI binary not found or not executable: $TAURI_BIN"
fi
info "Binaries verified."

# ── Install ────────────────────────────────────────────────────────────────
if $BUILD_ONLY; then
  info "Build-only mode — skipping install."
  info ""
  info "Built artifacts:"
  info "  Rust CLI:      $RUST_CLI"
  info "  Tauri GUI:     $TAURI_BIN"
  exit 0
fi

info "Installing to $BIN_DIR..."
mkdir -p "$BIN_DIR"

# Install Rust CLI
cp "$RUST_CLI" "$BIN_DIR/wallpaper-console-rust"
chmod +x "$BIN_DIR/wallpaper-console-rust"
info "  Installed: $BIN_DIR/wallpaper-console-rust"

# Install Tauri GUI
cp "$TAURI_BIN" "$BIN_DIR/wallpaper-console-gui-rust"
chmod +x "$BIN_DIR/wallpaper-console-gui-rust"
info "  Installed: $BIN_DIR/wallpaper-console-gui-rust"

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
info "Installed commands:"
info "  wallpaper-console-rust          Rust CLI"
info "  wallpaper-console-gui-rust      Tauri GUI"
info ""
info "The original commands are untouched:"
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
info "To try the Rust CLI with a temp config:"
info "  XDG_CONFIG_HOME=\$(mktemp -d) wallpaper-console-rust status"
info ""
info "To run the Tauri GUI:"
info "  $BIN_DIR/wallpaper-console-gui-rust"
info ""
info "Rollback (restore original Bash/Python):"
info "  # Your original wallpaper-console and wallpaper-console-gui are untouched."
info "  # Simply remove the -rust variants if you no longer want them:"
info "  rm $BIN_DIR/wallpaper-console-rust"
info "  rm $BIN_DIR/wallpaper-console-gui-rust"
info "  # Or uninstall everything created by this script:"
info "  ./install.sh --prefix \"$PREFIX\" --uninstall"
