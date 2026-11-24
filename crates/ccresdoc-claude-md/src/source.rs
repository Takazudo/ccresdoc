//! Node-free generation boundary for one configured source instance.
//! The caller builds every enabled source in a private candidate tree and owns
//! publication. No source file or live output is removed here.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use globset::{Glob, GlobSet, GlobSetBuilder};
use notify::{RecursiveMode, Watcher};
use notify_debouncer_full::{new_debouncer, DebounceEventResult, Debouncer, FileIdMap};
use walkdir::WalkDir;

use crate::escape::escape_untrusted_mdx;
use crate::{generate, generate_codex, CodexConfig, Config, GenerateError, Result};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SourceFormat {
    Markdown,
    Code,
    Skill,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SourceRule {
    ClaudeCode,
    Codex,
    Custom {
        format: SourceFormat,
        include: Vec<String>,
        exclude: Vec<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceSpec {
    pub id: String,
    pub label: String,
    pub root: PathBuf,
    /// Always explicit. The upstream default points at the documentation
    /// project, which would silently broaden discovery in this host.
    pub scan_root: PathBuf,
    pub mount: String,
    pub rule: SourceRule,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceReport {
    pub source_id: String,
    pub mount: String,
    pub pages: usize,
    pub mappings: BTreeMap<String, usize>,
}

pub fn generate_source(spec: &SourceSpec, docs_dir: &Path, marker: &str) -> Result<SourceReport> {
    validate(spec, docs_dir)?;
    let target = docs_dir.join(&spec.mount);
    if target.exists() {
        return Err(GenerateError::SlugCollision(format!(
            "mount '{}' already exists",
            spec.mount
        )));
    }
    let mut mapping_counts = BTreeMap::new();
    match &spec.rule {
        SourceRule::ClaudeCode | SourceRule::Codex => {
            generate_builtin(spec, docs_dir, &target, &mut mapping_counts)?;
        }
        SourceRule::Custom {
            format,
            include,
            exclude,
        } => {
            generate_custom(spec, &target, format, include, exclude, &mut mapping_counts)?;
        }
    }
    let pages = mapping_counts.values().sum();
    if !target.join("index.mdx").exists() {
        let cards = format!(
            "\n<CategoryNav category={} />\n",
            serde_json::to_string(&spec.mount).expect("mount serializes")
        );
        let index = format!("---\ntitle: \"{}\"\ngenerated: true\ndoc_source: \"{}\"\n---\n\n<div data-ccresdoc-source=\"{}\" data-ccresdoc-generation=\"{}\">\n\n{}\n{cards}</div>\n", yaml_scalar(&spec.label), yaml_scalar(&spec.id), html_attr(&spec.id), html_attr(marker), escape_untrusted_mdx(&spec.label));
        write(&target.join("index.mdx"), &index)?;
    } else {
        let source = std::fs::read_to_string(target.join("index.mdx"))
            .map_err(|error| io(&target.join("index.mdx"), error))?;
        let source = format!(
            "{}\n<div data-ccresdoc-source=\"{}\" data-ccresdoc-generation=\"{}\" />\n",
            source,
            html_attr(&spec.id),
            html_attr(marker)
        );
        write(&target.join("index.mdx"), &source)?;
    }
    Ok(SourceReport {
        source_id: spec.id.clone(),
        mount: spec.mount.clone(),
        pages,
        mappings: mapping_counts,
    })
}

fn validate(spec: &SourceSpec, docs_dir: &Path) -> Result<()> {
    if spec.id.is_empty()
        || spec.id.len() > 80
        || !spec.id.starts_with(|c: char| c.is_ascii_alphanumeric())
        || !spec
            .id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-'))
        || spec.label.trim().is_empty()
        || spec.mount.is_empty()
        || spec.mount == "ccresdoc"
        || !spec
            .mount
            .starts_with(|c: char| c.is_ascii_lowercase() || c.is_ascii_digit())
        || !spec
            .mount
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '_' | '-' | '.'))
    {
        return Err(GenerateError::InvalidConfig(
            "invalid source identity, label or mount".into(),
        ));
    }
    for (label, path) in [
        ("root", spec.root.as_path()),
        ("scan_root", spec.scan_root.as_path()),
        ("docs_dir", docs_dir),
    ] {
        if !path.is_absolute() {
            return Err(GenerateError::InvalidConfig(format!(
                "{label} must be absolute"
            )));
        }
    }
    crate::validate_no_overlap("root", &spec.root, docs_dir)?;
    crate::validate_no_overlap("scan_root", &spec.scan_root, docs_dir)?;
    if !spec.root.is_dir() {
        return Err(GenerateError::InvalidConfig(format!(
            "source root is unavailable: {}",
            spec.root.display()
        )));
    }
    if matches!(spec.rule, SourceRule::ClaudeCode | SourceRule::Codex) && !spec.scan_root.is_dir() {
        return Err(GenerateError::InvalidConfig(format!(
            "instruction scan root is unavailable: {}",
            spec.scan_root.display()
        )));
    }
    Ok(())
}

fn generate_builtin(
    spec: &SourceSpec,
    docs_dir: &Path,
    target: &Path,
    counts: &mut BTreeMap<String, usize>,
) -> Result<()> {
    let scratch = docs_dir.join(format!(".ccresdoc-generate-{}", spec.id));
    std::fs::create_dir_all(docs_dir).map_err(|error| io(docs_dir, error))?;
    std::fs::create_dir(&scratch).map_err(|error| io(&scratch, error))?;
    let mappings: &[(&str, &str)] = match spec.rule {
        SourceRule::ClaudeCode => {
            generate(&Config {
                claude_dir: spec.root.clone(),
                project_root: spec.scan_root.clone(),
                docs_dir: scratch.clone(),
            })?;
            &[("claude-skills", "skills")]
        }
        SourceRule::Codex => {
            generate_codex(&CodexConfig {
                codex_dir: spec.root.clone(),
                project_root: spec.scan_root.clone(),
                docs_dir: scratch.clone(),
            })?;
            &[
                ("codex-config", "config"),
                ("codex-agents", "agents"),
                ("codex-hooks", "hooks"),
                ("codex-rules", "rules"),
                ("codex-skills", "skills"),
            ]
        }
        SourceRule::Custom { .. } => unreachable!(),
    };
    std::fs::create_dir_all(target).map_err(|error| io(target, error))?;
    for (old, new) in mappings {
        let old_dir = scratch.join(old);
        if !old_dir.exists() {
            continue;
        }
        let new_dir = target.join(new);
        std::fs::rename(&old_dir, &new_dir).map_err(|error| io(&old_dir, error))?;
        let mut page_count = 0;
        for entry in WalkDir::new(&new_dir).follow_links(false) {
            let entry = entry.map_err(|error| GenerateError::InvalidConfig(error.to_string()))?;
            if !entry.file_type().is_file()
                || entry.path().extension().is_none_or(|ext| ext != "mdx")
            {
                continue;
            }
            let source =
                std::fs::read_to_string(entry.path()).map_err(|error| io(entry.path(), error))?;
            let is_category = entry.path() == new_dir.join("index.mdx")
                && source.contains("category_no_page: true");
            if is_category {
                let category = format!("{}/{}", spec.mount, new);
                let landing = format!("---\ntitle: \"{}\"\ngenerated: true\ndoc_source: \"{}\"\n---\n\n<CategoryNav category={} />\n", yaml_scalar(new), yaml_scalar(&spec.id), serde_json::to_string(&category).expect("category serializes"));
                write(entry.path(), &landing)?;
                continue;
            }
            let relative = entry
                .path()
                .strip_prefix(&new_dir)
                .expect("walk under new dir");
            let (mapping_id, rel) = builtin_provenance(spec, new, relative)?;
            let provenance = if is_category {
                format!("doc_source: \"{}\"\n", yaml_scalar(&spec.id))
            } else {
                page_count += 1;
                format!(
                    "doc_source: \"{}\"\ndoc_source_mapping: \"{}\"\ndoc_source_path: \"{}\"\n",
                    yaml_scalar(&spec.id),
                    mapping_id,
                    yaml_scalar(&rel)
                )
            };
            let annotated = source.replacen("---\n", &format!("---\n{provenance}"), 1);
            write(entry.path(), &annotated)?;
        }
        counts.insert((*new).into(), page_count);
    }
    match spec.rule {
        SourceRule::ClaudeCode => {
            render_preset_markdown_mapping(
                spec,
                &spec.scan_root,
                target,
                "instructions",
                MarkdownSelection::ClaudeInstructions,
                counts,
            )?;
            render_preset_markdown_mapping(
                spec,
                &spec.root.join("commands"),
                target,
                "commands",
                MarkdownSelection::AllMarkdown,
                counts,
            )?;
            render_preset_markdown_mapping(
                spec,
                &spec.root.join("agents"),
                target,
                "agents",
                MarkdownSelection::AllMarkdown,
                counts,
            )?;
        }
        SourceRule::Codex => {
            render_preset_markdown_mapping(
                spec,
                &spec.scan_root,
                target,
                "instructions",
                MarkdownSelection::CodexInstructions,
                counts,
            )?;
        }
        SourceRule::Custom { .. } => unreachable!(),
    }
    std::fs::remove_dir_all(&scratch).map_err(|error| io(&scratch, error))?;
    Ok(())
}

fn builtin_provenance(spec: &SourceSpec, mapping: &str, output: &Path) -> Result<(String, String)> {
    let rel = output.to_string_lossy().replace('\\', "/");
    let stem = rel.strip_suffix(".mdx").unwrap_or(&rel);
    let result = match mapping {
        "skills" if stem.ends_with("/index") => {
            let relative = format!("{}/SKILL.md", stem.trim_end_matches("/index"));
            let team = spec.scan_root.join(".agents/skills").join(&relative);
            if matches!(spec.rule, SourceRule::Codex)
                && !spec.root.join("skills").join(&relative).exists()
                && team.exists()
            {
                ("team-skills".into(), relative)
            } else {
                ("skills".into(), relative)
            }
        }
        "skills" => skill_subpage_provenance(spec, stem)?,
        "agents" => ("agents".into(), format!("{stem}.toml")),
        "rules" => ("rules".into(), format!("{stem}.rules")),
        "config" => {
            let mut found = None;
            if let Ok(entries) = std::fs::read_dir(&spec.root) {
                for entry in entries.flatten() {
                    let name = entry.file_name().to_string_lossy().into_owned();
                    if (name.ends_with(".toml") || name == "config.toml.example")
                        && route_segment(&name) == stem
                    {
                        found = Some(name);
                        break;
                    }
                }
            }
            (
                "config".into(),
                found.ok_or_else(|| {
                    GenerateError::InvalidConfig(format!(
                        "cannot resolve config provenance for {rel}"
                    ))
                })?,
            )
        }
        "hooks" if stem == "hooks-json" => ("hooks-config".into(), "hooks.json".into()),
        "hooks" => {
            let root = spec.root.join("hooks");
            let mut found = None;
            if root.exists() {
                for entry in WalkDir::new(&root)
                    .follow_links(false)
                    .into_iter()
                    .flatten()
                {
                    if !entry.file_type().is_file() {
                        continue;
                    }
                    let relative = entry
                        .path()
                        .strip_prefix(&root)
                        .expect("hook under root")
                        .to_string_lossy()
                        .replace('\\', "/");
                    if route_segment(&relative.replace('/', "-")) == stem {
                        found = Some(relative);
                        break;
                    }
                }
            }
            (
                "hooks".into(),
                found.ok_or_else(|| {
                    GenerateError::InvalidConfig(format!(
                        "cannot resolve hook provenance for {rel}"
                    ))
                })?,
            )
        }
        _ => {
            return Err(GenerateError::InvalidConfig(format!(
                "unknown builtin mapping {mapping}"
            )))
        }
    };
    Ok(result)
}

fn skill_subpage_provenance(spec: &SourceSpec, output_stem: &str) -> Result<(String, String)> {
    let (skill_slug, page_slug) = output_stem.split_once('/').ok_or_else(|| {
        GenerateError::InvalidConfig(format!("invalid skill output: {output_stem}"))
    })?;
    let roots = [
        ("skills", spec.root.join("skills")),
        ("team-skills", spec.scan_root.join(".agents/skills")),
    ];
    for (mapping, root) in roots {
        if mapping == "team-skills" && !matches!(spec.rule, SourceRule::Codex) {
            continue;
        }
        let Ok(entries) = std::fs::read_dir(&root) else {
            continue;
        };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if route_segment(&name) != skill_slug {
                continue;
            }
            let package = entry.path().canonicalize().unwrap_or_else(|_| entry.path());
            if !package.is_dir() {
                continue;
            }
            for file in WalkDir::new(&package)
                .follow_links(false)
                .into_iter()
                .flatten()
            {
                if !file.file_type().is_file()
                    || !matches!(
                        file.path().extension().and_then(|ext| ext.to_str()),
                        Some("md" | "mdx")
                    )
                {
                    continue;
                }
                let inner = file
                    .path()
                    .strip_prefix(&package)
                    .expect("skill file under package");
                if inner == Path::new("SKILL.md") {
                    continue;
                }
                let inner_text = inner.to_string_lossy().replace('\\', "/");
                let mut parts = inner_text.split('/');
                let family = match parts.next() {
                    Some("references") => "ref",
                    Some("scripts") => "script",
                    Some("assets") => "asset",
                    _ => "",
                };
                let basename = inner
                    .file_stem()
                    .and_then(|stem| stem.to_str())
                    .unwrap_or("");
                let slug = if family.is_empty() {
                    route_segment(basename)
                } else {
                    format!("{family}-{}", route_segment(basename))
                };
                if slug == page_slug {
                    return Ok((mapping.into(), format!("{name}/{inner_text}")));
                }
            }
        }
    }
    Err(GenerateError::InvalidConfig(format!(
        "cannot resolve skill subpage provenance for {output_stem}"
    )))
}

#[derive(Clone, Copy)]
enum MarkdownSelection {
    ClaudeInstructions,
    CodexInstructions,
    AllMarkdown,
}

fn render_preset_markdown_mapping(
    spec: &SourceSpec,
    root: &Path,
    target: &Path,
    mapping: &str,
    selection: MarkdownSelection,
    counts: &mut BTreeMap<String, usize>,
) -> Result<()> {
    if !root.exists() {
        counts.insert(mapping.into(), 0);
        return Ok(());
    }
    let section = target.join(mapping);
    let mut selected = BTreeSet::new();
    let mut inputs = Vec::new();
    let mut route_map = BTreeMap::new();
    for entry in WalkDir::new(root)
        .follow_links(false)
        .into_iter()
        .filter_entry(|entry| {
            if entry.depth() == 0 || !entry.file_type().is_dir() {
                return true;
            }
            let name = entry.file_name().to_string_lossy();
            !name.starts_with('.')
                && !matches!(
                    name.as_ref(),
                    "node_modules"
                        | "dist"
                        | "out"
                        | "public"
                        | "worktrees"
                        | "__inbox"
                        | "test-results"
                )
        })
    {
        let entry = entry.map_err(|error| GenerateError::InvalidConfig(error.to_string()))?;
        if !entry.file_type().is_file() {
            continue;
        }
        let filename = entry.file_name().to_string_lossy();
        let accepted = match selection {
            MarkdownSelection::ClaudeInstructions => filename == "CLAUDE.md",
            MarkdownSelection::CodexInstructions => {
                filename == "AGENTS.md" || filename == "AGENTS.override.md"
            }
            MarkdownSelection::AllMarkdown => filename.ends_with(".md"),
        };
        if !accepted {
            continue;
        }
        let relative = entry.path().strip_prefix(root).expect("walk under root");
        let rel = relative.to_string_lossy().replace('\\', "/");
        let mut route = PathBuf::new();
        for component in relative.parent().into_iter().flat_map(Path::components) {
            let slug = route_segment(&component.as_os_str().to_string_lossy());
            if slug.is_empty() {
                return Err(GenerateError::InvalidConfig(format!(
                    "unroutable directory: {rel}"
                )));
            }
            route.push(slug);
        }
        let landing = matches!(
            selection,
            MarkdownSelection::ClaudeInstructions | MarkdownSelection::CodexInstructions
        ) && filename != "AGENTS.override.md";
        if landing {
            route.push("index.mdx");
        } else {
            route.push(format!(
                "{}.mdx",
                route_segment(filename.trim_end_matches(".md"))
            ));
        }
        if !selected.insert(route.clone()) {
            return Err(GenerateError::SlugCollision(format!(
                "{mapping}/{}",
                route.display()
            )));
        }
        route_map.insert(rel.clone(), route.clone());
        inputs.push((entry.path().to_path_buf(), rel, route));
    }
    for (source_path, rel, route) in inputs {
        let content =
            std::fs::read_to_string(&source_path).map_err(|error| io(&source_path, error))?;
        let filename = source_path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or(&rel);
        let title = if matches!(
            selection,
            MarkdownSelection::ClaudeInstructions | MarkdownSelection::CodexInstructions
        ) {
            rel.as_str()
        } else {
            content
                .lines()
                .find_map(|line| line.strip_prefix("# ").map(str::trim))
                .filter(|line| !line.is_empty())
                .unwrap_or(filename.as_ref())
        };
        let body = escape_untrusted_mdx(&rewrite_mapped_links(&content, &rel, &route_map));
        let mdx = format!("---\ntitle: \"{}\"\ngenerated: true\ndoc_source: \"{}\"\ndoc_source_mapping: \"{}\"\ndoc_source_path: \"{}\"\n---\n\n{}\n", yaml_scalar(title), yaml_scalar(&spec.id), mapping, yaml_scalar(&rel), body);
        write(&section.join(route), &mdx)?;
    }
    let mut nested_dirs = BTreeSet::new();
    for route in &selected {
        let mut current = route.parent();
        while let Some(dir) = current {
            if dir.as_os_str().is_empty() {
                break;
            }
            nested_dirs.insert(dir.to_path_buf());
            current = dir.parent();
        }
    }
    for dir in nested_dirs {
        let index_path = section.join(&dir).join("index.mdx");
        if index_path.exists() {
            continue;
        }
        let title = dir.file_name().unwrap_or_default().to_string_lossy();
        let category = format!(
            "{}/{}/{}",
            spec.mount,
            mapping,
            dir.to_string_lossy().replace('\\', "/")
        );
        let index = format!("---\ntitle: \"{}\"\ngenerated: true\ndoc_source: \"{}\"\n---\n\n<CategoryNav category={} />\n", yaml_scalar(&title), yaml_scalar(&spec.id), serde_json::to_string(&category).expect("category serializes"));
        write(&index_path, &index)?;
    }
    if !selected.is_empty() && !section.join("index.mdx").exists() {
        let category = format!("{}/{}", spec.mount, mapping);
        let index = format!("---\ntitle: \"{}\"\ngenerated: true\ndoc_source: \"{}\"\n---\n\n<CategoryNav category={} />\n", yaml_scalar(mapping), yaml_scalar(&spec.id), serde_json::to_string(&category).expect("category serializes"));
        write(&section.join("index.mdx"), &index)?;
    }
    counts.insert(mapping.into(), selected.len());
    Ok(())
}

fn route_segment(value: &str) -> String {
    let mut result = String::new();
    for character in value.chars() {
        if character.is_alphanumeric() || matches!(character, '_' | '-') {
            result.extend(character.to_lowercase());
        } else if !result.ends_with('-') {
            result.push('-');
        }
    }
    result.trim_matches('-').to_owned()
}

fn generate_custom(
    spec: &SourceSpec,
    target: &Path,
    format: &SourceFormat,
    include: &[String],
    exclude: &[String],
    counts: &mut BTreeMap<String, usize>,
) -> Result<()> {
    let includes = globs(include)?;
    let excludes = globs(exclude)?;
    std::fs::create_dir_all(target).map_err(|error| io(target, error))?;
    let mut selected = BTreeSet::new();
    let mut inputs = Vec::new();
    let mut route_map = BTreeMap::new();
    for entry in WalkDir::new(&spec.root).follow_links(false) {
        let entry = entry.map_err(|error| GenerateError::InvalidConfig(error.to_string()))?;
        if !entry.file_type().is_file() {
            continue;
        }
        let relative = entry
            .path()
            .strip_prefix(&spec.root)
            .expect("walk under source");
        let relative = relative.to_string_lossy().replace('\\', "/");
        if !includes.is_match(&relative) || excludes.is_match(&relative) {
            continue;
        }
        let route = route_for(&relative, format)?;
        if !selected.insert(route.clone()) {
            return Err(GenerateError::SlugCollision(format!(
                "{}: {}",
                spec.id,
                route.display()
            )));
        }
        route_map.insert(relative.clone(), route.clone());
        inputs.push((entry.path().to_path_buf(), relative, route));
    }
    for (source_path, relative, route) in inputs {
        let destination = target.join(&route);
        let content =
            std::fs::read_to_string(&source_path).map_err(|error| io(&source_path, error))?;
        let filename = relative.rsplit('/').next().unwrap_or(&relative);
        let (frontmatter, content_body) = if matches!(format, SourceFormat::Code) {
            (None, content.as_str())
        } else {
            source_frontmatter(&content)
        };
        let skill_structure =
            if matches!(format, SourceFormat::Skill) && filename.eq_ignore_ascii_case("SKILL.md") {
                render_skill_package(spec, target, &source_path, &relative, &route, &mut selected)?
            } else {
                String::new()
            };
        let fallback_title =
            if matches!(format, SourceFormat::Skill) && filename.eq_ignore_ascii_case("SKILL.md") {
                source_path
                    .parent()
                    .and_then(Path::file_name)
                    .and_then(|name| name.to_str())
                    .unwrap_or(filename)
            } else {
                filename
            };
        let title = frontmatter
            .as_ref()
            .and_then(|data| data.get("title").or_else(|| data.get("name")))
            .and_then(serde_yaml::Value::as_str)
            .filter(|title| !title.is_empty())
            .or_else(|| {
                content_body
                    .lines()
                    .find_map(|line| line.strip_prefix("# ").map(str::trim))
                    .filter(|line| !line.is_empty())
            })
            .unwrap_or(fallback_title);
        let description = frontmatter
            .as_ref()
            .and_then(|data| data.get("description"))
            .and_then(serde_yaml::Value::as_str)
            .unwrap_or("");
        let body = match format {
            SourceFormat::Markdown | SourceFormat::Skill => format!(
                "{}{}",
                skill_structure,
                escape_untrusted_mdx(&rewrite_mapped_links(content_body, &relative, &route_map))
            ),
            SourceFormat::Code => {
                let fence_len = max_backtick_run(&content).max(2) + 1;
                let fence = "`".repeat(fence_len);
                format!("## Source\n\n{fence}\n{content}\n{fence}")
            }
        };
        let description_field = if description.is_empty() {
            String::new()
        } else {
            format!("description: \"{}\"\n", yaml_scalar(description))
        };
        let mdx = format!("---\ntitle: \"{}\"\n{}generated: true\ndoc_source: \"{}\"\ndoc_source_mapping: \"files\"\ndoc_source_path: \"{}\"\n---\n\n{}\n", yaml_scalar(title), description_field, yaml_scalar(&spec.id), yaml_scalar(&relative), body);
        write(&destination, &mdx)?;
    }
    let mut nested_dirs = BTreeSet::new();
    for route in &selected {
        let mut current = route.parent();
        while let Some(dir) = current {
            if dir.as_os_str().is_empty() {
                break;
            }
            nested_dirs.insert(dir.to_path_buf());
            current = dir.parent();
        }
    }
    for dir in nested_dirs {
        let path = target.join(&dir).join("index.mdx");
        if path.exists() {
            continue;
        }
        let title = dir.file_name().unwrap_or_default().to_string_lossy();
        let category = format!(
            "{}/{}",
            spec.mount,
            dir.to_string_lossy().replace('\\', "/")
        );
        let index = format!("---\ntitle: \"{}\"\ngenerated: true\ndoc_source: \"{}\"\n---\n\n<CategoryNav category={} />\n", yaml_scalar(&title), yaml_scalar(&spec.id), serde_json::to_string(&category).expect("category serializes"));
        write(&path, &index)?;
    }
    counts.insert("files".into(), selected.len());
    Ok(())
}

fn max_backtick_run(content: &str) -> usize {
    let mut longest = 0;
    let mut current = 0;
    for character in content.chars() {
        if character == '`' {
            current += 1;
            longest = longest.max(current);
        } else {
            current = 0;
        }
    }
    longest
}

fn render_skill_package(
    spec: &SourceSpec,
    target: &Path,
    skill_file: &Path,
    skill_rel: &str,
    skill_route: &Path,
    selected: &mut BTreeSet<PathBuf>,
) -> Result<String> {
    let package = skill_file
        .parent()
        .ok_or_else(|| GenerateError::InvalidConfig("skill has no package directory".into()))?;
    let package_route = skill_route.parent().unwrap_or_else(|| Path::new(""));
    let package_name = package
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("skill");
    let package_rel = Path::new(skill_rel)
        .parent()
        .unwrap_or_else(|| Path::new(""));
    let mut names = Vec::new();
    let mut links = Vec::new();
    for entry in WalkDir::new(package).follow_links(false) {
        let entry = entry.map_err(|error| GenerateError::InvalidConfig(error.to_string()))?;
        if !entry.file_type().is_file() || entry.path() == skill_file {
            continue;
        }
        let inner = entry
            .path()
            .strip_prefix(package)
            .expect("skill walk under package");
        let inner_text = inner.to_string_lossy().replace('\\', "/");
        names.push(inner_text.clone());
        if !matches!(
            entry.path().extension().and_then(|ext| ext.to_str()),
            Some("md" | "mdx")
        ) {
            continue;
        }
        let stem = inner_text
            .trim_end_matches(".mdx")
            .trim_end_matches(".md")
            .replace('/', "-");
        let slug = route_segment(&stem);
        if slug.is_empty() {
            continue;
        }
        let route = package_route.join(format!("{slug}.mdx"));
        if !selected.insert(route.clone()) {
            return Err(GenerateError::SlugCollision(format!(
                "skill page {}",
                route.display()
            )));
        }
        let rel = package_rel.join(inner).to_string_lossy().replace('\\', "/");
        let content =
            std::fs::read_to_string(entry.path()).map_err(|error| io(entry.path(), error))?;
        let title = inner
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or(&inner_text);
        let page = format!("---\ntitle: \"{}\"\nunlisted: true\ngenerated: true\ndoc_source: \"{}\"\ndoc_source_mapping: \"files\"\ndoc_source_path: \"{}\"\n---\n\n{}\n", yaml_scalar(title), yaml_scalar(&spec.id), yaml_scalar(&rel), escape_untrusted_mdx(&content));
        write(&target.join(&route), &page)?;
        links.push(format!("- [{inner_text}](./{slug}.mdx)"));
    }
    names.sort();
    links.sort();
    let mut out = String::new();
    if !names.is_empty() {
        out.push_str(&format!(
            "## File Structure\n\n```\n{package_name}/\n├── SKILL.md\n"
        ));
        for (index, name) in names.iter().enumerate() {
            out.push_str(&format!(
                "{} {name}\n",
                if index + 1 == names.len() {
                    "└──"
                } else {
                    "├──"
                }
            ));
        }
        out.push_str("```\n\n");
        out.push_str(&links.join("\n"));
        out.push_str("\n\n");
    }
    let metadata = package.join("agents/openai.yaml");
    if let Ok(yaml) = std::fs::read_to_string(metadata) {
        if let Ok(data) = serde_yaml::from_str::<serde_yaml::Value>(&yaml) {
            if let Some(name) = data
                .get("interface")
                .and_then(|v| v.get("display_name"))
                .and_then(serde_yaml::Value::as_str)
            {
                out.push_str(&format!(
                    "**Display name:** {}\n",
                    escape_untrusted_mdx(name)
                ));
            }
            if let Some(description) = data
                .get("interface")
                .and_then(|v| v.get("short_description"))
                .and_then(serde_yaml::Value::as_str)
            {
                out.push_str(&format!(
                    "**Short description:** {}\n",
                    escape_untrusted_mdx(description)
                ));
            }
            if data
                .get("policy")
                .and_then(|v| v.get("allow_implicit_invocation"))
                .and_then(serde_yaml::Value::as_bool)
                == Some(false)
            {
                out.push_str(&format!(
                    "**Invocation:** explicit only (`${package_name}`)\n"
                ));
            }
            if !out.is_empty() {
                out.push('\n');
            }
        }
    }
    Ok(out)
}

fn route_for(relative: &str, format: &SourceFormat) -> Result<PathBuf> {
    let path = Path::new(relative);
    if path.components().any(|part| {
        matches!(
            part,
            std::path::Component::ParentDir | std::path::Component::RootDir
        )
    }) {
        return Err(GenerateError::InvalidConfig("unsafe relative path".into()));
    }
    let mut route = PathBuf::new();
    for component in path.parent().into_iter().flat_map(Path::components) {
        let slug = route_segment(&component.as_os_str().to_string_lossy());
        if slug.is_empty() {
            return Err(GenerateError::InvalidConfig(format!(
                "unroutable directory in {relative}"
            )));
        }
        route.push(slug);
    }
    let filename = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| GenerateError::InvalidConfig("non-UTF-8 source filename".into()))?;
    let lower = filename.to_ascii_lowercase();
    let landing = match format {
        SourceFormat::Code => false,
        SourceFormat::Markdown => matches!(lower.as_str(), "index.md" | "index.mdx" | "readme.md"),
        SourceFormat::Skill => {
            lower == "skill.md" || matches!(lower.as_str(), "index.md" | "index.mdx" | "readme.md")
        }
    };
    if landing {
        route.push("index.mdx");
    } else {
        let stem = if matches!(format, SourceFormat::Code) {
            filename
        } else {
            filename
                .strip_suffix(".md")
                .or_else(|| filename.strip_suffix(".mdx"))
                .unwrap_or(filename)
        };
        let slug = route_segment(stem);
        if slug.is_empty() {
            return Err(GenerateError::InvalidConfig(format!(
                "unroutable source filename: {relative}"
            )));
        }
        route.push(format!("{slug}.mdx"));
    }
    Ok(route)
}

fn rewrite_mapped_links(
    content: &str,
    current_rel: &str,
    routes: &BTreeMap<String, PathBuf>,
) -> String {
    let mut output = String::with_capacity(content.len());
    let mut fence = None;
    for line in content.split_inclusive('\n') {
        let trimmed = line.trim_start();
        let marker = if trimmed.starts_with("```") {
            Some('`')
        } else if trimmed.starts_with("~~~") {
            Some('~')
        } else {
            None
        };
        if let Some(marker) = marker {
            fence = if fence == Some(marker) {
                None
            } else if fence.is_none() {
                Some(marker)
            } else {
                fence
            };
            output.push_str(line);
            continue;
        }
        if fence.is_some() {
            output.push_str(line);
            continue;
        }
        let mut rest = line;
        while let Some(open) = rest.find('`') {
            let bytes = rest.as_bytes();
            let count = bytes[open..]
                .iter()
                .take_while(|byte| **byte == b'`')
                .count();
            let needle = "`".repeat(count);
            let after_open = open + count;
            let Some(close) = rest[after_open..].find(&needle) else {
                break;
            };
            output.push_str(&rewrite_links_segment(&rest[..open], current_rel, routes));
            let end = after_open + close + count;
            output.push_str(&rest[open..end]);
            rest = &rest[end..];
        }
        output.push_str(&rewrite_links_segment(rest, current_rel, routes));
    }
    output
}

fn rewrite_links_segment(
    content: &str,
    current_rel: &str,
    routes: &BTreeMap<String, PathBuf>,
) -> String {
    let mut output = String::with_capacity(content.len());
    let mut rest = content;
    while let Some(open) = rest.find('[') {
        output.push_str(&rest[..open]);
        let after_open = &rest[open + 1..];
        let Some(close) = after_open.find("](") else {
            rest = &rest[open..];
            break;
        };
        let label = &after_open[..close];
        let after_destination = &after_open[close + 2..];
        let Some(end) = after_destination.find(')') else {
            rest = &rest[open..];
            break;
        };
        let destination = &after_destination[..end];
        let linked = if destination.starts_with('#')
            || destination.starts_with('/')
            || destination.contains("://")
            || destination.starts_with("mailto:")
        {
            format!("[{label}]({destination})")
        } else {
            let suffix_start = destination.find(['?', '#']).unwrap_or(destination.len());
            let (target, suffix) = destination.split_at(suffix_start);
            let resolved = resolve_relative_link(current_rel, target);
            if let (Some(current_route), Some(route)) = (
                routes.get(current_rel),
                resolved.as_ref().and_then(|path| routes.get(path)),
            ) {
                let mapped = relative_output_link(current_route, route);
                format!("[{label}]({mapped}{suffix})")
            } else {
                format!("{label} (`{destination}`)")
            }
        };
        output.push_str(&linked);
        rest = &after_destination[end + 1..];
    }
    output.push_str(rest);
    output
}

fn relative_output_link(from: &Path, to: &Path) -> String {
    let from = from.parent().unwrap_or_else(|| Path::new(""));
    let from = from
        .components()
        .map(|component| component.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    let to = to
        .components()
        .map(|component| component.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    let common = from
        .iter()
        .zip(&to)
        .take_while(|(left, right)| left == right)
        .count();
    let mut parts = vec!["..".to_owned(); from.len() - common];
    parts.extend(to.iter().skip(common).cloned());
    let relative = parts.join("/");
    if relative.starts_with('.') {
        relative
    } else {
        format!("./{relative}")
    }
}

fn source_frontmatter(content: &str) -> (Option<serde_yaml::Value>, &str) {
    let Some(rest) = content.strip_prefix("---\n") else {
        return (None, content);
    };
    let Some((yaml, body)) = rest.split_once("\n---\n") else {
        return (None, content);
    };
    match serde_yaml::from_str::<serde_yaml::Value>(yaml) {
        Ok(data) => (Some(data), body),
        Err(_) => (None, content),
    }
}

fn resolve_relative_link(current_rel: &str, target: &str) -> Option<String> {
    if target.is_empty() || target.contains('\\') {
        return None;
    }
    let mut parts = current_rel.split('/').rev().skip(1).collect::<Vec<_>>();
    parts.reverse();
    for part in target.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            part => parts.push(part),
        }
    }
    Some(parts.join("/"))
}

fn globs(patterns: &[String]) -> Result<GlobSet> {
    let mut builder = GlobSetBuilder::new();
    for pattern in patterns {
        if pattern.starts_with('/') || pattern.contains("..") {
            return Err(GenerateError::InvalidConfig(format!(
                "unsafe glob: {pattern}"
            )));
        }
        builder.add(
            Glob::new(pattern).map_err(|error| GenerateError::InvalidConfig(error.to_string()))?,
        );
    }
    builder
        .build()
        .map_err(|error| GenerateError::InvalidConfig(error.to_string()))
}

fn yaml_scalar(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace(['\n', '\r'], " ")
}
fn html_attr(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}
fn io(path: &Path, source: std::io::Error) -> GenerateError {
    GenerateError::Io {
        path: path.to_path_buf(),
        source,
    }
}
fn write(path: &Path, content: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| io(parent, error))?;
    }
    std::fs::write(path, content).map_err(|error| io(path, error))
}

#[derive(Debug)]
pub enum SourceWatchEvent {
    Changed,
    Error(GenerateError),
}

pub struct SourceWatchHandle {
    debouncer: Option<Box<dyn std::any::Any + Send>>,
    sender: Option<mpsc::Sender<WatchMessage>>,
    join: Option<JoinHandle<()>>,
}
impl Drop for SourceWatchHandle {
    fn drop(&mut self) {
        if let Some(sender) = &self.sender {
            let _ = sender.send(WatchMessage::Stop);
        }
        self.sender = None;
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
        self.debouncer = None;
    }
}

enum WatchMessage {
    Event(SourceWatchEvent),
    Stop,
}
type SourceDebouncer = Debouncer<notify::RecommendedWatcher, FileIdMap>;

/// Watches external roots and direct Codex skill-link targets discovered at
/// startup. A link changed to a different target requires source reapply or
/// watcher restart; target edits on the original link are observed.
pub fn watch_source<F>(
    spec: SourceSpec,
    debounce: Duration,
    on_change: F,
) -> Result<SourceWatchHandle>
where
    F: Fn(SourceWatchEvent) + Send + 'static,
{
    let (tx, rx) = mpsc::channel::<WatchMessage>();
    let callback = tx.clone();
    let mut watched_roots = if spec.root == spec.scan_root {
        vec![spec.root.clone()]
    } else {
        vec![spec.root.clone(), spec.scan_root.clone()]
    };
    if matches!(spec.rule, SourceRule::Codex) {
        for skill_root in [
            spec.root.join("skills"),
            spec.scan_root.join(".agents/skills"),
        ] {
            let Ok(entries) = std::fs::read_dir(skill_root) else {
                continue;
            };
            for entry in entries.flatten() {
                if std::fs::symlink_metadata(entry.path())
                    .is_ok_and(|meta| meta.file_type().is_symlink())
                {
                    if let Ok(target) = entry.path().canonicalize() {
                        if target.is_dir() && target.join("SKILL.md").is_file() {
                            watched_roots.push(target);
                        }
                    }
                }
            }
        }
    }
    let watched_roots = watched_roots
        .into_iter()
        .map(|root| crate::canonical_or_absolute(&root))
        .collect::<Vec<_>>();
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .map(|path| crate::canonical_or_absolute(&path));
    if watched_roots.iter().any(|root| {
        root.parent().is_none()
            || home
                .as_ref()
                .is_some_and(|home| root == home || home.starts_with(root))
    }) {
        return Err(GenerateError::InvalidConfig(
            "source watcher root is too broad".into(),
        ));
    }
    let event_roots = watched_roots.clone();
    let mut debouncer = new_debouncer(
        debounce,
        None,
        move |event: DebounceEventResult| match event {
            Ok(events)
                if events
                    .iter()
                    .flat_map(|event| &event.paths)
                    .map(|path| crate::canonical_or_absolute(path))
                    .any(|path| {
                        event_roots
                            .iter()
                            .any(|root| path.starts_with(root) || root.starts_with(&path))
                    }) =>
            {
                let _ = callback.send(WatchMessage::Event(SourceWatchEvent::Changed));
            }
            Err(errors) => {
                for error in errors {
                    let _ = callback.send(WatchMessage::Event(SourceWatchEvent::Error(
                        GenerateError::watch("source watcher backend", error),
                    )));
                }
            }
            _ => {}
        },
    )
    .map_err(|error| GenerateError::watch("start source watcher", error))?;
    let mut watched = BTreeMap::new();
    reconcile_source_watches(&mut debouncer, &watched_roots, &mut watched)?;
    let debouncer: Arc<Mutex<SourceDebouncer>> = Arc::new(Mutex::new(debouncer));
    let worker_debouncer = debouncer.clone();
    let join = std::thread::spawn(move || {
        while let Ok(message) = rx.recv() {
            match message {
                WatchMessage::Stop => break,
                WatchMessage::Event(event) => {
                    if let Err(error) = reconcile_source_watches(
                        &mut worker_debouncer.lock().unwrap(),
                        &watched_roots,
                        &mut watched,
                    ) {
                        on_change(SourceWatchEvent::Error(error));
                    }
                    on_change(event);
                }
            }
        }
    });
    Ok(SourceWatchHandle {
        debouncer: Some(Box::new(debouncer)),
        sender: Some(tx),
        join: Some(join),
    })
}

fn reconcile_source_watches(
    debouncer: &mut SourceDebouncer,
    roots: &[PathBuf],
    watched: &mut BTreeMap<PathBuf, RecursiveMode>,
) -> Result<()> {
    for root in roots {
        if root.exists() {
            if watched.get(root) != Some(&RecursiveMode::Recursive) {
                debouncer
                    .watcher()
                    .watch(root, RecursiveMode::Recursive)
                    .map_err(|error| {
                        GenerateError::watch(format!("watch source root {root:?}"), error)
                    })?;
                watched.insert(root.clone(), RecursiveMode::Recursive);
            }
            let parent = root
                .parent()
                .ok_or_else(|| GenerateError::InvalidConfig("source root has no parent".into()))?;
            if !watched.contains_key(parent) {
                debouncer
                    .watcher()
                    .watch(parent, RecursiveMode::NonRecursive)
                    .map_err(|error| {
                        GenerateError::watch(format!("watch source parent {parent:?}"), error)
                    })?;
                watched.insert(parent.to_path_buf(), RecursiveMode::NonRecursive);
            }
        } else {
            watched.remove(root);
            let mut anchor = root.as_path();
            while !anchor.exists() {
                anchor = anchor.parent().ok_or_else(|| {
                    GenerateError::InvalidConfig("source has no existing ancestor".into())
                })?;
            }
            if anchor.parent().is_none() {
                return Err(GenerateError::InvalidConfig(
                    "refusing broad root watcher for missing source".into(),
                ));
            }
            if !watched.contains_key(anchor) {
                debouncer
                    .watcher()
                    .watch(anchor, RecursiveMode::NonRecursive)
                    .map_err(|error| {
                        GenerateError::watch(
                            format!("watch missing-source parent {anchor:?}"),
                            error,
                        )
                    })?;
                watched.insert(anchor.to_path_buf(), RecursiveMode::NonRecursive);
            }
        }
    }
    Ok(())
}
