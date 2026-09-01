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
      "header[data-header]",
      "#desktop-sidebar",
      "nav[data-zd-toc]",
      '[data-zfb-island="FindInPageInit"] > div',
      "[data-sidebar-resizer]",
    ]) expect(css).toContain(selector);
    expect(css).toContain("@media (pointer: coarse)");
    expect(css).toContain("--ccresdoc-toolbar-control-size: 2.75rem");
    expect(css).toContain("@media (hover: hover)");
    expect(css).toContain("@media (prefers-reduced-motion: no-preference)");
    expect(css).not.toMatch(/\.ccresdoc-browser-toolbar__path[^}]*max-inline-size/s);
  });
});
