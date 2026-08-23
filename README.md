# CCResDoc

A macOS documentation viewer for `$HOME/.claude/` — renders CLAUDE.md hierarchies, skills, commands, and agent definitions as a browsable local web app inside a native Tauri window.

The app is a thin Tauri host around a **node-free sidecar architecture**: at launch it spawns the native `zfb` binary (`zfb dev --port 4892`) and a Rust watcher that generates MDX from `~/.claude/`. No Node.js or external runtime dependencies are required once the `.app` is built.

## Architecture

```
~/.claude/           ← source of truth (CLAUDE.md files, skills/, commands/, agents/)
     │
     ▼  Rust watcher (ccresdoc-claude-md crate, in-process)
app/src/content/docs/claude*/   ← generated MDX (gitignored)
     │
     ▼  zfb dev (native binary, port 4892, node-free at runtime)
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
- **Port 4892**: pinned in `app/zfb.config.ts` and `src-tauri/tauri.conf.json`.
- **Semantic readiness**: the host polls `GET /docs/` until the response is
  successful and contains the generator-owned `Claude Resources` marker; a
  generic `200` is not sufficient. `/` is the exact server-rendered alias.
- **Writable workspace**: a pruned, lockfile-faithful runtime tree is copied to `<app_data_dir>/app-workspace/` on first launch. A SHA-256 tree digest covers the staged app, theme assets, and staging/digest implementation; its refresh token gates the `.ccresdoc-workspace-ready` sentinel. Dev mode uses the repo `app/` directly.
- **Rust generator** (`crates/ccresdoc-claude-md`): `generate()` + `watch()` walk `~/.claude/` and emit zudo-doc-compatible MDX. `zfb dev` content-watch HMRs the result.

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
in-process, spawns `zfb dev --port 4892`, and opens the Tauri window only after
semantic `/docs/` readiness. The logo on both `/` and `/docs/` links directly to
`/docs/`. Changes to `~/.claude/` are picked up live via HMR.

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
scripts/         run-b4push.sh, test-launch.sh
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
