/** @jsxRuntime automatic */
/** @jsxImportSource preact */
// Host-owned dynamic route: zfb dev cannot render an injected dynamic route,
// and plugins are intentionally disabled. All route data and rendering below
// are nevertheless provided by zudo-doc's public factories.

import type { JSX } from "preact";
import type {
  DocPageAutoIndexProps,
  DocPageEntryProps,
} from "@takazudo/zudo-doc/doc-page-props";
import { renderDocPage } from "../lib/_chrome";
import { routeContext } from "../lib/_route-context";

export const frontmatter = { title: "Docs" };

type DocPageProps = DocPageEntryProps | DocPageAutoIndexProps;

export function paths(): Array<{
  params: { slug: string[] };
  props: DocPageProps;
}> {
  const locale = routeContext.defaultLocale;
  const source = routeContext.resolveNavSource(locale, undefined, {
    keepUnlisted: true,
  });

  return routeContext
    .buildDocRouteEntries({ source, locale, routeSig: `docs;${locale}` })
    .map((item) => ({
      params: { slug: item.slugParams },
      props: item.props,
    }));
}

type PageArgs = DocPageProps & { params: { slug: string[] } };

export default function DocsPage(props: PageArgs): JSX.Element {
  return renderDocPage(props, {
    locale: routeContext.defaultLocale,
    docHistoryContentDir: routeContext.settings.docsDir,
  });
}
