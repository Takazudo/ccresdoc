#!/usr/bin/env bash
set -euo pipefail

# Before-push comprehensive check script for CCResDoc.
# Runs: cargo fmt --check, cargo clippy, cargo test, pnpm --filter app build
# All steps run even if one fails; summary at end.

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

# ── Step 4: pnpm --filter app build ─────────────
step "Step 4/4: pnpm --filter app build"
if (cd "$ROOT_DIR" && pnpm --filter app build); then
  pass "pnpm --filter app build passed"
else
  fail "pnpm --filter app build"
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
