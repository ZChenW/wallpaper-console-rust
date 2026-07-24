#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

if ! command -v rg >/dev/null 2>&1; then
  echo "rg (ripgrep) is required for drift checks" >&2
  exit 2
fi

SCHEMA="${DRIFT_CONFIG_SCHEMA:-$ROOT/apps/tauri-gui/frontend/src/settings/configSchema.ts}"

bad=0

workspace_version="$(
  awk '
    /^\[workspace\.package\]$/ { in_workspace_package = 1; next }
    /^\[/ { in_workspace_package = 0 }
    in_workspace_package && /^version[[:space:]]*=/ {
      gsub(/"/, "", $3)
      print $3
      exit
    }
  ' "$ROOT/Cargo.toml"
)"
tauri_version="$(
  sed -nE '/^[[:space:]]*"version"[[:space:]]*:/ {
    s/^[[:space:]]*"version"[[:space:]]*:[[:space:]]*"([^"]+)".*/\1/
    p
    q
  }' "$ROOT/apps/tauri-gui/src-tauri/tauri.conf.json"
)"
frontend_version="$(
  sed -nE '/^[[:space:]]*"version"[[:space:]]*:/ {
    s/^[[:space:]]*"version"[[:space:]]*:[[:space:]]*"([^"]+)".*/\1/
    p
    q
  }' "$ROOT/apps/tauri-gui/frontend/package.json"
)"
frontend_lock_version="$(
  sed -nE '/^[[:space:]]*"version"[[:space:]]*:/ {
    s/^[[:space:]]*"version"[[:space:]]*:[[:space:]]*"([^"]+)".*/\1/
    p
    q
  }' "$ROOT/apps/tauri-gui/frontend/package-lock.json"
)"

for version_entry in \
  "Tauri config:$tauri_version" \
  "frontend package:$frontend_version" \
  "frontend lockfile:$frontend_lock_version"; do
  label="${version_entry%%:*}"
  version="${version_entry#*:}"
  if [[ -z "$workspace_version" || "$version" != "$workspace_version" ]]; then
    echo "DRIFT: $label version '$version' does not match workspace '$workspace_version'"
    bad=1
  fi
done

if ! rg -q '#\[command\(name = "wallpaper-console-rust", version\)\]' \
  "$ROOT/crates/wc-cli/src/main.rs"; then
  echo "DRIFT: CLI version is not sourced from Cargo package metadata"
  bad=1
fi

check_absent() {
  local pattern="$1"
  local label="$2"
  if rg -n "$pattern" "$ROOT/crates" "$ROOT/apps" "$ROOT/README.md" \
    --glob '!**/node_modules/**' \
    --glob '!**/dist/**' \
    --glob '!**/dist-mock/**' \
    --glob '!**/target/**'; then
    echo "DRIFT: $label"
    bad=1
  fi
}

# Multiline-safe: options arrays may format values across lines.
if rg -U --multiline-dotall -n "options:\s*\[[^\]]*slide" "$SCHEMA"; then
  echo "DRIFT: slide exposed as awww transition option"
  bad=1
fi
if rg -U --multiline-dotall -n "options:\s*\[[^\]]*window" "$SCHEMA"; then
  echo "DRIFT: window exposed as LWE target mode option"
  bad=1
fi
if rg -U --multiline-dotall -n "storage_backend[^}]*?options:\s*\[[^\]]*(?:'file'|'hybrid'|\"file\"|\"hybrid\")" "$SCHEMA"; then
  echo "DRIFT: storage_backend exposes legacy file/hybrid option"
  bad=1
fi

check_absent "Run migrate-to-sqlite first" "old sqlite migration wording"
check_absent "ensure_or_migrate_sqlite" "old sqlite migration API"
check_absent "sqlite_mirror_active" "old sqlite mirror API"
check_absent "CURRENT_STATUS" "stale reference to removed CURRENT_STATUS"

exit "$bad"
