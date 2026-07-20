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
