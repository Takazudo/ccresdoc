---
name: b4push
description: >-
  Run comprehensive pre-push validation covering Rust formatting, Clippy lints, Rust tests, and
  the zfb app build. Use when: (1) Completing a PR or feature implementation, (2) Before pushing
  significant changes, (3) After large refactors or multi-file edits, (4) User says 'b4push',
  'before push', 'check everything', 'run all checks', or 'ready to push'.
user-invocable: true
allowed-tools:
  - Bash
---

# Before Push Check

Run `pnpm b4push` from the project root. This executes `scripts/run-b4push.sh` which runs:

1. `cargo fmt --check` — Rust formatting check across all crates
2. `cargo clippy --workspace --all-targets` — Rust lints across workspace
3. `cargo test --workspace` — Rust unit and integration tests
4. `pnpm --filter app build` — zfb frontend build (requires zfb binary in PATH + env vars)

Takes ~2-4 minutes depending on whether cargo cache is warm. All steps must pass.

## On failure

1. Read the failure output to identify which step failed
2. Auto-fix what you can:
   - Formatting: `cargo fmt`
   - Clippy: address the lint warnings manually
   - Tests: investigate failing test output
   - App build: ensure `zfb` is installed (`cargo install --path $HOME/repos/myoss/zfb/crates/zfb`)
3. Re-run `pnpm b4push` to confirm all checks pass
4. Report the final status
