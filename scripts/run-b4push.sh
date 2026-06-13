#!/usr/bin/env bash
set -euo pipefail

# Before-push comprehensive check script for CCResDoc.
# Runs: cargo fmt --check, cargo clippy, cargo test, zfb build (app/)
# All steps run even if one fails; summary at end.
# Invocation: bash scripts/run-b4push.sh
#
# Node is used only by pnpm install (Step 4a). The zfb build itself is
# node-free: it invokes the native @takazudo/zfb-<platform>/zfb binary
# via pnpm exec, not the .bin/zfb Node-shebang wrapper.

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

# ── Step 4: pnpm install + zfb build (app/) ──────
step "Step 4/4: pnpm install + zfb build (app/)"

# 4a: ensure node_modules (including native zfb binary) are present.
# Node is only needed here at setup time; zfb dev/build is node-free at runtime.
INSTALL_OK=0
if (cd "$ROOT_DIR/app" && pnpm install); then
  pass "pnpm install (app/) passed"
  INSTALL_OK=1
else
  fail "pnpm install (app/)"
fi

# 4b: invoke zfb build via pnpm exec so the native @takazudo/zfb-<platform>/zfb
# binary is used — no global zfb on PATH required.
# Skip if pnpm install failed: node_modules may be incomplete, causing misleading errors.
if [ "$INSTALL_OK" -eq 1 ]; then
  if (cd "$ROOT_DIR/app" && pnpm exec zfb build); then
    pass "zfb build (app/) passed"
  else
    fail "zfb build (app/)"
  fi
else
  echo "⏭ skipping zfb build (pnpm install failed)"
  FAILURES+=("zfb build (app/) — skipped: pnpm install failed")
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
