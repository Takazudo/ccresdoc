# app/ — CCResDoc zfb project

Static shell built by zfb. Output in `dist/` is served by the axum runtime server (S5).

## Local dependency setup

`@takazudo/zfb` and `@takazudo/zfb-runtime` are embedded in the zfb binary
(upstream fixes #183/#198 — `crates/zfb/build.rs` snapshots them via `include_dir!`
at compile time). They no longer appear as `link:` entries in `package.json`.

`preact` and `preact-render-to-string` are consumer-side npm dependencies until
Wave 3 (#31) removes them (they will also be embedded in the zfb binary after
upstream fix #209 lands in the consuming build).

## Build

The zfb Rust binary must be in PATH. Build or install it once:

  cd $HOME/repos/myoss/zfb && cargo build --release
  # Then add target/release/ to PATH, or use cargo install --path crates/zfb

The zfb binary downloads and stages the esbuild and tailwindcss v4 standalone
binaries at `cargo build` time (sub-197 build.rs). No separate fetch step needed.

For local dev where the zfb binary is from `target/release/` (not cargo install),
esbuild and tailwindcss are resolved relative to the zfb WORKSPACE root. If the
default slot path (`crates/zfb/binaries/esbuild/esbuild`) is not on CWD, set:

  ZFB_ESBUILD_BIN=<absolute-path-to-esbuild>
  ZFB_TAILWIND_BIN=<absolute-path-to-tailwindcss-v4>

## Known zfb feature gaps worked around

1. **public/ not copied to dist/** — RESOLVED by zfb upstream fix #192. zfb now
   natively copies `public/` to `dist/` during production builds. The old
   `plugins/copy-public.mjs` workaround was removed in sub-28.

2. **Underscore pages skipped by router** — zfb skips any page whose filename
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
