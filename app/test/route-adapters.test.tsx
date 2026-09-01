/** @jsxRuntime automatic */
/** @jsxImportSource preact */

import { render } from "preact-render-to-string";
import type { ComponentType } from "preact";
import { describe, expect, it, vi } from "vitest";
import themePackCatalog from "@takazudo/zudo-doc/catalog";

const content = vi.hoisted(() => {
  type Components = Record<string, ComponentType<Record<string, unknown>>>;

  const entries = [
    {
      slug: "index",
      data: { title: "Docs home", sidebar_position: 1 },
      body: "# Docs home",
      module_specifier: "mdx://docs/index.mdx",
      Content: () => <p>Docs root content</p>,
    },
    {
      slug: "claude/index",
      data: {
        title: "Claude Resources",
        sidebar_position: 10,
        description: "Browse selected Claude resources.",
      },
      body: '<CategoryNav categories={["claude-md", "claude-absent"]} />',
      module_specifier: "mdx://docs/claude/index.mdx",
      Content: ({ components }: { components?: Components }) => {
        const CategoryNav = components?.CategoryNav;
        return (
          <article data-resource-landing="claude">
            <h1>Claude Resources</h1>
            {CategoryNav ? (
              <CategoryNav categories={["claude-md", "claude-absent"]} />
            ) : null}
          </article>
        );
      },
    },
    {
      slug: "codex/index",
      data: {
        title: "Codex Resources",
        sidebar_position: 20,
        description: "Browse selected Codex resources.",
      },
      body: '<CategoryNav categories={["codex-agents-md", "codex-absent"]} />',
      module_specifier: "mdx://docs/codex/index.mdx",
      Content: ({ components }: { components?: Components }) => {
        const CategoryNav = components?.CategoryNav;
        return (
          <article data-resource-landing="codex">
            <h1>Codex Resources</h1>
            {CategoryNav ? (
              <CategoryNav categories={["codex-agents-md", "codex-absent"]} />
            ) : null}
          </article>
        );
      },
    },
    {
      slug: "claude-md/index",
      data: {
        title: "CLAUDE.md",
        description: "Generated instruction files",
        sidebar_position: 11,
        category_no_page: true,
        generated: true,
      },
      body: "",
      module_specifier: "mdx://docs/claude-md/index.mdx",
      Content: () => null,
    },
    {
      slug: "codex-agents-md/index",
      data: {
        title: "AGENTS.md",
        sidebar_position: 21,
        category_no_page: true,
        generated: true,
      },
      body: "",
      module_specifier: "mdx://docs/codex-agents-md/index.mdx",
      Content: () => null,
    },
    {
      slug: "claude-md/project-nested",
      data: {
        title: "Nested generated resource",
        description: "A generated project instruction file",
        sidebar_position: 12,
        generated: true,
      },
      body: [
        "## Duplicate heading",
        "### Nested heading",
        "## Duplicate heading",
        "### Nested heading",
      ].join("\n\n"),
      module_specifier: "mdx://docs/claude-md/project-nested.mdx",
      Content: ({ components }: { components?: Components }) => {
        const CategoryNav = components?.CategoryNav;
        return (
          <article data-generated-resource>
            <h2 id="duplicate-heading">Duplicate heading</h2>
            <h3 id="duplicate-heading-nested-heading">Nested heading</h3>
            <h2 id="duplicate-heading-1">Duplicate heading</h2>
            <h3 id="duplicate-heading-1-nested-heading">Nested heading</h3>
            {CategoryNav ? <CategoryNav categories={["claude-md"]} /> : null}
          </article>
        );
      },
    },
    {
      slug: "codex-agents-md/project-nested",
      data: {
        title: "Nested Codex instruction",
        description: "A generated Codex instruction file",
        sidebar_position: 22,
        generated: true,
      },
      body: "Codex instruction content",
      module_specifier: "mdx://docs/codex-agents-md/project-nested.mdx",
      Content: () => <article data-generated-codex-resource>Codex detail</article>,
    },
    {
      slug: "private-draft",
      data: { title: "Draft", draft: true, sidebar_position: 90 },
      body: "draft",
      module_specifier: "mdx://docs/private-draft.mdx",
      Content: () => <p>draft</p>,
    },
    {
      slug: "guides/visible-child",
      data: { title: "Visible guide", sidebar_position: 50 },
      body: "visible guide",
      module_specifier: "mdx://docs/guides/visible-child.mdx",
      Content: () => <p>Visible guide route content</p>,
    },
    {
      slug: "deep/unlisted-resource",
      data: { title: "Unlisted", unlisted: true, sidebar_position: 91 },
      body: "unlisted",
      module_specifier: "mdx://docs/deep/unlisted-resource.mdx",
      Content: () => <p>Unlisted route content</p>,
    },
  ];

  const snapshot = { collections: { docs: entries } };
  return { entries, snapshot };
});

vi.mock("@takazudo/zfb/content", () => ({
  getCollection: (name: string) =>
    name === "docs" ? content.entries : [],
  getContentSnapshot: () => content.snapshot,
}));

import NotFoundPage from "../pages/404";
import HomePage from "../pages/index";
import DocsPage, { paths } from "../pages/docs/[[...slug]]";
import { routeContext } from "../pages/lib/_route-context";

describe("host-owned package route adapters", () => {
  const routeItems = paths();
  const routeParams = routeItems.map((item) => item.params.slug.join("/"));

  it("enumerates entry, auto-index, and unlisted routes with package rules", () => {
    expect(routeParams).toContain("");
    expect(routeParams).toContain("claude-md/project-nested");
    expect(routeParams).toContain("deep/unlisted-resource");
    expect(routeParams).toContain("guides/visible-child");
    expect(routeParams).toContain("guides");
    // Unlisted entries keep a real route but do not manufacture a visible
    // category auto-index from an otherwise hidden branch.
    expect(routeParams).not.toContain("deep");
    expect(routeParams).toContain("claude");
    expect(routeParams).toContain("codex");
    expect(routeParams).not.toContain("claude-md");
    expect(routeParams).not.toContain("codex-agents-md");
    expect(routeParams).not.toContain("private-draft");

    expect(routeContext.enumerateDocsRoutes("en")).toEqual(
      expect.arrayContaining([
        "/docs/",
        "/docs/claude/",
        "/docs/codex/",
        "/docs/claude-md/project-nested/",
        "/docs/codex-agents-md/project-nested/",
        "/docs/deep/unlisted-resource/",
        "/docs/guides/visible-child/",
        "/docs/guides/",
      ]),
    );
  });

  it("renders generated MDX through package components without fallback output", () => {
    const item = routeItems.find(
      (candidate) => candidate.params.slug.join("/") === "claude-md/project-nested",
    );
    expect(item).toBeDefined();

    const html = render(
      <DocsPage params={item!.params} {...item!.props} />,
    );

    expect(html).toContain("data-generated-resource");
    expect(html).toContain('id="duplicate-heading"');
    expect(html).toContain('id="duplicate-heading-1"');
    expect(html).toContain("Generated instruction files");
    expect(html).toContain("data-zfb-island");
    expect(html).not.toContain("data-zfb-content-fallback");
    expect(html).not.toContain("[zfb fallback render]");
  });

  it("renders both generic resource landings and omits absent category cards", () => {
    for (const slug of ["claude", "codex"]) {
      const item = routeItems.find(
        (candidate) => candidate.params.slug.join("/") === slug,
      );
      expect(item).toBeDefined();
      const html = render(<DocsPage params={item!.params} {...item!.props} />);

      expect(html).toContain(`data-resource-landing="${slug}"`);
      expect(html).not.toContain("claude-absent");
      expect(html).not.toContain("codex-absent");
      expect(html).not.toContain("data-zfb-content-fallback");
    }
  });

  it("renders / as the exact canonical docs shell with no marketing cover", () => {
    const home = render(<HomePage />);
    const docsRoot = routeItems.find((item) => item.params.slug.length === 0);
    expect(docsRoot).toBeDefined();
    const canonical = render(
      <DocsPage params={docsRoot!.params} {...docsRoot!.props} />,
    );

    expect(home).toBe(canonical);
    expect(home).toContain("Docs root content");
    expect(home).not.toContain("data-home-page");
  });

  it("renders ordered desktop tabs and an ordered mobile root menu", () => {
    expect(routeContext.settings.headerNav).toEqual([
      { label: "Claude", path: "/docs/claude", categoryMatch: "claude", versioned: false },
      { label: "Codex", path: "/docs/codex", categoryMatch: "codex", versioned: false },
    ]);

    const docsRoot = routeItems.find((item) => item.params.slug.length === 0);
    expect(docsRoot).toBeDefined();
    const html = render(
      <DocsPage params={docsRoot!.params} {...docsRoot!.props} />,
    );

    expect(html).toMatch(/<a href="\/docs\/" data-header-logo="true"/);
    expect(html).toContain('href="/docs/claude/"');
    expect(html).toContain('href="/docs/codex/"');
    expect(html.indexOf(">Claude</a>")).toBeLessThan(html.indexOf(">Codex</a>"));

    const shell = document.createElement("div");
    shell.innerHTML = html;
    const settingsIsland = shell.querySelector<HTMLElement>(
      '[data-zfb-island="SettingsHeaderButton"]',
    );
    expect(settingsIsland).not.toBeNull();
    expect(settingsIsland?.getAttribute("data-when")).toBe("load");
    expect(settingsIsland?.querySelector('button[aria-label="Open Settings"]')).not.toBeNull();
    const toolbarIsland = shell.querySelector<HTMLElement>(
      '[data-zfb-island="CCResDocBrowserToolbar"]',
    );
    expect(toolbarIsland).not.toBeNull();
    expect(toolbarIsland?.hasAttribute("data-ccresdoc-browser-toolbar-shell")).toBe(true);
    expect(toolbarIsland?.getAttribute("data-zfb-transition-persist")).toBe(
      "ccresdoc-browser-toolbar",
    );
    const siteSearch = shell.querySelector<HTMLElement>("site-search");
    expect(siteSearch).not.toBeNull();
    expect(siteSearch?.getAttribute("data-base")).toBe("/docs/");
    expect(siteSearch?.getAttribute("data-disable-built-in-shortcut")).toBe("true");
    const mobileToggle = shell.querySelector<HTMLElement>(
      '[data-zfb-island="SidebarToggle"]',
    );
    expect(mobileToggle).not.toBeNull();
    const mobileProps = JSON.parse(
      mobileToggle!.getAttribute("data-props") ?? "null",
    );
    expect(mobileProps.nodes).toEqual([]);
    expect(mobileProps.rootMenuItems).toEqual([
      { label: "Claude", href: "/docs/claude/" },
      { label: "Codex", href: "/docs/codex/" },
    ]);
    expect(mobileProps.backToMenuLabel).toBeDefined();
  });

  it("marks the current tab and scopes each landing sidebar by category prefix", () => {
    const claude = routeItems.find(
      (candidate) => candidate.params.slug.join("/") === "claude",
    );
    expect(claude).toBeDefined();
    const claudeHtml = render(
      <DocsPage params={claude!.params} {...claude!.props} />,
    );
    expect(claudeHtml).toContain('href="/docs/claude/" aria-current="page"');
    expect(claudeHtml).not.toContain('href="/docs/codex/" aria-current="page"');

    const shell = document.createElement("div");
    shell.innerHTML = claudeHtml;
    const mobileToggle = shell.querySelector<HTMLElement>(
      '[data-zfb-island="SidebarToggle"]',
    );
    expect(mobileToggle).not.toBeNull();
    const mobileProps = JSON.parse(
      mobileToggle!.getAttribute("data-props") ?? "null",
    );
    expect(mobileProps.nodes).toEqual(
      expect.arrayContaining([
        expect.objectContaining({ slug: "claude-md" }),
      ]),
    );
    expect(mobileProps.nodes).not.toEqual(
      expect.arrayContaining([
        expect.objectContaining({ slug: "codex-agents-md" }),
      ]),
    );
  });

  it("SSR-bootstraps every catalog theme pack and serializes switcher order", () => {
    const docsRoot = routeItems.find((item) => item.params.slug.length === 0);
    expect(docsRoot).toBeDefined();
    const html = render(
      <DocsPage params={docsRoot!.params} {...docsRoot!.props} />,
    );
    const shell = document.createElement("div");
    shell.innerHTML = html;

    expect(html).toContain('data-theme-pack="default"');
    expect(html).toContain('var respectPrefersColorScheme=true;');
    expect(html).toContain('var STORAGE_KEY="zudo-doc-theme";');
    expect(html).toContain("zudo-doc-theme-pack");
    expect(html).toContain("data-zd-theme-pack-loading");
    expect(html).toContain("theme-packs/\"+slug+\"/pack.css?v=");
    expect(html.indexOf('var STORAGE_KEY="zudo-doc-theme";')).toBeLessThan(
      html.indexOf('var KEY="zudo-doc-theme-pack";'),
    );

    const switcher = shell.querySelector<HTMLElement>(
      '[data-zfb-island="ThemePackSwitcher"]',
    );
    expect(switcher).not.toBeNull();
    const props = JSON.parse(switcher!.getAttribute("data-props") ?? "null");
    expect(props).toEqual({
      active: "default",
      base: "/",
      order: themePackCatalog.packs.map(
        ({ slug, name, mode, description }) => ({
          slug,
          name,
          mode,
          description,
        }),
      ),
    });
  });

  it("renders package 404 chrome with client-navigation wiring", () => {
    const notFound = render(<NotFoundPage />);

    expect(notFound).toContain("Page not found.");
    expect(notFound).toContain('name="robots" content="noindex, nofollow"');
    expect(notFound).toContain('name="zfb-view-transitions-enabled" content="true"');
    expect(notFound).toContain("zfb:after-swap");
    expect(notFound).not.toContain("[zfb fallback render]");
  });
});
