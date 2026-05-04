# app/ — CCResDoc zfb project

Static shell built by zfb. Output in `dist/` is served by the axum runtime server (S5).

## Local dependency setup

zfb is NOT published to npm. Dependencies are wired via pnpm `link:` protocol:

- `@takazudo/zfb`: `link:../../zfb/packages/zfb` (2 levels up from app/)
- `@takazudo/zfb-runtime`: `link:../../zfb/packages/zfb-runtime`

These resolve to `$HOME/repos/myoss/zfb/packages/{pkg}` from the main repo.

Approach tried: link: (pnpm symlink) — this works because the zfb workspace
node_modules/ is already populated, so transitive resolution succeeds.
file: was also considered but link: is simpler for this cross-repo setup.

**Worktree caveat:** the link: relative paths above are computed from
`<repo>/app/`. When the repo is checked out as a git worktree at
`<repo>/worktrees/<topic>/`, the `app/` directory is one level deeper, so the
link target resolves outside `$HOME/repos/myoss/zfb`. After creating any
worktree that touches `app/`, re-run `pnpm install` inside that worktree
(pnpm will rewrite the symlinks relative to the worktree's `app/`), then run
`pnpm install` again in the main repo before building from the main repo.
A future fix: switch to absolute path or pnpm pack vendoring.

## Build

The zfb Rust binary must be in PATH. Install it once:

  cd $HOME/repos/myoss/zfb && cargo install --path crates/zfb

Two additional tool binaries are needed (not in PATH; set via env vars):

  ZFB_ESBUILD_BIN  — esbuild 0.25.12 standalone CLI
  ZFB_TAILWIND_BIN — tailwindcss v4 standalone CLI

The `pnpm build` script has these hardcoded to the local zfb checkout paths.
To build manually with pnpm zfb build:

  cd app
  ZFB_ESBUILD_BIN="$HOME/repos/myoss/zfb/node_modules/.pnpm/@esbuild+darwin-arm64@0.25.12/node_modules/@esbuild/darwin-arm64/bin/esbuild" \
  ZFB_TAILWIND_BIN="$HOME/repos/myoss/zfb/crates/zfb/binaries/tailwindcss-v4" \
  pnpm zfb build

## Known zfb feature gaps worked around

1. **public/ not copied to dist/** — zfb only serves `public/` from the dev
   server; it does NOT copy it to `dist/` during `zfb build`. The
   `plugins/copy-public.mjs` postBuild plugin fills this gap.
   Remove the plugin once zfb adds native public-dir copy.

2. **Underscore pages skipped by router** — zfb skips any page whose filename
   starts with `_` (conventionally: framework internals). The shell template
   page (which must output to `dist/_shell/index.html`) is therefore named
   `pages/shell.tsx` instead of `pages/_shell.tsx`. The `plugins/copy-public.mjs`
   postBuild plugin renames `dist/shell/index.html` → `dist/_shell/index.html`.
   Remove this rename once zfb supports an opt-in escape hatch for
   underscore-prefixed pages (e.g., frontmatter `includeUnderscore: true`).

## Sentinels

`dist/_shell/index.html` contains two runtime substitution sentinels:
- `☃CCRESDOC_TITLE_SLOT☃` — inside `<title>`, replaced with the page title
- `☃CCRESDOC_CONTENT_SLOT☃` — inside `<main>`, replaced with rendered HTML

S5 (axum server) loads this file and string-replaces both sentinels at request time.
