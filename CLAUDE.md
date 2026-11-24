# CCResDoc

macOS documentation viewer for configured document sources and reusable reading
presets (initially Claude on with `~/.claude`, Codex off with `~/.codex`).
The thin Tauri host runs the in-process Rust generators/watchers
(`ccresdoc-claude-md`), spawns the native `zfb` binary on the effective loopback
port, and navigates there after semantic readiness.

This project consumes the published zfb toolchain through the app's frozen
lockfile. The hybrid architecture is documented in the epic issue (#41).

## Key architecture facts (claim checklist)

- `node_modules` is populated at **setup/build time only** via `pnpm install --frozen-lockfile` (Node at setup only — NOT at runtime).
- The published toolchain is `@takazudo/zfb*` `7.2.0` plus `@takazudo/zudo-doc` `14.0.0`, written by hand in `scripts/lib/toolchain-contract.mjs`; the app and compatibility fixture are validated from their own frozen lockfiles against that contract.
- Settings are schema v3 with the `classic` theme pack and a revision-aware source/preset catalog. Existing v2 settings migrate atomically; recognized v1 retirement is unchanged.
- The host resolves the **native** zfb binary at the package-root carrier `<workspace>/node_modules/@takazudo/zfb-<platform>/zfb` — NOT the `.bin/zfb` Node-shebang wrapper.
- **Port 4892**: the authored default. The host passes the validated effective
  port to zfb and may select a free loopback fallback without touching its owner.
- **Node-free at runtime**: the zfb config ends with `plugins: []`; host-owned route adapters replace package route/plugin entrypoints, and `zfb dev` spawns no Node host process.
- **Writable workspace model**: bundled `.app` copies `Resources/runtime-workspace/app/` to `<app_data_dir>/app-workspace/` on first launch. A SHA-256 tree digest covers the staged workspace plus the staging/digest implementation; its refresh token gates the `.ccresdoc-workspace-ready` sentinel. Staging admits only release-owned routes/config/article files and theme assets; it omits `dist` and every generated source namespace.
- **Rust document-source engine** (`crates/ccresdoc-claude-md`) generates enabled sources under their unique mounts. A private ownership manifest scopes promotion, rollback, pruning and search; each enabled source owns a watcher. Built-in Claude/Codex rules retain specialized semantics.
- Readiness polls semantic `GET /docs/`, the app-owned management article and each enabled mounted source. The management article remains reachable with zero sources or first-launch source failure.
- The main WebView source editor lives at `/docs/ccresdoc/document-sources/` as an ordinary zudo-doc article. Native Settings holds app options and a Resources summary/link. Its narrow IPC requires the current owned main window, loopback origin, editor route and runtime generation.
- Canonical compatibility facts and drift-checked outputs live under `compatibility/node-free-latest/evidence/`; the staged Linux probe is not a macOS WebView launch.

## When working on this repo

- For zfb-related work, consult the published package contract and this
  repository's architecture notes first. Keep dependency behavior and product
  guidance project-local; do not copy private provenance into this repository.
- For Tauri work, consult `/tauri-wisdom` (esp. the `recipes/doc-viewer-app.mdx` recipe).

## Per-directory context files

Detailed architecture notes are in per-directory CLAUDE.md files — read these before touching a subdirectory:

- `crates/CLAUDE.md` — Rust workspace layout; the `ccresdoc-claude-md` generator crate and the Tauri-free `ccresdoc-host-domain` test crate
- `src-tauri/CLAUDE.md` — Tauri host architecture (sidecar spawn, workspace resolution, native zfb binary, readiness poll)
- `app/CLAUDE.md` — zfb frontend project; MDX content contract; known zfb workarounds
