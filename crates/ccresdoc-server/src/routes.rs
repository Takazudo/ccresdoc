//! Router assembly — all routes wired together.
//!
//! Note: axum 0.7 uses `:param` syntax for named parameters and `*param` for
//! catch-all (wildcard) parameters.  The curly-brace `{param}` syntax is an
//! axum 0.8+ extension — do NOT use it here.

use axum::{routing::get, Router};

use crate::{
    handlers::{
        api_manifest, assets, claude_agent_page, claude_agents_index, claude_command_page,
        claude_commands_index, claude_md_index, claude_md_page, claude_skill_page,
        claude_skill_subpage, claude_skills_index, home, not_found_page, ready, shell_forbidden,
        sidebar_js, static_fallback, AppState,
    },
    ServerConfig,
};

/// Build the axum Router from a `ServerConfig`.
/// This function does I/O only to read `dist/_shell/index.html` at startup.
pub fn build_router(config: ServerConfig) -> anyhow::Result<Router> {
    let state = AppState::load(config.claude_dir, config.project_root, config.dist_dir)?;

    let router = Router::new()
        // Simple statics
        .route("/", get(home))
        .route("/404", get(not_found_page))
        .route("/___ready", get(ready))
        .route("/ccresdoc-sidebar.js", get(sidebar_js))
        // Assets with long cache (catch-all wildcard)
        .route("/assets/*path", get(assets))
        // Shell is forbidden — always 404
        .route("/_shell", get(shell_forbidden))
        .route("/_shell/*path", get(shell_forbidden))
        // API
        .route("/api/manifest.json", get(api_manifest))
        // CLAUDE.md pages
        .route("/claude-md/", get(claude_md_index))
        .route("/claude-md/:slug", get(claude_md_page))
        // Command pages
        .route("/claude-commands/", get(claude_commands_index))
        .route("/claude-commands/:name", get(claude_command_page))
        // Skill pages — sub-pages are dispatched by prefix in handler (axum/matchit
        // does not support literal-prefix + param within a single path segment, e.g.
        // "ref-:slug", so we capture the whole segment and parse the prefix ourselves)
        .route("/claude-skills/", get(claude_skills_index))
        .route("/claude-skills/:dir/:subpage", get(claude_skill_subpage))
        .route("/claude-skills/:dir", get(claude_skill_page))
        // Agent pages
        .route("/claude-agents/", get(claude_agents_index))
        .route("/claude-agents/:file", get(claude_agent_page))
        // Catch-all fallback
        .fallback(get(static_fallback))
        .with_state(state);

    Ok(router)
}
