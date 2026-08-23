import { readFileSync, readdirSync } from "node:fs";
import { resolve } from "node:path";
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

  it("uses only public zudo-doc entry points in the consumer and its tests", () => {
    const roots = ["pages", "src", "scripts", "test"];
    const files = [
      "zfb.config.ts",
      ...roots.flatMap((root) =>
        readdirSync(resolve(process.cwd(), root), {
          recursive: true,
          withFileTypes: true,
        })
          .filter((entry) => entry.isFile())
          .map((entry) => resolve(entry.parentPath, entry.name))
          .filter((file) => /\.(?:[cm]?[jt]sx?|mjs)$/.test(file)),
      ),
    ];

    for (const file of files) {
      const source = readFileSync(resolve(process.cwd(), file), "utf8");
      expect(source, file).not.toMatch(/@takazudo\/zudo-doc\/dist\//);
      expect(source, file).not.toMatch(/node_modules\/@takazudo\/zudo-doc/);
    }
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
