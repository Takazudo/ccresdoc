export type ContentFixture = {
  slug: string;
  route: string;
  frontmatter: {
    title: string;
    description: string;
    generated: boolean;
  };
  markdown: string;
};

/** Stable content used by route, Markdown, and DOM suites in later waves. */
export const contentFixture: ContentFixture = Object.freeze({
  slug: "fixture/overview",
  route: "/docs/fixture/overview/",
  frontmatter: Object.freeze({
    title: "Deterministic fixture",
    description: "A small, stable document for frontend harness tests.",
    generated: true,
  }),
  markdown: [
    "# Deterministic fixture",
    "",
    "A stable paragraph with a [link](https://example.com).",
    "",
    "> [!NOTE]",
    "> The fixture keeps the Markdown pipeline representative.",
    "",
    "- [x] deterministic content",
    "- [ ] DOM-ready test target",
    "",
    "## Duplicate heading",
    "",
    "## Duplicate heading",
    "",
  ].join("\n"),
});

type ContentFixtureOverrides = Partial<Omit<ContentFixture, "frontmatter">> & {
  frontmatter?: Partial<ContentFixture["frontmatter"]>;
};

export function createContentFixture(overrides: ContentFixtureOverrides = {}): ContentFixture {
  return {
    ...contentFixture,
    ...overrides,
    frontmatter: {
      ...contentFixture.frontmatter,
      ...overrides.frontmatter,
    },
  };
}
