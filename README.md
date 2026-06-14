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
WebView → http://localhost:4892/
```

Key facts:
- **Node-free at runtime**: `zfb dev` with zero `.mjs` plugins spawns no Node host. The native `@takazudo/zfb-<platform>/zfb` binary is bundled in `node_modules` (populated at build/setup time via `pnpm install`, Node at setup only).
- **Port 4892**: pinned in `app/zfb.config.ts` and `src-tauri/tauri.conf.json`.
- **Writable workspace**: the bundled `app/` tree is copied to `<app_data_dir>/app-workspace/` on first launch, gated by a version token + a `.ccresdoc-workspace-ready` sentinel. The token is the host's compiled `CARGO_PKG_VERSION` (bumped per release → the copy refreshes on upgrade); an optional `version.txt` beside the bundled `app/` overrides it if present. Dev mode uses the repo `app/` directly.
- **Rust generator** (`crates/ccresdoc-claude-md`): `generate()` + `watch()` walk `~/.claude/` and emit zudo-doc-compatible MDX. `zfb dev` content-watch HMRs the result.

## Prerequisites (development only)

End users need nothing beyond the `.app` bundle. To develop or build from source:

- **Rust** (stable) — `rustup install stable`
- **Tauri CLI** — `cargo install tauri-cli` or `cargo binstall tauri-cli`
- **pnpm** — used once at build time to install `app/node_modules` (incl. native `zfb` binary)

## Develop

```bash
cd app && pnpm install   # once — populates node_modules incl. native zfb binary
cargo tauri dev
```

`cargo tauri dev` resolves the native `zfb` binary from `app/node_modules`, runs the
Rust generator + watcher in-process, spawns `zfb dev --port 4892`, and opens the Tauri
window once `GET /` returns 200. Changes to `~/.claude/` are picked up live via HMR.

To rebuild the frontend shell manually (e.g. after changing `app/pages/`):

```bash
cd app && pnpm exec zfb build
```

(or just run `bash scripts/run-b4push.sh`.)

## Build the .app

```bash
cargo tauri build
```

`beforeBuildCommand` runs `cd app && pnpm install && pnpm exec zfb build` automatically
(Tauri runs build hooks from the project root) — no global `zfb` on PATH required. Output: `src-tauri/target/release/bundle/macos/CCResDoc.app`.

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

## CI

GitHub Actions runs `cargo fmt --check`, `cargo clippy --workspace --exclude ccresdoc`, and `cargo test --workspace --exclude ccresdoc` on every push and PR targeting `main` or `base/ccresdoc-zudo-doc-rewrite`. The `ccresdoc` (src-tauri) crate is excluded because webkit2gtk is not available on ubuntu-latest. The `zfb build` step is run locally (via `scripts/run-b4push.sh`) but deferred from CI — see `.github/workflows/ci.yml` for the rationale.

## Before pushing

```bash
bash scripts/run-b4push.sh
```

Runs all checks locally: cargo fmt, clippy (`--exclude ccresdoc`), test (`--exclude ccresdoc`), plus `pnpm install` and `pnpm exec zfb build` in `app/`. The `ccresdoc` (src-tauri) crate is excluded from clippy/test to match CI — it requires webkit2gtk/gtk3 which are not available on Linux CI runners.
