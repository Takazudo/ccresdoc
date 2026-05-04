use lol_html::{element, HtmlRewriter, Settings};

use crate::RenderError;

/// Post-process HTML to add `heading-link` class to the heading anchor.
///
/// Comrak's `header_ids` extension generates:
///   `<hN><a href="#id" aria-hidden="true" class="anchor" id="PREFIX+id">...</a>text</hN>`
///
/// We add `heading-link` to the anchor's class list, mirroring the
/// `rehype-heading-links` plugin which makes heading anchors stylable.
/// This satisfies the CSS class contract: `a.heading-link` on h2-h6.
pub fn apply_heading_links(html: &str) -> Result<String, RenderError> {
    let mut output = Vec::with_capacity(html.len());

    let mut rewriter = HtmlRewriter::new(
        Settings {
            element_content_handlers: vec![
                // Target the hidden anchor that comrak emits inside headings.
                // It has aria-hidden="true" and class="anchor".
                // We add "heading-link" to its class list.
                element!("h1 a[aria-hidden], h2 a[aria-hidden], h3 a[aria-hidden], h4 a[aria-hidden], h5 a[aria-hidden], h6 a[aria-hidden]", |el| {
                    let existing_class = el.get_attribute("class").unwrap_or_default();
                    let new_class = if existing_class.is_empty() {
                        "heading-link".to_owned()
                    } else {
                        format!("{existing_class} heading-link")
                    };
                    el.set_attribute("class", &new_class)
                        .map_err(|e| e.to_string())?;
                    Ok(())
                }),
            ],
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adds_heading_link_class_to_comrak_anchor() {
        // Simulate comrak output with a heading anchor
        let html = "<h2><a href=\"#hello\" aria-hidden=\"true\" class=\"anchor\" id=\"hello\"></a>Hello World</h2>";
        let out = apply_heading_links(html).unwrap();
        assert!(out.contains("heading-link"), "got: {out}");
        assert!(out.contains("Hello World"), "got: {out}");
    }

    #[test]
    fn no_anchor_no_change() {
        let html = "<h2>Hello</h2>";
        let out = apply_heading_links(html).unwrap();
        assert!(!out.contains("heading-link"), "got: {out}");
    }
}
