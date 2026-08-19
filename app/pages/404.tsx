/** @jsxRuntime automatic */
/** @jsxImportSource preact */

import type { JSX } from "preact";
import { DocLayoutWithDefaults } from "@takazudo/zudo-doc/doclayout";
import {
  BodyEndIslands,
  FooterWithDefaults,
  HeadWithDefaults,
  HeaderWithDefaults,
  composeMetaTitle,
} from "./lib/_chrome";
import { routeContext } from "./lib/_route-context";

export const frontmatter = { title: "404" };

/** Host-owned 404 route using only current package chrome resources. */
export default function NotFoundPage(): JSX.Element {
  const locale = routeContext.defaultLocale;
  const title = "Page Not Found";

  return (
    <DocLayoutWithDefaults
      title={composeMetaTitle(title)}
      head={<HeadWithDefaults title={title} />}
      lang={locale}
      noindex
      hideSidebar
      hideToc
      sidebarOverride={<></>}
      headerOverride={<HeaderWithDefaults lang={locale} />}
      footerOverride={<FooterWithDefaults lang={locale} />}
      bodyEndComponents={
        <BodyEndIslands basePath={routeContext.settings.base} />
      }
      enableClientRouter={routeContext.settings.dynamicPageTransition}
    >
      <div class="min-h-[60vh] flex flex-col items-center justify-center px-hsp-2xl py-vsp-xl">
        <h1 class="text-display font-bold mb-vsp-md">404</h1>
        <p class="text-title text-muted mb-vsp-xl">Page not found.</p>
        <a
          href={routeContext.withBase("/")}
          class="bg-accent px-hsp-lg py-vsp-xs font-medium text-bg hover:bg-accent-hover"
        >
          Back to Home
        </a>
      </div>
    </DocLayoutWithDefaults>
  );
}
