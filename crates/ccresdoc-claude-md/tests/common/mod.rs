#![allow(dead_code)]

use std::path::{Path, PathBuf};

/// Resolve a fixture at runtime: a shared Cargo target dir can reuse a cached
/// test binary whose compile-time `CARGO_MANIFEST_DIR` points at a deleted worktree.
pub fn find_fixture(start: &Path, name: &str) -> PathBuf {
    for ancestor in start.ancestors() {
        for base in ["tests/fixtures", "crates/ccresdoc-claude-md/tests/fixtures"] {
            let candidate = ancestor.join(base).join(name);
            if candidate.is_dir() {
                return candidate.canonicalize().unwrap_or_else(|e| {
                    panic!("failed to canonicalize fixture {candidate:?}: {e}")
                });
            }
        }
    }

    panic!("fixture not found: {name:?} (searched from {start:?} and its ancestors)")
}

pub fn fixture(name: &str) -> PathBuf {
    let cwd = std::env::current_dir().expect("failed to resolve the test working directory");
    find_fixture(&cwd, name)
}
