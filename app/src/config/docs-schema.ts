// CCResDoc docs frontmatter schema.
// Mirrors zudo-doc's standard schema, trimmed to what CCResDoc needs.
// Wave 2 (Rust generator) writes MDX with frontmatter fields from this schema.

import { z } from "zod";

// No .passthrough() — Zod strips unknown frontmatter keys silently,
// preventing typo'd fields from leaking into DocsData while not throwing.
export function buildDocsSchema() {
  return z.object({
    title: z.string(),
    description: z.string().optional(),
    sidebar_position: z.number().optional(),
    sidebar_label: z.string().optional(),
    draft: z.boolean().optional(),
    unlisted: z.boolean().optional(),
    hide_sidebar: z.boolean().optional(),
    hide_toc: z.boolean().optional(),
    // Slug is path-like: one or more segments separated by "/". Segments are
    // derived from real filesystem names by the Wave 2 generator, so they may
    // contain uppercase, digits, dots, underscores and hyphens (e.g.
    // "claude-skills/<SkillDir>/ref-<Name>"). The pattern rejects spaces,
    // leading/trailing/double slashes — not lowercase-only.
    slug: z
      .string()
      .regex(
        /^[A-Za-z0-9._-]+(\/[A-Za-z0-9._-]+)*$/,
        'slug must be "/"-separated path segments (no spaces or empty segments)',
      )
      .optional(),
    // Marks an index.mdx as category metadata (no page built).
    // Used by Wave 2 generator for category roots.
    generated: z.boolean().optional(),
    category_no_page: z.boolean().optional(),
  });
}

export type DocsData = z.infer<ReturnType<typeof buildDocsSchema>>;
