// Serializable product settings for the plugin-free host route context.
// zudo-doc owns the behavior; CCResDoc owns only these product choices.

import { DEFAULT_SETTINGS } from "@takazudo/zudo-doc/config";
import themePackCatalog from "@takazudo/zudo-doc/catalog";
import runtimeThemePackSlugs from "./theme-pack-slugs.json";
import type {
  HeaderRightItem,
  Settings,
} from "@takazudo/zudo-doc/settings";

const catalogThemePackSlugs = themePackCatalog.packs.map(({ slug }) => slug);
if (JSON.stringify(catalogThemePackSlugs) !== JSON.stringify(runtimeThemePackSlugs)) {
  throw new Error("CCResDoc runtime theme-pack catalog is out of sync");
}
export const themePackSlugs = [...runtimeThemePackSlugs];

export const settings = {
  ...DEFAULT_SETTINGS,
  siteName: "CCResDoc",
  siteDescription: "Browse Claude Code resources from your local ~/.claude/",
  base: "/",
  trailingSlash: true,
  docsDir: "src/content/docs",
  // The package generators stay off: the native host writes these resources.
  claudeResources: false,
  codexResources: false,
  // Every generated top-level category belongs in the site tree.
  siteTreeNavIgnore: [],
  defaultLocale: "en" as const,
  // No i18n: empty locales object (no locale-aware routes)
  locales: {} as Record<string, never>,
  // No versions
  versions: false as false,
  // No tags
  docTags: false,
  noindex: true,
  editUrl: false as string | false,
  githubUrl: false as string | false,
  siteUrl: "",
  metaTags: {
    description: false,
    keywords: false,
    ogImage: false,
    ogSiteName: false,
    twitterCard: false,
  },
  cjkFriendly: true,
  // Default dark, with light/dark toggle
  colorScheme: "Default Dark",
  colorMode: {
    defaultMode: "dark",
    lightScheme: "Default Light",
    darkScheme: "Default Dark",
    respectPrefersColorScheme: true,
  },
  // Sidebar resizer — drag the desktop sidebar's right edge to resize it
  // (width persisted in localStorage; client-side only, node-free).
  sidebarResizer: true,
  // Sidebar toggle for mobile
  sidebarToggle: true,
  tocToggle: true,
  dynamicPageTransition: true,
  findInPage: true,
  // Simple footer (no link columns, just copyright)
  footer: {
    links: [] as Array<{ title: string; items: Array<{ label: string; href: string }> }>,
    copyright: `Copyright © ${new Date().getFullYear()} CCResDoc`,
  },
  // The resource tree belongs in the unscoped sidebar; the logo is the only
  // primary navigation entry and points directly at the canonical doc shell.
  headerNav: [],
  headerRightItems: [
    { type: "component", component: "ccresdoc-settings" },
    { type: "component", component: "theme-toggle" },
  ] as HeaderRightItem[],
  // No default-locale-only paths (no i18n)
  defaultLocaleOnlyPrefixes: [] as string[],
  imageEnlarge: true,
  // No HTML preview
  htmlPreview: undefined,
  // No frontmatter preview
  frontmatterPreview: false as false,
  packageOwnedRoutes: false,
  themePack: "default",
  themePackSwitcher: true,
  themePacks: themePackSlugs,
} satisfies Settings;
