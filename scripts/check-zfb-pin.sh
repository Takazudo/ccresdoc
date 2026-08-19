#!/usr/bin/env bash
# Validate the standalone app's first-party dependency contract.
#
# This is deliberately safe to run before installation. It checks the manifest,
# workspace policy, lockfile importers, and peer suffixes. The installed-tree
# check is enabled by `pnpm -C app run validate:dependencies` after a frozen
# install.
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
exec node "$ROOT_DIR/scripts/validate-dependencies.mjs"
