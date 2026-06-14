// CCResDoc site settings — minimal zudo-doc consumer configuration.
//
// No i18n, no versions, no search plugin, no doc-history, no tags.
// All content generation is handled by the Rust sidecar (Wave 2 / Wave 3).

import type {
  HeaderNavItem,
  HeaderRightItem,
} from "@takazudo/zudo-doc/header";

export interface ColorModeConfig {
  defaultMode: "light" | "dark";
  lightScheme: string;
  darkScheme: string;
  respectPrefersColorScheme: boolean;
}

export const settings = {
  siteName: "CCResDoc",
  siteDescription: "Browse Claude Code resources from your local ~/.claude/",
  base: "/" as string,
  trailingSlash: true as boolean,
  docsDir: "src/content/docs",
  defaultLocale: "en" as const,
  // No i18n: empty locales object (no locale-aware routes)
  locales: {} as Record<string, never>,
  // No versions
  versions: false as false,
  // No tags
  docTags: false as boolean,
  noindex: false as boolean,
  editUrl: false as string | false,
  githubUrl: false as string | false,
  siteUrl: "" as string,
  // Default dark, with light/dark toggle
  colorScheme: "Default Dark",
  colorMode: {
    defaultMode: "dark",
    lightScheme: "Default Light",
    darkScheme: "Default Dark",
    respectPrefersColorScheme: true,
  } as ColorModeConfig | false,
  // Sidebar resizer — drag the desktop sidebar's right edge to resize it
  // (width persisted in localStorage; client-side only, node-free).
  sidebarResizer: true as boolean,
  // Sidebar toggle for mobile
  sidebarToggle: true as boolean,
  // Simple footer (no link columns, just copyright)
  footer: {
    links: [] as Array<{ title: string; items: Array<{ label: string; href: string }> }>,
    copyright: `Copyright &copy; ${new Date().getFullYear()} CCResDoc`,
  } as { links: Array<{ title: string; items: Array<{ label: string; href: string }> }>; copyright?: string } | false,
  // Header nav — link to the claude docs overview section
  headerNav: [
    { label: "Claude", path: "/docs/claude", categoryMatch: "claude" },
  ] as HeaderNavItem[],
  headerRightItems: [
    { type: "component", component: "theme-toggle" },
  ] as HeaderRightItem[],
  // No default-locale-only paths (no i18n)
  defaultLocaleOnlyPrefixes: [] as string[],
  // Heading ID strategy: flat (simpler for auto-generated content)
  headingIdStrategy: "flat" as "flat" | "hierarchical",
  // No image enlarge
  imageEnlarge: false as boolean,
  // No HTML preview
  htmlPreview: undefined as undefined,
  // No frontmatter preview
  frontmatterPreview: false as false,
};
