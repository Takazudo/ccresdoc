# app/ — CCResDoc zfb project

Static shell built by zfb. Output in `dist/` is served by the axum runtime server (S5).

zfb embeds `@takazudo/zfb`, `@takazudo/zfb-runtime`, `preact`, `preact-render-to-string`, and `hono` in its binary; ccresdoc has no `app/node_modules/` requirement.

Config is JSON (`zfb.config.json`) — no Node.js needed for config loading.

## Build

The zfb Rust binary must be in PATH. Build or install it once:

  cargo install --path $HOME/repos/myoss/zfb/crates/zfb

The zfb binary embeds esbuild + tailwindcss-v4 standalone binaries via
`include_dir!` and extracts them at runtime; consumer projects no longer need
to point at the zfb source tree via env vars. Embedded framework packages
(preact, preact-render-to-string, hono, @takazudo/*) also resolve from the
binary's `include_dir!` snapshot when `app/node_modules/` does not exist.

## Known zfb feature gaps worked around

1. **Underscore pages skipped by router** — zfb skips any page whose filename
   starts with `_` (conventionally: framework internals). The shell template
   page (which must output to `dist/_shell/index.html`) is therefore named
   `pages/shell.tsx` instead of `pages/_shell.tsx`. The `plugins/rename-shell.mjs`
   postBuild plugin renames `dist/shell/index.html` → `dist/_shell/index.html`.
   Remove this rename once zfb supports an opt-in escape hatch for
   underscore-prefixed pages (e.g., frontmatter `includeUnderscore: true`).

## Sentinels

`dist/_shell/index.html` contains two runtime substitution sentinels:
- `☃CCRESDOC_TITLE_SLOT☃` — inside `<title>`, replaced with the page title
- `☃CCRESDOC_CONTENT_SLOT☃` — inside `<main>`, replaced with rendered HTML

S5 (axum server) loads this file and string-replaces both sentinels at request time.
