#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
FIXTURE_DIR="$ROOT_DIR/compatibility/node-free-latest"
EVIDENCE_MODE="${1:---check}"

case "$EVIDENCE_MODE" in
  --check) EVIDENCE_SCRIPT="evidence:check" ;;
  --refresh) EVIDENCE_SCRIPT="evidence:refresh" ;;
  *)
    echo "usage: $0 [--check|--refresh]" >&2
    exit 2
    ;;
esac

pnpm --dir "$FIXTURE_DIR" install --frozen-lockfile
pnpm --dir "$FIXTURE_DIR" run "$EVIDENCE_SCRIPT"
pnpm --dir "$FIXTURE_DIR" run assert:packages
pnpm --dir "$FIXTURE_DIR" run assert:configs
pnpm --dir "$FIXTURE_DIR" run check
pnpm --dir "$FIXTURE_DIR" run build
