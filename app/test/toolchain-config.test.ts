import { compile } from "@takazudo/zfb-md-wasm";
import type { PipelineOptions } from "@takazudo/zfb-md-wasm";
import { describe, expect, it } from "vitest";
import { config } from "../zfb.config";
import { contentFixture } from "./fixtures/content";

describe("latest zudo-doc configuration", () => {
  it("keeps the resolved runtime strictly plugin-free", () => {
    expect(config.plugins).toEqual([]);
    expect(config).toMatchObject({
      framework: "preact",
      port: 4892,
      base: "/",
      trailingSlash: true,
      stripMdExt: true,
      collections: [{ name: "docs", path: "src/content/docs" }],
      resolveMarkdownLinks: {
        enabled: true,
        dirs: [{ dir: "src/content/docs", routePrefix: "/docs/" }],
        onBrokenLinks: "warn",
      },
    });
  });

  it("retains the current package Markdown defaults", () => {
    expect(config.markdown).toMatchObject({
      gfm: { taskListItem: true, footnoteDefinition: true },
      features: {
        directives: {
          note: "Note",
          tip: "Tip",
          info: "Info",
          warning: "Warning",
          danger: "Danger",
          caution: "Caution",
          details: "Details",
        },
        mermaid: true,
        githubAlerts: true,
        readingTime: true,
        codeEnrichment: {},
        codeTabs: true,
        ruby: true,
        tocExport: {},
        imageDimensions: {},
        linkValidation: { failOnBroken: false },
        headingIds: { strategy: "hierarchical" },
      },
    });
    expect(config.codeHighlight).toMatchObject({ mode: "class" });
  });

  it("compiles generated content with anchors, directives, GFM, links, code and Mermaid", async () => {
    const result = await compile(contentFixture.markdown, {
      filename: "generated-fixture.mdx",
      jsxRuntime: "preact",
      pipeline: {
        gfm:
          typeof config.markdown?.gfm === "object"
            ? config.markdown.gfm
            : undefined,
        cjkFriendly: config.markdown?.cjkFriendly,
        hardBreaks: config.markdown?.hardBreaks,
        features: config.markdown?.features,
        codeHighlight: { mode: "class" },
      } satisfies PipelineOptions,
    });

    expect(result.diagnostics).toEqual([]);
    expect(result.code).not.toBeNull();
    const code = result.code!;
    expect(code).toContain('slug: "duplicate-heading"');
    expect(code).toContain('slug: "duplicate-heading-nested-heading"');
    expect(code).toContain('slug: "duplicate-heading-1"');
    expect(code).toContain('slug: "duplicate-heading-1-nested-heading"');
    expect(code).toContain("const Warning = _components.Warning");
    expect(code).toContain('type: "checkbox"');
    expect(code).toContain('class: "footnotes"');
    expect(code).toContain('href: "https://example.com/resource"');
    expect(code).toContain('class: "hi-root"');
    expect(code).toContain('class=\\"hi-kw\\"');
    expect(code).toContain('class: "mermaid"');
    expect(code).toContain('"data-mermaid": ""');
  });
});
