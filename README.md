# CCResDoc

A macOS documentation viewer for `$HOME/.claude/` by default — renders CLAUDE.md hierarchies, skills, commands, and agent definitions as a browsable local web app inside a native Tauri window.

The app is a thin Tauri host around a **node-free sidecar architecture**: at launch it spawns the native `zfb` binary on the settings-selected loopback port and a Rust watcher that generates MDX from the configured source (`~/.claude/` by default). No Node.js or external runtime dependencies are required once the `.app` is built.

## Architecture

```
~/.claude/           ← source of truth (CLAUDE.md files, skills/, commands/, agents/)
     │
     ▼  Rust watcher (ccresdoc-claude-md crate, in-process)
app/src/content/docs/claude*/   ← generated MDX (gitignored)
     │
     ▼  zfb dev (native binary, default preferred port 4892, node-free at runtime)
WebView → http://localhost:4892/docs/
```

Key facts:
- **Published toolchain**: the `@takazudo/zfb*` family and all five native
  carrier packages are pinned to `2.10.1`; `@takazudo/zudo-doc` is pinned to
  `5.12.0`. The app and compatibility fixture use independent frozen lockfiles.
- **Node-free at runtime**: `zfb dev` with zero `.mjs` plugins spawns no Node host. The native `@takazudo/zfb-<platform>/zfb` binary is bundled in `node_modules` (populated at build/setup time via `pnpm install --frozen-lockfile`, Node at setup only).
- **Host-owned routes**: `app/` owns the route adapters because the selected
  zfb configuration ends with `plugins: []`; package route/plugin entrypoints
  are not enabled.
- **Port 4892**: the default preferred port in the authored settings schema; the active runtime may use a loopback fallback when that port is occupied.
- **Semantic readiness**: the host polls `GET /docs/` until the response is
  successful and contains the generator-owned `Claude Resources` marker; a
  generic `200` is not sufficient. `/` is the exact server-rendered alias.
- **Writable workspace**: a pruned, lockfile-faithful runtime tree is copied to `<app_data_dir>/app-workspace/` on first launch. A SHA-256 tree digest covers the staged app, theme assets, and staging/digest implementation; its refresh token gates the `.ccresdoc-workspace-ready` sentinel. Dev mode uses the repo `app/` directly.
- **Rust generator** (`crates/ccresdoc-claude-md`): `generate()` + `watch()` walk the authored source (default `~/.claude/`) and emit zudo-doc-compatible MDX. `zfb dev` content-watch HMRs the result.

## Settings contract

Settings are a human-editable TOML document. The path is resolved in this order:

1. `CCRESDOC_CONFIG` when it is set to a non-empty path;
2. `$XDG_CONFIG_HOME/ccresdoc/config.toml` when `XDG_CONFIG_HOME` is non-empty;
3. `$HOME/.config/ccresdoc/config.toml`.

Reading a missing file never creates it. The complete schema and defaults are:

```toml
schema_version = 1

[source]
claude_dir = "~/.claude"

[appearance]
mode = "system"       # system | light | dark
theme_pack = "default"

[server]
preferred_port = 4892
fallback_to_free_port = true
```

The editor reports authored values separately from the effective/active runtime.
The authored source and preferred port remain exactly what is in TOML; the
effective source is canonicalized and the effective port can differ when the
preferred loopback port is occupied. CCResDoc only binds its own loopback
listener and never signals or kills a foreign port owner. Set
`fallback_to_free_port = false` for strict mode; an occupied preferred port
then leaves the saved document untouched and exposes the bundled Settings
recovery surface.

Missing settings use defaults without writing TOML. Parseable semantic errors
(for example a relative or unavailable source, invalid mode, or out-of-range
port) are shown as blocking diagnostics and use safe effective defaults until
repaired. Malformed TOML is preserved byte-for-byte and requires the explicit
Replace malformed action. An unsupported future `schema_version` is read-only;
CCResDoc will not overwrite it. A valid but unavailable theme pack is retained
as authored data while the effective theme falls back to `default`. Existing
comments, unknown keys, newline style, and file permissions are preserved on a
successful atomic save.

Settings can be opened from the docs header gear, the native `CCResDoc → Settings…`
menu, `Cmd+,`, or the loading/error page's Settings action. All of these focus
one native Settings window; closing it hides the window and leaves the docs
runtime alive. Draft edits are validated by Rust, Reset restores the defaults in
the draft, Cancel discards the draft and clears the appearance preview, and
Save persists then applies the draft without closing the window.
Appearance-only changes update the active page without restarting the server;
source, preferred-port, and fallback changes restart the runtime once. A
restart failure is reported as saved-but-not-active and leaves recovery open.

External edits are guarded by a SHA-256 content revision. Save refuses a stale
revision. Reload discards the local draft; Reapply copies only the explicitly
dirty fields onto the latest valid document, retries boundedly, and refuses a
second racing edit rather than blindly overwriting it. Quick appearance
controls use the same TOML authority: a missing file may accept a valid legacy
browser preference as a first-save candidate, while any existing document
(including malformed or invalid TOML) blocks legacy import. Appearance is
initialized at document start before the ready marker and is scoped to the
current loopback origin.

The main docs WebView has only the narrow commands it needs. Privileged source,
server, config-file, and Settings mutations require the `settings` window; docs
appearance mutation is accepted only from the active `/docs/` loopback origin.
Navigation and CSP are loopback-only, and app quit tears down the Rust watcher
and the exact app-owned zfb process group while leaving unrelated listeners
alive.

### Settings verification

The deterministic settings suites and static production-leak guards run with:

```sh
bash scripts/test-macos-settings.sh --fixtures-only
pnpm run check:frontend
pnpm run check:runtime-package
cargo test --manifest-path src-tauri/Cargo.toml
pnpm run b4push
```

After a fresh macOS arm64 bundle is built, the bounded package smoke helper
receives only temporary paths and the production bundle identity:

```sh
CCRESDOC_SETTINGS_APP=/path/to/CCResDoc.app pnpm run test:macos-settings
```

The helper launches through LaunchServices with a unique temporary `HOME`,
`TMPDIR`, `XDG_CONFIG_HOME`, and `CCRESDOC_CONFIG`. It checks missing-file
defaults, authored source selection, preferred/effective fallback ports,
strict occupied-port and bad-source recovery, malformed-byte preservation,
semantic docs readiness across relaunch, and exact app-owned child cleanup while
the foreign fixture listener remains alive. It does not use a broad process kill or access
the tester's real `~/.claude`. The packaged Computer Use walkthrough remains a
manager/release step for Settings window focus, picker/save/relaunch,
appearance preview/cancel/persistence, external-edit rebase, and visual
first-paint confirmation.


## Prerequisites (development only)

End users need nothing beyond the `.app` bundle. To develop or build from source:

- **Rust** (stable) — `rustup install stable`
- **Tauri CLI** — `cargo install tauri-cli` or `cargo binstall tauri-cli`
- **pnpm** — used once at build time to install `app/node_modules` (incl. native `zfb` binary)

The repository requires Node `>=22` and pnpm `>=10` for setup/build tooling.

## Develop

```bash
pnpm --dir app install --frozen-lockfile   # once — populates node_modules incl. native zfb binary
pnpm --dir app run validate:dependencies
pnpm --dir app run validate:theme-packs
pnpm --dir app run typecheck
pnpm --dir app run check:zfb
pnpm --dir app run test:run
pnpm --dir app run test:theme-packs
cargo tauri dev
```

`cargo tauri dev` resolves the native package-root carrier from
`app/node_modules/@takazudo/zfb-<platform>/zfb`, runs the Rust generator + watcher
in-process, spawns `zfb dev` on the effective settings port, and opens the Tauri
window only after semantic `/docs/` readiness. The logo on both `/` and `/docs/`
links directly to `/docs/`. Changes to the authored source are picked up live
via HMR.

To rebuild the frontend shell manually (e.g. after changing `app/pages/`):

```bash
cd app && pnpm exec zfb build
```

(or just run `pnpm b4push`.)

## Build the .app

```bash
cargo tauri build
```

`beforeBuildCommand` performs a frozen install/build and stages only the
lockfile-reachable runtime workspace automatically. The staged workspace keeps
the direct package-root native carrier and the selected zero-plugin config. The `.app` does not bundle
frontend test/build tooling, non-host zfb binaries, or disabled Node-plugin
dependencies. Validate it with `pnpm run probe:runtime-package`; on macOS arm64,
run `scripts/test-macos-package.sh` for the separately mandatory packaged
app/WebView counterpart. Tauri runs build hooks from the project root, and no
global `zfb` on PATH is required.
Output: `src-tauri/target/release/bundle/macos/CCResDoc.app`.

See `.claude/skills/ccresdoc-build/SKILL.md` for the full install workflow (clean → build → verify → kill → install → launch).

## Project structure

```
crates/          Rust workspace crates
  ccresdoc-claude-md/   ~/.claude→MDX generator + watcher (the live engine)
src-tauri/       Tauri host (main.rs, tauri.conf.json, loading page)
app/             zfb frontend project (zudo-doc consumer, port 4892)
scripts/         run-b4push.sh, test-launch.sh, test-macos-settings.sh
.github/         GitHub Actions CI workflow
.claude/skills/  ccresdoc-build skill (local build + install steps)
```

See per-directory CLAUDE.md files for detailed architecture notes.
The final automated and host-only acceptance matrix is documented in
[`docs/architecture/verification-matrix.md`](docs/architecture/verification-matrix.md).

## CI

GitHub Actions runs frozen published frontend dependency, type, zfb-check,
Vitest, and native-build gates alongside `cargo fmt --check`,
`cargo clippy --workspace --exclude ccresdoc`, and `cargo test --workspace
--exclude ccresdoc`. The `ccresdoc` (src-tauri) crate is excluded because
webkit2gtk is not available on ubuntu-latest.

## Before pushing

```bash
pnpm b4push
```

Runs all reliable cross-platform checks locally: frozen dependency install and
validation, strict TypeScript, `zfb check`, Vitest, native zfb build, cargo fmt,
clippy/tests for the pure-Rust generator, the pruned node-free runtime lifecycle,
and the independent compatibility fixture. The `ccresdoc` Tauri crate is
excluded from Linux clippy/test because it requires webkit2gtk/gtk3. A release
still requires the separate `scripts/test-macos-package.sh` macOS-arm64
packaged app/WebView gate and the real-WebView visual checks listed in the
verification matrix. A Linux run may validate staged static inputs and the
native probe, but it does not execute or pass the macOS app/WebView launch.
