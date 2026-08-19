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
import { settings } from "@/config/settings";

const payload = {
  settings,
  translations: defaultTranslations,
  tagVocabulary: [],
  colorSchemes: defaultColorSchemes,
  // The routes plugin normally resolves theme-pack metadata. With no plugin,
  // null intentionally keeps the switcher/bootstrap inert.
  themePackRegistry: null,
} satisfies RouteContextPayload<typeof settings>;

export const routeContext = createRouteContext(payload);
