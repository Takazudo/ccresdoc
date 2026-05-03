use once_cell::sync::Lazy;
use syntect::easy::HighlightLines;
use syntect::highlighting::ThemeSet;
use syntect::html::{append_highlighted_html_for_styled_line, IncludeBackground};
use syntect::parsing::{SyntaxReference, SyntaxSet};
use syntect::util::LinesWithEndings;

use crate::RenderError;

static SYNTAX_SET: Lazy<SyntaxSet> = Lazy::new(SyntaxSet::load_defaults_newlines);
static THEME_SET: Lazy<ThemeSet> = Lazy::new(ThemeSet::load_defaults);

/// Apply syntect syntax highlighting to all `<pre><code class="language-X">` blocks.
///
/// Uses the named theme (e.g. "dracula" or "InspiredGitHub" for github-light).
/// Outputs inline `<span style="...">` tokens — the class contract fixture
/// documents the `language-*` class used by S4 to style the `<pre>` container.
pub fn apply_highlighting(html: &str, theme_name: &str) -> Result<String, RenderError> {
    // Resolve theme: support common aliases used in the Astro shiki config.
    // dracula → "Solarized (dark)" fallback, github-light → "InspiredGitHub".
    let resolved = resolve_theme(theme_name);

    let theme = THEME_SET.themes.get(resolved).or_else(|| {
        // Fallback to first available theme
        THEME_SET.themes.values().next()
    });

    let theme = match theme {
        Some(t) => t,
        None => return Ok(html.to_owned()),
    };

    // Walk the HTML finding <pre><code class="language-X">...</code></pre> blocks
    // and replace them with highlighted versions.
    replace_code_blocks(html, theme)
}

fn resolve_theme(name: &str) -> &str {
    match name {
        // dracula is not in syntect defaults; use dark theme
        "dracula" => "base16-ocean.dark",
        // github-light maps to InspiredGitHub which is in syntect defaults
        "github-light" => "InspiredGitHub",
        other => other,
    }
}

fn replace_code_blocks(
    html: &str,
    theme: &syntect::highlighting::Theme,
) -> Result<String, RenderError> {
    let mut result = String::with_capacity(html.len());
    let mut remaining = html;

    loop {
        // Find next <pre><code class="language-
        let Some(pre_start) = remaining.find("<pre>") else {
            result.push_str(remaining);
            break;
        };

        result.push_str(&remaining[..pre_start]);
        let after_pre = &remaining[pre_start + 5..]; // skip "<pre>"

        // Find <code ...class="language-X"...>
        let Some(code_start) = after_pre.find("<code") else {
            result.push_str("<pre>");
            result.push_str(after_pre);
            break;
        };

        let code_tag_area = &after_pre[code_start..];
        let Some(tag_end) = code_tag_area.find('>') else {
            result.push_str("<pre>");
            result.push_str(after_pre);
            break;
        };

        let code_open_tag = &code_tag_area[..tag_end + 1];
        let lang = extract_language(code_open_tag);

        // Find body between > and </code>
        let body_start = code_start + tag_end + 1;
        let Some(close_pos) = after_pre[body_start..].find("</code>") else {
            result.push_str("<pre>");
            result.push_str(after_pre);
            break;
        };

        let code_body = &after_pre[body_start..body_start + close_pos];
        let after_close = &after_pre[body_start + close_pos + 7..]; // skip </code>

        // Find </pre>
        let Some(pre_close_pos) = after_close.find("</pre>") else {
            result.push_str("<pre>");
            result.push_str(after_pre);
            break;
        };

        let code_decoded = decode_html_entities(code_body);

        if let Some(lang_name) = lang {
            let highlighted = highlight_code(&code_decoded, &lang_name, theme);
            // Emit <pre><code class="language-X">highlighted</code></pre>
            // Preserve the original class attribute for S4 CSS targeting
            result.push_str("<pre>");
            result.push_str(code_open_tag);
            result.push_str(&highlighted);
            result.push_str("</code>");
            result.push_str(&after_close[..pre_close_pos]);
            result.push_str("</pre>");
        } else {
            // No language — pass through unchanged
            result.push_str("<pre>");
            result.push_str(code_open_tag);
            result.push_str(code_body);
            result.push_str("</code>");
            result.push_str(&after_close[..pre_close_pos]);
            result.push_str("</pre>");
        }

        remaining = &after_close[pre_close_pos + 6..]; // skip </pre>
    }

    Ok(result)
}

fn extract_language(tag: &str) -> Option<String> {
    // Look for class="language-X" or class="... language-X ..."
    let class_attr = extract_attr(tag, "class")?;
    for part in class_attr.split_whitespace() {
        if let Some(lang) = part.strip_prefix("language-") {
            return Some(lang.to_owned());
        }
    }
    None
}

fn extract_attr<'a>(tag: &'a str, attr: &str) -> Option<&'a str> {
    let needle = format!("{attr}=\"");
    let start = tag.find(&needle)? + needle.len();
    let rest = &tag[start..];
    let end = rest.find('"')?;
    Some(&rest[..end])
}

fn decode_html_entities(s: &str) -> String {
    s.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&#x27;", "'")
}

fn highlight_code(code: &str, lang: &str, theme: &syntect::highlighting::Theme) -> String {
    let syntax = find_syntax(lang);

    let Some(syntax) = syntax else {
        // Unknown language: return as-is (HTML-escaped)
        return escape_html(code);
    };

    let mut highlighter = HighlightLines::new(syntax, theme);
    let mut output = String::new();

    for line in LinesWithEndings::from(code) {
        let ranges = match highlighter.highlight_line(line, &SYNTAX_SET) {
            Ok(r) => r,
            Err(_) => {
                output.push_str(&escape_html(line));
                continue;
            }
        };
        append_highlighted_html_for_styled_line(&ranges, IncludeBackground::No, &mut output)
            .unwrap_or_else(|_| output.push_str(&escape_html(line)));
    }

    output
}

fn find_syntax(lang: &str) -> Option<&'static SyntaxReference> {
    let ss = &*SYNTAX_SET;
    // Try exact name first, then extension
    ss.find_syntax_by_token(lang)
        .or_else(|| ss.find_syntax_by_extension(lang))
}

fn escape_html(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn highlights_rust_code() {
        let html = "<pre><code class=\"language-rust\">fn main() {}</code></pre>";
        let out = apply_highlighting(html, "dracula").unwrap();
        // Should contain span elements from syntect
        assert!(out.contains("<span"), "got: {out}");
        assert!(out.contains("language-rust"), "got: {out}");
    }

    #[test]
    fn passes_through_unknown_language() {
        let html = "<pre><code class=\"language-xyz123\">some code</code></pre>";
        let out = apply_highlighting(html, "dracula").unwrap();
        assert!(out.contains("some code"), "got: {out}");
    }

    #[test]
    fn no_code_block_unchanged() {
        let html = "<p>hello</p>";
        let out = apply_highlighting(html, "dracula").unwrap();
        assert_eq!(out, html);
    }
}
