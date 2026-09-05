# CCResDoc

macOS documentation viewer for settings-selected Claude and Codex resource
directories (Claude on with `~/.claude`, Codex off with `~/.codex` by default).
The thin Tauri host runs the in-process Rust generators/watchers
(`ccresdoc-claude-md`), spawns the native `zfb` binary on the effective loopback
port, and navigates there after semantic readiness.

This project consumes the published zfb toolchain through the app's frozen
lockfile. The hybrid architecture is documented in the epic issue (#41).

## Key architecture facts (claim checklist)

- `node_modules` is populated at **setup/build time only** via `pnpm install --frozen-lockfile` (Node at setup only — NOT at runtime).
- The published toolchain is `@takazudo/zfb*` `2.15.1` plus `@takazudo/zudo-doc` `5.17.2`; the app and compatibility fixture are validated from their own frozen lockfiles.
- The host resolves the **native** zfb binary at the package-root carrier `<workspace>/node_modules/@takazudo/zfb-<platform>/zfb` — NOT the `.bin/zfb` Node-shebang wrapper.
- **Port 4892**: the authored default. The host passes the validated effective
  port to zfb and may select a free loopback fallback without touching its owner.
- **Node-free at runtime**: the zfb config ends with `plugins: []`; host-owned route adapters replace package route/plugin entrypoints, and `zfb dev` spawns no Node host process.
- **Writable workspace model**: bundled `.app` copies `Resources/runtime-workspace/app/` to `<app_data_dir>/app-workspace/` on first launch. A SHA-256 tree digest covers the staged workspace plus the staging/digest implementation; its refresh token gates the `.ccresdoc-workspace-ready` sentinel. Staging admits only release-owned routes/config/landing files and theme assets; it omits `dist` and every generated Claude/Codex detail/status namespace.
- **Rust selected-resource engine** (`crates/ccresdoc-claude-md`) is the live engine: selected Claude and Codex sources are generated into disjoint MDX namespaces, then `zfb dev` content-watch HMRs the promoted tree. The coordinator owns overview/status pages and rollback; each enabled source owns one watcher.
- Readiness is polled on semantic `GET /docs/` (NOT a generic `GET /` and NOT `/___ready`); the response must contain the current `CCResDoc` shell marker and both selection-specific transition markers.
- Canonical compatibility facts and drift-checked outputs live under `compatibility/node-free-latest/evidence/`; the staged Linux probe is not a macOS WebView launch.

## When working on this repo

- For zfb-related work, consult the published package contract and this
  repository's architecture notes first. Keep dependency behavior and product
  guidance project-local; do not copy private provenance into this repository.
- For Tauri work, consult `/tauri-wisdom` (esp. the `recipes/doc-viewer-app.mdx` recipe).

## Per-directory context files

Detailed architecture notes are in per-directory CLAUDE.md files — read these before touching a subdirectory:

- `crates/CLAUDE.md` — Rust workspace layout; the single `ccresdoc-claude-md` generator crate
- `src-tauri/CLAUDE.md` — Tauri host architecture (sidecar spawn, workspace resolution, native zfb binary, readiness poll)
- `app/CLAUDE.md` — zfb frontend project; MDX content contract; known zfb workarounds
