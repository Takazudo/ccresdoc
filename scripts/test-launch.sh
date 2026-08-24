#!/usr/bin/env bash
set -uo pipefail
# Test script: launch CCResDoc.app via open (Finder simulation), verify docs load.
# Usage: bash scripts/test-launch.sh [--controls] [count]
#   count — number of launch iterations (default 3, must be a positive integer)
#   --controls — after the first ready /docs/ pass, use macOS accessibility to
#                activate the hydrated header ThemeToggle and require its AX
#                label to change. This is intentionally run after readiness,
#                without waiting for a livereload rescue reload.
# Exits 0 on success, 1 on failure.
#
# The --cold flag is intentionally absent: there are no Node.js deps in the
# .app's runtime path. The host spawns the native zfb binary from the bundled
# node_modules/@takazudo/zfb-<platform>/zfb directly — no Node required.
#
# Readiness is polled on GET /docs/ and requires generated navigation, NOT
# merely a generic 200 and NOT /___ready.
# The /___ready endpoint no longer exists in the sidecar architecture.

CHECK_CONTROLS=0
if [[ "${1:-}" == "--controls" ]]; then
  CHECK_CONTROLS=1
  shift
fi
if (( $# > 1 )); then
  echo "usage: bash scripts/test-launch.sh [--controls] [count]" >&2
  exit 1
fi
COUNT="${1:-3}"

# Validate that COUNT is a positive integer
if ! [[ "$COUNT" =~ ^[1-9][0-9]*$ ]]; then
  echo "error: count must be a positive integer, got: $COUNT" >&2
  exit 1
fi

PASS=0
FAIL=0

for RUN in $(seq 1 "$COUNT"); do
  echo "=== Run $RUN/$COUNT ==="

  # Kill CCResDoc processes and anything holding port 4892.
  # Use pkill -f with an anchored pattern; guard with || true so missing
  # processes don't abort the script.
  pkill -f "CCResDoc" || true
  lsof -ti :4892 | xargs kill 2>/dev/null || true
  sleep 3

  # Launch via open (use installed app or override via APP_OVERRIDE env var)
  APP_PATH="${APP_OVERRIDE:-/Applications/CCResDoc.app}"
  open "$APP_PATH"

  # Wait up to 300s for zfb dev to serve the generated docs shell.
  # Cold first-run can take ~135s (walking + rendering ~135 skills + site build).
  # A stale staged shell can return 200, so require the generator-owned marker.
  OK=0
  READY_OBSERVED=0
  for i in $(seq 1 100); do
    sleep 3
    HTTP=$(curl -s -o /tmp/ccresdoc-launch-docs.html -w "%{http_code}" http://localhost:4892/docs/ 2>/dev/null)
    if [ "$HTTP" = "200" ] && grep -Fq "CCResDoc Resources" /tmp/ccresdoc-launch-docs.html; then
      READY_OBSERVED=1
      OK=1
      echo "  Run $RUN: PASS (ready at $((i*3))s)"
      if [[ "$CHECK_CONTROLS" == "1" && "$RUN" == "1" ]]; then
        if [[ "$(uname -s)" != "Darwin" ]]; then
          echo "  Run $RUN: FAIL (--controls requires macOS WebKit accessibility)" >&2
          OK=0
        elif ! CONTROL_RESULT=$(osascript "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/assert-app-theme-toggle.applescript" 2>&1); then
          echo "  Run $RUN: FAIL (ThemeToggle accessibility check: $CONTROL_RESULT)" >&2
          OK=0
        else
          echo "  Run $RUN: PASS ($CONTROL_RESULT)"
        fi
      fi
      if [[ "$OK" == "1" ]]; then PASS=$((PASS + 1)); fi
      break
    fi
  done

  if [ "$OK" = "0" ]; then
    if [ "$READY_OBSERVED" = "0" ]; then
      echo "  Run $RUN: FAIL (server not ready after 300s)"
    fi
    FAIL=$((FAIL + 1))
  fi
done

# Cleanup
pkill -f "CCResDoc" || true
lsof -ti :4892 | xargs kill 2>/dev/null || true
rm -f /tmp/ccresdoc-launch-docs.html

echo ""
echo "=== Results: $PASS/$COUNT passed, $FAIL failed ==="
[ "$FAIL" -gt 0 ] && exit 1 || exit 0
