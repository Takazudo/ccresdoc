#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
FIXTURE_DIR="$ROOT_DIR/compatibility/node-free-latest"

pnpm --dir "$FIXTURE_DIR" install --frozen-lockfile
pnpm --dir "$FIXTURE_DIR" run assert:packages
pnpm --dir "$FIXTURE_DIR" run assert:configs
pnpm --dir "$FIXTURE_DIR" run probe:matrix
pnpm --dir "$FIXTURE_DIR" run check
pnpm --dir "$FIXTURE_DIR" run build
pnpm --dir "$FIXTURE_DIR" run probe:runtime
