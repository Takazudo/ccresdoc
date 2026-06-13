//! Watcher integration tests.
//!
//! These exercise the `notify`-based watcher against a temp `~/.claude`
//! fixture: adding, editing, and removing files must trigger a debounced
//! regeneration. We use a channel from the `on_change` callback to observe
//! regenerations deterministically (with a generous timeout).

use std::fs;
use std::path::Path;
use std::sync::mpsc;
use std::time::Duration;

use ccresdoc_claude_md::{watch, Config, WatchEvent};

fn write(base: &Path, rel: &str, content: &str) {
    let full = base.join(rel);
    if let Some(parent) = full.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(full, content).unwrap();
}

fn config_for(claude_dir: &Path, docs_dir: &Path) -> Config {
    Config {
        claude_dir: claude_dir.to_path_buf(),
        project_root: claude_dir.to_path_buf(),
        docs_dir: docs_dir.to_path_buf(),
    }
}

/// Wait up to `timeout` for the next successful regeneration, returning its
/// command count. Panics on timeout or a regeneration error.
fn wait_regen(rx: &mpsc::Receiver<WatchEvent>, timeout: Duration) -> usize {
    match rx.recv_timeout(timeout) {
        Ok(WatchEvent::Regenerated(report)) => report.commands,
        Ok(WatchEvent::Error(e)) => panic!("regeneration error: {e}"),
        Err(_) => panic!("timed out waiting for regeneration"),
    }
}

#[test]
fn watcher_regenerates_on_add_edit_remove() {
    let tmp = tempfile::TempDir::new().unwrap();
    let claude = tmp.path().join("dot-claude");
    let docs = tmp.path().join("docs");

    // Seed with one command so the dirs exist before watching.
    write(&claude, "CLAUDE.md", "root");
    write(&claude, "commands/first.md", "---\ndescription: first\n---\nbody");

    let (tx, rx) = mpsc::channel::<WatchEvent>();
    let handle = watch(
        config_for(&claude, &docs),
        // Short debounce keeps the test fast while still proving coalescing.
        Duration::from_millis(150),
        move |ev| {
            let _ = tx.send(ev);
        },
    )
    .expect("watch failed to start");

    // --- ADD a command ---
    write(&claude, "commands/second.md", "---\ndescription: second\n---\nbody");
    let count = wait_regen(&rx, Duration::from_secs(10));
    assert_eq!(count, 2, "after add, expected 2 commands");
    assert!(docs.join("claude-commands/second.mdx").exists());

    // --- EDIT a command (change its description) ---
    write(
        &claude,
        "commands/second.md",
        "---\ndescription: edited\n---\nnew body",
    );
    let _ = wait_regen(&rx, Duration::from_secs(10));
    let edited = fs::read_to_string(docs.join("claude-commands/second.mdx")).unwrap();
    assert!(
        edited.contains("description: \"edited\""),
        "edit should be reflected in regenerated MDX"
    );

    // --- REMOVE a command ---
    fs::remove_file(claude.join("commands/second.md")).unwrap();
    let count = wait_regen(&rx, Duration::from_secs(10));
    assert_eq!(count, 1, "after remove, expected 1 command");
    assert!(
        !docs.join("claude-commands/second.mdx").exists(),
        "removed command's MDX should be gone after regeneration"
    );

    handle.stop();
}

#[test]
fn watcher_stops_cleanly_on_drop() {
    let tmp = tempfile::TempDir::new().unwrap();
    let claude = tmp.path().join("dot-claude");
    let docs = tmp.path().join("docs");
    write(&claude, "CLAUDE.md", "root");

    let (tx, _rx) = mpsc::channel::<WatchEvent>();
    let handle = watch(
        config_for(&claude, &docs),
        Duration::from_millis(100),
        move |ev| {
            let _ = tx.send(ev);
        },
    )
    .expect("watch failed to start");

    // Dropping the handle must join the worker thread without hanging.
    drop(handle);
}
