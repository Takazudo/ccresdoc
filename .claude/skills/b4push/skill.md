---
name: b4push
description: >-
  Run the repository's ten-step pre-push validation: frozen install, frontend and Settings tests,
  Rust fmt/clippy/tests, native zfb build, runtime and compatibility probes, and actual-key
  Chromium browser navigation. Use when: (1) Completing a PR or feature implementation, (2) Before pushing
  significant changes, (3) After large refactors or multi-file edits, (4) User says 'b4push',
  'before push', 'check everything', 'run all checks', or 'ready to push'.
user-invocable: true
allowed-tools:
  - Bash
---

# Before Push Check

Run `pnpm b4push` from the project root. This executes the repository's actual
`scripts/run-b4push.sh` sequence:

1. zfb pin/lock consistency;
2. frozen `app/` install and installed-tree dependency validation;
3. app TypeScript, `zfb check`, Vitest, and the Settings Node tests
   (`pnpm --dir app run test:settings`);
4. `cargo fmt --check`;
5. `cargo clippy --workspace --exclude ccresdoc --all-targets -- -D warnings`;
6. `cargo test --workspace --exclude ccresdoc` (the Linux webkit boundary);
7. native `zfb build` from the published package-root carrier;
8. staged runtime digest/verification plus the serialized two-launch/HMR
   node-sentinel probe;
9. the frozen compatibility evidence check, package/config assertions, check,
   and build; and
10. actual-key Chromium browser navigation (`pnpm run test:browser-navigation`).

Node and pnpm are setup/build tools. The runtime checks resolve
`node_modules/@takazudo/zfb-<platform>/zfb` directly, never the
`node_modules/.bin/zfb` Node wrapper.

Step 10 needs the pinned Playwright Chromium installed locally. Install it once
with `pnpm --dir app exec playwright install chromium`; without it, step 10
fails.

b4push does not run the focused theme-pack validation or its standalone Node
test; run them first:

```sh
pnpm --dir app run validate:theme-packs
pnpm --dir app run test:theme-packs
pnpm b4push
```

All ten b4push steps must pass. b4push excludes the `ccresdoc` Tauri crate
from clippy/test on every host, macOS included. It does not run the mandatory
macOS-arm64 packaged app/WebView gate (`scripts/test-macos-package.sh`) or any
native/manual acceptance. Run those separately on that host and report them
separately.

## On failure

1. Read the failure output to identify which step failed
2. Auto-fix what you can:
   - Formatting: `cargo fmt`
   - Clippy: address the lint warnings manually
   - Tests: investigate failing test output
   - Browser navigation: install Chromium if the step reports it missing
   - App build: confirm the frozen app install completed and rerun the native
     package-root build (`pnpm --dir app exec zfb build`)
3. Re-run `pnpm b4push` to confirm all checks pass
4. Report the final status

Heavy steps (Vitest, Clippy, Rust tests, zfb build, compatibility fixture build,
browser navigation) queue through the machine-wide heavy-guard and print a
`heavy-guard: verdict=PASS|FAIL|ENV_SUSPECT` line. Exit 75 means the queue timed
out and the step never ran. On `ENV_SUSPECT`, rerun once; if it is still red with
no assertion / type / lint error, defer that step to CI under a
`deferred-verification` issue and report it as deferred, never as passed.
