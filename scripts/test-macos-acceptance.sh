#!/usr/bin/env bash
set -euo pipefail
if (( $# != 2 )) || [[ "$1" != /* || "$2" != /* ]]; then
  echo "Usage: bash scripts/test-macos-acceptance.sh /absolute/CCResDoc.app /absolute/new-evidence-directory" >&2
  exit 2
fi
if [[ "$(uname -s)" != Darwin || "$(uname -m)" != arm64 ]]; then
  echo "SKIP native acceptance: requires macOS arm64" >&2
  exit 77
fi
REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd -P)"
cd "$REPO_ROOT"
source "$REPO_ROOT/scripts/lib/macos-probe.sh"
source "$REPO_ROOT/scripts/lib/macos-evidence.sh"
probe_host_version
APP_PATH="$1"
REVISION="$(git rev-parse HEAD)"
IDENTITY_SCRIPT="$REPO_ROOT/scripts/macos-bundle-identity.mjs"
export CCRESDOC_EVIDENCE_DIR="$2"
mkdir "$CCRESDOC_EVIDENCE_DIR"
echo "Native acceptance evidence: $CCRESDOC_EVIDENCE_DIR"
{
  sw_vers
  uname -m
  node --version
  pnpm --version
  rustc --version
  cargo tauri --version
  git status --short
} > "$CCRESDOC_EVIDENCE_DIR/host.log" 2>&1
node "$IDENTITY_SCRIPT" "$APP_PATH" "$REVISION" > "$CCRESDOC_EVIDENCE_DIR/bundle.identity.json"

run_evidenced_check package bash scripts/test-macos-package.sh --existing-bundle "$APP_PATH"
run_evidenced_check settings env "CCRESDOC_SETTINGS_APP=$APP_PATH" pnpm run test:macos-settings
run_evidenced_check launch bash scripts/test-launch.sh --bundle "$APP_PATH" --controls
