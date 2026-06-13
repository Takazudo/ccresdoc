#!/bin/bash
# Test script: launch CCResDoc.app via open (Finder simulation), verify docs load.
# Usage: bash test-launch.sh [count]
#   count — number of launch iterations (default 3)
# Exits 0 on success, 1 on failure.
#
# The --cold flag is intentionally absent: there are no Node.js deps in the
# .app's runtime path. The host spawns the native zfb binary from the bundled
# node_modules/@takazudo/zfb-<platform>/zfb directly — no Node required.
#
# Readiness is polled on GET / (the zfb dev root), NOT /___ready.
# The /___ready endpoint no longer exists in the sidecar architecture.

COUNT=${1:-3}
PASS=0
FAIL=0

for RUN in $(seq 1 $COUNT); do
  echo "=== Run $RUN/$COUNT ==="

  # Kill everything
  ps aux | grep "[Cc][Cc][Rr]es[Dd]oc" | grep -v grep | awk '{print $2}' | xargs kill 2>/dev/null || true
  lsof -ti :4892 | xargs kill 2>/dev/null || true
  sleep 3

  # Launch via open (use installed app or override via APP_OVERRIDE env var)
  APP_PATH="${APP_OVERRIDE:-/Applications/CCResDoc.app}"
  open "$APP_PATH"

  # Wait up to 300s for zfb dev to serve the root page.
  # Cold first-run can take ~135s (walking + rendering ~135 skills + site build).
  # Poll GET / — a 200 means zfb dev is up and the site is built.
  OK=0
  for i in $(seq 1 100); do
    sleep 3
    HTTP=$(curl -s -o /dev/null -w "%{http_code}" http://localhost:4892/ 2>/dev/null)
    if [ "$HTTP" = "200" ]; then
      echo "  Run $RUN: PASS (ready at $((i*3))s)"
      OK=1
      PASS=$((PASS + 1))
      break
    fi
  done

  if [ "$OK" = "0" ]; then
    echo "  Run $RUN: FAIL (server not ready after 300s)"
    FAIL=$((FAIL + 1))
  fi
done

# Cleanup
ps aux | grep "[Cc][Cc][Rr]es[Dd]oc" | grep -v grep | awk '{print $2}' | xargs kill 2>/dev/null || true
lsof -ti :4892 | xargs kill 2>/dev/null || true

echo ""
echo "=== Results: $PASS/$COUNT passed, $FAIL failed ==="
[ "$FAIL" -gt 0 ] && exit 1 || exit 0
