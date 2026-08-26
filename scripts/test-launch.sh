#!/usr/bin/env bash
set -uo pipefail

# Opt-in packaged macOS release acceptance smoke.  This is intentionally not a
# CI or b4push gate: it launches a real .app, drives WebKit through
# Accessibility, and mutates one exact source fixture under the user's
# settings-selected resource tree.
#
# Usage: bash scripts/test-launch.sh [--controls] [count]
#   count — number of launch iterations (default 3, must be a positive integer)
#   --controls — after the first semantic /docs/ pass, verify the hydrated
#                theme control, native menus/Command-H, the live search index,
#                native Command-K and Command-F, and watcher freshness.
# Exits 0 on success, 1 on failure.

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

if ! [[ "$COUNT" =~ ^[1-9][0-9]*$ ]]; then
  echo "error: count must be a positive integer, got: $COUNT" >&2
  exit 1
fi

APP_PATH="${APP_OVERRIDE:-/Applications/CCResDoc.app}"
HOME_DIR="${HOME:-}"
APP_LOG=""
LOG_BASELINE_INODE=""
LOG_BASELINE_SIZE=""
LOG_BASELINE_HEAD=""
EFFECTIVE_PORT=""
SIDECAR_PID=""
READY_OBSERVED=0
CONFIG_PATH=""
CONFIG_SELECTION_JSON=""
INDEX_FILE=""
FIXTURE_SOURCE_KIND=""
FIXTURE_SOURCE_DIR=""
FIXTURE_SLUG=""
FIXTURE_DIR=""
FIXTURE_FILE=""
FIXTURE_TERM=""

TMP_PARENT="${TMPDIR:-/tmp}"
LAUNCH_TMP="$(mktemp -d "$TMP_PARENT/ccresdoc-launch.XXXXXX" 2>/dev/null || true)"
if [[ -z "$LAUNCH_TMP" || ! -d "$LAUNCH_TMP" ]]; then
  echo "error: could not create a private launch-smoke temporary directory" >&2
  exit 1
fi
INDEX_FILE="$LAUNCH_TMP/search-index.json"

script_dir() {
  cd "$(dirname "${BASH_SOURCE[0]}")" && pwd
}

resolve_config_path() {
  if [[ -n "${CCRESDOC_CONFIG:-}" ]]; then
    CONFIG_PATH="$CCRESDOC_CONFIG"
  elif [[ -n "${XDG_CONFIG_HOME:-}" ]]; then
    CONFIG_PATH="$XDG_CONFIG_HOME/ccresdoc/config.toml"
  elif [[ -n "$HOME_DIR" ]]; then
    CONFIG_PATH="$HOME_DIR/.config/ccresdoc/config.toml"
  else
    echo "error: HOME is empty; cannot resolve the CCResDoc settings file" >&2
    return 1
  fi
}

# Read only the settings authority used by the host.  The small parser accepts
# the versioned config's basic TOML fields without adding a runtime dependency
# to the packaged app.  Missing config intentionally maps to the host defaults;
# a present malformed config is rejected instead of guessing its selection.
read_config_selection() {
  resolve_config_path || return 1
  CONFIG_SELECTION_JSON="$(node - "$CONFIG_PATH" <<'NODE'
const fs = require("node:fs");
const path = require("node:path");
const os = require("node:os");

const configPath = process.argv[2];
const home = process.env.HOME || os.homedir();
const defaults = {
  claude: { enabled: true, dir: path.join(home, ".claude") },
  codex: { enabled: false, dir: path.join(home, ".codex") },
};

if (!fs.existsSync(configPath)) {
  process.stdout.write(JSON.stringify(defaults));
  process.exit(0);
}

const text = fs.readFileSync(configPath, "utf8");
let section = "";
const values = new Map();
function stripComment(line) {
  let quote = "";
  for (let i = 0; i < line.length; i += 1) {
    const c = line[i];
    if ((c === '"' || c === "'") && (i === 0 || line[i - 1] !== "\\")) {
      quote = quote === c ? "" : quote || c;
    } else if (c === "#" && !quote) {
      return line.slice(0, i);
    }
  }
  return line;
}
function parseString(raw, key) {
  const value = raw.trim();
  if (value.startsWith('"')) {
    try { return JSON.parse(value); } catch (error) { throw new Error(`${key} is not a valid basic string: ${error.message}`); }
  }
  if (value.startsWith("'")) {
    if (!value.endsWith("'")) throw new Error(`${key} is not a valid literal string`);
    return value.slice(1, -1);
  }
  throw new Error(`${key} must be a quoted source path`);
}
function parseBool(raw, key) {
  const value = raw.trim();
  if (value === "true") return true;
  if (value === "false") return false;
  throw new Error(`${key} must be true or false`);
}
for (const rawLine of text.split(/\r?\n/)) {
  const line = stripComment(rawLine).trim();
  if (!line) continue;
  const table = line.match(/^\[([A-Za-z0-9_-]+)\]$/);
  if (table) { section = table[1]; continue; }
  const assignment = line.match(/^([A-Za-z0-9_-]+)\s*=\s*(.+)$/);
  if (!assignment) throw new Error(`unsupported TOML line: ${rawLine}`);
  values.set(`${section}.${assignment[1]}`, assignment[2].trim());
}

function optionalBool(key, fallback) {
  return values.has(key) ? parseBool(values.get(key), key) : fallback;
}
function optionalPath(key, fallback) {
  const raw = values.has(key) ? parseString(values.get(key), key) : fallback;
  if (raw === "~") return home;
  if (raw.startsWith("~/")) return path.join(home, raw.slice(2));
  if (!path.isAbsolute(raw)) throw new Error(`${key} must be absolute or start with ~/`);
  return raw;
}

const result = {
  claude: {
    enabled: optionalBool("resources.claude", defaults.claude.enabled),
    dir: optionalPath("source.claude_dir", "~/.claude"),
  },
  codex: {
    enabled: optionalBool("resources.codex", defaults.codex.enabled),
    dir: optionalPath("source.codex_dir", "~/.codex"),
  },
};
for (const [kind, source] of Object.entries(result)) {
  if (source.enabled && !fs.statSync(source.dir, { throwIfNoEntry: false })?.isDirectory()) {
    throw new Error(`enabled ${kind} source is not an accessible directory: ${source.dir}`);
  }
}
if (!result.claude.enabled && !result.codex.enabled) {
  throw new Error("settings enable neither Claude nor Codex resources");
}
process.stdout.write(JSON.stringify(result));
NODE
  )" || {
    echo "error: could not read enabled resource selection from $CONFIG_PATH" >&2
    return 1
  }
  echo "  settings: $(node -e 'const value=JSON.parse(process.argv[1]); console.log(Object.entries(value).filter(([,v]) => v.enabled).map(([k,v]) => `${k}=${v.dir}`).join(", "))' "$CONFIG_SELECTION_JSON")"
}

capture_log_baseline() {
  if [[ -f "$APP_LOG" ]]; then
    LOG_BASELINE_INODE="$(stat -f '%i' "$APP_LOG" 2>/dev/null || true)"
    LOG_BASELINE_SIZE="$(stat -f '%z' "$APP_LOG" 2>/dev/null || true)"
    LOG_BASELINE_HEAD="$(head -c 1024 "$APP_LOG" | shasum -a 256 2>/dev/null | awk '{print $1}')"
  else
    LOG_BASELINE_INODE=""
    LOG_BASELINE_SIZE=""
    LOG_BASELINE_HEAD=""
  fi
}

# Return only log bytes that could have been written by the current launch.
# Normal launches append to the same file, so the byte offset is unambiguous.
# If the host replaces/truncates the file, use the last fresh launch-start
# marker and discard all older spawn evidence before considering a port.
fresh_log_delta() {
  [[ -f "$APP_LOG" ]] || return 0
  local inode size
  inode="$(stat -f '%i' "$APP_LOG" 2>/dev/null || true)"
  size="$(stat -f '%z' "$APP_LOG" 2>/dev/null || true)"
  local current_head
  current_head="$(head -c 1024 "$APP_LOG" | shasum -a 256 2>/dev/null | awk '{print $1}')"
  if [[ -n "$LOG_BASELINE_INODE" && "$inode" == "$LOG_BASELINE_INODE" &&
    -n "$LOG_BASELINE_SIZE" && "$size" =~ ^[0-9]+$ && "$size" -ge "$LOG_BASELINE_SIZE" &&
    -n "$LOG_BASELINE_HEAD" && "$current_head" == "$LOG_BASELINE_HEAD" ]]; then
    tail -c "+$((LOG_BASELINE_SIZE + 1))" "$APP_LOG" 2>/dev/null || true
    return 0
  fi
  awk '
    /launch\[[0-9][0-9]*\]: start/ { start = NR }
    { lines[NR] = $0 }
    END {
      if (!start) start = NR + 1
      for (i = start; i <= NR; i++) print lines[i]
    }
  ' "$APP_LOG" 2>/dev/null || true
}

fresh_spawn_port() {
  fresh_log_delta |
    sed -n 's/.*spawn_zfb_dev:.* port=\([0-9][0-9]*\).*/\1/p' |
    tail -n 1
}

fresh_spawn_pid() {
  fresh_log_delta |
    sed -n 's/.*spawn_zfb_dev: pid=\([0-9][0-9]*\).*/\1/p' |
    tail -n 1
}

stop_owned_sidecar() {
  local pid="$SIDECAR_PID"
  SIDECAR_PID=""
  [[ "$pid" =~ ^[0-9]+$ ]] || return 0
  kill -0 "$pid" 2>/dev/null || return 0
  # Only signal a live process whose command line still identifies the native
  # zfb sidecar.  This prevents stale PID reuse from touching user processes.
  local command_line
  command_line="$(ps -p "$pid" -o command= 2>/dev/null || true)"
  [[ "$command_line" == *zfb* ]] || return 0
  kill -TERM "$pid" 2>/dev/null || true
  for _ in $(seq 1 40); do
    kill -0 "$pid" 2>/dev/null || return 0
    sleep 0.1
  done
  kill -KILL "$pid" 2>/dev/null || true
}

stop_owned_app() {
  if [[ "$(uname -s 2>/dev/null || true)" == "Darwin" ]]; then
    osascript -e 'tell application id "com.takazudo.ccresdoc" to quit' >/dev/null 2>&1 || true
  fi
  for _ in $(seq 1 60); do
    pgrep -x CCResDoc >/dev/null 2>&1 || break
    sleep 0.1
  done
  # This is an exact process-name fallback for a hung app, never a port-wide
  # kill.  The app's own exit path normally tears down its sidecar first.
  if pgrep -x CCResDoc >/dev/null 2>&1; then
    pkill -x CCResDoc >/dev/null 2>&1 || true
    sleep 1
  fi
}

stop_owned_runtime() {
  stop_owned_app
  stop_owned_sidecar
}

sidecar_is_alive() {
  local pid="$1" command_line
  [[ "$pid" =~ ^[1-9][0-9]*$ ]] || return 1
  kill -0 "$pid" 2>/dev/null || return 1
  command_line="$(ps -p "$pid" -o command= 2>/dev/null || true)"
  [[ "$command_line" == *zfb* ]]
}

wait_ready() {
  EFFECTIVE_PORT=""
  SIDECAR_PID=""
  READY_OBSERVED=0
  local docs_snapshot="$LAUNCH_TMP/docs.html"
  for i in $(seq 1 300); do
    local fresh_port fresh_pid http
    fresh_port="$(fresh_spawn_port)"
    fresh_pid="$(fresh_spawn_pid)"
    if [[ "$fresh_port" =~ ^[1-9][0-9]*$ ]]; then
      EFFECTIVE_PORT="$fresh_port"
      SIDECAR_PID="$fresh_pid"
    fi
    if [[ "$EFFECTIVE_PORT" =~ ^[1-9][0-9]*$ && "$SIDECAR_PID" =~ ^[1-9][0-9]*$ ]] &&
      sidecar_is_alive "$SIDECAR_PID"; then
      http="$(curl -sS --max-time 2 -o "$docs_snapshot" -w '%{http_code}' "http://127.0.0.1:$EFFECTIVE_PORT/docs/" 2>/dev/null || true)"
      if [[ "$http" == "200" ]] && grep -Fq "CCResDoc Resources" "$docs_snapshot"; then
        READY_OBSERVED=1
        echo "  ready: /docs/ HTTP 200 at effective port $EFFECTIVE_PORT ($((i))s)"
        return 0
      fi
    fi
    sleep 1
  done
  if [[ -z "$EFFECTIVE_PORT" ]]; then
    echo "  error: no fresh spawn_zfb_dev port evidence appeared in $APP_LOG" >&2
  else
    echo "  error: effective port $EFFECTIVE_PORT did not serve the semantic /docs/ readiness marker" >&2
  fi
  fresh_log_delta >&2 || true
  return 1
}

docs_health() {
  local snapshot="$LAUNCH_TMP/docs-health.html" http
  [[ "$EFFECTIVE_PORT" =~ ^[1-9][0-9]*$ ]] || {
    echo "error: effective port is unavailable for /docs/ health" >&2
    return 1
  }
  http="$(curl -sS --max-time 3 -o "$snapshot" -w '%{http_code}' "http://127.0.0.1:$EFFECTIVE_PORT/docs/" 2>/dev/null || true)"
  if [[ "$http" != "200" ]] || ! grep -Fq "CCResDoc Resources" "$snapshot"; then
    echo "error: /docs/ health failed on effective port $EFFECTIVE_PORT (HTTP $http)" >&2
    return 1
  fi
  return 0
}

enabled_source_has_entries() {
  local kind="$1" prefix
  prefix="$kind:"
  jq -e --arg prefix "$prefix" 'any(.[]; (.id | startswith($prefix)))' "$INDEX_FILE" >/dev/null
}

wait_for_sample_url() {
  local sample_url="$1" sample_http="" attempt
  for attempt in $(seq 1 20); do
    sample_http="$(curl -sS --max-time 2 -o /dev/null -w '%{http_code}' "http://127.0.0.1:$EFFECTIVE_PORT$sample_url" 2>/dev/null || true)"
    if [[ "$sample_http" == "200" ]]; then
      return 0
    fi
    sleep 0.25
  done
  echo "error: sampled search URL did not reach HTTP 200 after 20 attempts (last HTTP $sample_http): $sample_url" >&2
  return 1
}

check_index_contract() {
  local phase="$1" http entry_count bytes sample_url
  [[ "$EFFECTIVE_PORT" =~ ^[1-9][0-9]*$ ]] || return 1
  http="$(curl -sS --max-time 3 -o "$INDEX_FILE" -w '%{http_code}' "http://127.0.0.1:$EFFECTIVE_PORT/docs/search-index.json" 2>/dev/null || true)"
  if [[ "$http" != "200" ]]; then
    echo "error: search index $phase returned HTTP $http on effective port $EFFECTIVE_PORT" >&2
    return 1
  fi
  if ! jq -e 'type == "array" and all(.[]; (type == "object" and ((keys | sort) == ["body", "description", "id", "title", "url"])))' "$INDEX_FILE" >/dev/null; then
    echo "error: search index $phase is not an array of exact five-key entries" >&2
    return 1
  fi
  local claude_enabled codex_enabled
  claude_enabled="$(jq -r '.claude.enabled' <<<"$CONFIG_SELECTION_JSON")"
  codex_enabled="$(jq -r '.codex.enabled' <<<"$CONFIG_SELECTION_JSON")"
  if [[ "$claude_enabled" == "true" ]] && ! enabled_source_has_entries claude; then
    echo "error: enabled Claude source has no search-index entries" >&2
    return 1
  fi
  if [[ "$codex_enabled" == "true" ]] && ! enabled_source_has_entries codex; then
    echo "error: enabled Codex source has no search-index entries" >&2
    return 1
  fi
  while IFS= read -r sample_url; do
    [[ -n "$sample_url" ]] || continue
    case "$sample_url" in
      /docs/*) ;;
      *) echo "error: sampled search URL is outside /docs/: $sample_url" >&2; return 1 ;;
    esac
    if ! wait_for_sample_url "$sample_url"; then
      return 1
    fi
  done < <(jq -r '.[0:3] | .[].url' "$INDEX_FILE")
  entry_count="$(jq 'length' "$INDEX_FILE")"
  bytes="$(wc -c < "$INDEX_FILE" | tr -d ' ')"
  echo "  search index $phase: HTTP 200, $entry_count entries, $bytes bytes; exact keys/source coverage/URL samples passed"
  return 0
}

known_indexed_term() {
  node - "$INDEX_FILE" <<'NODE'
const fs = require("node:fs");
const entries = JSON.parse(fs.readFileSync(process.argv[2], "utf8"));
const text = entries.slice(0, 1).flatMap((entry) => [entry.title, entry.description, entry.body]).join(" ");
const match = text.match(/[A-Za-z][A-Za-z0-9_-]{3,}/);
if (!match) process.exit(1);
process.stdout.write(match[0]);
NODE
}

run_search_surface_smoke() {
  local term="$1" result
  if ! result="$(osascript "$(script_dir)/assert-app-search.applescript" "$term" 2>&1)"; then
    echo "error: native search/find accessibility check failed: $result" >&2
    return 1
  fi
  echo "  $result"
}

create_fixture() {
  local source_kind="$1"
  FIXTURE_SOURCE_KIND="$source_kind"
  FIXTURE_SOURCE_DIR="$(jq -r --arg kind "$source_kind" '.[$kind].dir' <<<"$CONFIG_SELECTION_JSON")"
  FIXTURE_SLUG="ccresdoc-search-smoke-$(date +%s)-$$"
  FIXTURE_DIR="$FIXTURE_SOURCE_DIR/skills/$FIXTURE_SLUG"
  FIXTURE_FILE="$FIXTURE_DIR/SKILL.md"
  FIXTURE_TERM="ccresdocfresh$(date +%s)$$"
  if [[ -e "$FIXTURE_DIR" || -L "$FIXTURE_DIR" ]]; then
    echo "error: refusing to overwrite an existing fixture path: $FIXTURE_DIR" >&2
    return 1
  fi
  mkdir -p "$FIXTURE_DIR" || return 1
  if ! printf '%s\n' \
    '---' \
    'name: CCResDoc Search Freshness Fixture' \
    'description: Temporary release acceptance fixture.' \
    '---' \
    '' \
    "This exact indexed term proves watcher freshness: $FIXTURE_TERM" \
    > "$FIXTURE_FILE"; then
    echo "error: could not write the exact temporary source fixture: $FIXTURE_FILE" >&2
    return 1
  fi
  echo "  freshness fixture: $FIXTURE_FILE (enabled $source_kind source)"
}

fixture_index_has_term() {
  local expected="$1" prefix="$2"
  jq -e --arg expected "$expected" --arg prefix "$prefix" \
    'any(.[]; (.id | startswith($prefix)) and ([.title, .description, .body] | join(" ") | contains($expected)))' \
    "$INDEX_FILE" >/dev/null
}

wait_index_for_fixture() {
  local prefix="$FIXTURE_SOURCE_KIND:"
  for _ in $(seq 1 120); do
    local http
    http="$(curl -sS --max-time 3 -o "$INDEX_FILE" -w '%{http_code}' "http://127.0.0.1:$EFFECTIVE_PORT/docs/search-index.json" 2>/dev/null || true)"
    if [[ "$http" == "200" ]] && jq -e 'type == "array"' "$INDEX_FILE" >/dev/null 2>&1 && fixture_index_has_term "$FIXTURE_TERM" "$prefix"; then
      echo "  freshness: watcher regenerated the index with $FIXTURE_TERM"
      return 0
    fi
    sleep 1
  done
  echo "error: search index never exposed the fresh fixture term $FIXTURE_TERM" >&2
  return 1
}

remove_fixture() {
  [[ -n "$FIXTURE_DIR" ]] || return 0
  local expected_dir="$FIXTURE_SOURCE_DIR/skills/$FIXTURE_SLUG"
  if [[ "$FIXTURE_DIR" != "$expected_dir" || -L "$FIXTURE_DIR" ]]; then
    echo "error: refusing to remove an unexpected fixture path: $FIXTURE_DIR" >&2
    return 1
  fi
  if [[ -e "$FIXTURE_FILE" && ! -f "$FIXTURE_FILE" ]]; then
    echo "error: refusing to remove a non-file fixture payload: $FIXTURE_FILE" >&2
    return 1
  fi
  rm -f "$FIXTURE_FILE" || return 1
  # Remove only the exact directory we created.  If another process added
  # content, rmdir safely leaves that user content untouched.
  rmdir "$FIXTURE_DIR" 2>/dev/null || true
  return 0
}

wait_index_without_fixture() {
  [[ -n "$FIXTURE_TERM" ]] || return 0
  local prefix="$FIXTURE_SOURCE_KIND:"
  for _ in $(seq 1 90); do
    local http
    http="$(curl -sS --max-time 3 -o "$INDEX_FILE" -w '%{http_code}' "http://127.0.0.1:$EFFECTIVE_PORT/docs/search-index.json" 2>/dev/null || true)"
    if [[ "$http" == "200" ]] && jq -e 'type == "array"' "$INDEX_FILE" >/dev/null 2>&1 && ! fixture_index_has_term "$FIXTURE_TERM" "$prefix"; then
      echo "  freshness cleanup: fixture term disappeared from the live index"
      FIXTURE_DIR=""
      FIXTURE_FILE=""
      FIXTURE_TERM=""
      FIXTURE_SOURCE_DIR=""
      FIXTURE_SOURCE_KIND=""
      return 0
    fi
    sleep 1
  done
  echo "error: watcher did not settle after exact fixture cleanup" >&2
  return 1
}

run_controls() {
  local control_result hide_result known_term source_kind
  [[ "$(uname -s 2>/dev/null || true)" == "Darwin" ]] || {
    echo "error: --controls requires macOS WebKit accessibility" >&2
    return 1
  }
  read_config_selection || return 1

  control_result="$(osascript "$(script_dir)/assert-app-theme-toggle.applescript" 2>&1)" || {
    echo "error: ThemeToggle accessibility check failed: $control_result" >&2
    return 1
  }
  echo "  $control_result"
  hide_result="$(osascript "$(script_dir)/assert-app-menu-hide.applescript" 2>&1)" || {
    echo "error: native menu/Command-H accessibility check failed: $hide_result" >&2
    return 1
  }
  echo "  $hide_result"
  if ! docs_health; then
    echo "error: /docs/ was not healthy after native hide/unhide" >&2
    return 1
  fi
  echo "  /docs/: healthy after native hide/unhide on effective port $EFFECTIVE_PORT"

  check_index_contract initial || return 1
  known_term="$(known_indexed_term 2>/dev/null || true)"
  if [[ -z "$known_term" ]]; then
    echo "error: could not derive an ASCII term from the first live search-index entry" >&2
    return 1
  fi
  echo "  search smoke: using known indexed term '$known_term'"
  run_search_surface_smoke "$known_term" || return 1

  if [[ "$(jq -r '.claude.enabled' <<<"$CONFIG_SELECTION_JSON")" == "true" ]]; then
    source_kind="claude"
  else
    source_kind="codex"
  fi
  create_fixture "$source_kind" || return 1
  if ! wait_index_for_fixture; then
    return 1
  fi
  run_search_surface_smoke "$FIXTURE_TERM" || return 1
  if ! remove_fixture; then
    return 1
  fi
  wait_index_without_fixture || return 1
  check_index_contract post-freshness || return 1
  if ! docs_health; then
    echo "error: /docs/ or sidecar health failed after all search controls" >&2
    return 1
  fi
  echo "  /docs/: healthy after search, freshness, cleanup, and index checks"
}

cleanup() {
  local status=$? cleanup_failed=0
  trap - EXIT INT TERM
  if ! remove_fixture; then
    cleanup_failed=1
  elif [[ -n "$FIXTURE_TERM" ]]; then
    # On an interrupted controls run, let a live watcher observe the exact
    # deletion before tearing the app down.  If the sidecar is already gone,
    # there is no live state left to settle and the exact source path is clean.
    if [[ "$SIDECAR_PID" =~ ^[1-9][0-9]*$ ]] && sidecar_is_alive "$SIDECAR_PID"; then
      if ! wait_index_without_fixture; then cleanup_failed=1; fi
    else
      FIXTURE_DIR=""
      FIXTURE_FILE=""
      FIXTURE_TERM=""
      FIXTURE_SOURCE_DIR=""
      FIXTURE_SOURCE_KIND=""
    fi
  fi
  stop_owned_runtime
  case "$LAUNCH_TMP" in
    "$TMP_PARENT"/ccresdoc-launch.*) rm -rf "$LAUNCH_TMP" ;;
    *) echo "error: refusing to remove unexpected launch temp path: $LAUNCH_TMP" >&2; cleanup_failed=1 ;;
  esac
  if [[ "$status" -eq 0 && "$cleanup_failed" -ne 0 ]]; then status=1; fi
  exit "$status"
}
trap cleanup EXIT INT TERM

PASS=0
FAIL=0
APP_LOG="$HOME_DIR/Library/Application Support/com.takazudo.ccresdoc/ccresdoc.log"
if [[ -z "$HOME_DIR" ]]; then
  echo "error: HOME is empty; cannot locate the current CCResDoc host log" >&2
  exit 1
fi

for RUN in $(seq 1 "$COUNT"); do
  echo "=== Run $RUN/$COUNT ==="
  stop_owned_runtime
  capture_log_baseline

  if [[ ! -d "$APP_PATH" ]]; then
    echo "  Run $RUN: FAIL (app bundle not found: $APP_PATH)" >&2
    FAIL=$((FAIL + 1))
    continue
  fi
  if ! open "$APP_PATH" >/dev/null 2>&1; then
    echo "  Run $RUN: FAIL (could not open app bundle: $APP_PATH)" >&2
    FAIL=$((FAIL + 1))
    continue
  fi

  OK=0
  if wait_ready; then
    OK=1
    if [[ "$CHECK_CONTROLS" == "1" && "$RUN" == "1" ]]; then
      if ! run_controls; then OK=0; fi
    fi
  fi
  if [[ "$OK" == "1" ]]; then
    PASS=$((PASS + 1))
  else
    FAIL=$((FAIL + 1))
  fi
done

echo ""
echo "=== Results: $PASS/$COUNT passed, $FAIL failed ==="
[[ "$FAIL" -gt 0 ]] && exit 1 || exit 0
