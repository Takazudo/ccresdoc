//! Axum handler functions and shared server state.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use axum::{
    body::Body,
    extract::{Path as AxumPath, State},
    http::{header, HeaderValue, Response, StatusCode},
    response::{Html, IntoResponse},
};
use ccresdoc_renderer::{render_markdown, RenderOptions, SENTINEL_CONTENT, SENTINEL_TITLE};
use ccresdoc_resources::{
    walk_claude_dir, AgentItem, ClaudeMdItem, CommandItem, ResourceTree, SkillItem,
};

use crate::manifest::Manifest;

// ---------------------------------------------------------------------------
// Shared app state
// ---------------------------------------------------------------------------

/// Immutable state shared across all handlers.
#[derive(Clone)]
pub(crate) struct AppState {
    pub(crate) claude_dir: PathBuf,
    pub(crate) project_root: PathBuf,
    pub(crate) dist_dir: PathBuf,
    /// Contents of `dist/_shell/index.html` loaded once at startup.
    pub(crate) shell_html: Arc<String>,
}

impl AppState {
    pub(crate) fn load(
        claude_dir: PathBuf,
        project_root: PathBuf,
        dist_dir: PathBuf,
    ) -> anyhow::Result<Self> {
        let shell_path = dist_dir.join("_shell").join("index.html");
        let shell_html = std::fs::read_to_string(&shell_path)
            .map_err(|e| anyhow::anyhow!("cannot read shell HTML at {:?}: {}", shell_path, e))?;
        Ok(Self {
            claude_dir,
            project_root,
            dist_dir,
            shell_html: Arc::new(shell_html),
        })
    }

    /// Walk the claude directory. Cheap, no caching for v1.
    pub(crate) fn walk(&self) -> anyhow::Result<ResourceTree> {
        walk_claude_dir(&self.claude_dir, &self.project_root)
            .map_err(|e| anyhow::anyhow!("walker error: {}", e))
    }
}

// ---------------------------------------------------------------------------
// HTML escape helper
// ---------------------------------------------------------------------------

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#x27;")
}

// ---------------------------------------------------------------------------
// Shell substitution helper
// ---------------------------------------------------------------------------

fn render_shell(shell: &str, title: &str, content: &str) -> String {
    let escaped_title = html_escape(title);
    shell
        .replace(SENTINEL_TITLE, &escaped_title)
        .replace(SENTINEL_CONTENT, content)
}

fn render_md_to_html(raw: &str) -> anyhow::Result<String> {
    let opts = RenderOptions::default();
    render_markdown(raw, &opts).map_err(|e| anyhow::anyhow!("render error: {}", e))
}

// ---------------------------------------------------------------------------
// 404 helper
// ---------------------------------------------------------------------------

/// Return a 404 response from `dist/404.html` or a plain text fallback.
pub(crate) fn not_found_response(state: &AppState) -> Response<Body> {
    let path_404 = state.dist_dir.join("404.html");
    if let Ok(contents) = std::fs::read_to_string(&path_404) {
        (StatusCode::NOT_FOUND, Html(contents)).into_response()
    } else {
        (StatusCode::NOT_FOUND, "404 Not Found").into_response()
    }
}

// ---------------------------------------------------------------------------
// Static file helper
// ---------------------------------------------------------------------------

fn static_file_response(file_path: &Path, cache_secs: Option<u64>) -> Response<Body> {
    match std::fs::read(file_path) {
        Ok(bytes) => {
            let mime = mime_guess::from_path(file_path).first_or_octet_stream();
            let mut builder = Response::builder()
                .status(StatusCode::OK)
                .header(header::CONTENT_TYPE, mime.as_ref());

            if let Some(secs) = cache_secs {
                let val = format!("public, max-age={}", secs);
                builder = builder.header(header::CACHE_CONTROL, &val);
            }

            builder.body(Body::from(bytes)).unwrap()
        }
        Err(_) => Response::builder()
            .status(StatusCode::NOT_FOUND)
            .body(Body::from("Not Found"))
            .unwrap(),
    }
}

// ---------------------------------------------------------------------------
// Simple static handlers
// ---------------------------------------------------------------------------

/// GET /
pub(crate) async fn home(State(state): State<AppState>) -> Response<Body> {
    static_file_response(&state.dist_dir.join("index.html"), None)
}

/// GET /404
pub(crate) async fn not_found_page(State(state): State<AppState>) -> Response<Body> {
    let path = state.dist_dir.join("404.html");
    let mut resp = static_file_response(&path, None);
    *resp.status_mut() = StatusCode::NOT_FOUND;
    resp
}

/// GET /___ready
pub(crate) async fn ready() -> (StatusCode, &'static str) {
    (StatusCode::OK, "OK")
}

/// GET /ccresdoc-sidebar.js
pub(crate) async fn sidebar_js(State(state): State<AppState>) -> Response<Body> {
    static_file_response(&state.dist_dir.join("ccresdoc-sidebar.js"), None)
}

/// GET /assets/*path — long-lived cache (1 year)
pub(crate) async fn assets(
    State(state): State<AppState>,
    AxumPath(path): AxumPath<String>,
) -> Response<Body> {
    let file_path = state.dist_dir.join("assets").join(&path);
    // One-year cache for hashed assets
    static_file_response(&file_path, Some(31_536_000))
}

/// GET /_shell and /_shell/* — always 404
pub(crate) async fn shell_forbidden(State(state): State<AppState>) -> Response<Body> {
    not_found_response(&state)
}

// ---------------------------------------------------------------------------
// Manifest handler
// ---------------------------------------------------------------------------

/// GET /api/manifest.json
pub(crate) async fn api_manifest(State(state): State<AppState>) -> Response<Body> {
    match state.walk() {
        Ok(tree) => {
            let manifest = Manifest::build(&tree);
            match serde_json::to_string(&manifest) {
                Ok(json) => Response::builder()
                    .status(StatusCode::OK)
                    .header(
                        header::CONTENT_TYPE,
                        HeaderValue::from_static("application/json; charset=utf-8"),
                    )
                    .body(Body::from(json))
                    .unwrap(),
                Err(e) => {
                    eprintln!("manifest serialization error: {}", e);
                    Response::builder()
                        .status(StatusCode::INTERNAL_SERVER_ERROR)
                        .body(Body::from("Internal Server Error"))
                        .unwrap()
                }
            }
        }
        Err(e) => {
            eprintln!("walk error: {}", e);
            Response::builder()
                .status(StatusCode::INTERNAL_SERVER_ERROR)
                .body(Body::from("Internal Server Error"))
                .unwrap()
        }
    }
}

// ---------------------------------------------------------------------------
// Category index pages
// ---------------------------------------------------------------------------

fn category_index_html(label: &str, intro: &str, items: &[(String, String, String)]) -> String {
    // items: (href, label, description)
    if items.is_empty() {
        format!(
            "<h1>{}</h1>\n<p>{}</p>\n<p>No items found.</p>",
            html_escape(label),
            intro
        )
    } else {
        let mut li_parts = String::new();
        for (href, item_label, description) in items {
            li_parts.push_str(&format!(
                "<li><a href=\"{}\"><strong>{}</strong></a> — {}</li>\n",
                html_escape(href),
                html_escape(item_label),
                html_escape(description),
            ));
        }
        format!(
            "<h1>{}</h1>\n<p>{}</p>\n<ul class=\"ccresdoc-category-index\">\n{}</ul>",
            html_escape(label),
            intro,
            li_parts
        )
    }
}

/// GET /claude-md/
pub(crate) async fn claude_md_index(State(state): State<AppState>) -> Response<Body> {
    let tree = match state.walk() {
        Ok(t) => t,
        Err(e) => {
            eprintln!("walk error: {}", e);
            return not_found_response(&state);
        }
    };

    let items: Vec<(String, String, String)> = tree
        .claude_mds
        .iter()
        .map(|item| {
            (
                format!("/claude-md/{}", item.slug),
                item.display_path.clone(),
                item.display_path.clone(),
            )
        })
        .collect();

    let content = category_index_html("CLAUDE.md", "CLAUDE.md files in your project.", &items);
    let page = render_shell(&state.shell_html, "CLAUDE.md — CCResDoc", &content);
    html_200(page)
}

/// GET /claude-commands/
pub(crate) async fn claude_commands_index(State(state): State<AppState>) -> Response<Body> {
    let tree = match state.walk() {
        Ok(t) => t,
        Err(e) => {
            eprintln!("walk error: {}", e);
            return not_found_response(&state);
        }
    };

    let items: Vec<(String, String, String)> = tree
        .commands
        .iter()
        .map(|cmd| {
            (
                format!("/claude-commands/{}", cmd.name),
                cmd.name.clone(),
                cmd.description.clone(),
            )
        })
        .collect();

    let content = category_index_html(
        "Commands",
        "Custom slash commands defined under ~/.claude/commands/.",
        &items,
    );
    let page = render_shell(&state.shell_html, "Commands — CCResDoc", &content);
    html_200(page)
}

/// GET /claude-skills/
pub(crate) async fn claude_skills_index(State(state): State<AppState>) -> Response<Body> {
    let tree = match state.walk() {
        Ok(t) => t,
        Err(e) => {
            eprintln!("walk error: {}", e);
            return not_found_response(&state);
        }
    };

    let items: Vec<(String, String, String)> = tree
        .skills
        .iter()
        .map(|skill| {
            (
                format!("/claude-skills/{}", skill.dir),
                skill.name.clone(),
                skill.description.clone(),
            )
        })
        .collect();

    let content = category_index_html(
        "Skills",
        "Reusable skill modules under ~/.claude/skills/.",
        &items,
    );
    let page = render_shell(&state.shell_html, "Skills — CCResDoc", &content);
    html_200(page)
}

/// GET /claude-agents/
pub(crate) async fn claude_agents_index(State(state): State<AppState>) -> Response<Body> {
    let tree = match state.walk() {
        Ok(t) => t,
        Err(e) => {
            eprintln!("walk error: {}", e);
            return not_found_response(&state);
        }
    };

    let items: Vec<(String, String, String)> = tree
        .agents
        .iter()
        .map(|agent| {
            (
                format!("/claude-agents/{}", agent.file_slug),
                agent.name.clone(),
                agent.description.clone(),
            )
        })
        .collect();

    let content = category_index_html(
        "Agents",
        "Subagent definitions under ~/.claude/agents/.",
        &items,
    );
    let page = render_shell(&state.shell_html, "Agents — CCResDoc", &content);
    html_200(page)
}

// ---------------------------------------------------------------------------
// Dynamic content handlers
// ---------------------------------------------------------------------------

fn html_200(body: String) -> Response<Body> {
    Response::builder()
        .status(StatusCode::OK)
        .header(
            header::CONTENT_TYPE,
            HeaderValue::from_static("text/html; charset=utf-8"),
        )
        .body(Body::from(body))
        .unwrap()
}

/// GET /claude-md/{slug}
pub(crate) async fn claude_md_page(
    State(state): State<AppState>,
    AxumPath(slug): AxumPath<String>,
) -> Response<Body> {
    let tree = match state.walk() {
        Ok(t) => t,
        Err(e) => {
            eprintln!("walk error: {}", e);
            return not_found_response(&state);
        }
    };

    let item: Option<&ClaudeMdItem> = tree.claude_mds.iter().find(|x| x.slug == slug);
    match item {
        None => not_found_response(&state),
        Some(item) => {
            let rendered = match render_md_to_html(&item.raw_content) {
                Ok(h) => h,
                Err(e) => {
                    eprintln!("render error: {}", e);
                    return not_found_response(&state);
                }
            };
            let title = format!("{} — CCResDoc", item.display_path);
            let page = render_shell(&state.shell_html, &title, &rendered);
            html_200(page)
        }
    }
}

/// GET /claude-commands/{name}
pub(crate) async fn claude_command_page(
    State(state): State<AppState>,
    AxumPath(name): AxumPath<String>,
) -> Response<Body> {
    let tree = match state.walk() {
        Ok(t) => t,
        Err(e) => {
            eprintln!("walk error: {}", e);
            return not_found_response(&state);
        }
    };

    let item: Option<&CommandItem> = tree.commands.iter().find(|x| x.name == name);
    match item {
        None => not_found_response(&state),
        Some(item) => {
            let rendered = match render_md_to_html(&item.raw_content) {
                Ok(h) => h,
                Err(e) => {
                    eprintln!("render error: {}", e);
                    return not_found_response(&state);
                }
            };
            let title = format!("{} — CCResDoc", item.name);
            let page = render_shell(&state.shell_html, &title, &rendered);
            html_200(page)
        }
    }
}

/// GET /claude-skills/{dir}
pub(crate) async fn claude_skill_page(
    State(state): State<AppState>,
    AxumPath(dir): AxumPath<String>,
) -> Response<Body> {
    let tree = match state.walk() {
        Ok(t) => t,
        Err(e) => {
            eprintln!("walk error: {}", e);
            return not_found_response(&state);
        }
    };

    let item: Option<&SkillItem> = tree.skills.iter().find(|x| x.dir == dir);
    match item {
        None => not_found_response(&state),
        Some(item) => {
            let mut body_parts = render_md_to_html(&item.raw_content).unwrap_or_default();

            // Append file-tree sections
            if !item.references.is_empty() {
                body_parts.push_str("<h2>References</h2><ul class=\"skill-file-tree\">");
                for r in &item.references {
                    body_parts.push_str(&format!(
                        "<li><a href=\"/claude-skills/{}/ref-{}\">{}</a></li>",
                        html_escape(&item.dir),
                        html_escape(&r.name),
                        html_escape(&r.title),
                    ));
                }
                body_parts.push_str("</ul>");
            }
            if !item.script_files.is_empty() {
                body_parts.push_str("<h2>Scripts</h2><ul class=\"skill-file-tree\">");
                for f in &item.script_files {
                    if f.is_markdown {
                        let label = f.title.as_deref().unwrap_or(&f.filename);
                        let stem = f.filename.trim_end_matches(".md");
                        body_parts.push_str(&format!(
                            "<li><a href=\"/claude-skills/{}/script-{}\">{}</a></li>",
                            html_escape(&item.dir),
                            html_escape(stem),
                            html_escape(label),
                        ));
                    } else {
                        body_parts.push_str(&format!("<li>{}</li>", html_escape(&f.filename),));
                    }
                }
                body_parts.push_str("</ul>");
            }
            if !item.asset_files.is_empty() {
                body_parts.push_str("<h2>Assets</h2><ul class=\"skill-file-tree\">");
                for f in &item.asset_files {
                    if f.is_markdown {
                        let label = f.title.as_deref().unwrap_or(&f.filename);
                        let stem = f.filename.trim_end_matches(".md");
                        body_parts.push_str(&format!(
                            "<li><a href=\"/claude-skills/{}/asset-{}\">{}</a></li>",
                            html_escape(&item.dir),
                            html_escape(stem),
                            html_escape(label),
                        ));
                    } else {
                        body_parts.push_str(&format!("<li>{}</li>", html_escape(&f.filename),));
                    }
                }
                body_parts.push_str("</ul>");
            }

            let title = format!("{} — CCResDoc", item.name);
            let page = render_shell(&state.shell_html, &title, &body_parts);
            html_200(page)
        }
    }
}

/// GET /claude-skills/{dir}/{subpage}
///
/// Dispatches based on the `subpage` prefix:
///   - `ref-{name}`    → skill reference
///   - `script-{name}` → skill script (.md only)
///   - `asset-{name}`  → skill asset (.md only)
///
/// Axum/matchit does not support literal prefixes mixed with named parameters
/// within a single path segment (e.g. "ref-{slug}"), so we capture the whole
/// segment and parse the prefix ourselves.
pub(crate) async fn claude_skill_subpage(
    State(state): State<AppState>,
    AxumPath((dir, subpage)): AxumPath<(String, String)>,
) -> Response<Body> {
    let tree = match state.walk() {
        Ok(t) => t,
        Err(e) => {
            eprintln!("walk error: {}", e);
            return not_found_response(&state);
        }
    };

    let skill = match tree.skills.iter().find(|x| x.dir == dir) {
        None => return not_found_response(&state),
        Some(s) => s,
    };

    if let Some(slug) = subpage.strip_prefix("ref-") {
        // Reference sub-page
        let reference = match skill.references.iter().find(|r| r.name == slug) {
            None => return not_found_response(&state),
            Some(r) => r,
        };
        let rendered = match render_md_to_html(&reference.raw_content) {
            Ok(h) => h,
            Err(e) => {
                eprintln!("render error: {}", e);
                return not_found_response(&state);
            }
        };
        let title = format!("{} — CCResDoc", reference.title);
        let page = render_shell(&state.shell_html, &title, &rendered);
        return html_200(page);
    }

    if let Some(slug) = subpage.strip_prefix("script-") {
        // Script sub-page (.md only)
        let file = skill
            .script_files
            .iter()
            .find(|f| f.filename.trim_end_matches(".md") == slug && f.is_markdown);
        match file {
            None => return not_found_response(&state),
            Some(f) => {
                let raw = f.raw_content.as_deref().unwrap_or("");
                let rendered = match render_md_to_html(raw) {
                    Ok(h) => h,
                    Err(e) => {
                        eprintln!("render error: {}", e);
                        return not_found_response(&state);
                    }
                };
                let label = f.title.as_deref().unwrap_or(&f.filename);
                let title = format!("{} — CCResDoc", label);
                let page = render_shell(&state.shell_html, &title, &rendered);
                return html_200(page);
            }
        }
    }

    if let Some(slug) = subpage.strip_prefix("asset-") {
        // Asset sub-page (.md only)
        let file = skill
            .asset_files
            .iter()
            .find(|f| f.filename.trim_end_matches(".md") == slug && f.is_markdown);
        match file {
            None => return not_found_response(&state),
            Some(f) => {
                let raw = f.raw_content.as_deref().unwrap_or("");
                let rendered = match render_md_to_html(raw) {
                    Ok(h) => h,
                    Err(e) => {
                        eprintln!("render error: {}", e);
                        return not_found_response(&state);
                    }
                };
                let label = f.title.as_deref().unwrap_or(&f.filename);
                let title = format!("{} — CCResDoc", label);
                let page = render_shell(&state.shell_html, &title, &rendered);
                return html_200(page);
            }
        }
    }

    // Unknown subpage prefix
    not_found_response(&state)
}

/// GET /claude-agents/{file}
pub(crate) async fn claude_agent_page(
    State(state): State<AppState>,
    AxumPath(file_slug): AxumPath<String>,
) -> Response<Body> {
    let tree = match state.walk() {
        Ok(t) => t,
        Err(e) => {
            eprintln!("walk error: {}", e);
            return not_found_response(&state);
        }
    };

    let item: Option<&AgentItem> = tree.agents.iter().find(|x| x.file_slug == file_slug);
    match item {
        None => not_found_response(&state),
        Some(item) => {
            let mut content_parts = String::new();

            // Model badge if present
            if !item.model.is_empty() {
                content_parts.push_str(&format!(
                    "<p class=\"agent-model-badge\">Model: <code>{}</code></p>\n",
                    html_escape(&item.model)
                ));
            }

            let rendered = match render_md_to_html(&item.raw_content) {
                Ok(h) => h,
                Err(e) => {
                    eprintln!("render error: {}", e);
                    return not_found_response(&state);
                }
            };
            content_parts.push_str(&rendered);

            let title = format!("{} — CCResDoc", item.name);
            let page = render_shell(&state.shell_html, &title, &content_parts);
            html_200(page)
        }
    }
}

// ---------------------------------------------------------------------------
// Catch-all static fallback
// ---------------------------------------------------------------------------

/// Fallback: serve `dist/{path}` if it exists, else 404.
/// Paths starting with `/_shell` are always 404.
pub(crate) async fn static_fallback(
    State(state): State<AppState>,
    axum::extract::OriginalUri(uri): axum::extract::OriginalUri,
) -> Response<Body> {
    let raw_path = uri.path();

    // Guard: /_shell* must always 404
    if raw_path.starts_with("/_shell") {
        return not_found_response(&state);
    }

    let relative = raw_path.trim_start_matches('/');
    if relative.is_empty() {
        return not_found_response(&state);
    }

    // Per-segment validation. Reject any segment that decodes to "." or "..", is empty,
    // or contains a path separator. percent_decode is needed because plain
    // `relative.contains("..")` can be bypassed by `%2e%2e/etc/passwd`.
    let mut safe = std::path::PathBuf::new();
    for raw_segment in relative.split('/') {
        let decoded = percent_encoding::percent_decode_str(raw_segment)
            .decode_utf8()
            .ok();
        let segment = match decoded.as_deref() {
            Some(s) => s,
            None => return not_found_response(&state),
        };
        if segment.is_empty()
            || segment == "."
            || segment == ".."
            || segment.contains('/')
            || segment.contains('\\')
        {
            return not_found_response(&state);
        }
        safe.push(segment);
    }

    let file_path = state.dist_dir.join(&safe);
    // Defense in depth: ensure the resolved path stays inside dist_dir even after
    // any future symlink-aware joining.
    let canonical_dist = std::fs::canonicalize(&state.dist_dir);
    if let Ok(dist_root) = canonical_dist {
        if let Ok(resolved) = std::fs::canonicalize(&file_path) {
            if !resolved.starts_with(&dist_root) {
                return not_found_response(&state);
            }
        }
    }

    if file_path.is_file() {
        static_file_response(&file_path, None)
    } else {
        not_found_response(&state)
    }
}
