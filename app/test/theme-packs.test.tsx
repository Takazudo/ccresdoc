import { describe, expect, it } from "vitest";
import themePackCatalog from "@takazudo/zudo-doc/catalog";
import { resolveThemePackSsrSlug } from "@takazudo/zudo-doc/theme";
import { settings, themePackSlugs } from "@/config/settings";
import { themePackRegistry } from "../pages/lib/_route-context";
import { config } from "../zfb.config";

describe("CCResDoc theme-pack host integration", () => {
  it("derives settings and the typed route registry from the public catalog", () => {
    expect(themePackSlugs).toEqual(
      themePackCatalog.packs.map(({ slug }) => slug),
    );
    expect(settings.themePack).toBe("default");
    expect(settings.themePackSwitcher).toBe(true);
    expect(settings.themePacks).toEqual(themePackSlugs);
    expect(themePackRegistry).toEqual(
      themePackCatalog.packs.map((meta) => ({
        slug: meta.slug,
        meta,
        hasStylesheet: meta.slug !== "default",
      })),
    );
    expect(themePackRegistry[0]).toMatchObject({
      slug: "default",
      hasStylesheet: false,
    });
    expect(themePackRegistry.slice(1).every(({ hasStylesheet }) => hasStylesheet))
      .toBe(true);
    expect(resolveThemePackSsrSlug(themePackRegistry, settings)).toBe(
      "default",
    );
  });

  it("keeps the compatible preset settings and intentional host deviations", () => {
    expect(settings).toMatchObject({
      cjkFriendly: true,
      imageEnlarge: true,
      tocToggle: true,
      noindex: true,
      sidebarResizer: true,
      sidebarToggle: true,
      dynamicPageTransition: true,
      findInPage: true,
      claudeResources: false,
      codexResources: false,
      siteTreeNavIgnore: [],
      packageOwnedRoutes: false,
      headerNav: [
        { label: "Claude", path: "/docs/claude", categoryMatch: "claude", versioned: false },
        { label: "Codex", path: "/docs/codex", categoryMatch: "codex", versioned: false },
      ],
      metaTags: {
        description: false,
        keywords: false,
        ogImage: false,
        ogSiteName: false,
        twitterCard: false,
      },
    });
    expect(settings.headerRightItems).toEqual([
      { type: "component", component: "ccresdoc-settings" },
      { type: "component", component: "theme-toggle" },
    ]);
    expect(config.plugins).toEqual([]);
    expect(config.markdown?.cjkFriendly).toBe(true);
  });
});
