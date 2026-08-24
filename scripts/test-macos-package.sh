#!/usr/bin/env bash
set -euo pipefail

if [[ "$(uname -s)" != "Darwin" || "$(uname -m)" != "arm64" ]]; then
  echo "SKIP host gate macos-arm64-packaged-app-webview: requires macOS arm64" >&2
  exit 0
fi

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
EXISTING_BUNDLE=""
if (( $# == 2 )) && [[ "$1" == "--existing-bundle" ]]; then
  [[ -d "$2" ]] || { echo "Existing app bundle not found: $2" >&2; exit 1; }
  EXISTING_BUNDLE="$(cd "$(dirname "$2")" && pwd)/$(basename "$2")"
elif (( $# != 0 )); then
  echo "Usage: bash scripts/test-macos-package.sh [--existing-bundle <CCResDoc.app>]" >&2
  exit 2
fi

PROBE_DIR="$(mktemp -d /tmp/ccresdoc-macos-package.XXXXXX)"
PROBE_HOME="$PROBE_DIR/home"
SENTINEL_DIR="$PROBE_DIR/bin"
SENTINEL_LOG="$PROBE_DIR/node-invocations.log"
PROCESS_LOG="$PROBE_DIR/process-samples.log"
CARGO_TARGET_DIR="$PROBE_DIR/cargo-target"
FIXTURE_LABEL="Package Readiness Probe $(basename "$PROBE_DIR")"
FIXTURE_BODY="Generated package route $(basename "$PROBE_DIR")"
FIXTURE_AUTOLINK="https://example.com/package-readiness-probe"
FIXTURE_ROUTE="http://127.0.0.1:4892/docs/claude-skills/package-readiness-probe/"
APP_PID=""
PORT_LOCK="${TMPDIR:-/tmp}"
PORT_LOCK="${PORT_LOCK%/}/ccresdoc-runtime-port-4892.lock"
PORT_LOCK_HELD=0
PACKAGE_FACTS="$REPO_ROOT/compatibility/node-free-latest/evidence/package-facts.json"
DARWIN_PACKAGE=""
DARWIN_RELATIVE_PATH=""
DARWIN_SIZE=""
DARWIN_SHA256=""

cleanup() {
  local status=$?
  trap - EXIT INT TERM
  if [[ -n "$APP_PID" ]] && kill -0 "$APP_PID" 2>/dev/null; then
    kill -TERM "$APP_PID" 2>/dev/null || true
    wait "$APP_PID" 2>/dev/null || true
  fi
  case "$PROBE_DIR" in
    /tmp/ccresdoc-macos-package.*) rm -rf "$PROBE_DIR" ;;
    *) echo "Refusing to remove unexpected probe directory: $PROBE_DIR" >&2 ;;
  esac
  if [[ "$PORT_LOCK_HELD" = "1" ]]; then
    rm -rf "$PORT_LOCK"
    PORT_LOCK_HELD=0
  fi
  exit "$status"
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM

acquire_port_lock() {
  local owner_pid=""
  if ! mkdir "$PORT_LOCK" 2>/dev/null; then
    owner_pid="$(node -e 'try { process.stdout.write(String(require(process.argv[1]).pid ?? "")) } catch {}' "$PORT_LOCK/owner.json")"
    if [[ ! "$owner_pid" =~ ^[0-9]+$ ]]; then
      echo "fixed-port runtime probe lock is initializing or invalid" >&2
      exit 1
    fi
    if kill -0 "$owner_pid" 2>/dev/null; then
      echo "fixed-port runtime probe already running (pid $owner_pid)" >&2
      exit 1
    fi
    rm -rf "$PORT_LOCK"
    mkdir "$PORT_LOCK"
  fi
  printf '{"pid":%s,"port":4892}\n' "$$" > "$PORT_LOCK/owner.json"
  PORT_LOCK_HELD=1
}

DARWIN_PACKAGE="$(node -e 'const f=require(process.argv[1]).nativeCarriers["darwin-arm64"]; process.stdout.write(f.package)' "$PACKAGE_FACTS")"
DARWIN_RELATIVE_PATH="$(node -e 'const f=require(process.argv[1]).nativeCarriers["darwin-arm64"]; process.stdout.write(f.relativePath)' "$PACKAGE_FACTS")"
DARWIN_SIZE="$(node -e 'const f=require(process.argv[1]).nativeCarriers["darwin-arm64"]; process.stdout.write(String(f.sizeBytes))' "$PACKAGE_FACTS")"
DARWIN_SHA256="$(node -e 'const f=require(process.argv[1]).nativeCarriers["darwin-arm64"]; process.stdout.write(f.sha256)' "$PACKAGE_FACTS")"

mkdir -p "$PROBE_HOME/.claude/skills/package-readiness-probe" "$SENTINEL_DIR"
printf '%s\n' \
  '---' \
  "name: \"$FIXTURE_LABEL\"" \
  'description: Proves packaged semantic readiness uses generated content.' \
  '---' \
  '' \
  "$FIXTURE_BODY" \
  '' \
  "Source: <$FIXTURE_AUTOLINK>" \
  '' \
  '| Result |' \
  '| --- |' \
  '| before<br>after |' > "$PROBE_HOME/.claude/skills/package-readiness-probe/SKILL.md"
: > "$SENTINEL_LOG"
cat > "$SENTINEL_DIR/node" <<EOF
#!/bin/sh
printf '%s\n' "\$*" >> '$SENTINEL_LOG'
exit 97
EOF
chmod 755 "$SENTINEL_DIR/node"

cd "$REPO_ROOT"
if [[ -z "$EXISTING_BUNDLE" ]]; then
  export CARGO_TARGET_DIR
  pnpm --dir app install --frozen-lockfile
  pnpm --dir app exec zfb build
  pnpm run probe:runtime-package
  cargo tauri build --bundles app
  APP_PATH="$CARGO_TARGET_DIR/release/bundle/macos/CCResDoc.app"
else
  APP_PATH="$EXISTING_BUNDLE"
fi

acquire_port_lock
test -z "$(lsof -ti :4892 2>/dev/null || true)"

RUNTIME_ROOT="$APP_PATH/Contents/Resources/runtime-workspace/app"
ZFB_BIN="$RUNTIME_ROOT/$DARWIN_RELATIVE_PATH"

test "$DARWIN_PACKAGE" = "@takazudo/zfb-darwin-arm64@$(node -p 'require("./app/package.json").optionalDependencies["@takazudo/zfb-darwin-arm64"]')"
test -x "$ZFB_BIN"
test "$(stat -f%z "$ZFB_BIN")" = "$DARWIN_SIZE"
test "$(shasum -a 256 "$ZFB_BIN" | awk '{print $1}')" = "$DARWIN_SHA256"
file "$ZFB_BIN" | grep -q "Mach-O 64-bit executable arm64"
test ! -e "$RUNTIME_ROOT/test"
test ! -e "$RUNTIME_ROOT/node_modules/typescript"
test ! -e "$RUNTIME_ROOT/node_modules/vitest"
test ! -e "$RUNTIME_ROOT/node_modules/@takazudo/zfb-darwin-x64"

for RUN in 1 2; do
  # Launch through LaunchServices so Tauri resolves the app bundle's resource
  # directory. Running Contents/MacOS/ccresdoc directly lacks NSBundle context
  # and fails before it can copy the staged runtime workspace.
  open -n -W \
    --env "HOME=$PROBE_HOME" \
    --env "ZFB_DEV_BOOT_LAZY=1" \
    --env "PATH=$SENTINEL_DIR:$PATH" \
    "$APP_PATH" &
  APP_PID=$!
  READY=0
  for _ in $(seq 1 300); do
    ps -axo pid=,ppid=,args= | grep "$PROBE_DIR\|$APP_PATH" >> "$PROCESS_LOG" || true
    if grep -q "plugin-host.mjs" "$PROCESS_LOG"; then
      echo "plugin host observed during packaged launch" >&2
      exit 1
    fi
    if [[ "$(curl -s -o "$PROBE_DIR/root.html" -w '%{http_code}' http://127.0.0.1:4892/ || true)" = "200" ]] \
      && grep -Fq "Claude Code Resources" "$PROBE_DIR/root.html" \
      && grep -Fq "$FIXTURE_LABEL" "$PROBE_DIR/root.html" \
      && ! grep -Fq "data-home-page" "$PROBE_DIR/root.html" \
      && ! grep -Fq ">Claude</a>" "$PROBE_DIR/root.html" \
      && grep -Fq "data-header-logo" "$PROBE_DIR/root.html" \
      && grep -Eq 'href="?/docs/' "$PROBE_DIR/root.html" \
      && [[ "$(curl -s -o "$PROBE_DIR/docs.html" -w '%{http_code}' http://127.0.0.1:4892/docs/ || true)" = "200" ]] \
      && grep -Fq "Claude Resources" "$PROBE_DIR/docs.html" \
      && grep -Fq "$FIXTURE_LABEL" "$PROBE_DIR/docs.html" \
      && [[ "$(curl -s -o "$PROBE_DIR/fixture.html" -w '%{http_code}' "$FIXTURE_ROUTE" || true)" = "200" ]] \
      && grep -Fq "$FIXTURE_LABEL" "$PROBE_DIR/fixture.html" \
      && grep -Fq "$FIXTURE_BODY" "$PROBE_DIR/fixture.html" \
      && grep -Fq "href=\"$FIXTURE_AUTOLINK\"" "$PROBE_DIR/fixture.html" \
      && grep -Fq 'before<br' "$PROBE_DIR/fixture.html" \
      && grep -Fq '>after<' "$PROBE_DIR/fixture.html"; then
        READY=1
        break
    fi
    sleep 1
  done
  test "$READY" = "1"
  grep -Fq "CCResDoc" "$PROBE_DIR/docs.html"
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

echo "PASS host gate macos-arm64-packaged-app-webview: packaged workspace, WebView launch, node sentinel, relaunch, and process-group shutdown"
if [[ -z "$EXISTING_BUNDLE" ]]; then
  echo "Fresh Cargo target: $CARGO_TARGET_DIR"
else
  echo "Existing bundle mode: verified without rebuilding"
fi
echo "Bundle: $APP_PATH"
