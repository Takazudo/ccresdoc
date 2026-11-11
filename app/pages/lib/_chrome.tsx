// One package-facing chrome seam for all host-owned pages. zudo-doc owns the
// markup and behavior. The host binding changes only product-level slots; the
// package derives desktop/mobile navigation and scoped sidebars from settings.

import { Island } from "@takazudo/zfb";
import { createChrome } from "@takazudo/zudo-doc/chrome";
import { createHeaderWithDefaults } from "@takazudo/zudo-doc/header-with-defaults";
import { FindInPageInit } from "@takazudo/zudo-doc/find-in-page";
import { SearchWidget, type SearchWidgetProps } from "@takazudo/zudo-doc/search-widget";
import { PageLoadingOverlay } from "@takazudo/zudo-doc/page-loading";
import { routeContext } from "./_route-context";
import { SettingsHeaderButton } from "./_settings-button";
import { AppearanceBridge } from "@/appearance/bridge";
import { CCResDocBrowserToolbar } from "@/browser-chrome/toolbar";

export { openSettingsFromDocs, SettingsHeaderButton } from "./_settings-button";

export const BROWSER_TOOLBAR_PERSIST_KEY = "ccresdoc-browser-toolbar";

// Stored in header component maps, so it keeps zfb's Fragment-wrapped
// fixed-target shape; a bare Island return would escape into an opaque map.
function SettingsHeaderButtonIsland() {
  return (
    <>
      <Island when="load">
        <SettingsHeaderButton />
      </Island>
    </>
  );
}

// The toolbar island persists across client navigation so its history
// listeners stay alive for page-load. The host-owned exclusion marker sits on
// the toolbar's own <nav>, not on the island transport.
function BrowserToolbarIsland() {
  return (
    <Island when="load" persist={BROWSER_TOOLBAR_PERSIST_KEY}>
      <CCResDocBrowserToolbar />
    </Island>
  );
}

function ControlledSearchWidget(props: Record<string, unknown>) {
  const searchProps = props as unknown as SearchWidgetProps;
  return (
    <SearchWidget
      {...searchProps}
      base={routeContext.withBase("/docs/")}
      disableBuiltInShortcut={true}
    />
  );
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
// The wrapper itself must NEVER carry a persist key. zfb lifts each persist
// root independently, so a nested child root's swap target is discarded
// together with the incoming ancestor subtree it lives in. A persisted
// wrapper would also defeat the header's locale-keyed replacement.
function ChromeRegion(props: Parameters<typeof PackageHeaderBase>[0]) {
  return (
    <div data-ccresdoc-chrome-region="">
      <BrowserToolbarIsland />
      <PackageHeaderBase {...props} />
    </div>
  );
}

function AppearanceBodyEnd() {
  return (
    <>
      {routeContext.settings.dynamicPageTransition ? <PageLoadingOverlay /> : null}
      <Island when="load">
        <AppearanceBridge />
      </Island>
      <Island when="load">
        <FindInPageInit disableBuiltInShortcut={true} />
      </Island>
    </>
  );
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
