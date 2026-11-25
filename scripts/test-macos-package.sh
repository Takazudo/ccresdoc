#!/usr/bin/env bash
set -euo pipefail

if [[ "$(uname -s)" != "Darwin" || "$(uname -m)" != "arm64" ]]; then
  echo "SKIP host gate macos-arm64-packaged-app-webview: requires macOS arm64" >&2
  exit 0
fi

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd -P)"
source "$REPO_ROOT/scripts/lib/macos-probe.sh"
probe_host_version
EXISTING_BUNDLE=""
if (( $# == 2 )) && [[ "$1" == "--existing-bundle" ]]; then
  probe_bundle "$2"
  EXISTING_BUNDLE="$APP_PATH"
elif (( $# != 0 )); then
  echo "Usage: bash scripts/test-macos-package.sh [--existing-bundle <absolute CCResDoc.app path>]" >&2
  exit 2
fi
probe_preflight
PROBE_DIR="$(mktemp -d /tmp/ccresdoc-macos-package.XXXXXX)"
PROBE_DIR="$(cd "$PROBE_DIR" && pwd -P)"
PROBE_HOME="$PROBE_DIR/home"
HOST_LOG="$PROBE_HOME/Library/Application Support/com.takazudo.ccresdoc/ccresdoc.log"
SENTINEL_DIR="$PROBE_DIR/bin"
SENTINEL_LOG="$PROBE_DIR/node-invocations.log"
PROCESS_LOG="$PROBE_DIR/process-samples.log"
CARGO_TARGET_DIR="$PROBE_DIR/cargo-target"
FIXTURE_LABEL="Package Readiness Probe $(basename "$PROBE_DIR")"
FIXTURE_BODY="Generated package route $(basename "$PROBE_DIR")"
FIXTURE_AUTOLINK="https://example.com/package-readiness-probe"
FIXTURE_ROUTE="http://127.0.0.1:4892/docs/claude/skills/package-readiness-probe/"
CONFIG_PATH="$PROBE_DIR/config.toml"
PACKAGE_SOURCE="$PROBE_DIR/package-source"
PACKAGE_FACTS="$REPO_ROOT/compatibility/node-free-latest/evidence/package-facts.json"
DARWIN_PACKAGE=""
DARWIN_RELATIVE_PATH=""
DARWIN_SIZE=""
DARWIN_SHA256=""

cleanup() {
  local status=$?
  trap - EXIT
  trap '' INT TERM
  probe_quit || status=1
  if [[ "$status" != 0 && -f "$HOST_LOG" ]]; then tail -200 "$HOST_LOG" >&2; fi
  probe_save_logs package || status=1
  probe_remove_directory || status=1
  exit "$status"
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM

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

probe_bundle "$APP_PATH"
# A browser may retain a CLOSE_WAIT client socket after a prior run. Only a
# listener would conflict with the app-owned loopback server.
test -z "$(lsof -nP -tiTCP:4892 -sTCP:LISTEN 2>/dev/null || true)"

RUNTIME_ROOT="$APP_PATH/Contents/Resources/runtime-workspace/app"
ZFB_BIN="$RUNTIME_ROOT/$DARWIN_RELATIVE_PATH"

# Audit the final bundle's staged app before any user fixture is introduced.
# This checks the same explicit source/namespace/privacy contract as the Linux
# staged probe and receives only synthetic temporary paths as rejection inputs.
node "$REPO_ROOT/scripts/audit-runtime-workspace.mjs" "$RUNTIME_ROOT" "$PROBE_HOME" "$APP_PATH"

# Create the temporary source and failing Node sentinel only after the final
# bundle has passed its privacy audit. They are launch inputs, never staging
# inputs, and therefore cannot influence the audited package bytes.
mkdir -p "$PACKAGE_SOURCE/skills/package-readiness-probe" "$SENTINEL_DIR"
cat > "$CONFIG_PATH" <<EOF
schema_version = 3

[server]
preferred_port = 4892
fallback_to_free_port = false

[[document_sources]]
id = "package-probe"
label = "Claude"
enabled = true
root = "$PACKAGE_SOURCE"
mount = "claude"

[document_sources.scan_root]
kind = "same"

[document_sources.rules]
kind = "claude_code"
definition_version = 1
EOF
config_hash="$(shasum -a 256 "$CONFIG_PATH")"
printf '%s\n' \
  '---' \
  "name: \"$FIXTURE_LABEL\"" \
  'description: Proves packaged semantic readiness uses generated content.' \
  '---' \
  '' \
  "$FIXTURE_BODY" \
  '' \
  "[Source]($FIXTURE_AUTOLINK)" \
  '' \
  '| Result |' \
  '| --- |' \
  '| before<br>after |' > "$PACKAGE_SOURCE/skills/package-readiness-probe/SKILL.md"
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
  probe_launch --env "PATH=$SENTINEL_DIR:$PATH"
  READY=0
  for _ in $(seq 1 300); do
    probe_host_alive || probe_find_host || true
    if ! probe_capture_sidecar; then
      probe_live "$APP_OPEN_PID" || break
      sleep 1
      continue
    fi
    ps -ww -axo pid=,ppid=,pgid=,args= |
      awk -v host="$APP_PID" -v group="$SIDECAR_PID" '$1 == host || $2 == host || $3 == group' >> "$PROCESS_LOG"
    if grep -q "plugin-host.mjs" "$PROCESS_LOG"; then
      echo "plugin host observed during packaged launch" >&2
      exit 1
    fi
    if [[ "$(lsof -nP -tiTCP:4892 -sTCP:LISTEN 2>/dev/null | sort -u)" == "$SIDECAR_PID" ]] \
      && [[ "$(curl --noproxy '*' --max-time 3 -s -o "$PROBE_DIR/root.html" -w '%{http_code}' http://127.0.0.1:4892/ || true)" = "200" ]] \
      && grep -Fq "CCResDoc Resources" "$PROBE_DIR/root.html" \
      && grep -Fq "$FIXTURE_LABEL" "$PROBE_DIR/root.html" \
      && ! grep -Fq "data-home-page" "$PROBE_DIR/root.html" \
      && grep -Fq "Claude" "$PROBE_DIR/root.html" \
      && grep -Fq "Document sources" "$PROBE_DIR/root.html" \
      && grep -Fq "data-header-logo" "$PROBE_DIR/root.html" \
      && grep -Eq 'href="?/docs/' "$PROBE_DIR/root.html" \
      && [[ "$(curl --noproxy '*' --max-time 3 -s -o "$PROBE_DIR/docs.html" -w '%{http_code}' http://127.0.0.1:4892/docs/ || true)" = "200" ]] \
      && grep -Fq "CCResDoc Resources" "$PROBE_DIR/docs.html" \
      && grep -Fq "$FIXTURE_LABEL" "$PROBE_DIR/docs.html" \
      && grep -Fq "Claude" "$PROBE_DIR/docs.html" \
      && grep -Fq "Document sources" "$PROBE_DIR/docs.html" \
      && [[ "$(curl --noproxy '*' --max-time 3 -s -o "$PROBE_DIR/manager.html" -w '%{http_code}' http://127.0.0.1:4892/docs/ccresdoc/document-sources/ || true)" = "200" ]] \
      && grep -Fq "data-ccresdoc-source-manager" "$PROBE_DIR/manager.html" \
      && [[ "$(curl --noproxy '*' --max-time 3 -s -o "$PROBE_DIR/fixture.html" -w '%{http_code}' "$FIXTURE_ROUTE" || true)" = "200" ]] \
      && grep -Fq "$FIXTURE_LABEL" "$PROBE_DIR/fixture.html" \
      && grep -Fq "$FIXTURE_BODY" "$PROBE_DIR/fixture.html" \
      && grep -Fq "href=\"$FIXTURE_AUTOLINK\"" "$PROBE_DIR/fixture.html" \
      && grep -Fq 'before&lt;br' "$PROBE_DIR/fixture.html" \
      && grep -Fq 'after' "$PROBE_DIR/fixture.html"; then
        READY=1
        break
    fi
    sleep 1
  done
  test "$READY" = "1"
  probe_host_isolated
  grep -Fq "CCResDoc" "$PROBE_DIR/docs.html"
  test ! -s "$SENTINEL_LOG"

  test "$(shasum -a 256 "$CONFIG_PATH")" = "$config_hash"
  test ! -e "$PROBE_HOME/.claude"
  probe_quit
  test -z "$(lsof -nP -tiTCP:4892 -sTCP:LISTEN 2>/dev/null || true)"
done

echo "PASS host gate macos-arm64-packaged-app-webview: packaged workspace, WebView launch, node sentinel, relaunch, and process-group shutdown"
if [[ -z "$EXISTING_BUNDLE" ]]; then
  echo "Temporary bundle (removed on exit): $APP_PATH"
else
  echo "Existing bundle mode: verified without rebuilding: $APP_PATH"
fi
