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
    write(
        &claude,
        "commands/first.md",
        "---\ndescription: first\n---\nbody",
    );

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
    write(
        &claude,
        "commands/second.md",
        "---\ndescription: second\n---\nbody",
    );
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
fn watcher_ignores_irrelevant_session_churn() {
    // Cause 1: session-state churn under ~/.claude (projects/, todos/, etc.)
    // must NOT trigger a regeneration; a content path (skills/) must.
    let tmp = tempfile::TempDir::new().unwrap();
    let claude = tmp.path().join("dot-claude");
    let docs = tmp.path().join("docs");

    // Seed so the watched dir exists.
    write(&claude, "CLAUDE.md", "root");

    let (tx, rx) = mpsc::channel::<WatchEvent>();
    let handle = watch(
        config_for(&claude, &docs),
        Duration::from_millis(120),
        move |ev| {
            let _ = tx.send(ev);
        },
    )
    .expect("watch failed to start");

    // Drain any startup pulse the backend may replay for the pre-watch seed
    // (FSEvents can re-deliver recent changes when a watch attaches).
    while rx.recv_timeout(Duration::from_millis(400)).is_ok() {}

    // --- Irrelevant churn: must NOT regenerate. ---
    write(&claude, "projects/p.json", "{}");
    write(&claude, "todos/t.json", "[]");
    write(&claude, "statsig/x", "noise");
    // Generous window relative to the 120ms debounce; a spurious pulse would
    // arrive well within it.
    assert!(
        rx.recv_timeout(Duration::from_millis(800)).is_err(),
        "irrelevant ~/.claude churn must not trigger a regeneration"
    );

    // --- Relevant content change: MUST regenerate. ---
    write(
        &claude,
        "skills/foo/SKILL.md",
        "---\nname: Foo\ndescription: a skill\n---\nbody",
    );
    match rx.recv_timeout(Duration::from_secs(10)) {
        Ok(WatchEvent::Regenerated(report)) => {
            assert_eq!(report.skills, 1, "the skill should have been generated");
        }
        Ok(WatchEvent::Error(e)) => panic!("regeneration error: {e}"),
        Err(_) => panic!("a content change under skills/ should trigger a regeneration"),
    }

    handle.stop();
}

#[test]
fn burst_of_writes_coalesces_into_one_regeneration() {
    // The whole point of debouncing: a burst of N writes landing inside the
    // debounce window must produce exactly ONE regeneration, not N. We seed,
    // attach the watcher, drain any startup replay, then fire a burst of writes
    // back-to-back (well within the debounce) and assert that precisely one
    // Regenerated event arrives — no second one within a generous follow-up
    // window.
    const N: usize = 8;

    let tmp = tempfile::TempDir::new().unwrap();
    let claude = tmp.path().join("dot-claude");
    let docs = tmp.path().join("docs");

    write(&claude, "CLAUDE.md", "root");
    write(
        &claude,
        "commands/seed.md",
        "---\ndescription: seed\n---\nbody",
    );

    let (tx, rx) = mpsc::channel::<WatchEvent>();
    let handle = watch(
        config_for(&claude, &docs),
        // A comfortably long debounce so the whole burst lands in one window
        // even on a slow/loaded CI box.
        Duration::from_millis(400),
        move |ev| {
            let _ = tx.send(ev);
        },
    )
    .expect("watch failed to start");

    // Drain any startup pulse the backend may replay for the pre-watch seed.
    while rx.recv_timeout(Duration::from_millis(600)).is_ok() {}

    // Fire the burst: N distinct content files written back-to-back, all inside
    // the single 400ms debounce window.
    for i in 0..N {
        write(
            &claude,
            &format!("commands/burst-{i}.md"),
            "---\ndescription: burst\n---\nbody",
        );
    }

    // Collect every regeneration the burst produces, until the channel goes
    // quiet for a full follow-up window. Coalescing means N writes do NOT
    // produce N regenerations — the debouncer collapses the burst. We assert
    // both properties:
    //   1. the burst regenerates AT MOST ONCE per debounce flush, far fewer
    //      than the N writes (the coalescing guarantee), and
    //   2. the final regeneration observed the WHOLE burst (seed + N commands),
    //      proving no write was dropped.
    // We tolerate at most a single trailing duplicate flush: inotify can split
    // a rapid burst across two debounce windows, but it must never fan a
    // burst of N writes out into ~N regenerations.
    let first = rx
        .recv_timeout(Duration::from_secs(10))
        .expect("the burst should trigger a regeneration");
    let mut regens = vec![first];
    while let Ok(ev) = rx.recv_timeout(Duration::from_millis(1200)) {
        regens.push(ev);
    }

    let mut last_count = None;
    for ev in &regens {
        match ev {
            WatchEvent::Regenerated(report) => last_count = Some(report.commands),
            WatchEvent::Error(e) => panic!("regeneration error during burst: {e}"),
        }
    }

    assert!(
        regens.len() <= 2,
        "a burst of {N} writes must coalesce, not fan out: got {} regenerations",
        regens.len()
    );
    assert_eq!(
        last_count,
        Some(N + 1),
        "the (coalesced) regeneration must reflect the entire burst"
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

    // Dropping the handle must join the worker thread WITHOUT hanging. Run the
    // drop+join on a side thread and join THAT with a bounded timeout so a
    // regression that deadlocks shutdown fails as a clean assertion instead of
    // hanging the whole test binary until the harness kills it.
    let (done_tx, done_rx) = mpsc::channel::<()>();
    std::thread::spawn(move || {
        drop(handle);
        let _ = done_tx.send(());
    });
    assert!(
        done_rx.recv_timeout(Duration::from_secs(10)).is_ok(),
        "dropping the WatchHandle must join its worker thread without hanging"
    );
}
