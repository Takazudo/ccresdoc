//! Pure transformation of generated documentation pages into zudo-doc's
//! search-index contract.
//!
//! The coordinator owns collecting pages and publishing the resulting JSON.
//! This module deliberately does neither: callers hand [`build_search_index`]
//! already-collected pages and may pass the returned entries to
//! [`serialize_search_index`].

use std::collections::HashSet;

use serde::Serialize;

/// The maximum number of Unicode scalar values retained in an entry body.
///
/// The upstream search widget calls this `MAX_BODY_LENGTH` and treats the
/// value as a character count.  Truncating with [`str::chars`] rather than a
/// byte slice also means Japanese and other multi-byte text can never be
/// split in the middle of a UTF-8 sequence.
pub const MAX_BODY_LENGTH: usize = 300;

/// The generated application is configured with `trailingSlash: true` and
/// mounts docs below `/docs/`.  Every generated page therefore gets exactly
/// one trailing slash in its search result URL, including the docs root.
pub const DOCS_URL_PREFIX: &str = "/docs";

/// The two generated resource families have independent slug spaces.  The
/// namespace is included in every id (`claude:<route-slug>` or
/// `codex:<route-slug>`) so equal slugs from Claude and Codex can never
/// collide.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum SearchIndexNamespace {
    Claude,
    Codex,
}

impl SearchIndexNamespace {
    /// Stable textual namespace used in ids.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::Codex => "codex",
        }
    }
}

/// A page collected by the coordinator before index transformation.
///
/// `slug` is the content-file slug relative to `/docs/`, without a leading or
/// trailing slash (for example, `claude-md/global`).  `index` is canonicalised
/// to the route slug used by zudo-doc: bare `index` becomes the docs root and
/// a nested `x/index` becomes `x`.  `body` is the Markdown body after
/// frontmatter has been removed.  The four flags mirror zudo-doc's
/// `isExcluded` predicate exactly: any one of them prevents an entry from
/// being emitted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchIndexPage {
    pub namespace: SearchIndexNamespace,
    pub slug: String,
    pub title: String,
    pub body: String,
    pub description: String,
    pub search_exclude: bool,
    pub draft: bool,
    pub unlisted: bool,
    pub category_no_page: bool,
}

/// Explicit name for the coordinator-facing input type.  `SearchIndexPage`
/// remains the concise API spelling, while this alias makes it clear that the
/// builder consumes generated pages rather than filesystem paths.
pub type GeneratedPage = SearchIndexPage;

impl SearchIndexPage {
    /// Construct a visible page with the ordinary frontmatter defaults.
    pub fn new(
        namespace: SearchIndexNamespace,
        slug: impl Into<String>,
        title: impl Into<String>,
        body: impl Into<String>,
        description: impl Into<String>,
    ) -> Self {
        Self {
            namespace,
            slug: slug.into(),
            title: title.into(),
            body: body.into(),
            description: description.into(),
            search_exclude: false,
            draft: false,
            unlisted: false,
            category_no_page: false,
        }
    }

    /// Return whether zudo-doc's collector would omit this page.
    pub const fn is_excluded(&self) -> bool {
        self.search_exclude || self.draft || self.unlisted || self.category_no_page
    }
}

/// One search-index row.  Keep this struct to exactly the five fields in the
/// frozen zudo-doc contract; adding a Rust field would add a JSON key.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SearchIndexEntry {
    pub id: String,
    pub title: String,
    pub body: String,
    pub url: String,
    pub description: String,
}

/// Build deterministic search entries from already-collected pages.
///
/// Excluded pages are removed, entries are sorted by their stable namespace
/// id, and duplicate ids panic.  A duplicate is a generator invariant
/// violation: silently dropping one page would make search stale and hide a
/// source collision from the coordinator.
pub fn build_search_index<I>(pages: I) -> Vec<SearchIndexEntry>
where
    I: IntoIterator<Item = SearchIndexPage>,
{
    let mut entries = pages
        .into_iter()
        .filter(|page| !page.is_excluded())
        .map(entry_from_page)
        .collect::<Vec<_>>();

    entries.sort_by(|left, right| left.id.cmp(&right.id));

    let mut ids = HashSet::with_capacity(entries.len());
    for entry in &entries {
        assert!(
            ids.insert(entry.id.clone()),
            "duplicate search-index id generated: {}",
            entry.id
        );
    }

    entries
}

/// Serialize search entries as the exact JSON array consumed by zudo-doc.
///
/// In particular, an empty slice becomes `[]`, not `null` or a wrapper object.
pub fn serialize_search_index(entries: &[SearchIndexEntry]) -> serde_json::Result<String> {
    serde_json::to_string(entries)
}

/// Build and serialize the search index in one pure operation.
pub fn search_index_json<I>(pages: I) -> serde_json::Result<String>
where
    I: IntoIterator<Item = SearchIndexPage>,
{
    serialize_search_index(&build_search_index(pages))
}

/// Compatibility alias for callers that prefer a verb-oriented name.
pub fn build_search_index_json<I>(pages: I) -> serde_json::Result<String>
where
    I: IntoIterator<Item = SearchIndexPage>,
{
    search_index_json(pages)
}

fn entry_from_page(page: SearchIndexPage) -> SearchIndexEntry {
    let slug = to_route_slug(&normalise_slug(&page.slug));
    let title = if page.title.is_empty() {
        slug.clone()
    } else {
        page.title
    };
    let id = format!("{}:{}", page.namespace.as_str(), slug);

    SearchIndexEntry {
        id,
        title,
        body: truncate_chars(&strip_markdown(&page.body), MAX_BODY_LENGTH),
        url: route_url(&slug),
        description: page.description,
    }
}

fn normalise_slug(slug: &str) -> String {
    slug.trim_matches('/').to_owned()
}

/// Match zudo-doc's shared route-slug rule (`toRouteSlug`).  Content files
/// named `index.mdx` are route roots, not pages at a literal `/index/` URL.
fn to_route_slug(slug: &str) -> String {
    if slug == "index" {
        String::new()
    } else {
        slug.strip_suffix("/index").unwrap_or(slug).to_owned()
    }
}

fn route_url(slug: &str) -> String {
    if slug.is_empty() {
        format!("{DOCS_URL_PREFIX}/")
    } else {
        format!("{DOCS_URL_PREFIX}/{slug}/")
    }
}

fn truncate_chars(value: &str, max_chars: usize) -> String {
    value.chars().take(max_chars).collect()
}

/// Strip the Markdown constructs used by zudo-doc's search collector.
///
/// This intentionally follows the upstream order: code and MDX comments are
/// removed before tags and emphasis, links retain their visible text, and
/// whitespace is normalised only at the end.  It is conservative and
/// dependency-free because this crate's search path must remain a pure Rust
/// transformation.
pub fn strip_markdown(markdown: &str) -> String {
    let without_code = remove_delimited_pair(markdown, "```");
    let without_inline_code = remove_delimited_pair(&without_code, "`");
    let without_comments = remove_delimited_open_close(&without_inline_code, "{/*", "*/");
    let without_tags = remove_html_tags(&without_comments);
    let decoded = decode_character_references(&without_tags);
    let headings = remove_heading_markers(&decoded);
    let emphasis = remove_emphasis_markers(&headings);
    let images = remove_markdown_images(&emphasis);
    let links = replace_markdown_links(&images);
    let blockquotes = remove_blockquote_markers(&links);
    let horizontal_rules = remove_horizontal_rules(&blockquotes);
    let lists = remove_list_markers(&horizontal_rules);
    let imports_exports = remove_import_export_lines(&lists);
    collapse_whitespace(&imports_exports)
}

/// Remove text between every pair of a delimiter.  This is sufficient for
/// Markdown fences/backticks and mirrors the upstream non-nesting regex
/// pipeline.  An unmatched opening delimiter is retained, like that pipeline.
fn remove_delimited_pair(input: &str, delimiter: &str) -> String {
    let mut output = String::with_capacity(input.len());
    let mut cursor = 0;

    while let Some(relative_start) = input[cursor..].find(delimiter) {
        let start = cursor + relative_start;
        output.push_str(&input[cursor..start]);
        let content_start = start + delimiter.len();
        let Some(relative_end) = input[content_start..].find(delimiter) else {
            output.push_str(&input[start..]);
            return output;
        };
        cursor = content_start + relative_end + delimiter.len();
    }

    output.push_str(&input[cursor..]);
    output
}

fn remove_delimited_open_close(input: &str, open: &str, close: &str) -> String {
    let mut output = String::with_capacity(input.len());
    let mut cursor = 0;

    while let Some(relative_start) = input[cursor..].find(open) {
        let start = cursor + relative_start;
        output.push_str(&input[cursor..start]);
        let content_start = start + open.len();
        let Some(relative_end) = input[content_start..].find(close) else {
            output.push_str(&input[start..]);
            return output;
        };
        cursor = content_start + relative_end + close.len();
    }

    output.push_str(&input[cursor..]);
    output
}

fn remove_html_tags(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    let mut cursor = 0;

    while let Some(relative_start) = input[cursor..].find('<') {
        let start = cursor + relative_start;
        let Some(relative_end) = input[start..].find('>') else {
            output.push_str(&input[cursor..]);
            return output;
        };
        output.push_str(&input[cursor..start]);
        cursor = start + relative_end + 1;
    }

    output.push_str(&input[cursor..]);
    output
}

fn decode_character_references(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    let mut cursor = 0;

    while let Some(relative_start) = input[cursor..].find('&') {
        let start = cursor + relative_start;
        output.push_str(&input[cursor..start]);
        let Some(relative_end) = input[start..].find(';') else {
            output.push_str(&input[start..]);
            return output;
        };
        let end = start + relative_end + 1;
        let reference = &input[start..end];
        if let Some(decoded) = decode_reference(reference) {
            output.push_str(&decoded);
        } else {
            output.push_str(reference);
        }
        cursor = end;
    }

    output.push_str(&input[cursor..]);
    // Decode ampersands last, matching the upstream pipeline.  This is a
    // separate pass so `&amp;lt;` becomes `&lt;` rather than `<` in one run.
    output.replace("&amp;", "&")
}

fn decode_reference(reference: &str) -> Option<String> {
    let name = reference.strip_prefix('&')?.strip_suffix(';')?;
    let decoded = match name {
        "lt" => Some('<'),
        "gt" => Some('>'),
        "quot" => Some('"'),
        "apos" => Some('\''),
        "nbsp" => Some('\u{a0}'),
        _ => None,
    };
    if let Some(decoded) = decoded {
        return Some(decoded.to_string());
    }

    let (radix, digits) = if let Some(digits) = name.strip_prefix("#x") {
        (16, digits)
    } else if let Some(digits) = name.strip_prefix("#X") {
        (16, digits)
    } else if let Some(digits) = name.strip_prefix('#') {
        (10, digits)
    } else {
        // Ampersands are decoded last upstream, so an encoded ampersand is
        // intentionally handled here only after all other references have
        // been resolved by the caller's left-to-right pass.
        return None;
    };

    let code_point = u32::from_str_radix(digits, radix).ok()?;
    char::from_u32(code_point).map(|character| character.to_string())
}

fn remove_heading_markers(input: &str) -> String {
    input
        .lines()
        .map(|line| {
            let bytes = line.as_bytes();
            let mut marker_len = 0;
            while marker_len < bytes.len() && marker_len < 6 && bytes[marker_len] == b'#' {
                marker_len += 1;
            }
            if marker_len > 0 && marker_len < bytes.len() && bytes[marker_len].is_ascii_whitespace()
            {
                line[marker_len..].trim_start_matches(char::is_whitespace)
            } else {
                line
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn remove_emphasis_markers(input: &str) -> String {
    // Match the common Markdown emphasis forms while preserving literal
    // asterisks/underscores.  We repeatedly remove a run when a corresponding
    // run closes the same plain-text span.
    let mut output = input.to_owned();
    for marker in ['*', '_'] {
        for run_len in (1..=3).rev() {
            let marker_run = marker.to_string().repeat(run_len);
            let mut cursor = 0;
            let mut transformed = String::with_capacity(output.len());
            while let Some(relative_start) = output[cursor..].find(&marker_run) {
                let start = cursor + relative_start;
                let content_start = start + run_len;
                let Some(relative_end) = output[content_start..].find(&marker_run) else {
                    transformed.push_str(&output[cursor..]);
                    cursor = output.len();
                    break;
                };
                let end = content_start + relative_end;
                let content = &output[content_start..end];
                let is_underscore_word = marker == '_'
                    && (output[..start]
                        .chars()
                        .next_back()
                        .is_some_and(char::is_alphanumeric)
                        || output[end + run_len..]
                            .chars()
                            .next()
                            .is_some_and(char::is_alphanumeric));
                if content.is_empty() || content.contains(marker) || is_underscore_word {
                    transformed.push_str(&output[cursor..content_start]);
                    cursor = content_start;
                    continue;
                }
                transformed.push_str(&output[cursor..start]);
                transformed.push_str(content);
                cursor = end + run_len;
            }
            if cursor < output.len() {
                transformed.push_str(&output[cursor..]);
            }
            output = transformed;
        }
    }
    output
}

fn remove_markdown_images(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    let mut cursor = 0;
    while let Some(relative_start) = input[cursor..].find("![") {
        let start = cursor + relative_start;
        let Some(close_alt) = input[start + 2..].find(']') else {
            output.push_str(&input[cursor..]);
            return output;
        };
        let open_url = start + 2 + close_alt + 1;
        if input.as_bytes().get(open_url) != Some(&b'(') {
            output.push_str(&input[cursor..start + 2]);
            cursor = start + 2;
            continue;
        }
        let Some(close_url) = input[open_url + 1..].find(')') else {
            output.push_str(&input[cursor..]);
            return output;
        };
        output.push_str(&input[cursor..start]);
        cursor = open_url + 1 + close_url + 1;
    }
    output.push_str(&input[cursor..]);
    output
}

fn replace_markdown_links(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    let mut cursor = 0;
    while let Some(relative_start) = input[cursor..].find('[') {
        let start = cursor + relative_start;
        let Some(close_text) = input[start + 1..].find(']') else {
            output.push_str(&input[cursor..]);
            return output;
        };
        let open_url = start + 1 + close_text + 1;
        if input.as_bytes().get(open_url) != Some(&b'(') {
            output.push_str(&input[cursor..start + 1]);
            cursor = start + 1;
            continue;
        }
        let Some(close_url) = input[open_url + 1..].find(')') else {
            output.push_str(&input[cursor..]);
            return output;
        };
        output.push_str(&input[cursor..start]);
        output.push_str(&input[start + 1..start + 1 + close_text]);
        cursor = open_url + 1 + close_url + 1;
    }
    output.push_str(&input[cursor..]);
    output
}

fn remove_blockquote_markers(input: &str) -> String {
    input
        .lines()
        .map(|line| {
            let Some(rest) = line.strip_prefix('>') else {
                return line;
            };
            if rest.chars().next().is_some_and(char::is_whitespace) {
                rest.trim_start_matches(char::is_whitespace)
            } else {
                line
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn remove_horizontal_rules(input: &str) -> String {
    input
        .lines()
        .filter(|line| {
            let trimmed = line.trim();
            trimmed.len() < 3
                || !trimmed
                    .chars()
                    .all(|character| matches!(character, '-' | '*' | '_'))
                || trimmed.chars().next() != trimmed.chars().nth(1)
                || trimmed.chars().nth(1) != trimmed.chars().nth(2)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn remove_list_markers(input: &str) -> String {
    input
        .lines()
        .map(|line| {
            let without_indent = line.trim_start_matches([' ', '\t']);
            let marker_len = if without_indent.starts_with(['-', '*', '+'])
                && without_indent
                    .as_bytes()
                    .get(1)
                    .is_some_and(u8::is_ascii_whitespace)
            {
                2
            } else {
                let digits = without_indent
                    .as_bytes()
                    .iter()
                    .take_while(|byte| byte.is_ascii_digit())
                    .count();
                if digits > 0
                    && without_indent.as_bytes().get(digits) == Some(&b'.')
                    && without_indent
                        .as_bytes()
                        .get(digits + 1)
                        .is_some_and(u8::is_ascii_whitespace)
                {
                    digits + 2
                } else {
                    0
                }
            };
            if marker_len > 0 {
                &without_indent[marker_len..]
            } else {
                line
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn remove_import_export_lines(input: &str) -> String {
    input
        .lines()
        .filter(|line| {
            let trimmed = line.trim_start();
            !trimmed.starts_with("import ") && !trimmed.starts_with("export ")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn collapse_whitespace(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    let mut newline_count = 0;
    for character in input.chars() {
        if character == '\n' {
            newline_count += 1;
            if newline_count <= 2 {
                output.push(character);
            }
        } else {
            newline_count = 0;
            output.push(character);
        }
    }
    output.trim().to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page(
        namespace: SearchIndexNamespace,
        slug: &str,
        title: &str,
        body: &str,
    ) -> SearchIndexPage {
        SearchIndexPage::new(namespace, slug, title, body, "description")
    }

    #[test]
    fn emits_exact_five_key_schema() {
        let entries = build_search_index([page(
            SearchIndexNamespace::Claude,
            "claude-md/global",
            "Global",
            "body",
        )]);
        let json = serialize_search_index(&entries).unwrap();
        assert_eq!(
            json,
            r#"[{"id":"claude:claude-md/global","title":"Global","body":"body","url":"/docs/claude-md/global/","description":"description"}]"#
        );
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        let object = value[0].as_object().unwrap();
        assert_eq!(object.len(), 5);
        assert!(object.keys().all(|key| {
            matches!(
                key.as_str(),
                "id" | "title" | "body" | "url" | "description"
            )
        }));
    }

    #[test]
    fn strips_markdown_in_upstream_order() {
        let markdown = "# Heading\n\n**bold** and *em* with [link](https://example.test)\n\n![image](image.png)\n\n> quote\n\n- item\n\n```rust\nignored()\n```\n\n`code` <Widget> &amp; &#x65;";
        assert_eq!(
            strip_markdown(markdown),
            "Heading\n\nbold and em with link\n\nquote\n\nitem\n\n  & e"
        );
    }

    #[test]
    fn preserves_underscores_inside_words() {
        assert_eq!(strip_markdown("a_b_c and _emphasis_"), "a_b_c and emphasis");
    }

    #[test]
    fn truncates_by_unicode_scalar_value() {
        let body = format!("{}終わり", "あ".repeat(MAX_BODY_LENGTH));
        let entries = build_search_index([page(
            SearchIndexNamespace::Codex,
            "codex-skills/japanese",
            "日本語",
            &body,
        )]);
        assert_eq!(entries[0].body.chars().count(), MAX_BODY_LENGTH);
        assert_eq!(entries[0].body, "あ".repeat(MAX_BODY_LENGTH));
        assert!(entries[0].body.is_char_boundary(entries[0].body.len()));
    }

    #[test]
    fn applies_all_exclusion_flags() {
        let mut pages = Vec::new();
        for flag in 0..4 {
            let mut item = page(
                SearchIndexNamespace::Claude,
                &format!("claude-commands/hidden-{flag}"),
                "hidden",
                "body",
            );
            match flag {
                0 => item.search_exclude = true,
                1 => item.draft = true,
                2 => item.unlisted = true,
                3 => item.category_no_page = true,
                _ => unreachable!(),
            }
            pages.push(item);
        }
        assert!(build_search_index(pages).is_empty());
    }

    #[test]
    fn ids_are_namespace_disjoint_and_sorted() {
        let entries = build_search_index([
            page(SearchIndexNamespace::Codex, "same", "Codex", "codex"),
            page(SearchIndexNamespace::Claude, "z", "Claude z", "z"),
            page(SearchIndexNamespace::Claude, "same", "Claude", "claude"),
        ]);
        assert_eq!(
            entries
                .iter()
                .map(|entry| entry.id.as_str())
                .collect::<Vec<_>>(),
            ["claude:same", "claude:z", "codex:same"]
        );
    }

    #[test]
    #[should_panic(expected = "duplicate search-index id generated")]
    fn duplicate_ids_are_an_invariant_violation() {
        let _ = build_search_index([
            page(SearchIndexNamespace::Claude, "same", "one", "one"),
            page(SearchIndexNamespace::Claude, "same", "two", "two"),
        ]);
    }

    #[test]
    fn urls_have_one_trailing_slash() {
        let entries = build_search_index([
            page(SearchIndexNamespace::Claude, "/claude/", "root", "body"),
            page(
                SearchIndexNamespace::Claude,
                "claude-skills/demo/index",
                "nested root",
                "body",
            ),
            page(SearchIndexNamespace::Codex, "index", "site root", "body"),
            page(
                SearchIndexNamespace::Codex,
                "codex/config",
                "config",
                "body",
            ),
        ]);
        assert_eq!(
            entries
                .iter()
                .find(|entry| entry.title == "root")
                .unwrap()
                .url,
            "/docs/claude/"
        );
        assert_eq!(
            entries
                .iter()
                .find(|entry| entry.title == "nested root")
                .unwrap()
                .url,
            "/docs/claude-skills/demo/"
        );
        assert_eq!(
            entries
                .iter()
                .find(|entry| entry.title == "site root")
                .unwrap()
                .url,
            "/docs/"
        );
        assert_eq!(
            entries
                .iter()
                .find(|entry| entry.title == "config")
                .unwrap()
                .url,
            "/docs/codex/config/"
        );
        assert!(entries
            .iter()
            .all(|entry| entry.url.ends_with('/') && !entry.url.ends_with("//")));
    }

    #[test]
    fn empty_input_serializes_to_empty_array() {
        assert_eq!(
            search_index_json(Vec::<SearchIndexPage>::new()).unwrap(),
            "[]"
        );
    }
}
