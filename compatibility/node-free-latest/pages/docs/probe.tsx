/** @jsxRuntime automatic */
/** @jsxImportSource preact */

import { getEntry } from "@takazudo/zfb/content";
import { mdxComponents } from "../_mdx-components";

export default function ProbeDocPage() {
  const entry = getEntry<{ title: string }>("docs", "probe");
  if (!entry) return <main><h1>Missing probe entry</h1></main>;
  return (
    <main>
      <h1>{entry.data.title}</h1>
      <entry.Content components={mdxComponents} />
    </main>
  );
}
