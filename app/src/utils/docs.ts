// Docs navigation utilities for CCResDoc.
//
// Bridges @takazudo/zudo-doc/sidebar-tree's SidebarNode shape (id, type, ...)
// to the NavNode shape that SidebarTree / SidebarToggle components consume
// (slug, position, ...). The divergence exists because SidebarNode comes from
// the published framework package while NavNode matches the template's client
// component prop API (SidebarRootMenuItem / SidebarNavNode from
// @takazudo/zudo-doc/sidebar/types).

import {
  buildSidebarTree,
  type SidebarNode,
  type BuildSidebarTreeOptions,
} from "@takazudo/zudo-doc/sidebar-tree";
import type { CollectionEntryLike } from "@takazudo/zudo-doc/sidebar-tree";
import { withBase } from "./base";

// ---------------------------------------------------------------------------
// NavNode — host type matching the template's sidebar-tree.tsx API
// ---------------------------------------------------------------------------

export interface NavNode {
  slug: string;
  label: string;
  description?: string;
  position: number;
  href?: string;
  hasPage: boolean;
  children: NavNode[];
  sortOrder?: "asc" | "desc";
  collapsed?: boolean;
}

// ---------------------------------------------------------------------------
// SidebarNode → NavNode conversion
// ---------------------------------------------------------------------------

function sidebarNodeToNavNode(node: SidebarNode): NavNode {
  return {
    slug: node.id,
    label: node.label,
    description: node.description,
    position: node.sidebar_position ?? 999,
    href: node.href,
    hasPage: node.hasPage,
    children: node.children.map(sidebarNodeToNavNode),
    sortOrder: node.sortOrder,
    collapsed: node.collapsed,
  };
}

// ---------------------------------------------------------------------------
// buildNavTree — wraps buildSidebarTree with the NavNode conversion
// ---------------------------------------------------------------------------

export function buildNavTree<T extends { title: string }>(
  entries: CollectionEntryLike<T>[],
  locale: string,
  options?: BuildSidebarTreeOptions,
): NavNode[] {
  const nodes = buildSidebarTree(entries, locale, {
    defaultLocale: "en",
    buildHref: (slug) => withBase(`/docs/${slug}`),
    ...options,
  });
  return nodes.map(sidebarNodeToNavNode);
}
