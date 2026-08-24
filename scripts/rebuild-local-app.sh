#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
APP_DEST="/Applications/CCResDoc.app"
BUNDLE_ID="com.takazudo.ccresdoc"
TMP_BASE="${TMPDIR:-/tmp}"
TMP_BASE="${TMP_BASE%/}"
WORK_DIR=""
BACKUP_PATH=""
INSTALL_STARTED=0
INSTALL_VERIFIED=0

if (( $# != 0 )); then
  echo "Usage: pnpm rebuild:local-app" >&2
  echo "For the fast path, set SKIP_APP_BUILD=1 instead of passing arguments." >&2
  exit 2
fi

app_running() {
  [[ "$(/usr/bin/osascript -e 'application id "com.takazudo.ccresdoc" is running' 2>/dev/null || true)" == "true" ]]
}

cleanup() {
  local status=$?
  trap - EXIT INT TERM

  if [[ "$status" -ne 0 && "$INSTALL_STARTED" == "1" && "$INSTALL_VERIFIED" != "1" ]]; then
    if [[ -d "$APP_DEST" && -n "$WORK_DIR" ]]; then
      mv "$APP_DEST" "$WORK_DIR/CCResDoc.failed.app" 2>/dev/null || true
    fi
    if [[ -n "$BACKUP_PATH" && -d "$BACKUP_PATH" ]]; then
      mv "$BACKUP_PATH" "$APP_DEST" 2>/dev/null || true
      echo "Restored the previous /Applications install after installation failed." >&2
    fi
  fi

  if [[ -n "$WORK_DIR" ]]; then
    case "$WORK_DIR" in
      "$TMP_BASE"/ccresdoc-local-build.*) rm -rf "$WORK_DIR" ;;
      *) echo "Refusing to remove unexpected temporary path: $WORK_DIR" >&2 ;;
    esac
  fi
  exit "$status"
}
trap cleanup EXIT INT TERM

if [[ "$(uname -s)" != "Darwin" ]]; then
  echo "rebuild-local-app.sh: CCResDoc.app installation requires macOS." >&2
  exit 1
fi

WORK_DIR="$(mktemp -d "$TMP_BASE/ccresdoc-local-build.XXXXXX")"
BACKUP_PATH="$WORK_DIR/CCResDoc.previous.app"
PROBE_HTML="$WORK_DIR/docs.html"

HEAD_LINE="$(git -C "$REPO_ROOT" rev-parse --short HEAD 2>/dev/null) $(git -C "$REPO_ROOT" log -1 --format=%s 2>/dev/null)"
echo "git HEAD: $HEAD_LINE"
if [[ -n "$(git -C "$REPO_ROOT" status --porcelain=v1 --untracked-files=normal 2>/dev/null)" ]]; then
  echo "⚠ working tree has uncommitted changes — the build will include them."
fi

if [[ "${SKIP_APP_BUILD:-}" == "1" ]]; then
  echo "==> [1/4] SKIP_APP_BUILD=1 — reuse the existing release bundle"
else
  echo "==> [1/4] Build the release app bundle"
  (cd "$REPO_ROOT" && cargo tauri build --bundles app)
fi

TARGET_DIR="$(
  cd "$REPO_ROOT"
  cargo metadata --manifest-path src-tauri/Cargo.toml --no-deps --format-version 1 |
    node -e 'const fs=require("fs"); process.stdout.write(JSON.parse(fs.readFileSync(0,"utf8")).target_directory)'
)"
APP_SRC="$TARGET_DIR/release/bundle/macos/CCResDoc.app"
RUNTIME_ROOT="$APP_SRC/Contents/Resources/runtime-workspace/app"
TOKEN_FILE="$APP_SRC/Contents/Resources/runtime-workspace/version.txt"
APP_BINARY="$APP_SRC/Contents/MacOS/ccresdoc"

echo "==> [2/4] Verify the release bundle"
[[ -d "$APP_SRC" ]] || { echo "Release bundle not found at $APP_SRC" >&2; exit 1; }
[[ "$(/usr/libexec/PlistBuddy -c 'Print :CFBundleIdentifier' "$APP_SRC/Contents/Info.plist")" == "$BUNDLE_ID" ]] || {
  echo "Unexpected bundle identifier in $APP_SRC" >&2
  exit 1
}
[[ -x "$APP_BINARY" ]] || { echo "Bundled executable is missing or not executable." >&2; exit 1; }
[[ ! -e "$RUNTIME_ROOT/dist" ]] || { echo "Build dist must not be copied into the packaged runtime workspace." >&2; exit 1; }
[[ -s "$TOKEN_FILE" ]] || { echo "Bundled runtime refresh token is missing." >&2; exit 1; }
node "$REPO_ROOT/scripts/audit-runtime-workspace.mjs" "$RUNTIME_ROOT" "$HOME"
ZFB_BIN="$(find "$RUNTIME_ROOT/node_modules/@takazudo" -mindepth 2 -maxdepth 2 -type f -name zfb -print -quit 2>/dev/null || true)"
[[ -n "$ZFB_BIN" && -x "$ZFB_BIN" ]] || { echo "Bundled native zfb carrier is missing or not executable." >&2; exit 1; }
[[ ! -e "$RUNTIME_ROOT/node_modules/.bin/zfb" ]] || { echo "Node wrapper leaked into the packaged runtime." >&2; exit 1; }

echo "==> [3/4] Install the verified bundle at $APP_DEST"
if app_running; then
  /usr/bin/osascript -e 'tell application id "com.takazudo.ccresdoc" to quit' >/dev/null
  for _ in $(seq 1 80); do
    app_running || break
    sleep 0.25
  done
  if app_running; then
    echo "CCResDoc did not quit cleanly; leaving the existing installation untouched." >&2
    exit 1
  fi
fi

if [[ -e "$APP_DEST" && ! -d "$APP_DEST" ]]; then
  echo "$APP_DEST exists but is not an app directory." >&2
  exit 1
fi
INSTALL_STARTED=1
if [[ -d "$APP_DEST" ]]; then
  mv "$APP_DEST" "$BACKUP_PATH"
fi
/usr/bin/ditto "$APP_SRC" "$APP_DEST"
xattr -dr com.apple.quarantine "$APP_DEST" 2>/dev/null || true

SOURCE_SHA="$(shasum -a 256 "$APP_BINARY" | awk '{print $1}')"
INSTALLED_BINARY="$APP_DEST/Contents/MacOS/ccresdoc"
INSTALLED_SHA="$(shasum -a 256 "$INSTALLED_BINARY" | awk '{print $1}')"
[[ "$INSTALLED_SHA" == "$SOURCE_SHA" ]] || { echo "Installed executable does not match the release bundle." >&2; exit 1; }
cmp -s "$TOKEN_FILE" "$APP_DEST/Contents/Resources/runtime-workspace/version.txt" || {
  echo "Installed runtime token does not match the release bundle." >&2
  exit 1
}
INSTALL_VERIFIED=1

LOG_PATH="$HOME/Library/Application Support/$BUNDLE_ID/ccresdoc.log"
LOG_START=0
if [[ -f "$LOG_PATH" ]]; then
  LOG_START="$(wc -l < "$LOG_PATH" | tr -d ' ')"
fi

echo "==> [4/4] Launch the installed app and confirm semantic docs readiness"
open -n "$APP_DEST"
for _ in $(seq 1 80); do
  app_running && break
  sleep 0.25
done
app_running || { echo "The installed app did not launch." >&2; exit 1; }

READY=0
EFFECTIVE_PORT=""
for _ in $(seq 1 300); do
  app_running || break
  if [[ -f "$LOG_PATH" ]]; then
    EFFECTIVE_PORT="$(
      tail -n +$((LOG_START + 1)) "$LOG_PATH" |
        sed -n 's/.*spawn_zfb_dev:.* port=\([0-9][0-9]*\).*/\1/p' |
        tail -n 1
    )"
  fi
  if [[ "$EFFECTIVE_PORT" =~ ^[0-9]+$ ]] &&
    curl -fsS --max-time 2 -o "$PROBE_HTML" "http://127.0.0.1:$EFFECTIVE_PORT/docs/" 2>/dev/null &&
    grep -Fq "CCResDoc Resources" "$PROBE_HTML"; then
    READY=1
    break
  fi
  sleep 1
done

if [[ "$READY" != "1" ]]; then
  echo "Installed app launched but semantic /docs/ readiness was not observed." >&2
  if [[ -f "$LOG_PATH" ]]; then
    tail -n +$((LOG_START + 1)) "$LOG_PATH" | tail -n 80 >&2
  fi
  exit 1
fi

echo ""
echo "PASS local app build: installed executable and runtime match the release bundle"
echo "installed app: $APP_DEST"
echo "installed executable: $(stat -f '%Sm' -t '%Y-%m-%d %H:%M:%S %z' "$INSTALLED_BINARY")"
echo "executable sha256: $INSTALLED_SHA"
echo "runtime token: $(tr -d '\r\n' < "$TOKEN_FILE")"
echo "ready: HTTP 200 http://127.0.0.1:$EFFECTIVE_PORT/docs/"
