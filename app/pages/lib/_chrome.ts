// One package-facing chrome seam for all host-owned pages. zudo-doc owns the
// markup and behavior. The host binding changes only product-level slots; the
// package derives desktop/mobile navigation and scoped sidebars from settings.

import { h } from "preact";
import { useEffect } from "preact/hooks";
import { Island } from "@takazudo/zfb";
import { createChrome } from "@takazudo/zudo-doc/chrome";
import {
  createHeaderWithDefaults,
} from "@takazudo/zudo-doc/header-with-defaults";
import { FindInPageInit } from "@takazudo/zudo-doc/find-in-page";
import { routeContext } from "./_route-context";
import { SettingsHeaderButton } from "./_settings-button";
import { AppearanceBridge } from "@/appearance/bridge";

export { openSettingsFromDocs, SettingsHeaderButton } from "./_settings-button";

function SettingsHeaderButtonIsland() {
  return Island({ when: "load", children: h(SettingsHeaderButton, {}) });
}

const SEARCH_DIALOG_SELECTOR = "dialog[data-search-dialog]";
const FIND_IN_PAGE_INPUT_SELECTOR = 'input[aria-label="Find in page"]';

export function SearchShortcutBoundary() {
  useEffect(() => {
    const handleKeyDown = (event: KeyboardEvent) => {
      if (!(event.metaKey || event.ctrlKey)) return;

      if (event.key === "f") {
        const searchDialog = document.querySelector<HTMLDialogElement>(SEARCH_DIALOG_SELECTOR);
        if (!searchDialog?.open) return;
      } else if (event.key === "k") {
        if (!document.querySelector<HTMLInputElement>(FIND_IN_PAGE_INPUT_SELECTOR)) return;
      } else {
        return;
      }

      event.preventDefault();
      event.stopImmediatePropagation();
    };

    document.addEventListener("keydown", handleKeyDown, true);
    return () => document.removeEventListener("keydown", handleKeyDown, true);
  }, []);
  return null;
}

SearchShortcutBoundary.displayName = "SearchShortcutBoundary";

const PackageHeader = createHeaderWithDefaults({
  ...routeContext,
  components: {},
  hostBindings: {
    headerRightComponents: { "ccresdoc-settings": SettingsHeaderButtonIsland },
  },
  withBase: (path: string) =>
    routeContext.withBase(path === "/" ? "/docs/" : path),
});

function AppearanceBodyEnd() {
  return [
    Island({ when: "load", children: h(AppearanceBridge, {}) }),
    Island({ when: "load", children: h(FindInPageInit, {}) }),
    Island({ when: "load", children: h(SearchShortcutBoundary, {}) }),
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
