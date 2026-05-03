use lol_html::{element, HtmlRewriter, Settings};

use crate::RenderError;

/// Post-process HTML to strip `.md` / `.mdx` extensions from relative anchor hrefs.
///
/// Mirrors `rehype-strip-md-extension`: converts `./guide.md` → `./guide`.
/// External links (http://, https://) and fragment-only links (#…) are untouched.
pub fn apply_strip_md(html: &str) -> Result<String, RenderError> {
    let mut output = Vec::with_capacity(html.len());

    let mut rewriter = HtmlRewriter::new(
        Settings {
            element_content_handlers: vec![element!("a[href]", |el| {
                if let Some(href) = el.get_attribute("href") {
                    if let Some(new_href) = strip_md_extension(&href) {
                        el.set_attribute("href", &new_href)
                            .map_err(|e| e.to_string())?;
                    }
                }
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

/// Returns `Some(stripped)` if the href should be rewritten, `None` otherwise.
fn strip_md_extension(href: &str) -> Option<String> {
    // Skip absolute URLs and fragment-only links
    if href.starts_with('#') {
        return None;
    }
    // Skip scheme-based URLs (http:, https:, mailto:, etc.)
    if href.contains("://") || href.starts_with("mailto:") {
        return None;
    }

    // Strip .md or .mdx, optionally followed by a fragment
    if let Some(stripped) = strip_extension(href, ".mdx") {
        return Some(stripped);
    }
    if let Some(stripped) = strip_extension(href, ".md") {
        return Some(stripped);
    }

    None
}

fn strip_extension(href: &str, ext: &str) -> Option<String> {
    // href may be "path.md", "path.md#section", etc.
    if let Some(pos) = href.find(ext) {
        let after = &href[pos + ext.len()..];
        // Only strip if extension is at end or followed by '#'
        if after.is_empty() || after.starts_with('#') {
            let before = &href[..pos];
            return Some(format!("{before}{after}"));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_md_extension() {
        let html = r#"<a href="guide.md">link</a>"#;
        let out = apply_strip_md(html).unwrap();
        assert!(out.contains(r#"href="guide""#), "got: {out}");
    }

    #[test]
    fn strips_md_with_fragment() {
        let html = r#"<a href="guide.md#section">link</a>"#;
        let out = apply_strip_md(html).unwrap();
        assert!(out.contains(r#"href="guide#section""#), "got: {out}");
    }

    #[test]
    fn leaves_external_links() {
        let html = r#"<a href="https://example.com/foo.md">link</a>"#;
        let out = apply_strip_md(html).unwrap();
        assert!(out.contains("https://example.com/foo.md"), "got: {out}");
    }

    #[test]
    fn leaves_fragment_only() {
        let html = "<a href=\"#section\">link</a>";
        let out = apply_strip_md(html).unwrap();
        assert!(out.contains("href=\"#section\""), "got: {out}");
    }
}
