//! # ccresdoc-claude-md
//!
//! Generates **zudo-doc-compatible MDX** from a `~/.claude/` resource tree and
//! watches it for changes. This is the node-free live-update engine for
//! CCResDoc: the Rust watcher writes MDX into the app content dir, and
//! `zfb dev`'s content-watch HMRs the result. (`zfb`'s `extraWatchPaths` does
//! NOT re-run `preBuild`, so generation + watch must live here in Rust.)
//!
//! ## What it emits
//!
//! Per the Wave 1 content contract (`app/CLAUDE.md`), it writes:
//!
//! ```text
//! <docs_dir>/
//!   claude/index.mdx            overview (sidebar_position 899, category_no_page, <CategoryNav/>)
//!   claude-md/index.mdx         category header (900)
//!   claude-md/global.mdx        ~/.claude/CLAUDE.md
//!   claude-md/project-<slug>.mdx  nested CLAUDE.md files
//!   claude-commands/index.mdx   category header (901)
//!   claude-commands/<name>.mdx  one file per command
//!   claude-skills/index.mdx     category header (902)
//!   claude-skills/<dir>.mdx     one file per skill
//!   claude-skills/<dir>--{ref,script,asset}-<name>.mdx  unlisted skill sub-pages
//!   claude-agents/index.mdx     category header (903)
//!   claude-agents/<name>.mdx    one file per agent
//! ```
//!
//! The MDX escaping (`escape_for_mdx`) and frontmatter shape are faithful ports
//! of zudo-doc's `escape-for-mdx.ts` / `generate.ts`, so output renders
//! identically under zudo-doc.
//!
//! ## Public API
//!
//! ```no_run
//! use std::path::PathBuf;
//! use std::time::Duration;
//! use ccresdoc_claude_md::{Config, generate, watch, WatchEvent, DEFAULT_DEBOUNCE};
//!
//! // The Tauri host (Wave 3) resolves these ABSOLUTE paths and passes them in.
//! let config = Config {
//!     claude_dir: PathBuf::from("/Users/me/.claude"),
//!     project_root: PathBuf::from("/Users/me/.claude"),
//!     docs_dir: PathBuf::from("/path/to/app/src/content/docs"),
//! };
//!
//! // One-shot generation at boot:
//! let report = generate(&config).expect("generate failed");
//! println!("{} skills, {} commands", report.skills, report.commands);
//!
//! // Live watch: regenerate on every change under ~/.claude (debounced + serialized):
//! let handle = watch(config, DEFAULT_DEBOUNCE, |event| match event {
//!     WatchEvent::Regenerated(report) => eprintln!("regenerated: {report:?}"),
//!     WatchEvent::Error(e) => eprintln!("regeneration failed: {e}"),
//! }).expect("watch failed");
//!
//! // ... app runs ...
//! handle.stop(); // or drop(handle)
//! ```
//!
//! ### Exact signatures
//!
//! - `fn generate(config: &Config) -> Result<GenerateReport, GenerateError>`
//! - `fn watch<F>(config: Config, debounce: Duration, on_change: F) -> Result<WatchHandle, GenerateError>`
//!   where `F: Fn(WatchEvent) + Send + 'static`
//! - `struct Config { claude_dir: PathBuf, project_root: PathBuf, docs_dir: PathBuf }`
//! - `struct GenerateReport { claude_md: usize, commands: usize, skills: usize, agents: usize }`
//! - `enum WatchEvent { Regenerated(GenerateReport), Error(GenerateError) }`
//! - `struct WatchHandle` — keeps the watch alive; `stop(self)` or `Drop` ends it
//! - `const DEFAULT_DEBOUNCE: Duration` — 300ms
//!
//! ## Safety: scoped walk
//!
//! `project_root` MUST be `~/.claude` (or another specific directory), NOT
//! `$HOME`. Passing `$HOME` returns [`GenerateError::ProjectRootTooBroad`] — the
//! CLAUDE.md walk must never escape `~/.claude` (zudolab/zudo-doc#2115).
//! Symlinks are NOT followed during the CLAUDE.md walk (skills contain
//! symlinks that could point back into the tree or out to a slow mount).

mod error;
mod escape;
mod generate;
mod walk;
mod watch;

use std::path::PathBuf;

pub use error::{GenerateError, Result};
pub use generate::GenerateReport;
pub use watch::{watch, WatchEvent, WatchHandle, DEFAULT_DEBOUNCE};

/// Resolved, absolute paths the generator/watcher operate on.
///
/// The Tauri host (Wave 3) resolves these and passes them in. All three must
/// be absolute; the walk is scoped to `project_root` (which must be
/// `~/.claude`, not `$HOME`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    /// Absolute path to `~/.claude` — root for `commands/`, `skills/`,
    /// `agents/` discovery.
    pub claude_dir: PathBuf,
    /// Absolute root for the recursive CLAUDE.md walk. Must be `~/.claude`
    /// (NOT `$HOME`). Usually equal to `claude_dir`.
    pub project_root: PathBuf,
    /// Absolute path to the zudo-doc content root
    /// (`app/src/content/docs`). The generator writes the `claude*/` tree here.
    pub docs_dir: PathBuf,
}

impl Config {
    /// Validate the config up front (before any walk or watch): paths must be
    /// absolute, and `project_root` must not be `$HOME` — the CLAUDE.md walk is
    /// scoped to `~/.claude` (zudolab/zudo-doc#2115). Running this in both
    /// `generate()` and `watch()` means `watch()` rejects a bad config
    /// synchronously rather than failing later as a `WatchEvent::Error`.
    pub(crate) fn validate(&self) -> Result<()> {
        for (label, p) in [
            ("claude_dir", &self.claude_dir),
            ("project_root", &self.project_root),
            ("docs_dir", &self.docs_dir),
        ] {
            if !p.is_absolute() {
                return Err(GenerateError::InvalidConfig(format!(
                    "{label} must be an absolute path, got {p:?}"
                )));
            }
        }

        // Refuse project_root that is $HOME, an ancestor of $HOME, or any
        // other broad directory. Both sides are canonicalized so trailing
        // slashes / symlinks don't matter.
        //
        // Allowed: the canonicalized root is strictly inside $HOME (i.e.
        // $HOME is a prefix of root but root != $HOME), OR the root path ends
        // in the component ".claude" (e.g. ~/.claude itself).
        //
        // Rejected: root == $HOME, root is an ancestor of $HOME (e.g. "/",
        // "/Users"), or root is a broad non-.claude dir at the same level.
        if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
            let pr = self
                .project_root
                .canonicalize()
                .unwrap_or_else(|_| self.project_root.clone());
            let home_canon = home.canonicalize().unwrap_or(home);

            // Check 1: pr == $HOME exactly.
            let is_home = pr == home_canon;

            // Check 2: $HOME starts_with pr → pr is an ancestor of $HOME
            // (e.g. pr == "/" or "/Users" or "/home").
            let is_ancestor_of_home = home_canon.starts_with(&pr);

            // Check 3: pr is directly inside $HOME but is not named ".claude"
            // (e.g. pr == $HOME/repos — too broad; $HOME/.claude is fine).
            let last_component = pr.file_name().and_then(|n| n.to_str()).unwrap_or("");
            let is_home_child_non_claude = pr.parent().map(|p| p == home_canon).unwrap_or(false)
                && last_component != ".claude";

            if is_home || is_ancestor_of_home || is_home_child_non_claude {
                return Err(GenerateError::ProjectRootTooBroad(
                    self.project_root.clone(),
                ));
            }
        }
        Ok(())
    }
}

/// Generate the full MDX tree once (used at boot).
///
/// Walks `~/.claude` per `config` and writes the `claude*/` MDX tree under
/// `config.docs_dir`, returning counts of what was emitted.
///
/// Returns [`GenerateError::ProjectRootTooBroad`] if `project_root` is `$HOME`.
pub fn generate(config: &Config) -> Result<GenerateReport> {
    generate::run(config)
}
