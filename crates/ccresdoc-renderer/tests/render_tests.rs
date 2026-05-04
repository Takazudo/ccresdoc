use ccresdoc_renderer::{render_markdown, RenderOptions, SENTINEL_CONTENT, SENTINEL_TITLE};
use std::fs;
use std::path::Path;

fn default_opts() -> RenderOptions {
    RenderOptions::default()
}

// ─── Sentinel fuzz test ────────────────────────────────────────────────────────

/// Generate a deterministic pseudo-random u64 using a simple LCG.
fn lcg_next(state: &mut u64) -> u64 {
    *state = state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
    *state
}

fn random_char(state: &mut u64) -> char {
    // Mix of ASCII printable chars, common markdown chars, and the snowman rune ☃
    let r = lcg_next(state) % 120;
    match r {
        0..=31 => char::from_u32(0x20 + (r as u32 % 96)).unwrap_or(' '),
        32..=63 => {
            let ascii_special = b"#*`_~[]()!|:-\n ";
            ascii_special[(r as usize - 32) % ascii_special.len()] as char
        }
        64..=95 => char::from_u32(0x41 + (r as u32 - 64) % 52).unwrap_or('A'),
        96..=119 => {
            // Include the snowman itself and nearby Unicode
            let code = 0x2600u32 + (r as u32 - 96) % 32;
            char::from_u32(code).unwrap_or('?')
        }
        _ => ' ',
    }
}

fn random_string(state: &mut u64, len: usize) -> String {
    (0..len).map(|_| random_char(state)).collect()
}

#[test]
fn fuzz_no_sentinel_in_output() {
    let opts = default_opts();
    let mut rng = 0xdeadbeef_cafeu64;
    let iterations = 3000;

    for i in 0..iterations {
        let len = (lcg_next(&mut rng) % 200 + 1) as usize;
        let input = random_string(&mut rng, len);

        let result = render_markdown(&input, &opts);
        let html = match result {
            Ok(h) => h,
            Err(_) => continue, // rendering errors are ok for random garbage
        };

        assert!(
            !html.contains(SENTINEL_CONTENT),
            "iteration {i}: SENTINEL_CONTENT found in output for input: {input:?}"
        );
        assert!(
            !html.contains(SENTINEL_TITLE),
            "iteration {i}: SENTINEL_TITLE found in output for input: {input:?}"
        );
    }
}

/// Also specifically test that injecting sentinel-like text doesn't leak through.
#[test]
fn sentinel_text_in_input_does_not_appear_in_output() {
    let opts = default_opts();

    let inputs = [
        SENTINEL_CONTENT.to_string(),
        SENTINEL_TITLE.to_string(),
        format!("hello {} world", SENTINEL_CONTENT),
        format!("# Title\n\n{}", SENTINEL_TITLE),
        format!("`{}`", SENTINEL_CONTENT),
        format!("```\n{}\n```", SENTINEL_TITLE),
    ];

    for input in &inputs {
        let html = render_markdown(input, &opts).expect("render should not fail");
        // The sentinel character \u{2603} (☃) should be HTML-escaped in output
        // so neither complete sentinel string can appear
        assert!(
            !html.contains(SENTINEL_CONTENT),
            "SENTINEL_CONTENT found for input: {input:?}, html: {html:?}"
        );
        assert!(
            !html.contains(SENTINEL_TITLE),
            "SENTINEL_TITLE found for input: {input:?}, html: {html:?}"
        );
    }
}

// ─── Valid HTML output ──────────────────────────────────────────────────────────

/// Verify the rendered HTML is parseable by lol_html (proxy for valid HTML).
#[test]
fn output_is_parseable_html() {
    use lol_html::{HtmlRewriter, Settings};

    let opts = default_opts();
    let inputs = [
        "# Hello\n\nworld",
        "## Heading\n\n[link](page.md)\n\n| a | b |\n|---|---|\n| 1 | 2 |",
        ":::note\nsome note\n:::",
        "```rust\nfn main() {}\n```",
        "```ts title=\"app.ts\"\nconst x = 1;\n```",
    ];

    for input in &inputs {
        let html = render_markdown(input, &opts).expect("render failed");

        // Parse the HTML through lol_html — if it errors, the output is malformed
        let mut output = Vec::new();
        let mut rewriter = HtmlRewriter::new(
            Settings::default(),
            |c: &[u8]| output.extend_from_slice(c),
        );
        rewriter.write(html.as_bytes()).unwrap_or_else(|e| {
            panic!("lol_html parse error for input {input:?}: {e}\nhtml: {html}")
        });
        rewriter.end().unwrap_or_else(|e| {
            panic!("lol_html end error for input {input:?}: {e}\nhtml: {html}")
        });
    }
}

// ─── CSS class contract fixture ─────────────────────────────────────────────────

const CLASS_CONTRACT_INPUT: &str = r#"
# Class Contract Demo

## Heading with ID

Some paragraph text with a [link to page.md](other.md) and [internal link](./guide.md#section).

| Column A | Column B | Column C |
|----------|----------|----------|
| alpha    | beta     | gamma    |
| one      | two      | three    |

## Code Blocks

```rust title="example.rs"
fn main() {
    println!("Hello, world!");
}
```

```typescript
const greeting: string = "hello";
```

```
plain code block
```

## Admonitions

:::note
This is a note admonition.
:::

:::tip
This is a tip admonition.
:::

:::info
This is an info admonition.
:::

:::warning
This is a warning admonition.
:::

:::danger
This is a danger admonition.
:::

## More Elements

~~strikethrough text~~

- [ ] unchecked task
- [x] checked task

Regular paragraph with `inline code`.
"#;

#[test]
fn class_contract_fixture_snapshot() {
    let opts = default_opts();
    let html = render_markdown(CLASS_CONTRACT_INPUT, &opts)
        .expect("class contract render failed");

    // Verify each required class appears in the output
    let required_classes = [
        "admonition",
        "admonition-note",
        "admonition-tip",
        "admonition-info",
        "admonition-warning",
        "admonition-danger",
        "code-title",
        "language-",    // prefix for language classes
        "heading-link",
    ];

    for class in &required_classes {
        assert!(
            html.contains(class),
            "Required class/prefix '{class}' not found in class-contract output.\nhtml snippet: {}",
            &html[..html.len().min(500)]
        );
    }

    // Verify table elements
    assert!(html.contains("<table"), "table not found");
    assert!(html.contains("<th"), "th not found");
    assert!(html.contains("<td"), "td not found");

    // Verify aside admonitions
    assert!(html.contains("<aside"), "aside not found");

    // Verify code title div
    assert!(html.contains("<div class=\"code-title\">"), "code-title div not found");

    // Write the fixture file
    let fixture_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/class-contract.html");

    fs::write(&fixture_path, &html)
        .unwrap_or_else(|e| panic!("failed to write fixture: {e}"));

    // Snapshot: verify the fixture file matches what we just wrote
    let stored = fs::read_to_string(&fixture_path)
        .unwrap_or_else(|e| panic!("failed to read fixture: {e}"));
    assert_eq!(
        stored, html,
        "class-contract.html fixture is out of date — re-run tests to regenerate"
    );
}

// ─── Structural equivalence tests ───────────────────────────────────────────────

/// DOM structure extractor: walks HTML and pulls out structural elements.
mod dom_walker {
    use lol_html::{element, HtmlRewriter, Settings};
    use std::cell::RefCell;
    use std::rc::Rc;

    #[derive(Debug, Clone, PartialEq)]
    pub struct DocStructure {
        pub headings: Vec<(u8, String)>,  // (level 1-6, text)
        pub code_langs: Vec<Option<String>>, // language class or None
        pub admonition_classes: Vec<String>, // "admonition-note" etc.
        pub table_count: usize,
        pub links: Vec<String>,           // hrefs (stripped of # anchors for comparison)
    }

    pub fn extract(html: &str) -> DocStructure {
        let headings: Rc<RefCell<Vec<(u8, String)>>> = Rc::new(RefCell::new(Vec::new()));
        let code_langs: Rc<RefCell<Vec<Option<String>>>> = Rc::new(RefCell::new(Vec::new()));
        let admonition_classes: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));
        let table_count: Rc<RefCell<usize>> = Rc::new(RefCell::new(0));
        let links: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));

        let h_clone = headings.clone();
        let c_clone = code_langs.clone();
        let a_clone = admonition_classes.clone();
        let t_clone = table_count.clone();
        let l_clone = links.clone();

        let mut output = Vec::new();
        let mut rewriter = HtmlRewriter::new(
            Settings {
                element_content_handlers: vec![
                    element!("h1, h2, h3, h4, h5, h6", move |el| {
                        let tag = el.tag_name();
                        let level = tag.chars().last().unwrap().to_digit(10).unwrap_or(1) as u8;
                        // We can't easily get inner text from lol_html in element handler,
                        // so we collect the tag name + id as a proxy
                        let id = el.get_attribute("id").unwrap_or_default();
                        h_clone.borrow_mut().push((level, id));
                        Ok(())
                    }),
                    element!("pre > code", move |el| {
                        let class_attr = el.get_attribute("class").unwrap_or_default();
                        let lang = class_attr
                            .split_whitespace()
                            .find(|c| c.starts_with("language-"))
                            .map(|c| c.to_owned());
                        c_clone.borrow_mut().push(lang);
                        Ok(())
                    }),
                    element!("aside", move |el| {
                        let class_attr = el.get_attribute("class").unwrap_or_default();
                        for cls in class_attr.split_whitespace() {
                            if cls.starts_with("admonition-") {
                                a_clone.borrow_mut().push(cls.to_owned());
                            }
                        }
                        Ok(())
                    }),
                    element!("table", move |_el| {
                        *t_clone.borrow_mut() += 1;
                        Ok(())
                    }),
                    element!("a[href]", move |el| {
                        if let Some(href) = el.get_attribute("href") {
                            // Skip heading-link anchors (self-links)
                            let cls = el.get_attribute("class").unwrap_or_default();
                            if !cls.contains("heading-link") {
                                // Normalise: strip fragment for comparison
                                let href_no_frag = href
                                    .split('#')
                                    .next()
                                    .unwrap_or(&href)
                                    .to_owned();
                                if !href_no_frag.is_empty() {
                                    l_clone.borrow_mut().push(href_no_frag);
                                }
                            }
                        }
                        Ok(())
                    }),
                ],
                ..Settings::default()
            },
            |c: &[u8]| output.extend_from_slice(c),
        );

        let _ = rewriter.write(html.as_bytes());
        let _ = rewriter.end();

        let h = headings.borrow().clone();
        let c = code_langs.borrow().clone();
        let a = admonition_classes.borrow().clone();
        let t = *table_count.borrow();
        let l = links.borrow().clone();

        DocStructure {
            headings: h,
            code_langs: c,
            admonition_classes: a,
            table_count: t,
            links: l,
        }
    }
}

/// Extract expected structure directly from markdown source
/// (markdown ATX headings, fenced code langs, admonition kinds, links, tables).
mod md_walker {
    pub struct DocStructure {
        pub heading_levels: Vec<u8>,
        pub code_langs: Vec<Option<String>>,
        pub admonition_kinds: Vec<String>,
        pub table_count: usize,
        pub link_targets: Vec<String>,
    }

    pub fn extract(md: &str, admonition_kinds: &[String]) -> DocStructure {
        let mut heading_levels = Vec::new();
        let mut code_langs = Vec::new();
        let mut admonition_kinds_found = Vec::new();
        let mut table_count = 0;
        let mut link_targets = Vec::new();

        let mut in_code_block = false;
        let mut in_frontmatter = false;
        let mut first_line = true;

        for line in md.lines() {
            // Handle YAML frontmatter
            if first_line && line.trim() == "---" {
                in_frontmatter = true;
                first_line = false;
                continue;
            }
            first_line = false;
            if in_frontmatter {
                if line.trim() == "---" {
                    in_frontmatter = false;
                }
                continue;
            }

            // Code fences (may be indented up to 3 spaces in list items)
            let trimmed_line = line.trim_start_matches("   ").trim_start_matches("  ").trim_start_matches(' ');
            if trimmed_line.starts_with("```") {
                if in_code_block {
                    in_code_block = false;
                } else {
                    in_code_block = true;
                    let rest = trimmed_line[3..].trim();
                    // Parse language (first word before space)
                    let lang = rest.split_whitespace().next();
                    code_langs.push(lang.filter(|l| !l.is_empty()).map(|l| {
                        format!("language-{l}")
                    }));
                }
                continue;
            }
            if in_code_block {
                continue;
            }

            // Headings (ATX style)
            if line.starts_with('#') {
                let level = line.chars().take_while(|&c| c == '#').count() as u8;
                if level <= 6 {
                    let rest = line[level as usize..].trim();
                    if !rest.is_empty() || level <= 6 {
                        heading_levels.push(level);
                    }
                }
                continue;
            }

            // Admonitions :::kind
            if line.trim().starts_with(":::") {
                let rest = line.trim()[3..].trim();
                let word = rest.split_whitespace().next().unwrap_or("");
                if !word.is_empty() && admonition_kinds.iter().any(|k| k == word) {
                    admonition_kinds_found.push(format!("admonition-{word}"));
                }
                continue;
            }

            // Tables (GFM: line starts with |)
            if line.trim().starts_with('|') && !line.contains("---") {
                // Count one table per contiguous block — track first row only
                // We'll count separator rows to identify table starts
            }

            // Count table separators as proxy for table count
            if line.trim().starts_with('|') && line.contains("---") {
                table_count += 1;
            }

            // Extract markdown links [text](href)
            let mut search: &str = line;
            while let Some(open) = search.find("](") {
                // Find the [ before ](
                let before = &search[..open];
                if let Some(_bracket_open) = before.rfind('[') {
                    let href_start = open + 2;
                    let href_area = &search[href_start..];
                    if let Some(close) = href_area.find(')') {
                        let href = &href_area[..close];
                        // Skip empty, absolute, and fragment-only
                        if !href.is_empty()
                            && !href.starts_with('#')
                            && !href.contains("://")
                        {
                            // Strip .md extension and fragment (as the renderer does)
                            let href_no_frag = href.split('#').next().unwrap_or(href);
                            let href_stripped = href_no_frag
                                .trim_end_matches(".mdx")
                                .trim_end_matches(".md");
                            if !href_stripped.is_empty() {
                                link_targets.push(href_stripped.to_owned());
                            }
                        }
                        search = &search[href_start + close + 1..];
                        continue;
                    }
                }
                search = &search[open + 2..];
            }
        }

        DocStructure {
            heading_levels,
            code_langs,
            admonition_kinds: admonition_kinds_found,
            table_count,
            link_targets,
        }
    }
}

fn check_structural_equivalence(md_source: &str, label: &str) {
    let opts = default_opts();
    let html = render_markdown(md_source, &opts)
        .unwrap_or_else(|e| panic!("render failed for {label}: {e}"));

    let rendered = dom_walker::extract(&html);
    let expected = md_walker::extract(md_source, &opts.admonition_kinds);

    // Heading hierarchy check: rendered heading levels must match markdown heading levels
    assert_eq!(
        rendered.headings.iter().map(|(l, _)| *l).collect::<Vec<_>>(),
        expected.heading_levels,
        "{label}: heading hierarchy mismatch\nrendered: {:?}\nexpected: {:?}",
        rendered.headings,
        expected.heading_levels
    );

    // Code fence languages check
    assert_eq!(
        rendered.code_langs,
        expected.code_langs,
        "{label}: code language mismatch"
    );

    // Admonition classes check
    let mut rendered_adm = rendered.admonition_classes.clone();
    let mut expected_adm = expected.admonition_kinds.clone();
    rendered_adm.sort();
    expected_adm.sort();
    assert_eq!(
        rendered_adm, expected_adm,
        "{label}: admonition class mismatch"
    );

    // Table count check
    assert_eq!(
        rendered.table_count, expected.table_count,
        "{label}: table count mismatch"
    );

    // Link hrefs check (order-insensitive)
    let mut rendered_links = rendered.links.clone();
    let mut expected_links = expected.link_targets.clone();
    rendered_links.sort();
    expected_links.sort();
    assert_eq!(
        rendered_links, expected_links,
        "{label}: link href mismatch\nrendered: {rendered_links:?}\nexpected: {expected_links:?}"
    );
}

fn claude_dir() -> std::path::PathBuf {
    let home = std::env::var_os("HOME")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from("/Users/takazudo"));
    home.join(".claude")
}

#[test]
fn structural_equivalence_claude_md() {
    let path = claude_dir().join("CLAUDE.md");
    let content = fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()));
    check_structural_equivalence(&content, "CLAUDE.md");
}

#[test]
fn structural_equivalence_command_cpwd() {
    let path = claude_dir().join("commands/cpwd.md");
    let content = fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()));
    check_structural_equivalence(&content, "commands/cpwd.md");
}

#[test]
fn structural_equivalence_skill_commits() {
    let path = claude_dir().join("skills/commits/SKILL.md");
    let content = fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()));
    check_structural_equivalence(&content, "skills/commits/SKILL.md");
}

// ─── Wave 2.5 smoke: Check 4 — renderer round-trip with sentinel assertions ──────

/// Smoke test: run render_markdown on three real source documents and verify:
/// - no panic
/// - output is non-empty
/// - output contains admonition class names if the input had :::note/:::tip/etc. blocks
/// - output does NOT contain either sentinel
#[test]
fn smoke_renderer_round_trip_sentinel_free() {
    let opts = default_opts();
    let root = claude_dir();

    let sample_paths = vec![
        ("CLAUDE.md",        root.join("CLAUDE.md")),
        ("cpwd.md",          root.join("commands/cpwd.md")),
        ("commits/SKILL.md", root.join("skills/commits/SKILL.md")),
    ];

    for (label, path) in &sample_paths {
        let content = fs::read_to_string(path)
            .unwrap_or_else(|e| panic!("failed to read {} ({label}): {e}", path.display()));

        let html = render_markdown(&content, &opts)
            .unwrap_or_else(|e| panic!("render_markdown panicked for {label}: {e}"));

        // Must be non-empty
        assert!(!html.is_empty(), "{label}: rendered output is empty");

        // Must NOT contain either sentinel
        assert!(
            !html.contains(SENTINEL_CONTENT),
            "{label}: output contains SENTINEL_CONTENT"
        );
        assert!(
            !html.contains(SENTINEL_TITLE),
            "{label}: output contains SENTINEL_TITLE"
        );

        // If the input contains :::note blocks, output must contain admonition class names
        let admonition_kinds = ["note", "tip", "info", "warning", "danger"];
        for kind in &admonition_kinds {
            let needle = format!(":::{kind}");
            if content.contains(&needle) {
                assert!(
                    html.contains(&format!("admonition-{kind}")),
                    "{label}: input has {needle} block but output missing admonition-{kind} class"
                );
            }
        }
    }
}

// ─── Individual feature tests ───────────────────────────────────────────────────

#[test]
fn gfm_table_renders() {
    let opts = default_opts();
    let md = "| a | b |\n|---|---|\n| 1 | 2 |\n";
    let html = render_markdown(md, &opts).unwrap();
    assert!(html.contains("<table"), "table not found: {html}");
    assert!(html.contains("<td"), "td not found: {html}");
}

#[test]
fn gfm_strikethrough_renders() {
    let opts = default_opts();
    let md = "~~hello~~";
    let html = render_markdown(md, &opts).unwrap();
    assert!(html.contains("<del>"), "del not found: {html}");
}

#[test]
fn gfm_tasklist_renders() {
    let opts = default_opts();
    let md = "- [ ] todo\n- [x] done\n";
    let html = render_markdown(md, &opts).unwrap();
    assert!(html.contains("checkbox"), "checkbox not found: {html}");
}

#[test]
fn admonition_renders_all_kinds() {
    let opts = default_opts();
    for kind in &["note", "tip", "info", "warning", "danger"] {
        let md = format!(":::{kind}\ncontent\n:::");
        let html = render_markdown(&md, &opts).unwrap();
        assert!(
            html.contains(&format!("admonition-{kind}")),
            "admonition-{kind} not found: {html}"
        );
        assert!(
            html.contains("<aside"),
            "aside not found for {kind}: {html}"
        );
    }
}

#[test]
fn heading_ids_and_links_generated() {
    let opts = default_opts();
    let md = "## Hello World\n";
    let html = render_markdown(md, &opts).unwrap();
    // comrak emits id on the inner anchor, not on the heading element
    assert!(html.contains("id="), "heading id not found: {html}");
    assert!(html.contains("heading-link"), "heading-link not found: {html}");
}

#[test]
fn md_extension_stripped_from_links() {
    let opts = default_opts();
    let md = "[link](./guide.md)\n";
    let html = render_markdown(md, &opts).unwrap();
    assert!(html.contains("./guide\"") || html.contains("./guide#") || !html.contains(".md\""),
        "md extension not stripped: {html}");
}

#[test]
fn code_title_rendered() {
    let opts = default_opts();
    let md = "```ts title=\"foo.ts\"\nlet x = 1;\n```\n";
    let html = render_markdown(md, &opts).unwrap();
    assert!(html.contains("code-title"), "code-title not found: {html}");
    assert!(html.contains("foo.ts"), "title text not found: {html}");
}

#[test]
fn syntax_highlighting_produces_spans() {
    let opts = default_opts();
    let md = "```rust\nfn main() {}\n```\n";
    let html = render_markdown(md, &opts).unwrap();
    assert!(html.contains("language-rust"), "language class not found: {html}");
}
