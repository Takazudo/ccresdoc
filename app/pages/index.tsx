/** @jsxRuntime automatic */
/** @jsxImportSource preact */

import type { JSX } from "preact";
import DocsPage, { paths as docsPaths } from "./docs/[[...slug]]";

export const frontmatter = { title: "Claude Code Resources" };

/** `/` is an SSR alias of the canonical `/docs/` document shell. */
export default function IndexPage(): JSX.Element {
  const root = docsPaths().find((item) => item.params.slug.length === 0);
  if (!root) {
    throw new Error("The canonical /docs/ route is missing");
  }

  return <DocsPage params={root.params} {...root.props} />;
}
