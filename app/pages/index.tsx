/** @jsxRuntime automatic */
/** @jsxImportSource preact */

import type { JSX } from "preact";
import { prepareHomeData } from "@takazudo/zudo-doc/home-page";
import { HomePageView } from "./lib/_chrome";
import { routeContext } from "./lib/_route-context";

export const frontmatter = { title: "Home" };

/** Host-owned static route; zudo-doc owns its data preparation and body. */
export default function IndexPage(): JSX.Element {
  const locale = routeContext.defaultLocale;
  const data = prepareHomeData(routeContext, locale);

  return (
    <HomePageView
      locale={locale}
      {...data}
      wide={routeContext.settings.home?.wide ?? false}
    />
  );
}
