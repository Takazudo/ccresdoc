import { buildDocsSchema } from "@takazudo/zudo-doc/docs-schema";
import { defaultDirectiveVocabulary } from "@takazudo/zudo-doc/directive-vocabulary-defaults";
import { defineConfig } from "@takazudo/zfb/config";
import { z } from "zod";

// Control case: the same runtime invariant without zudoDoc() composition.
// This is viable, but duplicates package policy and is intentionally rejected
// in favor of configs/selected.mjs.
export default defineConfig({
  framework: "preact",
  port: 4892,
  base: "/",
  tailwind: { enabled: true },
  collections: [
    {
      name: "docs",
      path: "src/content/docs",
      schema: z.toJSONSchema(buildDocsSchema()),
    },
  ],
  plugins: [],
  markdown: {
    features: {
      directives: { ...defaultDirectiveVocabulary },
      mermaid: true,
      headingMarkerToc: true,
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
    cjkFriendly: false,
    gfm: { taskListItem: true, footnoteDefinition: true },
  },
  codeHighlight: { mode: "class", defaultStylesheet: true },
  resolveMarkdownLinks: {
    enabled: true,
    dirs: [{ dir: "src/content/docs", routePrefix: "/docs/" }],
    onBrokenLinks: "warn",
  },
  stripMdExt: true,
  trailingSlash: true,
  minifyHtml: true,
});
