---
name: ccresdoc-build
description: "Build, verify, and install CCResDoc.app locally for testing. Use when: (1) User says 'build ccresdoc', 'rebuild ccresdoc', 'install ccresdoc', 'ccresdoc-build', (2) User wants a fresh CCResDoc.app after Rust, app/, or src-tauri changes, (3) After Rust or zfb-output changes that need verification in the bundled .app."
---

# Local CCResDoc Build & Install

Clean rebuild, verify fresh assets, kill any running instance, install to `/Applications/`, launch.

## Arguments

None. Always builds the single Tauri target defined in `src-tauri/tauri.conf.json`.

## Prerequisites

- macOS arm64
- Rust stable toolchain + Tauri CLI (`cargo install tauri-cli` or `cargo binstall tauri-cli`)
- `pnpm` (Node at setup time only — **not** required at runtime)
- The user is in the `admin` group (default on personal Macs) — `/Applications/` write needs no sudo

## Architecture recap

`CCResDoc.app` is a thin Tauri host with **node-free runtime**:
- The bundled `app/` tree (under `Contents/Resources/_up_/app/`) carries a pre-installed
  `node_modules` that includes the native `@takazudo/zfb-<platform>/zfb` binary.
- At launch the Tauri host resolves a **writable workspace**: dev = the repo `app/`
  (already has `node_modules`); bundled = a versioned copy placed at
  `<app_data_dir>/app-workspace/`.
- The host spawns the in-process Rust watcher (`ccresdoc-claude-md`: walks `~/.claude/`
  → writes MDX), then spawns `node_modules/@takazudo/zfb-<platform>/zfb dev --port 4892`
  (the native binary, NOT the `.bin/zfb` Node-shebang wrapper), polls `GET /` on
  `http://localhost:4892/`, and navigates the WebView once ready.

## Workflow

### Step 1: Ensure app/ deps are installed

`cargo tauri build`'s `beforeBuildCommand` runs this automatically, but a manual
pre-check speeds up debugging if `node_modules` is stale:

```bash
cd app && pnpm install
```

This populates `node_modules` including the native `@takazudo/zfb-darwin-arm64/zfb` binary.
Node is only needed here — not at runtime.

### Step 2: Clean

Cargo can cache stale `Resources/` artifacts when `app/dist/` changes. Always clean the Tauri crate first:

```bash
cargo clean -p ccresdoc
```

### Step 3: Build app/ (zfb static shell) + .app bundle

`cargo tauri build` runs `cd ../app && pnpm install && pnpm exec zfb build` automatically
(via `beforeBuildCommand` in `src-tauri/tauri.conf.json`). This invokes the native zfb
binary through pnpm — no global `zfb` on PATH required. Produces both `.app` and `.dmg`
bundles under `target/release/bundle/`.

```bash
cargo tauri build
```

Build time on a warm cache: ~1.5 min Rust + ~10 s bundling. Cold cache: ~3-5 min total.

### Step 4: Verify bundle freshness

Check that the bundled `app/dist/` exists and `node_modules` includes the native binary:

```bash
BUNDLE=target/release/bundle/macos/CCResDoc.app/Contents/Resources/_up_/app
[ -d "$BUNDLE/dist" ] || { echo "FAIL: bundled dist/ missing"; exit 1; }
# Verify any @takazudo/zfb-* native binary exists in the bundled node_modules.
# On the primary build host (macOS arm64) this is @takazudo/zfb-darwin-arm64.
ls "$BUNDLE/node_modules/@takazudo"/zfb-*/zfb 2>/dev/null | grep -q . || \
  { echo "FAIL: no native zfb binary found in bundled node_modules/@takazudo/"; exit 1; }
echo "Bundle looks good: $(du -sh $BUNDLE/dist | cut -f1) dist"
```

If verification fails, the build is stale or `pnpm install` did not run. Inspect:

```bash
cd app && pnpm install && pnpm exec zfb build   # re-run manually
```

Then go back to Step 2.

### Step 5: Kill running instance + free port 4892

`zfb dev` binds `127.0.0.1:4892`. Old instances must be killed before reinstall.

```bash
killall CCResDoc 2>/dev/null
lsof -ti :4892 2>/dev/null | xargs -r kill 2>/dev/null
sleep 1
lsof -i :4892 2>&1 | head -3   # should be empty
```

### Step 6: Move old, copy fresh

**CRITICAL:** `cp -rf` does NOT reliably update macOS `.app` bundles in-place. Always move the old bundle aside first.

```bash
mv /Applications/CCResDoc.app /tmp/CCResDoc-old-$$.app 2>/dev/null
cp -R target/release/bundle/macos/CCResDoc.app /Applications/CCResDoc.app
xattr -dr com.apple.quarantine /Applications/CCResDoc.app
```

### Step 7: Verify binary timestamp

```bash
stat -f "%Sm" /Applications/CCResDoc.app/Contents/MacOS/CCResDoc
```

Timestamp must be from just now. If older, the copy failed — surface it loudly.

### Step 8: Launch

```bash
open /Applications/CCResDoc.app
```

Poll `GET /` to confirm `zfb dev` bound and the doc site is up:

```bash
sleep 5
curl -s -o /dev/null -w "ready: HTTP %{http_code}\n" http://localhost:4892/
```

Report:

- Bundle size (`du -sh /Applications/CCResDoc.app`)
- Binary timestamp
- `GET /` HTTP response (expect 200)
- Anything unexpected
