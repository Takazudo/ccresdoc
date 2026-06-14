// zfb.config.ts — CCResDoc zudo-doc consumer configuration.
//
// CCResDoc is a zudo-doc consumer with zero Node.js plugins — all content
// generation is handled by Rust (Wave 2 / Wave 3). This means `zfb dev` runs
// in node-free mode: no plugin-host.mjs is spawned.
//
// Key decisions:
//   - port: 4892  (pinned; NOT the zfb default 4321)
//   - docsDir: src/content/docs
//   - ZERO .mjs plugins (no search-index, no doc-history, no claude-resources)
//   - Single "docs" collection, no locales, no versions
//   - stripMdExt + resolveMarkdownLinks: true (standard zudo-doc setup)
//
// NOTE: This file is bundled with --platform=neutral. Do NOT import node:os
// or node:path. process.env IS allowed.

import { defineConfig } from "zfb/config";

export default defineConfig({
  framework: "preact",
  // Pinned port — keep in sync with src-tauri/tauri.conf.json (devUrl) and settings.ts
  port: 4892,
  // base defaults to "/" — matches settings.ts base: "/" (no sub-path deployment)
  base: "/",
  tailwind: { enabled: true },
  collections: [
    { name: "docs", path: "src/content/docs" },
  ],
  // Strip .md/.mdx from internal links and add trailing slash
  stripMdExt: true,
  trailingSlash: true,
  resolveMarkdownLinks: {
    enabled: true,
    dirs: [{ dir: "src/content/docs", routePrefix: "/docs/" }],
    onBrokenLinks: "warn",
  },
  markdown: {
    features: {
      directives: {
        note: "Note",
        tip: "Tip",
        info: "Info",
        warning: "Warning",
        danger: "Danger",
        caution: "Caution",
      },
      mermaid: true,
      headingMarkerToc: true,
      githubAlerts: true,
      readingTime: true,
      codeEnrichment: {},
      codeTabs: true,
      ruby: true,
      tocExport: {},
      imageDimensions: {},
      headingIds: { strategy: "flat" },
    },
  },
  // ZERO plugins — node-free operation (no plugin-host.mjs spawned)
  plugins: [],
});
