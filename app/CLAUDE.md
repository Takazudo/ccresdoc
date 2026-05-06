# app/ — CCResDoc zfb project

Static shell built by zfb. Output in `dist/` is served by the axum runtime server (S5).

zfb embeds `@takazudo/zfb`, `@takazudo/zfb-runtime`, `preact`, `preact-render-to-string`, and `hono` in its binary; ccresdoc has no `app/node_modules/` requirement.

Config is still TS (`zfb.config.ts`); JSON conversion deferred pending Takazudo/zudo-front-builder#211.

## Build

The zfb Rust binary must be in PATH. Build or install it once:

  cargo install --path $HOME/repos/myoss/zfb/crates/zfb

`cargo install` downloads esbuild + tailwindcss-v4 standalone binaries to the
zfb source tree (`crates/zfb/binaries/`). The zfb runtime currently resolves
these via fixed staged-slot paths or env vars — not via the binary's embedded
snapshot — so consumer projects (like ccresdoc) must point at them via env
vars when running `zfb build` outside the zfb workspace root. ccresdoc's build
invocations (`tauri.conf.json` `beforeBuildCommand` and `scripts/run-b4push.sh`)
set `ZFB_ESBUILD_BIN` and `ZFB_TAILWIND_BIN` for this reason.

The runtime extraction DOES work for embedded framework packages (preact,
preact-render-to-string, hono, @takazudo/*) — those resolve from the binary's
`include_dir!` snapshot when `app/node_modules/` does not exist.

## Known zfb feature gaps worked around

1. **Underscore pages skipped by router** — zfb skips any page whose filename
   starts with `_` (conventionally: framework internals). The shell template
   page (which must output to `dist/_shell/index.html`) is therefore named
   `pages/shell.tsx` instead of `pages/_shell.tsx`. The `plugins/rename-shell.mjs`
   postBuild plugin renames `dist/shell/index.html` → `dist/_shell/index.html`.
   Remove this rename once zfb supports an opt-in escape hatch for
   underscore-prefixed pages (e.g., frontmatter `includeUnderscore: true`).

2. **TS config loader uses staged slot for esbuild, not embedded extraction** —
   `crates/zfb/src/config.rs::resolve_esbuild_binary` resolves esbuild from
   `ZFB_ESBUILD_BIN` or a fixed `crates/zfb/binaries/esbuild/esbuild` slot
   (relative to PWD). Consumer projects must set `ZFB_ESBUILD_BIN`. Same for
   tailwindcss-v4 in the CSS engine. Filed upstream as a follow-up to
   Takazudo/zudo-front-builder#210 (separate from #211 which covers JSON
   plugin loading). Resolve once zfb extracts these binaries from `include_dir!`
   at runtime, then drop the env-var prefixes from `tauri.conf.json` and
   `scripts/run-b4push.sh`.

3. **JSON config silently skips plugins** — Takazudo/zudo-front-builder#211.
   Blocks the `zfb.config.ts` → `zfb.config.json` conversion that would have
   killed the Node.js dependency in the config-load step. ccresdoc keeps
   `zfb.config.ts` until upstream fix lands.

## Sentinels

`dist/_shell/index.html` contains two runtime substitution sentinels:
- `☃CCRESDOC_TITLE_SLOT☃` — inside `<title>`, replaced with the page title
- `☃CCRESDOC_CONTENT_SLOT☃` — inside `<main>`, replaced with rendered HTML

S5 (axum server) loads this file and string-replaces both sentinels at request time.
