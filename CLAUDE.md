# CCResDoc

macOS documentation viewer for `$HOME/.claude/`. Thin Tauri host around a **node-free sidecar** architecture: at launch the host runs the in-process Rust generator/watcher (`ccresdoc-claude-md`) and spawns the native `zfb` binary (`zfb dev --port 4892`); the WebView navigates to `http://localhost:4892/` once the site is ready.

This repo uses zfb (Rust SSG orchestrator at `$HOME/repos/myoss/zfb`) for the frontend build. The hybrid architecture is documented in the epic issue (#41).

## Key architecture facts (claim checklist)

- `node_modules` is populated at **setup/build time only** via `pnpm install` (Node at setup only — NOT at runtime).
- The host resolves the **native** zfb binary at `<workspace>/node_modules/@takazudo/zfb-<platform>/zfb` — NOT the `.bin/zfb` Node-shebang wrapper.
- **Port 4892**: pinned in `app/zfb.config.ts` and `src-tauri/tauri.conf.json`.
- **Node-free at runtime**: `zfb dev` with zero `.mjs` plugins spawns no Node host process.
- **Writable workspace model**: bundled `.app` copies its `app/` tree to `<app_data_dir>/app-workspace/` on first launch, versioned by `version.txt` + `.ccresdoc-workspace-ready` sentinel.
- **Rust `~/.claude`→MDX generator** (`crates/ccresdoc-claude-md`) is the live engine: `generate()` + `watch()` write MDX → `zfb dev` content-watch → HMR.
- Readiness is polled on `GET /` (NOT `/___ready`).

## When working on this repo

- For zfb-related work, **invoke `/refer-another-project zfb` first** so you pick up zfb's repo structure, crate layout, and CLAUDE.md context. zfb is a real-world test of this restructure — bugs found in zfb may be fixed upstream by PR + merge to zfb's main, per project authorisation.
- **Any state-mutating zfb work — edits, branches, commits, builds — goes through the `zfb-upstream-dev` skill** (mandatory `git worktree`-based flow). The zfb checkout at `$HOME/repos/myoss/zfb` is shared with concurrent Claude sessions, so touching its working tree directly races their state. The skill describes the rule, the worktree pattern, and recipes for read-only inspection, edit-and-build, and pin bumps.
- For Tauri work, consult `/tauri-wisdom` (esp. the `recipes/doc-viewer-app.mdx` recipe).

## Per-directory context files

Detailed architecture notes are in per-directory CLAUDE.md files — read these before touching a subdirectory:

- `crates/CLAUDE.md` — Rust workspace layout; the single `ccresdoc-claude-md` generator crate
- `src-tauri/CLAUDE.md` — Tauri host architecture (sidecar spawn, workspace resolution, native zfb binary, readiness poll)
- `app/CLAUDE.md` — zfb frontend project; MDX content contract; known zfb workarounds
