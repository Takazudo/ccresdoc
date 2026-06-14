// MDX component map for CCResDoc doc pages.
//
// Maps MDX tag names to Preact components. Includes:
// - htmlOverrides from @takazudo/zudo-doc/content (heading anchors, styled elements)
// - CategoryNav for the claude/index.mdx overview page (Wave 2 generated)
// - Admonition wrappers for :::note, :::tip, etc.
// - Stub for Island (SSR pass-through)

import { h } from "preact";
import type { ComponentChildren } from "preact";
import { htmlOverrides } from "@takazudo/zudo-doc/content";
import { CategoryNav } from "@takazudo/zudo-doc/nav-indexing";
import type { NavNode } from "@takazudo/zudo-doc/nav-indexing";
import { withBase } from "@/utils/base";
import { loadDocs } from "./_data";
import { buildSidebarTree } from "@takazudo/zudo-doc/sidebar-tree";

// ---------------------------------------------------------------------------
// Stubs
// ---------------------------------------------------------------------------

// Pass children through so MDX content inside stub tags is not silently dropped.
// These components have no meaningful visual representation in CCResDoc's SSG
// output, but discarding their children would erase prose/code nested inside
// Details, CodeGroup, Tabs, etc.
function MdxStub(props: { children?: ComponentChildren }) {
  return h("div", { "data-mdx-stub": true }, props.children);
}

function IslandWrapper(props: {
  when?: "load" | "idle" | "visible" | "media";
  children?: ComponentChildren;
}): ComponentChildren {
  return props.children ?? null;
}

// ---------------------------------------------------------------------------
// CategoryNav wrapper
//
// The Wave 2 generator emits <CategoryNav categories={["slug1","slug2"]} />
// This wrapper resolves those slugs from the nav tree into NavNode[]
// and passes them to the @takazudo/zudo-doc CategoryNav component.
// ---------------------------------------------------------------------------

import type { SidebarNode } from "@takazudo/zudo-doc/sidebar-tree";

// Recursively convert a SidebarNode tree to nav-indexing NavNode[] shape.
// SidebarNode uses `id` + `sidebar_position`; NavNode uses `label`, `href`,
// `hasPage`, `children` — the minimal surface CategoryNav reads.
function sidebarNodesToNavNodes(nodes: SidebarNode[]): NavNode[] {
  return nodes.map((node) => ({
    label: node.label,
    description: node.description,
    href: node.href,
    hasPage: node.hasPage,
    children: sidebarNodesToNavNodes(node.children),
  }));
}

function flattenTree(nodes: SidebarNode[]): Map<string, SidebarNode> {
  const map = new Map<string, SidebarNode>();
  for (const node of nodes) {
    map.set(node.id, node);
    if (node.children.length > 0) {
      for (const [k, v] of flattenTree(node.children)) {
        map.set(k, v);
      }
    }
  }
  return map;
}

function CategoryNavWrapper(props: { categories?: string[] }) {
  const docs = loadDocs("docs");
  const tree = buildSidebarTree(docs, "en", {
    defaultLocale: "en",
    buildHref: (slug) => withBase(`/docs/${slug}`),
  });

  const nodeMap = flattenTree(tree);
  const children: NavNode[] = (props.categories ?? [])
    .map((slug) => {
      const node = nodeMap.get(slug);
      if (!node) return null;
      return {
        label: node.label,
        description: node.description,
        href: node.href,
        hasPage: node.hasPage,
        children: sidebarNodesToNavNodes(node.children),
      } satisfies NavNode;
    })
    .filter((n): n is NavNode => n !== null);

  return h(CategoryNav, { children }) as unknown as ComponentChildren;
}

// ---------------------------------------------------------------------------
// Admonitions
// ---------------------------------------------------------------------------

function makeAdmonition(variant: string) {
  return function Admonition(props: {
    title?: string;
    children?: ComponentChildren;
  }) {
    // role="note" surfaces the container as an advisory region to assistive tech.
    // aria-label names the region with the variant so screen readers announce
    // e.g. "note region" or "warning region" without requiring visible text.
    const titleEl = props.title
      ? h("p", { class: "admonition-title" }, props.title)
      : null;
    const bodyEl = h("div", { class: "admonition-body" }, props.children);
    return h(
      "div",
      {
        "data-admonition": variant,
        role: "note",
        "aria-label": variant,
      },
      titleEl,
      bodyEl,
    );
  };
}

// ---------------------------------------------------------------------------
// Main export
// ---------------------------------------------------------------------------

export const mdxComponents = {
  ...htmlOverrides,
  // CategoryNav — used in Wave 2's generated claude/index.mdx
  CategoryNav: CategoryNavWrapper,
  // Admonitions (:::note, :::tip, etc.)
  Note: makeAdmonition("note"),
  Tip: makeAdmonition("tip"),
  Info: makeAdmonition("info"),
  Warning: makeAdmonition("warning"),
  Danger: makeAdmonition("danger"),
  Caution: makeAdmonition("caution"),
  Important: makeAdmonition("important"),
  // Island — SSR pass-through (children render server-side)
  Island: IslandWrapper,
  // Stubs for tags that may appear in MDX content
  Details: MdxStub,
  CodeGroup: MdxStub,
  Tabs: MdxStub,
  TabItem: MdxStub,
  MathBlock: MdxStub,
};
