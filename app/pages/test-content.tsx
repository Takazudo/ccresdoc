/**
 * test-content.tsx — S4 fixture page.
 *
 * Renders sample content covering all content components and CSS features
 * ported in S4:
 *   - Breadcrumb
 *   - DocFrontmatter
 *   - DocMetainfo
 *   - DocTags
 *   - Toc / MobileToc
 *   - Admonitions (note, tip, info, warning, danger)
 *   - Code blocks with syntax highlighting, titles
 *   - Tables, blockquotes, lists, links, headings
 *   - Details / summary
 *   - Tabs container layout
 *   - .zd-content prose styles
 *
 * This page is also Wave 5 (S6)'s verification fixture — keep the filename
 * stable: app/pages/test-content.tsx.
 */

import DefaultLayout from "../layouts/default";
import { Breadcrumb } from "../components/breadcrumb";
import { DocFrontmatter } from "../components/doc-frontmatter";
import { DocMetainfo } from "../components/doc-metainfo";
import { DocTags } from "../components/doc-tags";

const SAMPLE_BREADCRUMBS = [
  { label: "Home", href: "/" },
  { label: "Docs", href: "/docs" },
  { label: "Test Content" },
];

const SAMPLE_FRONTMATTER = [
  { key: "title", value: "Test Content" },
  { key: "description", value: "Fixture page covering all S4 content styles" },
  { key: "tags", value: ["test", "fixture", "s4"] },
  { key: "draft", value: false },
];

const SAMPLE_METAINFO = {
  createdAt: "2026-01-01T00:00:00Z",
  updatedAt: "2026-05-05T00:00:00Z",
  author: "CCResDoc",
};

const SAMPLE_TAGS = ["test", "fixture", "s4", "content"];

/** Simulated HTML fragment as would be produced by ccresdoc-renderer. */
const CONTENT_HTML = `
<h2><a href="#headings" aria-hidden="true" class="anchor heading-link" id="headings"></a>Headings</h2>
<h3><a href="#h3-sample" aria-hidden="true" class="anchor heading-link" id="h3-sample"></a>H3 Sample heading</h3>
<h4><a href="#h4-sample" aria-hidden="true" class="anchor heading-link" id="h4-sample"></a>H4 Sample heading</h4>
<h5><a href="#h5-sample" aria-hidden="true" class="anchor heading-link" id="h5-sample"></a>H5 Sample heading</h5>

<h2><a href="#prose" aria-hidden="true" class="anchor heading-link" id="prose"></a>Prose content</h2>
<p>This is a regular paragraph with <strong>bold text</strong>, <em>italic text</em>, and <a href="https://claude.ai">an external link</a>. Links use the accent color token.</p>
<p>A second paragraph follows. The prose uses <code>--text-body</code> size at <code>--leading-relaxed</code> line-height.</p>
<blockquote>
<p>This is a blockquote. It has a left border and muted color.</p>
<p>Second blockquote paragraph.</p>
</blockquote>

<h2><a href="#lists" aria-hidden="true" class="anchor heading-link" id="lists"></a>Lists</h2>
<ul>
<li>Unordered item one</li>
<li>Unordered item two
  <ul>
  <li>Nested item A</li>
  <li>Nested item B</li>
  </ul>
</li>
<li>Unordered item three</li>
</ul>
<ol>
<li>Ordered item one</li>
<li>Ordered item two</li>
<li>Ordered item three</li>
</ol>

<h2><a href="#tables" aria-hidden="true" class="anchor heading-link" id="tables"></a>Tables</h2>
<table>
<thead>
<tr>
<th>Token</th>
<th>Value (light)</th>
<th>Value (dark)</th>
</tr>
</thead>
<tbody>
<tr>
<td><code>--color-accent</code></td>
<td>#a35e0f</td>
<td>#d69a66</td>
</tr>
<tr>
<td><code>--color-muted</code></td>
<td>#6b6b6b</td>
<td>#888888</td>
</tr>
<tr>
<td><code>--color-success</code></td>
<td>#266538</td>
<td>#93bb77</td>
</tr>
</tbody>
</table>

<h2><a href="#code-blocks" aria-hidden="true" class="anchor heading-link" id="code-blocks"></a>Code blocks</h2>
<p>Inline code: <code>const x = 42;</code> — uses code-bg and code-fg tokens.</p>
<div class="code-title">example.ts</div>
<pre><code class="language-typescript">// Example TypeScript — syntax highlighted by syntect
interface PageProps {
  title: string;
  tags: string[];
}

export function render({ title, tags }: PageProps): string {
  return \`&lt;h1&gt;\${title}&lt;/h1&gt;\`;
}
</code></pre>
<pre><code class="language-rust">// Rust example
fn main() {
    println!("Hello, world!");
}
</code></pre>

<h2><a href="#admonitions" aria-hidden="true" class="anchor heading-link" id="admonitions"></a>Admonitions</h2>
<aside class="admonition admonition-note">
<p>Note: This is a note admonition. Uses --color-accent border + background.</p>
<p>Additional content in the note.</p>
</aside>
<aside class="admonition admonition-tip">
<p>Tip: This is a tip admonition. Uses --color-success border + background.</p>
</aside>
<aside class="admonition admonition-info">
<p>Info: This is an info admonition. Uses --color-info border + background.</p>
</aside>
<aside class="admonition admonition-warning">
<p>Warning: This is a warning admonition. Uses --color-warning border + background.</p>
</aside>
<aside class="admonition admonition-danger">
<p>Danger: This is a danger admonition. Uses --color-danger border + background.</p>
</aside>

<h2><a href="#details" aria-hidden="true" class="anchor heading-link" id="details"></a>Details / Summary</h2>
<details>
<summary>Click to expand this section</summary>
<p>This content is hidden by default. The triangle indicator rotates on open.</p>
<p>Additional expanded content here.</p>
</details>
<details open>
<summary>Already open section</summary>
<p>This details element is open by default (<code>open</code> attribute).</p>
</details>

<h2><a href="#tabs" aria-hidden="true" class="anchor heading-link" id="tabs"></a>Tabs</h2>
<div class="tabs-container">
  <div class="tabs-list" role="tablist">
    <button class="tabs-tab" role="tab" aria-selected="true" aria-controls="tab-panel-1" id="tab-1">Tab One</button>
    <button class="tabs-tab" role="tab" aria-selected="false" aria-controls="tab-panel-2" id="tab-2">Tab Two</button>
    <button class="tabs-tab" role="tab" aria-selected="false" aria-controls="tab-panel-3" id="tab-3">Tab Three</button>
  </div>
  <div id="tab-panel-1" class="tabs-panel" role="tabpanel" aria-labelledby="tab-1">
    <p>Content of tab one. Tabs click handler is wired in S5.</p>
  </div>
  <div id="tab-panel-2" class="tabs-panel" role="tabpanel" aria-labelledby="tab-2" hidden>
    <p>Content of tab two.</p>
  </div>
  <div id="tab-panel-3" class="tabs-panel" role="tabpanel" aria-labelledby="tab-3" hidden>
    <p>Content of tab three.</p>
  </div>
</div>

<h2><a href="#horizontal-rule" aria-hidden="true" class="anchor heading-link" id="horizontal-rule"></a>Horizontal rule</h2>
<p>Content above the rule.</p>
<hr>
<p>Content below the rule.</p>
`;

export default function TestContentPage() {
  return (
    <DefaultLayout title="Test Content — S4 Fixture | CCResDoc">
      <Breadcrumb items={SAMPLE_BREADCRUMBS} />
      <DocFrontmatter entries={SAMPLE_FRONTMATTER} />
      <DocMetainfo info={SAMPLE_METAINFO} />
      <h1>Test Content Fixture (S4)</h1>
      <DocTags tags={SAMPLE_TAGS} />
      <div
        class="zd-content"
        dangerouslySetInnerHTML={{ __html: CONTENT_HTML }}
      />
    </DefaultLayout>
  );
}
