# Phase 1 Parity Report

- Source SHA captured at: 2026-05-05
- ccresdoc commit: 7814c3533582c4935976cee31b1f0f190a83b5b8 (branch: doc-theme-restore-no-node/s6-confirm)
- ~/.claude/doc/ commit: 9916c4c10077f2c0d9be8029e9b3fa4a5fdf71bf
- Verification surface: static analysis only (per workflow rule); browser screenshots deferred to manager dispatch

---

## A. DOM Structure

### Page: home

ccresdoc dist/index.html: A zfb-rendered SSG page with the DefaultLayout shell. The outer structure is:
`html > head > [scripts + style#zd-color-scheme + link(css) + script(module)] > body.min-h-screen.antialiased`

Body children in order:
1. `header.sticky.top-0.z-50` — site header with logo + ThemeToggle island
2. `aside#desktop-sidebar.hidden.lg:block.fixed` — sidebar with Sidebar island
3. `div[data-zfb-island="DesktopSidebarToggle"]` — sidebar toggle island
4. `div.lg:ml-[var(--zd-sidebar-w)].zd-sidebar-content-wrapper` — main wrapper
   - `div.flex.min-h-[calc(100vh-3.5rem)].justify-center`
     - `div.flex.w-full.gap-[...].max-w-[...]`
       - `main.flex-1.min-w-0.px-hsp-xl.py-vsp-xl.lg:px-hsp-2xl.lg:py-vsp-2xl`
         - MobileToc island
         - `article.zd-content.max-w-none` with content
       - Toc island
   - `footer.border-t.border-muted.bg-surface`
5. `div.code-block-sr-announce[aria-live="polite"]` — screen reader announce div
6. CodeBlockEnhancer inline script
7. TabsInit inline script
8. MermaidInit inline script
9. PageLoadingOverlay style + div

No deltas on home page — no equivalent home page in original to compare against. ccresdoc has a distinct index.tsx placeholder page.

### Page: doc with code blocks

ccresdoc test-content page renders (from dist/test-content/index.html):
- article.zd-content wraps a nested div.zd-content containing rendered markdown
- Code blocks: `div.code-title` + `pre > code.language-{lang}` (plain code, no syntax colors — renderer is Rust syntect at runtime, not Shiki at build)
- Original doc uses: `div.code-block-title` + `pre.astro-code[style][data-language]` with inline Shiki span tokens

Delta: class names differ: ccresdoc uses `.code-title`; original uses `.code-block-title`. Both are styled (ccresdoc added `.code-title` rule to global.css). No visual delta expected since both rules produce the same visual output (mono font, muted color, code-bg background, muted border, no bottom border, vsp-2xs/hsp-lg padding).

Delta: ccresdoc pre tags have `class="language-{lang}"` without `.astro-code`. Original uses `class="astro-code catppuccin-latte"` with inline Shiki span tokens. This is an intentional architectural difference: syntax highlighting happens server-side at runtime by syntect (not build-time by Shiki). The Shiki dual-theme CSS rule `[data-theme] .shiki, .astro-code span` in global.css does not apply to ccresdoc's pre tags. Syntect renders colors directly into the token spans.

### Page: doc with admonitions

ccresdoc renders: `aside.admonition.admonition-{type}` with first-child p as title.
Original renders: Astro component `<div data-admonition>` with inline style for border-color/background-color/color.

Delta: element tag differs — ccresdoc uses `aside`, original uses `div`. Both have semantic content. The CSS selectors in global.css for ccresdoc target `.admonition` class directly, not `[data-admonition]`. No overlap issue. The color/background approach is also different: ccresdoc uses static CSS classes (`.admonition-note`, `.admonition-tip`, etc.) with `color-mix()` backgrounds; original uses inline styles computed in Astro frontmatter. End result is functionally identical color output.

### Page: doc with tables

ccresdoc renders standard HTML table: `table > thead > tr > th` / `tbody > tr > td`.
Original same. `.zd-content :where(table)`, `.zd-content :where(th)`, `.zd-content :where(td)` selectors apply in both.

No deltas.

### Page: 404

ccresdoc dist/404.html: Standard DefaultLayout shell with `article.zd-content` containing a "Page not found" h1 and links.
Original dist/404.html: Similar structure under doc-layout.

No structural deltas.

---

## B. Stylesheet Comparison

### @theme block

Lines 1–139 of ccresdoc app/styles/global.css are token-for-token identical to lines 1–139 of ~/.claude/doc/src/styles/global.css. Verified by side-by-side read. All 4 base tokens, 16 raw palette tokens, 20 semantic alias tokens, all spacing tokens, all typography tokens, all radius tokens, and all breakpoints match exactly.

@theme block hex literal count: 0 (confirmed — no hex literals, only var() references).

### Content rules (lines 141–841)

Lines 141–841 of ccresdoc's global.css are token-for-token identical to lines 141–841 of the original. All .zd-content selectors, code-block selectors, Shiki selectors, sidebar selectors, and animation keyframes match exactly.

### Additions in ccresdoc beyond the original (lines 842+)

These are ccresdoc-specific additions that cover Rust renderer output:

- `.heading-link` rules — mirrors `.hash-link` from original but for the heading anchor class emitted by comrak (ccresdoc's Rust markdown renderer). Correct: original's `hash-link` CSS rule is also present at lines 233–253 and provides identical visual output.
- `.code-title` rules — matches original's `.code-block-title` rules exactly in computed style, just different class name.
- `.admonition` and `.admonition-{type}` rules — replicate original's inline-style approach in static CSS.
- `.zd-content :where(details)` and `.zd-content :where(summary)` rules — original had a `details.astro` component that managed this; ccresdoc handles in CSS.
- `.tabs-container`, `.tabs-list`, `.tabs-tab`, `.tabs-panel` rules — original used Astro tabs.astro component with different class names (`[data-tabs]`, `.tab-panel`, `.tabs-nav`). Delta in tabs class names — see Must-fix section.

### Selectors from original MISSING in ccresdoc

- `.code-block-container` and `.code-block-container pre.astro-code` — present in original at lines 554–573 and also present in ccresdoc at lines 554–573 (identical). No gap.
- `.code-block-wrapper`, `.code-btn`, `.code-btn-copy`, `.code-btn-wrap` etc. — present in both.
- `pre.astro-code.word-wrap` rules — present in both originals. ccresdoc's code enhancer script targets `pre.astro-code` — potential functional gap since ccresdoc's Rust renderer emits `pre.language-{lang}` without `.astro-code`. The enhancer will not attach copy/wrap buttons to ccresdoc content code blocks.

---

## C. Component Check

| Original Component          | ccresdoc Equivalent           | Status            |
|-----------------------------|-------------------------------|-------------------|
| header.astro                | header.tsx                    | Match (token classes identical; missing headerNav bar and Search — acceptable for Phase 1 |
| footer.astro                | footer.tsx                    | Match (token classes identical; footer content is hardcoded rather than config-driven — acceptable) |
| sidebar.astro               | sidebar.tsx                   | Match (token classes via SidebarTree identical; data source is /api/manifest.json instead of build-time nav — acceptable) |
| sidebar-tree.tsx            | sidebar-tree.tsx              | Match (all token classes identical; ccresdoc version lacks `astro:after-swap` listener — acceptable for zfb) |
| tree-nav-shared.tsx         | tree-nav-shared.tsx           | Match (identical) |
| toc.tsx                     | toc.tsx                       | Match (token classes identical; ccresdoc version lacks onClick activate — acceptable) |
| mobile-toc.tsx              | mobile-toc.tsx                | Match (token classes identical) |
| desktop-sidebar-toggle.tsx  | desktop-sidebar-toggle.tsx    | Match (identical logic, same SIDEBAR_STORAGE_KEY) |
| theme-toggle.tsx            | theme-toggle.tsx              | STORAGE_KEY differs: original="zudo-doc-theme", ccresdoc="ccresdoc.theme". Same DOM/classes |
| color-scheme-provider.astro | color-scheme-provider.tsx     | Match (CSS output verified identical in built shell HTML) |
| breadcrumb.astro            | breadcrumb.tsx                | Match (token classes identical) |
| doc-frontmatter.astro       | doc-frontmatter.tsx           | Match (token classes identical) |
| doc-metainfo.astro          | doc-metainfo.tsx              | Match (token classes identical) |
| doc-tags.astro              | doc-tags.tsx                  | Match (token classes identical) |
| page-loading-overlay.astro  | page-loading-overlay.tsx      | Match (CSS output identical; JS hook is no-op in zfb — acceptable) |
| mermaid-init.astro          | mermaid-init.tsx              | Match (same themeVariables, same --zd-* variable references) |
| code-block-enhancer.astro   | code-block-enhancer.tsx       | PARTIAL — enhancer targets `pre.astro-code`; ccresdoc renderer emits `pre.language-{lang}` (no `.astro-code` class). Copy/wrap buttons won't attach at runtime. |
| tabs-init.astro             | tabs-init.tsx                 | Tabs class names differ. Original uses `[data-tabs]`, `.tab-panel`, `.tabs-nav`. ccresdoc uses `.tabs-container`, `.tabs-list`, `.tabs-tab`, `.tabs-panel`. JS init targets `[data-tabs]` selector — won't match ccresdoc renderer's tabs markup. |
| admonitions/admonition.astro| (CSS only in global.css)      | Different approach but equivalent visual output |
| search.astro                | (not ported)                  | Out-of-scope for Phase 1 |
| ai-chat-modal.tsx           | (not ported)                  | Out-of-scope for Phase 1 |
| doc-history.tsx             | (not ported)                  | Out-of-scope for Phase 1 |

---

## D. Layout Shell

### ccresdoc default.tsx vs original doc-layout.astro

Body classes: both have `class="min-h-screen antialiased"` — match.

Pre-paint scripts:
- Original: two separate `is:inline` scripts for sidebarResizer and sidebarToggle, gated by `settings.sidebarResizer` / `settings.sidebarToggle` flags.
- ccresdoc: three inline scripts: THEME_BOOTSTRAP_SCRIPT, SIDEBAR_WIDTH_SCRIPT, SIDEBAR_VISIBILITY_SCRIPT — always present (not gated).
- Functional parity: sidebar width/visibility restoration scripts are identical content. Theme bootstrap script is ccresdoc-specific (original uses ClientRouter + astro:after-swap lifecycle; ccresdoc needs its own data-theme setter).

Outer container structure:
- Original: `div[class:list]` → `div.flex.min-h...justify-center` → `div.flex.w-full.gap-...max-w-...` → `main + Toc`
- ccresdoc: identical structure and class values.

Sidebar aside: class names identical — `hidden lg:block fixed top-[3.5rem] left-0 z-30 w-[var(--zd-sidebar-w)] h-[calc(100vh-3.5rem)] overflow-y-auto bg-bg border-r border-muted pb-vsp-xl`

Main element: class names identical — `flex-1 min-w-0 px-hsp-xl py-vsp-xl lg:px-hsp-2xl lg:py-vsp-2xl`

Article: `class="zd-content max-w-none"` — match.

MobileToc placement: both place MobileToc immediately inside `<main>` before `<article>` — match.

TOC placement: both place `Toc` as sibling to `<main>` inside the inner flex row — match.

Footer placement: both place `Footer` inside the sidebar-content-wrapper div, after the flex row — match.

init scripts: both place CodeBlockEnhancer, TabsInit, MermaidInit, PageLoadingOverlay after the wrapper div — match.

Sidebar resizer drag handle: Original has a full inline `<script>` for a sidebar drag resize handle. ccresdoc does NOT include this resizer script. The `--zd-sidebar-w` CSS variable is still user-adjustable via localStorage restore, but drag-to-resize is missing. This is a functional regression but borderline cosmetic — the sidebar shows and hides correctly.

ClientRouter: Original uses Astro's `<ClientRouter />` for view transitions. ccresdoc has no equivalent (zfb uses real browser navigations). No DOM delta from this — it's just how navigation works differently.

Sidebar scroll restore: Original has an `astro:before-swap / astro:after-swap` script to save/restore sidebar scroll position on SPA navigation. ccresdoc does not (navigation is full-page; scroll restore is handled by the browser). Acceptable.

---

## E. Color Schemes / Palette

### --zd-* variables in built shell HTML

All --zd-* variables are confirmed present in the built shell's inline style block (verified against the S0 reference tables):

Light scheme (html[data-theme="light"]): --zd-bg, --zd-fg, --zd-cursor, --zd-sel-bg, --zd-sel-fg, --zd-0 through --zd-15, --zd-surface, --zd-muted, --zd-accent, --zd-accent-hover, --zd-code-bg, --zd-code-fg, --zd-success, --zd-danger, --zd-warning, --zd-info, --zd-mermaid-node-bg, --zd-mermaid-text, --zd-mermaid-line, --zd-mermaid-label-bg, --zd-mermaid-note-bg, --zd-chat-user-bg, --zd-chat-user-text, --zd-chat-assistant-bg, --zd-chat-assistant-text — 29 variables total. ALL MATCH reference hex values exactly.

Dark scheme (html[data-theme="dark"]): Same 29 variables. ALL MATCH reference hex values exactly.

### --color-* variables in @theme block

All --color-* aliases confirmed present in ccresdoc's global.css @theme block, lines 30–72. All 34 --color-* aliases (4 base + 16 raw palette + 14 semantic aliases) match the S0 reference exactly.

### Palette hex values verified

Light scheme: --zd-accent=#a35e0f (p5 override), --zd-surface=#ece9e9 (p10), --zd-muted=#6b6b6b (p8), --zd-bg=#e2ddda (p9) — all match.
Dark scheme: --zd-accent=#d69a66 (p12 override), --zd-surface=#1c1c1c (p0 override), --zd-muted=#888888 (p8), --zd-bg=#181818 (p9) — all match.

No hex literals in @theme block: confirmed (zero occurrences).

---

## Delta Classification

### Must-fix

1. **code-block-enhancer targets wrong selector**: The enhancer JS script queries `pre.astro-code` but ccresdoc's Rust renderer emits `pre.language-{lang}` (no `.astro-code` class). Copy and word-wrap buttons will never attach to content code blocks at runtime.

2. **tabs-init targets wrong selector**: The tabs init JS script uses `[data-tabs]`, `.tab-panel`, `.tabs-nav` selectors, but the ccresdoc test-content fixture has `.tabs-container`, `.tabs-list`, `.tabs-tab`, `.tabs-panel` in its HTML. The JS will not find any tabs containers and tabs will be non-interactive.

### Acceptable

1. **Header: no headerNav bar and no Search widget** — original header has a horizontal nav bar and Search component. ccresdoc header has only logo + ThemeToggle. Search and multi-section nav are explicitly out of scope for Phase 1.

2. **theme-toggle storage key differs**: ccresdoc uses `"ccresdoc.theme"`, original uses `"zudo-doc-theme"`. This is intentional — ccresdoc is a separate app; reusing the original's localStorage key would cause conflicts if both apps run in the same browser profile.

3. **Admonition element: aside vs div** — ccresdoc uses `aside.admonition` (semantically correct for supplemental content), original uses `div[data-admonition]` with inline styles. Visual output is identical via CSS class approach. Semantically better.

4. **Page loading overlay JS is a no-op**: The overlay DOM and CSS are present for future use; zfb real-navigation pages use browser-native loading feedback. Consistent with architecture.

5. **Sidebar drag-resize handle missing**: The original has a pointerdown drag-resize JS block. ccresdoc omits it. Sidebar width is still restored from localStorage. Drag-resize is a quality-of-life feature, not a visual parity issue.

6. **Footer content is hardcoded**: ccresdoc footer has hardcoded "Links" column with Claude Code + GitHub links, while original reads from `settings.footer`. Token classes and structure are identical.

7. **Code block selector `pre.astro-code` in shared Shiki CSS rules**: ccresdoc renderer does not use Shiki (uses syntect at runtime), so the `[data-theme] .astro-code span` color rules are inactive. Syntect emits its own inline token spans. Acceptable — separate rendering path.

8. **`.code-block-title` vs `.code-title`**: Both rules produce identical visual output. The class name difference exists because the Rust renderer uses a different class name from Astro's code-block-enhancer.

9. **SidebarTree lacks `astro:after-swap` listener**: ccresdoc uses real browser navigations, so View Transition swap events do not fire. The active slug is determined on initial load via window.location.pathname. Acceptable.

### Out-of-scope

1. Search component (search.astro) — pagefind-based search is explicitly not in Phase 1.
2. AI chat modal (ai-chat-modal.tsx) — not in Phase 1.
3. Doc history / diff viewer (doc-history.tsx) — not in Phase 1.
4. Version switcher and versioned docs UI — not in Phase 1.
5. Edit link, category nav, category tree nav — not in Phase 1.
6. HTML preview wrapper — not in Phase 1.
7. Multiple locale support (i18n) — ccresdoc is single-locale; original is multi-locale.

### Suspected visual deltas (browser-screenshot recheck recommended)

1. **Syntax highlighting appearance**: ccresdoc uses syntect (Rust, server-side at runtime) for code highlighting; original uses Shiki (build-time). Color fidelity may differ between catppuccin-latte (Shiki) and whatever syntect theme is used. Cannot verify without browser.

2. **Tabs are non-interactive**: The tabs JS init targets `[data-tabs]` but test-content fixture uses `.tabs-container` markup. Visually the tabs-list may render correctly as a static list but clicking tabs will not switch panels. Browser recheck recommended to confirm rendering state.

3. **Code copy/wrap buttons missing from content blocks**: The enhancer JS targets `pre.astro-code` — ccresdoc content code blocks lack that class. Buttons will not appear on hover. Browser recheck to confirm buttons are absent.

---

## Patches Applied (Must-Fix Items Closed)

### Must-fix #1 — CLOSED: code-block-enhancer selector updated

`app/components/code-block-enhancer.tsx`: changed querySelector from `pre.astro-code` to `pre.astro-code, pre:has(> code[class])`. This catches both Shiki-rendered blocks (`.astro-code`) and syntect-rendered blocks from ccresdoc-renderer (emits `<pre><code class="language-{lang}">`).

`app/styles/global.css`: added `pre.word-wrap` alongside `pre.astro-code.word-wrap` so word-wrap CSS applies to both block types.

### Must-fix #2 — CLOSED: tabs-init updated for ccresdoc-renderer markup

`app/components/tabs-init.tsx`: refactored to support two patterns:
- Pattern A: original `[data-tabs]` + `.tab-panel[data-tab-value]` + dynamically-created buttons
- Pattern B: ccresdoc-renderer `.tabs-container` + pre-rendered `.tabs-tab` buttons with `aria-controls` attributes

Pattern B wires click handlers to existing buttons and uses `aria-controls` attribute to find target panels by ID.

Both builds succeeded after patches (4 pages, 2 sentinels confirmed).

Zero must-fix deltas remain open.
