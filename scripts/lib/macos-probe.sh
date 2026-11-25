# Shared LaunchServices ownership and teardown for the packaged smoke scripts.
MACOS_PROBE_LIB="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)"
APP_OPEN_PID=""
APP_PID=""
APP_START=""
SIDECAR_PID=""
SIDECAR_START=""
PROBE_RUN_TOKEN=""

native_app() {
  /usr/bin/osascript -l JavaScript "$MACOS_PROBE_LIB/macos-probe-app.js" "$@"
}

probe_preflight() {
  local running
  running="$(native_app list)" || return 1
  if [[ "$running" != 0 ]]; then
    echo "BLOCKED: found $running running CCResDoc instance(s); quit them before the isolated probe" >&2
    return 1
  fi
}

probe_host_version() {
  local version major
  version="$(sw_vers -productVersion)" || return 1
  major="${version%%.*}"
  if ! [[ "$major" =~ ^[0-9]+$ && "$major" -ge 27 ]]; then
    echo "BLOCKED packaged smoke: requires macOS 27 or newer, found $version" >&2
    return 77
  fi
}

probe_bundle() {
  if [[ "$1" != /* || "$1" != *.app || ! -d "$1" ]]; then
    echo "error: provide an absolute path to a fresh CCResDoc.app" >&2
    return 1
  fi
  APP_PATH="$(cd "$1" && pwd -P)" || return 1
  case "$APP_PATH" in
    /Applications/*|/System/Applications/*)
      echo "error: use a fresh build outside the system Applications directories" >&2
      return 1
      ;;
  esac
  local bundle_id
  bundle_id="$(/usr/libexec/PlistBuddy -c 'Print :CFBundleIdentifier' "$APP_PATH/Contents/Info.plist")" || return 1
  if [[ "$bundle_id" != com.takazudo.ccresdoc ||
        ! -x "$APP_PATH/Contents/MacOS/ccresdoc" ||
        ! -s "$APP_PATH/Contents/Resources/runtime-workspace/version.txt" ]]; then
    echo "error: unexpected bundle identity at $APP_PATH" >&2
    return 1
  fi
}

probe_launch() {
  probe_preflight || return 1
  mkdir -p "$PROBE_HOME" "$PROBE_DIR/tmp" "$PROBE_DIR/xdg" || return 1
  PROBE_RUN_TOKEN="$(basename "$PROBE_DIR")-$RANDOM-$RANDOM"
  open -n -W \
    --env "HOME=$PROBE_HOME" --env "TMPDIR=$PROBE_DIR/tmp" \
    --env "XDG_CONFIG_HOME=$PROBE_DIR/xdg" --env "CCRESDOC_CONFIG=$CONFIG_PATH" \
    --env "CCRESDOC_EPHEMERAL_WEBVIEW=1" --env "CCRESDOC_PROBE_RUN=$PROBE_RUN_TOKEN" \
    "$@" "$APP_PATH" >> "$PROBE_DIR/open.log" 2>&1 &
  APP_OPEN_PID=$!
}

probe_process_start() {
  ps -p "$1" -o lstart= 2>/dev/null
}

probe_live() {
  local state
  state="$(ps -p "$1" -o stat= 2>/dev/null)" && [[ -n "$state" && "$state" != Z* ]]
}

probe_host_matches() {
  local pid="$1"
  [[ "$pid" =~ ^[1-9][0-9]*$ && -n "$PROBE_RUN_TOKEN" ]] || return 1
  [[ "$(ps -ww -p "$pid" -o command= 2>/dev/null)" == "$APP_PATH/Contents/MacOS/ccresdoc" ]] &&
    probe_env_matches "$pid" "CCRESDOC_PROBE_RUN=$PROBE_RUN_TOKEN"
}

probe_env_matches() {
  local pid="$1" args expected
  shift
  # Darwin ps exposes the launched process's original environment with eww.
  # Never print it. The unique per-run value disambiguates a racing launch
  # of the same bundle, even if LaunchServices returns its registration.
  args="$(ps eww -p "$pid" -o command= 2>/dev/null)" || return 1
  for expected in "$@"; do
    [[ " $args " == *" $expected "* ]] || return 1
  done
}

probe_host_isolated() {
  [[ "$(native_app list)" == 1 ]] && probe_host_alive && probe_env_matches "$APP_PID" \
    "HOME=$PROBE_HOME" "TMPDIR=$PROBE_DIR/tmp" "XDG_CONFIG_HOME=$PROBE_DIR/xdg" \
    "CCRESDOC_CONFIG=$CONFIG_PATH" "CCRESDOC_EPHEMERAL_WEBVIEW=1"
}

probe_find_host() {
  local candidates candidate owned=""
  candidates="$(native_app pids "$APP_PATH")" || return 1
  if [[ "${1:-}" != cleanup ]]; then
    [[ "$candidates" =~ ^[1-9][0-9]*$ ]] || return 1
  fi
  while IFS= read -r candidate; do
    if probe_host_matches "$candidate"; then
      [[ -z "$owned" ]] || return 1
      owned="$candidate"
    fi
  done <<< "$candidates"
  [[ -n "$owned" ]] || return 1
  APP_START="$(probe_process_start "$owned")" || return 1
  APP_PID="$owned"
  printf 'host pid=%s start=%s bundle=%s\n' "$APP_PID" "$APP_START" "$APP_PATH" >> "$PROBE_DIR/process-samples.log"
}

probe_host_alive() {
  [[ -n "$APP_PID" && -n "$APP_START" ]] &&
    [[ "$(probe_process_start "$APP_PID")" == "$APP_START" ]] &&
    probe_host_matches "$APP_PID"
}

probe_sidecar_matches() {
  local pid="$1" pgid command port
  read -r pgid command < <(ps -ww -p "$pid" -o pgid=,command= 2>/dev/null) || return 1
  local workspace="$PROBE_HOME/Library/Application Support/com.takazudo.ccresdoc/app-workspace"
  local prefix="$workspace/node_modules/@takazudo/zfb-darwin-arm64/zfb dev --host 127.0.0.1 --port "
  [[ "$pgid" == "$pid" && "$command" == "$prefix"* ]] || return 1
  port="${command#"$prefix"}"
  [[ "$port" =~ ^[1-9][0-9]{0,4}$ ]] || return 1
  lsof -a -p "$pid" -d cwd -Fn 2>/dev/null | grep -Fx "n$workspace" >/dev/null
}

probe_capture_sidecar() {
  local pid parent matches=""
  probe_host_alive || return 1
  while read -r pid parent; do
    [[ "$parent" == "$APP_PID" ]] || continue
    if probe_sidecar_matches "$pid"; then
      [[ -z "$matches" ]] || return 1
      matches="$pid"
    fi
  done < <(ps -axo pid=,ppid=)
  [[ -n "$matches" ]] || return 1
  if [[ "$matches" != "$SIDECAR_PID" ]]; then
    SIDECAR_START="$(probe_process_start "$matches")" || return 1
    SIDECAR_PID="$matches"
    printf 'sidecar pid=%s pgid=%s start=%s parent=%s\n' "$matches" "$matches" "$SIDECAR_START" "$APP_PID" >> "$PROBE_DIR/process-samples.log"
  fi
}

# A startup failure can kill the host before parentage is sampled. In that
# case a child still needs the current host log, run token, workspace and PGID.
probe_recover_sidecar() {
  [[ -z "$SIDECAR_PID" && -f "$HOST_LOG" ]] || return 0
  local pid
  pid="$(sed -n 's/.*spawn_zfb_dev: pid=\([0-9][0-9]*\)$/\1/p' "$HOST_LOG" | tail -n 1)"
  if [[ "$pid" =~ ^[1-9][0-9]*$ ]] && probe_sidecar_matches "$pid" &&
     probe_env_matches "$pid" "CCRESDOC_PROBE_RUN=$PROBE_RUN_TOKEN"; then
    SIDECAR_START="$(probe_process_start "$pid")" || return 1
    SIDECAR_PID="$pid"
  fi
}

probe_group_alive() {
  ps -axo pgid=,stat= | awk -v pgid="$1" '$1 == pgid && $2 !~ /^Z/ { found=1 } END { exit !found }'
}

probe_stop_sidecar() {
  [[ -n "$SIDECAR_PID" ]] || return 0
  local attempt
  for attempt in $(seq 1 40); do
    probe_group_alive "$SIDECAR_PID" || break
    sleep 0.1
  done
  if probe_group_alive "$SIDECAR_PID"; then
    # A group without its verified leader is ambiguous. Leave it for diagnosis.
    if [[ "$(probe_process_start "$SIDECAR_PID")" != "$SIDECAR_START" ]] ||
      ! probe_sidecar_matches "$SIDECAR_PID"; then
      echo "error: ownership of remaining sidecar group $SIDECAR_PID is ambiguous" >&2
      return 1
    fi
    /bin/kill -TERM -- "-$SIDECAR_PID" 2>/dev/null || return 1
    for attempt in $(seq 1 40); do
      probe_group_alive "$SIDECAR_PID" || break
      sleep 0.1
    done
    if probe_group_alive "$SIDECAR_PID"; then
      [[ "$(probe_process_start "$SIDECAR_PID")" == "$SIDECAR_START" ]] &&
        probe_sidecar_matches "$SIDECAR_PID" || return 1
      /bin/kill -KILL -- "-$SIDECAR_PID" 2>/dev/null || return 1
      for attempt in $(seq 1 40); do
        probe_group_alive "$SIDECAR_PID" || break
        sleep 0.1
      done
      if probe_group_alive "$SIDECAR_PID"; then
        echo "error: owned sidecar group $SIDECAR_PID survived KILL" >&2
        return 1
      fi
    fi
    echo "error: sidecar group $SIDECAR_PID required fallback cleanup" >&2
    SIDECAR_PID=""
    return 1
  fi
  SIDECAR_PID=""
}

probe_quit() {
  [[ -n "$APP_OPEN_PID" ]] || return 0
  local attempt requested="" status=0
  if [[ -z "$APP_PID" ]]; then
    for attempt in $(seq 1 40); do
      probe_find_host cleanup && break
      probe_live "$APP_OPEN_PID" || break
      sleep 0.1
    done
  fi
  if probe_host_alive; then
    probe_capture_sidecar || true
    requested="$(native_app quit "$APP_PATH" "$APP_PID")" || status=1
    [[ "$requested" == requested ]] || status=1
    for attempt in $(seq 1 80); do
      probe_live "$APP_PID" || break
      sleep 0.25
    done
    if probe_host_alive; then
      echo "error: owned host $APP_PID did not quit gracefully" >&2
      status=1
      kill -TERM "$APP_PID" 2>/dev/null || true
      for attempt in $(seq 1 40); do
        probe_live "$APP_PID" || break
        sleep 0.1
      done
    fi
  fi
  if [[ -n "$APP_PID" ]] && probe_live "$APP_PID"; then
    echo "error: host $APP_PID still present after cleanup" >&2
    return 1
  fi
  for attempt in $(seq 1 30); do
    probe_live "$APP_OPEN_PID" || break
    sleep 0.1
  done
  if probe_live "$APP_OPEN_PID"; then
    echo "error: LaunchServices waiter $APP_OPEN_PID is still active; retaining probe directory" >&2
    return 1
  fi
  wait "$APP_OPEN_PID" 2>/dev/null || status=1
  APP_OPEN_PID=""
  APP_PID=""
  APP_START=""
  probe_recover_sidecar || status=1
  probe_stop_sidecar || status=1
  return "$status"
}

probe_remove_directory() {
  if [[ -n "$APP_OPEN_PID" || -n "$SIDECAR_PID" ]]; then
    echo "error: retaining $PROBE_DIR while runtime ownership or shutdown is unresolved" >&2
    return 1
  fi
  case "$(basename "$PROBE_DIR")" in
    ccresdoc-launch.*|ccresdoc-macos-package.*|ccresdoc-settings-smoke.*)
      (cd "$PROBE_DIR/.." && rm -rf "./$(basename "$PROBE_DIR")")
      ;;
    *) echo "error: unexpected probe directory $PROBE_DIR" >&2; return 1 ;;
  esac
}

probe_save_logs() {
  [[ -n "${CCRESDOC_EVIDENCE_DIR:-}" ]] || return 0
  local evidence file
  evidence="$(mktemp -d "$CCRESDOC_EVIDENCE_DIR/$1.XXXXXX")" || return 1
  for file in "${HOST_LOG:-}" "$PROBE_DIR/open.log" "$PROBE_DIR/process-samples.log" "$PROBE_DIR/node-invocations.log"; do
    if [[ -f "$file" ]]; then cp "$file" "$evidence/" || return 1; fi
  done
  echo "Probe logs: $evidence"
}
