#!/bin/bash
# Test script: launch app via open (Finder simulation), verify docs load.
# Usage: bash test-launch.sh [count]
#   count — number of launch iterations (default 3)
# Exits 0 on success, 1 on failure.
#
# The --cold flag is intentionally absent: there are no node_modules in the
# .app's runtime path (the embedded axum server requires no Node.js), so
# there is nothing to wipe for a cold-start test.

COUNT=${1:-3}
PASS=0
FAIL=0

for RUN in $(seq 1 $COUNT); do
  echo "=== Run $RUN/$COUNT ==="

  # Kill everything
  ps aux | grep "ccresdoc" | grep -v grep | awk '{print $2}' | xargs kill 2>/dev/null || true
  lsof -ti :4892 | xargs kill 2>/dev/null || true
  sleep 3

  # Launch via open (use build output or installed app)
  APP_PATH="${APP_OVERRIDE:-$HOME/.claude/doc/src-tauri/target/release/bundle/macos/CCResDoc.app}"
  open "$APP_PATH"

  # Wait up to 60s for docs to be available
  OK=0
  for i in $(seq 1 20); do
    sleep 3
    READY=$(curl -s -o /dev/null -w "%{http_code}" http://localhost:4892/___ready 2>/dev/null)
    if [ "$READY" = "200" ]; then
      # Verify docs page responds
      HTTP=$(curl -s -o /dev/null -w "%{http_code}" http://localhost:4892/ 2>/dev/null)
      if [ "$HTTP" = "200" ]; then
        echo "  Run $RUN: PASS (ready at $((i*3))s)"
        OK=1
        PASS=$((PASS + 1))
        break
      fi
    fi
  done

  if [ "$OK" = "0" ]; then
    echo "  Run $RUN: FAIL (server not ready after 60s)"
    FAIL=$((FAIL + 1))
  fi
done

# Cleanup
ps aux | grep "ccresdoc" | grep -v grep | awk '{print $2}' | xargs kill 2>/dev/null || true
lsof -ti :4892 | xargs kill 2>/dev/null || true

echo ""
echo "=== Results: $PASS/$COUNT passed, $FAIL failed ==="
[ "$FAIL" -gt 0 ] && exit 1 || exit 0
