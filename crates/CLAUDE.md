# crates/ — CCResDoc Rust Workspace

One library crate implements the node-free live-update engine. It is a member
of the workspace defined in the root `Cargo.toml` (`members = ["crates/*", "src-tauri"]`).

## Workspace layout

```
crates/
  ccresdoc-claude-md/   Generator + watcher: ~/.claude/ → zudo-doc MDX
```

`src-tauri/` is also a workspace member (the Tauri binary). It depends on
`ccresdoc-claude-md` for in-process generation + watching.

> History: Waves 1-2 replaced the old three-crate pipeline
> (`ccresdoc-resources` walker, `ccresdoc-renderer`, `ccresdoc-server`) with the
> zudo-doc-based architecture. The resources walker was reborn inside
> `ccresdoc-claude-md`; the renderer and server were deleted (rendering is now
> zudo-doc's job).

## What the crate owns

### ccresdoc-claude-md

Walks `~/.claude/` and emits **zudo-doc-compatible MDX**, then watches for
changes and regenerates. This is the live engine: the Rust watcher writes MDX →
`zfb dev` content-watch → HMR. (`zfb`'s `extraWatchPaths` does NOT re-run
`preBuild`, so generation + watch must live in Rust, not a zfb prebuild step.)

- **`generate(&Config) -> Result<GenerateReport>`** — one-shot generation (boot).
- **`watch(Config, Duration, Fn(WatchEvent)+Send+'static) -> Result<WatchHandle>`**
  — `notify`-based watcher, ~300ms debounced, serialized so two regenerations
  never write the same MDX concurrently. Drop the handle (or `stop()`) to end it.
- **`Config { claude_dir, project_root, docs_dir }`** — absolute paths resolved
  and passed by the Tauri host (Wave 3).
- **`GenerateReport { claude_md, commands, skills, agents }`** — emitted counts.

Internal modules: `escape` (faithful port of zudo-doc's `escape-for-mdx.ts`),
`walk` (the reborn `~/.claude` walker), `generate` (MDX emission per the Wave 1
content contract in `app/CLAUDE.md`), `watch` (the debounced/serialized
watcher), `error`.

Key invariants:
- The CLAUDE.md walk is **scoped to `~/.claude`**; `project_root = $HOME` is
  rejected (`GenerateError::ProjectRootTooBroad`, zudolab/zudo-doc#2115).
- `followSymlinks = false` (skills contain symlinks).
- Files lacking frontmatter are skipped (matches the JS generator).
- Output filenames/positions follow the Wave 1 contract: overview 899,
  CLAUDE.md 900, commands 901, skills 902, agents 903; CLAUDE.md pages are
  `global.mdx` / `project-<slug>.mdx`.

## Dependency graph

```
ccresdoc-claude-md  (no internal deps; external: notify, walkdir, serde_yaml, ...)
src-tauri           → ccresdoc-claude-md
```

## Adding a new crate

1. `cargo new --lib crates/ccresdoc-<name>`
2. The workspace `members` glob (`crates/*`) picks it up automatically.
3. Add it as a path dependency in whichever crate needs it.
