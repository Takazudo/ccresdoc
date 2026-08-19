#!/usr/bin/env bash
set -euo pipefail

if [[ "$(uname -s)" != "Darwin" || "$(uname -m)" != "arm64" ]]; then
  echo "SKIP: packaged lifecycle is supported/tested only on macOS arm64" >&2
  exit 0
fi

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
PROBE_DIR="$(mktemp -d /tmp/ccresdoc-macos-package.XXXXXX)"
PROBE_HOME="$PROBE_DIR/home"
SENTINEL_DIR="$PROBE_DIR/bin"
SENTINEL_LOG="$PROBE_DIR/node-invocations.log"
PROCESS_LOG="$PROBE_DIR/process-samples.log"
APP_PID=""

cleanup() {
  if [[ -n "$APP_PID" ]] && kill -0 "$APP_PID" 2>/dev/null; then
    kill -TERM "$APP_PID" 2>/dev/null || true
    wait "$APP_PID" 2>/dev/null || true
  fi
  rm -rf "$PROBE_DIR"
}
trap cleanup EXIT INT TERM

mkdir -p "$PROBE_HOME/.claude/skills/package-probe" "$SENTINEL_DIR"
printf '%s\n' '# Package probe' > "$PROBE_HOME/.claude/skills/package-probe/SKILL.md"
: > "$SENTINEL_LOG"
cat > "$SENTINEL_DIR/node" <<EOF
#!/bin/sh
printf '%s\n' "\$*" >> '$SENTINEL_LOG'
exit 97
EOF
chmod 755 "$SENTINEL_DIR/node"

cd "$REPO_ROOT"
pnpm --dir app install --frozen-lockfile
pnpm --dir app exec zfb build
pnpm run probe:runtime-package
cargo tauri build --bundles app

APP_PATH="${APP_OVERRIDE:-$REPO_ROOT/target/release/bundle/macos/CCResDoc.app}"
RUNTIME_ROOT="$APP_PATH/Contents/Resources/runtime-workspace/app"
ZFB_BIN="$RUNTIME_ROOT/node_modules/@takazudo/zfb-darwin-arm64/zfb"

test -x "$ZFB_BIN"
test "$(stat -f%z "$ZFB_BIN")" = "173246016"
test "$(shasum -a 256 "$ZFB_BIN" | awk '{print $1}')" = "35bfa2b2cf8ffc6b5ddefdf712155e02ad6aa5e947ffcf41ee57f8e48ff2d7a0"
file "$ZFB_BIN" | grep -q "Mach-O 64-bit executable arm64"
test ! -e "$RUNTIME_ROOT/test"
test ! -e "$RUNTIME_ROOT/node_modules/typescript"
test ! -e "$RUNTIME_ROOT/node_modules/vitest"
test ! -e "$RUNTIME_ROOT/node_modules/@takazudo/zfb-darwin-x64"

for RUN in 1 2; do
  HOME="$PROBE_HOME" PATH="$SENTINEL_DIR:$PATH" "$APP_PATH/Contents/MacOS/ccresdoc" &
  APP_PID=$!
  READY=0
  for _ in $(seq 1 300); do
    ps -axo pid=,ppid=,args= | grep "$PROBE_DIR\|$APP_PATH" >> "$PROCESS_LOG" || true
    if grep -q "plugin-host.mjs" "$PROCESS_LOG"; then
      echo "plugin host observed during packaged launch" >&2
      exit 1
    fi
    if [[ "$(curl -s -o "$PROBE_DIR/root.html" -w '%{http_code}' http://127.0.0.1:4892/ || true)" = "200" ]]; then
      READY=1
      break
    fi
    sleep 1
  done
  test "$READY" = "1"
  grep -q "CCResDoc" "$PROBE_DIR/root.html"
  test ! -s "$SENTINEL_LOG"

  osascript -e 'tell application id "com.takazudo.ccresdoc" to quit' || kill -TERM "$APP_PID"
  wait "$APP_PID" || true
  APP_PID=""
  for _ in $(seq 1 20); do
    if ! lsof -ti :4892 >/dev/null 2>&1; then break; fi
    sleep 0.25
  done
  test -z "$(lsof -ti :4892 2>/dev/null || true)"
done

echo "PASS: macOS arm64 packaged workspace, node sentinel, relaunch, and process-group shutdown"
