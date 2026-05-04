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
- `zfb` Rust binary in PATH: `cargo install --path $HOME/repos/myoss/zfb/crates/zfb` (one-time)
- `pnpm install` resolves the cross-repo `link:` deps (zfb at `$HOME/repos/myoss/zfb`)
- The user is in the `admin` group (default on personal Macs) — `/Applications/` write needs no sudo

## Workflow

### Step 1: Clean

Cargo can cache stale `Resources/` artifacts when `app/dist/` changes. Always clean the Tauri crate first:

```bash
cargo clean -p ccresdoc
```

### Step 2: Build app/ (zfb static shell) + .app bundle

`cargo tauri build` runs `pnpm --filter app build` automatically (via `beforeBuildCommand`). Produces both `.app` and `.dmg` bundles under `target/release/bundle/`.

```bash
cargo tauri build
```

Build time on a warm cache: ~1.5 min Rust + ~10 s bundling. Cold cache: ~3-5 min total.

### Step 3: Verify bundle freshness

Check that the bundled shell has both runtime sentinels (which means zfb's build output was actually included):

```bash
SHELL=target/release/bundle/macos/CCResDoc.app/Contents/Resources/_up_/app/dist/_shell/index.html
[ -f "$SHELL" ] || { echo "FAIL: bundled shell missing"; exit 1; }
grep -c '☃CCRESDOC_TITLE_SLOT☃' "$SHELL"     # must be 1
grep -c '☃CCRESDOC_CONTENT_SLOT☃' "$SHELL"   # must be 1
```

Note: Tauri encodes the `bundle.resources` parent-dir traversal (`../app/dist/**/*`) as the `_up_/` prefix inside `Contents/Resources/`. `src-tauri/src/main.rs` joins `_up_` at runtime to find the real dist.

If verification fails, the build is stale or zfb didn't run. Inspect:

```bash
pnpm --filter app build   # re-run zfb manually
```

Then go back to Step 1.

### Step 4: Kill running instance + free port 4892

The embedded axum server binds `127.0.0.1:4892`. Old instances must be killed before reinstall, otherwise the new instance will fail `wait_for_ready` polling.

```bash
killall CCResDoc 2>/dev/null
lsof -ti :4892 2>/dev/null | xargs -r kill 2>/dev/null
sleep 1
lsof -i :4892 2>&1 | head -3   # should be empty
```

### Step 5: Move old, copy fresh

**CRITICAL:** `cp -rf` does NOT reliably update macOS `.app` bundles in-place. Always move the old bundle aside first.

```bash
mv /Applications/CCResDoc.app /tmp/CCResDoc-old-$$.app 2>/dev/null
cp -R target/release/bundle/macos/CCResDoc.app /Applications/CCResDoc.app
```

### Step 6: Verify binary timestamp

```bash
stat -f "%Sm" /Applications/CCResDoc.app/Contents/MacOS/CCResDoc
```

Timestamp must be from just now. If older, the copy failed — surface it loudly.

### Step 7: Launch

```bash
open /Applications/CCResDoc.app
```

Optionally probe `/___ready` to confirm the server bound:

```bash
sleep 2
curl -s -o /dev/null -w "ready: HTTP %{http_code}\n" http://localhost:4892/___ready
```

Report:

- Bundle size (`du -sh /Applications/CCResDoc.app`)
- Binary timestamp
- `/___ready` response
- Anything unexpected
