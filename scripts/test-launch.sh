#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd -P)"
source "$SCRIPT_DIR/lib/macos-probe.sh"
source "$SCRIPT_DIR/lib/launch-fixture.sh"

usage() {
  echo "Usage: bash scripts/test-launch.sh --bundle /absolute/path/CCResDoc.app [--controls] [count]"
}

if [[ "${1:-}" == --help ]]; then usage; exit 0; fi
if [[ "${1:-}" != --bundle || $# -lt 2 ]]; then usage >&2; exit 2; fi
APP_PATH="$2"
shift 2
CONTROLS=0
if [[ "${1:-}" == --controls ]]; then CONTROLS=1; shift; fi
COUNT="${1:-3}"
if (( $# > 1 )) || [[ ! "$COUNT" =~ ^[1-9][0-9]*$ ]]; then usage >&2; exit 2; fi
if [[ "$APP_PATH" != /* || "$APP_PATH" != *.app || ! -d "$APP_PATH" ]]; then
  echo "error: provide an absolute path to a fresh CCResDoc.app" >&2
  exit 2
fi
if [[ "$(uname -s)" != Darwin || "$(uname -m)" != arm64 ]]; then
  echo "SKIP launch smoke: requires macOS arm64" >&2
  exit 77
fi
probe_host_version
probe_bundle "$APP_PATH"
probe_preflight
if (( CONTROLS )); then
  if [[ "$(native_app accessibility)" != trusted ]] ||
     [[ "$(osascript -e 'tell application "System Events" to return UI elements enabled')" != true ]]; then
    echo "BLOCKED launch controls: Accessibility and System Events UI scripting must be available to the invoking terminal" >&2
    exit 77
  fi
fi

cleanup() {
  local status=$?
  trap - EXIT
  trap '' INT TERM
  probe_quit || status=1
  if [[ -n "${PROBE_DIR:-}" ]]; then
    probe_save_logs "launch-$RUN" || status=1
    probe_remove_directory || status=1
  fi
  exit "$status"
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM

wait_ready() {
  local attempt port
  for attempt in $(seq 1 300); do
    probe_host_alive || probe_find_host || true
    port="$(sed -n 's/.*spawn_zfb_dev:.* port=\([0-9][0-9]*\)$/\1/p' "$HOST_LOG" 2>/dev/null | tail -n 1 || true)"
    if [[ "$port" =~ ^[1-9][0-9]*$ ]] && probe_capture_sidecar &&
       [[ "$(lsof -nP -tiTCP:"$port" -sTCP:LISTEN 2>/dev/null | sort -u)" == "$SIDECAR_PID" ]]; then
      BASE_URL="http://127.0.0.1:$port"
      if docs_health &&
        fetch /docs/ccresdoc/document-sources/ "$PROBE_DIR/manager.html" &&
        grep -Fq data-ccresdoc-source-manager "$PROBE_DIR/manager.html" &&
        fetch "$(launch_fixture_route "$INITIAL_SLUG")" "$PROBE_DIR/fixture.html" &&
        grep -Fq "$INITIAL_TERM" "$PROBE_DIR/fixture.html"; then
        echo "ready: owned host $APP_PID, zfb $SIDECAR_PID, loopback port $port, manager and synthetic source"
        return 0
      fi
    fi
    probe_live "$APP_OPEN_PID" || break
    sleep 1
  done
  echo "error: the owned app did not serve the docs, manager and authored synthetic route" >&2
  return 1
}

fetch() {
  curl --noproxy '*' -fsS --max-time 3 -o "$2" "$BASE_URL$1" 2>/dev/null
}

docs_health() {
  probe_host_alive && probe_sidecar_matches "$SIDECAR_PID" &&
    fetch /docs/ "$PROBE_DIR/docs.html" &&
    grep -Fq 'CCResDoc Resources' "$PROBE_DIR/docs.html"
}

read_index() {
  fetch /docs/search-index.json "$INDEX_FILE" &&
    jq -e 'type == "array" and all(.[]; type == "object" and keys == ["body", "description", "id", "title", "url"])' \
      "$INDEX_FILE" >/dev/null 2>&1
}

check_index() {
  read_index && launch_fixture_in_index "$INDEX_FILE" "$INITIAL_SLUG" "$INITIAL_TERM" &&
    fetch "$(launch_fixture_route "$INITIAL_SLUG")" /dev/null
}

wait_index() {
  local state="$1" attempt
  for attempt in $(seq 1 120); do
    if check_index; then
      if [[ "$state" == added ]]; then
        launch_fixture_in_index "$INDEX_FILE" "$FRESH_SLUG" "$FRESH_TERM" &&
          fetch "$(launch_fixture_route "$FRESH_SLUG")" /dev/null && return 0
      elif jq -e --arg route "$(launch_fixture_route "$FRESH_SLUG")" --arg term "$FRESH_TERM" \
        'all(.[]; .url != $route and ([.title, .description, .body] | join(" ") | contains($term) | not))' \
        "$INDEX_FILE" >/dev/null; then
        return 0
      fi
    fi
    sleep 1
  done
  echo "error: watcher did not publish the $state search-index state" >&2
  return 1
}

control() {
  local script="$1"
  shift
  probe_host_alive
  osascript "$SCRIPT_DIR/$script.applescript" "$APP_PID" "$@"
}

for RUN in $(seq 1 "$COUNT"); do
  echo "=== Run $RUN/$COUNT ==="
  PROBE_DIR="$(mktemp -d /tmp/ccresdoc-launch.XXXXXX)"
  PROBE_DIR="$(cd "$PROBE_DIR" && pwd -P)"
  PROBE_HOME="$PROBE_DIR/home"
  HOST_LOG="$PROBE_HOME/Library/Application Support/com.takazudo.ccresdoc/ccresdoc.log"
  INDEX_FILE="$PROBE_DIR/search-index.json"
  prepare_launch_fixture
  probe_launch
  wait_ready
  probe_host_isolated
  if (( CONTROLS && RUN == 1 )); then
    control assert-app-theme-toggle
    control assert-app-menu-hide
    docs_health
    check_index
    control assert-app-search "$INITIAL_TERM"
    write_launch_skill "$FRESH_SLUG" "$FRESH_TERM"
    wait_index added
    control assert-app-search "$FRESH_TERM"
    rm "$SOURCE_ROOT/skills/$FRESH_SLUG/SKILL.md"
    rmdir "$SOURCE_ROOT/skills/$FRESH_SLUG"
    wait_index removed
    docs_health
    echo "PASS native controls and watcher add/remove freshness"
  fi
  probe_quit
  probe_save_logs "launch-$RUN"
  probe_remove_directory
  PROBE_DIR=""
done
echo "PASS $COUNT isolated packaged launch(es)"
