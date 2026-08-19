// Serializable product settings for the plugin-free host route context.
// zudo-doc owns the behavior; CCResDoc owns only these product choices.

import { DEFAULT_SETTINGS } from "@takazudo/zudo-doc/config";
import type {
  HeaderNavItem,
  HeaderRightItem,
  Settings,
} from "@takazudo/zudo-doc/settings";

export const settings = {
  ...DEFAULT_SETTINGS,
  siteName: "CCResDoc",
  siteDescription: "Browse Claude Code resources from your local ~/.claude/",
  base: "/",
  trailingSlash: true,
  docsDir: "src/content/docs",
  defaultLocale: "en" as const,
  // No i18n: empty locales object (no locale-aware routes)
  locales: {} as Record<string, never>,
  // No versions
  versions: false as false,
  // No tags
  docTags: false,
  noindex: false,
  editUrl: false as string | false,
  githubUrl: false as string | false,
  siteUrl: "",
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
  dynamicPageTransition: true,
  findInPage: true,
  // Simple footer (no link columns, just copyright)
  footer: {
    links: [] as Array<{ title: string; items: Array<{ label: string; href: string }> }>,
    copyright: `Copyright © ${new Date().getFullYear()} CCResDoc`,
  },
  // Header nav — link to the claude docs overview section
  headerNav: [
    { label: "Claude", path: "/docs", categoryMatch: "claude" },
  ] as HeaderNavItem[],
  headerRightItems: [
    { type: "component", component: "theme-toggle" },
  ] as HeaderRightItem[],
  // No default-locale-only paths (no i18n)
  defaultLocaleOnlyPrefixes: [] as string[],
  // No image enlarge
  imageEnlarge: false,
  // No HTML preview
  htmlPreview: undefined,
  // No frontmatter preview
  frontmatterPreview: false as false,
  packageOwnedRoutes: false,
} satisfies Settings;
