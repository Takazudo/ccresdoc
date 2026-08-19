/** @jsxRuntime automatic */
/** @jsxImportSource preact */

import { render } from "preact-render-to-string";
import type { ComponentType } from "preact";
import { describe, expect, it, vi } from "vitest";

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
        category_no_page: true,
        generated: true,
      },
      body: '<CategoryNav categories={["claude-md"]} />',
      module_specifier: "mdx://docs/claude/index.mdx",
      Content: () => <p>Category metadata must never render as a page.</p>,
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
    expect(routeParams).not.toContain("claude");
    expect(routeParams).not.toContain("claude-md");
    expect(routeParams).not.toContain("private-draft");

    expect(routeContext.enumerateDocsRoutes("en")).toEqual(
      expect.arrayContaining([
        "/docs/",
        "/docs/claude-md/project-nested/",
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

  it("renders package home and 404 chrome with client-navigation wiring", () => {
    const home = render(<HomePage />);
    const notFound = render(<NotFoundPage />);

    expect(home).toContain("CCResDoc");
    expect(home).toContain("Nested generated resource");
    expect(home).toContain("data-zfb-island");
    expect(notFound).toContain("Page not found.");
    expect(notFound).toContain('name="robots" content="noindex, nofollow"');
    expect(notFound).toContain('name="zfb-view-transitions-enabled" content="true"');
    expect(notFound).toContain("zfb:after-swap");
    expect(notFound).not.toContain("[zfb fallback render]");
  });
});
