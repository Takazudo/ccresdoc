# Node-free latest-toolchain compatibility decision

Status: the integrated application contract is implemented and verified for zfb
2.10.1 / zudo-doc 5.12.0. This document preserves the historical issue #93
architecture decision and its reproducible evidence; the current acceptance
commands and explicit host-only gaps are in
[`verification-matrix.md`](verification-matrix.md).

## Current integrated contract

- The published `@takazudo/zfb`, `@takazudo/zfb-runtime`,
  `@takazudo/zfb-md-wasm`, and five native carrier packages are pinned to
  `2.10.1`; `@takazudo/zudo-doc` is pinned to `5.12.0`. The app and the
  compatibility fixture each use a frozen lockfile and independently validate
  the installed tree.
- The host resolves the native package-root carrier at
  `node_modules/@takazudo/zfb-<platform>/zfb`, never the Node-shebang
  `node_modules/.bin/zfb` wrapper. The selected zfb config ends with
  `plugins: []`, so CCResDoc keeps its route adapters host-owned and does not
  start the package Node plugin host.
- Readiness is semantic: the Tauri host accepts `GET /docs/` only when it is
  successful and contains the generator-owned `Claude Resources` marker. `/`
  is the exact server-rendered alias, not a separate marketing home.
- The build stages a pruned, lockfile-faithful workspace at
  `src-tauri/runtime-workspace/app/`. Its refresh token is derived from a
  `sha256-tree-v1` digest covering staged bytes, generated theme assets, and the
  staging/digest implementation; the writable app-data copy is refreshed only
  when that token changes or its ready sentinel is absent.
- Runtime pruning excludes development tools, non-host carriers, disabled
  plugin/CLI dependencies, and audited Node-only edges including `esbuild` and
  `smol-toml`; the staged verifier asserts those exclusions before launch.
- `compatibility/node-free-latest/evidence/` is canonical: package facts,
  resolved configs, the four-way config matrix, and native-runtime observations
  are checked for drift before the complete frozen fixture check. The Linux
  staged probe proves HMR, zero plugin descriptors, zero Node-sentinel calls,
  process-group shutdown, and two serialized launches; it is not a macOS
  packaged WebView launch.

## Decision

Use zudo-doc's single-entry configuration to adopt its schema, collections, Markdown policy, and defaults, then override the plugin list after composition:

```ts
import { defineConfig } from "zfb/config";
import { zudoDoc } from "@takazudo/zudo-doc/config";

export default defineConfig({
  ...zudoDoc({
    siteName: "CCResDoc",
    siteDescription: "Browse Claude Code resources from your local ~/.claude/",
    port: 4892,
    base: "/",
    trailingSlash: true,
    docsDir: "src/content/docs",
    defaultLocale: "en",
    locales: {},
    versions: false,
    onBrokenMarkdownLinks: "warn",
    sidebarResizer: true,
    sidebarToggle: true,
    dynamicPageTransition: true,
    findInPage: true,
    claudeResources: false,
    codexResources: false,
    siteTreeNavIgnore: [],
    docHistory: false,
    llmsTxt: false,
    changelogs: false,
    packageOwnedRoutes: false,
    themePack: "default",
  }),
  plugins: [],
});
```

The final `plugins: []` is a security/runtime invariant, not a convenience. A regression assertion must inspect the composed object. `packageOwnedRoutes: false` alone is insufficient.

The preset warning that content exists while package routes are disabled is expected. CCResDoc must keep host-owned route files because every `@takazudo/zudo-doc/routes/*` entrypoint imports virtual modules created by `@takazudo/zudo-doc/plugins/routes`; enabling that plugin starts the Node plugin host. The host routes should use the public pure factories instead: `@takazudo/zudo-doc/route-context`, `chrome`, `home-page`, `doc-page-props`, `route-enumerators`, `mdx-components`, and the browser-safe `site-schema` helpers.

## Evidence and immutable inputs

The reproducible fixture is in `compatibility/node-free-latest/`. Its lockfile is independent of the current application migration. Run:

```sh
pnpm --dir compatibility/node-free-latest install --frozen-lockfile
pnpm --dir compatibility/node-free-latest run evidence:check
pnpm --dir compatibility/node-free-latest run assert:packages
pnpm --dir compatibility/node-free-latest run assert:configs
pnpm --dir compatibility/node-free-latest run probe:matrix
pnpm --dir compatibility/node-free-latest run check
pnpm --dir compatibility/node-free-latest run build
pnpm --dir compatibility/node-free-latest run probe:runtime
```

Committed evidence:

- `evidence/package-facts.json`: published package versions, npm integrity values, platform packages, and native binary facts.
- `evidence/resolved-configs.json`: resolved collections, Markdown configuration, and full plugin descriptors for all four candidates.
- `evidence/config-matrix.json`: native `zfb check` and build results plus Node-invocation evidence.
- `evidence/native-runtime.json`: port 4892 route serving, island marker, live content update, process sample, and silent failing sentinel.

The current canonical npm integrity values and native-carrier facts are in
`evidence/package-facts.json`; the app lockfile records the exact published
tarball resolutions. The immutable source anchors from issue #93 are retained
below as historical evidence rather than being treated as the current pin.

## Candidate results

| Candidate | Resolved plugins | Check | Build | Node during build | Decision |
| --- | --- | --- | --- | --- | --- |
| wholesale `zudoDoc()` | routes, search-index, theme-packs | pass | fails: injected chrome reaches optional `diff` | plugin host invoked | reject |
| `packageOwnedRoutes:false` | search-index, theme-packs | pass | pass | plugin host invoked | reject |
| selected spread override | none | pass | pass | zero | accept |
| fully manual zfb config | none | pass | pass | zero | viable control, reject duplicated policy |

The selected native Linux probe served `/` and `/docs/probe/`, emitted the `ProbeCounter` hydration marker and props, rebuilt after a watched MDX edit, sampled only the native zfb process, and recorded no call to the failing `node` sentinel. The integrated staged-runtime probe repeats this contract against the pruned application workspace. Real-WebView visual parity remains a documented macOS release gate.

## Dependency contract

Pin first-party packages exactly for the current integrated contract:

- `@takazudo/zfb`, `@takazudo/zfb-runtime`, and `@takazudo/zfb-md-wasm`: `2.10.1`.
- `@takazudo/zudo-doc`: `5.12.0`.
- Direct optional platform packages retained at `2.10.1`: `zfb-darwin-arm64`, `zfb-darwin-x64`, `zfb-linux-arm64-gnu`, `zfb-linux-x64-gnu`, `zfb-win32-x64-msvc`. pnpm installs only the matching host package, but explicit declarations keep the Tauri resolver and cross-platform package map stable.
- Reachable peers: `preact@10.29.1`, `preact-render-to-string@6.6.7`, `zod@4.3.6`, and `katex@0.16.22`. KaTeX is reachable even with `math:false` because `createMdxComponents()` imports the package `MathBlock` implementation.
- Build-only foundation: `tailwindcss@4.2.0`, `@tailwindcss/vite@4.2.0`, `typescript@5.9.2`. The downstream test harness uses `vitest@4.0.17` with `happy-dom@20.7.0`.

Remove the Cloudflare adapter. After the owning migrations remove local mirrors, remove direct `clsx`, `gray-matter`, `mermaid`, `remark-cjk-friendly`, `remark-directive`, and `@types/react`: the selected graph either no longer reaches them directly or provides the behavior in zfb/zudo-doc. Do not add optional peers `diff`, `@takazudo/zdtp`, or `@takazudo/zudo-doc-history-server`; those belong to disabled features/plugin routes. `@takazudo/zfb-md-wasm` remains pinned for zudo-doc family parity and browser-side Markdown facilities even though the minimal compatibility route does not execute it.

Use Node `>=22`, pnpm `>=10`, an exact `packageManager`, `nodeLinker: hoisted`, a frozen install, and `minimumReleaseAgeExclude` entries for every just-released first-party/platform pin. Hoisting remains required by the current Tauri copy/dereference staging design until issue #99 proves a replacement layout.

## Entrypoint contract

Usable with `plugins: []`:

- configuration/defaults: `config`, `settings`, `docs-schema`, `directive-vocabulary-defaults`, `i18n-defaults`, `color-schemes-defaults`;
- host route composition: `route-context`, `chrome`, `home-page`, `doc-page-props`, `route-enumerators`, `doc-route-paths`, `mdx-components`, `category-nav`;
- navigation/islands: `site-schema`, `sidebar-tree`, `sidebar-tree-island`, `sidebar-toggle-island`, `desktop-sidebar-toggle-island`, `site-tree-nav-island`, `tree-nav-shared`, `smart-break`, `sidebar-active-slug`, `current-path`;
- styling/types: `theme.css`, `safelist.css`, `content.css`, `page-loading.css`, `features.css`, `tsconfig.base.json`, `virtual-modules.d.ts`, and the official zfb config declarations.

Not usable under the invariant: `routes/*` (requires route-context virtual modules) and `plugins/*` (starts the Node host). Package route source files may be read as reference but must not be copied wholesale.

## File-by-file ownership matrix

| Current path/domain | Action | Replacement/constraint |
| --- | --- | --- |
| `app/zfb.config.ts` | replace | selected spread override above; assert final plugins are exactly `[]` |
| `app/zfb-shim.d.ts` | delete | official `zfb/config` declarations plus zudo-doc base/virtual declarations |
| `app/tsconfig.json` | replace | extend `@takazudo/zudo-doc/tsconfig.base.json`; Preact automatic JSX; keep `@/*`; no `@types/react` |
| `app/src/styles/global.css` | replace baseline, retain product overrides | import `theme.css`, `safelist.css`, `content.css`, `page-loading.css`, `features.css` in that order; retain only CCResDoc tokens, code presentation, focus/accessibility, and Tauri-specific overrides |
| `app/src/config/settings.ts` | keep as thin typed data | one host settings object shared by `zudoDoc()` and local `createRouteContext`; remove flat heading-ID option |
| `color-schemes.ts`, `color-scheme-utils.ts`, `i18n.ts`, `docs-schema.ts` | delete | package default exports; schema becomes package `.passthrough()` and gains current standard fields |
| `pages/index.tsx`, `pages/404.tsx`, `pages/docs/[[...slug]].tsx` | keep filenames, replace bodies | thin host-owned wrappers built from `createRouteContext`/`createChrome`; do not import `routes/*` |
| `pages/_data.ts` | delete | route-context `stableDocs`, nav-source, and route-enumerator APIs |
| `pages/_mdx-components.ts` | replace | thin `createMdxComponents()` binding; retain only the generated `CategoryNav categories={...}` bridge and genuinely host-only extras |
| `pages/lib/_*.ts(x)` | delete after consolidation | replace with at most `_route-context.ts` and `_chrome.ts` thin adapters using public factories |
| `src/types/docs-entry.ts`, `src/types/locale.ts` | delete | `site-schema`, `doc-page-props`, and settings-derived types |
| `src/utils/base.ts`, `docs.ts`, `slug.ts` | delete | route-context URL helpers, browser-safe `site-schema`, and `@takazudo/zudo-doc/slug` |
| `src/components/sidebar-tree.tsx`, `sidebar-toggle.tsx`, `tree-nav-shared.tsx`, `src/utils/smart-break.tsx` | delete after parity tests | package navigation/island/helper entrypoints listed above |
| `src/components/client-router-bootstrap.tsx` | delete | package chrome/body-end islands and `@takazudo/zfb-runtime/client-router` |
| `src/content/docs/**` | keep | Rust-generated/live content remains the source; representative committed fixture stays |
| `crates/ccresdoc-claude-md/**` | keep | in-process safe generator/watcher; only schema-contract deltas may change later |
| `src-tauri/src/main.rs` | keep | native package map/direct binary supervision; never use `.bin/zfb` |
| `src-tauri/tauri.conf.json` | keep | port 4892, writable resource copy, CSP/navigation contract; downstream packaging prunes by measured reachability |

Schema delta: use zudo-doc's standard passthrough schema. Existing `title`, `description`, `sidebar_position`, `sidebar_label`, `draft`, `unlisted`, `hide_sidebar`, `hide_toc`, `slug`, `generated`, and `category_no_page` remain valid. It additionally supports category/tag/pagination/wide/history/standalone/category ordering fields and preserves unknown host frontmatter. Heading IDs change from flat to mandatory hierarchical allocation; tests must cover duplicate headings and TOC anchors. The Rust generator does not need a format change for this gate.

## Native/Tauri facts and remaining verification

The current canonical fact for `@takazudo/zfb-darwin-arm64@2.10.1` is a
173,196,448-byte executable `zfb` at archive mode `0755`, with SHA-256
`795efa2f456fe6314925189e4bcd3b08b7603447a5c9adfa3695023b406cc2bc`. Its
runtime path is `app/node_modules/@takazudo/zfb-darwin-arm64/zfb`; the npm JS
wrapper is Node-based and forbidden at runtime. The package facts file records
the corresponding integrity values for all five published carriers. The
Mach-O, package extraction, staged-bundle, and real WebView launch assertions
run in the separate macOS-arm64 host gate; Linux must not claim that launch.

Historical issue #93 ran the native lifecycle on Linux x64. Its earlier package
facts and decision-gate outputs remain below as history; the integrated staged
probe and the separate macOS-arm64 host gate are the current evidence.

## Historical decision-gate records

The issue-numbered outputs below preserve the decisions and version facts that
led to the current architecture. They are historical records, not current
dependency instructions; use the current integrated contract and verification
matrix above for present-day work.

## Exact downstream `Decision gate output` replacements

### Issue #94

Decision gate output: Pin `@takazudo/zfb`, `@takazudo/zfb-runtime`, and `@takazudo/zfb-md-wasm` exactly to `2.7.1`, and `@takazudo/zudo-doc` exactly to `5.6.0`. Retain direct optional `@takazudo/zfb-{darwin-arm64,darwin-x64,linux-arm64-gnu,linux-x64-gnu,win32-x64-msvc}` pins at `2.7.1`; pnpm installs only the host match, but the manifest must preserve the Tauri package map. Direct reachable peers are `preact@10.29.1`, `preact-render-to-string@6.6.7`, `zod@4.3.6`, and `katex@0.16.22`; KaTeX is reachable through `createMdxComponents()`. Use `tailwindcss@4.2.0`, `@tailwindcss/vite@4.2.0`, and `typescript@5.9.2`; establish `vitest@4.0.17` + `happy-dom@20.7.0`. Remove `@takazudo/zfb-adapter-cloudflare`; remove direct `clsx`, `gray-matter`, `mermaid`, `remark-cjk-friendly`, `remark-directive`, and `@types/react` when their local consumers are removed. Do not add `diff`, `@takazudo/zdtp`, or `@takazudo/zudo-doc-history-server`. Require Node >=22, pnpm >=10, an exact packageManager, `nodeLinker: hoisted`, frozen install, and minimumReleaseAgeExclude entries for every first-party/platform pin. Commands: `pnpm install --frozen-lockfile`, pin/lock validator, `pnpm exec tsc --noEmit`, `pnpm exec zfb check`, Vitest, then native `zfb build`.

### Issue #95

Decision gate output: Implement `defineConfig({ ...zudoDoc({ siteName:"CCResDoc", siteDescription:"Browse Claude Code resources from your local ~/.claude/", port:4892, base:"/", trailingSlash:true, docsDir:"src/content/docs", defaultLocale:"en", locales:{}, versions:false, onBrokenMarkdownLinks:"warn", sidebarResizer:true, sidebarToggle:true, dynamicPageTransition:true, findInPage:true, claudeResources:false, docHistory:false, llmsTxt:false, changelogs:false, packageOwnedRoutes:false, themePack:"default" }), plugins:[] })`; assert the final resolved list is exactly empty. Use `@takazudo/zudo-doc/config`, `settings`, `docs-schema`, `directive-vocabulary-defaults`, `i18n-defaults`, and `color-schemes-defaults`. Retain zudo-doc defaults for hierarchical heading IDs, GFM task lists/footnotes/autolinks, directives, Mermaid, reading time, code enrichment/tabs, ruby, TOC export, image dimensions, link validation, class-mode highlighting, stripMdExt, and resolveMarkdownLinks. Delete `app/zfb-shim.d.ts`, extend `@takazudo/zudo-doc/tsconfig.base.json`, use Preact automatic JSX, and remove `@types/react`. In `global.css`, import `theme.css`, `safelist.css`, `content.css`, `page-loading.css`, then `features.css`; retain only CCResDoc tokens, code presentation, focus/accessibility, and Tauri overrides. Delete local color-scheme/default-i18n/docs-schema mirrors after consumers move. Test duplicate hierarchical anchors, directives, GFM, links, code classes, Mermaid markup, and zero plugins with `pnpm exec tsc --noEmit && pnpm exec zfb check && pnpm exec zfb build`.

### Issue #96

Decision gate output: Keep host-owned `pages/index.tsx`, `pages/404.tsx`, and `pages/docs/[[...slug]].tsx` because `plugins:[]` means `virtual:zudo-doc-*` modules do not exist; do not import `@takazudo/zudo-doc/routes/*`. Replace their bodies with thin adapters using `@takazudo/zudo-doc/route-context`, `chrome`, `home-page`, `doc-page-props`, `route-enumerators`, `doc-route-paths`, `mdx-components`, `category-nav`, and browser-safe `site-schema`. Consolidate `pages/lib/_*.ts(x)` to at most `_route-context.ts` and `_chrome.ts`; delete `_data.ts`, local route/data/chrome mirrors, `src/types/{docs-entry,locale}.ts`, and `src/utils/{base,docs,slug}.ts` only after parity. Replace `_mdx-components.ts` with `createMdxComponents()` plus the generated `<CategoryNav categories={...}>` bridge and genuine host extras. Virtual modules and package `routes/*` are unavailable by design. Required routes/states: `/`, `/404`, `/docs/`, `/docs/claude/`, a nested generated resource, a category_no_page root, draft/unlisted fixtures, and a missing URL; verify production and native dev rendering, duplicate-heading anchors, island markers/client navigation, and no fallback-render marker.

### Issue #97

Decision gate output: Target `@takazudo/zudo-doc/sidebar-tree-island`, `sidebar-toggle-island`, `desktop-sidebar-toggle-island`, `site-tree-nav-island`, `tree-nav-shared`, `smart-break`, `sidebar-active-slug`, `current-path`, `sidebar-utils`, and browser-safe `site-schema`; delete local `sidebar-tree.tsx`, `sidebar-toggle.tsx`, `tree-nav-shared.tsx`, and `smart-break.tsx` after parity. Retain only a thin adapter for CCResDoc's generated category grouping if package `nav-scope` cannot express it; do not retain duplicated tree keyboard/rendering logic. Automated DOM states: roving tabindex, Arrow/Home/End/Enter/Space, roles/labels, expanded/selected, focus restore, persisted open state, filter, active path after soft navigation, no duplicate handlers, mobile drawer/inert, theme toggle, smart path wrapping, and connector geometry. Run Vitest/happy-dom plus the issue's browser harness at desktop 1440x900 and narrow 390x844; manually inspect horizontal overflow and focus rings in both light/dark modes.

Issue #97 resolved this gate with direct package ownership and no generated-
category adapter. The historical zudo-doc 5.6.0 public sidebar used native links and
disclosure buttons rather than a WAI-ARIA tree, so roving tabindex and tree-level
Arrow/Home/End handling are not applicable to the selected upstream component.
The supported DOM states and this explicit deviation are guarded and documented
in `app/test/navigation-islands.test.tsx` and
`docs/architecture/sidebar-navigation.md`; a host keyboard/rendering fork was
not retained.

### Issue #98

Decision gate output: The final schema is `@takazudo/zudo-doc/docs-schema`'s passthrough schema. Existing generator fields remain valid: `title`, `description`, `sidebar_position`, `sidebar_label`, `draft`, `unlisted`, `hide_sidebar`, `hide_toc`, `slug`, `generated`, and `category_no_page`; current schema additionally accepts `category`, `tags`, `search_exclude`, pagination overrides, `wide`, `doc_history`, `standalone`, and `category_sort_order`, and preserves unknown fields. Routes remain `/docs/` plus hierarchical generated slugs; `category_no_page` emits navigation metadata without a page, draft stays unrouted, and unlisted stays routable but absent from navigation. Heading IDs are now hierarchical and require no generator frontmatter change. Keep the in-process Rust generator/watcher and `<CategoryNav categories={...}>`; update only the thin MDX binding if Task #96 requires it. Test unchanged-input idempotence, safe `~/.claude` scoping, category ordering/no-page behavior, generated markers, escaping, duplicate headings, changed-content HMR, and representative skills/commands/agents/CLAUDE.md fixtures.

### Issue #99

Decision gate output: Resolve macOS arm64 directly to `app/node_modules/@takazudo/zfb-darwin-arm64/zfb`, never `node_modules/.bin/zfb`. Package `2.7.1` contains a mode-0755 Mach-O arm64 binary, size 173246016, SHA-256 `35bfa2b2cf8ffc6b5ddefdf712155e02ad6aa5e947ffcf41ee57f8e48ff2d7a0`, npm integrity `sha512-fQUZIsEXxl35N12lKUBAd7QNZT5o7dKuZDHNkNe8IblP7zvw1uGqxJO7M4cQ2m+AITMCSmHK5lG2uI/vKgMPRA==`. Preserve the five-platform map, but claim lifecycle support only for tested macOS arm64. Stage a writable lockfile-faithful app workspace with the selected zero-plugin config, required package JS/CSS, Preact/runtime/Zod/KaTeX and matched native binary; exclude TypeScript, test/lint tools, Cloudflare adapter, non-host binaries, and disabled-plugin optional peers only after an actual pruned-copy reachability probe. Launch `zfb dev --port 4892`; prepend a recording/failing node sentinel, sample repeatedly through startup/route/HMR, assert zero plugin descriptors and zero sentinel calls, then verify readiness/retry/refresh-token/process-group shutdown. Reuse `pnpm --dir compatibility/node-free-latest run probe:runtime` as the Linux contract and add the macOS-arm64 Tauri/package counterpart.

### Issue #100

Decision gate output: Final matrix: frozen install and pin/lock/installed validation; `pnpm exec tsc --noEmit`; `pnpm exec zfb check`; Vitest/happy-dom frontend suites; production native `zfb build`; rendered `/`, `/404`, `/docs/`, `/docs/claude/`, nested generated resource, category_no_page, draft/unlisted, duplicate-heading/directive/GFM/Mermaid/code, and missing-route assertions; hydration/client-navigation/theme/sidebar keyboard/mobile/inert states; `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`; Rust generate/watch/live-smoke and unchanged-write checks; macOS-arm64 `cargo tauri build` plus packaged launch. Across startup, representative routes, and content HMR, use the recording/failing Node sentinel, repeated process samples, zero resolved plugins, and assert no `plugin-host.mjs`; inspect the pruned packaged workspace and direct `@takazudo/zfb-darwin-arm64/zfb` mode/path. Fixture commands are `pnpm --dir compatibility/node-free-latest install --frozen-lockfile`, `run assert:packages`, `run assert:configs`, `run probe:matrix`, `run check`, `run build`, and `run probe:runtime`. Visual checks: 1440x900 and 390x844, light/dark, focus rings, overflow, mobile drawer, theme toggle, hydrated counter/control interaction, and soft navigation. Close debt #54/#55/#56 only when final CSS evidence, type/check gates, and packaged dependency inspection respectively pass.
