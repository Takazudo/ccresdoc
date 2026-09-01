#!/usr/bin/env bash
set -euo pipefail

# Before-push comprehensive check script for CCResDoc.
# Runs: dependency pin/lock checks, frozen install, strict frontend checks,
# cargo fmt/clippy/test, native zfb build, pruned runtime lifecycle, and the
# frozen zero-plugin compatibility fixture, plus actual-key Chromium browser
# navigation confirmation. Install Chromium first with:
#   pnpm --dir app exec playwright install chromium
# All steps run even if one fails; summary at end.
# Invocation: bash scripts/run-b4push.sh
#
# Node is development/build tooling for package validation and probes. The
# runtime probes put a failing Node sentinel first on PATH and invoke the
# native @takazudo/zfb-<platform>/zfb binary directly.

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
step "Step 1/10: zfb pin/lock consistency (check-zfb-pin.sh)"
if bash "$ROOT_DIR/scripts/check-zfb-pin.sh"; then
  pass "zfb pin check passed"
else
  fail "zfb pin drift — all @takazudo/zfb* entries in app/package.json must share one version"
fi

# ── Step 2: frozen frontend install + installed-tree validation ─────────────
step "Step 2/10: frozen frontend install + dependency validation"
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
step "Step 3/10: strict TypeScript + zfb check + Vitest"
if [ "$INSTALL_OK" -eq 1 ]; then
  if (cd "$ROOT_DIR/app" && pnpm run typecheck); then pass "strict TypeScript passed"; else fail "strict TypeScript"; fi
  if (cd "$ROOT_DIR/app" && pnpm run check:zfb); then pass "zfb check passed"; else fail "zfb check"; fi
  if (cd "$ROOT_DIR/app" && pnpm run test:run); then pass "frontend tests passed"; else fail "frontend tests"; fi
  if (cd "$ROOT_DIR/app" && pnpm run test:settings); then pass "Settings Node tests passed"; else fail "Settings Node tests"; fi
else
  echo "⏭ skipping frontend gates (frozen install failed)"
  FAILURES+=("frontend gates — skipped: frozen install failed")
fi

# ── Step 4: cargo fmt --check ────────────────────
step "Step 4/10: cargo fmt --check"
if (cd "$ROOT_DIR" && cargo fmt --check); then
  pass "cargo fmt passed"
else
  fail "cargo fmt --check (run: cargo fmt)"
fi

# ── Step 5: cargo clippy ─────────────────────────
# --exclude ccresdoc mirrors CI: tauri crate needs webkit2gtk/gtk3, unavailable on Linux CI runners
step "Step 5/10: cargo clippy --workspace --exclude ccresdoc --all-targets -- -D warnings"
if (cd "$ROOT_DIR" && cargo clippy --workspace --exclude ccresdoc --all-targets -- -D warnings); then
  pass "cargo clippy passed"
else
  fail "cargo clippy --workspace --exclude ccresdoc --all-targets -- -D warnings"
fi

# ── Step 6: cargo test ───────────────────────────
# --exclude ccresdoc mirrors CI: tauri crate needs webkit2gtk/gtk3, unavailable on Linux CI runners
step "Step 6/10: cargo test --workspace --exclude ccresdoc"
if (cd "$ROOT_DIR" && cargo test --workspace --exclude ccresdoc); then
  pass "cargo test passed"
else
  fail "cargo test --workspace --exclude ccresdoc"
fi

# ── Step 7: native zfb build (app/) ──────────────
step "Step 7/10: native zfb build (app/)"

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

# ── Step 8: staged runtime lifecycle ─────────────
step "Step 8/10: pruned runtime workspace + node-free lifecycle"
if [ "$INSTALL_OK" -eq 1 ]; then
  if (cd "$ROOT_DIR" && pnpm run probe:runtime-package); then
    pass "pruned runtime lifecycle passed"
  else
    fail "pruned runtime lifecycle"
  fi
else
  echo "⏭ skipping runtime lifecycle (frozen install failed)"
  FAILURES+=("pruned runtime lifecycle — skipped: frozen install failed")
fi

# ── Step 9: frozen compatibility fixture ─────────
step "Step 9/10: frozen zero-plugin compatibility fixture"
if (cd "$ROOT_DIR" && pnpm run check:compatibility); then
  pass "zero-plugin compatibility fixture passed"
else
  fail "zero-plugin compatibility fixture"
fi

# ── Step 10: actual-key browser navigation ──────────────────────────────────
step "Step 10/10: actual-key Chromium browser navigation"
if [ "$INSTALL_OK" -eq 1 ]; then
  if (cd "$ROOT_DIR" && pnpm run test:browser-navigation); then
    pass "actual-key Chromium browser navigation passed"
  else
    fail "actual-key Chromium browser navigation (install Chromium first if unavailable)"
  fi
else
  echo "⏭ skipping browser navigation (frozen install failed)"
  FAILURES+=("actual-key Chromium browser navigation — skipped: frozen install failed")
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
  for f in "${FAILURES[@]:-}"; do
    echo "   - $f"
  done
  exit 1
fi
