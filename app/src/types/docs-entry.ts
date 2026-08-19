import type { DocsData } from "@takazudo/zudo-doc/docs-schema";

export interface DocsEntry {
  id: string;
  slug: string;
  body?: string;
  collection: string;
  data: DocsData;
}
