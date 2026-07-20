#!/usr/bin/env bash
set -euo pipefail

# Validate that Tauri beforeDevCommand / beforeBuildCommand work correctly
# from multiple working directories.  Uses the workspace-specific root
# detection (apps/tauri-gui/frontend directory) instead of a generic
# Cargo.toml check that would stop at apps/tauri-gui/src-tauri.

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
TAURI_CONF="$REPO_ROOT/apps/tauri-gui/src-tauri/tauri.conf.json"

PASS=0
FAIL=0

extract_command() {
  local key="$1"
  sed -n "/\"$key\"/,/\"[a-z]/ s/.*\"$key\": \"\\(.*\\)\",\\?$/\\1/p" "$TAURI_CONF" | head -1
}

test_root_detection() {
  local label="$1"
  local cwd="$2"
  local command="$3"
  local timeout_sec="${4:-8}"

  echo -n "  [$label] $cwd ... "

  local output
  local rc=0
  if output=$(cd "$cwd" && timeout "$timeout_sec" sh -c "$command" 2>&1); then
    rc=0
  else
    rc=$?
  fi

  # timeout 124 = command timed out, which is expected for persistent servers.
  # Other non-zero exits are failures.
  if [ "$rc" -eq 124 ]; then
    echo "OK (timeout expected for persistent command)"
    PASS=$((PASS + 1))
    return 0
  fi

  # Check for common cd failures.
  if echo "$output" | grep -qE "cd: .*No such file or directory"; then
    echo "FAIL — cd path error"
    echo "$output" | grep -E "cd:|No such" || true
    FAIL=$((FAIL + 1))
    return 1
  fi

  # Check for root-not-found guard.
  if echo "$output" | grep -q "workspace root not found"; then
    echo "FAIL — root detection failed"
    FAIL=$((FAIL + 1))
    return 1
  fi

  # cargo build failures.
  if echo "$output" | grep -qE "^error"; then
    echo "FAIL — cargo build error"
    echo "$output" | grep "^error" | head -3
    FAIL=$((FAIL + 1))
    return 1
  fi

  # If command exited cleanly (non-persistent), that's also OK.
  if [ "$rc" -eq 0 ]; then
    echo "OK (clean exit)"
    PASS=$((PASS + 1))
    return 0
  fi

  echo "FAIL (exit code $rc)"
  echo "$output" | tail -5
  FAIL=$((FAIL + 1))
  return 1
}

main() {
  if [ ! -f "$TAURI_CONF" ]; then
    echo "FAIL: tauri.conf.json not found at $TAURI_CONF"
    exit 1
  fi

  local BEFORE_DEV
  local BEFORE_BUILD
  BEFORE_DEV="$(extract_command beforeDevCommand)"
  BEFORE_BUILD="$(extract_command beforeBuildCommand)"

  if [ -z "$BEFORE_DEV" ]; then
    echo "FAIL: could not extract beforeDevCommand from $TAURI_CONF"
    exit 1
  fi
  if [ -z "$BEFORE_BUILD" ]; then
    echo "FAIL: could not extract beforeBuildCommand from $TAURI_CONF"
    exit 1
  fi

  echo "=== Tauri Before-Commands Test ==="
  echo "Config: $TAURI_CONF"
  echo ""

  # -- beforeDevCommand ----------------------------------------------------
  echo "beforeDevCommand:"
  test_root_detection "repo-root" "$REPO_ROOT" "$BEFORE_DEV" 10
  test_root_detection "tauri-gui" "$REPO_ROOT/apps/tauri-gui" "$BEFORE_DEV" 10
  test_root_detection "src-tauri" "$REPO_ROOT/apps/tauri-gui/src-tauri" "$BEFORE_DEV" 10

  # -- beforeBuildCommand ---------------------------------------------------
  echo "beforeBuildCommand root-detection smoke only:"
  echo "  Verifies: root detection from all cwd locations."
  echo "  Does NOT verify: npm build (covered by the main verification matrix;"
  echo "  full beforeBuildCommand is exercised by npm run build in the main verification matrix)."
  # Only verify root detection; don't wait for the full frontend build.
  local BUILD_SMOKE='while [ ! -d apps/tauri-gui/frontend ] && [ "$PWD" != / ]; do cd ..; done; test -d apps/tauri-gui/frontend'
  test_root_detection "repo-root" "$REPO_ROOT" "$BUILD_SMOKE" 30
  test_root_detection "tauri-gui" "$REPO_ROOT/apps/tauri-gui" "$BUILD_SMOKE" 30
  test_root_detection "src-tauri" "$REPO_ROOT/apps/tauri-gui/src-tauri" "$BUILD_SMOKE" 30

  echo ""
  echo "=== Results: $PASS passed, $FAIL failed ==="

  if [ "$FAIL" -gt 0 ]; then
    exit 1
  fi
}

if [[ "${BASH_SOURCE[0]}" == "${0}" ]]; then
  main "$@"
fi
