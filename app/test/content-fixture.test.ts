import { describe, expect, it } from "vitest";
import { contentFixture, createContentFixture } from "./fixtures/content";

describe("frontend test foundation", () => {
  it("provides a deterministic generated-document fixture", () => {
    expect(contentFixture.route).toBe("/docs/fixture/overview/");
    expect(contentFixture.frontmatter.generated).toBe(true);
    expect(contentFixture.markdown).toContain("[!NOTE]");
    expect(contentFixture.markdown.match(/^## Duplicate heading$/gm)).toHaveLength(2);
  });

  it("supports isolated fixture overrides without mutating the shared value", () => {
    const variant = createContentFixture({
      slug: "fixture/variant",
      route: "/docs/fixture/variant/",
      frontmatter: { title: "Variant" },
    });

    expect(variant.slug).toBe("fixture/variant");
    expect(variant.frontmatter).toEqual({
      title: "Variant",
      description: contentFixture.frontmatter.description,
      generated: true,
    });
    expect(contentFixture.slug).toBe("fixture/overview");
  });

  it("runs in the browser-like DOM environment", () => {
    const element = document.createElement("article");
    element.dataset.fixtureRoute = contentFixture.route;
    element.textContent = contentFixture.frontmatter.title;
    document.body.append(element);

    expect(document.querySelector("article")?.dataset.fixtureRoute).toBe(contentFixture.route);
    expect(document.body.textContent).toContain("Deterministic fixture");
  });
});
