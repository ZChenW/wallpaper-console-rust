#!/usr/bin/env bash
# install.sh — build and install wallpaper-console-rust (CLI + Wails GUI)
# side-by-side with existing Bash/Python versions.
#
# Usage:
#   ./install.sh              # build + install to ~/.local/bin
#   ./install.sh --build-only # build only, don't install
#   ./install.sh --prefix /usr/local  # custom install prefix
#
# Installs:
#   $PREFIX/bin/wallpaper-console-rust      Rust CLI
#   $PREFIX/bin/wallpaper-console-gui-rust  Wails GUI
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

while [[ $# -gt 0 ]]; do
  case "$1" in
    --build-only) BUILD_ONLY=true ;;
    --prefix)
      if [[ $# -lt 2 || "$2" == --* ]]; then
        err "--prefix requires a value (e.g. --prefix /usr/local)"
      fi
      PREFIX="$2"; shift ;;
    --prefix=*) PREFIX="${1#*=}" ;;
    *)
      err "Unknown option: $1

Usage: $0 [--build-only] [--prefix DIR]

  --build-only  Build only, don't install
  --prefix DIR  Install prefix (default: ~/.local)" ;;
  esac
  shift
done

BIN_DIR="$PREFIX/bin"

# ── Prerequisites check ───────────────────────────────────────────────────
check_cmd() { command -v "$1" >/dev/null 2>&1 || err "$1 is required but not found. Install it first."; }

info "Checking prerequisites..."
check_cmd cargo
check_cmd go
check_cmd node
check_cmd npm

# wails3 may be in ~/go/bin
export PATH="$HOME/go/bin:$PATH"
check_cmd wails3

# ── Build Rust CLI ─────────────────────────────────────────────────────────
info "Building Rust CLI (release)..."
cd "$SCRIPT_DIR"
cargo build -p wc-cli --release
RUST_CLI="$(realpath target/release/wallpaper-console-rust)"
info "Rust CLI built: $RUST_CLI"

# ── Build Wails GUI ────────────────────────────────────────────────────────
info "Building Wails GUI..."
cd "$SCRIPT_DIR/apps/wails-gui"
wails3 build
WAILS_GUI="$(realpath bin/wallpaper-console-gui)"
info "Wails GUI built: $WAILS_GUI"

# ── Verify binaries ────────────────────────────────────────────────────────
info "Verifying binaries..."

# Smoke test with temp config — never touches real ~/.config/wallpaper-console
tmp_config="$(mktemp -d)"
trap "rm -rf '$tmp_config'" EXIT
XDG_CONFIG_HOME="$tmp_config" "$RUST_CLI" status >/dev/null 2>&1 \
  || warn "Rust CLI smoke test failed (may be ok if no config exists)"
"$RUST_CLI" --version >/dev/null 2>&1 || warn "Rust CLI --version failed"
rm -rf "$tmp_config"
trap - EXIT

if [[ ! -x "$WAILS_GUI" ]]; then
  err "Wails GUI binary not found or not executable: $WAILS_GUI"
fi

info "Binaries verified."

# ── Install ────────────────────────────────────────────────────────────────
if $BUILD_ONLY; then
  info "Build-only mode — skipping install."
  info ""
  info "Built artifacts:"
  info "  Rust CLI:   $RUST_CLI"
  info "  Wails GUI:  $WAILS_GUI"
  exit 0
fi

info "Installing to $BIN_DIR..."
mkdir -p "$BIN_DIR"

# Install Rust CLI
cp "$RUST_CLI" "$BIN_DIR/wallpaper-console-rust"
chmod +x "$BIN_DIR/wallpaper-console-rust"
info "  Installed: $BIN_DIR/wallpaper-console-rust"

# Install Wails GUI
cp "$WAILS_GUI" "$BIN_DIR/wallpaper-console-gui-rust"
chmod +x "$BIN_DIR/wallpaper-console-gui-rust"
info "  Installed: $BIN_DIR/wallpaper-console-gui-rust"

# ── Post-install ───────────────────────────────────────────────────────────
info ""
info "=============================================="
info " Installation complete"
info "=============================================="
info ""
info "Installed commands:"
info "  wallpaper-console-rust       Rust CLI"
info "  wallpaper-console-gui-rust   Wails GUI"
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
info "To run the Wails GUI (pointing at the Rust CLI):"
info "  WALLPAPER_CONSOLE_RUST=$BIN_DIR/wallpaper-console-rust wallpaper-console-gui-rust"
info ""
info "Rollback (restore original Bash/Python):"
info "  # Your original wallpaper-console and wallpaper-console-gui are untouched."
info "  # Simply remove the -rust variants if you no longer want them:"
info "  rm $BIN_DIR/wallpaper-console-rust"
info "  rm $BIN_DIR/wallpaper-console-gui-rust"
