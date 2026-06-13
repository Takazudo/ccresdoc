//! Filesystem walker for `~/.claude/` that discovers Claude resources.
//!
//! Repurposed from the original `ccresdoc-resources` crate. The walker
//! discovers the four resource families (CLAUDE.md hierarchy, commands,
//! skills, agents) and returns a [`ResourceTree`] of pure data. All I/O is
//! confined to [`walk_claude_dir`]; the MDX emission lives in `generate.rs`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::error::{GenerateError, Result};

// ---------------------------------------------------------------------------
// Public(crate) data types — no I/O in constructors
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ClaudeMdItem {
    /// Absolute-rooted display path, e.g. "/CLAUDE.md" or "/some/dir/CLAUDE.md"
    pub display_path: String,
    /// "root" for the top-level file; otherwise dir path with "/" replaced by "--"
    pub slug: String,
    /// Relative path from project_root
    pub rel_path: String,
    /// File body after frontmatter is stripped (raw content if no frontmatter)
    pub raw_content: String,
    /// Whether the source file began with a `---` frontmatter block.
    pub has_frontmatter: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CommandItem {
    pub name: String,
    pub description: String,
    pub raw_content: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SkillReference {
    pub name: String,
    pub title: String,
    pub raw_content: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SkillSubFile {
    pub filename: String,
    pub is_markdown: bool,
    pub title: Option<String>,
    pub raw_content: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SkillItem {
    pub name: String,
    pub dir: String,
    pub description: String,
    pub raw_content: String,
    pub references: Vec<SkillReference>,
    pub script_files: Vec<SkillSubFile>,
    pub asset_files: Vec<SkillSubFile>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AgentItem {
    pub name: String,
    pub file_slug: String,
    pub description: String,
    pub model: String,
    pub raw_content: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct ResourceTree {
    pub claude_mds: Vec<ClaudeMdItem>,
    pub commands: Vec<CommandItem>,
    pub skills: Vec<SkillItem>,
    pub agents: Vec<AgentItem>,
}

// ---------------------------------------------------------------------------
// Frontmatter parsing (gray-matter equivalent)
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize, Default)]
struct Frontmatter {
    #[serde(default)]
    description: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    model: String,
    #[serde(flatten)]
    _extra: HashMap<String, serde_yaml::Value>,
}

/// Split `---\n<yaml>\n---\n<body>` into `(frontmatter, body, had_frontmatter)`.
/// If the content does not begin with `---`, returns a default frontmatter, the
/// whole content as body, and `had_frontmatter = false`.
fn parse_frontmatter(content: &str) -> (Frontmatter, String, bool) {
    if !content.starts_with("---") {
        return (Frontmatter::default(), content.to_owned(), false);
    }
    let after_open = &content[3..];
    let after_open = after_open.trim_start_matches(['\r', '\n']);
    if let Some(end) = after_open.find("\n---") {
        let yaml_str = &after_open[..end];
        let body_start = end + 4; // skip "\n---"
        let body = after_open[body_start..].trim_start_matches(['\r', '\n']);
        let fm: Frontmatter = serde_yaml::from_str(yaml_str).unwrap_or_default();
        (fm, body.to_owned(), true)
    } else {
        // Opened a frontmatter block but never closed it — treat as no
        // frontmatter (matches gray-matter, which only parses a closed block).
        (Frontmatter::default(), content.to_owned(), false)
    }
}

fn extract_h1(content: &str) -> Option<String> {
    for line in content.lines() {
        if let Some(rest) = line.strip_prefix("# ") {
            let title = rest.trim();
            if !title.is_empty() {
                return Some(title.to_owned());
            }
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Safe file reading
// ---------------------------------------------------------------------------

fn read_file(path: &Path) -> Result<String> {
    std::fs::read_to_string(path).map_err(|e| GenerateError::Io {
        path: path.to_owned(),
        source: e,
    })
}

/// True if `path` resolves to a real directory (following symlinks).
fn is_real_dir(path: &Path) -> bool {
    match std::fs::metadata(path) {
        Ok(m) => m.is_dir(),
        Err(e) => {
            log::warn!("skipping {}: {}", path.display(), e);
            false
        }
    }
}

// ---------------------------------------------------------------------------
// CLAUDE.md discovery
// ---------------------------------------------------------------------------

/// Directory names skipped while walking for CLAUDE.md files.
const EXCLUDE_DIR_NAMES: &[&str] = &[".git", "node_modules", "worktrees"];

fn find_claude_md_files(dir: &Path, exclude_paths: &[PathBuf]) -> Vec<PathBuf> {
    use walkdir::WalkDir;

    let mut results = Vec::new();
    let canon_dir = dir.canonicalize().unwrap_or_else(|_| dir.to_owned());

    let walker = WalkDir::new(&canon_dir)
        // `followSymlinks = false` — skills contain symlinks; a symlinked dir
        // can point back into the tree or out to a slow mount and turn the walk
        // into a multi-minute hang.
        .follow_links(false)
        .into_iter()
        .filter_entry(|entry| {
            let path = entry.path();
            if path == canon_dir {
                return true;
            }
            for excl in exclude_paths {
                if path.starts_with(excl) {
                    return false;
                }
            }
            if entry.file_type().is_dir() {
                if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                    if EXCLUDE_DIR_NAMES.contains(&name) {
                        return false;
                    }
                }
            }
            true
        });

    for entry in walker {
        match entry {
            Ok(e) => {
                if e.file_type().is_dir() {
                    continue;
                }
                if e.path_is_symlink() && e.metadata().is_err() {
                    log::warn!("broken symlink, skipping: {}", e.path().display());
                    continue;
                }
                if e.file_name() == "CLAUDE.md" {
                    results.push(e.into_path());
                }
            }
            Err(e) => {
                log::warn!("walk error (skipping): {e}");
            }
        }
    }

    results
}

fn collect_claude_mds(project_root: &Path, exclude_paths: &[PathBuf]) -> Result<Vec<ClaudeMdItem>> {
    let paths = find_claude_md_files(project_root, exclude_paths);
    let canon_root = project_root
        .canonicalize()
        .unwrap_or_else(|_| project_root.to_owned());

    let mut items = Vec::new();
    for file_path in &paths {
        let raw_content = read_file(file_path)?;
        let canon_file = file_path
            .canonicalize()
            .unwrap_or_else(|_| file_path.to_owned());
        let rel = canon_file
            .strip_prefix(&canon_root)
            .unwrap_or(&canon_file)
            .to_string_lossy()
            .replace('\\', "/");
        let display_path = format!("/{rel}");
        let dir_part = {
            let p = std::path::Path::new(&rel);
            match p.parent().and_then(|d| d.to_str()) {
                None | Some("") | Some(".") => ".".to_owned(),
                Some(d) => d.to_owned(),
            }
        };
        let slug = if dir_part == "." {
            "root".to_owned()
        } else {
            dir_part.replace('/', "--")
        };

        // CLAUDE.md files carry no useful frontmatter, but parse anyway so the
        // body excludes any leading `---` block, matching the generator which
        // emits the file's content directly.
        let (_fm, _body, has_frontmatter) = parse_frontmatter(&raw_content);

        items.push(ClaudeMdItem {
            display_path,
            slug,
            rel_path: rel.to_string(),
            // The generator embeds the FULL trimmed content (not the body), so
            // keep raw_content as the whole file.
            raw_content,
            has_frontmatter,
        });
    }

    items.sort_by(|a, b| {
        if a.slug == "root" {
            return std::cmp::Ordering::Less;
        }
        if b.slug == "root" {
            return std::cmp::Ordering::Greater;
        }
        a.display_path.cmp(&b.display_path)
    });

    Ok(items)
}

fn collect_commands(commands_dir: &Path) -> Result<Vec<CommandItem>> {
    if !commands_dir.exists() {
        return Ok(vec![]);
    }

    let mut entries: Vec<_> = std::fs::read_dir(commands_dir)
        .map_err(|e| GenerateError::Io {
            path: commands_dir.to_owned(),
            source: e,
        })?
        .filter_map(|res| res.ok())
        .filter(|e| {
            e.file_name()
                .to_str()
                .map(|n| n.ends_with(".md"))
                .unwrap_or(false)
                && e.file_type().map(|t| t.is_file()).unwrap_or(false)
        })
        .collect();

    entries.sort_by_key(|e| e.file_name());

    let mut items = Vec::new();
    for entry in &entries {
        let path = entry.path();
        let raw = read_file(&path)?;
        let (fm, body, has_frontmatter) = parse_frontmatter(&raw);
        // Generator skips files lacking frontmatter (parseFrontmatter returns
        // an object but the JS only proceeds when matter() succeeds; gray-matter
        // never throws, so it keeps all .md. We match the Rust walker's prior
        // behaviour AND the JS by keeping all .md files but tracking the flag).
        let _ = has_frontmatter;
        let name = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_owned();
        items.push(CommandItem {
            name,
            description: fm.description,
            raw_content: body,
        });
    }

    items.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(items)
}

fn collect_skill_references(refs_dir: &Path) -> Result<Vec<SkillReference>> {
    if !refs_dir.exists() {
        return Ok(vec![]);
    }

    let mut files: Vec<_> = std::fs::read_dir(refs_dir)
        .map_err(|e| GenerateError::Io {
            path: refs_dir.to_owned(),
            source: e,
        })?
        .filter_map(|r| r.ok())
        .filter(|e| {
            e.file_name()
                .to_str()
                .map(|n| n.ends_with(".md"))
                .unwrap_or(false)
                && e.file_type().map(|t| t.is_file()).unwrap_or(false)
        })
        .collect();

    files.sort_by_key(|e| e.file_name());

    let mut refs = Vec::new();
    for entry in &files {
        let path = entry.path();
        let raw = read_file(&path)?;
        let name = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_owned();
        let title = extract_h1(&raw).unwrap_or_else(|| name.clone());
        refs.push(SkillReference {
            name,
            title,
            raw_content: raw,
        });
    }

    refs.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(refs)
}

fn collect_sub_files(sub_dir: &Path) -> Result<Vec<SkillSubFile>> {
    if !sub_dir.exists() {
        return Ok(vec![]);
    }

    let mut files: Vec<_> = std::fs::read_dir(sub_dir)
        .map_err(|e| GenerateError::Io {
            path: sub_dir.to_owned(),
            source: e,
        })?
        .filter_map(|r| r.ok())
        .filter(|e| e.file_type().map(|t| t.is_file()).unwrap_or(false))
        .collect();

    files.sort_by_key(|e| e.file_name());

    let mut result = Vec::new();
    for entry in &files {
        let path = entry.path();
        let filename = path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_owned();
        let is_markdown = filename.ends_with(".md");

        if is_markdown {
            let raw = read_file(&path)?;
            let title = extract_h1(&raw);
            result.push(SkillSubFile {
                filename,
                is_markdown: true,
                title,
                raw_content: Some(raw),
            });
        } else {
            result.push(SkillSubFile {
                filename,
                is_markdown: false,
                title: None,
                raw_content: None,
            });
        }
    }

    Ok(result)
}

fn collect_skills(skills_dir: &Path) -> Result<Vec<SkillItem>> {
    if !skills_dir.exists() {
        return Ok(vec![]);
    }

    let mut dirs: Vec<_> = std::fs::read_dir(skills_dir)
        .map_err(|e| GenerateError::Io {
            path: skills_dir.to_owned(),
            source: e,
        })?
        .filter_map(|r| r.ok())
        .filter(|e| {
            is_real_dir(&e.path()) && {
                let skill_md = e.path().join("SKILL.md");
                skill_md.is_file()
            }
        })
        .collect();

    dirs.sort_by_key(|e| e.file_name());

    let mut items = Vec::new();
    for entry in &dirs {
        let skill_path = entry.path();
        let dir_name = skill_path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_owned();

        let skill_md_path = skill_path.join("SKILL.md");
        let raw = read_file(&skill_md_path)?;
        let (fm, body, has_frontmatter) = parse_frontmatter(&raw);
        // Generator skips skills whose SKILL.md has no parseable frontmatter
        // (`if (!parsed) continue;`). gray-matter only returns null on a YAML
        // throw; an absent block yields `data = {}`. We approximate the
        // generator's intent: a SKILL.md without a frontmatter block is skipped
        // because skills are expected to carry name/description frontmatter.
        if !has_frontmatter {
            log::warn!(
                "skipping skill {dir_name}: SKILL.md has no frontmatter block"
            );
            continue;
        }

        let name = if fm.name.is_empty() {
            dir_name.clone()
        } else {
            fm.name
        };

        let references = collect_skill_references(&skill_path.join("references"))?;
        let script_files = collect_sub_files(&skill_path.join("scripts"))?;
        let asset_files = collect_sub_files(&skill_path.join("assets"))?;

        items.push(SkillItem {
            name,
            dir: dir_name,
            description: fm.description,
            raw_content: body,
            references,
            script_files,
            asset_files,
        });
    }

    items.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(items)
}

fn collect_agents(agents_dir: &Path) -> Result<Vec<AgentItem>> {
    if !agents_dir.exists() {
        return Ok(vec![]);
    }

    let mut files: Vec<_> = std::fs::read_dir(agents_dir)
        .map_err(|e| GenerateError::Io {
            path: agents_dir.to_owned(),
            source: e,
        })?
        .filter_map(|r| r.ok())
        .filter(|e| {
            e.file_name()
                .to_str()
                .map(|n| n.ends_with(".md"))
                .unwrap_or(false)
                && e.file_type().map(|t| t.is_file()).unwrap_or(false)
        })
        .collect();

    files.sort_by_key(|e| e.file_name());

    let mut items = Vec::new();
    for entry in &files {
        let path = entry.path();
        let raw = read_file(&path)?;
        let (fm, body, _has_fm) = parse_frontmatter(&raw);
        let file_slug = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_owned();
        let name = if fm.name.is_empty() {
            file_slug.clone()
        } else {
            fm.name
        };

        items.push(AgentItem {
            name,
            file_slug,
            description: fm.description,
            model: fm.model,
            raw_content: body,
        });
    }

    items.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(items)
}

// ---------------------------------------------------------------------------
// Walk entry point
// ---------------------------------------------------------------------------

/// Walk `claude_dir` / `project_root` and return the full [`ResourceTree`].
///
/// `project_root` must NOT be the user's home directory itself — the CLAUDE.md
/// walk is scoped to `~/.claude` (zudolab/zudo-doc#2115). Passing `$HOME`
/// returns [`GenerateError::ProjectRootTooBroad`].
pub(crate) fn walk_claude_dir(claude_dir: &Path, project_root: &Path) -> Result<ResourceTree> {
    if let Some(ref home_path) = dirs_home() {
        let pr = project_root
            .canonicalize()
            .unwrap_or_else(|_| project_root.to_owned());
        let home_canon = home_path
            .canonicalize()
            .unwrap_or_else(|_| home_path.to_owned());
        if pr == home_canon {
            return Err(GenerateError::ProjectRootTooBroad(project_root.to_owned()));
        }
    }

    let exclude_paths: Vec<PathBuf> = EXCLUDE_DIR_NAMES
        .iter()
        .map(|name| project_root.join(name))
        .collect();

    let claude_mds = collect_claude_mds(project_root, &exclude_paths)?;
    let commands = collect_commands(&claude_dir.join("commands"))?;
    let skills = collect_skills(&claude_dir.join("skills"))?;
    let agents = collect_agents(&claude_dir.join("agents"))?;

    Ok(ResourceTree {
        claude_mds,
        commands,
        skills,
        agents,
    })
}

fn dirs_home() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}
