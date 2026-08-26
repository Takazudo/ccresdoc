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

app_running() {
  [[ "$(/usr/bin/osascript -e 'application id "com.takazudo.ccresdoc" is running' 2>/dev/null || true)" == "true" ]]
}

if app_running; then
  echo "Refusing to run beside an existing CCResDoc instance." >&2
  exit 1
fi

PROBE_DIR="$(mktemp -d /tmp/ccresdoc-macos-package.XXXXXX)"
PROBE_HOME="$PROBE_DIR/home"
HOST_LOG="$PROBE_HOME/Library/Application Support/com.takazudo.ccresdoc/ccresdoc.log"
SENTINEL_DIR="$PROBE_DIR/bin"
SENTINEL_LOG="$PROBE_DIR/node-invocations.log"
PROCESS_LOG="$PROBE_DIR/process-samples.log"
CARGO_TARGET_DIR="$PROBE_DIR/cargo-target"
FIXTURE_LABEL="Package Readiness Probe $(basename "$PROBE_DIR")"
FIXTURE_BODY="Generated package route $(basename "$PROBE_DIR")"
FIXTURE_AUTOLINK="https://example.com/package-readiness-probe"
FIXTURE_ROUTE="http://127.0.0.1:4892/docs/claude-skills/package-readiness-probe/"
APP_PID=""
OWNED_SIDECAR_PID=""
OWNED_SIDECAR_PGID=""
OWNED_ZFB_BIN=""
OWNED_ZFB_COMMAND=""
PORT_LOCK="${TMPDIR:-/tmp}"
PORT_LOCK="${PORT_LOCK%/}/ccresdoc-runtime-port-4892.lock"
PORT_LOCK_HELD=0
PACKAGE_FACTS="$REPO_ROOT/compatibility/node-free-latest/evidence/package-facts.json"
DARWIN_PACKAGE=""
DARWIN_RELATIVE_PATH=""
DARWIN_SIZE=""
DARWIN_SHA256=""

owned_group_alive() {
  local pgid="$1"
  ps -axo pgid= | awk -v pgid="$pgid" '$1 == pgid { found=1 } END { exit found ? 0 : 1 }'
}

owned_sidecar_alive() {
  local pid=""
  local pgid=""
  local command=""
  while read -r pid pgid command; do
    if [[ "$pid" = "$OWNED_SIDECAR_PID" && "$pgid" = "$OWNED_SIDECAR_PGID" && "$command" = "$OWNED_ZFB_COMMAND" ]]; then
      return 0
    fi
  done < <(ps -ww -axo pid=,pgid=,command=)
  return 1
}

owned_group_is_probe_scoped() {
  local expected_pgid="$1"
  local pid=""
  local pgid=""
  local command=""
  while read -r pid pgid command; do
    if [[ "$pgid" = "$expected_pgid" && "$command" = "$OWNED_ZFB_BIN"* ]]; then
      return 0
    fi
  done < <(ps -ww -axo pid=,pgid=,command=)
  return 1
}

find_owned_sidecar() {
  local pid=""
  local pgid=""
  local command=""
  while read -r pid pgid command; do
    if [[ "$command" = "$OWNED_ZFB_COMMAND" ]]; then
      printf '%s %s\n' "$pid" "$pgid"
    fi
  done < <(ps -ww -axo pid=,pgid=,command=)
}

stop_owned_sidecar_for_cleanup() {
  if [[ -z "$OWNED_SIDECAR_PID" || -z "$OWNED_SIDECAR_PGID" ]]; then
    return 0
  fi
  if ! owned_group_alive "$OWNED_SIDECAR_PGID" && ! owned_sidecar_alive; then
    OWNED_SIDECAR_PID=""
    OWNED_SIDECAR_PGID=""
    return 0
  fi
  if ! owned_group_is_probe_scoped "$OWNED_SIDECAR_PGID"; then
    echo "Refusing to signal process group $OWNED_SIDECAR_PGID without the probe-owned zfb path." >&2
    return 1
  fi
  /bin/kill -TERM "-$OWNED_SIDECAR_PGID" 2>/dev/null || true
  for _ in $(seq 1 40); do
    if ! owned_group_alive "$OWNED_SIDECAR_PGID" && ! owned_sidecar_alive; then break; fi
    sleep 0.25
  done
  if owned_group_alive "$OWNED_SIDECAR_PGID" || owned_sidecar_alive; then
    if ! owned_group_is_probe_scoped "$OWNED_SIDECAR_PGID"; then
      echo "Refusing to force-kill process group $OWNED_SIDECAR_PGID after its probe identity changed." >&2
      return 1
    fi
    /bin/kill -KILL "-$OWNED_SIDECAR_PGID" 2>/dev/null || true
    for _ in $(seq 1 40); do
      if ! owned_group_alive "$OWNED_SIDECAR_PGID" && ! owned_sidecar_alive; then break; fi
      sleep 0.25
    done
  fi
  if owned_group_alive "$OWNED_SIDECAR_PGID" || owned_sidecar_alive; then
    echo "Probe-owned process group $OWNED_SIDECAR_PGID survived cleanup." >&2
    return 1
  fi
  OWNED_SIDECAR_PID=""
  OWNED_SIDECAR_PGID=""
}

cleanup() {
  local status=$?
  trap - EXIT INT TERM
  if [[ "$status" != "0" && -f "$HOST_LOG" ]]; then
    echo "--- packaged host lifecycle log ---" >&2
    tail -200 "$HOST_LOG" >&2 || true
    echo "--- end packaged host lifecycle log ---" >&2
  fi
  if [[ -n "$APP_PID" ]]; then
    if app_running; then
      /usr/bin/osascript -e 'tell application id "com.takazudo.ccresdoc" to quit' >/dev/null 2>&1 || true
      for _ in $(seq 1 80); do
        app_running || break
        sleep 0.25
      done
      if app_running; then
        echo "Packaged CCResDoc did not quit during probe cleanup." >&2
        status=1
      fi
    fi
    if kill -0 "$APP_PID" 2>/dev/null; then
      kill -TERM "$APP_PID" 2>/dev/null || true
    fi
    wait "$APP_PID" 2>/dev/null || true
  fi
  if ! stop_owned_sidecar_for_cleanup; then
    status=1
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
OWNED_ZFB_BIN="$PROBE_HOME/Library/Application Support/com.takazudo.ccresdoc/app-workspace/$DARWIN_RELATIVE_PATH"
OWNED_ZFB_COMMAND="$OWNED_ZFB_BIN dev --host 127.0.0.1 --port 4892"

# Audit the final bundle's staged app before any user fixture is introduced.
# This checks the same explicit source/namespace/privacy contract as the Linux
# staged probe and receives only synthetic temporary paths as rejection inputs.
node "$REPO_ROOT/scripts/audit-runtime-workspace.mjs" "$RUNTIME_ROOT" "$PROBE_HOME" "$APP_PATH"

# Create the temporary source and failing Node sentinel only after the final
# bundle has passed its privacy audit. They are launch inputs, never staging
# inputs, and therefore cannot influence the audited package bytes.
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
    ps -ww -axo pid=,ppid=,pgid=,args= | grep "$PROBE_DIR\|$APP_PATH" >> "$PROCESS_LOG" || true
    if grep -q "plugin-host.mjs" "$PROCESS_LOG"; then
      echo "plugin host observed during packaged launch" >&2
      exit 1
    fi
    if [[ "$(curl -s -o "$PROBE_DIR/root.html" -w '%{http_code}' http://127.0.0.1:4892/ || true)" = "200" ]] \
      && grep -Fq "CCResDoc Resources" "$PROBE_DIR/root.html" \
      && grep -Fq "$FIXTURE_LABEL" "$PROBE_DIR/root.html" \
      && ! grep -Fq "data-home-page" "$PROBE_DIR/root.html" \
      && grep -Fq "Claude" "$PROBE_DIR/root.html" \
      && grep -Fq "Codex" "$PROBE_DIR/root.html" \
      && grep -Fq "data-header-logo" "$PROBE_DIR/root.html" \
      && grep -Eq 'href="?/docs/' "$PROBE_DIR/root.html" \
      && [[ "$(curl -s -o "$PROBE_DIR/docs.html" -w '%{http_code}' http://127.0.0.1:4892/docs/ || true)" = "200" ]] \
      && grep -Fq "CCResDoc Resources" "$PROBE_DIR/docs.html" \
      && grep -Fq "$FIXTURE_LABEL" "$PROBE_DIR/docs.html" \
      && grep -Fq "Claude" "$PROBE_DIR/docs.html" \
      && grep -Fq "Codex" "$PROBE_DIR/docs.html" \
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

  OWNED_SIDECAR_ROW="$(find_owned_sidecar)"
  if [[ -z "$OWNED_SIDECAR_ROW" || "$OWNED_SIDECAR_ROW" = *$'\n'* ]]; then
    echo "Expected exactly one probe-owned zfb sidecar, got: ${OWNED_SIDECAR_ROW:-none}" >&2
    exit 1
  fi
  read -r OWNED_SIDECAR_PID OWNED_SIDECAR_PGID <<< "$OWNED_SIDECAR_ROW"
  [[ "$OWNED_SIDECAR_PID" =~ ^[1-9][0-9]*$ ]]
  [[ "$OWNED_SIDECAR_PGID" =~ ^[1-9][0-9]*$ ]]
  test "$OWNED_SIDECAR_PID" = "$OWNED_SIDECAR_PGID"
  printf 'owned-sidecar run=%s pid=%s pgid=%s command=%s\n' \
    "$RUN" "$OWNED_SIDECAR_PID" "$OWNED_SIDECAR_PGID" "$OWNED_ZFB_COMMAND" >> "$PROCESS_LOG"

  osascript -e 'tell application id "com.takazudo.ccresdoc" to quit' || kill -TERM "$APP_PID"
  wait "$APP_PID" || true
  APP_PID=""
  for _ in $(seq 1 80); do
    if ! owned_group_alive "$OWNED_SIDECAR_PGID" && ! owned_sidecar_alive; then break; fi
    sleep 0.25
  done
  if owned_group_alive "$OWNED_SIDECAR_PGID" || owned_sidecar_alive; then
    echo "Packaged quit left probe-owned PID $OWNED_SIDECAR_PID / PGID $OWNED_SIDECAR_PGID alive." >&2
    ps -ww -axo pid=,ppid=,pgid=,stat=,args= | awk -v pgid="$OWNED_SIDECAR_PGID" '$3 == pgid'
    exit 1
  fi
  OWNED_SIDECAR_PID=""
  OWNED_SIDECAR_PGID=""
  test -z "$(lsof -ti :4892 2>/dev/null || true)"
done

echo "PASS host gate macos-arm64-packaged-app-webview: packaged workspace, WebView launch, node sentinel, relaunch, and process-group shutdown"
if [[ -z "$EXISTING_BUNDLE" ]]; then
  echo "Fresh Cargo target: $CARGO_TARGET_DIR"
else
  echo "Existing bundle mode: verified without rebuilding"
fi
echo "Bundle: $APP_PATH"
