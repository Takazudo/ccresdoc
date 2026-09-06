import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, expect, it } from "vitest";

const css = readFileSync(
  resolve(process.cwd(), "src/styles/global.css"),
  "utf8",
);

describe("package CSS foundation", () => {
  it("loads package styles in their required order", () => {
    const imports = [
      "@takazudo/zudo-doc/theme.css",
      "@takazudo/zudo-doc/safelist.css",
      "@takazudo/zudo-doc/content.css",
      "@takazudo/zudo-doc/page-loading.css",
      "@takazudo/zudo-doc/features.css",
    ];

    expect(imports.map((specifier) => css.indexOf(specifier))).toEqual(
      [...imports].map((specifier) => css.indexOf(specifier)).sort((a, b) => a - b),
    );
    for (const specifier of imports) {
      expect(css.match(new RegExp(specifier.replaceAll(".", "\\."), "g"))).toHaveLength(1);
    }
  });

  it("does not retain package-owned token or code-style mirrors", () => {
    expect(css).not.toContain("@theme {");
    expect(css).not.toContain("pre.hi-root");
    expect(css).not.toContain("--color-accent:");
    expect(css).toContain("@media (forced-colors: active)");
  });

  it("limits the pre-hydration affordance to JS-only load controls", () => {
    expect(css).toContain(
      'html:not([data-ccresdoc-load-controls-ready]) [data-zfb-island="ThemeToggle"] button',
    );
    expect(css).toContain(
      'html:not([data-ccresdoc-load-controls-ready]) [data-zfb-island="ThemePackSwitcher"] button',
    );
    expect(css).toContain(
      'html:not([data-ccresdoc-load-controls-ready]) [data-zfb-island="SettingsHeaderButton"] button',
    );
    expect(css).not.toMatch(/data-ccresdoc-load-controls-ready[^\n]*SidebarTree/);
    expect(css).not.toMatch(/\[data-zfb-island\][^\n]*button/);
  });

  it("coordinates the browser toolbar and package sticky offsets", () => {
    expect(css).toContain("--ccresdoc-browser-toolbar-h: 3.25rem");
    expect(css).toContain("--ccresdoc-complete-chrome-offset:");
    for (const selector of [
      "[data-ccresdoc-chrome-region]",
      "[data-ccresdoc-chrome-region] > header[data-header]",
      "#desktop-sidebar",
      "nav[data-zd-toc]",
      "[data-find-in-page-bar]",
      "[data-sidebar-resizer]",
    ]) expect(css).toContain(selector);
    expect(css).toContain("@media (pointer: coarse)");
    expect(css).toContain("--ccresdoc-toolbar-control-size: 2.75rem");
    expect(css).toContain("@media (hover: hover)");
    expect(css).toContain("@media (prefers-reduced-motion: no-preference)");
    expect(css).toContain(".find-in-page-control:focus-visible");
    expect(css).toContain("@media (max-width: 30rem)");
    expect(css).toMatch(/@media \(pointer: coarse\)[\s\S]*\.find-in-page-control/);
    expect(css).toMatch(/@media \(forced-colors: active\)[\s\S]*\.find-in-page-control/);
    expect(css).not.toMatch(/\.ccresdoc-browser-toolbar__path[^}]*max-inline-size/s);
  });

  it("extracts the browser toolbar row from the root view-transition snapshot", () => {
    // Persisting the toolbar node keeps it alive across the swap; only a
    // view-transition-name keeps it visually static. features.css assigns one
    // for its own five persist keys and would leave this row cross-fading with
    // the article, so the host layer names it and stops its animation.
    expect(css).toContain(
      ".ccresdoc-browser-toolbar {\n  position: relative;\n  z-index: var(--z-index-dropdown);\n  view-transition-name: ccresdoc-browser-toolbar;",
    );
    expect(css).toMatch(
      /::view-transition-old\(ccresdoc-browser-toolbar\),\n::view-transition-new\(ccresdoc-browser-toolbar\),\n::view-transition-group\(ccresdoc-browser-toolbar\) \{\n {2}animation: none;/,
    );
    // The name must NOT sit on the toolbar's persist root: view-transition-name
    // makes its element a stacking context, which sinks the whole toolbar below
    // the header and leaves the overflow menu unclickable.
    expect(css).not.toMatch(
      /\[data-zfb-transition-persist="ccresdoc-browser-toolbar"\] \{[^}]*view-transition-name/,
    );
    // Nor on the chrome region wrapper: a named element is also a backdrop
    // root, and the wrapper is an ancestor of header[data-header], whose
    // backdrop-filter in the fjord/drift packs would then blur nothing.
    expect(css).not.toMatch(
      /\[data-ccresdoc-chrome-region\] \{[^}]*view-transition-name/,
    );
    // The one-sided entry/exit pair must stay neutralised under reduced motion,
    // which features.css only does for its own names.
    expect(css).toMatch(
      /@media \(prefers-reduced-motion: reduce\) \{[\s\S]*?::view-transition-old\(ccresdoc-browser-toolbar\):only-child,\n {2}::view-transition-new\(ccresdoc-browser-toolbar\):only-child \{\n {4}animation: none;/,
    );
    // Host rules must follow the package import to win on source order.
    expect(css.indexOf("@takazudo/zudo-doc/features.css")).toBeLessThan(
      css.indexOf("view-transition-name: ccresdoc-browser-toolbar"),
    );
  });

  it("documents every surviving package-internal !important override", () => {
    const lines = css.split("\n");
    const documented = new Set<number>();

    for (const [index, line] of lines.entries()) {
      const trimmed = line.trimStart();
      if (trimmed.startsWith("*") || trimmed.startsWith("/*")) continue;
      if (!line.includes("!important")) continue;
      let open = index;
      while (open > 0 && !lines[open].trimEnd().endsWith("{")) open -= 1;
      let selector = open;
      while (selector > 0 && lines[selector - 1].trimEnd().endsWith(",")) selector -= 1;
      // A rule nested in an at-rule (@media, @supports) is documented above the
      // at-rule, not between it and the selector — keep walking past openers.
      let comment = selector - 1;
      while (comment > 0 && lines[comment].trimStart().startsWith("@")) comment -= 1;
      // Each rule reaching into a package-internal selector must say why.
      expect(
        lines[comment]?.trimEnd().endsWith("*/"),
        `undocumented !important rule: ${lines[selector]?.trim()}`,
      ).toBe(true);
      documented.add(selector);
    }

    // The wrapper removed header[data-header]{top}; the rest survive because
    // zudo-doc hardcodes 3.5rem in markup it fully owns.
    expect(documented.size).toBe(6);
    expect(css).not.toMatch(/^header\[data-header\] \{/m);
  });
});
