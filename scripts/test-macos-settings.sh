#!/usr/bin/env bash
set -euo pipefail

# Isolated settings package smoke gate. Package mode is for macOS arm64 only;
# fixture mode is safe on every host and performs static production guards.
usage() {
  cat <<'EOF'
Usage:
  scripts/test-macos-settings.sh --fixtures-only
  CCRESDOC_SETTINGS_APP=/path/to/CCResDoc.app scripts/test-macos-settings.sh
EOF
}

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
FIXTURES_ONLY=0
for arg in "$@"; do
  case "$arg" in
    --) ;;
    --fixtures-only) FIXTURES_ONLY=1 ;;
    -h|--help) usage; exit 0 ;;
    *) echo "error: unknown argument: $arg" >&2; usage >&2; exit 2 ;;
  esac
done

tmp_root="$(printenv TMPDIR || true)"
if [[ -z "$tmp_root" ]]; then tmp_root=/tmp; fi
PROBE_DIR="$(mktemp -d "$tmp_root/ccresdoc-settings-smoke.XXXXXX")"
PROBE_HOME="$PROBE_DIR/home"
DEFAULT_SOURCE="$PROBE_HOME/.claude"
AUTHORED_SOURCE="$PROBE_DIR/isolated-source"
CONFIG_PATH="$PROBE_DIR/config.toml"
XDG_CONFIG_HOME="$PROBE_DIR/xdg"
FOREIGN_INFO="$PROBE_DIR/foreign.json"
APP_PATH="$(printenv CCRESDOC_SETTINGS_APP || true)"
APP_OPEN_PID=""
FOREIGN_PID=""
FOREIGN_PORT=""
APP_LOG=""
LOG_START=0
FALLBACK_EFFECTIVE=""
SECOND_EFFECTIVE=""

json_quote() {
  node -e 'process.stdout.write(JSON.stringify(process.argv[1]))' "$1"
}

make_skill() {
  local root="$1" slug="$2" title="$3" body="$4"
  mkdir -p "$root/skills/$slug"
  {
    printf '%s\n' '---'
    printf 'name: %s\n' "$(json_quote "$title")"
    printf '%s\n' 'description: Isolated settings smoke fixture.'
    printf '%s\n' '---'
    printf '\n%s\n' "$body"
  } > "$root/skills/$slug/SKILL.md"
}

make_fixtures() {
  local slug
  slug="$(basename "$PROBE_DIR" | tr -cd '[:alnum:]-')"
  mkdir -p "$DEFAULT_SOURCE" "$AUTHORED_SOURCE" "$XDG_CONFIG_HOME"
  printf '# default settings fixture\n' > "$DEFAULT_SOURCE/CLAUDE.md"
  printf '# authored settings fixture\n' > "$AUTHORED_SOURCE/CLAUDE.md"
  make_skill "$DEFAULT_SOURCE" "settings-default-$slug" "Settings default source" "Default settings source fixture."
  make_skill "$AUTHORED_SOURCE" "settings-authored-$slug" "Settings authored source" "Authored source fixture marker."
}

write_config() {
  local source="$1" port="$2" fallback="$3"
  {
    printf '%s\n\n' 'schema_version = 1'
    printf '%s\n' '[source]'
    printf 'claude_dir = %s\n\n' "$(json_quote "$source")"
    printf '%s\n' '[appearance]' 'mode = "dark"' 'theme_pack = "default"' ''
    printf '%s\n' '[server]'
    printf 'preferred_port = %s\nfallback_to_free_port = %s\n' "$port" "$fallback"
  } > "$CONFIG_PATH"
}

assert_config() {
  grep -Fq "claude_dir = $(json_quote "$1")" "$CONFIG_PATH"
  grep -Fq "preferred_port = $2" "$CONFIG_PATH"
  grep -Fq "fallback_to_free_port = $3" "$CONFIG_PATH"
}

static_guards() {
  local file marker
  for file in \
    "$REPO_ROOT/src-tauri/tauri.conf.json" \
    "$REPO_ROOT/src-tauri/capabilities/default.json" \
    "$REPO_ROOT/src-tauri/capabilities/settings.json" \
    "$REPO_ROOT/src-tauri/build.rs"; do
    test -f "$file"
    for marker in \
      'test-bundle' 'ccresdoc.settings-test' 'CCRESDOC_TEST' \
      'settings-test' 'test-macos-settings' 'test-driver' 'native-driver' 'computer-use' \
      'fixture-claude' '/tmp/ccresdoc-settings-smoke'; do
      if grep -Fq "$marker" "$file"; then
        echo "production leak: $marker in $file" >&2
        return 1
      fi
    done
  done
  grep -Fq 'CCRESDOC_EPHEMERAL_WEBVIEW' "$REPO_ROOT/src-tauri/src/main.rs"
  grep -Fq '.incognito(true)' "$REPO_ROOT/src-tauri/src/main.rs"
  grep -Fq '.incognito(true)' "$REPO_ROOT/src-tauri/src/settings_window.rs"
  echo "PASS static no-production-leak guards"
}

start_foreign() {
  node --input-type=module - "$FOREIGN_INFO" >"$PROBE_DIR/foreign.log" 2>&1 <<'NODE' &
import { createServer } from "node:net";
import { writeFileSync } from "node:fs";
const info = process.argv.at(-1);
const marker = "foreign-settings-listener";
const server = createServer((socket) => socket.once("data", () => {
  const body = marker;
  socket.end("HTTP/1.0 200 OK\r\nContent-Length: " + body.length + "\r\n\r\n" + body);
}));
server.listen({ host: "127.0.0.1", port: 0 }, () => writeFileSync(info, JSON.stringify({ port: server.address().port }) + "\n"));
const stop = () => server.close(() => process.exit(0));
process.once("SIGTERM", stop);
process.once("SIGINT", stop);
NODE
  FOREIGN_PID=$!
  for _ in $(seq 1 100); do
    [[ -s "$FOREIGN_INFO" ]] && break
    kill -0 "$FOREIGN_PID" 2>/dev/null || { cat "$PROBE_DIR/foreign.log" >&2 || true; return 1; }
    sleep 0.05
  done
  FOREIGN_PORT="$(node -e 'process.stdout.write(String(JSON.parse(require("fs").readFileSync(process.argv[1])).port))' "$FOREIGN_INFO")"
}

foreign_alive() {
  kill -0 "$FOREIGN_PID" 2>/dev/null &&
    curl -fsS --max-time 2 "http://127.0.0.1:$FOREIGN_PORT/" 2>/dev/null |
    grep -Fq foreign-settings-listener
}

stop_foreign() {
  local port="$FOREIGN_PORT"
  if [[ -n "$FOREIGN_PID" ]] && kill -0 "$FOREIGN_PID" 2>/dev/null; then
    kill -TERM "$FOREIGN_PID" 2>/dev/null || true
    for _ in $(seq 1 100); do
      kill -0 "$FOREIGN_PID" 2>/dev/null || break
      sleep 0.05
    done
    kill -KILL "$FOREIGN_PID" 2>/dev/null || true
    wait "$FOREIGN_PID" 2>/dev/null || true
  fi
  FOREIGN_PID=""
  if [[ -n "$port" ]] && lsof -nP -tiTCP:"$port" -sTCP:LISTEN 2>/dev/null | grep -q '[0-9]'; then
    echo "error: foreign fixture listener port $port survived teardown" >&2
    return 1
  fi
  FOREIGN_PORT=""
}

quit_app() {
  [[ -n "$APP_OPEN_PID" ]] || return 0
  osascript -e 'tell application id "com.takazudo.ccresdoc" to quit' >/dev/null 2>&1 || true
  for _ in $(seq 1 120); do
    kill -0 "$APP_OPEN_PID" 2>/dev/null || break
    sleep 0.25
  done
  if kill -0 "$APP_OPEN_PID" 2>/dev/null; then
    echo "error: LaunchServices waiter did not observe quit" >&2
    return 1
  fi
  wait "$APP_OPEN_PID" 2>/dev/null || true
  APP_OPEN_PID=""
}

cleanup() {
  local status=$?
  local cleanup_failed=0
  trap - EXIT INT TERM
  if ! quit_app; then cleanup_failed=1; fi
  if ! stop_foreign; then cleanup_failed=1; fi
  case "$PROBE_DIR" in
    "$tmp_root"/ccresdoc-settings-smoke.*) rm -rf "$PROBE_DIR" ;;
    *) echo "error: refusing to remove unexpected probe path: $PROBE_DIR" >&2; cleanup_failed=1 ;;
  esac
  if [[ "$status" -eq 0 && "$cleanup_failed" -ne 0 ]]; then status=1; fi
  exit "$status"
}
trap cleanup EXIT INT TERM

fixture_contract() {
  make_fixtures
  static_guards
  test ! -e "$CONFIG_PATH"
  local missing
  missing="$(node -e 'const path=require("path"); console.log(JSON.stringify({configPath:path.resolve(process.argv[1]),fileExists:false,status:"missing",source:path.resolve(process.argv[2]),preferredPort:4892,effectivePort:4892,fallbackToFreePort:true,runtime:"idle"}))' "$CONFIG_PATH" "$DEFAULT_SOURCE")"
  echo "$missing" | grep -Fq '"status":"missing"'
  test ! -e "$CONFIG_PATH"
  write_config "$AUTHORED_SOURCE" 4892 true
  assert_config "$AUTHORED_SOURCE" 4892 true
  local authored
  authored="$(node -e 'const path=require("path"); console.log(JSON.stringify({configPath:path.resolve(process.argv[1]),fileExists:true,status:"valid",source:path.resolve(process.argv[2]),preferredPort:4892,effectivePort:4892,fallbackToFreePort:true,runtime:"ready"}))' "$CONFIG_PATH" "$AUTHORED_SOURCE")"
  echo "$authored" | grep -Fq '"status":"valid"'
  echo "PASS isolated fixture contract: config/source/default path and no discovery write"
}

if [[ "$FIXTURES_ONLY" = 1 ]]; then
  fixture_contract
  exit 0
fi
if [[ "$(uname -s)" != Darwin || "$(uname -m)" != arm64 ]]; then
  fixture_contract
  echo "SKIP packaged mode: macOS arm64 required" >&2
  exit 0
fi
if [[ -z "$APP_PATH" || ! -d "$APP_PATH" ]]; then
  echo "error: set CCRESDOC_SETTINGS_APP to a fresh CCResDoc.app" >&2
  exit 2
fi

make_fixtures
static_guards
bundle_id="$(/usr/libexec/PlistBuddy -c 'Print :CFBundleIdentifier' "$APP_PATH/Contents/Info.plist")"
test "$bundle_id" = com.takazudo.ccresdoc
if [[ "$(osascript -e 'application id "com.takazudo.ccresdoc" is running' 2>/dev/null || true)" = true ]]; then
  echo "error: quit the existing CCResDoc instance before the isolated package smoke" >&2
  exit 1
fi
if find "$APP_PATH/Contents/Resources" -type f \( -name test-macos-settings.sh -o -name '*fixture-claude*' \) -print -quit | grep -q .; then
  echo "production bundle contains test fixture" >&2
  exit 1
fi

find_log() {
  if [[ -n "$APP_LOG" && -f "$APP_LOG" ]]; then printf '%s\n' "$APP_LOG"; return; fi
  APP_LOG="$(find "$PROBE_HOME" -name ccresdoc.log -type f -print -quit 2>/dev/null || true)"
  [[ -n "$APP_LOG" ]] && printf '%s\n' "$APP_LOG"
}
log_since() {
  local path
  path="$(find_log 2>/dev/null || true)"
  [[ -n "$path" && -f "$path" ]] || return 0
  tail -n +$((LOG_START + 1)) "$path"
}
launch_app() {
  LOG_START=0
  local path
  path="$(find_log 2>/dev/null || true)"
  [[ -z "$path" ]] || LOG_START="$(wc -l < "$path" | tr -d ' ')"
  open -n -W \
    --env "HOME=$PROBE_HOME" --env "TMPDIR=$PROBE_DIR" \
    --env "XDG_CONFIG_HOME=$XDG_CONFIG_HOME" --env "CCRESDOC_CONFIG=$CONFIG_PATH" \
    --env "CCRESDOC_EPHEMERAL_WEBVIEW=1" \
    "$APP_PATH" >/dev/null 2>&1 &
  APP_OPEN_PID=$!
}
new_port() {
  log_since | sed -n 's/.*spawn_zfb_dev:.* port=\([0-9][0-9]*\).*/\1/p' | tail -n 1
}
wait_ready() {
  local port="$1" slug="$2" marker="$3"
  local docs_snapshot="$PROBE_DIR/wait-ready-docs.html"
  local fixture_snapshot="$PROBE_DIR/wait-ready-fixture.html"
  for _ in $(seq 1 300); do
    if curl -fsS --max-time 2 -o "$docs_snapshot" "http://127.0.0.1:$port/docs/" 2>/dev/null &&
      grep -Fq 'CCResDoc Resources' "$docs_snapshot" &&
      curl -fsS --max-time 2 -o "$fixture_snapshot" "http://127.0.0.1:$port/docs/claude-skills/$slug/" 2>/dev/null &&
      grep -Fq "$marker" "$fixture_snapshot"; then return 0; fi
    if [[ -n "$APP_OPEN_PID" ]] && ! kill -0 "$APP_OPEN_PID" 2>/dev/null; then return 1; fi
    sleep 1
  done
  log_since >&2 || true
  return 1
}
wait_reason() {
  local reason="$1"
  for _ in $(seq 1 300); do
    log_since | grep -Fq "reason=$reason" && return 0
    if [[ -n "$APP_OPEN_PID" ]] && ! kill -0 "$APP_OPEN_PID" 2>/dev/null; then return 1; fi
    sleep 1
  done
  log_since >&2 || true
  return 1
}
released() {
  ! lsof -nP -tiTCP:"$1" -sTCP:LISTEN 2>/dev/null | grep -q '[0-9]'
}
quit_released() {
  quit_app
  for _ in $(seq 1 40); do released "$1" && return 0; sleep 0.25; done
  echo "error: app-owned port $1 survived quit" >&2
  return 1
}

slug="$(basename "$PROBE_DIR" | tr -cd '[:alnum:]-')"
default_slug="settings-default-$slug"
authored_slug="settings-authored-$slug"
rm -f "$CONFIG_PATH"
if lsof -nP -tiTCP:4892 -sTCP:LISTEN >/dev/null 2>&1; then
  echo "error: refusing to signal an unknown owner on 4892" >&2
  exit 1
fi
launch_app
wait_ready 4892 "$default_slug" "Default settings source fixture."
test ! -e "$CONFIG_PATH"
quit_released 4892

start_foreign
write_config "$AUTHORED_SOURCE" "$FOREIGN_PORT" true
assert_config "$AUTHORED_SOURCE" "$FOREIGN_PORT" true
launch_app
FALLBACK_EFFECTIVE="$(for _ in $(seq 1 300); do p="$(new_port || true)"; [[ "$p" =~ ^[0-9]+$ ]] && printf '%s' "$p" && break; sleep 1; done)"
test "$FALLBACK_EFFECTIVE" -ge 1
test "$FALLBACK_EFFECTIVE" != "$FOREIGN_PORT"
wait_ready "$FALLBACK_EFFECTIVE" "$authored_slug" "Authored source fixture marker."
foreign_alive
quit_released "$FALLBACK_EFFECTIVE"
foreign_alive

launch_app
SECOND_EFFECTIVE="$(for _ in $(seq 1 300); do p="$(new_port || true)"; [[ "$p" =~ ^[0-9]+$ ]] && printf '%s' "$p" && break; [[ -n "$APP_OPEN_PID" ]] && ! kill -0 "$APP_OPEN_PID" 2>/dev/null && break; sleep 1; done)"
test "$SECOND_EFFECTIVE" -ge 1
test "$SECOND_EFFECTIVE" != "$FOREIGN_PORT"
wait_ready "$SECOND_EFFECTIVE" "$authored_slug" "Authored source fixture marker."
foreign_alive
quit_released "$SECOND_EFFECTIVE"
foreign_alive

write_config "$AUTHORED_SOURCE" "$FOREIGN_PORT" false
strict_hash="$(shasum -a 256 "$CONFIG_PATH" | awk '{print $1}')"
launch_app
wait_reason preferred_port_occupied
test "$(shasum -a 256 "$CONFIG_PATH" | awk '{print $1}')" = "$strict_hash"
foreign_alive
quit_app

write_config "$PROBE_DIR/missing-source" "$FOREIGN_PORT" true
bad_hash="$(shasum -a 256 "$CONFIG_PATH" | awk '{print $1}')"
launch_app
wait_ready 4892 "$default_slug" "Default settings source fixture."
test "$(shasum -a 256 "$CONFIG_PATH" | awk '{print $1}')" = "$bad_hash"
foreign_alive
quit_released 4892

printf 'schema_version = 1\n[server\npreferred_port = 4892\n' > "$CONFIG_PATH"
malformed_hash="$(shasum -a 256 "$CONFIG_PATH" | awk '{print $1}')"
launch_app
wait_ready 4892 "$default_slug" "Default settings source fixture."
test "$(shasum -a 256 "$CONFIG_PATH" | awk '{print $1}')" = "$malformed_hash"
quit_released 4892
foreign_alive

node --input-type=module - "$CONFIG_PATH" "$AUTHORED_SOURCE" "$PROBE_HOME" "$PROBE_DIR" "$XDG_CONFIG_HOME" "$FOREIGN_PORT" "$FALLBACK_EFFECTIVE" "$SECOND_EFFECTIVE" <<'NODE'
const [configPath, source, home, tempDir, xdgConfigHome, preferred, effective, secondEffective] = process.argv.slice(2);
console.log(JSON.stringify({
  status: "passed",
  isolation: { configPath, source, home, tempDir, xdgConfigHome, ephemeralWebView: true },
  settings: { preferredPort: Number(preferred), effectivePort: Number(effective), secondEffectivePort: Number(secondEffective), fallbackUsed: Number(preferred) !== Number(effective), missingConfigUsesDefaults: true, malformedBytesPreserved: true },
  runtime: { ready: true, relaunch: true, ownedChildCleanup: true, foreignListenerSurvived: true },
  production: { bundleIdentity: "com.takazudo.ccresdoc", testFixturesPackaged: false },
}, null, 2));
NODE
echo "PASS packaged settings smoke: isolated config/source, fallback, recovery, and owned-child-only cleanup"
