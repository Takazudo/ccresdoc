#!/usr/bin/env bash
set -euo pipefail

# Before-push comprehensive check script for CCResDoc.
# Runs: cargo fmt --check, cargo clippy, cargo test, zfb build (app/)
# All steps run even if one fails; summary at end.
# Invocation: bash scripts/run-b4push.sh  (no pnpm / Node required)

START_TIME=$(date +%s)
FAILURES=()

step() {
  echo ""
  echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
  echo "▶ $1"
  echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
}

pass() {
  echo "✅ $1"
}

fail() {
  echo "❌ $1"
  FAILURES+=("$1")
}

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"

# ── Step 1: cargo fmt --check ────────────────────
step "Step 1/4: cargo fmt --check"
if (cd "$ROOT_DIR" && cargo fmt --check); then
  pass "cargo fmt passed"
else
  fail "cargo fmt --check (run: cargo fmt)"
fi

# ── Step 2: cargo clippy ─────────────────────────
step "Step 2/4: cargo clippy --workspace --all-targets"
if (cd "$ROOT_DIR" && cargo clippy --workspace --all-targets); then
  pass "cargo clippy passed"
else
  fail "cargo clippy --workspace --all-targets"
fi

# ── Step 3: cargo test ───────────────────────────
step "Step 3/4: cargo test --workspace"
if (cd "$ROOT_DIR" && cargo test --workspace); then
  pass "cargo test passed"
else
  fail "cargo test --workspace"
fi

# ── Step 4: zfb build (app/) ─────────────────────
# zfb's TS config loader and CSS engine resolve esbuild + tailwindcss-v4 from
# fixed staged-slot paths or env vars (not from the binary's embedded snapshot).
# Until upstream zfb extracts these via include_dir at runtime, point at the
# zfb source-tree binaries via env vars. ZFB_HOME defaults to the standard
# myoss layout but can be overridden in the developer's shell.
ZFB_HOME="${ZFB_HOME:-$HOME/repos/myoss/zfb}"
step "Step 4/4: zfb build (app/)"
if (cd "$ROOT_DIR/app" \
      && ZFB_ESBUILD_BIN="$ZFB_HOME/crates/zfb/binaries/esbuild/esbuild" \
         ZFB_TAILWIND_BIN="$ZFB_HOME/crates/zfb/binaries/tailwindcss-v4" \
         zfb build); then
  pass "zfb build passed"
else
  fail "zfb build (app/)"
fi

# ── Summary ─────────────────────────────────────
END_TIME=$(date +%s)
DURATION=$((END_TIME - START_TIME))

echo ""
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "  SUMMARY (${DURATION}s)"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"

if [ ${#FAILURES[@]} -eq 0 ]; then
  echo "✅ All checks passed! Safe to push."
  exit 0
else
  echo "❌ ${#FAILURES[@]} check(s) failed:"
  for f in "${FAILURES[@]}"; do
    echo "   - $f"
  done
  exit 1
fi
