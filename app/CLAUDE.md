# app/ — CCResDoc zfb frontend project

zudo-doc consumer project built by zfb. Output in `dist/` is served by `zfb dev` (sidecar, port 4892, node-free at runtime).

## Architecture

- **Framework**: Preact + zfb SSG
- **Published toolchain**: `@takazudo/zfb@2.15.1` plus native carrier packages
  (the host resolves the matching package-root `zfb` binary) and
  `@takazudo/zudo-doc@5.17.2` (components)
- **Port**: 4892 (pinned in `zfb.config.ts`)
- **Node-free mode**: Zero `.mjs` plugins → no `plugin-host.mjs` spawned
- **Collections**: single `"docs"` collection at `src/content/docs/`

## Build

`node_modules` must be populated at setup time via `pnpm install --frozen-lockfile` (Node at setup only — not at runtime). Build/dev checks may use `pnpm exec zfb`, whose package wrapper spawns the native `@takazudo/zfb-<platform>/zfb` binary. The Tauri runtime must resolve that native binary directly — do NOT use `node_modules/.bin/zfb`, which is a Node-shebang wrapper that requires Node at runtime.

```sh
cd app
pnpm install --frozen-lockfile  # once — populates node_modules incl. native zfb binary
pnpm run validate:dependencies  # manifest/lock/installed-tree contract
pnpm run validate:theme-packs
pnpm run typecheck
pnpm run check:zfb
pnpm run test:run
pnpm run test:theme-packs
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
`2.15.1`; zudo-doc is pinned to `5.17.2`. Reachable runtime peers are pinned to
`preact@10.29.1`, `preact-render-to-string@6.6.7`, `zod@4.3.6`, and `katex@0.16.22`.
The Cloudflare adapter and legacy local Markdown mirrors are intentionally absent:
zudo-doc supplies the selected Markdown pipeline while CCResDoc's Rust generator
owns content generation.

The final zfb configuration has `plugins: []`, so route files under `pages/` are
host-owned adapters built from public zudo-doc factories. Runtime resolution uses
the direct package-root native carrier (`node_modules/@takazudo/zfb-<platform>/zfb`),
not `node_modules/.bin/zfb`; Node is needed for install and build checks only.

## Structure

```
app/
  zfb.config.ts           — zudoDoc config with a final plugins: [] override
  package.json            — deps: @takazudo/zudo-doc + zfb devDep
  tsconfig.json           — extends zudo-doc's Preact base; paths: @/* → src/*
  pages/
    index.tsx             — home page
    404.tsx               — 404 page
    docs/
      [[...slug]].tsx     — catch-all docs route
    lib/
      _route-context.ts   — serializable host payload → zudo-doc RouteContext
      _chrome.ts          — one createChrome() seam shared by host routes
  src/
    config/
      settings.ts         — shared typed settings for config and host routes
    content/
      docs/               — MDX content root
        index.mdx         — routed resource-category landing page
        welcome.mdx       — placeholder page (draft: true; excluded from build)
        claude*/          — selected Claude detail/status MDX (gitignored)
        codex*/           — selected Codex detail/status MDX (gitignored)
    styles/
      global.css          — package CSS imports + CCResDoc accessibility overrides
```

Sidebar/tree rendering, mobile and desktop toggles, path-aware labels, active
route tracking, filtering, and connector geometry come directly from the public
`@takazudo/zudo-doc` navigation entry points. Do not add host copies of those
components; see `docs/architecture/sidebar-navigation.md` for the ownership and
accessibility contract.

### Load-control readiness marker

`AppearanceBridge` sets the host-owned
`data-ccresdoc-load-controls-ready` attribute on `<html>` one animation frame
after it mounts. The marker means only that synchronous `when: "load"` islands
have completed their mount pass; it is deliberately not named or treated as a
general hydration marker. Until it appears, host CSS gives the JS-only
`ThemeToggle`, `ThemePackSwitcher`, and `SettingsHeaderButton` controls a pending
visual and pointer affordance without affecting progressively enhanced links.
This CSS does not provide keyboard or ARIA disabled semantics.

Dynamic page transitions remove the attribute during each body swap and the
remounted bridge sets it again. Do not add it to
`zfb-preserve-html-attrs`: the incoming load islands really are pending during
that interval. Bridge cleanup cancels its pending animation frame and removes
the marker so a torn-down page cannot leave or re-arm stale readiness state.

## MDX Content Contract

The Rust generator (`crates/ccresdoc-claude-md`) writes selected MDX to
`src/content/docs/claude*/` and `src/content/docs/codex*/`. These detail/status
directories are **gitignored** — they are rebuilt on every app launch by the
Tauri host (`src-tauri/`) running the generator and coordinator in-process.
The checked-in `claude/index.mdx` and `codex/index.mdx` files are permanent
generic landing shells; they contain no user content and are the only resource
inputs admitted to the packaged workspace.

### Directory layout

```
src/content/docs/
  claude/                    ← overview category (no route)
    index.mdx                 ← permanent landing shell, sidebar_position: 899
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
  codex/                     ← permanent overview category (routed)
  codex-agents-md/           ← AGENTS.md hierarchy, position 905
  codex-config/              ← config.toml, position 906
  codex-agents/              ← agent TOML definitions, position 907
  codex-hooks/               ← hooks.json and readable hook files, position 908
  codex-rules/               ← *.rules files, position 909
  codex-skills/              ← skills and controlled direct package links, position 910
```

The permanent top-level header categories are `Claude` at position 899 and
`Codex` at position 904. Claude detail categories occupy 900–903; Codex detail
categories occupy 905–910. The overview shells are always present so all four
selection states have a stable navigation surface; disabled sources publish a
coordinator-owned disabled marker and no detail namespace.

### Frontmatter fields

All generated MDX files use a subset of the `DocsData` schema:

```yaml
---
title: string          # required — page title
description: string    # optional — card description
sidebar_position: number  # required — controls sidebar order
sidebar_label: string  # optional — override sidebar display label
generated: true        # marks generator output
category_no_page: true # set ONLY on category index.mdx files
---
```

### Category index pattern

Category `index.mdx` files use `category_no_page: true` so the sidebar
renders them as non-linked headers (no route is built for them). The
path is NOT emitted as a docs page.

### Coordinator overview pages

The checked-in routed `docs/index.mdx` includes a `<CategoryNav>` component
that renders category cards. The coordinator-owned `claude/index.mdx` and
`codex/index.mdx` shells are routed overview pages; generated detail category
`index.mdx` files remain `category_no_page` metadata and do not emit routes:

```mdx
---
title: Claude Resources
description: Browse selected Claude resources.
---

<CategoryNav categories={["claude-md", "claude-commands", "claude-skills", "claude-agents"]} />
```

The wrapper produced by zudo-doc's MDX factory resolves those slug strings
against the current package-owned navigation tree.

Codex source formats are intentionally narrow: root/project `AGENTS.md`,
`config.toml`, agent TOML, `hooks.json`, rules, and skill packages. The walker
does not follow arbitrary links. A symlink is accepted only when it is the
direct entry under the configured `skills/` directory; the linked skill tree
is then copied into generated MDX under the selected Codex namespace. Managed
output directories and generated files are never symlinks and must remain
inside the docs root.

The host generates only enabled sources, prunes stale detail output, and
publishes one candidate transactionally. A failed generation or watcher
transition rolls back the previous selected tree; readiness waits for the
neutral shell plus both Claude/Codex overview markers for the same generation.

The packaged runtime is privacy-scoped. `scripts/stage-runtime-workspace.mjs`
copies an explicit file allowlist and generated theme assets, omits the build
`dist` tree, rejects all `claude-*`/`codex-*` detail/status paths and
`.ccresdoc-*` transition state, and audits staged text for synthetic fixture
sentinels and checkout paths. Node-only packages, `.bin` wrappers, and
non-host native carriers remain excluded; the direct native zfb binary is the
only runtime launcher.

### Route building

The `[[...slug]].tsx` catch-all route filters `category_no_page: true`
entries so no page is built for category headers. Generated resource pages must
not set `draft: true`; only the checked-in placeholder stub is a draft.

### Content escaping

MDX bodies must escape or avoid sequences that break MDX parsing:
- `<`, `>` in prose → use HTML entities
- `{`, `}` in prose → wrap in backticks or JSX expression `{'{'}`
- Backtick content inside code fences is safe
