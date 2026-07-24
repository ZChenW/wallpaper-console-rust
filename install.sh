#!/usr/bin/env bash
# install.sh — build release Rust binaries from this workspace and install them.
#
# Always:
#   1) build from the directory that contains this script (project root)
#   2) write release artifacts to $ROOT/target/release
#   3) install only those freshly built binaries
#
# Usage:
#   ./install.sh              # build + install to ~/.local/bin
#   ./install.sh --build-only # build only, don't install
#   ./install.sh --prefix /usr/local  # custom install prefix
#   ./install.sh --uninstall          # remove files installed by this script
#   ./install.sh --force              # replace an untracked legacy install
#   ./install.sh --help               # show usage
#
# Installs:
#   $PREFIX/bin/wallpaper-console-gui-rust  Tauri GUI launcher
#   $PREFIX/bin/wallpaper-console-rust      Rust CLI helper
#   $PREFIX/share/licenses/wallpaper-console-rust/LICENSE
#   $PREFIX/share/wallpaper-console-rust/install-manifest-v1
#
# Does NOT touch or replace:
#   wallpaper-console        (Bash)
#   wallpaper-console-gui    (Python GTK)

set -euo pipefail

ROOT="$(cd "$(dirname "$0")" && pwd)"

# ── Colour helpers ────────────────────────────────────────────────────────
RED='\033[0;31m'; GREEN='\033[0;32m'; YELLOW='\033[1;33m'; NC='\033[0m'
info()  { printf "${GREEN}[INFO]${NC} %s\n" "$*"; }
warn()  { printf "${YELLOW}[WARN]${NC} %s\n" "$*" >&2; }
err()   { printf "${RED}[ERROR]${NC} %s\n" "$*" >&2; exit 1; }
shell_quote() { printf '%q' "$1"; }

usage() {
  cat <<'EOF'
Wallpaper Console installer

Usage:
  ./install.sh [options]

Options:
  --build-only       Build release binaries without installing them
  --prefix DIR       Install under DIR (default: ~/.local)
  --uninstall        Remove files installed by this script
  --force            Replace conflicting files from an untracked legacy install
  -h, --help         Show this help

The default installation is user-local and does not require root access.
EOF
}

PREFIX="${PREFIX:-$HOME/.local}"
BUILD_ONLY=false
UNINSTALL=false
FORCE=false

while [[ $# -gt 0 ]]; do
  case "$1" in
    --build-only) BUILD_ONLY=true ;;
    --uninstall) UNINSTALL=true ;;
    --force) FORCE=true ;;
    -h|--help)
      usage
      exit 0
      ;;
    --prefix)
      if [[ $# -lt 2 || "$2" == --* ]]; then
        err "--prefix requires a value (e.g. --prefix /usr/local)"
      fi
      PREFIX="$2"; shift ;;
    --prefix=*) PREFIX="${1#*=}" ;;
    *)
      usage >&2
      err "Unknown option: $1"
      ;;
  esac
  shift
done

[[ -n "$PREFIX" ]] || err "--prefix must not be empty"
[[ "$PREFIX" == /* ]] || err "--prefix must be an absolute path (found: $PREFIX)"
if [[ "$PREFIX" == *$'\n'* || "$PREFIX" == *$'\r'* ]]; then
  err "--prefix must not contain newline characters"
fi
if [[ "$PREFIX" == *\\* ]]; then
  err "--prefix must not contain backslash characters (Desktop Entry launchers cannot represent them reliably)"
fi
while [[ "$PREFIX" != "/" && "$PREFIX" == */ ]]; do
  PREFIX="${PREFIX%/}"
done
[[ "$PREFIX" != "/" ]] || err "--prefix must not be the filesystem root"
case "$PREFIX/" in
  *"/../"*|*"/./"*|*"//"*)
    err "--prefix must not contain '.', '..', or empty path components"
    ;;
esac
if $UNINSTALL && $BUILD_ONLY; then
  err "--uninstall and --build-only cannot be used together"
fi
if $UNINSTALL && $FORCE; then
  err "--force is only valid when installing"
fi

BIN_DIR="$PREFIX/bin"
LIBEXEC_DIR="$PREFIX/lib/wallpaper-console-rust"
APP_ID="wallpaper-console-gui-rust"
LICENSE_DIR="$PREFIX/share/licenses/wallpaper-console-rust"
TARGET_DIR="$ROOT/target"
TAURI_BIN_NAME="wallpaper-console-tauri"
CLI_BIN_NAME="wallpaper-console-rust"
MANIFEST_REL="share/wallpaper-console-rust/install-manifest-v1"
MANIFEST_FILE="$PREFIX/$MANIFEST_REL"
MANIFEST_HEADER="wallpaper-console-rust-install-manifest-v1"
INSTALL_LOCK_DIR="$PREFIX/.wallpaper-console-rust.install.lock"
INSTALL_LOCK_OWNER="$INSTALL_LOCK_DIR/owner-pid"
INSTALL_LOCK_HELD=false
OWNED_RELATIVE_PATHS=(
  "bin/wallpaper-console-gui-rust"
  "bin/wallpaper-console-rust"
  "lib/wallpaper-console-rust/wallpaper-console-gui-rust"
  "share/applications/wallpaper-console-gui-rust.desktop"
  "share/icons/hicolor/128x128/apps/wallpaper-console-gui-rust.png"
  "share/licenses/wallpaper-console-rust/LICENSE"
)
declare -A MANIFEST_HASH_BY_PATH=()
SAFE_PARENT_CHAIN_ERROR=""

is_owned_relative_path() {
  case "$1" in
    "bin/wallpaper-console-gui-rust" \
      |"bin/wallpaper-console-rust" \
      |"lib/wallpaper-console-rust/wallpaper-console-gui-rust" \
      |"share/applications/wallpaper-console-gui-rust.desktop" \
      |"share/icons/hicolor/128x128/apps/wallpaper-console-gui-rust.png" \
      |"share/licenses/wallpaper-console-rust/LICENSE")
      return 0
      ;;
    *)
      return 1
      ;;
  esac
}

file_sha256() {
  sha256sum -- "$1" | awk '{print $1}'
}

check_safe_prefix_ancestors() {
  local current="/"
  local component=""
  local components=()

  SAFE_PARENT_CHAIN_ERROR=""
  IFS='/' read -r -a components <<< "${PREFIX#/}"
  for component in "${components[@]}"; do
    current="${current%/}/$component"
    if [[ -L "$current" ]]; then
      SAFE_PARENT_CHAIN_ERROR="Install prefix and its ancestors must not be symbolic links: $current"
      return 1
    fi
    if [[ -e "$current" && ! -d "$current" ]]; then
      SAFE_PARENT_CHAIN_ERROR="Install prefix ancestor is not a directory: $current"
      return 1
    fi
  done
  return 0
}

check_safe_parent_chain() {
  local relative="$1"
  local parent="${relative%/*}"
  local current="$PREFIX"
  local component=""
  local components=()

  check_safe_prefix_ancestors || return 1
  IFS='/' read -r -a components <<< "$parent"
  for component in "${components[@]}"; do
    current="$current/$component"
    if [[ -L "$current" ]]; then
      SAFE_PARENT_CHAIN_ERROR="Refusing to follow symbolic-link install directory: $current"
      return 1
    fi
    if [[ -e "$current" && ! -d "$current" ]]; then
      SAFE_PARENT_CHAIN_ERROR="Install parent path is not a directory: $current"
      return 1
    fi
  done
  return 0
}

assert_safe_parent_chain() {
  local relative="$1"
  check_safe_parent_chain "$relative" || err "$SAFE_PARENT_CHAIN_ERROR"
}

validate_owned_parent_paths() {
  local relative=""
  for relative in "${OWNED_RELATIVE_PATHS[@]}" "$MANIFEST_REL"; do
    assert_safe_parent_chain "$relative"
  done
}

release_install_lock() {
  if ! $INSTALL_LOCK_HELD; then
    return
  fi
  rm -f -- "$INSTALL_LOCK_OWNER"
  if ! rmdir -- "$INSTALL_LOCK_DIR" 2>/dev/null; then
    warn "Could not remove install lock directory: $INSTALL_LOCK_DIR"
  fi
  INSTALL_LOCK_HELD=false
}

acquire_install_lock() {
  validate_owned_parent_paths
  mkdir -p -- "$PREFIX"
  [[ ! -L "$INSTALL_LOCK_DIR" ]] \
    || err "Install lock path must not be a symbolic link: $INSTALL_LOCK_DIR"

  if ! mkdir -- "$INSTALL_LOCK_DIR" 2>/dev/null; then
    [[ -d "$INSTALL_LOCK_DIR" && ! -L "$INSTALL_LOCK_DIR" ]] \
      || err "Install lock path is not a directory: $INSTALL_LOCK_DIR"
    [[ -f "$INSTALL_LOCK_OWNER" && ! -L "$INSTALL_LOCK_OWNER" ]] \
      || err "Install lock is malformed; inspect and remove it manually: $INSTALL_LOCK_DIR"

    local owner_pid=""
    IFS= read -r owner_pid < "$INSTALL_LOCK_OWNER" || true
    [[ "$owner_pid" =~ ^[1-9][0-9]*$ ]] \
      || err "Install lock has an invalid owner PID: $INSTALL_LOCK_DIR"
    if [[ -d "/proc/$owner_pid" ]]; then
      err "Another install or uninstall is already active for $PREFIX (PID $owner_pid)."
    fi

    warn "Removing stale install lock left by PID $owner_pid."
    rm -f -- "$INSTALL_LOCK_OWNER"
    rmdir -- "$INSTALL_LOCK_DIR" 2>/dev/null \
      || err "Stale install lock contains unexpected files: $INSTALL_LOCK_DIR"
    mkdir -- "$INSTALL_LOCK_DIR" \
      || err "Could not acquire install lock: $INSTALL_LOCK_DIR"
  fi

  INSTALL_LOCK_HELD=true
  trap release_install_lock EXIT
  printf '%s\n' "$$" > "$INSTALL_LOCK_OWNER"
  chmod 0600 "$INSTALL_LOCK_OWNER"
}

load_install_manifest() {
  local header=""
  local hash=""
  local relative=""
  local extra=""
  local seen=0

  [[ -f "$MANIFEST_FILE" && ! -L "$MANIFEST_FILE" ]] \
    || err "Install manifest is not a regular file: $MANIFEST_FILE"

  MANIFEST_HASH_BY_PATH=()
  {
    IFS= read -r header || err "Install manifest is empty: $MANIFEST_FILE"
    [[ "$header" == "$MANIFEST_HEADER" ]] \
      || err "Unsupported or damaged install manifest: $MANIFEST_FILE"

    while IFS=$'\t' read -r hash relative extra; do
      [[ "$hash" =~ ^[[:xdigit:]]{64}$ && -n "$relative" && -z "$extra" ]] \
        || err "Malformed entry in install manifest: $MANIFEST_FILE"
      is_owned_relative_path "$relative" \
        || err "Unsafe path in install manifest: $relative"
      [[ -z "${MANIFEST_HASH_BY_PATH[$relative]+present}" ]] \
        || err "Duplicate path in install manifest: $relative"
      MANIFEST_HASH_BY_PATH["$relative"]="${hash,,}"
      ((seen += 1))
    done
  } < "$MANIFEST_FILE"

  [[ "$seen" -eq "${#OWNED_RELATIVE_PATHS[@]}" ]] \
    || err "Install manifest has an unexpected number of entries: $MANIFEST_FILE"
  for relative in "${OWNED_RELATIVE_PATHS[@]}"; do
    [[ -n "${MANIFEST_HASH_BY_PATH[$relative]+present}" ]] \
      || err "Install manifest is missing owned path: $relative"
  done
}

check_existing_install_for_upgrade() {
  local announce_force="${1:-true}"
  validate_owned_parent_paths
  command -v sha256sum >/dev/null 2>&1 \
    || err "sha256sum is required to verify install ownership."

  if [[ -e "$MANIFEST_FILE" || -L "$MANIFEST_FILE" ]]; then
    load_install_manifest
    local relative=""
    local target=""
    local actual=""
    for relative in "${OWNED_RELATIVE_PATHS[@]}"; do
      target="$PREFIX/$relative"
      [[ -f "$target" && ! -L "$target" ]] \
        || err "Owned install file is missing or not regular: $target"
      actual="$(file_sha256 "$target")"
      [[ "$actual" == "${MANIFEST_HASH_BY_PATH[$relative]}" ]] \
        || err "Owned install file was modified; refusing to overwrite it: $target"
    done
    return
  fi

  local conflicts=()
  local relative=""
  local target=""
  for relative in "${OWNED_RELATIVE_PATHS[@]}"; do
    target="$PREFIX/$relative"
    if [[ -e "$target" || -L "$target" ]]; then
      [[ ! -d "$target" || -L "$target" ]] \
        || err "Install target is a directory and cannot be replaced: $target"
      conflicts+=("$target")
    fi
  done

  if (( ${#conflicts[@]} > 0 )); then
    if ! $FORCE; then
      err "Existing untracked install files would be overwritten: ${conflicts[*]}

Move them aside, or rerun with --force to adopt and replace this legacy install."
    fi
    if $announce_force; then
      warn "Replacing ${#conflicts[@]} untracked legacy install file(s) because --force was supplied."
    fi
  fi
}

uninstall_owned_files() {
  info "Uninstalling from $PREFIX..."
  if [[ ! -e "$PREFIX" && ! -L "$PREFIX" ]]; then
    info "Nothing installed by this installer."
    return
  fi
  validate_owned_parent_paths
  acquire_install_lock
  command -v sha256sum >/dev/null 2>&1 \
    || err "sha256sum is required to verify install ownership."

  if [[ ! -e "$MANIFEST_FILE" && ! -L "$MANIFEST_FILE" ]]; then
    local found_untracked=false
    local relative=""
    for relative in "${OWNED_RELATIVE_PATHS[@]}"; do
      if [[ -e "$PREFIX/$relative" || -L "$PREFIX/$relative" ]]; then
        found_untracked=true
        break
      fi
    done
    if $found_untracked; then
      warn "No ownership manifest found; existing files were preserved. Use a normal install with --force to adopt a legacy install."
    else
      info "Nothing installed by this installer."
    fi
    return
  fi

  load_install_manifest
  local relative=""
  local target=""
  local actual=""
  local removed=0
  local preserved=0
  for relative in "${OWNED_RELATIVE_PATHS[@]}"; do
    target="$PREFIX/$relative"
    if [[ ! -e "$target" && ! -L "$target" ]]; then
      continue
    fi
    if [[ ! -f "$target" || -L "$target" ]]; then
      warn "Preserving changed owned path: $target"
      ((preserved += 1))
      continue
    fi
    actual="$(file_sha256 "$target")"
    if [[ "$actual" != "${MANIFEST_HASH_BY_PATH[$relative]}" ]]; then
      warn "Preserving modified owned file: $target"
      ((preserved += 1))
      continue
    fi
    rm -f -- "$target"
    ((removed += 1))
  done

  rm -f -- "$MANIFEST_FILE"
  rmdir "$PREFIX/share/wallpaper-console-rust" 2>/dev/null || true
  rmdir "$LIBEXEC_DIR" 2>/dev/null || true
  rmdir "$LICENSE_DIR" 2>/dev/null || true
  info "Removed $removed verified owned file(s); preserved $preserved changed file(s)."
}

if $UNINSTALL; then
  uninstall_owned_files
  exit 0
fi

if ! $BUILD_ONLY; then
  check_existing_install_for_upgrade false
fi

# ── Prerequisites check ───────────────────────────────────────────────────
ARCH_RECOMMENDED_BUILD_PACKAGES=(
  webkit2gtk-4.1
  base-devel
  curl
  wget
  file
  openssl
  appmenu-gtk-module
  libappindicator-gtk3
  librsvg
  xdotool
  zenity
)

version_at_least() {
  local actual="$1"
  local required="$2"
  [[ "$(printf '%s\n%s\n' "$required" "$actual" | sort -V | head -n 1)" == "$required" ]]
}

check_prerequisites() {
  local missing_commands=()
  local command_name=""

  for command_name in cargo rustc node npm; do
    if ! command -v "$command_name" >/dev/null 2>&1; then
      missing_commands+=("$command_name")
    fi
  done

  if (( ${#missing_commands[@]} > 0 )); then
    if command -v pacman >/dev/null 2>&1; then
      err "Missing build tools: ${missing_commands[*]}

Install them on Arch Linux with:
  sudo pacman -S --needed rust nodejs npm"
    fi
    err "Missing build tools: ${missing_commands[*]}. Install Rust, Node.js, and npm first."
  fi

  local rust_version
  rust_version="$(rustc --version | awk '{print $2}')"
  if ! version_at_least "$rust_version" "1.88.0"; then
    err "Rust 1.88.0 or newer is required (found $rust_version)."
  fi

  local node_version
  node_version="$(node --version)"
  node_version="${node_version#v}"
  if ! version_at_least "$node_version" "22.6.0"; then
    err "Node.js 22.6.0 or newer is required (found $node_version)."
  fi

  if command -v pacman >/dev/null 2>&1; then
    local missing_packages=()
    mapfile -t missing_packages < <(
      pacman -T "${ARCH_RECOMMENDED_BUILD_PACKAGES[@]}" 2>/dev/null || true
    )
    if (( ${#missing_packages[@]} > 0 )); then
      warn "Recommended Arch Linux packages not found: ${missing_packages[*]}"
      warn "If the build fails, install them with:"
      warn "  sudo pacman -S --needed ${missing_packages[*]}"
    fi
  fi

  if ! command -v zenity >/dev/null 2>&1 \
    && ! command -v kdialog >/dev/null 2>&1 \
    && ! command -v yad >/dev/null 2>&1; then
    warn "No supported directory picker found. Install zenity, kdialog, or yad to add folders from the GUI."
  fi
}

ensure_build_tmpdir() {
  # cc/cargo write large temps under TMPDIR. Small/full /tmp (tmpfs + quota)
  # fails with "Disk quota exceeded" while compiling bundled sqlite.
  local preferred="${WCR_BUILD_TMPDIR:-$HOME/tmp/rust-tmp}"
  local avail_kb=""
  mkdir -p "$preferred"
  avail_kb="$(df -Pk /tmp 2>/dev/null | awk 'NR==2 {print $4}')"
  case "${TMPDIR:-}" in
    ""|/tmp|/tmp/*)
      # < 2 GiB free on /tmp → prefer home.
      if [[ -z "$avail_kb" || "$avail_kb" -lt 2097152 ]]; then
        export TMPDIR="$preferred"
        info "Using TMPDIR=$TMPDIR (/tmp free space insufficient)"
      fi
      ;;
  esac
}

# ── Build from current workspace source ────────────────────────────────────
if [[ "${WCR_INSTALL_SKIP_BUILD:-}" == "1" ]]; then
  TAURI_BIN="${WCR_INSTALL_TAURI_BIN:?WCR_INSTALL_TAURI_BIN is required when WCR_INSTALL_SKIP_BUILD=1}"
  CLI_BIN="${WCR_INSTALL_CLI_BIN:?WCR_INSTALL_CLI_BIN is required when WCR_INSTALL_SKIP_BUILD=1}"
  FRONTEND_DIST="${WCR_INSTALL_FRONTEND_DIST:?WCR_INSTALL_FRONTEND_DIST is required when WCR_INSTALL_SKIP_BUILD=1}"
  info "Skipping build; using provided test artifacts."
else
  info "Checking prerequisites..."
  check_prerequisites

  ensure_build_tmpdir

  # Predictable artifact location — ignore Cursor/sandbox CARGO_TARGET_DIR.
  if [[ -n "${CARGO_TARGET_DIR:-}" && "$CARGO_TARGET_DIR" != "$TARGET_DIR" ]]; then
    warn "Overriding CARGO_TARGET_DIR=$CARGO_TARGET_DIR → $TARGET_DIR"
  fi
  export CARGO_TARGET_DIR="$TARGET_DIR"
  info "CARGO_TARGET_DIR=$CARGO_TARGET_DIR"
  info "Building from workspace: $ROOT"

  # Remove previous release binaries so a failed build cannot install leftovers.
  rm -f "$TARGET_DIR/release/$TAURI_BIN_NAME" "$TARGET_DIR/release/$CLI_BIN_NAME"

  info "Installing frontend dependencies..."
  (
    cd "$ROOT/apps/tauri-gui/frontend"
    npm ci
  )

  info "Building frontend (production)..."
  (
    cd "$ROOT/apps/tauri-gui/frontend"
    npm run build
  )

  FRONTEND_DIST="$ROOT/apps/tauri-gui/frontend/dist"
  if [[ ! -s "$FRONTEND_DIST/index.html" ]]; then
    err "Frontend dist/index.html missing after npm run build"
  fi

  info "Building release binaries (cargo build --release)..."
  (
    cd "$ROOT"
    # No cargo clean — keep incremental cache. Fail → set -e exits before install.
    cargo build --locked --release \
      --package wallpaper-console-tauri \
      --features production
    cargo build --locked --release --package wc-cli
  )

  TAURI_BIN="$TARGET_DIR/release/$TAURI_BIN_NAME"
  CLI_BIN="$TARGET_DIR/release/$CLI_BIN_NAME"

  # Binaries were deleted before build; existence here proves this run produced them.
  if [[ ! -x "$TAURI_BIN" ]]; then
    err "Release build did not produce executable: $TAURI_BIN"
  fi
  if [[ ! -x "$CLI_BIN" ]]; then
    err "Release build did not produce executable: $CLI_BIN"
  fi
fi

info "Tauri GUI built: $TAURI_BIN"
info "Rust CLI helper built: $CLI_BIN"

# ── Verify GUI build artifacts ─────────────────────────────────────────────
info "Verifying GUI build artifacts..."

if [[ ! -s "$FRONTEND_DIST/index.html" ]]; then
  err "Frontend dist/index.html missing. Did npm run build succeed?"
fi

if [[ ! -x "$TAURI_BIN" || ! -s "$TAURI_BIN" ]]; then
  err "Tauri GUI binary not found or not executable: $TAURI_BIN"
fi

if [[ ! -x "$CLI_BIN" || ! -s "$CLI_BIN" ]]; then
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

# Recheck after the potentially long build so a concurrently changed legacy or
# owned file is never silently overwritten.
acquire_install_lock
check_existing_install_for_upgrade

STAGING_DIR=""
PUBLISH_STARTED=false
PUBLISH_COMPLETE=false
ROLLBACK_INCOMPLETE=false
PUBLISHED_RELATIVE_PATHS=()

cleanup_install_stage() {
  local status="$1"
  if [[ "$status" -ne 0 ]] && $PUBLISH_STARTED && ! $PUBLISH_COMPLETE; then
    warn "Install publish failed; restoring the previous owned files."
    local index=0
    local relative=""
    local target=""
    local backup=""
    for ((index=${#PUBLISHED_RELATIVE_PATHS[@]} - 1; index >= 0; index--)); do
      relative="${PUBLISHED_RELATIVE_PATHS[$index]}"
      target="$PREFIX/$relative"
      backup="$STAGING_DIR/.backup/$relative"
      if ! check_safe_parent_chain "$relative"; then
        warn "Refusing unsafe rollback for $target: $SAFE_PARENT_CHAIN_ERROR"
        ROLLBACK_INCOMPLETE=true
        continue
      fi
      if [[ -e "$backup" || -L "$backup" ]]; then
        mv -fT -- "$backup" "$target" \
          || warn "Could not restore previous install file: $target"
      else
        rm -f -- "$target" \
          || warn "Could not remove partially published file: $target"
      fi
    done
  fi

  if [[ -n "$STAGING_DIR" ]]; then
    if $ROLLBACK_INCOMPLETE; then
      warn "Preserving install staging tree for manual recovery: $STAGING_DIR"
      return
    fi
    case "$STAGING_DIR" in
      "$PREFIX"/.wallpaper-console-rust.install.*)
        rm -rf -- "$STAGING_DIR"
        ;;
      *)
        warn "Refusing to remove unexpected staging path: $STAGING_DIR"
        ;;
    esac
  fi
}

cleanup_install_process() {
  local status=$?
  set +e
  cleanup_install_stage "$status"
  release_install_lock
  return "$status"
}

publish_staged_file() {
  local relative="$1"
  local staged="$STAGING_DIR/$relative"
  local target="$PREFIX/$relative"
  local backup="$STAGING_DIR/.backup/$relative"

  assert_safe_parent_chain "$relative"
  mkdir -p -- "$(dirname -- "$target")"
  assert_safe_parent_chain "$relative"
  if [[ -e "$target" || -L "$target" ]]; then
    [[ ! -d "$target" || -L "$target" ]] \
      || err "Install target is a directory and cannot be replaced: $target"
    mkdir -p -- "$(dirname -- "$backup")"
    cp -a -- "$target" "$backup"
  fi

  # Pure shell cannot make the final lstat-to-rename interval indivisible, but
  # this catches any stable parent replacement before the atomic publish.
  assert_safe_parent_chain "$relative"
  mv -fT -- "$staged" "$target"
  PUBLISHED_RELATIVE_PATHS+=("$relative")

  local fail_after="${WCR_INSTALL_FAIL_AFTER_PUBLISH:-0}"
  [[ "$fail_after" =~ ^[0-9]+$ ]] \
    || err "WCR_INSTALL_FAIL_AFTER_PUBLISH must be a non-negative integer"
  if [[ "$fail_after" -gt 0 && "${#PUBLISHED_RELATIVE_PATHS[@]}" -eq "$fail_after" ]]; then
    err "Injected install publish failure after $fail_after file(s)"
  fi
}

info "Staging installation under $PREFIX..."
mkdir -p -- "$PREFIX"
STAGING_DIR="$(mktemp -d -- "$PREFIX/.wallpaper-console-rust.install.XXXXXX")"
trap cleanup_install_process EXIT

for relative in "${OWNED_RELATIVE_PATHS[@]}" "$MANIFEST_REL"; do
  mkdir -p -- "$(dirname -- "$STAGING_DIR/$relative")"
done

# Generate every payload in the same-filesystem staging tree. Nothing becomes
# visible at the final paths until all payloads have been validated.
cp -- "$TAURI_BIN" "$STAGING_DIR/lib/wallpaper-console-rust/wallpaper-console-gui-rust"
chmod 0755 "$STAGING_DIR/lib/wallpaper-console-rust/wallpaper-console-gui-rust"
cat > "$STAGING_DIR/bin/wallpaper-console-gui-rust" <<'EOF_GUI_WRAPPER'
#!/usr/bin/env sh
if [ "${WCR_WEBKIT_DISABLE_DMABUF_RENDERER:-0}" = "1" ] && [ -z "${WEBKIT_DISABLE_DMABUF_RENDERER+x}" ]; then
  export WEBKIT_DISABLE_DMABUF_RENDERER=1
fi
launcher_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
install_prefix=$(CDPATH= cd -- "$launcher_dir/.." && pwd)
exec "$install_prefix/lib/wallpaper-console-rust/wallpaper-console-gui-rust" "$@"
EOF_GUI_WRAPPER
chmod 0755 "$STAGING_DIR/bin/wallpaper-console-gui-rust"

cp -- "$CLI_BIN" "$STAGING_DIR/bin/wallpaper-console-rust"
chmod 0755 "$STAGING_DIR/bin/wallpaper-console-rust"
cp -- "$ROOT/LICENSE" "$STAGING_DIR/share/licenses/wallpaper-console-rust/LICENSE"
chmod 0644 "$STAGING_DIR/share/licenses/wallpaper-console-rust/LICENSE"
cp -- "$ROOT/apps/tauri-gui/src-tauri/icons/128x128.png" \
  "$STAGING_DIR/share/icons/hicolor/128x128/apps/wallpaper-console-gui-rust.png"
chmod 0644 "$STAGING_DIR/share/icons/hicolor/128x128/apps/wallpaper-console-gui-rust.png"

DESKTOP_EXEC="$BIN_DIR/wallpaper-console-gui-rust"
DESKTOP_EXEC="${DESKTOP_EXEC//\\/\\\\}"
DESKTOP_EXEC="${DESKTOP_EXEC//\"/\\\"}"
DESKTOP_EXEC="${DESKTOP_EXEC//\`/\\\`}"
DESKTOP_EXEC="${DESKTOP_EXEC//\$/\\\$}"
DESKTOP_EXEC="${DESKTOP_EXEC//%/%%}"
cat > "$STAGING_DIR/share/applications/wallpaper-console-gui-rust.desktop" <<EOF_DESKTOP
[Desktop Entry]
Type=Application
Name=Wallpaper Console
Comment=Manage wallpapers with the Rust Tauri GUI
Exec="$DESKTOP_EXEC"
TryExec=$BIN_DIR/wallpaper-console-gui-rust
Icon=$APP_ID
Terminal=false
StartupNotify=true
Categories=Graphics;
Keywords=Wallpaper;Background;Wayland;
EOF_DESKTOP
chmod 0644 "$STAGING_DIR/share/applications/wallpaper-console-gui-rust.desktop"
if command -v desktop-file-validate >/dev/null 2>&1; then
  desktop-file-validate \
    "$STAGING_DIR/share/applications/wallpaper-console-gui-rust.desktop"
fi

for relative in "${OWNED_RELATIVE_PATHS[@]}"; do
  staged="$STAGING_DIR/$relative"
  [[ -f "$staged" && ! -L "$staged" && -s "$staged" ]] \
    || err "Staged payload is missing or not regular: $relative"
done
for relative in \
  "bin/wallpaper-console-gui-rust" \
  "bin/wallpaper-console-rust" \
  "lib/wallpaper-console-rust/wallpaper-console-gui-rust"; do
  [[ "$(stat -c '%a' "$STAGING_DIR/$relative")" == "755" ]] \
    || err "Staged executable has the wrong mode: $relative"
done
for relative in \
  "share/applications/wallpaper-console-gui-rust.desktop" \
  "share/icons/hicolor/128x128/apps/wallpaper-console-gui-rust.png" \
  "share/licenses/wallpaper-console-rust/LICENSE"; do
  [[ "$(stat -c '%a' "$STAGING_DIR/$relative")" == "644" ]] \
    || err "Staged data file has the wrong mode: $relative"
done

{
  printf '%s\n' "$MANIFEST_HEADER"
  for relative in "${OWNED_RELATIVE_PATHS[@]}"; do
    printf '%s\t%s\n' "$(file_sha256 "$STAGING_DIR/$relative")" "$relative"
  done
} > "$STAGING_DIR/$MANIFEST_REL"
chmod 0644 "$STAGING_DIR/$MANIFEST_REL"

# Publish each complete file with a same-filesystem rename. The ownership
# manifest is deliberately last, so it never describes a partially staged set.
PUBLISH_STARTED=true
for relative in "${OWNED_RELATIVE_PATHS[@]}"; do
  publish_staged_file "$relative"
done
publish_staged_file "$MANIFEST_REL"
PUBLISH_COMPLETE=true
cleanup_install_stage 0
STAGING_DIR=""
release_install_lock
trap - EXIT

info "Installed verified payloads:"
for relative in "${OWNED_RELATIVE_PATHS[@]}"; do
  info "  $PREFIX/$relative"
done
info "  $MANIFEST_FILE"

# ── Post-install ───────────────────────────────────────────────────────────
info ""
info "Installation complete."
info "Open Wallpaper Console from the application menu or run:"
info "  $(shell_quote "$BIN_DIR/wallpaper-console-gui-rust")"

# Check PATH
if [[ ":$PATH:" != *":$BIN_DIR:"* ]]; then
  warn ""
  warn "$BIN_DIR is not in your PATH."
  warn "Add this to your Bash or Zsh shell profile:"
  warn ""
  warn "  export PATH=$(shell_quote "$BIN_DIR"):\"\$PATH\""
fi

info ""
info "Uninstall:"
info "  $(shell_quote "$ROOT/install.sh") --prefix $(shell_quote "$PREFIX") --uninstall"
