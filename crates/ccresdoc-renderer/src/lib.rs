pub mod admonitions;
pub mod code_title;
pub mod heading_links;
pub mod highlight;
pub mod strip_md;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum RenderError {
    #[error("lol_html error: {0}")]
    LolHtml(String),
    #[error("syntect error: {0}")]
    Syntect(#[from] syntect::Error),
}

/// Options controlling rendering behaviour.
pub struct RenderOptions {
    /// Prefix for generated heading `id` attributes (comrak `header_ids`).
    pub heading_id_prefix: Option<String>,
    /// Strip `.md` / `.mdx` from relative anchor `href`s.
    pub strip_md_extension_in_links: bool,
    /// Recognised admonition kinds, e.g. `["note","tip","info","warning","danger"]`.
    pub admonition_kinds: Vec<String>,
    /// Syntect theme name for syntax highlighting.
    /// Use `"dracula"` (default dark) or `"github-light"` (default light).
    pub syntax_highlight_theme: String,
}

impl Default for RenderOptions {
    fn default() -> Self {
        Self {
            heading_id_prefix: Some(String::new()),
            strip_md_extension_in_links: true,
            admonition_kinds: vec![
                "note".into(),
                "tip".into(),
                "info".into(),
                "warning".into(),
                "danger".into(),
            ],
            // dracula is the default dark theme to match existing shiki config
            syntax_highlight_theme: "dracula".into(),
        }
    }
}

/// Sentinel strings that MUST NOT appear in any rendered output.
/// These are substituted at runtime by the Tauri shell.
pub const SENTINEL_CONTENT: &str = "\u{2603}CCRESDOC_CONTENT_SLOT\u{2603}";
pub const SENTINEL_TITLE: &str = "\u{2603}CCRESDOC_TITLE_SLOT\u{2603}";

/// Strip YAML frontmatter (`---\n...\n---\n`) from the start of a markdown string.
/// Returns the input unchanged if no valid frontmatter is present.
/// Handles both Unix (\n) and Windows (\r\n) line endings.
fn strip_frontmatter(input: &str) -> &str {
    // Frontmatter must start at byte 0 with exactly "---" followed by a line ending
    let after_open = if input.starts_with("---\r\n") {
        &input[5..]
    } else if input.starts_with("---\n") {
        &input[4..]
    } else {
        return input;
    };

    // Walk lines looking for a closing "---" on its own line.
    // We accumulate byte offsets into `after_open`.
    let mut offset = 0usize;
    for line in after_open.split('\n') {
        // Normalise \r\n: strip trailing \r
        let line_trimmed = line.trim_end_matches('\r');
        let line_len = line.len() + 1; // +1 for the '\n' split consumed

        if line_trimmed == "---" {
            // offset now points to the start of "---\n"
            let tail = &after_open[offset + line_len..];
            // Skip blank lines immediately after the closing fence
            let tail = if tail.starts_with("\r\n") { &tail[2..] } else if tail.starts_with('\n') { &tail[1..] } else { tail };
            return tail;
        }

        offset += line_len;
        if offset >= after_open.len() {
            break;
        }
    }

    // No closing --- found — not valid frontmatter
    input
}

/// Render `input` markdown to an HTML body fragment.
///
/// The returned string is suitable for direct insertion into a page shell.
/// It is guaranteed never to contain [`SENTINEL_CONTENT`] or [`SENTINEL_TITLE`].
pub fn render_markdown(input: &str, opts: &RenderOptions) -> Result<String, RenderError> {
    // Strip YAML frontmatter before any processing
    let input = strip_frontmatter(input);

    // Pre-process: convert :::kind ... ::: blocks to raw HTML aside elements
    let preprocessed = admonitions::preprocess(input, &opts.admonition_kinds);

    // Render with comrak
    let comrak_opts = build_comrak_options(opts);
    let raw_html = comrak::markdown_to_html(&preprocessed, &comrak_opts);

    // Post-process with lol_html passes
    let html = post_process(&raw_html, opts)?;

    // Escape the sentinel delimiter character (☃ U+2603) so neither
    // SENTINEL_CONTENT nor SENTINEL_TITLE can ever appear in rendered output.
    // This is a safety invariant: the sentinels are used for runtime slot
    // substitution by the Tauri shell and must never be emitted by user content.
    let html = html.replace('\u{2603}', "&#x2603;");

    Ok(html)
}

fn build_comrak_options(opts: &RenderOptions) -> comrak::Options<'_> {
    let mut options = comrak::Options::default();

    // GFM extensions
    options.extension.table = true;
    options.extension.strikethrough = true;
    options.extension.tasklist = true;
    options.extension.autolink = false;

    // Generate heading ids
    if let Some(prefix) = &opts.heading_id_prefix {
        options.extension.header_ids = Some(prefix.clone());
    }

    // Allow raw HTML (needed for our admonition pre-processing)
    options.render.unsafe_ = true;

    // Emit the full info string (everything after the language token) as data-meta
    // so our code-title post-pass can extract title="..." from it.
    options.render.full_info_string = true;

    options
}

fn post_process(html: &str, opts: &RenderOptions) -> Result<String, RenderError> {
    // Apply syntax highlighting first (replaces <code class="language-X"> blocks)
    let html = highlight::apply_highlighting(html, &opts.syntax_highlight_theme)?;

    // Extract code title attributes and insert <div class="code-title"> siblings
    let html = code_title::apply_code_titles(&html)?;

    // Wrap heading content in <a class="heading-link"> anchors
    let html = heading_links::apply_heading_links(&html)?;

    // Strip .md/.mdx from relative links
    let html = if opts.strip_md_extension_in_links {
        strip_md::apply_strip_md(&html)?
    } else {
        html
    };

    Ok(html)
}
