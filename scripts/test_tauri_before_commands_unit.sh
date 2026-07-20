#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
# shellcheck source=scripts/test_tauri_before_commands.sh
source "$SCRIPT_DIR/test_tauri_before_commands.sh"

PASS=0
FAIL=0

if test_root_detection "missing-root" "/tmp" "exit 17"; then
  echo "expected test_root_detection to fail for exit 17"
  exit 1
fi

if [ "$FAIL" -ne 1 ] || [ "$PASS" -ne 0 ]; then
  echo "expected FAIL=1 PASS=0 after exit 17, got FAIL=$FAIL PASS=$PASS"
  exit 1
fi

echo "test_tauri_before_commands_unit: OK"
