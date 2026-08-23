// One package-facing chrome seam for all host-owned pages. zudo-doc owns the
// markup and behavior. The host binding changes only product-level navigation:
// the logo targets `/docs/`, and the mobile drawer receives the same unscoped
// resource tree as the desktop sidebar without a now-empty root-menu layer.

import { cloneElement, h, type VNode } from "preact";
import { Island } from "@takazudo/zfb";
import { createChrome } from "@takazudo/zudo-doc/chrome";
import {
  createHeaderWithDefaults,
  type HeaderWithDefaultsProps,
} from "@takazudo/zudo-doc/header-with-defaults";
import {
  SidebarToggle,
  type SidebarToggleProps,
} from "@takazudo/zudo-doc/sidebar-toggle-island";
import { routeContext } from "./_route-context";
import { AppearanceBridge } from "@/appearance/bridge";

type TauriGlobal = typeof globalThis & {
  __TAURI__?: { core?: { invoke?: (command: string) => Promise<unknown> } };
};

export function openSettingsFromDocs(
  root: TauriGlobal = globalThis as TauriGlobal,
): Promise<unknown> {
  const invoke = root.__TAURI__?.core?.invoke;
  if (typeof invoke !== "function") {
    return Promise.reject(new Error("Settings are available only in the CCResDoc app."));
  }
  return invoke("open_settings_window");
}

export function SettingsHeaderButton() {
  return h(
    "button",
    {
      type: "button",
      title: "Settings",
      "aria-label": "Open Settings",
      class: "flex min-h-11 min-w-11 items-center justify-center rounded-md text-muted focus-visible:outline-2 focus-visible:outline-offset-2",
      onClick: () => {
        void openSettingsFromDocs().catch((error) => {
          console.error("open_settings_window failed:", error);
        });
      },
    },
    h(
      "svg",
      { viewBox: "0 0 24 24", width: 20, height: 20, fill: "none", stroke: "currentColor", "stroke-width": 2, "aria-hidden": "true" },
      h("path", { d: "M12 15.5a3.5 3.5 0 1 0 0-7 3.5 3.5 0 0 0 0 7Z" }),
      h("path", { d: "M19.4 15a1.7 1.7 0 0 0 .34 1.88l.06.06-2.86 2.86-.06-.06A1.7 1.7 0 0 0 15 19.4a1.7 1.7 0 0 0-1 .6 1.7 1.7 0 0 0-.4 1.1V21H9.6v-.1A1.7 1.7 0 0 0 8.5 19.4a1.7 1.7 0 0 0-1.88.34l-.06.06-2.86-2.86.06-.06A1.7 1.7 0 0 0 4.1 15a1.7 1.7 0 0 0-.6-1 1.7 1.7 0 0 0-1.1-.4H2V9.6h.4A1.7 1.7 0 0 0 4.1 8.5a1.7 1.7 0 0 0-.34-1.88l-.06-.06L6.56 3.7l.06.06A1.7 1.7 0 0 0 8.5 4.1a1.7 1.7 0 0 0 1-.6 1.7 1.7 0 0 0 .4-1.1V2h4v.4A1.7 1.7 0 0 0 15 4.1a1.7 1.7 0 0 0 1.88-.34l.06-.06 2.86 2.86-.06.06A1.7 1.7 0 0 0 19.4 8.5a1.7 1.7 0 0 0 .6 1 1.7 1.7 0 0 0 1.1.4h.4v4h-.4A1.7 1.7 0 0 0 19.4 15Z" }),
    ),
  );
}

const PackageHeader = createHeaderWithDefaults({
  ...routeContext,
  components: {},
  hostBindings: {
    headerRightComponents: { "ccresdoc-settings": SettingsHeaderButton },
  },
  withBase: (path: string) =>
    routeContext.withBase(path === "/" ? "/docs/" : path),
});

type HeaderElementProps = {
  sidebarToggle?: VNode<{ children?: VNode<SidebarToggleProps> }>;
};

function DocsLandingHeader(props: HeaderWithDefaultsProps): VNode {
  // The package intentionally emits no mobile nodes for an unsectioned page.
  // CCResDoc has no header sections at all, so an empty-string section asks the
  // package builder for its full tree (`filterByCategory` treats it unscoped).
  const header = PackageHeader({
    ...props,
    navSection: props.navSection ?? "",
  }) as VNode<HeaderElementProps>;
  const sidebarIsland = header.props.sidebarToggle;
  const packageSidebar = sidebarIsland?.props.children;
  if (!packageSidebar) {
    throw new Error("zudo-doc package header did not provide SidebarToggle");
  }

  // `headerNav: []` correctly removes desktop nav, but the package serializes
  // that empty array as a truthy root-menu layer on mobile. Rebuild only the
  // public island boundary with those optional props omitted so the drawer
  // opens directly on the full resource tree and has no dead Back control.
  const sidebarToggle = Island({
    when: "visible",
    children: h(SidebarToggle, {
      ...packageSidebar.props,
      rootMenuItems: undefined,
      backToMenuLabel: undefined,
    }),
  });

  return cloneElement(header, { sidebarToggle });
}

function AppearanceBodyEnd() {
  return Island({ when: "load", children: h(AppearanceBridge, {}) });
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
  Header: DocsLandingHeader,
  BodyEndIslands: AppearanceBodyEnd,
  headerRightComponents: { "ccresdoc-settings": SettingsHeaderButton },
});
