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

Run `pnpm b4push` from the project root. This executes the repository's actual
`scripts/run-b4push.sh` sequence:

1. zfb pin consistency;
2. frozen `app/` install and installed-tree dependency validation;
3. app TypeScript, `zfb check`, and Vitest;
4. `cargo fmt --check`;
5. `cargo clippy --workspace --exclude ccresdoc --all-targets -- -D warnings`;
6. `cargo test --workspace --exclude ccresdoc` (the Linux webkit boundary);
7. native `zfb build` from the published package-root carrier;
8. staged runtime digest/verification plus the serialized two-launch/HMR
   node-sentinel probe; and
9. the frozen compatibility evidence check, package/config assertions, check,
   and build.

Node and pnpm are setup/build tools. The runtime checks resolve
`node_modules/@takazudo/zfb-<platform>/zfb` directly, never the
`node_modules/.bin/zfb` Node wrapper. Run the focused theme-pack validation and
standalone Node test before b4push because they are separate from its Vitest
step:

```sh
pnpm --dir app run validate:theme-packs
pnpm --dir app run test:theme-packs
pnpm b4push
```

All nine b4push steps must pass. A Linux pass does not cover the mandatory
macOS-arm64 packaged app/WebView gate; run `scripts/test-macos-package.sh` on
that host and report it separately.

## On failure

1. Read the failure output to identify which step failed
2. Auto-fix what you can:
   - Formatting: `cargo fmt`
   - Clippy: address the lint warnings manually
   - Tests: investigate failing test output
   - App build: confirm the frozen app install completed and rerun the native
     package-root build (`pnpm --dir app exec zfb build`)
3. Re-run `pnpm b4push` to confirm all checks pass
4. Report the final status
