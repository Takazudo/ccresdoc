#!/usr/bin/env bash
set -euo pipefail

# Before-push comprehensive check script for CCResDoc.
# Runs: zfb pin check, cargo fmt --check, cargo clippy, cargo test, zfb build (app/)
# All steps run even if one fails; summary at end.
# Invocation: bash scripts/run-b4push.sh
#
# Node is used only by pnpm install (Step 5a). The zfb build itself is
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

# ── Step 1: zfb pin consistency ──────────────────
step "Step 1/5: zfb pin consistency (check-zfb-pin.sh)"
if bash "$ROOT_DIR/scripts/check-zfb-pin.sh"; then
  pass "zfb pin check passed"
else
  fail "zfb pin drift — all @takazudo/zfb* entries in app/package.json must share one version"
fi

# ── Step 2: cargo fmt --check ────────────────────
step "Step 2/5: cargo fmt --check"
if (cd "$ROOT_DIR" && cargo fmt --check); then
  pass "cargo fmt passed"
else
  fail "cargo fmt --check (run: cargo fmt)"
fi

# ── Step 3: cargo clippy ─────────────────────────
# --exclude ccresdoc mirrors CI: tauri crate needs webkit2gtk/gtk3, unavailable on Linux CI runners
step "Step 3/5: cargo clippy --workspace --exclude ccresdoc --all-targets -- -D warnings"
if (cd "$ROOT_DIR" && cargo clippy --workspace --exclude ccresdoc --all-targets -- -D warnings); then
  pass "cargo clippy passed"
else
  fail "cargo clippy --workspace --exclude ccresdoc --all-targets -- -D warnings"
fi

# ── Step 4: cargo test ───────────────────────────
# --exclude ccresdoc mirrors CI: tauri crate needs webkit2gtk/gtk3, unavailable on Linux CI runners
step "Step 4/5: cargo test --workspace --exclude ccresdoc"
if (cd "$ROOT_DIR" && cargo test --workspace --exclude ccresdoc); then
  pass "cargo test passed"
else
  fail "cargo test --workspace --exclude ccresdoc"
fi

# ── Step 5: pnpm install + zfb build (app/) ──────
step "Step 5/5: pnpm install + zfb build (app/)"

# 5a: ensure node_modules (including native zfb binary) are present.
# Node is only needed here at setup time; zfb dev/build is node-free at runtime.
INSTALL_OK=0
if (cd "$ROOT_DIR/app" && pnpm install); then
  pass "pnpm install (app/) passed"
  INSTALL_OK=1
else
  fail "pnpm install (app/)"
fi

# 5b: invoke zfb build via pnpm exec so the native @takazudo/zfb-<platform>/zfb
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

if [ ${#FAILURES[@]:-} -eq 0 ]; then
  echo "✅ All checks passed! Safe to push."
  exit 0
else
  echo "❌ ${#FAILURES[@]} check(s) failed:"
  for f in "${FAILURES[@]:-}"; do
    echo "   - $f"
  done
  exit 1
fi
