//! Walker for `$HOME/.claude/` that discovers Claude resources and returns a
//! structured [`ResourceTree`].
//!
//! The only entry point that performs I/O is [`walk_claude_dir`].  All struct
//! constructors are pure data holders with no I/O side-effects.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use thiserror::Error;

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

#[derive(Debug, Error)]
pub enum ResourceError {
    #[error("project_root is too broad: {0:?}. Pass a specific directory such as $HOME/.claude, not $HOME.")]
    ProjectRootTooBroad(PathBuf),

    #[error("I/O error reading {path:?}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

pub type Result<T> = std::result::Result<T, ResourceError>;

// ---------------------------------------------------------------------------
// Public data types  (no I/O in constructors)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaudeMdItem {
    /// Absolute-rooted display path, e.g. "/CLAUDE.md" or "/some/dir/CLAUDE.md"
    pub display_path: String,
    /// "root" for the top-level file; otherwise dir path with "/" replaced by "--"
    pub slug: String,
    /// Relative path from project_root
    pub rel_path: String,
    /// Full file contents
    pub raw_content: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandItem {
    /// Filename without the `.md` extension
    pub name: String,
    /// Value of the `description` frontmatter field (empty string if absent)
    pub description: String,
    /// File body after frontmatter is stripped
    pub raw_content: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillReference {
    /// Filename without `.md`
    pub name: String,
    /// First H1 heading in the body; falls back to `name`
    pub title: String,
    /// Full file contents (including frontmatter if any)
    pub raw_content: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillSubFile {
    /// Bare filename (with extension)
    pub filename: String,
    pub is_markdown: bool,
    /// Only set when `is_markdown` is true
    pub title: Option<String>,
    /// Only set when `is_markdown` is true
    pub raw_content: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillItem {
    /// `name` frontmatter field; falls back to the directory name
    pub name: String,
    /// Directory name inside `<claude_dir>/skills/`
    pub dir: String,
    /// `description` frontmatter field
    pub description: String,
    /// File body of `SKILL.md` after frontmatter is stripped
    pub raw_content: String,
    /// Files in `<skill>/references/*.md`, sorted by name
    pub references: Vec<SkillReference>,
    /// Files in `<skill>/scripts/*`, sorted by filename
    pub script_files: Vec<SkillSubFile>,
    /// Files in `<skill>/assets/*`, sorted by filename
    pub asset_files: Vec<SkillSubFile>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentItem {
    /// `name` frontmatter field; falls back to filename without `.md`
    pub name: String,
    /// Filename without `.md`
    pub file_slug: String,
    /// `description` frontmatter field
    pub description: String,
    /// `model` frontmatter field (empty string if absent)
    pub model: String,
    /// File body after frontmatter is stripped
    pub raw_content: String,
}

/// Top-level result returned by [`walk_claude_dir`].
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ResourceTree {
    /// CLAUDE.md files found under `project_root`.
    /// Sorted: "root" first, then alphabetically by `display_path`.
    pub claude_mds: Vec<ClaudeMdItem>,
    /// `.md` files found under `<claude_dir>/commands/`, sorted by `name`.
    pub commands: Vec<CommandItem>,
    /// Skill directories under `<claude_dir>/skills/` (each must contain `SKILL.md`),
    /// sorted by `name`.
    pub skills: Vec<SkillItem>,
    /// `.md` files under `<claude_dir>/agents/`, sorted by `name`.
    pub agents: Vec<AgentItem>,
}

// ---------------------------------------------------------------------------
// Internal frontmatter helper
// ---------------------------------------------------------------------------

/// Loose frontmatter parsed from YAML between `---` delimiters.
#[derive(Debug, Deserialize, Default)]
struct Frontmatter {
    #[serde(default)]
    description: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    model: String,
    /// Catch-all for any extra fields we don't care about
    #[serde(flatten)]
    _extra: HashMap<String, serde_yaml::Value>,
}

/// Split `---\n<yaml>\n---\n<body>` into `(Frontmatter, body)`.
/// If the file doesn't start with `---`, returns a default Frontmatter and
/// the whole content as body.
fn parse_frontmatter(content: &str) -> (Frontmatter, String) {
    if !content.starts_with("---") {
        return (Frontmatter::default(), content.to_owned());
    }
    // Find the closing `---`
    let after_open = &content[3..];
    // Allow `---\n` or `---\r\n`
    let after_open = after_open.trim_start_matches(['\r', '\n']);
    if let Some(end) = after_open.find("\n---") {
        let yaml_str = &after_open[..end];
        let body_start = end + 4; // skip "\n---"
        // skip optional trailing newline(s) after the closing `---`
        let body = after_open[body_start..].trim_start_matches(['\r', '\n']);
        let fm: Frontmatter = serde_yaml::from_str(yaml_str).unwrap_or_default();
        (fm, body.to_owned())
    } else {
        (Frontmatter::default(), content.to_owned())
    }
}

/// Extract the first `# Heading` in a markdown body.
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
// Helpers for safe file reading
// ---------------------------------------------------------------------------

fn read_file(path: &Path) -> Result<String> {
    std::fs::read_to_string(path).map_err(|e| ResourceError::Io {
        path: path.to_owned(),
        source: e,
    })
}

/// Returns `true` if `path` is a real directory (resolving symlinks).
/// Returns `false` for broken symlinks, files, or I/O errors (warn + continue).
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

/// Directories to skip while walking for CLAUDE.md files.
const EXCLUDE_DIR_NAMES: &[&str] = &[".git", "node_modules", "worktrees"];

/// Recursively collect all `CLAUDE.md` paths under `dir`, skipping the
/// directories whose **full path** starts with any entry in `exclude_paths`,
/// and also skipping dirs whose **name** is in `EXCLUDE_DIR_NAMES`.
fn find_claude_md_files(dir: &Path, exclude_paths: &[PathBuf]) -> Vec<PathBuf> {
    use walkdir::WalkDir;

    let mut results = Vec::new();

    // Canonicalize so path comparisons work on macOS (tempfile /var -> /private/var)
    let canon_dir = dir.canonicalize().unwrap_or_else(|_| dir.to_owned());

    let walker = WalkDir::new(&canon_dir)
        .follow_links(false)
        .into_iter()
        .filter_entry(|entry| {
            let path = entry.path();

            // Always allow the root itself
            if path == canon_dir {
                return true;
            }

            // Skip if the path starts with any excluded absolute path
            for excl in exclude_paths {
                if path.starts_with(excl) {
                    return false;
                }
            }

            // For directories, also skip by name
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
                // Skip directories themselves; we only want files named CLAUDE.md
                if e.file_type().is_dir() {
                    continue;
                }
                // Tolerate broken symlinks: if metadata fails, warn and continue
                if e.path_is_symlink() && e.metadata().is_err() {
                    log::warn!("broken symlink, skipping: {}", e.path().display());
                    continue;
                }
                if e.file_name() == "CLAUDE.md" {
                    results.push(e.into_path());
                }
            }
            Err(e) => {
                // walkdir returns an error for broken symlinks among other things
                log::warn!("walk error (skipping): {}", e);
            }
        }
    }

    results
}

// ---------------------------------------------------------------------------
// Section walkers
// ---------------------------------------------------------------------------

fn collect_claude_mds(
    project_root: &Path,
    exclude_paths: &[PathBuf],
) -> Result<Vec<ClaudeMdItem>> {
    let paths = find_claude_md_files(project_root, exclude_paths);

    // Canonicalize so that strip_prefix works correctly on macOS where
    // tempfile creates dirs under /var -> /private/var symlinks.
    let canon_root = project_root.canonicalize().unwrap_or_else(|_| project_root.to_owned());

    let mut items = Vec::new();
    for file_path in &paths {
        let raw_content = read_file(file_path)?;
        // Try canonical path first, then fall back to the raw file_path
        let canon_file = file_path.canonicalize().unwrap_or_else(|_| file_path.to_owned());
        let rel = canon_file
            .strip_prefix(&canon_root)
            .unwrap_or(&canon_file)
            .to_string_lossy()
            .replace('\\', "/"); // normalise Windows separators
        let display_path = format!("/{}", rel);
        let dir_part = {
            let p = std::path::Path::new(&rel);
            // Note: parent() of "CLAUDE.md" returns Some("") not None, so we
            // must treat both "" and "." as the root case.
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

        items.push(ClaudeMdItem {
            display_path,
            slug,
            rel_path: rel.to_string(),
            raw_content,
        });
    }

    // Sort: "root" first, then alphabetically by display_path
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
        .map_err(|e| ResourceError::Io {
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
        let (fm, body) = parse_frontmatter(&raw);
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

    // Sort by name (locale-independent string sort, matching TS `localeCompare`)
    items.sort_by(|a, b| a.name.cmp(&b.name));

    Ok(items)
}

fn collect_skill_references(refs_dir: &Path) -> Result<Vec<SkillReference>> {
    if !refs_dir.exists() {
        return Ok(vec![]);
    }

    let mut files: Vec<_> = std::fs::read_dir(refs_dir)
        .map_err(|e| ResourceError::Io {
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
        .map_err(|e| ResourceError::Io {
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
        .map_err(|e| ResourceError::Io {
            path: skills_dir.to_owned(),
            source: e,
        })?
        .filter_map(|r| r.ok())
        .filter(|e| {
            // Must be a real directory (tolerate broken symlinks)
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
        let (fm, body) = parse_frontmatter(&raw);

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

    // Sort alphabetically by name
    items.sort_by(|a, b| a.name.cmp(&b.name));

    Ok(items)
}

fn collect_agents(agents_dir: &Path) -> Result<Vec<AgentItem>> {
    if !agents_dir.exists() {
        return Ok(vec![]);
    }

    let mut files: Vec<_> = std::fs::read_dir(agents_dir)
        .map_err(|e| ResourceError::Io {
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
        let (fm, body) = parse_frontmatter(&raw);
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

    // Sort alphabetically by name
    items.sort_by(|a, b| a.name.cmp(&b.name));

    Ok(items)
}

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

/// Walk `claude_dir` and return the full [`ResourceTree`].
///
/// Both `claude_dir` and `project_root` must point to `$HOME/.claude` (not
/// `$HOME` or some other broad directory).  Passing `project_root = $HOME`
/// will return an [`ResourceError::ProjectRootTooBroad`] error.
///
/// All I/O happens inside this function.  The returned struct and its nested
/// types are pure data — their constructors perform no I/O.
pub fn walk_claude_dir(claude_dir: &Path, project_root: &Path) -> Result<ResourceTree> {
    // Guard: refuse project_root that is the user's home directory itself.
    // Both paths are resolved so that trailing slashes / symlinks don't matter.
    let home = dirs_home();
    if let Some(ref home_path) = home {
        let pr = project_root.canonicalize().unwrap_or_else(|_| project_root.to_owned());
        let home_canon = home_path.canonicalize().unwrap_or_else(|_| home_path.to_owned());
        if pr == home_canon {
            return Err(ResourceError::ProjectRootTooBroad(project_root.to_owned()));
        }
    }

    // Build the exclude list for CLAUDE.md discovery
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

/// Best-effort home directory detection — delegates to `$HOME` env var.
fn dirs_home() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

// ---------------------------------------------------------------------------
// Tests are in tests/walker.rs
// ---------------------------------------------------------------------------
