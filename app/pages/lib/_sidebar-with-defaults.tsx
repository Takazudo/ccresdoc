/** @jsxRuntime automatic */
/** @jsxImportSource preact */
// Sidebar wrapper for CCResDoc doc pages.
//
// Wraps the host SidebarTree island with the nav tree for the active nav
// section. Single-locale (EN only), no versions.

import type { JSX } from "preact";
import { Island } from "@takazudo/zfb";
import SidebarTree from "@/components/sidebar-tree";
import { settings } from "@/config/settings";
import { withBase } from "@/utils/base";
import { loadDocs } from "../_data";
import { buildNavTree } from "@/utils/docs";

export interface SidebarWithDefaultsProps {
  currentSlug?: string;
  navSection?: string;
}

export function SidebarWithDefaults({
  currentSlug,
  navSection,
}: SidebarWithDefaultsProps): JSX.Element {
  const docs = loadDocs("docs");
  const allNodes = buildNavTree(docs, "en");

  const sidebarNodes = navSection
    ? allNodes.filter(
        (n) => n.slug === navSection || n.slug.startsWith(navSection + "/"),
      )
    : allNodes;

  const rootMenuItems = settings.headerNav.map((item) => ({
    label: item.label,
    href: withBase(item.path),
    children: item.children?.map((c) => ({
      label: c.label,
      href: withBase(c.path),
    })),
  }));

  const themeDefaultMode = settings.colorMode
    ? settings.colorMode.defaultMode
    : undefined;

  return Island({
    when: "load",
    children: (
      <SidebarTree
        nodes={sidebarNodes}
        currentSlug={currentSlug}
        rootMenuItems={rootMenuItems}
        backToMenuLabel="Main menu"
        themeDefaultMode={themeDefaultMode}
      />
    ),
  }) as unknown as JSX.Element;
}
