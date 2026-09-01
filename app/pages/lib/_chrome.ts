// One package-facing chrome seam for all host-owned pages. zudo-doc owns the
// markup and behavior. The host binding changes only product-level slots; the
// package derives desktop/mobile navigation and scoped sidebars from settings.

import { Fragment, h } from "preact";
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
  return h(
    "div",
    {
      "data-ccresdoc-browser-toolbar-shell": true,
      "data-zfb-transition-persist": "ccresdoc-browser-toolbar",
    },
    Island({ when: "load", children: h(CCResDocBrowserToolbar, {}) }),
  );
}

function ControlledSearchWidget(props: Record<string, unknown>) {
  const searchProps = props as unknown as SearchWidgetSlotProps;
  return h(SearchWidget, {
    ...searchProps,
    base: routeContext.withBase("/"),
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

function PackageHeader(props: Parameters<typeof PackageHeaderBase>[0]) {
  return h(Fragment, {}, h(BrowserToolbarIsland, {}), h(PackageHeaderBase, props));
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
  Header: PackageHeader,
  BodyEndIslands: AppearanceBodyEnd,
  headerRightComponents: { "ccresdoc-settings": SettingsHeaderButtonIsland },
});
