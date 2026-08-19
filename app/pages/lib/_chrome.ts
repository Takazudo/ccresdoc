// One package-facing chrome seam for all host-owned pages. zudo-doc owns the
// markup and behavior. The only host binding is a package-default header whose
// root URL helper maps the logo to the canonical `/docs/` shell.

import { createChrome } from "@takazudo/zudo-doc/chrome";
import { routeContext } from "./_route-context";

const DocsLandingHeader = createChrome({
  ...routeContext,
  withBase: (path: string) =>
    routeContext.withBase(path === "/" ? "/docs/" : path),
}).HeaderWithDefaults;

export const {
  BodyEndIslands,
  FooterWithDefaults,
  HeadWithDefaults,
  HeaderWithDefaults,
  SidebarWithDefaults,
  composeMetaTitle,
  renderDocPage,
} = createChrome(routeContext, { Header: DocsLandingHeader });
