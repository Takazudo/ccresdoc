// One package-facing chrome seam for all host-owned pages. The default
// bindings are intentional: current zudo-doc owns the header, footer, sidebar,
// MDX component map (including CategoryNav), body-end islands, and doc renderer.
// CCResDoc has no product-specific chrome override at this layer.

import { createChrome } from "@takazudo/zudo-doc/chrome";
import { routeContext } from "./_route-context";

export const {
  BodyEndIslands,
  FooterWithDefaults,
  HeadWithDefaults,
  HeaderWithDefaults,
  HomePageView,
  SidebarWithDefaults,
  composeMetaTitle,
  renderDocPage,
} = createChrome(routeContext);
