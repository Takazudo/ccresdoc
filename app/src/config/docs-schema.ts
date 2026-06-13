// CCResDoc docs frontmatter schema.
// Mirrors zudo-doc's standard schema, trimmed to what CCResDoc needs.
// Wave 2 (Rust generator) writes MDX with frontmatter fields from this schema.

import { z } from "zod";

export function buildDocsSchema() {
  return z
    .object({
      title: z.string(),
      description: z.string().optional(),
      sidebar_position: z.number().optional(),
      sidebar_label: z.string().optional(),
      draft: z.boolean().optional(),
      unlisted: z.boolean().optional(),
      hide_sidebar: z.boolean().optional(),
      hide_toc: z.boolean().optional(),
      slug: z.string().optional(),
      // Marks an index.mdx as category metadata (no page built).
      // Used by Wave 2 generator for category roots.
      generated: z.boolean().optional(),
      category_no_page: z.boolean().optional(),
    })
    .passthrough();
}

export type DocsData = z.infer<ReturnType<typeof buildDocsSchema>>;
