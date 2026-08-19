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

const PackageHeader = createHeaderWithDefaults({
  ...routeContext,
  components: {},
  hostBindings: {},
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

export const {
  BodyEndIslands,
  FooterWithDefaults,
  HeadWithDefaults,
  HeaderWithDefaults,
  SidebarWithDefaults,
  composeMetaTitle,
  renderDocPage,
} = createChrome(routeContext, { Header: DocsLandingHeader });
