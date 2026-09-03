// One package-facing chrome seam for all host-owned pages. zudo-doc owns the
// markup and behavior. The host binding changes only product-level slots; the
// package derives desktop/mobile navigation and scoped sidebars from settings.

import { h } from "preact";
import { Island } from "@takazudo/zfb";
import { createChrome } from "@takazudo/zudo-doc/chrome";
import {
  createHeaderWithDefaults,
} from "@takazudo/zudo-doc/header-with-defaults";
import { FindInPageInit } from "@takazudo/zudo-doc/find-in-page";
import { SearchWidget } from "@takazudo/zudo-doc/search-widget";
import type { SearchWidgetSlotProps } from "@takazudo/zudo-doc/chrome-bindings";
import { routeContext } from "./_route-context";
import { SettingsHeaderButton } from "./_settings-button";
import { AppearanceBridge } from "@/appearance/bridge";
import { CCResDocBrowserToolbar } from "@/browser-chrome/toolbar";

export { openSettingsFromDocs, SettingsHeaderButton } from "./_settings-button";

function SettingsHeaderButtonIsland() {
  return Island({ when: "load", children: h(SettingsHeaderButton, {}) });
}

function BrowserToolbarIsland() {
  const island = Island({ when: "load", children: h(CCResDocBrowserToolbar, {}) });
  // Persistence belongs on the island marker itself. zfb moves persisted
  // ancestor wrappers too, but only an island carrying this ID is exempt from
  // pre-swap unmount, which keeps its history listeners alive for page-load.
  return h(island.type, {
    ...island.props,
    "data-ccresdoc-browser-toolbar-shell": true,
    "data-zfb-transition-persist": "ccresdoc-browser-toolbar",
  });
}

function ControlledSearchWidget(props: Record<string, unknown>) {
  const searchProps = props as unknown as SearchWidgetSlotProps;
  return h(SearchWidget, {
    ...searchProps,
    base: routeContext.withBase("/docs/"),
    disableBuiltInShortcut: true,
  });
}

const PackageHeaderBase = createHeaderWithDefaults({
  ...routeContext,
  components: {},
  hostBindings: {
    headerRightComponents: { "ccresdoc-settings": SettingsHeaderButtonIsland },
    SearchWidget: ControlledSearchWidget,
  },
  withBase: (path: string) =>
    routeContext.withBase(path === "/" ? "/docs/" : path),
});

// The browser toolbar row and the package header are one fixed chrome unit.
// The host-owned wrapper carries the sticky positioning and the stacking
// context; both children keep their own independent persist keys.
//
// The wrapper itself must NEVER carry data-zfb-transition-persist. zfb's
// swapBodyElement flat-enumerates persist roots and lifts each one
// independently, so a nested child root's swap target is discarded together
// with the incoming ancestor subtree it lives in; zudo-doc's
// nested-island-props-refresh drops nested roots outright for the same reason.
// A persisted wrapper would also defeat the header's locale-keyed replacement.
function ChromeRegion(props: Parameters<typeof PackageHeaderBase>[0]) {
  return h(
    "div",
    { "data-ccresdoc-chrome-region": true },
    h(BrowserToolbarIsland, {}),
    h(PackageHeaderBase, props),
  );
}

function AppearanceBodyEnd() {
  return [
    Island({ when: "load", children: h(AppearanceBridge, {}) }),
    Island({ when: "load", children: h(FindInPageInit, { disableBuiltInShortcut: true }) }),
  ];
}

export const {
  BodyEndIslands,
  FooterWithDefaults,
  HeadWithDefaults,
  HeaderWithDefaults,
  SidebarWithDefaults,
  composeMetaTitle,
  renderDocPage,
} = createChrome(routeContext, {
  Header: ChromeRegion,
  BodyEndIslands: AppearanceBodyEnd,
  headerRightComponents: { "ccresdoc-settings": SettingsHeaderButtonIsland },
});
