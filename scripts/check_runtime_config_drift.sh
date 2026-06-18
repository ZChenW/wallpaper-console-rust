#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

bad=0

check_absent() {
  local pattern="$1"
  local label="$2"
  if rg -n "$pattern" "$ROOT/crates" "$ROOT/apps" "$ROOT/README.md" "$ROOT/docs/DEVELOPMENT.md" "$ROOT/docs/TAURI_ARCHITECTURE.md" "$ROOT/docs/TAURI_MANUAL_SMOKE_CHECKLIST.md" "$ROOT/docs/PERFORMANCE_BASELINE.md" "$ROOT/docs/RUNTIME_FORMATS.md" \
    --glob '!**/node_modules/**' \
    --glob '!**/dist/**' \
    --glob '!**/dist-mock/**' \
    --glob '!**/target/**' \
    --glob '!docs/superpowers/plans/**'; then
    echo "DRIFT: $label"
    bad=1
  fi
}

# Check schema options only, not tests
if rg -n "options: .*slide" "$ROOT/apps/tauri-gui/frontend/src/settings/configSchema.ts"; then
  echo "DRIFT: slide exposed as awww transition option"
  bad=1
fi
if rg -n "options: .*window" "$ROOT/apps/tauri-gui/frontend/src/settings/configSchema.ts"; then
  echo "DRIFT: window exposed as LWE target mode option"
  bad=1
fi

check_absent "Run migrate-to-sqlite first" "old sqlite migration wording"
check_absent "ensure_or_migrate_sqlite" "old sqlite migration API"
check_absent "sqlite_mirror_active" "old sqlite mirror API"

exit "$bad"
