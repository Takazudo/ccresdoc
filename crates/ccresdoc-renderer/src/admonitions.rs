/// Pre-process admonition fences before comrak runs.
///
/// Converts:
///   :::note
///   content
///   :::
/// into raw HTML:
///   <aside class="admonition admonition-note">
///   content
///   </aside>
///
/// This mirrors the remark-directive + remark-admonitions plugin behaviour.
pub fn preprocess(input: &str, kinds: &[String]) -> String {
    let lines: Vec<&str> = input.lines().collect();
    let mut out = String::with_capacity(input.len() + 64);
    let mut i = 0;

    while i < lines.len() {
        let line = lines[i];
        if let Some(kind) = parse_open_fence(line, kinds) {
            // Opening fence found — collect body lines until closing :::
            // kind is already validated against the known-kinds allowlist, but we
            // HTML-attribute-escape it defensively in case the list is user-supplied.
            let kind_escaped = escape_class_value(kind);
            out.push_str(&format!(
                "<aside class=\"admonition admonition-{kind_escaped}\">\n"
            ));
            i += 1;
            while i < lines.len() {
                let inner = lines[i];
                if inner.trim() == ":::" {
                    out.push_str("</aside>\n");
                    i += 1;
                    break;
                }
                out.push_str(inner);
                out.push('\n');
                i += 1;
            }
        } else {
            out.push_str(line);
            out.push('\n');
            i += 1;
        }
    }

    // Preserve trailing newline behaviour of the original
    if !input.ends_with('\n') && out.ends_with('\n') {
        out.pop();
    }

    out
}

/// Escape characters that are unsafe in HTML class attribute values.
/// kind values come from an allowlist but we escape defensively.
fn escape_class_value(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('\'', "&#x27;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn parse_open_fence<'a>(line: &str, kinds: &'a [String]) -> Option<&'a str> {
    let trimmed = line.trim();
    if !trimmed.starts_with(":::") {
        return None;
    }
    let rest = trimmed[3..].trim();
    // Closing fence is ":::" with no kind
    if rest.is_empty() {
        return None;
    }
    // Take the first word as the kind
    let word = rest.split_whitespace().next()?;
    kinds.iter().find(|k| k.as_str() == word).map(|k| k.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic_admonition() {
        let input = ":::note\nhello\n:::";
        let kinds: Vec<String> = vec!["note".into()];
        let out = preprocess(input, &kinds);
        assert!(out.contains("<aside class=\"admonition admonition-note\">"));
        assert!(out.contains("</aside>"));
        assert!(out.contains("hello"));
    }

    #[test]
    fn unknown_kind_passthrough() {
        let input = ":::unknown\nhello\n:::";
        let kinds: Vec<String> = vec!["note".into()];
        let out = preprocess(input, &kinds);
        assert!(out.contains(":::unknown"));
    }

    #[test]
    fn multiple_admonitions() {
        let input = ":::note\nfoo\n:::\n\n:::warning\nbar\n:::";
        let kinds: Vec<String> = vec!["note".into(), "warning".into()];
        let out = preprocess(input, &kinds);
        assert!(out.contains("admonition-note"));
        assert!(out.contains("admonition-warning"));
    }
}
