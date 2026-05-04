# crates/ — CCResDoc Rust Workspace

Three library crates that together implement the documentation server pipeline. All are members of the workspace defined in the root `Cargo.toml`.

## Workspace layout

```
crates/
  ccresdoc-resources/   Walker: discovers .claude/ → ResourceTree
  ccresdoc-renderer/    Renderer: Markdown + syntax highlighting → HTML
  ccresdoc-server/      Server: axum routes + shell template substitution
```

`src-tauri/` is also a workspace member (the Tauri binary). It depends on `ccresdoc-server`.

## What each crate owns

### ccresdoc-resources

Walker for `$HOME/.claude/` that returns a structured `ResourceTree`.

- Entry point: `walk_claude_dir(claude_dir, project_root)` — performs all I/O
- Output: `ResourceTree` containing discovered commands, skills, agents, and CLAUDE.md files
- All struct constructors are pure data holders with no I/O side-effects
- Key types: `ResourceTree`, `ResourceItem`, `ResourceError`

### ccresdoc-renderer

Converts Markdown source to styled HTML. No I/O — purely a transformation library.

- Markdown parsing: `comrak` (CommonMark + extensions)
- Syntax highlighting: `syntect` with default themes
- HTML post-processing: `lol_html` for link rewriting, heading anchors, admonitions
- Entry point: `render(markdown: &str) -> Result<String, RenderError>`
- Sub-modules: `highlight`, `heading_links`, `admonitions`, `code_title`, `strip_md`

### ccresdoc-server

Axum HTTP server that serves the zfb static shell and renders Markdown dynamically.

- Entry points: `serve(config)` (run forever) and `serve_with_shutdown(config, signal)` (Tauri use)
- Configuration: `ServerConfig { port, claude_dir, project_root, dist_dir }`
- Static files: served from `dist_dir` (the compiled `app/dist/` tree)
- Dynamic routes: `/docs/*` — loads `_shell/index.html`, substitutes `☃CCRESDOC_TITLE_SLOT☃` and `☃CCRESDOC_CONTENT_SLOT☃` sentinels with rendered content
- Manifest route: `/manifest.json` — returns the full resource tree as JSON
- Readiness probe: `GET /___ready` returns 200 once the server is bound

## Dependency graph

```
ccresdoc-resources  (no internal deps)
ccresdoc-renderer   (no internal deps)
ccresdoc-server     → ccresdoc-resources, ccresdoc-renderer
src-tauri           → ccresdoc-server
```

## Adding a new crate

1. `cargo new --lib crates/ccresdoc-<name>`
2. Add it to workspace `members` in root `Cargo.toml` (already covered by `crates/*` glob)
3. Add it as a path dependency in whichever crate needs it
