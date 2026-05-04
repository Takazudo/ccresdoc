//! Axum HTTP server for CCResDoc.
//!
//! Entry points:
//!   - [`serve`]              — run forever
//!   - [`serve_with_shutdown`] — run until a `Future` resolves (used by Tauri)

mod handlers;
mod manifest;
pub mod routes;

use std::future::Future;
use std::path::PathBuf;

pub use manifest::{Manifest, ManifestCategory, ManifestItem};

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Configuration for the CCResDoc server.
pub struct ServerConfig {
    /// TCP port to bind.
    pub port: u16,
    /// Root of `$HOME/.claude/` — where commands/, skills/, agents/ live.
    pub claude_dir: PathBuf,
    /// Project root for CLAUDE.md discovery. Must equal `$HOME/.claude`, not `$HOME`.
    pub project_root: PathBuf,
    /// Path to the compiled `app/dist/` tree (index.html, assets/, _shell/, …).
    pub dist_dir: PathBuf,
}

/// Run the server forever.
pub async fn serve(config: ServerConfig) -> anyhow::Result<()> {
    serve_with_shutdown(config, std::future::pending()).await
}

/// Run the server until `shutdown` resolves.
pub async fn serve_with_shutdown(
    config: ServerConfig,
    shutdown: impl Future<Output = ()> + Send + 'static,
) -> anyhow::Result<()> {
    let port = config.port;
    let router = routes::build_router(config)?;
    let bind_addr = std::net::SocketAddr::from(([127, 0, 0, 1], port));

    let listener = tokio::net::TcpListener::bind(bind_addr).await?;
    axum::serve(listener, router.into_make_service())
        .with_graceful_shutdown(shutdown)
        .await?;
    Ok(())
}

/// Build the raw `axum::Router` from a `ServerConfig` — exposed for integration tests
/// that use `tower::ServiceExt::oneshot` (no port binding required).
pub fn routes_for_test(config: ServerConfig) -> anyhow::Result<axum::Router> {
    routes::build_router(config)
}
