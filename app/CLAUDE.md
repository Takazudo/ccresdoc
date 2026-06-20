# app/ — CCResDoc zfb frontend project

zudo-doc consumer project built by zfb. Output in `dist/` is served by `zfb dev` (sidecar, port 4892, node-free at runtime).

## Architecture

- **Framework**: Preact + zfb SSG
- **Package**: `@takazudo/zfb@0.1.0-next.53` (binary) + `@takazudo/zudo-doc@^0.2.4` (components)
- **Port**: 4892 (pinned in `zfb.config.ts`)
- **Node-free mode**: Zero `.mjs` plugins → no `plugin-host.mjs` spawned
- **Collections**: single `"docs"` collection at `src/content/docs/`

## Build

`node_modules` must be populated at setup time via `pnpm install` (Node at setup only — not at runtime). The native `@takazudo/zfb-<platform>/zfb` binary is then invoked via `pnpm exec zfb` — do NOT use `node_modules/.bin/zfb`, which is a Node-shebang wrapper that requires Node at runtime.

```sh
cd app
pnpm install          # once — populates node_modules incl. native zfb binary
pnpm exec zfb build   # node-free: invokes native binary directly
```

**`app/` is a STANDALONE pnpm project with `node-linker=hoisted`** (see `app/.npmrc`),
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

### @takazudo/zfb-adapter-cloudflare
This dep is a peer/runtime requirement imposed by zfb itself: zfb requires an adapter
to be declared even for local dev/build targets. The Cloudflare adapter is the
supported default for zudo-doc consumers. It does NOT mean the app is deployed to
Cloudflare — at runtime the Tauri host uses `zfb dev` locally with no adapter code
executed.

## Structure

```
app/
  zfb.config.ts           — zfb config (port 4892, node-free)
  package.json            — deps: @takazudo/zudo-doc + zfb devDep
  tsconfig.json           — paths: @/* → src/*
  zfb-shim.d.ts           — type shims for zfb/config, zfb/content
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
      settings.ts         — site settings (siteName, colorMode, headerNav, etc.)
      i18n.ts             — single-locale "en" helpers
      docs-schema.ts      — Zod schema for docs frontmatter
      color-schemes.ts    — light/dark color scheme definitions
      color-scheme-utils.ts — builds ColorSchemeProvider cssText
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
      global.css          — Tailwind CSS v4 + @theme tokens (from zudo-doc template)
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
