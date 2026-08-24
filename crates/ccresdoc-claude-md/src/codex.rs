//! Native Codex resource generation and watching.
//!
//! This module intentionally owns only the six detail namespaces. The routed
//! `docs/codex/index.mdx` landing page belongs to the application coordinator.

use std::collections::{HashMap, HashSet};
use std::ffi::OsStr;
use std::path::{Component, Path, PathBuf};
use std::sync::mpsc;
use std::thread::JoinHandle;
use std::time::Duration;

use notify::{RecursiveMode, Watcher};
use notify_debouncer_full::{new_debouncer, DebounceEventResult};
use serde::Deserialize;
use unicode_casefold::UnicodeCaseFold;
use unicode_normalization::UnicodeNormalization;
use walkdir::WalkDir;

use crate::error::{GenerateError, Result};
use crate::escape::escape_for_mdx;
use crate::generate::{downgrade_repo_relative_links, rewrite_skill_links};
use crate::{canonical_or_absolute, validate_no_overlap};

const EXCLUDED_DIRS: &[&str] = &[
    // Codex runtime state can be large, high-churn, and may contain captured
    // repository/session material. It is never an instruction discovery root.
    "sessions",
    "archived_sessions",
    "shell_snapshots",
    "history",
    "log",
    "logs",
    "tmp",
    "node_modules",
    "worktrees",
    "dist",
    "out",
    "public",
    "__inbox",
    "test-results",
    "fixtures",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodexConfig {
    /// Configured Codex home (normally `~/.codex`).
    pub codex_dir: PathBuf,
    /// Configured root for recursive AGENTS.md discovery and `.agents/skills`.
    /// The desktop integration normally passes the same value as `codex_dir`.
    pub project_root: PathBuf,
    /// zudo-doc content root. Only `codex-{agents-md,config,agents,hooks,rules,skills}`
    /// are managed; `codex/` is never written or removed.
    pub docs_dir: PathBuf,
}

impl CodexConfig {
    pub fn validate(&self) -> Result<()> {
        for (label, path) in [
            ("codex_dir", &self.codex_dir),
            ("project_root", &self.project_root),
            ("docs_dir", &self.docs_dir),
        ] {
            if !path.is_absolute() {
                return Err(GenerateError::InvalidConfig(format!(
                    "{label} must be an absolute path, got {path:?}"
                )));
            }
        }

        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .map(|p| canonical_or_absolute(&p));
        for (label, path) in [
            ("codex_dir", &self.codex_dir),
            ("project_root", &self.project_root),
        ] {
            let canonical = canonical_or_absolute(path);
            if canonical.parent().is_none()
                || home
                    .as_ref()
                    .is_some_and(|home| canonical == *home || home.starts_with(&canonical))
            {
                return Err(GenerateError::ProjectRootTooBroad(path.clone()));
            }
            validate_no_overlap(label, path, &self.docs_dir)?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodexSource {
    Instructions,
    Config,
    Agents,
    Hooks,
    Rules,
    Skills,
    Watcher,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GenerateWarning {
    pub source: CodexSource,
    pub path: PathBuf,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CodexGenerateReport {
    pub agents_md: usize,
    pub config: usize,
    pub agents: usize,
    pub hooks: usize,
    pub rules: usize,
    pub skills: usize,
    pub warnings: Vec<GenerateWarning>,
}

struct Context<'a> {
    config: &'a CodexConfig,
    warnings: Vec<GenerateWarning>,
}

impl Context<'_> {
    fn warn(&mut self, source: CodexSource, path: impl Into<PathBuf>, message: impl Into<String>) {
        let warning = GenerateWarning {
            source,
            path: path.into(),
            message: message.into(),
        };
        log::warn!("{}: {}", warning.path.display(), warning.message);
        self.warnings.push(warning);
    }
}

pub fn generate_codex(config: &CodexConfig) -> Result<CodexGenerateReport> {
    config.validate()?;
    validate_managed_outputs(config)?;
    let mut cx = Context {
        config,
        warnings: Vec::new(),
    };
    let agents_md = generate_agents_md(&mut cx)?;
    let config_count = generate_config(&mut cx)?;
    let agents = generate_agents(&mut cx)?;
    let hooks = generate_hooks(&mut cx)?;
    let rules = generate_rules(&mut cx)?;
    let skills = generate_skills(&mut cx)?;
    Ok(CodexGenerateReport {
        agents_md,
        config: config_count,
        agents,
        hooks,
        rules,
        skills,
        warnings: cx.warnings,
    })
}

fn validate_managed_outputs(config: &CodexConfig) -> Result<()> {
    ensure_dir(&config.docs_dir)?;
    let docs = config
        .docs_dir
        .canonicalize()
        .map_err(|e| io(&config.docs_dir, e))?;
    for name in [
        "codex-agents-md",
        "codex-config",
        "codex-agents",
        "codex-hooks",
        "codex-rules",
        "codex-skills",
    ] {
        let path = config.docs_dir.join(name);
        match std::fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(GenerateError::InvalidConfig(format!(
                    "managed output namespace must not be a symlink: {path:?}"
                )));
            }
            Ok(_) => {
                let canonical = path.canonicalize().map_err(|e| io(&path, e))?;
                if !canonical.starts_with(&docs) {
                    return Err(GenerateError::InvalidConfig(format!(
                        "managed output escapes docs_dir: {path:?}"
                    )));
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(io(&path, error)),
        }
    }
    Ok(())
}

fn io(path: &Path, source: std::io::Error) -> GenerateError {
    GenerateError::Io {
        path: path.to_path_buf(),
        source,
    }
}

fn ensure_dir(path: &Path) -> Result<()> {
    std::fs::create_dir_all(path).map_err(|e| io(path, e))
}

fn ensure_managed_child(parent: &Path, name: &str) -> Result<PathBuf> {
    let path = parent.join(name);
    match std::fs::symlink_metadata(&path) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
            return Err(GenerateError::InvalidConfig(format!(
                "managed output directory must be a real directory: {path:?}"
            )));
        }
        Ok(_) => {
            let canonical_parent = parent.canonicalize().map_err(|e| io(parent, e))?;
            let canonical = path.canonicalize().map_err(|e| io(&path, e))?;
            if !canonical.starts_with(canonical_parent) {
                return Err(GenerateError::InvalidConfig(format!(
                    "managed output directory escapes its parent: {path:?}"
                )));
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => ensure_dir(&path)?,
        Err(error) => return Err(io(&path, error)),
    }
    Ok(path)
}

fn remove_managed_dir(path: &Path) -> Result<()> {
    match std::fs::remove_dir_all(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(io(path, e)),
    }
}

fn write_if_changed(path: &Path, content: &str) -> Result<()> {
    if std::fs::symlink_metadata(path).is_ok_and(|metadata| metadata.file_type().is_symlink()) {
        return Err(GenerateError::InvalidConfig(format!(
            "generated output file must not be a symlink: {path:?}"
        )));
    }
    if std::fs::read(path).ok().as_deref() == Some(content.as_bytes()) {
        return Ok(());
    }
    std::fs::write(path, content).map_err(|e| io(path, e))
}

fn prune(output: &Path, keep_files: &HashSet<String>, keep_dirs: &HashSet<String>) -> Result<()> {
    if !output.exists() {
        return Ok(());
    }
    for entry in std::fs::read_dir(output).map_err(|e| io(output, e))? {
        let entry = entry.map_err(|e| io(output, e))?;
        let name = entry.file_name().to_string_lossy().into_owned();
        let ty = entry.file_type().map_err(|e| io(&entry.path(), e))?;
        if ty.is_symlink() && !keep_files.contains(&name) && !keep_dirs.contains(&name) {
            std::fs::remove_file(entry.path()).map_err(|e| io(&entry.path(), e))?;
        } else if ty.is_file() && !keep_files.contains(&name) {
            std::fs::remove_file(entry.path()).map_err(|e| io(&entry.path(), e))?;
        } else if ty.is_dir() && !keep_dirs.contains(&name) {
            std::fs::remove_dir_all(entry.path()).map_err(|e| io(&entry.path(), e))?;
        }
    }
    Ok(())
}

fn yaml_string(value: &str) -> String {
    format!(
        "\"{}\"",
        value
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('\n', "\\n")
            .replace('\r', "\\r")
    )
}

fn category(output: &Path, title: &str, description: &str, position: u32) -> Result<()> {
    write_if_changed(&output.join("index.mdx"), &format!(
        "---\ntitle: {}\ndescription: {}\nsidebar_position: {position}\ncategory_no_page: true\ngenerated: true\n---\n",
        yaml_string(title), yaml_string(description)
    ))
}

fn page(
    output: &Path,
    title: &str,
    description: &str,
    label: Option<&str>,
    position: Option<usize>,
    body: &str,
) -> Result<()> {
    let position = position
        .map(|p| format!("sidebar_position: {p}\n"))
        .unwrap_or_default();
    let label = label
        .map(|l| format!("sidebar_label: {}\n", yaml_string(l)))
        .unwrap_or_default();
    write_if_changed(
        output,
        &format!(
            "---\ntitle: {}\ndescription: {}\n{position}{label}generated: true\n---\n\n{}\n",
            yaml_string(title),
            yaml_string(description),
            body.trim()
        ),
    )
}

fn normalized_key(value: &str) -> String {
    value.nfkc().case_fold().nfkc().collect()
}

fn validate_slug(
    slug: &str,
    category: &str,
    source: &Path,
    seen: &mut HashMap<String, PathBuf>,
) -> Result<()> {
    if slug.is_empty()
        || normalized_key(slug) == "index"
        || Path::new(slug)
            .components()
            .any(|c| !matches!(c, Component::Normal(_)))
        || slug.contains('/')
        || slug.contains('\\')
    {
        return Err(GenerateError::ReservedSlug(format!(
            "{category}: unsafe or reserved output slug {slug:?} from {}",
            source.display()
        )));
    }
    let key = normalized_key(slug);
    if let Some(previous) = seen.insert(key, source.to_path_buf()) {
        return Err(GenerateError::SlugCollision(format!(
            "{category}: {slug:?} is produced by both {} and {}",
            previous.display(),
            source.display()
        )));
    }
    Ok(())
}

fn read_utf8(cx: &mut Context<'_>, source: CodexSource, path: &Path) -> Option<String> {
    match std::fs::read_to_string(path) {
        Ok(value) => Some(value),
        Err(error) => {
            cx.warn(
                source,
                path,
                format!("unable to read UTF-8 source; skipping: {error}"),
            );
            None
        }
    }
}

fn table_cell(value: Option<&str>) -> String {
    let value = value
        .filter(|v| !v.is_empty())
        .unwrap_or("—")
        .replace("\r\n", " ")
        .replace(['\r', '\n'], " ")
        .replace('|', "\\|");
    let longest = value
        .as_bytes()
        .split(|b| *b != b'`')
        .map(<[u8]>::len)
        .max()
        .unwrap_or(0);
    let fence = "`".repeat((longest + 1).max(1));
    if value.starts_with(['`', ' ']) || value.ends_with(['`', ' ']) {
        format!("{fence} {value} {fence}")
    } else {
        format!("{fence}{value}{fence}")
    }
}

fn code_fence(source: &str, language: &str) -> String {
    let longest = source
        .as_bytes()
        .split(|b| *b != b'`')
        .map(<[u8]>::len)
        .max()
        .unwrap_or(0);
    let fence = "`".repeat((longest + 1).max(3));
    let newline = if source.ends_with('\n') { "" } else { "\n" };
    format!("{fence}{language}\n{source}{newline}{fence}")
}

fn filename_slug(filename: &str) -> String {
    filename.replace('.', "-")
}

fn excluded(entry: &walkdir::DirEntry, root: &Path, docs: &Path) -> bool {
    if entry.path() == root {
        return false;
    }
    let path = entry.path();
    if path.starts_with(docs) {
        return true;
    }
    entry.file_type().is_dir()
        && entry
            .file_name()
            .to_str()
            .is_some_and(|name| name.starts_with('.') || EXCLUDED_DIRS.contains(&name))
}

// ---- AGENTS.md -----------------------------------------------------------

fn generate_agents_md(cx: &mut Context<'_>) -> Result<usize> {
    let output = cx.config.docs_dir.join("codex-agents-md");
    let root = canonical_or_absolute(&cx.config.project_root);
    let docs = canonical_or_absolute(&cx.config.docs_dir);
    if !root.exists() {
        remove_managed_dir(&output)?;
        return Ok(0);
    }
    let mut found = Vec::new();
    for entry in WalkDir::new(&root)
        .follow_links(false)
        .into_iter()
        .filter_entry(|e| !excluded(e, &root, &docs))
    {
        match entry {
            Ok(entry)
                if entry.file_type().is_file()
                    && matches!(
                        entry.file_name().to_str(),
                        Some("AGENTS.md" | "AGENTS.override.md")
                    ) =>
            {
                found.push(entry.path().to_path_buf())
            }
            Ok(_) => {}
            Err(error) => cx.warn(
                CodexSource::Instructions,
                error.path().unwrap_or(&root),
                format!("walk entry skipped: {error}"),
            ),
        }
    }
    found.sort_by(|a, b| {
        let ar = a.strip_prefix(&root).unwrap_or(a);
        let br = b.strip_prefix(&root).unwrap_or(b);
        let a_nested = ar
            .parent()
            .is_some_and(|parent| !parent.as_os_str().is_empty());
        let b_nested = br
            .parent()
            .is_some_and(|parent| !parent.as_os_str().is_empty());
        (a_nested, ar).cmp(&(b_nested, br))
    });
    if found.is_empty() {
        remove_managed_dir(&output)?;
        return Ok(0);
    }
    ensure_dir(&output)?;
    let mut keep = HashSet::from(["index.mdx".to_string()]);
    let mut seen = HashMap::new();
    let mut count = 0;
    for path in found {
        let rel = path
            .strip_prefix(&root)
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('\\', "/");
        let parent = Path::new(&rel)
            .parent()
            .and_then(Path::to_str)
            .filter(|p| !p.is_empty() && *p != ".");
        let base = parent
            .map(|p| p.replace('/', "--"))
            .unwrap_or_else(|| "root".into());
        let slug = if path.file_name() == Some(OsStr::new("AGENTS.override.md")) {
            format!("{base}--override")
        } else {
            base
        };
        validate_slug(&slug, "codex-agents-md", &path, &mut seen)?;
        let Some(source) = read_utf8(cx, CodexSource::Instructions, &path) else {
            continue;
        };
        page(
            &output.join(format!("{slug}.mdx")),
            &format!("/{rel}"),
            &format!("Codex instructions at /{rel}"),
            Some(&rel),
            Some(count + 1),
            &format!(
                "**Path:** {}\n\n{}",
                table_cell(Some(&rel)),
                escape_for_mdx(&downgrade_repo_relative_links(source.trim()))
            ),
        )?;
        keep.insert(format!("{slug}.mdx"));
        count += 1;
    }
    if count == 0 {
        remove_managed_dir(&output)?;
        return Ok(0);
    }
    category(&output, "AGENTS.md", "Project instructions for Codex", 905)?;
    prune(&output, &keep, &HashSet::new())?;
    Ok(count)
}

// ---- config and agents TOML ---------------------------------------------

fn sorted_files(dir: &Path, predicate: impl Fn(&str) -> bool) -> std::io::Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if ty.is_file() && predicate(&name) {
            files.push(entry.path());
        }
    }
    files.sort_by(|a, b| a.file_name().cmp(&b.file_name()));
    Ok(files)
}

fn confined_subdir(base: &Path, name: &str) -> Option<PathBuf> {
    let mut path = base.to_path_buf();
    for component in Path::new(name).components() {
        let Component::Normal(component) = component else {
            return None;
        };
        path.push(component);
        let metadata = std::fs::symlink_metadata(&path).ok()?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return None;
        }
    }
    let canonical = path.canonicalize().ok()?;
    canonical
        .starts_with(canonical_or_absolute(base))
        .then_some(path)
}

fn is_regular_file(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok_and(|metadata| metadata.is_file())
}

fn toml_scalar(value: &toml::Value) -> Option<String> {
    match value {
        toml::Value::String(v) => Some(v.clone()),
        toml::Value::Integer(v) => Some(v.to_string()),
        toml::Value::Float(v) => Some(v.to_string()),
        toml::Value::Boolean(v) => Some(v.to_string()),
        toml::Value::Datetime(v) => Some(v.to_string()),
        toml::Value::Array(v) => Some(toml::Value::Array(v.clone()).to_string()),
        toml::Value::Table(_) => None,
    }
}

fn generate_config(cx: &mut Context<'_>) -> Result<usize> {
    let output = cx.config.docs_dir.join("codex-config");
    if !cx.config.codex_dir.is_dir() {
        remove_managed_dir(&output)?;
        return Ok(0);
    }
    let mut files = match sorted_files(&cx.config.codex_dir, |name| {
        name == "config.toml" || name == "config.toml.example" || name.ends_with(".config.toml")
    }) {
        Ok(files) => files,
        Err(error) => {
            cx.warn(
                CodexSource::Config,
                &cx.config.codex_dir,
                format!("unable to list config files: {error}"),
            );
            remove_managed_dir(&output)?;
            return Ok(0);
        }
    };
    files.sort_by_key(|p| {
        (
            p.file_name() != Some(OsStr::new("config.toml")),
            p.file_name().map(OsStr::to_os_string),
        )
    });
    ensure_dir(&output)?;
    let mut keep = HashSet::from(["index.mdx".to_string()]);
    let mut seen = HashMap::new();
    let mut count = 0;
    for path in files {
        let Some(source) = read_utf8(cx, CodexSource::Config, &path) else {
            continue;
        };
        let parsed: toml::Table = match toml::from_str(&source) {
            Ok(value) => value,
            Err(error) => {
                cx.warn(
                    CodexSource::Config,
                    &path,
                    format!("unable to parse TOML; skipping: {error}"),
                );
                continue;
            }
        };
        let filename = path.file_name().unwrap().to_string_lossy();
        let slug = filename_slug(&filename);
        validate_slug(&slug, "codex-config", &path, &mut seen)?;
        let mut rows: Vec<_> = parsed
            .iter()
            .filter_map(|(key, value)| {
                toml_scalar(value)
                    .map(|v| format!("| {} | {} |", table_cell(Some(key)), table_cell(Some(&v))))
            })
            .collect();
        if rows.is_empty() {
            rows.push("| — | — |".into());
        }
        let mut sections = Vec::new();
        for line in source.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with('[') && trimmed.ends_with(']') {
                sections.push(format!("- {}", table_cell(Some(trimmed))));
            }
        }
        if sections.is_empty() {
            sections.push("—".into());
        }
        let body = format!("## Settings\n\n| Key | Value |\n| --- | --- |\n{}\n\n## Sections\n\n{}\n\n## Source\n\n{}", rows.join("\n"), sections.join("\n"), code_fence(&source, "toml"));
        page(
            &output.join(format!("{slug}.mdx")),
            &filename,
            &format!("Codex configuration from {filename}"),
            Some(&filename),
            None,
            &body,
        )?;
        keep.insert(format!("{slug}.mdx"));
        count += 1;
    }
    if count == 0 {
        remove_managed_dir(&output)?;
        return Ok(0);
    }
    category(&output, "Config", "config.toml and profiles", 906)?;
    prune(&output, &keep, &HashSet::new())?;
    Ok(count)
}

fn optional_toml_string<'a>(
    cx: &mut Context<'_>,
    table: &'a toml::Table,
    field: &str,
    path: &Path,
) -> Option<&'a str> {
    match table.get(field) {
        None => None,
        Some(toml::Value::String(value)) => Some(value),
        Some(_) => {
            cx.warn(
                CodexSource::Agents,
                path,
                format!("{field} must be a string; omitting"),
            );
            None
        }
    }
}

fn generate_agents(cx: &mut Context<'_>) -> Result<usize> {
    let source_dir = cx.config.codex_dir.join("agents");
    let output = cx.config.docs_dir.join("codex-agents");
    if confined_subdir(&cx.config.codex_dir, "agents").is_none() {
        remove_managed_dir(&output)?;
        return Ok(0);
    }
    let files = match sorted_files(&source_dir, |name| name.ends_with(".toml")) {
        Ok(files) => files,
        Err(error) => {
            cx.warn(
                CodexSource::Agents,
                &source_dir,
                format!("unable to list agents: {error}"),
            );
            remove_managed_dir(&output)?;
            return Ok(0);
        }
    };
    ensure_dir(&output)?;
    let mut keep = HashSet::from(["index.mdx".to_string()]);
    let mut seen = HashMap::new();
    let mut count = 0;
    for path in files {
        let Some(source) = read_utf8(cx, CodexSource::Agents, &path) else {
            continue;
        };
        let parsed: toml::Table = match toml::from_str(&source) {
            Ok(v) => v,
            Err(e) => {
                cx.warn(
                    CodexSource::Agents,
                    &path,
                    format!("unable to parse TOML; skipping: {e}"),
                );
                continue;
            }
        };
        let filename = path.file_name().unwrap().to_string_lossy();
        let stem = filename.strip_suffix(".toml").unwrap_or(&filename);
        let slug = filename_slug(stem);
        validate_slug(&slug, "codex-agents", &path, &mut seen)?;
        let name = optional_toml_string(cx, &parsed, "name", &path)
            .unwrap_or(&slug)
            .to_string();
        let description = optional_toml_string(cx, &parsed, "description", &path)
            .unwrap_or("")
            .to_string();
        let model = optional_toml_string(cx, &parsed, "model", &path).map(str::to_string);
        let reasoning =
            optional_toml_string(cx, &parsed, "model_reasoning_effort", &path).map(str::to_string);
        let sandbox = optional_toml_string(cx, &parsed, "sandbox_mode", &path).map(str::to_string);
        let instructions =
            optional_toml_string(cx, &parsed, "developer_instructions", &path).map(str::to_string);
        let mut parts = Vec::new();
        if let Some(v) = model {
            parts.push(format!("**Model:** {}", table_cell(Some(&v))));
        }
        if let Some(v) = reasoning {
            parts.push(format!("**Reasoning effort:** {}", table_cell(Some(&v))));
        }
        if let Some(v) = sandbox {
            parts.push(format!("**Sandbox:** {}", table_cell(Some(&v))));
        }
        if let Some(v) = instructions {
            parts.push(format!(
                "## Developer instructions\n\n{}",
                escape_for_mdx(v.trim())
            ));
        }
        parts.push(format!("## Source\n\n{}", code_fence(&source, "toml")));
        page(
            &output.join(format!("{slug}.mdx")),
            &name,
            &description,
            Some(&name),
            None,
            &parts.join("\n\n"),
        )?;
        keep.insert(format!("{slug}.mdx"));
        count += 1;
    }
    if count == 0 {
        remove_managed_dir(&output)?;
        return Ok(0);
    }
    category(&output, "Agents", "Custom subagents", 907)?;
    prune(&output, &keep, &HashSet::new())?;
    Ok(count)
}

// ---- hooks ---------------------------------------------------------------

fn json_display(value: Option<&serde_json::Value>) -> Option<String> {
    match value {
        None | Some(serde_json::Value::Null) => None,
        Some(serde_json::Value::String(v)) => Some(v.clone()),
        Some(value) => Some(value.to_string()),
    }
}

fn script_language(filename: &str) -> &'static str {
    match Path::new(filename)
        .extension()
        .and_then(OsStr::to_str)
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("sh") => "bash",
        Some("py") => "python",
        Some("js" | "mjs") => "javascript",
        Some("ts") => "typescript",
        _ => "",
    }
}

fn script_description(source: &str) -> String {
    let mut lines = source.lines();
    let first = lines.next().unwrap_or("");
    let comment = if first.starts_with("#!") {
        lines.next().unwrap_or("")
    } else {
        first
    };
    comment
        .strip_prefix("# ")
        .or_else(|| comment.strip_prefix("// "))
        .unwrap_or("")
        .to_string()
}

fn generate_hooks(cx: &mut Context<'_>) -> Result<usize> {
    let output = cx.config.docs_dir.join("codex-hooks");
    let hooks_json = cx.config.codex_dir.join("hooks.json");
    let hooks_dir = cx.config.codex_dir.join("hooks");
    let mut keep = HashSet::from(["index.mdx".to_string()]);
    let mut seen = HashMap::new();
    let mut count = 0;
    ensure_dir(&output)?;

    if is_regular_file(&hooks_json) {
        if let Some(source) = read_utf8(cx, CodexSource::Hooks, &hooks_json) {
            match serde_json::from_str::<serde_json::Value>(&source) {
                Err(error) => cx.warn(
                    CodexSource::Hooks,
                    &hooks_json,
                    format!("unable to parse JSON; skipping hooks.json page: {error}"),
                ),
                Ok(root) => {
                    if let Some(events) = root.get("hooks").and_then(serde_json::Value::as_object) {
                        let slug = "hooks-json";
                        validate_slug(slug, "codex-hooks", &hooks_json, &mut seen)?;
                        let mut rows = Vec::new();
                        for (event, groups) in events {
                            let Some(groups) = groups.as_array() else {
                                cx.warn(
                                    CodexSource::Hooks,
                                    &hooks_json,
                                    format!("hooks.{event} must be an array; skipping event"),
                                );
                                continue;
                            };
                            for (group_index, group) in groups.iter().enumerate() {
                                let Some(handlers) =
                                    group.get("hooks").and_then(serde_json::Value::as_array)
                                else {
                                    cx.warn(CodexSource::Hooks, &hooks_json, format!("hooks.{event}[{group_index}].hooks must be an array; skipping group"));
                                    continue;
                                };
                                for (handler_index, handler) in handlers.iter().enumerate() {
                                    let Some(command) =
                                        handler.get("command").and_then(serde_json::Value::as_str)
                                    else {
                                        cx.warn(CodexSource::Hooks, &hooks_json, format!("hooks.{event}[{group_index}].hooks[{handler_index}] needs a string command; skipping row"));
                                        continue;
                                    };
                                    let values = [
                                        Some(event.to_string()),
                                        json_display(group.get("matcher")),
                                        json_display(handler.get("type")),
                                        Some(command.to_string()),
                                        json_display(handler.get("timeout")),
                                        json_display(handler.get("async")),
                                    ];
                                    rows.push(format!(
                                        "| {} |",
                                        values
                                            .iter()
                                            .map(|v| table_cell(v.as_deref()))
                                            .collect::<Vec<_>>()
                                            .join(" | ")
                                    ));
                                }
                            }
                        }
                        if rows.is_empty() {
                            rows.push("| — | — | — | — | — | — |".into());
                        }
                        let body = format!("| Event | Matcher | Type | Command | Timeout | Async |\n| --- | --- | --- | --- | --- | --- |\n{}\n\n## Source\n\n{}", rows.join("\n"), code_fence(&source, "json"));
                        page(
                            &output.join("hooks-json.mdx"),
                            "hooks.json",
                            "Codex lifecycle hook configuration",
                            Some("hooks.json"),
                            None,
                            &body,
                        )?;
                        keep.insert("hooks-json.mdx".into());
                        count += 1;
                    } else {
                        cx.warn(
                            CodexSource::Hooks,
                            &hooks_json,
                            "hooks.hooks must be an object; skipping hooks.json page",
                        );
                    }
                }
            }
        }
    }

    if confined_subdir(&cx.config.codex_dir, "hooks").is_some() {
        let files = match sorted_files(&hooks_dir, |_| true) {
            Ok(v) => v,
            Err(e) => {
                cx.warn(
                    CodexSource::Hooks,
                    &hooks_dir,
                    format!("unable to list hook scripts: {e}"),
                );
                Vec::new()
            }
        };
        for path in files {
            let Some(source) = read_utf8(cx, CodexSource::Hooks, &path) else {
                continue;
            };
            let filename = path.file_name().unwrap().to_string_lossy();
            let slug = filename_slug(&filename);
            validate_slug(&slug, "codex-hooks", &path, &mut seen)?;
            page(
                &output.join(format!("{slug}.mdx")),
                &filename,
                &script_description(&source),
                Some(&filename),
                None,
                &format!(
                    "## Source\n\n{}",
                    code_fence(&source, script_language(&filename))
                ),
            )?;
            keep.insert(format!("{slug}.mdx"));
            count += 1;
        }
    }
    if count == 0 {
        remove_managed_dir(&output)?;
        return Ok(0);
    }
    category(&output, "Hooks", "Lifecycle hooks", 908)?;
    prune(&output, &keep, &HashSet::new())?;
    Ok(count)
}

// ---- rules ---------------------------------------------------------------

#[derive(Clone, Copy, Default)]
struct ParseState {
    quote: Option<u8>,
    triple: bool,
    escaped: bool,
    comment: bool,
}

fn advance(source: &[u8], index: usize, state: &mut ParseState) -> usize {
    let ch = source[index];
    if state.comment {
        if ch == b'\n' {
            state.comment = false;
        }
        return 1;
    }
    if let Some(quote) = state.quote {
        if state.escaped {
            state.escaped = false;
            return 1;
        }
        if ch == b'\\' {
            state.escaped = true;
            return 1;
        }
        if state.triple && source.get(index..index + 3) == Some(&[quote, quote, quote]) {
            state.quote = None;
            state.triple = false;
            return 3;
        }
        if !state.triple && ch == quote {
            state.quote = None;
        }
        return 1;
    }
    if ch == b'#' {
        state.comment = true;
        return 1;
    }
    if matches!(ch, b'\'' | b'"') {
        state.quote = Some(ch);
        state.triple = source.get(index..index + 3) == Some(&[ch, ch, ch]);
        return if state.triple { 3 } else { 1 };
    }
    1
}

fn prefix_rule_bodies(source: &str) -> Vec<&str> {
    let bytes = source.as_bytes();
    let mut bodies = Vec::new();
    let mut state = ParseState::default();
    let mut index = 0;
    while index < bytes.len() {
        if state.quote.is_none()
            && !state.comment
            && bytes.get(index..index + "prefix_rule".len()) == Some(b"prefix_rule")
        {
            let before_ok =
                index == 0 || !bytes[index - 1].is_ascii_alphanumeric() && bytes[index - 1] != b'_';
            let end = index + "prefix_rule".len();
            let after_ok =
                end == bytes.len() || !bytes[end].is_ascii_alphanumeric() && bytes[end] != b'_';
            if before_ok && after_ok {
                let mut open = end;
                while open < bytes.len() && bytes[open].is_ascii_whitespace() {
                    open += 1;
                }
                if bytes.get(open) == Some(&b'(') {
                    let mut call = ParseState::default();
                    let mut depth = 1;
                    let mut cursor = open + 1;
                    while cursor < bytes.len() {
                        if call.quote.is_none() && !call.comment {
                            if bytes[cursor] == b'(' {
                                depth += 1;
                            }
                            if bytes[cursor] == b')' {
                                depth -= 1;
                                if depth == 0 {
                                    break;
                                }
                            }
                        }
                        cursor += advance(bytes, cursor, &mut call);
                    }
                    bodies.push(&source[open + 1..cursor]);
                    index = (cursor + 1).min(bytes.len());
                    continue;
                }
            }
        }
        index += advance(bytes, index, &mut state);
    }
    bodies
}

fn split_top_level(text: &str, delimiter: u8) -> Vec<&str> {
    let bytes = text.as_bytes();
    let mut state = ParseState::default();
    let (mut square, mut round, mut curly) = (0i32, 0i32, 0i32);
    let mut start = 0;
    let mut index = 0;
    let mut result = Vec::new();
    while index < bytes.len() {
        if state.quote.is_none() && !state.comment {
            match bytes[index] {
                b'[' => square += 1,
                b']' => square -= 1,
                b'(' => round += 1,
                b')' => round -= 1,
                b'{' => curly += 1,
                b'}' => curly -= 1,
                ch if ch == delimiter && square == 0 && round == 0 && curly == 0 => {
                    result.push(&text[start..index]);
                    start = index + 1;
                }
                _ => {}
            }
        }
        index += advance(bytes, index, &mut state);
    }
    result.push(&text[start..]);
    result
}

fn string_literal(value: &str) -> Option<String> {
    let value = value.trim();
    let quote = *value.as_bytes().first()?;
    if !matches!(quote, b'\'' | b'"') {
        return None;
    }
    let triple = value.as_bytes().starts_with(&[quote, quote, quote])
        && value.as_bytes().ends_with(&[quote, quote, quote]);
    if triple && value.len() >= 6 {
        return Some(value[3..value.len() - 3].to_string());
    }
    if value.as_bytes().last() != Some(&quote) || value.len() < 2 {
        return None;
    }
    let mut out = String::new();
    let mut chars = value[1..value.len() - 1].chars();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            out.push(ch);
            continue;
        }
        out.push(match chars.next()? {
            'n' => '\n',
            'r' => '\r',
            't' => '\t',
            '\\' => '\\',
            '\'' => '\'',
            '"' => '"',
            other => other,
        });
    }
    Some(out)
}

fn pattern_value(value: &str) -> Option<Vec<String>> {
    let value = value.trim();
    if let Some(value) = string_literal(value) {
        return Some(vec![value]);
    }
    if !(value.starts_with('[') && value.ends_with(']')) {
        return None;
    }
    let mut out = Vec::new();
    for part in split_top_level(&value[1..value.len() - 1], b',') {
        if part.trim().is_empty() {
            continue;
        }
        let nested = pattern_value(part)?;
        out.push(if part.trim().starts_with('[') {
            nested.join("|")
        } else {
            nested.into_iter().next()?
        });
    }
    Some(out)
}

fn parse_rule(body: &str) -> (Option<String>, Option<String>, Option<String>) {
    let mut args = HashMap::new();
    for part in split_top_level(body, b',') {
        let assignment = split_top_level(part, b'=');
        if assignment.len() == 2 {
            args.insert(assignment[0].trim(), assignment[1].trim());
        }
    }
    let pattern = args
        .get("pattern")
        .and_then(|v| pattern_value(v))
        .map(|v| v.join(" "));
    let decision = args
        .get("decision")
        .and_then(|v| string_literal(v))
        .or_else(|| Some("allow".into()));
    let justification = args.get("justification").and_then(|v| string_literal(v));
    (pattern, decision, justification)
}

fn generate_rules(cx: &mut Context<'_>) -> Result<usize> {
    let source_dir = cx.config.codex_dir.join("rules");
    let output = cx.config.docs_dir.join("codex-rules");
    if confined_subdir(&cx.config.codex_dir, "rules").is_none() {
        remove_managed_dir(&output)?;
        return Ok(0);
    }
    let files = match sorted_files(&source_dir, |name| name.ends_with(".rules")) {
        Ok(v) => v,
        Err(e) => {
            cx.warn(
                CodexSource::Rules,
                &source_dir,
                format!("unable to list rules: {e}"),
            );
            remove_managed_dir(&output)?;
            return Ok(0);
        }
    };
    ensure_dir(&output)?;
    let mut keep = HashSet::from(["index.mdx".into()]);
    let mut seen = HashMap::new();
    let mut count = 0;
    for path in files {
        let Some(source) = read_utf8(cx, CodexSource::Rules, &path) else {
            continue;
        };
        let filename = path.file_name().unwrap().to_string_lossy();
        let slug = filename
            .strip_suffix(".rules")
            .unwrap_or(&filename)
            .to_string();
        validate_slug(&slug, "codex-rules", &path, &mut seen)?;
        let mut rows: Vec<String> = prefix_rule_bodies(&source)
            .into_iter()
            .map(parse_rule)
            .map(|(p, d, j)| {
                format!(
                    "| {} | {} | {} |",
                    table_cell(p.as_deref()),
                    table_cell(d.as_deref()),
                    table_cell(j.as_deref())
                )
            })
            .collect();
        if rows.is_empty() {
            rows.push("| — | — | — |".into());
        }
        let body = format!("## Rules\n\n| Pattern | Decision | Justification |\n| --- | --- | --- |\n{}\n\n## Source\n\n{}", rows.join("\n"), code_fence(&source, "python"));
        page(
            &output.join(format!("{slug}.mdx")),
            &filename,
            &format!("Command approval rules from {filename}"),
            Some(&filename),
            None,
            &body,
        )?;
        keep.insert(format!("{slug}.mdx"));
        count += 1;
    }
    if count == 0 {
        remove_managed_dir(&output)?;
        return Ok(0);
    }
    category(&output, "Rules", "Command approval rules", 909)?;
    prune(&output, &keep, &HashSet::new())?;
    Ok(count)
}

// ---- skills --------------------------------------------------------------

#[derive(Debug)]
struct SkillPackage {
    name_on_disk: String,
    path: PathBuf,
    canonical: PathBuf,
}

#[derive(Debug, Default, Deserialize)]
struct SkillFrontmatter {
    #[serde(default)]
    name: Option<serde_yaml::Value>,
    #[serde(default)]
    description: Option<serde_yaml::Value>,
}

fn parse_skill_frontmatter(source: &str) -> Option<(SkillFrontmatter, &str)> {
    let rest = source.strip_prefix("---")?.trim_start_matches(['\r', '\n']);
    let end = rest.find("\n---")?;
    let data = serde_yaml::from_str(&rest[..end]).ok()?;
    Some((data, rest[end + 4..].trim_start_matches(['\r', '\n'])))
}

fn value_string(
    cx: &mut Context<'_>,
    value: Option<serde_yaml::Value>,
    field: &str,
    path: &Path,
) -> Option<String> {
    match value {
        None => None,
        Some(serde_yaml::Value::String(v)) => Some(v),
        Some(_) => {
            cx.warn(
                CodexSource::Skills,
                path,
                format!("{field} must be a string; using fallback"),
            );
            None
        }
    }
}

fn skill_roots(config: &CodexConfig) -> Vec<PathBuf> {
    let candidates = [
        confined_subdir(&config.codex_dir, "skills"),
        confined_subdir(&config.project_root, ".agents/skills"),
    ];
    let mut seen = HashSet::new();
    let mut result = Vec::new();
    for candidate in candidates.into_iter().flatten() {
        if let Ok(canonical) = candidate.canonicalize() {
            if canonical.is_dir() && seen.insert(canonical) {
                result.push(candidate);
            }
        }
    }
    result
}

fn discover_skill_packages(cx: &mut Context<'_>) -> Vec<SkillPackage> {
    let mut packages = Vec::new();
    let mut seen_targets = HashSet::new();
    let mut seen_names: HashMap<String, PathBuf> = HashMap::new();
    for source_root in skill_roots(cx.config) {
        let canonical_root = match source_root.canonicalize() {
            Ok(v) => v,
            Err(e) => {
                cx.warn(
                    CodexSource::Skills,
                    &source_root,
                    format!("unable to resolve skill root: {e}"),
                );
                continue;
            }
        };
        let mut entries: Vec<_> = match std::fs::read_dir(&source_root) {
            Ok(v) => v
                .filter_map(|entry| match entry {
                    Ok(v) => Some(v),
                    Err(e) => {
                        cx.warn(
                            CodexSource::Skills,
                            &source_root,
                            format!("directory entry skipped: {e}"),
                        );
                        None
                    }
                })
                .collect(),
            Err(e) => {
                cx.warn(
                    CodexSource::Skills,
                    &source_root,
                    format!("unable to read skill root: {e}"),
                );
                continue;
            }
        };
        entries.sort_by_key(std::fs::DirEntry::file_name);
        for entry in entries {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') {
                continue;
            }
            let path = entry.path();
            let metadata = match std::fs::symlink_metadata(&path) {
                Ok(v) => v,
                Err(e) => {
                    cx.warn(
                        CodexSource::Skills,
                        &path,
                        format!("unable to inspect skill package: {e}"),
                    );
                    continue;
                }
            };
            let direct_symlink = metadata.file_type().is_symlink();
            let canonical = match path.canonicalize() {
                Ok(v) => v,
                Err(e) => {
                    cx.warn(
                        CodexSource::Skills,
                        &path,
                        format!("broken/cyclic skill symlink or deletion race; skipping: {e}"),
                    );
                    continue;
                }
            };
            if !canonical.is_dir() || (!direct_symlink && !canonical.starts_with(&canonical_root)) {
                cx.warn(
                    CodexSource::Skills,
                    &path,
                    "skill package is not a confined directory; skipping",
                );
                continue;
            }
            if !is_regular_file(&canonical.join("SKILL.md")) {
                continue;
            }
            if !seen_targets.insert(canonical.clone()) {
                continue;
            }
            let key = normalized_key(&name);
            if let Some(previous) = seen_names.get(&key) {
                cx.warn(
                    CodexSource::Skills,
                    &path,
                    format!(
                        "same-name skill conflicts with {}; keeping the first",
                        previous.display()
                    ),
                );
                continue;
            }
            seen_names.insert(key, path.clone());
            packages.push(SkillPackage {
                name_on_disk: name,
                path,
                canonical,
            });
        }
    }
    packages
}

fn flat_files(cx: &mut Context<'_>, directory: &Path) -> Vec<PathBuf> {
    let metadata = match std::fs::symlink_metadata(directory) {
        Ok(v) => v,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Vec::new(),
        Err(e) => {
            cx.warn(
                CodexSource::Skills,
                directory,
                format!("unable to inspect directory: {e}"),
            );
            return Vec::new();
        }
    };
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Vec::new();
    }
    let mut files = Vec::new();
    let entries = match std::fs::read_dir(directory) {
        Ok(v) => v,
        Err(e) => {
            cx.warn(
                CodexSource::Skills,
                directory,
                format!("unable to list directory: {e}"),
            );
            return Vec::new();
        }
    };
    for entry in entries {
        let entry = match entry {
            Ok(v) => v,
            Err(e) => {
                cx.warn(
                    CodexSource::Skills,
                    directory,
                    format!("directory entry skipped: {e}"),
                );
                continue;
            }
        };
        match entry.file_type() {
            Ok(ty) if ty.is_file() => files.push(entry.path()),
            Ok(_) => {}
            Err(e) => cx.warn(
                CodexSource::Skills,
                entry.path(),
                format!("unable to inspect file: {e}"),
            ),
        }
    }
    files.sort_by(|a, b| a.file_name().cmp(&b.file_name()));
    files
}

fn h1_or_stem(source: &str, path: &Path) -> String {
    source
        .lines()
        .find_map(|line| {
            line.strip_prefix("# ")
                .map(str::trim)
                .filter(|v| !v.is_empty())
                .map(str::to_string)
        })
        .or_else(|| path.file_stem().and_then(OsStr::to_str).map(str::to_string))
        .unwrap_or_else(|| "Resource".into())
}

fn openai_metadata(cx: &mut Context<'_>, package: &SkillPackage) -> String {
    let Some(agents_dir) = confined_subdir(&package.canonical, "agents") else {
        return String::new();
    };
    let path = agents_dir.join("openai.yaml");
    if !is_regular_file(&path) {
        return String::new();
    }
    let Some(source) = read_utf8(cx, CodexSource::Skills, &path) else {
        return String::new();
    };
    let value: serde_yaml::Value = match serde_yaml::from_str(&source) {
        Ok(v) => v,
        Err(e) => {
            cx.warn(
                CodexSource::Skills,
                &path,
                format!("unable to parse YAML; omitting metadata: {e}"),
            );
            return String::new();
        }
    };
    let Some(root) = value.as_mapping() else {
        cx.warn(
            CodexSource::Skills,
            &path,
            "YAML root must be an object; omitting metadata",
        );
        return String::new();
    };
    let interface = yaml_optional_mapping(cx, root, "interface", &path);
    let policy = yaml_optional_mapping(cx, root, "policy", &path);
    let mut lines = Vec::new();
    if let Some(v) = yaml_optional_string(cx, interface, "display_name", &path) {
        lines.push(format!("**Display name:** {}", escape_for_mdx(v)));
    }
    if let Some(v) = yaml_optional_string(cx, interface, "short_description", &path) {
        lines.push(format!("**Short description:** {}", escape_for_mdx(v)));
    }
    if let Some(value) = policy.and_then(|mapping| {
        mapping.get(serde_yaml::Value::String(
            "allow_implicit_invocation".into(),
        ))
    }) {
        match value.as_bool() {
            Some(false) => lines.push(format!(
                "**Invocation:** explicit only (`${}`)",
                package.name_on_disk
            )),
            Some(true) => {}
            None => cx.warn(
                CodexSource::Skills,
                &path,
                "policy.allow_implicit_invocation must be a boolean; omitting invocation metadata",
            ),
        }
    }
    lines.join("\n")
}

fn yaml_optional_mapping<'a>(
    cx: &mut Context<'_>,
    root: &'a serde_yaml::Mapping,
    field: &str,
    path: &Path,
) -> Option<&'a serde_yaml::Mapping> {
    match root.get(serde_yaml::Value::String(field.into())) {
        None => None,
        Some(value) => match value.as_mapping() {
            Some(mapping) => Some(mapping),
            None => {
                cx.warn(
                    CodexSource::Skills,
                    path,
                    format!("{field} must be an object; omitting metadata"),
                );
                None
            }
        },
    }
}

fn yaml_optional_string<'a>(
    cx: &mut Context<'_>,
    map: Option<&'a serde_yaml::Mapping>,
    field: &str,
    path: &Path,
) -> Option<&'a str> {
    match map.and_then(|mapping| mapping.get(serde_yaml::Value::String(field.into()))) {
        None => None,
        Some(value) => match value.as_str() {
            Some(value) => Some(value),
            None => {
                cx.warn(
                    CodexSource::Skills,
                    path,
                    format!("{field} must be a string; omitting metadata"),
                );
                None
            }
        },
    }
}

fn file_tree(skill: &str, groups: &[(&str, &[PathBuf])]) -> String {
    let mut lines = vec![format!("{skill}/")];
    let total = 1 + groups.len();
    lines.push(format!(
        "{}SKILL.md",
        if total == 1 {
            "└── "
        } else {
            "├── "
        }
    ));
    for (index, (name, files)) in groups.iter().enumerate() {
        let last = index + 2 == total;
        lines.push(format!("{}{name}/", if last { "└── " } else { "├── " }));
        for (file_index, file) in files.iter().enumerate() {
            lines.push(format!(
                "{}{}{}",
                if last { "    " } else { "│   " },
                if file_index + 1 == files.len() {
                    "└── "
                } else {
                    "├── "
                },
                file.file_name().unwrap().to_string_lossy()
            ));
        }
    }
    lines.join("\n")
}

fn generate_skills(cx: &mut Context<'_>) -> Result<usize> {
    let output = cx.config.docs_dir.join("codex-skills");
    let packages = discover_skill_packages(cx);
    if packages.is_empty() {
        remove_managed_dir(&output)?;
        return Ok(0);
    }
    ensure_dir(&output)?;
    let keep_files = HashSet::from(["index.mdx".into()]);
    let mut keep_dirs = HashSet::new();
    let mut seen = HashMap::new();
    let mut count = 0;
    for package in packages {
        validate_slug(
            &package.name_on_disk,
            "codex-skills",
            &package.path,
            &mut seen,
        )?;
        let skill_md = package.canonical.join("SKILL.md");
        let Some(source) = read_utf8(cx, CodexSource::Skills, &skill_md) else {
            continue;
        };
        let Some((frontmatter, body)) = parse_skill_frontmatter(&source) else {
            cx.warn(
                CodexSource::Skills,
                &skill_md,
                "missing or malformed frontmatter; skipping skill",
            );
            continue;
        };
        let name = value_string(cx, frontmatter.name, "name", &skill_md)
            .filter(|v| !v.is_empty())
            .unwrap_or_else(|| package.name_on_disk.clone());
        let description =
            value_string(cx, frontmatter.description, "description", &skill_md).unwrap_or_default();
        let short = if description.chars().count() > 200 {
            format!("{}...", description.chars().take(200).collect::<String>())
        } else {
            description.clone()
        };
        let references = flat_files(cx, &package.canonical.join("references"));
        let scripts = flat_files(cx, &package.canonical.join("scripts"));
        let assets = flat_files(cx, &package.canonical.join("assets"));
        let groups: Vec<_> = [
            ("scripts", scripts.as_slice()),
            ("references", references.as_slice()),
            ("assets", assets.as_slice()),
        ]
        .into_iter()
        .filter(|(_, files)| !files.is_empty())
        .collect();
        let mut parts = Vec::new();
        let metadata = openai_metadata(cx, &package);
        if !metadata.is_empty() {
            parts.push(metadata);
        }
        if !groups.is_empty() {
            let mut links = Vec::new();
            for (kind, files, prefix) in [
                ("references", references.as_slice(), "ref"),
                ("scripts", scripts.as_slice(), "script"),
                ("assets", assets.as_slice(), "asset"),
            ] {
                for file in files
                    .iter()
                    .filter(|p| p.extension() == Some(OsStr::new("md")))
                {
                    let filename = file.file_name().unwrap().to_string_lossy();
                    let stem = file.file_stem().unwrap().to_string_lossy();
                    links.push(format!("- [{kind}/{filename}](./{prefix}-{stem})"));
                }
            }
            let links = if links.is_empty() {
                String::new()
            } else {
                format!("\n\n{}", links.join("\n"))
            };
            parts.push(format!(
                "## File Structure\n\n```\n{}\n```{links}",
                file_tree(&package.name_on_disk, &groups)
            ));
        }
        parts.push(escape_for_mdx(rewrite_skill_links(body.trim()).as_str()));
        let skill_output = ensure_managed_child(&output, &package.name_on_disk)?;
        let mut skill_keep = HashSet::from(["index.mdx".into()]);
        let mut sub_seen = HashMap::new();
        page(
            &skill_output.join("index.mdx"),
            &name,
            &short,
            Some(&name),
            None,
            &parts.join("\n\n"),
        )?;
        for (files, prefix) in [
            (&references, "ref"),
            (&scripts, "script"),
            (&assets, "asset"),
        ] {
            for file in files
                .iter()
                .filter(|p| p.extension() == Some(OsStr::new("md")))
            {
                let stem = file.file_stem().unwrap().to_string_lossy();
                let slug = format!("{prefix}-{stem}");
                validate_slug(&slug, "codex-skill subpage", file, &mut sub_seen)?;
                let Some(raw) = read_utf8(cx, CodexSource::Skills, file) else {
                    continue;
                };
                let title = h1_or_stem(&raw, file);
                write_if_changed(
                    &skill_output.join(format!("{slug}.mdx")),
                    &format!(
                        "---\ntitle: {}\nunlisted: true\ngenerated: true\n---\n\n{}\n",
                        yaml_string(&title),
                        escape_for_mdx(raw.trim())
                    ),
                )?;
                skill_keep.insert(format!("{slug}.mdx"));
            }
        }
        prune(&skill_output, &skill_keep, &HashSet::new())?;
        keep_dirs.insert(package.name_on_disk);
        count += 1;
    }
    if count == 0 {
        remove_managed_dir(&output)?;
        return Ok(0);
    }
    category(&output, "Skills", "Skill packages", 910)?;
    prune(&output, &keep_files, &keep_dirs)?;
    Ok(count)
}

// ---- watcher -------------------------------------------------------------

#[derive(Debug)]
pub enum CodexWatchEvent {
    Regenerated(CodexGenerateReport),
    Error {
        source: CodexSource,
        error: GenerateError,
    },
}

pub struct CodexWatchHandle {
    debouncer: Option<Box<dyn std::any::Any + Send>>,
    event_tx: Option<mpsc::Sender<()>>,
    join: Option<JoinHandle<()>>,
}

impl CodexWatchHandle {
    pub fn stop(mut self) {
        self.stop_inner();
    }
    fn stop_inner(&mut self) {
        self.debouncer = None;
        self.event_tx = None;
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}
impl Drop for CodexWatchHandle {
    fn drop(&mut self) {
        self.stop_inner();
    }
}

fn relevant(path: &Path, config: &CodexConfig, targets: &[PathBuf]) -> bool {
    if path.starts_with(&config.docs_dir) {
        return false;
    }
    if targets.iter().any(|target| path.starts_with(target)) {
        return true;
    }
    if path.starts_with(config.project_root.join(".agents/skills")) {
        return true;
    }
    if path.starts_with(&config.project_root)
        && matches!(
            path.file_name().and_then(OsStr::to_str),
            Some("AGENTS.md" | "AGENTS.override.md")
        )
    {
        return true;
    }
    if !path.starts_with(&config.codex_dir) {
        return false;
    }
    let relative = path.strip_prefix(&config.codex_dir).unwrap_or(path);
    relative == Path::new("config.toml")
        || relative == Path::new("config.toml.example")
        || (relative.components().count() == 1
            && relative
                .file_name()
                .and_then(OsStr::to_str)
                .is_some_and(|n| n.ends_with(".config.toml")))
        || relative == Path::new("hooks.json")
        || ["agents", "hooks", "rules", "skills", ".agents/skills"]
            .iter()
            .any(|dir| relative.starts_with(dir))
        || matches!(
            relative.file_name().and_then(OsStr::to_str),
            Some("AGENTS.md" | "AGENTS.override.md")
        )
}

fn direct_skill_targets(config: &CodexConfig) -> Vec<PathBuf> {
    let mut targets = HashSet::new();
    for root in skill_roots(config) {
        let Ok(entries) = std::fs::read_dir(root) else {
            continue;
        };
        for entry in entries.flatten() {
            if std::fs::symlink_metadata(entry.path()).is_ok_and(|m| m.file_type().is_symlink()) {
                if let Ok(target) = entry.path().canonicalize() {
                    if target.is_dir() {
                        targets.insert(target);
                    }
                }
            }
        }
    }
    let mut targets: Vec<_> = targets.into_iter().collect();
    targets.sort();
    targets
}

pub fn watch_codex<F>(
    config: CodexConfig,
    debounce: Duration,
    on_change: F,
) -> Result<CodexWatchHandle>
where
    F: Fn(CodexWatchEvent) + Send + 'static,
{
    config.validate()?;
    let targets = direct_skill_targets(&config);
    let (event_tx, event_rx) = mpsc::channel();
    let (error_tx, error_rx) = mpsc::channel();
    let callback_tx = event_tx.clone();
    let callback_config = CodexConfig {
        codex_dir: canonical_or_absolute(&config.codex_dir),
        project_root: canonical_or_absolute(&config.project_root),
        docs_dir: canonical_or_absolute(&config.docs_dir),
    };
    let callback_targets = targets.clone();
    let mut debouncer =
        new_debouncer(
            debounce,
            None,
            move |result: DebounceEventResult| match result {
                Ok(events) => {
                    if events.iter().flat_map(|e| &e.paths).any(|path| {
                        relevant(
                            &canonical_or_absolute(path),
                            &callback_config,
                            &callback_targets,
                        )
                    }) {
                        let _ = callback_tx.send(());
                    }
                }
                Err(errors) => {
                    for error in errors {
                        let _ = error_tx
                            .send(GenerateError::watch("Codex watcher backend error", error));
                    }
                    // Wake the worker so backend errors are delivered even if
                    // no later content event arrives.
                    let _ = callback_tx.send(());
                }
            },
        )
        .map_err(|e| GenerateError::watch("failed to start Codex watcher", e))?;
    let mut watched = vec![config.codex_dir.clone()];
    if config.project_root != config.codex_dir {
        watched.push(config.project_root.clone());
    }
    watched.extend(targets);
    let mut unique = HashSet::new();
    for path in watched {
        let path = canonical_or_absolute(&path);
        if path.exists() && unique.insert(path.clone()) {
            debouncer
                .watcher()
                .watch(&path, RecursiveMode::Recursive)
                .map_err(|e| GenerateError::watch(format!("failed to watch {path:?}"), e))?;
        }
    }
    let worker = config.clone();
    let join = std::thread::Builder::new()
        .name("ccresdoc-codex-watch".into())
        .spawn(move || {
            while event_rx.recv().is_ok() {
                while event_rx.try_recv().is_ok() {}
                while let Ok(error) = error_rx.try_recv() {
                    on_change(CodexWatchEvent::Error {
                        source: CodexSource::Watcher,
                        error,
                    });
                }
                match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    generate_codex(&worker)
                })) {
                    Ok(Ok(report)) => on_change(CodexWatchEvent::Regenerated(report)),
                    Ok(Err(error)) => on_change(CodexWatchEvent::Error {
                        source: CodexSource::Watcher,
                        error,
                    }),
                    Err(_) => on_change(CodexWatchEvent::Error {
                        source: CodexSource::Watcher,
                        error: GenerateError::watch("Codex regeneration panicked", PanicError),
                    }),
                }
            }
        })
        .map_err(|e| GenerateError::watch("failed to spawn Codex watch thread", e))?;
    Ok(CodexWatchHandle {
        debouncer: Some(Box::new(debouncer)),
        event_tx: Some(event_tx),
        join: Some(join),
    })
}

#[derive(Debug)]
struct PanicError;
impl std::fmt::Display for PanicError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Codex regeneration panicked")
    }
}
impl std::error::Error for PanicError {}
