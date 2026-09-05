// Server-side route adapter for CCResDoc's plugin-free host-owned pages.
//
// zudo-doc normally serializes this payload through a virtual module emitted
// by its routes plugin. CCResDoc deliberately runs with `plugins: []`, so the
// host supplies the same serializable resources directly and lets the package
// reconstruct all URL, collection, route, heading, and navigation behavior.

import { createRouteContext } from "@takazudo/zudo-doc/route-context";
import type { RouteContextPayload } from "@takazudo/zudo-doc/route-context";
import { defaultColorSchemes } from "@takazudo/zudo-doc/color-schemes-defaults";
import { defaultTranslations } from "@takazudo/zudo-doc/i18n-defaults";
import themePackCatalog from "@takazudo/zudo-doc/catalog";
import { settings } from "@/config/settings";

type ThemePackRegistry = NonNullable<
  RouteContextPayload["themePackRegistry"]
>;

export const themePackRegistry = themePackCatalog.packs satisfies ThemePackRegistry;

const payload = {
  settings,
  translations: defaultTranslations,
  tagVocabulary: [],
  colorSchemes: defaultColorSchemes,
  // The routes plugin normally resolves this browser-safe registry. CCResDoc's
  // host-owned routes recreate it from the public catalog without filesystem
  // access so the runtime remains node-free.
  themePackRegistry,
} satisfies RouteContextPayload<typeof settings>;

export const routeContext = createRouteContext(payload);
