//! MDX generation — port of zudo-doc's `generate.ts`, adapted to Wave 1's
//! CCResDoc content contract (`app/CLAUDE.md`).
//!
//! Differences from upstream `generate.ts`, driven by the Wave 1 contract:
//! - CLAUDE.md per-item filenames follow the contract: the root file is
//!   `global.mdx` (slug `claude-md/global`) and nested files are
//!   `project-<dir-slug>.mdx`. Upstream uses `<dir-slug>.mdx` / `root.mdx`.
//! - Category index `sidebar_position` values follow the contract:
//!   CLAUDE.md=900, commands=901, skills=902, agents=903. The coordinator owns
//!   the routed `claude/index.mdx` overview at position 899.
//!
//! Skill sub-pages and frontmatter remain aligned with upstream. Escaping also
//! normalizes CommonMark constructs that zudo-doc's MDX mode would reject.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use crate::error::{GenerateError, Result};
use crate::escape::escape_for_mdx;
use crate::walk::{self, AgentItem, ClaudeMdItem, CommandItem, SkillItem};
use crate::Config;

/// Counts of generated resources, returned by [`crate::generate`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct GenerateReport {
    pub claude_md: usize,
    pub commands: usize,
    pub skills: usize,
    pub agents: usize,
}

// ---------------------------------------------------------------------------
// Filesystem helpers
// ---------------------------------------------------------------------------

fn ensure_dir(dir: &Path) -> Result<()> {
    if !dir.exists() {
        std::fs::create_dir_all(dir).map_err(|e| GenerateError::Io {
            path: dir.to_owned(),
            source: e,
        })?;
    }
    Ok(())
}

fn clean_dir(dir: &Path) -> Result<()> {
    match std::fs::remove_dir_all(dir) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            // Already gone — nothing to clean; not an error.
        }
        Err(e) => {
            // Non-fatal: log but don't abort generate(). A transient read/remove
            // error during cleanup (e.g. locked file on Windows) should not
            // prevent the subsequent write phase from succeeding.
            log::warn!("clean_dir: could not remove {}: {e}", dir.display());
        }
    }
    Ok(())
}

/// Remove any regular file directly inside `dir` whose file name is NOT in
/// `keep`. Subdirectories are left alone (the category dirs are flat).
///
/// This replaces the old "wipe the whole dir then rewrite everything" cleanup:
/// kept files are never touched here (their mtimes are preserved by the
/// idempotent [`write_file`]), and only genuinely stale outputs are deleted.
fn prune_stale(dir: &Path, keep: &HashSet<String>) -> Result<()> {
    if !dir.exists() {
        return Ok(());
    }
    let entries = std::fs::read_dir(dir).map_err(|e| GenerateError::Io {
        path: dir.to_owned(),
        source: e,
    })?;
    for entry in entries {
        let entry = entry.map_err(|e| GenerateError::Io {
            path: dir.to_owned(),
            source: e,
        })?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let name = match path.file_name().and_then(|n| n.to_str()) {
            Some(n) => n.to_owned(),
            None => continue,
        };
        if !keep.contains(&name) {
            std::fs::remove_file(&path).map_err(|e| GenerateError::Io {
                path: path.clone(),
                source: e,
            })?;
        }
    }
    Ok(())
}

/// Remove generated child directories that are no longer present in `keep`.
///
/// Skill pages use the latest zudo-doc hierarchical layout
/// (`<skill>/index.mdx` plus sibling sub-pages), so stale skills are whole
/// directories rather than flat files. The category output directory is owned
/// by this generator; removing an unreferenced child directory is therefore
/// safe and prevents deleted skills from remaining routable.
fn prune_stale_subdirs(dir: &Path, keep: &HashSet<String>) -> Result<()> {
    if !dir.exists() {
        return Ok(());
    }
    let entries = std::fs::read_dir(dir).map_err(|e| GenerateError::Io {
        path: dir.to_owned(),
        source: e,
    })?;
    for entry in entries {
        let entry = entry.map_err(|e| GenerateError::Io {
            path: dir.to_owned(),
            source: e,
        })?;
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if !keep.contains(name) {
            std::fs::remove_dir_all(&path).map_err(|e| GenerateError::Io {
                path: path.clone(),
                source: e,
            })?;
        }
    }
    Ok(())
}

/// Write `contents` to `path`, but skip the write if the file already holds
/// byte-identical content.
///
/// This makes regeneration **idempotent at the filesystem layer**: a no-op
/// regen (e.g. one triggered by unrelated `~/.claude` churn) leaves every
/// unchanged MDX file's mtime untouched. That is load-bearing for the watch
/// loop — `zfb dev`'s content-watch keys off mtimes, so an unconditional
/// rewrite of all ~237 files would retrigger a full rebuild even when nothing
/// changed. Read errors (e.g. file absent) fall through to a normal write.
fn write_file(path: &Path, contents: &str) -> Result<()> {
    if let Ok(existing) = std::fs::read(path) {
        if existing == contents.as_bytes() {
            return Ok(());
        }
    }
    std::fs::write(path, contents).map_err(|e| GenerateError::Io {
        path: path.to_owned(),
        source: e,
    })
}

/// Escape a value embedded in a double-quoted YAML frontmatter scalar.
/// Backslashes first (so `C:\path` / `\d` stay valid), then double quotes.
fn escape_title(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

fn assert_not_index_reserved(name_or_slug: &str, message: &str) -> Result<()> {
    if name_or_slug == "index" {
        return Err(GenerateError::ReservedSlug(message.to_owned()));
    }
    Ok(())
}

fn write_category_index(
    output_dir: &Path,
    label: &str,
    position: u32,
    description: &str,
) -> Result<()> {
    let mdx = format!(
        "---\ntitle: \"{}\"\ndescription: \"{}\"\nsidebar_position: {}\ncategory_no_page: true\ngenerated: true\n---\n",
        escape_title(label),
        escape_title(description),
        position,
    );
    write_file(&output_dir.join("index.mdx"), &mdx)
}

/// Write an unlisted sub-page MDX file.
///
/// Its route is derived from its hierarchical on-disk path. This is required
/// by current zfb Markdown-link resolution: a `./ref-name` link from
/// `<skill>/index.mdx` resolves to the sibling `<skill>/ref-name.mdx` file.
fn write_unlisted_sub_page(output_path: &Path, title: &str, body: &str) -> Result<()> {
    let mdx = format!(
        "---\ntitle: \"{}\"\nunlisted: true\ngenerated: true\n---\n\n{}\n",
        escape_title(title),
        body,
    );
    write_file(output_path, &mdx)
}

/// Whether a Markdown link points at a repository-relative file rather than a
/// URL the generated documentation site can resolve.
fn is_repo_relative_link(url: &str) -> bool {
    let url = url.trim();
    if url.is_empty() || url.starts_with('#') || url.starts_with('/') {
        return false;
    }

    // URI scheme: ALPHA *( ALPHA / DIGIT / "+" / "-" / "." ) ":"
    let Some(colon) = url.find(':') else {
        return true;
    };
    let scheme = &url[..colon];
    let mut chars = scheme.chars();
    !matches!(chars.next(), Some(first) if first.is_ascii_alphabetic())
        || !chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
}

/// Downgrade repository-relative links in a mirrored `CLAUDE.md` to inline
/// code. The flattened CLAUDE.md output has no matching repository files, so
/// retaining those hrefs produces broken-link diagnostics in current zfb.
/// Resolvable URLs, anchors, site-absolute links, fenced code, and inline code
/// remain untouched.
pub(crate) fn downgrade_repo_relative_links(content: &str) -> String {
    let mut out = String::with_capacity(content.len());
    let mut fence: Option<(char, usize)> = None;

    for line in content.split_inclusive('\n') {
        let trimmed = line.trim_start();
        let delimiter = trimmed.chars().next().filter(|c| matches!(c, '`' | '~'));
        let run = delimiter.map_or(0, |delimiter| {
            trimmed.chars().take_while(|c| *c == delimiter).count()
        });

        if let Some((open, width)) = fence {
            out.push_str(line);
            if delimiter == Some(open) && run >= width {
                fence = None;
            }
            continue;
        }
        if let Some(delimiter) = delimiter.filter(|_| run >= 3) {
            fence = Some((delimiter, run));
            out.push_str(line);
            continue;
        }

        out.push_str(&downgrade_links_in_prose(line));
    }

    // `split_inclusive` emits the final non-newline-terminated segment, but it
    // emits nothing for an empty input.
    out
}

fn downgrade_links_in_prose(line: &str) -> String {
    let bytes = line.as_bytes();
    let mut out = String::with_capacity(line.len());
    let mut i = 0;

    while i < bytes.len() {
        // Preserve an inline-code span, including its exact backtick width.
        if bytes[i] == b'`' {
            let width = bytes[i..].iter().take_while(|b| **b == b'`').count();
            if let Some(close) = find_backtick_close(bytes, i + width, width) {
                out.push_str(&line[i..close]);
                i = close;
                continue;
            }
        }

        let marker_start = i;
        let bracket_start = if bytes[i] == b'!' && bytes.get(i + 1) == Some(&b'[') {
            i + 1
        } else if bytes[i] == b'[' {
            i
        } else {
            let width = utf8_char_width(bytes[i]);
            out.push_str(&line[i..i + width]);
            i += width;
            continue;
        };

        let Some(label_end_rel) = line[bracket_start + 1..].find(']') else {
            out.push_str(&line[marker_start..]);
            break;
        };
        let label_end = bracket_start + 1 + label_end_rel;
        if bytes.get(label_end + 1) != Some(&b'(') {
            let width = utf8_char_width(bytes[marker_start]);
            out.push_str(&line[marker_start..marker_start + width]);
            i = marker_start + width;
            continue;
        }
        let Some(url_end_rel) = line[label_end + 2..].find(')') else {
            out.push_str(&line[marker_start..]);
            break;
        };
        let url_end = label_end + 2 + url_end_rel;
        let label = &line[bracket_start + 1..label_end];
        let url = &line[label_end + 2..url_end];
        if is_repo_relative_link(url) {
            out.push('`');
            out.push_str(label);
            out.push('`');
        } else {
            out.push_str(&line[marker_start..=url_end]);
        }
        i = url_end + 1;
    }

    out
}

fn find_backtick_close(bytes: &[u8], mut i: usize, width: usize) -> Option<usize> {
    while i < bytes.len() {
        if bytes[i] != b'`' {
            i += utf8_char_width(bytes[i]);
            continue;
        }
        let run = bytes[i..].iter().take_while(|b| **b == b'`').count();
        if run == width {
            return Some(i + run);
        }
        i += run;
    }
    None
}

fn utf8_char_width(first: u8) -> usize {
    match first {
        0x00..=0x7f => 1,
        0xc0..=0xdf => 2,
        0xe0..=0xef => 3,
        _ => 4,
    }
}

// ---------------------------------------------------------------------------
// CLAUDE.md generation
// ---------------------------------------------------------------------------

/// Map a CLAUDE.md walker slug to the contract output filename (without
/// extension): `root` -> `global`, otherwise `project-<dir-slug>`.
fn claude_md_output_name(slug: &str) -> String {
    if slug == "root" {
        "global".to_owned()
    } else {
        format!("project-{slug}")
    }
}

fn generate_claudemd_docs(items: &[ClaudeMdItem], docs_dir: &Path) -> Result<usize> {
    let output_dir = docs_dir.join("claude-md");

    if items.is_empty() {
        clean_dir(&output_dir)?;
        return Ok(0);
    }

    ensure_dir(&output_dir)?;

    let mut keep: HashSet<String> = HashSet::new();
    keep.insert("index.mdx".to_owned());

    let mut emitted: HashMap<String, String> = HashMap::new();
    for (index, item) in items.iter().enumerate() {
        let out_name = claude_md_output_name(&item.slug);
        assert_not_index_reserved(
            &out_name,
            &format!(
                "claude-resources: \"{}\" maps to the reserved slug \"index\". Rename the directory to resolve the conflict.",
                item.rel_path
            ),
        )?;
        // Key on the lowercased name: the macOS target filesystem is
        // case-insensitive, so `Foo.mdx` and `foo.mdx` collide on disk.
        let out_key = out_name.to_ascii_lowercase();
        if let Some(prev) = emitted.get(&out_key) {
            return Err(GenerateError::SlugCollision(format!(
                "\"{out_name}\" is produced by both \"{prev}\" and \"{}\". Rename one of the directories.",
                item.rel_path
            )));
        }
        emitted.insert(out_key, item.rel_path.clone());

        // sidebar_position is 1-based within the category. The category
        // index itself carries 900; per-item positions start at 1. Cap at
        // 899 so no item ever collides with the adjacent category header
        // (the next category starts at 901 and the overview is 899). In
        // practice ~/.claude rarely holds >100 CLAUDE.md files, so the cap
        // is a safety net rather than an everyday limit.
        let position = (index + 1).min(899);
        let mdx = format!(
            "---\ntitle: \"{}\"\ndescription: \"CLAUDE.md at {}\"\nsidebar_position: {}\nsidebar_label: \"{}\"\ngenerated: true\n---\n\n**Path:** `{}`\n\n{}\n",
            escape_title(&item.display_path),
            escape_title(&item.display_path),
            position,
            escape_title(&item.rel_path),
            item.rel_path,
            escape_for_mdx(&downgrade_repo_relative_links(item.raw_content.trim())),
        );
        let file_name = format!("{out_name}.mdx");
        write_file(&output_dir.join(&file_name), &mdx)?;
        keep.insert(file_name);
    }

    write_category_index(
        &output_dir,
        "CLAUDE.md",
        900,
        "Project-specific instructions",
    )?;
    prune_stale(&output_dir, &keep)?;
    Ok(items.len())
}

// ---------------------------------------------------------------------------
// Commands generation
// ---------------------------------------------------------------------------

fn generate_commands_docs(items: &[CommandItem], docs_dir: &Path) -> Result<usize> {
    let output_dir = docs_dir.join("claude-commands");

    if items.is_empty() {
        clean_dir(&output_dir)?;
        return Ok(0);
    }

    ensure_dir(&output_dir)?;

    let mut keep: HashSet<String> = HashSet::new();
    keep.insert("index.mdx".to_owned());

    // Per-category slug collision tracking: command files whose stems differ
    // only by extension or case would produce the same output .mdx filename.
    let mut emitted_slugs: HashMap<String, String> = HashMap::new();

    for item in items {
        assert_not_index_reserved(
            &item.name,
            "claude-resources: a command named \"index\" conflicts with the category metadata file. Rename the command file.",
        )?;
        if let Some(prev) = emitted_slugs.get(&item.name.to_ascii_lowercase()) {
            return Err(GenerateError::SlugCollision(format!(
                "claude-commands: slug \"{}\" would be produced by both \"{}\" and \"{}\". Rename one of the command files.",
                item.name, prev, item.name
            )));
        }
        emitted_slugs.insert(item.name.to_ascii_lowercase(), item.name.clone());
        let mdx = format!(
            "---\ntitle: \"{}\"\ndescription: \"{}\"\nsidebar_label: \"{}\"\ngenerated: true\n---\n\n{}\n",
            escape_title(&item.name),
            escape_title(&item.description),
            escape_title(&item.name),
            escape_for_mdx(item.raw_content.trim()),
        );
        let file_name = format!("{}.mdx", item.name);
        write_file(&output_dir.join(&file_name), &mdx)?;
        keep.insert(file_name);
    }

    write_category_index(&output_dir, "Commands", 901, "Custom slash commands")?;
    prune_stale(&output_dir, &keep)?;
    Ok(items.len())
}

// ---------------------------------------------------------------------------
// Skills generation
// ---------------------------------------------------------------------------

fn render_skill_file_tree(skill_dir: &str, sub_dirs: &[(&str, Vec<String>)]) -> String {
    let mut lines: Vec<String> = vec![format!("{skill_dir}/")];

    // entries: SKILL.md (file) + each sub dir
    let total = 1 + sub_dirs.len();
    let mut idx = 0;

    // SKILL.md
    let is_last = idx == total - 1;
    lines.push(format!("{}SKILL.md", if is_last { "└── " } else { "├── " }));
    idx += 1;

    for (name, children) in sub_dirs {
        let is_last = idx == total - 1;
        let prefix = if is_last { "└── " } else { "├── " };
        lines.push(format!("{prefix}{name}/"));
        let continuation = if is_last { "    " } else { "│   " };
        for (j, child) in children.iter().enumerate() {
            let child_is_last = j == children.len() - 1;
            let child_prefix = if child_is_last {
                "└── "
            } else {
                "├── "
            };
            lines.push(format!("{continuation}{child_prefix}{child}"));
        }
        idx += 1;
    }

    lines.join("\n")
}

fn generate_skills_docs(items: &[SkillItem], docs_dir: &Path) -> Result<usize> {
    let output_dir = docs_dir.join("claude-skills");

    if items.is_empty() {
        clean_dir(&output_dir)?;
        return Ok(0);
    }

    ensure_dir(&output_dir)?;

    let mut keep_files: HashSet<String> = HashSet::new();
    keep_files.insert("index.mdx".to_owned());
    let mut keep_dirs: HashSet<String> = HashSet::new();

    // Per-category slug collision tracking: skill dirs whose names differ
    // only by case would produce the same output .mdx filename.
    let mut emitted_slugs: HashMap<String, String> = HashMap::new();

    for skill in items {
        assert_not_index_reserved(
            &skill.dir,
            "claude-resources: a skill directory named \"index\" conflicts with the category metadata file. Rename the skill directory.",
        )?;
        if let Some(prev) = emitted_slugs.get(&skill.dir.to_ascii_lowercase()) {
            return Err(GenerateError::SlugCollision(format!(
                "claude-skills: slug \"{}\" would be produced by both \"{}\" and \"{}\". Rename one of the skill directories.",
                skill.dir, prev, skill.dir
            )));
        }
        emitted_slugs.insert(skill.dir.to_ascii_lowercase(), skill.dir.clone());
        keep_dirs.insert(skill.dir.clone());
        let skill_output_dir = output_dir.join(&skill.dir);
        ensure_dir(&skill_output_dir)?;
        let mut keep_skill_files: HashSet<String> = HashSet::new();
        keep_skill_files.insert("index.mdx".to_owned());

        let script_md: Vec<&_> = skill
            .script_files
            .iter()
            .filter(|f| f.is_markdown)
            .collect();
        let asset_md: Vec<&_> = skill.asset_files.iter().filter(|f| f.is_markdown).collect();

        // File tree (lists ALL sub-files, markdown and binary).
        let mut sub_dirs: Vec<(&str, Vec<String>)> = Vec::new();
        if !skill.script_files.is_empty() {
            sub_dirs.push((
                "scripts",
                skill
                    .script_files
                    .iter()
                    .map(|f| f.filename.clone())
                    .collect(),
            ));
        }
        if !skill.references.is_empty() {
            sub_dirs.push((
                "references",
                skill
                    .references
                    .iter()
                    .map(|r| format!("{}.md", r.name))
                    .collect(),
            ));
        }
        if !skill.asset_files.is_empty() {
            sub_dirs.push((
                "assets",
                skill
                    .asset_files
                    .iter()
                    .map(|f| f.filename.clone())
                    .collect(),
            ));
        }

        let mut file_structure_section = String::new();
        if !sub_dirs.is_empty() {
            let tree = format!(
                "```\n{}\n```",
                render_skill_file_tree(&skill.dir, &sub_dirs)
            );

            let mut links: Vec<String> = Vec::new();
            for r in &skill.references {
                links.push(format!("- [references/{}.md](./ref-{})", r.name, r.name));
            }
            for f in &script_md {
                let slug = f.filename.trim_end_matches(".md");
                links.push(format!("- [scripts/{}](./script-{})", f.filename, slug));
            }
            for f in &asset_md {
                let slug = f.filename.trim_end_matches(".md");
                links.push(format!("- [assets/{}](./asset-{})", f.filename, slug));
            }

            let link_list = if links.is_empty() {
                String::new()
            } else {
                format!("\n\n{}", links.join("\n"))
            };
            file_structure_section = format!("## File Structure\n\n{tree}{link_list}");
        }

        let short_desc = if skill.description.chars().count() > 200 {
            let truncated: String = skill.description.chars().take(200).collect();
            format!("{truncated}...")
        } else {
            skill.description.clone()
        };

        // Rewrite references/scripts/assets links in the skill body to match
        // doc-site URLs.
        let skill_body = rewrite_skill_links(skill.raw_content.trim());

        let body = {
            let escaped = escape_for_mdx(&skill_body);
            if file_structure_section.is_empty() {
                escaped
            } else {
                format!("{file_structure_section}\n\n{escaped}")
            }
        };

        let mdx = format!(
            "---\ntitle: \"{}\"\ndescription: \"{}\"\nsidebar_label: \"{}\"\ngenerated: true\n---\n\n{}",
            escape_title(&skill.name),
            escape_title(&short_desc),
            escape_title(&skill.name),
            body,
        );
        write_file(&skill_output_dir.join("index.mdx"), &mdx)?;

        // Unlisted sub-pages are genuine siblings of the skill's index page.

        for r in &skill.references {
            let file_name = format!("ref-{}.mdx", r.name);
            write_unlisted_sub_page(
                &skill_output_dir.join(&file_name),
                &r.title,
                &escape_for_mdx(r.raw_content.trim()),
            )?;
            keep_skill_files.insert(file_name);
        }
        for f in &script_md {
            let slug = f.filename.trim_end_matches(".md");
            let raw = f.raw_content.as_deref().unwrap_or("");
            let title = f.title.clone().unwrap_or_else(|| slug.to_owned());
            let file_name = format!("script-{slug}.mdx");
            write_unlisted_sub_page(
                &skill_output_dir.join(&file_name),
                &title,
                &escape_for_mdx(raw.trim()),
            )?;
            keep_skill_files.insert(file_name);
        }
        for f in &asset_md {
            let slug = f.filename.trim_end_matches(".md");
            let raw = f.raw_content.as_deref().unwrap_or("");
            let title = f.title.clone().unwrap_or_else(|| slug.to_owned());
            let file_name = format!("asset-{slug}.mdx");
            write_unlisted_sub_page(
                &skill_output_dir.join(&file_name),
                &title,
                &escape_for_mdx(raw.trim()),
            )?;
            keep_skill_files.insert(file_name);
        }
        prune_stale(&skill_output_dir, &keep_skill_files)?;
    }

    write_category_index(&output_dir, "Skills", 902, "Skill packages")?;
    // Remove legacy flat skill pages while preserving the category index, then
    // prune nested directories for skills that were deleted.
    prune_stale(&output_dir, &keep_files)?;
    prune_stale_subdirs(&output_dir, &keep_dirs)?;
    Ok(items.len())
}

/// Rewrite `](references/x.md)` → `](./ref-x)`, `scripts/` → `./script-`,
/// `assets/` → `./asset-`, matching the JS regex replacements.
///
/// Rewrites are skipped inside fenced code blocks (``` ... ```) so that
/// example code showing the original `](references/...)` syntax is preserved
/// verbatim.
pub(crate) fn rewrite_skill_links(body: &str) -> String {
    // Split the body into fenced-code and prose segments, rewrite only prose.
    let segments = split_code_and_prose(body);
    let mut out = String::with_capacity(body.len());
    for (is_code, segment) in segments {
        if is_code {
            out.push_str(segment);
        } else {
            let mut prose = segment.to_owned();
            prose = rewrite_link_kind(&prose, "references/", "ref-");
            prose = rewrite_link_kind(&prose, "scripts/", "script-");
            prose = rewrite_link_kind(&prose, "assets/", "asset-");
            out.push_str(&prose);
        }
    }
    out
}

/// Split `input` into alternating `(is_code, slice)` pairs. Fenced code
/// blocks (3+ backtick fence lines) are returned with `is_code = true`;
/// everything else with `is_code = false`.
///
/// This mirrors the fence-detection logic in `escape.rs` so link rewriting
/// never touches the interior of a fenced code block.
fn split_code_and_prose(input: &str) -> Vec<(bool, &str)> {
    let bytes = input.as_bytes();
    let len = bytes.len();
    let mut segments: Vec<(bool, &str)> = Vec::new();
    let mut prose_start = 0;
    let mut i = 0;

    while i < len {
        // Fence may only open at the start of a line.
        let prev = if i > 0 { bytes[i - 1] } else { b'\n' };
        let at_line_start = prev == b'\n' || prev == b'\r';
        if at_line_start && bytes[i] == b'`' {
            let fence_start = i;
            let mut fence_len = 0;
            while i < len && bytes[i] == b'`' {
                fence_len += 1;
                i += 1;
            }
            if fence_len >= 3 {
                // Consume the rest of the opening line.
                while i < len && bytes[i] != b'\n' {
                    i += 1;
                }
                if i < len {
                    i += 1; // include the newline
                }
                // Find the closing fence (>= fence_len backticks at line start).
                let close_end = find_prose_closing_fence(bytes, i, fence_len);
                let block_end = close_end.unwrap_or(len);
                // Emit any prose accumulated before this fence.
                if prose_start < fence_start {
                    segments.push((false, &input[prose_start..fence_start]));
                }
                segments.push((true, &input[fence_start..block_end]));
                prose_start = block_end;
                i = block_end;
                continue;
            }
            // Not a real fence (1-2 backticks) — fall through, let i advance.
            // (i already moved past the backticks in the counting loop above)
            continue;
        }
        i += 1;
    }

    // Remaining prose after the last fence (or the whole input if no fences).
    if prose_start < len {
        segments.push((false, &input[prose_start..]));
    }
    segments
}

/// Find the byte offset just past a closing fence of at least `fence_len`
/// backticks, scanning from `from`. Returns `None` if no closing fence is
/// found (unclosed fence — treat remainder as code).
fn find_prose_closing_fence(bytes: &[u8], from: usize, fence_len: usize) -> Option<usize> {
    let len = bytes.len();
    let mut i = from;
    while i < len {
        // Must be at line start.
        let prev = if i > 0 { bytes[i - 1] } else { b'\n' };
        if (prev == b'\n' || prev == b'\r') && bytes[i] == b'`' {
            let run_start = i;
            let mut run = 0;
            while i < len && bytes[i] == b'`' {
                run += 1;
                i += 1;
            }
            if run >= fence_len {
                // Consume the rest of the closing fence line.
                while i < len && bytes[i] != b'\n' {
                    i += 1;
                }
                if i < len {
                    i += 1; // include the newline
                }
                let _ = run_start; // suppress unused warning
                return Some(i);
            }
        } else {
            i += 1;
        }
    }
    None
}

/// Replace `](<dir><name>.md)` with `](./<prefix><name>)`. `<name>` matches the
/// JS `[^)]+` (any run of non-`)` chars).
fn rewrite_link_kind(input: &str, dir: &str, prefix: &str) -> String {
    let needle_open = format!("]({dir}");
    let mut out = String::with_capacity(input.len());
    let bytes = input.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        // Literal Markdown examples commonly put a link in inline code. Keep
        // the entire span byte-for-byte, including arbitrary delimiter width.
        if bytes[index] == b'`' {
            let width = bytes[index..].iter().take_while(|b| **b == b'`').count();
            if let Some(close) = find_backtick_close(bytes, index + width, width) {
                out.push_str(&input[index..close]);
                index = close;
                continue;
            }
        }

        if input[index..].starts_with(&needle_open) {
            let after_start = index + needle_open.len();
            let after = &input[after_start..];
            if let Some(close) = after.find(')') {
                let inner = &after[..close];
                if let Some(name) = inner.strip_suffix(".md") {
                    out.push_str(&format!("](./{prefix}{name})"));
                    index = after_start + close + 1;
                    continue;
                }
            }
        }

        let width = utf8_char_width(bytes[index]);
        out.push_str(&input[index..index + width]);
        index += width;
    }
    out
}

// ---------------------------------------------------------------------------
// Agents generation
// ---------------------------------------------------------------------------

fn generate_agents_docs(items: &[AgentItem], docs_dir: &Path) -> Result<usize> {
    let output_dir = docs_dir.join("claude-agents");

    if items.is_empty() {
        clean_dir(&output_dir)?;
        return Ok(0);
    }

    ensure_dir(&output_dir)?;

    let mut keep: HashSet<String> = HashSet::new();
    keep.insert("index.mdx".to_owned());

    // Per-category slug collision tracking: agent files whose stems differ
    // only by extension or case would produce the same output .mdx filename.
    let mut emitted_slugs: HashMap<String, String> = HashMap::new();

    for agent in items {
        assert_not_index_reserved(
            &agent.file_slug,
            "claude-resources: an agent named \"index\" conflicts with the category metadata file. Rename the agent file.",
        )?;
        if let Some(prev) = emitted_slugs.get(&agent.file_slug.to_ascii_lowercase()) {
            return Err(GenerateError::SlugCollision(format!(
                "claude-agents: slug \"{}\" would be produced by both \"{}\" and \"{}\". Rename one of the agent files.",
                agent.file_slug, prev, agent.file_slug
            )));
        }
        emitted_slugs.insert(
            agent.file_slug.to_ascii_lowercase(),
            agent.file_slug.clone(),
        );
        let model_badge = if agent.model.is_empty() {
            String::new()
        } else {
            format!("**Model:** `{}`\n", agent.model)
        };
        let mdx = format!(
            "---\ntitle: \"{}\"\ndescription: \"{}\"\nsidebar_label: \"{}\"\ngenerated: true\n---\n\n{}\n{}\n",
            escape_title(&agent.name),
            escape_title(&agent.description),
            escape_title(&agent.name),
            model_badge,
            escape_for_mdx(agent.raw_content.trim()),
        );
        let file_name = format!("{}.mdx", agent.file_slug);
        write_file(&output_dir.join(&file_name), &mdx)?;
        keep.insert(file_name);
    }

    write_category_index(&output_dir, "Agents", 903, "Custom subagents")?;
    prune_stale(&output_dir, &keep)?;
    Ok(items.len())
}

// ---------------------------------------------------------------------------
// Public driver (called by lib::generate)
// ---------------------------------------------------------------------------

/// Walk `~/.claude` per `config` and emit the full MDX tree under
/// `config.docs_dir`.
pub(crate) fn run(config: &Config) -> Result<GenerateReport> {
    config.validate()?;

    let tree = walk::walk_claude_dir(&config.claude_dir, &config.project_root, &config.docs_dir)?;

    let docs_dir = &config.docs_dir;
    ensure_dir(docs_dir)?;

    let claude_md = generate_claudemd_docs(&tree.claude_mds, docs_dir)?;
    let commands = generate_commands_docs(&tree.commands, docs_dir)?;
    let skills = generate_skills_docs(&tree.skills, docs_dir)?;
    let agents = generate_agents_docs(&tree.agents, docs_dir)?;

    Ok(GenerateReport {
        claude_md,
        commands,
        skills,
        agents,
    })
}
