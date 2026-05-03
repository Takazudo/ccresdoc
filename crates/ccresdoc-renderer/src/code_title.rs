use lol_html::{element, HtmlRewriter, Settings};

use crate::RenderError;

/// Post-process HTML to extract `title="..."` meta from code blocks and insert
/// a sibling `<div class="code-title">` immediately before the `<pre>`.
///
/// Comrak passes through the meta string of a fenced code block as a
/// `data-meta` attribute on the `<code>` element.  We scan for that attribute,
/// extract any `title="..."` value, and emit a title div.
pub fn apply_code_titles(html: &str) -> Result<String, RenderError> {
    // Collect (pre_index -> title) from the HTML
    let titles = extract_titles(html);

    if titles.is_empty() || titles.iter().all(|t| t.is_none()) {
        return Ok(html.to_owned());
    }

    let mut output = Vec::with_capacity(html.len() + 256);
    let mut pre_counter = 0usize;

    let mut rewriter = HtmlRewriter::new(
        Settings {
            element_content_handlers: vec![element!("pre", |el| {
                let maybe_title = titles.get(pre_counter).and_then(|t| t.as_ref());
                if let Some(title) = maybe_title {
                    el.before(
                        &format!("<div class=\"code-title\">{}</div>", escape_html(title)),
                        lol_html::html_content::ContentType::Html,
                    );
                }
                pre_counter += 1;
                Ok(())
            })],
            ..Settings::default()
        },
        |c: &[u8]| output.extend_from_slice(c),
    );

    rewriter
        .write(html.as_bytes())
        .map_err(|e| RenderError::LolHtml(e.to_string()))?;
    rewriter
        .end()
        .map_err(|e| RenderError::LolHtml(e.to_string()))?;

    String::from_utf8(output).map_err(|e| RenderError::LolHtml(e.to_string()))
}

/// Scan `html` for `<code data-meta="...title=...">` patterns inside `<pre>` blocks
/// and return a Vec where index `i` is the title (or None) for the i-th `<pre>`.
fn extract_titles(html: &str) -> Vec<Option<String>> {
    let mut results = Vec::new();
    let mut search = html;

    while let Some(pre_pos) = find_tag(search, "<pre") {
        let after_pre = &search[pre_pos..];

        // Find the closing > of this <pre ...>
        let pre_end = after_pre.find('>').map(|p| p + 1).unwrap_or(after_pre.len());
        let after_pre_tag = &after_pre[pre_end..];

        // Look for a <code ... data-meta=...> inside this <pre>
        let title = if let Some(code_pos) = find_tag(after_pre_tag, "<code") {
            let code_snippet = &after_pre_tag[code_pos..];
            let code_end = code_snippet.find('>').map(|p| p + 1).unwrap_or(code_snippet.len());
            let code_tag = &code_snippet[..code_end];
            extract_title_from_code_tag(code_tag)
        } else {
            None
        };

        results.push(title);

        // Advance past this <pre
        search = &after_pre[pre_end..];
    }

    results
}

fn find_tag(s: &str, tag: &str) -> Option<usize> {
    s.find(tag)
}

/// Extract the title from a `<code ...data-meta="..."...>` tag string.
fn extract_title_from_code_tag(tag: &str) -> Option<String> {
    let meta_attr = "data-meta=\"";
    let start = tag.find(meta_attr)? + meta_attr.len();
    let rest = &tag[start..];
    let end = rest.find('"')?;
    let meta = &rest[..end];

    // Unescape HTML entities (comrak encodes " as &quot; in attribute values)
    let meta_unescaped = meta.replace("&quot;", "\"").replace("&amp;", "&");

    extract_title_attr(&meta_unescaped)
}

fn extract_title_attr(meta: &str) -> Option<String> {
    let key = "title=\"";
    let start = meta.find(key)? + key.len();
    let rest = &meta[start..];
    let end = rest.find('"')?;
    Some(rest[..end].to_owned())
}

fn escape_html(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#x27;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inserts_code_title_div() {
        // Simulated comrak output for ```ts title="foo.ts"
        let html = "<pre><code class=\"language-ts\" data-meta=\"title=&quot;foo.ts&quot;\">let x = 1;\n</code></pre>";
        let out = apply_code_titles(html).unwrap();
        assert!(out.contains("<div class=\"code-title\">foo.ts</div>"), "got: {out}");
        assert!(out.contains("<pre>"));
    }

    #[test]
    fn no_title_no_div() {
        let html = "<pre><code class=\"language-ts\">let x = 1;\n</code></pre>";
        let out = apply_code_titles(html).unwrap();
        assert!(!out.contains("code-title"));
    }
}
