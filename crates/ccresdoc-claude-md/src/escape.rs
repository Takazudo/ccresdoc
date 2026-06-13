//! Faithful Rust port of zudo-doc's `escape-for-mdx.ts`.
//!
//! Escapes angle brackets and curly braces in prose so the content is valid
//! MDX, while preserving fenced code blocks (3+ backticks) and inline code
//! spans (1-3 backticks). The logic mirrors the upstream TypeScript
//! implementation exactly so generated MDX renders identically under zudo-doc.

use std::collections::HashSet;
use std::sync::OnceLock;

/// HTML tag names that MDX accepts as-is (lowercased). Any `<Name>` whose tag
/// is NOT in this set is treated as a JSX component reference and escaped.
fn html_tags() -> &'static HashSet<&'static str> {
    static TAGS: OnceLock<HashSet<&'static str>> = OnceLock::new();
    TAGS.get_or_init(|| {
        [
            "div", "span", "p", "a", "img", "br", "hr", "ul", "ol", "li", "h1", "h2", "h3", "h4",
            "h5", "h6", "code", "pre", "blockquote", "table", "tr", "td", "th", "thead", "tbody",
            "tfoot", "colgroup", "col", "strong", "em", "b", "i", "u", "s", "del", "ins", "sub",
            "sup", "details", "summary", "figure", "figcaption", "mark", "small", "cite", "q",
            "abbr", "dfn", "time", "var", "samp", "kbd", "section", "article", "aside", "header",
            "footer", "nav", "main", "form", "input", "button", "select", "option", "textarea",
            "label", "fieldset", "legend", "dl", "dt", "dd", "caption",
        ]
        .into_iter()
        .collect()
    })
}

/// Escape angle brackets and curly braces in `content` for MDX compatibility.
///
/// Fenced code blocks (` ```lang ... ``` `, supporting 3+ backtick fences) and
/// inline code spans are preserved verbatim.
pub fn escape_for_mdx(content: &str) -> String {
    // Phase 1: extract fenced code blocks, replacing each with a placeholder.
    let (with_placeholders, code_blocks) = extract_code_blocks(content);

    // Phase 2: split on the placeholders and escape only the non-code segments.
    let mut out = String::with_capacity(with_placeholders.len());
    for segment in split_placeholders(&with_placeholders, CODE_PLACEHOLDER) {
        match segment {
            // A real placeholder restores its code block. A forged sentinel
            // present in the SOURCE (e.g. literal `\0CODEBLOCK_99\0`) whose
            // index is out of range is emitted as escaped text rather than
            // panicking — generate::run runs on the watcher worker thread, so a
            // panic here would silently kill the watcher.
            Segment::Placeholder { idx, raw } => match code_blocks.get(idx) {
                Some(block) => out.push_str(block),
                None => out.push_str(&escape_text_segment(raw)),
            },
            Segment::Text(text) => {
                out.push_str(&escape_text_segment(text));
            }
        }
    }
    out
}

const CODE_PLACEHOLDER: &str = "\u{0}CODEBLOCK_";
const INLINE_PLACEHOLDER: &str = "\u{0}INLINE_";

/// Extract fenced code blocks (3+ backtick fences with a matching close fence)
/// and replace each with `\0CODEBLOCK_<idx>\0`. Mirrors the JS regex
/// `/(`{3,})[^\n]*\n[\s\S]*?\1/g`.
fn extract_code_blocks(content: &str) -> (String, Vec<String>) {
    let bytes = content.as_bytes();
    let mut out = String::with_capacity(content.len());
    let mut blocks: Vec<String> = Vec::new();
    let mut i = 0;
    let len = bytes.len();

    while i < len {
        // A fence may only open at the start of a line (start of string, or
        // immediately after a newline). The JS regex is not anchored to line
        // start, but `[^\n]*\n` after the open fence plus the greedy/lazy body
        // means in practice fences are matched line-wise; we anchor to line
        // start to avoid matching backtick runs mid-prose, matching how
        // CommonMark and the upstream output behave for real content.
        let at_line_start = i == 0 || bytes[i - 1] == b'\n';
        if at_line_start && bytes[i] == b'`' {
            // Count the run of backticks (fence length >= 3).
            let fence_start = i;
            let mut fence_len = 0;
            while i < len && bytes[i] == b'`' {
                fence_len += 1;
                i += 1;
            }
            if fence_len >= 3 {
                // Consume the rest of the opening line ([^\n]*\n).
                while i < len && bytes[i] != b'\n' {
                    i += 1;
                }
                if i < len {
                    // include the newline
                    i += 1;
                }
                // Find the closing fence: a run of exactly `fence_len`+ backticks.
                // JS backreference \1 matches the SAME number of backticks; a
                // longer run still starts with that many so `find` semantics
                // match the lazy body up to the first occurrence.
                let body_start = i;
                let close = find_closing_fence(content, body_start, fence_len);
                match close {
                    Some(close_end) => {
                        let block = &content[fence_start..close_end];
                        let idx = blocks.len();
                        blocks.push(block.to_string());
                        out.push_str(CODE_PLACEHOLDER);
                        out.push_str(&idx.to_string());
                        out.push('\u{0}');
                        i = close_end;
                        continue;
                    }
                    None => {
                        // No closing fence — emit the consumed text verbatim and
                        // continue scanning after it (the JS regex would simply
                        // not match here).
                        out.push_str(&content[fence_start..i]);
                        continue;
                    }
                }
            } else {
                // Not a fence (1-2 backticks at line start) — emit and continue.
                out.push_str(&content[fence_start..i]);
                continue;
            }
        }
        // Default: copy this char.
        let ch_start = i;
        // advance one UTF-8 char
        i += utf8_char_len(bytes[i]);
        out.push_str(&content[ch_start..i]);
    }

    (out, blocks)
}

/// Find the byte offset just past a closing fence of at least `fence_len`
/// backticks, starting the search at `from`. The closing fence must itself sit
/// at the start of a line OR be the standard `\n```` close; the JS regex `\1`
/// simply finds the next run of that many backticks, so we replicate that:
/// scan for the next backtick run whose length >= fence_len and return the
/// offset past exactly `fence_len` of them (matching `\1`'s capture length).
fn find_closing_fence(content: &str, from: usize, fence_len: usize) -> Option<usize> {
    let bytes = content.as_bytes();
    let len = bytes.len();
    let mut i = from;
    while i < len {
        if bytes[i] == b'`' {
            let run_start = i;
            let mut run = 0;
            while i < len && bytes[i] == b'`' {
                run += 1;
                i += 1;
            }
            if run >= fence_len {
                // `\1` captures exactly fence_len backticks; the match ends
                // there.
                return Some(run_start + fence_len);
            }
        } else {
            i += 1;
        }
    }
    None
}

enum Segment<'a> {
    Text(&'a str),
    /// `idx` is the parsed placeholder index; `raw` is the full matched
    /// placeholder slice, used as a fallback when `idx` is out of range.
    Placeholder { idx: usize, raw: &'a str },
}

/// Split `s` into alternating text / placeholder segments. A placeholder has
/// the form `<prefix><digits>\0`.
fn split_placeholders<'a>(s: &'a str, prefix: &str) -> Vec<Segment<'a>> {
    let mut segments = Vec::new();
    let mut rest = s;
    while let Some(pos) = rest.find(prefix) {
        let after_prefix = &rest[pos + prefix.len()..];
        // Read digits then a NUL.
        let digit_end = after_prefix
            .find(|c: char| !c.is_ascii_digit())
            .unwrap_or(after_prefix.len());
        if digit_end > 0 && after_prefix.as_bytes().get(digit_end) == Some(&0) {
            // Valid placeholder.
            if pos > 0 {
                segments.push(Segment::Text(&rest[..pos]));
            }
            let idx: usize = after_prefix[..digit_end].parse().unwrap_or(usize::MAX);
            // advance past digits + NUL
            let consumed = pos + prefix.len() + digit_end + 1;
            let raw = &rest[pos..consumed];
            segments.push(Segment::Placeholder { idx, raw });
            rest = &rest[consumed..];
        } else {
            // Not a real placeholder; treat the prefix occurrence as text by
            // emitting up to and including it, then continuing.
            let consumed = pos + prefix.len();
            segments.push(Segment::Text(&rest[..consumed]));
            rest = &rest[consumed..];
        }
    }
    if !rest.is_empty() {
        segments.push(Segment::Text(rest));
    }
    segments
}

/// Escape a non-code-block text segment: preserve inline code, escape unknown
/// JSX-like tags and curly braces.
fn escape_text_segment(part: &str) -> String {
    // Extract inline code spans first.
    let (with_inline, inline_codes) = extract_inline_code(part);

    // Escape tags + curly braces on the remaining text.
    let mut escaped = escape_tags_and_braces(&with_inline);

    // Restore inline code placeholders.
    for (idx, code) in inline_codes.iter().enumerate() {
        let needle = format!("{INLINE_PLACEHOLDER}{idx}\u{0}");
        escaped = escaped.replace(&needle, code);
    }
    escaped
}

/// Extract inline code spans, mirroring the JS regex
/// `/(`{1,3})(?!`)([\s\S]*?[^`])\1(?!`)/g`.
///
/// The regex engine tries every starting position left-to-right and advances by
/// ONE on failure, so a run of 4+ backticks does not block an inline span that
/// opens on a *later* backtick of the run (e.g. ` ````<=` ` matches ` `<=` `
/// using the 4th backtick as the opener). We replicate that by scanning
/// position-by-position rather than consuming whole backtick runs.
fn extract_inline_code(part: &str) -> (String, Vec<String>) {
    let bytes = part.as_bytes();
    let len = bytes.len();
    let mut out = String::with_capacity(len);
    let mut codes: Vec<String> = Vec::new();
    let mut i = 0;

    while i < len {
        if bytes[i] == b'`' {
            // Greedy `{1,3}` with the `(?!`)` lookahead: pick the largest opener
            // length L in 1..=3 such that the char at i+L is NOT a backtick.
            let run = backtick_run_len(bytes, i);
            if let Some(open) = greedy_opener_len(bytes, i, run) {
                if let Some((content_end, close_end)) = find_inline_close(part, i + open, open)
                {
                    // The JS body group requires at least one char
                    // (`[\s\S]*?[^`]`), so an empty body does NOT match.
                    if content_end > i + open {
                        let full = &part[i..close_end];
                        let idx = codes.len();
                        codes.push(full.to_string());
                        out.push_str(INLINE_PLACEHOLDER);
                        out.push_str(&idx.to_string());
                        out.push('\u{0}');
                        i = close_end;
                        continue;
                    }
                }
            }
            // No match anchored at i — emit this single backtick and advance by
            // one (regex engine bumps the start position by one on failure).
            out.push('`');
            i += 1;
            continue;
        }
        let ch_start = i;
        i += utf8_char_len(bytes[i]);
        out.push_str(&part[ch_start..i]);
    }

    (out, codes)
}

/// Number of consecutive backticks starting at `i`.
fn backtick_run_len(bytes: &[u8], i: usize) -> usize {
    let mut n = 0;
    while i + n < bytes.len() && bytes[i + n] == b'`' {
        n += 1;
    }
    n
}

/// The greedy opener length the regex `(`{1,3})(?!`)` would capture at `i`,
/// given the run length: the largest L in 1..=min(run,3) with `bytes[i+L]` not a
/// backtick. Returns `None` if no such L exists (i.e. run length forces a
/// trailing backtick for every L ≤ 3 — only when run > 3 and the opener can't
/// reach the end of the run; the regex then fails at `i`).
fn greedy_opener_len(bytes: &[u8], i: usize, run: usize) -> Option<usize> {
    let max = run.min(3);
    // Greedy: try 3, then 2, then 1.
    for l in (1..=max).rev() {
        let next = i + l;
        let next_is_backtick = bytes.get(next) == Some(&b'`');
        if !next_is_backtick {
            return Some(l);
        }
    }
    None
}

/// Find an inline-code closing fence of exactly `open` backticks starting at
/// `from`, where the char before the run is not a backtick and the char after
/// the run is not a backtick. Returns `(content_end, close_end)` byte offsets:
/// `content_end` is where the body ends (start of closing run), `close_end` is
/// just past the closing run.
fn find_inline_close(part: &str, from: usize, open: usize) -> Option<(usize, usize)> {
    let bytes = part.as_bytes();
    let len = bytes.len();
    let mut i = from;
    while i < len {
        if bytes[i] == b'`' {
            let run_start = i;
            let mut run = 0;
            while i < len && bytes[i] == b'`' {
                run += 1;
                i += 1;
            }
            // Closing run must be exactly `open` backticks, with the preceding
            // char ([^`]) — ensured because run_start>from and the char before
            // is part of the body — and the following char not a backtick
            // (already true since the run ended).
            if run == open && run_start > from {
                return Some((run_start, run_start + open));
            }
            // Otherwise keep scanning (a longer/shorter run is part of body).
        } else {
            i += utf8_char_len(bytes[i]);
        }
    }
    None
}

/// Escape JSX-like opening/closing/self-closing tags whose name is not a known
/// HTML tag, plus `<` before `-`/`=`/digits, plus curly braces. Mirrors the
/// chain of `.replace()` calls in the JS source.
fn escape_tags_and_braces(input: &str) -> String {
    let mut s = escape_open_tags(input);
    s = escape_close_tags(&s);
    s = escape_self_closing_tags(&s);
    s = escape_lt_runs(&s);
    s = escape_lt_digit(&s);
    // Curly braces last.
    s = s.replace('{', "&#123;").replace('}', "&#125;");
    s
}

fn is_tag_name_start(c: u8) -> bool {
    c.is_ascii_alphabetic()
}

fn is_tag_name_char(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_' || c == b'-'
}

/// `/<([A-Za-z][A-Za-z0-9_-]*)(\s[^>]*)?>/g` — opening tags (also matches the
/// spaced self-closing form `<Foo />`).
fn escape_open_tags(input: &str) -> String {
    let bytes = input.as_bytes();
    let len = bytes.len();
    let mut out = String::with_capacity(len);
    let mut i = 0;
    while i < len {
        if bytes[i] == b'<' && i + 1 < len && is_tag_name_start(bytes[i + 1]) {
            let mut j = i + 1;
            while j < len && is_tag_name_char(bytes[j]) {
                j += 1;
            }
            let name = &input[i + 1..j];
            // Optional (\s[^>]*)? then `>`.
            let mut k = j;
            if k < len && bytes[k].is_ascii_whitespace() {
                // consume [^>]* up to the next '>'
                while k < len && bytes[k] != b'>' {
                    k += 1;
                }
            }
            if k < len && bytes[k] == b'>' {
                let full = &input[i..=k];
                if html_tags().contains(name.to_ascii_lowercase().as_str()) {
                    out.push_str(full);
                } else {
                    out.push_str(&full.replace('<', "&lt;").replace('>', "&gt;"));
                }
                i = k + 1;
                continue;
            }
        }
        let ch_start = i;
        i += utf8_char_len(bytes[i]);
        out.push_str(&input[ch_start..i]);
    }
    out
}

/// `/<\/([A-Za-z][A-Za-z0-9_-]*)>/g` — closing tags.
fn escape_close_tags(input: &str) -> String {
    let bytes = input.as_bytes();
    let len = bytes.len();
    let mut out = String::with_capacity(len);
    let mut i = 0;
    while i < len {
        if bytes[i] == b'<'
            && i + 2 < len
            && bytes[i + 1] == b'/'
            && is_tag_name_start(bytes[i + 2])
        {
            let mut j = i + 2;
            while j < len && is_tag_name_char(bytes[j]) {
                j += 1;
            }
            if j < len && bytes[j] == b'>' {
                let name = &input[i + 2..j];
                if html_tags().contains(name.to_ascii_lowercase().as_str()) {
                    out.push_str(&input[i..=j]);
                } else {
                    out.push_str(&format!("&lt;/{name}&gt;"));
                }
                i = j + 1;
                continue;
            }
        }
        let ch_start = i;
        i += utf8_char_len(bytes[i]);
        out.push_str(&input[ch_start..i]);
    }
    out
}

/// `/<([A-Za-z][A-Za-z0-9_-]*)(\s[^>]*)?\s*\/>/g` — compact self-closing form
/// `<Foo/>`. The spaced form is already handled by `escape_open_tags`.
fn escape_self_closing_tags(input: &str) -> String {
    let bytes = input.as_bytes();
    let len = bytes.len();
    let mut out = String::with_capacity(len);
    let mut i = 0;
    while i < len {
        if bytes[i] == b'<' && i + 1 < len && is_tag_name_start(bytes[i + 1]) {
            let mut j = i + 1;
            while j < len && is_tag_name_char(bytes[j]) {
                j += 1;
            }
            let name = &input[i + 1..j];
            let mut k = j;
            if k < len && bytes[k].is_ascii_whitespace() {
                while k < len && bytes[k] != b'>' && bytes[k] != b'/' {
                    k += 1;
                }
            }
            // optional trailing whitespace then "/>"
            while k < len && bytes[k].is_ascii_whitespace() {
                k += 1;
            }
            if k + 1 < len && bytes[k] == b'/' && bytes[k + 1] == b'>' {
                let full = &input[i..=k + 1];
                if html_tags().contains(name.to_ascii_lowercase().as_str()) {
                    out.push_str(full);
                } else {
                    out.push_str(&full.replace('<', "&lt;").replace('>', "&gt;"));
                }
                i = k + 2;
                continue;
            }
        }
        let ch_start = i;
        i += utf8_char_len(bytes[i]);
        out.push_str(&input[ch_start..i]);
    }
    out
}

/// `/<(-+|=+)/g` → `&lt;$1`.
fn escape_lt_runs(input: &str) -> String {
    let bytes = input.as_bytes();
    let len = bytes.len();
    let mut out = String::with_capacity(len);
    let mut i = 0;
    while i < len {
        if bytes[i] == b'<' && i + 1 < len && (bytes[i + 1] == b'-' || bytes[i + 1] == b'=') {
            out.push_str("&lt;");
            i += 1;
            continue;
        }
        let ch_start = i;
        i += utf8_char_len(bytes[i]);
        out.push_str(&input[ch_start..i]);
    }
    out
}

/// `/<(\d)/g` → `&lt;$1`.
fn escape_lt_digit(input: &str) -> String {
    let bytes = input.as_bytes();
    let len = bytes.len();
    let mut out = String::with_capacity(len);
    let mut i = 0;
    while i < len {
        if bytes[i] == b'<' && i + 1 < len && bytes[i + 1].is_ascii_digit() {
            out.push_str("&lt;");
            i += 1;
            continue;
        }
        let ch_start = i;
        i += utf8_char_len(bytes[i]);
        out.push_str(&input[ch_start..i]);
    }
    out
}

/// Length in bytes of the UTF-8 char starting at a given leading byte.
fn utf8_char_len(b: u8) -> usize {
    if b < 0x80 {
        1
    } else if b >> 5 == 0b110 {
        2
    } else if b >> 4 == 0b1110 {
        3
    } else if b >> 3 == 0b11110 {
        4
    } else {
        1
    }
}

#[cfg(test)]
mod tests {
    use super::escape_for_mdx;

    #[test]
    fn escapes_jsx_like_tag() {
        assert_eq!(escape_for_mdx("a <Foo> b"), "a &lt;Foo&gt; b");
    }

    #[test]
    fn preserves_known_html_tag() {
        assert_eq!(escape_for_mdx("a <div> b"), "a <div> b");
    }

    #[test]
    fn escapes_curly_braces() {
        assert_eq!(escape_for_mdx("use {x}"), "use &#123;x&#125;");
    }

    #[test]
    fn preserves_inline_code() {
        assert_eq!(escape_for_mdx("call `foo<Bar>` now"), "call `foo<Bar>` now");
    }

    #[test]
    fn preserves_inline_code_with_braces() {
        assert_eq!(escape_for_mdx("`{a: 1}`"), "`{a: 1}`");
    }

    #[test]
    fn preserves_fenced_code_block() {
        let input = "before\n```ts\nconst x = <Foo>;\nif (a) {b}\n```\nafter <Baz>";
        let out = escape_for_mdx(input);
        assert!(out.contains("const x = <Foo>;"));
        assert!(out.contains("if (a) {b}"));
        assert!(out.contains("after &lt;Baz&gt;"));
    }

    #[test]
    fn escapes_closing_jsx_tag() {
        assert_eq!(escape_for_mdx("</Foo>"), "&lt;/Foo&gt;");
    }

    #[test]
    fn preserves_closing_html_tag() {
        assert_eq!(escape_for_mdx("</div>"), "</div>");
    }

    #[test]
    fn escapes_compact_self_closing() {
        assert_eq!(escape_for_mdx("<Foo/>"), "&lt;Foo/&gt;");
    }

    #[test]
    fn escapes_spaced_self_closing() {
        assert_eq!(escape_for_mdx("<Foo />"), "&lt;Foo /&gt;");
    }

    #[test]
    fn escapes_lt_arrow_and_digit() {
        assert_eq!(escape_for_mdx("a <- b"), "a &lt;- b");
        assert_eq!(escape_for_mdx("x <3 y"), "x &lt;3 y");
    }

    #[test]
    fn escapes_attrs_on_jsx_tag() {
        assert_eq!(
            escape_for_mdx(r#"<Foo bar="baz">"#),
            r#"&lt;Foo bar="baz"&gt;"#
        );
    }

    #[test]
    fn multibyte_content_is_preserved() {
        // Japanese text around an escapable token must not corrupt byte indices.
        let out = escape_for_mdx("日本語 <Foo> テスト {x}");
        assert_eq!(out, "日本語 &lt;Foo&gt; テスト &#123;x&#125;");
    }

    #[test]
    fn tilde_fence_is_not_special_only_backticks() {
        // Only backtick fences are recognized; angle brackets outside are escaped.
        let out = escape_for_mdx("text <Comp> more");
        assert_eq!(out, "text &lt;Comp&gt; more");
    }

    #[test]
    fn inline_opener_inside_longer_backtick_run() {
        // Regression (C1): a 4-backtick run followed by `<=` then one backtick.
        // The regex opens the inline span on the LAST backtick of the run, so
        // `<=` stays unescaped (matches escape-for-mdx.ts behaviour).
        let out = escape_for_mdx("````<=`");
        assert_eq!(out, "````<=`");
    }

    #[test]
    fn forged_codeblock_sentinel_does_not_panic() {
        // Regression (C2): a literal CODEBLOCK sentinel with an out-of-range
        // index must not panic; it is emitted as escaped text.
        let forged = "\u{0}CODEBLOCK_99\u{0} and <Foo>";
        let out = escape_for_mdx(forged);
        // Must not panic and must still escape the trailing JSX tag.
        assert!(out.contains("&lt;Foo&gt;"));
    }

    #[test]
    fn quadruple_backtick_run_alone_is_unchanged() {
        // A lone 4-backtick run with no valid close stays verbatim.
        assert_eq!(escape_for_mdx("````"), "````");
    }
}
