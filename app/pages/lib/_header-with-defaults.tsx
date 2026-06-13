/** @jsxRuntime automatic */
/** @jsxImportSource preact */
// Simplified header wrapper for CCResDoc (single-locale, no versions, no search).
//
// Wraps @takazudo/zudo-doc's <Header> shell with site settings. No locale
// switcher, no version switcher, no search widget — just logo + nav + theme
// toggle + mobile sidebar toggle.

import type { JSX, VNode } from "preact";
import { Island } from "@takazudo/zfb";
import { Header, filterHeaderRightItems } from "@takazudo/zudo-doc/header";
import { ThemeToggle } from "@takazudo/zudo-doc/theme-toggle";
import { settings } from "@/config/settings";
import { withBase, stripBase, navHref } from "@/utils/base";
import { t } from "@/config/i18n";
import SidebarToggle from "@/components/sidebar-toggle";
import { loadDocs } from "../_data";
import { buildNavTree } from "@/utils/docs";

export interface HeaderWithDefaultsProps {
  lang?: string;
  currentPath?: string;
  currentSlug?: string;
  navSection?: string;
}

export function HeaderWithDefaults({
  lang = "en",
  currentPath = "",
  currentSlug,
  navSection,
}: HeaderWithDefaultsProps): JSX.Element {
  const themeDefaultMode =
    settings.colorMode ? settings.colorMode.defaultMode : undefined;

  // Build nav tree for the mobile sidebar
  const docs = loadDocs("docs");
  const allNodes = buildNavTree(docs, "en");

  // Scope sidebar tree to the active nav section when provided
  const sidebarNodes = navSection
    ? allNodes.filter(
        (n) => n.slug === navSection || n.slug.startsWith(navSection + "/"),
      )
    : allNodes;

  // Root menu items from headerNav for back-to-menu view
  const rootMenuItems = settings.headerNav.map((item) => ({
    label: item.label,
    href: withBase(item.path),
    children: item.children?.map((c) => ({
      label: c.label,
      href: withBase(c.path),
    })),
  }));

  // Mobile sidebar toggle island (hamburger + slide-in aside + SidebarTree)
  const sidebarToggle = Island({
    when: "visible",
    children: (
      <SidebarToggle
        nodes={sidebarNodes}
        currentSlug={currentSlug}
        rootMenuItems={rootMenuItems}
        backToMenuLabel="Main menu"
        themeDefaultMode={themeDefaultMode}
      />
    ),
  }) as unknown as VNode;

  // Theme toggle island
  const themeToggle = themeDefaultMode
    ? (Island({
        when: "load",
        children: <ThemeToggle defaultMode={themeDefaultMode} />,
      }) as unknown as VNode)
    : undefined;

  const headerRightItems = filterHeaderRightItems(settings.headerRightItems, {
    designTokenPanel: false,
    aiAssistant: false,
    colorMode: Boolean(settings.colorMode),
    hasLocales: false,
    hasVersions: false,
    hasGithubUrl: Boolean(settings.githubUrl),
  });

  return (
    <Header
      lang={lang}
      currentPath={currentPath}
      sidebarToggle={sidebarToggle}
      themeToggle={themeToggle}
      persistKey={`header-${lang}`}
      siteName={settings.siteName}
      headerNav={settings.headerNav}
      headerRightItems={headerRightItems}
      colorModeEnabled={Boolean(settings.colorMode)}
      hasLocales={false}
      hasVersions={false}
      githubRepoUrl={null}
      githubLabel={t("header.github")}
      urlHelpers={{ withBase, stripBase, navHref }}
      i18n={{ defaultLocale: "en", locales: ["en"] as readonly string[], t }}
    />
  );
}
