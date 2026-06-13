//! MDX generation — port of zudo-doc's `generate.ts`, adapted to Wave 1's
//! CCResDoc content contract (`app/CLAUDE.md`).
//!
//! Differences from upstream `generate.ts`, driven by the Wave 1 contract:
//! - CLAUDE.md per-item filenames follow the contract: the root file is
//!   `global.mdx` (slug `claude-md/global`) and nested files are
//!   `project-<dir-slug>.mdx`. Upstream uses `<dir-slug>.mdx` / `root.mdx`.
//! - Category index `sidebar_position` values follow the contract:
//!   CLAUDE.md=900, commands=901, skills=902, agents=903; the overview
//!   `claude/index.mdx` is 899 and carries `category_no_page: true`.
//!
//! Everything else (escaping, skill sub-pages, frontmatter shape) is ported
//! faithfully so output renders identically under zudo-doc.

use std::collections::HashMap;
use std::path::Path;

use crate::error::{GenerateError, Result};
use crate::escape::escape_for_mdx;
use crate::walk::{self, AgentItem, ClaudeMdItem, CommandItem, SkillItem};
use crate::Config;

/// Which resource families produced at least one page — drives the overview
/// `<CategoryNav>` slug list.
#[derive(Debug, Clone, Copy, Default)]
struct ResourceItemKinds {
    has_claudemd: bool,
    has_commands: bool,
    has_skills: bool,
    has_agents: bool,
}

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
    if dir.exists() {
        std::fs::remove_dir_all(dir).map_err(|e| GenerateError::Io {
            path: dir.to_owned(),
            source: e,
        })?;
    }
    Ok(())
}

fn write_file(path: &Path, contents: &str) -> Result<()> {
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

/// Write an unlisted sub-page MDX file (flat file with a custom nested slug).
fn write_unlisted_sub_page(
    output_path: &Path,
    title: &str,
    slug: &str,
    body: &str,
) -> Result<()> {
    let mdx = format!(
        "---\ntitle: \"{}\"\nslug: \"{}\"\nunlisted: true\ngenerated: true\n---\n\n{}\n",
        escape_title(title),
        slug,
        body,
    );
    write_file(output_path, &mdx)
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

fn generate_claudemd_docs(
    items: &[ClaudeMdItem],
    docs_dir: &Path,
) -> Result<usize> {
    let output_dir = docs_dir.join("claude-md");
    clean_dir(&output_dir)?;

    if items.is_empty() {
        return Ok(0);
    }

    ensure_dir(&output_dir)?;

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
        if let Some(prev) = emitted.get(&out_name) {
            return Err(GenerateError::SlugCollision(format!(
                "\"{out_name}\" is produced by both \"{prev}\" and \"{}\". Rename one of the directories.",
                item.rel_path
            )));
        }
        emitted.insert(out_name.clone(), item.rel_path.clone());

        // sidebar_position offsets from the category index (900). The first
        // CLAUDE.md gets 901-equivalent ordering relative to category, but to
        // match upstream's `index + 1` scheme while staying below the next
        // category (901), the per-item positions are 1-based within the
        // category; the category header itself carries 900.
        let position = index + 1;
        let mdx = format!(
            "---\ntitle: \"{}\"\ndescription: \"CLAUDE.md at {}\"\nsidebar_position: {}\nsidebar_label: \"{}\"\ngenerated: true\n---\n\n**Path:** `{}`\n\n{}\n",
            escape_title(&item.display_path),
            escape_title(&item.display_path),
            position,
            escape_title(&item.rel_path),
            item.rel_path,
            escape_for_mdx(item.raw_content.trim()),
        );
        write_file(&output_dir.join(format!("{out_name}.mdx")), &mdx)?;
    }

    write_category_index(&output_dir, "CLAUDE.md", 900, "Project-specific instructions")?;
    Ok(items.len())
}

// ---------------------------------------------------------------------------
// Commands generation
// ---------------------------------------------------------------------------

fn generate_commands_docs(items: &[CommandItem], docs_dir: &Path) -> Result<usize> {
    let output_dir = docs_dir.join("claude-commands");
    clean_dir(&output_dir)?;

    if items.is_empty() {
        return Ok(0);
    }

    ensure_dir(&output_dir)?;

    for item in items {
        assert_not_index_reserved(
            &item.name,
            "claude-resources: a command named \"index\" conflicts with the category metadata file. Rename the command file.",
        )?;
        let mdx = format!(
            "---\ntitle: \"{}\"\ndescription: \"{}\"\nsidebar_label: \"{}\"\ngenerated: true\n---\n\n{}\n",
            escape_title(&item.name),
            escape_title(&item.description),
            escape_title(&item.name),
            escape_for_mdx(item.raw_content.trim()),
        );
        write_file(&output_dir.join(format!("{}.mdx", item.name)), &mdx)?;
    }

    write_category_index(&output_dir, "Commands", 901, "Custom slash commands")?;
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
            let child_prefix = if child_is_last { "└── " } else { "├── " };
            lines.push(format!("{continuation}{child_prefix}{child}"));
        }
        idx += 1;
    }

    lines.join("\n")
}

fn generate_skills_docs(items: &[SkillItem], docs_dir: &Path) -> Result<usize> {
    let output_dir = docs_dir.join("claude-skills");
    clean_dir(&output_dir)?;

    if items.is_empty() {
        return Ok(0);
    }

    ensure_dir(&output_dir)?;

    for skill in items {
        assert_not_index_reserved(
            &skill.dir,
            "claude-resources: a skill directory named \"index\" conflicts with the category metadata file. Rename the skill directory.",
        )?;

        let script_md: Vec<&_> = skill
            .script_files
            .iter()
            .filter(|f| f.is_markdown)
            .collect();
        let asset_md: Vec<&_> = skill
            .asset_files
            .iter()
            .filter(|f| f.is_markdown)
            .collect();

        // File tree (lists ALL sub-files, markdown and binary).
        let mut sub_dirs: Vec<(&str, Vec<String>)> = Vec::new();
        if !skill.script_files.is_empty() {
            sub_dirs.push((
                "scripts",
                skill.script_files.iter().map(|f| f.filename.clone()).collect(),
            ));
        }
        if !skill.references.is_empty() {
            sub_dirs.push((
                "references",
                skill.references.iter().map(|r| format!("{}.md", r.name)).collect(),
            ));
        }
        if !skill.asset_files.is_empty() {
            sub_dirs.push((
                "assets",
                skill.asset_files.iter().map(|f| f.filename.clone()).collect(),
            ));
        }

        let mut file_structure_section = String::new();
        if !sub_dirs.is_empty() {
            let tree = format!("```\n{}\n```", render_skill_file_tree(&skill.dir, &sub_dirs));

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
        write_file(&output_dir.join(format!("{}.mdx", skill.dir)), &mdx)?;

        // Unlisted sub-pages.
        let skill_slug_base = format!("claude-skills/{}", skill.dir);

        for r in &skill.references {
            write_unlisted_sub_page(
                &output_dir.join(format!("{}--ref-{}.mdx", skill.dir, r.name)),
                &r.title,
                &format!("{skill_slug_base}/ref-{}", r.name),
                &escape_for_mdx(r.raw_content.trim()),
            )?;
        }
        for f in &script_md {
            let slug = f.filename.trim_end_matches(".md");
            let raw = f.raw_content.as_deref().unwrap_or("");
            let title = f.title.clone().unwrap_or_else(|| slug.to_owned());
            write_unlisted_sub_page(
                &output_dir.join(format!("{}--script-{}.mdx", skill.dir, slug)),
                &title,
                &format!("{skill_slug_base}/script-{slug}"),
                &escape_for_mdx(raw.trim()),
            )?;
        }
        for f in &asset_md {
            let slug = f.filename.trim_end_matches(".md");
            let raw = f.raw_content.as_deref().unwrap_or("");
            let title = f.title.clone().unwrap_or_else(|| slug.to_owned());
            write_unlisted_sub_page(
                &output_dir.join(format!("{}--asset-{}.mdx", skill.dir, slug)),
                &title,
                &format!("{skill_slug_base}/asset-{slug}"),
                &escape_for_mdx(raw.trim()),
            )?;
        }
    }

    write_category_index(&output_dir, "Skills", 902, "Skill packages")?;
    Ok(items.len())
}

/// Rewrite `](references/x.md)` → `](./ref-x)`, `scripts/` → `./script-`,
/// `assets/` → `./asset-`, matching the JS regex replacements.
fn rewrite_skill_links(body: &str) -> String {
    let mut out = body.to_owned();
    out = rewrite_link_kind(&out, "references/", "ref-");
    out = rewrite_link_kind(&out, "scripts/", "script-");
    out = rewrite_link_kind(&out, "assets/", "asset-");
    out
}

/// Replace `](<dir><name>.md)` with `](./<prefix><name>)`. `<name>` matches the
/// JS `[^)]+` (any run of non-`)` chars).
fn rewrite_link_kind(input: &str, dir: &str, prefix: &str) -> String {
    let needle_open = format!("]({dir}");
    let mut out = String::with_capacity(input.len());
    let mut rest = input;
    while let Some(pos) = rest.find(&needle_open) {
        out.push_str(&rest[..pos]);
        let after = &rest[pos + needle_open.len()..];
        // name = up to ".md)" — find the first ")" and require the chars before
        // it end with ".md".
        if let Some(close) = after.find(')') {
            let inner = &after[..close];
            if let Some(name) = inner.strip_suffix(".md") {
                out.push_str(&format!("](./{prefix}{name})"));
                rest = &after[close + 1..];
                continue;
            }
        }
        // Not a match — emit the literal needle and continue past it.
        out.push_str(&needle_open);
        rest = after;
    }
    out.push_str(rest);
    out
}

// ---------------------------------------------------------------------------
// Agents generation
// ---------------------------------------------------------------------------

fn generate_agents_docs(items: &[AgentItem], docs_dir: &Path) -> Result<usize> {
    let output_dir = docs_dir.join("claude-agents");
    clean_dir(&output_dir)?;

    if items.is_empty() {
        return Ok(0);
    }

    ensure_dir(&output_dir)?;

    for agent in items {
        assert_not_index_reserved(
            &agent.file_slug,
            "claude-resources: an agent named \"index\" conflicts with the category metadata file. Rename the agent file.",
        )?;
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
        write_file(&output_dir.join(format!("{}.mdx", agent.file_slug)), &mdx)?;
    }

    write_category_index(&output_dir, "Agents", 903, "Custom subagents")?;
    Ok(items.len())
}

// ---------------------------------------------------------------------------
// Overview index
// ---------------------------------------------------------------------------

fn generate_overview_index(docs_dir: &Path, kinds: ResourceItemKinds) -> Result<()> {
    let output_dir = docs_dir.join("claude");
    clean_dir(&output_dir)?;
    ensure_dir(&output_dir)?;

    // Build the category slug list in the contract's order: CLAUDE.md,
    // commands, skills, agents (only those that were generated).
    let mut slugs: Vec<&str> = Vec::new();
    if kinds.has_claudemd {
        slugs.push("claude-md");
    }
    if kinds.has_commands {
        slugs.push("claude-commands");
    }
    if kinds.has_skills {
        slugs.push("claude-skills");
    }
    if kinds.has_agents {
        slugs.push("claude-agents");
    }

    // JSON-encode the slug list for the JSX attribute. Slugs are fixed ASCII
    // identifiers (no quotes/backslashes), so a plain join is exact.
    let categories_attr = format!(
        "[{}]",
        slugs
            .iter()
            .map(|s| format!("\"{s}\""))
            .collect::<Vec<_>>()
            .join(",")
    );

    // Contract: overview carries category_no_page: true at position 899.
    let index = format!(
        "---\ntitle: \"Claude Resources\"\ndescription: \"Claude Code configuration reference.\"\nsidebar_position: 899\ncategory_no_page: true\ngenerated: true\n---\n\nClaude Code configuration reference.\n\n## Resources\n\n<CategoryNav categories={{{categories_attr}}} />\n",
    );
    write_file(&output_dir.join("index.mdx"), &index)
}

// ---------------------------------------------------------------------------
// Public driver (called by lib::generate)
// ---------------------------------------------------------------------------

/// Walk `~/.claude` per `config` and emit the full MDX tree under
/// `config.docs_dir`.
pub(crate) fn run(config: &Config) -> Result<GenerateReport> {
    config.validate()?;

    let tree =
        walk::walk_claude_dir(&config.claude_dir, &config.project_root, &config.docs_dir)?;

    let docs_dir = &config.docs_dir;
    ensure_dir(docs_dir)?;

    let claude_md = generate_claudemd_docs(&tree.claude_mds, docs_dir)?;
    let commands = generate_commands_docs(&tree.commands, docs_dir)?;
    let skills = generate_skills_docs(&tree.skills, docs_dir)?;
    let agents = generate_agents_docs(&tree.agents, docs_dir)?;

    generate_overview_index(
        docs_dir,
        ResourceItemKinds {
            has_claudemd: claude_md > 0,
            has_commands: commands > 0,
            has_skills: skills > 0,
            has_agents: agents > 0,
        },
    )?;

    Ok(GenerateReport {
        claude_md,
        commands,
        skills,
        agents,
    })
}
