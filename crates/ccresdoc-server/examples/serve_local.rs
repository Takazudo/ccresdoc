//! Manual smoke-test: start the server against the real $HOME/.claude/ and app/dist/.
//!
//! Usage:
//!   cargo run -p ccresdoc-server --example serve_local
//!
//! Then:
//!   curl localhost:4892/claude-md/root
//!   curl localhost:4892/api/manifest.json

use std::path::PathBuf;

use ccresdoc_server::{serve, ServerConfig};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let home = PathBuf::from(std::env::var("HOME").expect("$HOME must be set"));
    let claude_dir = home.join(".claude");

    // project_root must equal $HOME/.claude (not $HOME)
    let project_root = claude_dir.clone();

    // app/dist/ is two levels above CARGO_MANIFEST_DIR:
    //   CARGO_MANIFEST_DIR = <workspace>/crates/ccresdoc-server
    //   .parent()          = <workspace>/crates
    //   .parent()          = <workspace>          ← workspace root
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let workspace_root = manifest_dir
        .parent() // <workspace>/crates
        .and_then(|p| p.parent()) // <workspace>
        .expect("cannot resolve workspace root");
    let dist_dir = workspace_root.join("app").join("dist");

    if !dist_dir.exists() {
        eprintln!(
            "dist/ not found at {:?}. Build the app first: cd app && pnpm build",
            dist_dir
        );
        std::process::exit(1);
    }

    let config = ServerConfig {
        port: 4892,
        claude_dir,
        project_root,
        dist_dir,
    };

    println!("Listening on http://127.0.0.1:4892 — press Ctrl+C to stop");
    serve(config).await
}
