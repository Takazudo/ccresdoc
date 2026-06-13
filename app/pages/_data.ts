// pages/_data.ts — zfb data helpers for CCResDoc doc pages.
//
// Bridges zfb's CollectionEntry with the DocsEntry shape that sidebar/nav
// helpers expect. Synchronous per zfb ADR-004 (getCollection is synchronous
// inside paths() evaluation).

import { getCollection } from "zfb/content";
import type { CollectionEntry } from "zfb/content";
import type { DocsData } from "@/config/docs-schema";
import type { DocsEntry } from "@/types/docs-entry";
import { toRouteSlug } from "@/utils/slug";

export type ZfbDocsData = DocsData;

export type ZfbDocsEntry = CollectionEntry<ZfbDocsData> & {
  id: string;
  collection: string;
};

function stripIndexSuffix(slug: string): string {
  return toRouteSlug(slug);
}

export function getDocs(collectionName: string): ZfbDocsEntry[] {
  const entries = getCollection<ZfbDocsData>(collectionName);
  return entries.map((e) => ({
    ...e,
    id: stripIndexSuffix(e.slug),
    collection: collectionName,
  }));
}

export function loadDocs(collectionName: string): DocsEntry[] {
  return getDocs(collectionName) as DocsEntry[];
}
