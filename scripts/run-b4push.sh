#!/usr/bin/env bash
set -euo pipefail

# Before-push comprehensive check script for CCResDoc.
# Runs: dependency pin/lock checks, frozen install, strict frontend checks,
# cargo fmt/clippy/test, and the native zfb build (app/).
# All steps run even if one fails; summary at end.
# Invocation: bash scripts/run-b4push.sh
#
# Node is used only by pnpm install (Step 2). The zfb build itself is
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
step "Step 1/8: zfb pin/lock consistency (check-zfb-pin.sh)"
if bash "$ROOT_DIR/scripts/check-zfb-pin.sh"; then
  pass "zfb pin check passed"
else
  fail "zfb pin drift — all @takazudo/zfb* entries in app/package.json must share one version"
fi

# ── Step 2: frozen frontend install + installed-tree validation ─────────────
step "Step 2/8: frozen frontend install + dependency validation"
INSTALL_OK=0
if (cd "$ROOT_DIR/app" && pnpm install --frozen-lockfile); then
  pass "pnpm install --frozen-lockfile (app/) passed"
  INSTALL_OK=1
else
  fail "pnpm install --frozen-lockfile (app/)"
fi

if [ "$INSTALL_OK" -eq 1 ]; then
  if (cd "$ROOT_DIR/app" && pnpm run validate:dependencies); then
    pass "installed dependency validation passed"
  else
    fail "installed dependency validation"
  fi
fi

# ── Step 3: strict frontend gates ──────────────────────────────────────────
step "Step 3/8: strict TypeScript + zfb check + Vitest"
if [ "$INSTALL_OK" -eq 1 ]; then
  if (cd "$ROOT_DIR/app" && pnpm run typecheck); then pass "strict TypeScript passed"; else fail "strict TypeScript"; fi
  if (cd "$ROOT_DIR/app" && pnpm run check:zfb); then pass "zfb check passed"; else fail "zfb check"; fi
  if (cd "$ROOT_DIR/app" && pnpm run test:run); then pass "frontend tests passed"; else fail "frontend tests"; fi
else
  echo "⏭ skipping frontend gates (frozen install failed)"
  FAILURES+=("frontend gates — skipped: frozen install failed")
fi

# ── Step 4: cargo fmt --check ────────────────────
step "Step 4/8: cargo fmt --check"
if (cd "$ROOT_DIR" && cargo fmt --check); then
  pass "cargo fmt passed"
else
  fail "cargo fmt --check (run: cargo fmt)"
fi

# ── Step 5: cargo clippy ─────────────────────────
# --exclude ccresdoc mirrors CI: tauri crate needs webkit2gtk/gtk3, unavailable on Linux CI runners
step "Step 5/8: cargo clippy --workspace --exclude ccresdoc --all-targets -- -D warnings"
if (cd "$ROOT_DIR" && cargo clippy --workspace --exclude ccresdoc --all-targets -- -D warnings); then
  pass "cargo clippy passed"
else
  fail "cargo clippy --workspace --exclude ccresdoc --all-targets -- -D warnings"
fi

# ── Step 6: cargo test ───────────────────────────
# --exclude ccresdoc mirrors CI: tauri crate needs webkit2gtk/gtk3, unavailable on Linux CI runners
step "Step 6/8: cargo test --workspace --exclude ccresdoc"
if (cd "$ROOT_DIR" && cargo test --workspace --exclude ccresdoc); then
  pass "cargo test passed"
else
  fail "cargo test --workspace --exclude ccresdoc"
fi

# ── Step 7: native zfb build (app/) ──────────────
step "Step 7/8: native zfb build (app/)"

# Invoke zfb build via pnpm exec so the native @takazudo/zfb-<platform>/zfb
# binary is used — no global zfb on PATH required.
# Skip if pnpm install failed: node_modules may be incomplete, causing misleading errors.
if [ "$INSTALL_OK" -eq 1 ]; then
  if (cd "$ROOT_DIR/app" && pnpm exec zfb build); then
    pass "zfb build (app/) passed"
  else
    fail "zfb build (app/)"
  fi
else
  echo "⏭ skipping zfb build (frozen install failed)"
  FAILURES+=("zfb build (app/) — skipped: frozen install failed")
fi

# ── Step 8: summary ──────────────────────────────

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
  for f in "${FAILURES[@]:-}"; do
    echo "   - $f"
  done
  exit 1
fi
