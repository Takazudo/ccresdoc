import type { DocsData } from "@/config/docs-schema";

export interface DocsEntry {
  id: string;
  slug: string;
  body?: string;
  collection: string;
  data: DocsData;
}
