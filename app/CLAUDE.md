# app/ — CCResDoc zfb frontend project

zudo-doc consumer project built by zfb. Output in `dist/` is served by `zfb dev` (sidecar, port 4892, node-free at runtime).

The current source manager is the app-owned ordinary article at
`src/content/docs/ccresdoc/document-sources.mdx`. Its client island is
`src/document-sources/manager.tsx`; native Settings links to it and retains
app options and protected recovery. The checked-in docs home and manager are
the release-owned content. Rust generates every enabled source under its
validated unique mount, including the initial Claude and Codex mounts.
Source paths and the catalog stay in trusted native/editor state, not SSR
documents, frontmatter or search payloads. The app-owned manager and docs home define the release content surface.

## Architecture

- **Framework**: zudo-react islands + zfb SSG (no Preact)
- **Published toolchain**: `@takazudo/zfb@7.2.0` plus native carrier packages
  (the host resolves the matching package-root `zfb` binary) and
  `@takazudo/zudo-doc@14.0.0` (components)
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
`nodeLinker: hoisted` in `app/pnpm-workspace.yaml` (the only place it is set),
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

The audited tuple is written by hand in `scripts/lib/toolchain-contract.mjs`;
the validators compare manifests, lockfiles and installed trees against it, never
against values read back from the manifests. The first-party zfb packages and
all five native platform packages are pinned to `7.2.0`; zudo-doc is pinned to
`14.0.0`. The audited 7.2.0 family and 14.0.0 are exact-version `minimumReleaseAge` exclusions. Reachable runtime peers
are pinned to `zod@4.6.7` and `katex@0.19.1`. Preact, `preact-render-to-string`,
`@types/react` and the other packages in `FORBIDDEN_DIRECT_DEPENDENCIES` must not
be declared. The single zudo-doc patch is described in
`docs/architecture/zudo-doc-find-search-patch.md`.
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
  tsconfig.json           — extends zudo-doc's zudo-react base; paths: @/* → src/*
  pages/
    index.tsx             — home page
    404.tsx               — 404 page
    docs/
      [[...slug]].tsx     — catch-all docs route
    lib/
      _route-context.ts   — serializable host payload → zudo-doc RouteContext
      _chrome.tsx         — one createChrome() seam shared by host routes
      _settings-button.tsx — the docs Settings gear island
  src/
    config/
      settings.ts         — shared typed settings for config and host routes
    content/
      docs/               — MDX content root
        index.mdx         — app-owned docs landing page
        welcome.mdx       — placeholder page (draft: true; excluded from build)
        ccresdoc/         — app-owned source manager article
        <mount>/          — runtime-generated source namespace
    styles/
      global.css          — package CSS imports + CCResDoc accessibility overrides
```

Sidebar/tree rendering, mobile and desktop toggles, path-aware labels, active
route tracking, filtering, and connector geometry come directly from the public
`@takazudo/zudo-doc` navigation entry points. Do not add host copies of those
components; see `docs/architecture/sidebar-navigation.md` for the ownership and
accessibility contract.

### Load-control readiness

There is no host readiness marker. zudo-doc marks its own JS-only controls with
`data-zd-pending` until each island activates: the `ThemeToggle` button and the
`ThemePackSwitcher` `[data-switcher-launcher]`. CCResDoc's
`SettingsHeaderButton` follows the same convention and is `aria-disabled` while
pending. Browser checks wait for every selector in
`scripts/lib/load-controls.mjs` to be present with no `data-zd-pending`.

## MDX Content Contract

Rust writes generated MDX beneath `src/content/docs/<mount>/` in the
runtime workspace. Mount names come from the schema-v3 catalog. Built-in
rule sets keep their specialized format and category handling under each
mount. The released content contains the docs home and manager article. The
host scopes generation, watcher updates, and search entries with its private
ownership manifest.

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

Codex source formats are intentionally narrow: root/project `AGENTS.md`,
`config.toml`, agent TOML, `hooks.json`, rules, and skill packages. The walker
does not follow arbitrary links. A symlink is accepted only when it is the
direct entry under the configured `skills/` directory; the linked skill tree
is then copied into generated MDX under the selected Codex namespace. Managed
output directories and generated files are never symlinks and must remain
inside the docs root.

The host publishes one candidate transactionally. Failed generation or
watcher startup rolls back the previous published tree. Readiness checks the
manager and active mounts before navigating the WebView.

The packaged runtime is privacy-scoped. `scripts/stage-runtime-workspace.mjs`
copies an explicit file allowlist and generated theme assets, omits the build
`dist` tree, rejects generated source namespaces and private transition state, and audits staged text for synthetic fixture
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
