# CCResDoc

This repo is a restructure of the existing ccresdoc Tauri app (originally at $HOME/.claude/doc/) using zfb (Rust SSG orchestrator at $HOME/repos/myoss/zfb). The hybrid architecture is documented in the epic issue.

## When working on this repo

- For zfb-related work, **invoke `/refer-another-project zfb` first** so you pick up zfb's repo structure, crate layout, and CLAUDE.md context. zfb is a real-world test of this restructure — bugs found in zfb may be fixed upstream by PR + merge to zfb's main, per project authorisation.
- **Any state-mutating zfb work — edits, branches, commits, builds — goes through the `zfb-upstream-dev` skill** (mandatory `git worktree`-based flow). The zfb checkout at `$HOME/repos/myoss/zfb` is shared with concurrent Claude sessions, so touching its working tree directly races their state. The skill describes the rule, the worktree pattern, and recipes for read-only inspection, edit-and-build, and pin bumps.
- For Tauri work, consult `/tauri-wisdom` (esp. the `recipes/doc-viewer-app.mdx` recipe).

## Per-directory context files

Detailed architecture notes are in per-directory CLAUDE.md files — read these before touching a subdirectory:

- `crates/CLAUDE.md` — Rust workspace layout; what each crate owns
- `src-tauri/CLAUDE.md` — Tauri wrapper architecture (embedded axum server, window lifecycle)
- `app/CLAUDE.md` — zfb frontend project; embedded package credits; known zfb workarounds
