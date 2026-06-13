//! Error type shared across the crate.

use std::path::PathBuf;

use thiserror::Error;

/// Errors returned by [`crate::generate`] and the watcher.
#[derive(Debug, Error)]
pub enum GenerateError {
    /// `project_root` resolved to the user's home directory. The CLAUDE.md walk
    /// MUST stay scoped to `~/.claude` (zudolab/zudo-doc#2115) so it never
    /// crawls all of `$HOME`.
    #[error("project_root is too broad: {0:?}. Pass a specific directory such as ~/.claude, not $HOME.")]
    ProjectRootTooBroad(PathBuf),

    /// A `Config` field is malformed (e.g. a non-absolute path). Distinct from
    /// [`GenerateError::Watch`] so the host can tell a bad config from a runtime
    /// watcher failure.
    #[error("invalid config: {0}")]
    InvalidConfig(String),

    /// A discovered resource slug collided with the reserved `index` name used
    /// for the category metadata file.
    #[error("reserved slug conflict: {0}")]
    ReservedSlug(String),

    /// Two resources produced the same output slug.
    #[error("slug collision: {0}")]
    SlugCollision(String),

    /// An I/O error reading from `~/.claude` or writing the MDX tree.
    #[error("I/O error at {path:?}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    /// The file watcher backend failed to start or to deliver an event.
    #[error("watch error: {0}")]
    Watch(String),
}

pub type Result<T> = std::result::Result<T, GenerateError>;
