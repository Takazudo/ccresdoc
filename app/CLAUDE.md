# app/ — CCResDoc zfb frontend project

zudo-doc consumer project built by zfb. Output in `dist/` is served by `zfb dev` (sidecar, port 4892, node-free at runtime).

## Architecture

- **Framework**: Preact + zfb SSG
- **Package**: `@takazudo/zfb@2.7.1` (binary) + `@takazudo/zudo-doc@5.6.0` (components)
- **Port**: 4892 (pinned in `zfb.config.ts`)
- **Node-free mode**: Zero `.mjs` plugins → no `plugin-host.mjs` spawned
- **Collections**: single `"docs"` collection at `src/content/docs/`

## Build

`node_modules` must be populated at setup time via `pnpm install --frozen-lockfile` (Node at setup only — not at runtime). Build/dev checks may use `pnpm exec zfb`, whose package wrapper spawns the native `@takazudo/zfb-<platform>/zfb` binary. The Tauri runtime must resolve that native binary directly — do NOT use `node_modules/.bin/zfb`, which is a Node-shebang wrapper that requires Node at runtime.

```sh
cd app
pnpm install --frozen-lockfile  # once — populates node_modules incl. native zfb binary
pnpm run validate:dependencies  # manifest/lock/installed-tree contract
pnpm run typecheck
pnpm run check:zfb
pnpm run test:run
pnpm exec zfb build   # setup/build check: wrapper spawns native binary
```

**`app/` is a STANDALONE pnpm project with a hoisted node-linker** — set as
`nodeLinker: hoisted` in `app/pnpm-workspace.yaml` (pnpm 10+/11 no longer reads
`node-linker` from `app/.npmrc`, which is kept only as a legacy/back-compat marker),
NOT a workspace member. This is required for the bundled `.app`: the Tauri host
bundles `app/node_modules` and copies it (dereferencing symlinks) into a writable
workspace at runtime. pnpm's default isolated `.pnpm` store does not survive that
dereferencing copy — transitive deps like `hono` (via `@takazudo/zfb-runtime`)
become unresolvable and the zfb renderer is disabled (every content page 404s). A
flat hoisted `node_modules` copies cleanly. The platform binary packages
(`@takazudo/zfb-<platform>`) are declared as `optionalDependencies` so the host can
resolve `node_modules/@takazudo/zfb-<platform>/zfb` directly.

## Dependency notes

### zfb pin
All `@takazudo/zfb*` packages in `package.json` (deps, optionalDeps) are pinned to
the same version and must move in lockstep — they are released together. There is no
single-source mechanism in JSON; `scripts/check-zfb-pin.sh` is the enforcement gate
(run by `scripts/run-b4push.sh` step 1). When bumping the pin, update every
`@takazudo/zfb*` entry simultaneously.

### Published toolchain contract
The first-party zfb packages and all five native platform packages are pinned to
`2.7.1`; zudo-doc is pinned to `5.6.0`. Reachable runtime peers are pinned to
`preact@10.29.1`, `preact-render-to-string@6.6.7`, `zod@4.3.6`, and `katex@0.16.22`.
The Cloudflare adapter and legacy local Markdown mirrors are intentionally absent:
zudo-doc supplies the selected Markdown pipeline while CCResDoc's Rust generator
owns content generation.

## Structure

```
app/
  zfb.config.ts           — zudoDoc config with a final plugins: [] override
  package.json            — deps: @takazudo/zudo-doc + zfb devDep
  tsconfig.json           — extends zudo-doc's Preact base; paths: @/* → src/*
  pages/
    index.tsx             — home page
    404.tsx               — 404 page
    _data.ts              — zfb collection → DocsEntry bridge
    _mdx-components.ts    — MDX component map (CategoryNav, admonitions, etc.)
    docs/
      [[...slug]].tsx     — catch-all docs route
    lib/
      _head-with-defaults.tsx     — <head> slot with ColorSchemeProvider
      _header-with-defaults.tsx   — site header wrapper
      _footer-with-defaults.tsx   — minimal footer wrapper
      _sidebar-with-defaults.tsx  — SidebarTree island wrapper
      _body-end-islands.tsx       — ClientRouterBootstrap island
      _compose-meta-title.ts      — "<page> | CCResDoc" title helper
  src/
    config/
      settings.ts         — temporary host-route adapter (removed by issue #96)
      i18n.ts             — temporary single-locale adapter over package data
    types/
      docs-entry.ts       — DocsEntry interface
      locale.ts           — LocaleLink (single-locale stub)
    utils/
      base.ts             — withBase, stripBase, navHref, docsUrl
      slug.ts             — toRouteSlug, toSlugParams
      docs.ts             — NavNode type + buildNavTree (SidebarNode → NavNode bridge)
      smart-break.tsx     — smart word-break for path-like labels
    components/
      sidebar-tree.tsx    — SidebarTree island (filter + tree nav)
      sidebar-toggle.tsx  — mobile hamburger + slide-in aside
      tree-nav-shared.tsx — connector lines, icons shared by sidebar components
      client-router-bootstrap.tsx — SPA router activation island
    content/
      docs/               — MDX content root
        welcome.mdx       — placeholder page (draft: true; excluded from build)
        claude*/          — Wave 2 generated (gitignored — see below)
    styles/
      global.css          — package CSS imports + CCResDoc accessibility overrides
```

## MDX Content Contract (Wave 2)

The Rust generator (`crates/ccresdoc-claude-md`) writes MDX to
`src/content/docs/claude*/`. These directories are **gitignored** — they are
rebuilt on every app launch by the Tauri host (`src-tauri/`) running the
generator in-process.

### Directory layout

```
src/content/docs/
  claude/                    ← overview category (no route)
    index.mdx                ← category_no_page: true, sidebar_position: 899
  claude-md/                 ← CLAUDE.md category
    index.mdx                ← category_no_page: true, sidebar_position: 900
    global.mdx               ← ~/.claude/CLAUDE.md
    project-<name>.mdx       ← per-project CLAUDE.md
  claude-commands/           ← commands category
    index.mdx                ← category_no_page: true, sidebar_position: 901
    <command-name>.mdx       ← one file per command
  claude-skills/             ← skills category
    index.mdx                ← category_no_page: true, sidebar_position: 902
    <skill-name>.mdx         ← one file per skill
  claude-agents/             ← agents category
    index.mdx                ← category_no_page: true, sidebar_position: 903
    <agent-name>.mdx         ← one file per agent
```

### Frontmatter fields

All generated MDX files use a subset of the `DocsData` schema:

```yaml
---
title: string          # required — page title
description: string    # optional — card description
sidebar_position: number  # required — controls sidebar order
sidebar_label: string  # optional — override sidebar display label
generated: true        # marks file as auto-generated (Wave 2)
category_no_page: true # set ONLY on category index.mdx files
---
```

### Category index pattern

Category `index.mdx` files use `category_no_page: true` so the sidebar
renders them as non-linked headers (no route is built for them). The
path is NOT emitted as a docs page.

### Claude overview page

`claude/index.mdx` includes a `<CategoryNav>` component that renders
the category cards:

```mdx
---
title: Claude Resources
sidebar_position: 899
category_no_page: true
generated: true
---

<CategoryNav categories={["claude-md", "claude-commands", "claude-skills", "claude-agents"]} />
```

The `CategoryNavWrapper` in `pages/_mdx-components.ts` resolves the
slug strings to `NavNode[]` from the built sidebar tree.

### Route building

The `[[...slug]].tsx` catch-all route filters `category_no_page: true`
entries so no page is built for category headers. Wave 2 must not set
`draft: true` on any content page (only on placeholder stubs).

### Content escaping

MDX bodies must escape or avoid sequences that break MDX parsing:
- `<`, `>` in prose → use HTML entities
- `{`, `}` in prose → wrap in backticks or JSX expression `{'{'}`
- Backtick content inside code fences is safe
