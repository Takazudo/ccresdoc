# ccresdoc-renderer

Rust library crate that renders Markdown to an HTML body fragment using
comrak, syntect, and lol_html. It is the Rust port of the Astro pipeline
used by the original CCResDoc site (remark-directive + remark-admonitions +
rehype-code-title + rehype-heading-links + rehype-strip-md-extension + shiki).

## Usage

```rust
use ccresdoc_renderer::{render_markdown, RenderOptions};

let opts = RenderOptions::default();
let html = render_markdown("# Hello\n\nworld", &opts).unwrap();
```

## RenderOptions

- `heading_id_prefix`: Prefix for generated heading id attributes. Default: empty string.
- `strip_md_extension_in_links`: Strip .md/.mdx from relative anchor hrefs. Default: true.
- `admonition_kinds`: Recognised admonition types. Default: note, tip, info, warning, danger.
- `syntax_highlight_theme`: Syntect theme name. Use "dracula" (dark) or "github-light" (light).

## Supported Markdown Features

- GFM tables, strikethrough, tasklists (comrak built-ins)
- Heading IDs with auto-link wrapping (heading-link class)
- Code titles: triple-backtick with title="..." meta → div.code-title sibling
- Admonitions: :::note ... ::: → aside.admonition.admonition-{kind}
- Internal link .md stripping
- Syntax highlighting via syntect (inline style spans)
- YAML frontmatter stripped before rendering

## Runtime Sentinels

Two strings are reserved for runtime slot substitution by the Tauri shell and
MUST NEVER appear in rendered output. The renderer ensures this by HTML-encoding
the snowman delimiter character (U+2603) wherever it appears in user content.

- SENTINEL_CONTENT: snowman + CCRESDOC_CONTENT_SLOT + snowman
- SENTINEL_TITLE: snowman + CCRESDOC_TITLE_SLOT + snowman

The snowman character (U+2603, ☃) is HTML-encoded to &#x2603; in the renderer
output so these exact byte sequences can never appear in rendered HTML.

## CSS Class Contract

The following classes are emitted and must be styled by the S4 CSS layer:

| Class / Selector | Source | S4 Must Style |
|---|---|---|
| aside.admonition | admonition pre-process | base box (padding, border-radius, margin) |
| aside.admonition-note | kind=note | colour treatment |
| aside.admonition-tip | kind=tip | colour treatment |
| aside.admonition-info | kind=info | colour treatment |
| aside.admonition-warning | kind=warning | colour treatment |
| aside.admonition-danger | kind=danger | colour treatment |
| div.code-title | code-title rewrite | label above sibling pre |
| pre > code[class*="language-"] | comrak code blocks | mono font + padding |
| a.heading-link | heading-link rewrite | invisible-until-hover anchor on h1-h6 |
| table, th, td | comrak GFM tables | basic borders + padding |

See tests/fixtures/class-contract.html for a rendered example of every class.
