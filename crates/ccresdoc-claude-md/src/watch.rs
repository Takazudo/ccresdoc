//! `notify`-based file watcher for `~/.claude`.
//!
//! On any change beneath `claude_dir` (and `project_root`, if different), the
//! watcher regenerates the MDX tree. Regenerations are:
//! - **debounced** (~300ms) via `notify-debouncer-full`, so a burst of editor
//!   writes coalesces into a single regeneration;
//! - **serialized** through a mutex, so two regenerations never write the same
//!   MDX concurrently.
//!
//! `zfb dev`'s content-watch then HMRs the regenerated MDX. `extraWatchPaths`
//! does NOT re-run zfb's `preBuild`, which is exactly why generation lives here
//! in Rust rather than in a zfb prebuild step.

use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::thread::JoinHandle;
use std::time::Duration;

use notify::{RecursiveMode, Watcher};
use notify_debouncer_full::{new_debouncer, DebounceEventResult};

use crate::error::{GenerateError, Result};
use crate::generate;
use crate::{Config, GenerateReport};

/// Default debounce window for coalescing filesystem-event bursts.
pub const DEFAULT_DEBOUNCE: Duration = Duration::from_millis(300);

/// Resource subdirectories of `claude_dir` whose contents feed the generated
/// docs. A change anywhere under one of these is content-relevant.
const RESOURCE_SUBDIRS: [&str; 3] = ["commands", "skills", "agents"];

/// Is a changed path part of the generated docs' source content?
///
/// The watcher subscribes to `claude_dir` (`~/.claude`) **recursively**, but
/// that directory also holds high-churn session state — `projects/`, `todos/`,
/// `statsig/`, `shell-snapshots/`, `history*`, `logs/`, `ide/`, `.git/`,
/// `.credentials*`, etc. — that changes whenever ANY Claude Code session runs
/// and is NOT part of the docs. Regenerating on that churn keeps `zfb dev` in a
/// perpetual rebuild loop, so we **allowlist** only the paths that actually
/// drive generation:
///
/// - any `CLAUDE.md` file at any depth (the CLAUDE.md hierarchy), under either
///   `claude_dir` or `project_root`;
/// - anything under `<claude_dir>/commands/`, `<claude_dir>/skills/`, or
///   `<claude_dir>/agents/`.
///
/// Everything else is ignored. Kept as a pure fn so it is unit-testable in
/// isolation from the `notify` machinery.
fn is_content_relevant(path: &Path, claude_dir: &Path, project_root: &Path) -> bool {
    // CLAUDE.md hierarchy: match the filename at any depth, but only inside one
    // of the watched roots (so an unrelated CLAUDE.md elsewhere can't qualify).
    if path.file_name().is_some_and(|n| n == "CLAUDE.md")
        && (path.starts_with(claude_dir) || path.starts_with(project_root))
    {
        return true;
    }

    // Resource families: anything under <claude_dir>/{commands,skills,agents}/.
    RESOURCE_SUBDIRS
        .iter()
        .any(|sub| path.starts_with(claude_dir.join(sub)))
}

/// Normalize `path` so symlinked path prefixes don't defeat `starts_with`
/// comparisons against canonicalized roots.
///
/// macOS `notify`/FSEvents delivers realpath-resolved paths (e.g.
/// `/private/var/...`) while a configured root may still be the symlink form
/// (`/var/...`). We canonicalize the path's deepest *existing* ancestor and
/// re-attach the (possibly already-deleted) tail, so deletions — whose final
/// component no longer exists — still normalize correctly.
fn canonicalize_with_deleted_tail(path: &Path) -> PathBuf {
    if let Ok(canon) = path.canonicalize() {
        return canon;
    }
    let mut tail: Vec<std::ffi::OsString> = Vec::new();
    let mut cur = path;
    while let Some(parent) = cur.parent() {
        if let Some(name) = cur.file_name() {
            tail.push(name.to_owned());
        }
        if let Ok(canon_parent) = parent.canonicalize() {
            let mut out = canon_parent;
            for seg in tail.iter().rev() {
                out.push(seg);
            }
            return out;
        }
        cur = parent;
    }
    path.to_owned()
}

/// Outcome of a single watcher-triggered regeneration, passed to the
/// `on_change` callback.
#[derive(Debug)]
pub enum WatchEvent {
    /// A regeneration completed successfully.
    Regenerated(GenerateReport),
    /// A regeneration failed; the watcher keeps running.
    Error(GenerateError),
}

/// A running watch session.
///
/// Shutdown ordering matters: the handle owns the `notify` debouncer and the
/// only remaining event-channel sender, while the worker thread blocks on
/// `event_rx.recv()`. Stopping drops the debouncer first (which drops the
/// sender clone its closure holds), then drops our sender — at which point the
/// channel is fully disconnected and the worker's `recv()` returns `Err`,
/// breaking its loop. We then join. This replaces the old busy-wait poll: the
/// worker no longer wakes 5x/sec, and stop is observed immediately rather than
/// after a 200ms timeout tick.
pub struct WatchHandle {
    /// The `notify` debouncer, type-erased to avoid naming its generics. Held
    /// here (not in the worker) so we can drop it on stop and disconnect the
    /// event channel. Dropping it also tears down the OS-level watch.
    debouncer: Option<Box<dyn std::any::Any + Send>>,
    /// The last live event-channel sender. Dropping it (after the debouncer's
    /// clone is gone) disconnects `event_rx`, unblocking the worker's `recv()`.
    event_tx: Option<mpsc::Sender<()>>,
    join: Option<JoinHandle<()>>,
}

impl WatchHandle {
    /// Stop the watcher and wait for its background thread to finish. Called
    /// automatically on drop, but exposed for explicit shutdown.
    pub fn stop(mut self) {
        self.stop_inner();
    }

    fn stop_inner(&mut self) {
        // Order is load-bearing: drop the debouncer (and the sender clone its
        // closure owns) BEFORE our sender, so that dropping our sender leaves
        // zero senders and `event_rx.recv()` returns Disconnected.
        self.debouncer = None;
        self.event_tx = None;
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}

impl Drop for WatchHandle {
    fn drop(&mut self) {
        self.stop_inner();
    }
}

/// Start watching `~/.claude` and regenerate the MDX tree on every change.
///
/// `on_change` is invoked after each debounced regeneration with a
/// [`WatchEvent`]. The callback runs on the watcher's background thread; keep
/// it cheap (e.g. log, or signal another thread).
///
/// The returned [`WatchHandle`] keeps the watch alive; drop it (or call
/// [`WatchHandle::stop`]) to stop watching.
pub fn watch<F>(config: Config, debounce: Duration, on_change: F) -> Result<WatchHandle>
where
    F: Fn(WatchEvent) + Send + 'static,
{
    config.validate()?;

    // Channel carrying debounced "something changed" pulses to the worker.
    // Stop is signalled by dropping every sender (see `WatchHandle::stop_inner`)
    // so the worker can block on a single `recv()` with no separate stop poll.
    let (event_tx, event_rx) = mpsc::channel::<()>();

    // The debouncer's error arm needs to surface backend failures (e.g. inotify
    // limit, backend drop) to the host. It can't call the move-only `on_change`
    // directly (the worker owns that), so it forwards errors on a side channel
    // that the worker relays. The worker treats this channel like the event
    // channel for shutdown: both share the same lifetime.
    let (err_tx, err_rx) = mpsc::channel::<GenerateError>();

    // The debouncer forwards debounced batches; we collapse each batch into a
    // single pulse on event_tx — but ONLY if the batch touched a content path.
    // Canonicalize the roots once so they match the realpath-resolved paths
    // that the backend reports (macOS reports `/private/var/...` for `/var/...`).
    //
    // NOTE: roots are canonicalized once, here. A watched root that does not yet
    // exist will fail to canonicalize and fall back to its non-realpath form, so
    // later realpath-resolved events for it may not match. Callers must ensure
    // every watched root exists before calling `watch()` (the Tauri host creates
    // `~/.claude` and the docs dir at boot, so this holds in practice).
    let event_tx_for_debouncer = event_tx.clone();
    let claude_dir = config
        .claude_dir
        .canonicalize()
        .unwrap_or_else(|_| config.claude_dir.clone());
    let project_root = config
        .project_root
        .canonicalize()
        .unwrap_or_else(|_| config.project_root.clone());
    let mut debouncer = new_debouncer(debounce, None, move |res: DebounceEventResult| match res {
        Ok(events) => {
            // Filter out pure session-state churn: only pulse if at least one
            // changed path is part of the generated docs' source content.
            // Ignore send errors (worker gone => watch is shutting down).
            let relevant = events.iter().any(|event| {
                event.paths.iter().any(|p| {
                    // Try the raw path first (no syscall). Only the rare path
                    // that fails this cheap check — but might still be relevant
                    // behind a symlinked prefix — pays for canonicalization.
                    is_content_relevant(p, &claude_dir, &project_root)
                        || is_content_relevant(
                            &canonicalize_with_deleted_tail(p),
                            &claude_dir,
                            &project_root,
                        )
                })
            });
            if relevant {
                let _ = event_tx_for_debouncer.send(());
            }
        }
        Err(errors) => {
            // A backend failure (inotify limit hit, backend dropped) means the
            // watch is no longer reliable. Log it AND forward it as a
            // WatchEvent::Error so the host learns the watcher effectively died
            // instead of silently going quiet. (Ignore send errors: the worker
            // is gone => we're already shutting down.)
            for e in errors {
                log::warn!("watch backend error: {e}");
                let _ = err_tx.send(GenerateError::watch("watch backend error", e));
            }
        }
    })
    .map_err(|e| GenerateError::watch("failed to start file watcher", e))?;

    // Watch claude_dir recursively; also watch project_root if it differs.
    let mut watched: Vec<PathBuf> = vec![config.claude_dir.clone()];
    if config.project_root != config.claude_dir {
        watched.push(config.project_root.clone());
    }
    for path in &watched {
        debouncer
            .watcher()
            .watch(path, RecursiveMode::Recursive)
            .map_err(|e| GenerateError::watch(format!("failed to watch {path:?}"), e))?;
    }

    let worker_config = config.clone();
    // The worker blocks on the event channel; it no longer owns the debouncer.
    // The debouncer lives in the WatchHandle so stop can drop it (and the sender
    // clone its closure holds), disconnecting the channel to unblock recv().
    let join = std::thread::Builder::new()
        .name("ccresdoc-claude-md-watch".to_string())
        .spawn(move || {
            // Block until a change pulse arrives; the loop ends when every sender
            // is dropped (stop) and `recv()` returns `Err(Disconnected)`. No
            // timeout: stop is observed via disconnection, so the thread sleeps
            // fully while idle instead of polling.
            while event_rx.recv().is_ok() {
                // Drain any additional queued pulses so a burst that produced
                // several debounced batches collapses into one regeneration.
                // The worker thread is the sole writer, so regenerations are
                // inherently serialized — no two ever write the same MDX
                // concurrently.
                while event_rx.try_recv().is_ok() {}

                // First, relay any backend errors the debouncer reported
                // out-of-band so the host learns the watcher is degraded.
                while let Ok(backend_err) = err_rx.try_recv() {
                    on_change(WatchEvent::Error(backend_err));
                }

                // The generator runs arbitrary walk/IO code; a panic here must
                // NOT kill the watch thread (which would silently end all future
                // regenerations). Catch it, report it as a WatchEvent::Error, and
                // keep looping.
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    generate::run(&worker_config)
                }));
                match result {
                    Ok(Ok(report)) => on_change(WatchEvent::Regenerated(report)),
                    Ok(Err(e)) => on_change(WatchEvent::Error(e)),
                    Err(panic) => {
                        let msg = panic_message(&panic);
                        log::error!("regeneration panicked: {msg}");
                        on_change(WatchEvent::Error(GenerateError::watch(
                            "regeneration panicked",
                            PanicError(msg),
                        )));
                    }
                }
            }
        })
        .map_err(|e| GenerateError::watch("failed to spawn watch thread", e))?;

    Ok(WatchHandle {
        debouncer: Some(Box::new(debouncer)),
        event_tx: Some(event_tx),
        join: Some(join),
    })
}

/// Extract a human-readable message from a `catch_unwind` payload.
fn panic_message(panic: &Box<dyn std::any::Any + Send>) -> String {
    if let Some(s) = panic.downcast_ref::<&str>() {
        (*s).to_string()
    } else if let Some(s) = panic.downcast_ref::<String>() {
        s.clone()
    } else {
        "unknown panic".to_string()
    }
}

/// A simple `std::error::Error` carrying a captured panic message, so a panic in
/// the regenerator can be surfaced through `GenerateError::Watch`'s `#[source]`.
#[derive(Debug)]
struct PanicError(String);

impl std::fmt::Display for PanicError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for PanicError {}

#[cfg(test)]
mod tests {
    use super::{canonicalize_with_deleted_tail, is_content_relevant};
    use std::path::Path;

    #[test]
    fn content_relevant_paths_trigger_regen() {
        let claude = Path::new("/home/u/.claude");
        let project = claude; // real-world case: project_root == claude_dir

        let relevant = [
            "/home/u/.claude/CLAUDE.md",
            "/home/u/.claude/some/nested/CLAUDE.md",
            "/home/u/.claude/commands/x.md",
            "/home/u/.claude/skills/foo/SKILL.md",
            "/home/u/.claude/skills/foo/references/ref-a.md",
            "/home/u/.claude/agents/bar.md",
        ];
        for p in relevant {
            assert!(
                is_content_relevant(Path::new(p), claude, project),
                "expected {p} to be content-relevant"
            );
        }
    }

    #[test]
    fn session_state_churn_is_ignored() {
        let claude = Path::new("/home/u/.claude");
        let project = claude;

        let irrelevant = [
            "/home/u/.claude/projects/p.jsonl",
            "/home/u/.claude/todos/t.json",
            "/home/u/.claude/shell-snapshots/s.sh",
            "/home/u/.claude/.git/HEAD",
            "/home/u/.claude/statsig/x",
            "/home/u/.claude/history.jsonl",
            "/home/u/.claude/logs/today.log",
            "/home/u/.claude/ide/lock",
            "/home/u/.claude/.credentials.json",
            // A CLAUDE.md OUTSIDE the watched roots must NOT qualify.
            "/somewhere/else/CLAUDE.md",
            // Subdir name that merely starts with an allowlisted prefix.
            "/home/u/.claude/commands-archive/x.md",
        ];
        for p in irrelevant {
            assert!(
                !is_content_relevant(Path::new(p), claude, project),
                "expected {p} to be ignored"
            );
        }
    }

    #[test]
    fn claude_md_under_distinct_project_root_is_relevant() {
        // When project_root differs from claude_dir, a CLAUDE.md under either
        // root counts; resource subdirs are keyed off claude_dir only.
        let claude = Path::new("/home/u/.claude");
        let project = Path::new("/home/u/proj");
        assert!(is_content_relevant(
            Path::new("/home/u/proj/sub/CLAUDE.md"),
            claude,
            project
        ));
        assert!(!is_content_relevant(
            Path::new("/home/u/proj/commands/x.md"),
            claude,
            project
        ));
    }

    // Directly exercise the macOS symlink-prefix normalizer (the actual fix
    // behind the watcher integration test) so a regression in the
    // tail-reattachment loop fails fast as a unit test, not a flaky timeout.

    #[test]
    fn canonicalize_resolves_existing_path() {
        let tmp = tempfile::TempDir::new().unwrap();
        let nested = tmp.path().join("commands");
        std::fs::create_dir_all(&nested).unwrap();
        let file = nested.join("x.md");
        std::fs::write(&file, "body").unwrap();

        // An existing path canonicalizes to its realpath, which must equal the
        // realpath of the tempdir root joined with the same tail.
        let got = canonicalize_with_deleted_tail(&file);
        let want = tmp.path().canonicalize().unwrap().join("commands/x.md");
        assert_eq!(got, want);
    }

    #[test]
    fn canonicalize_reattaches_deleted_tail() {
        // The interesting case: the leaf no longer exists (a deletion event),
        // so canonicalize() on the full path fails and we must canonicalize the
        // deepest existing ancestor and re-attach the missing tail.
        let tmp = tempfile::TempDir::new().unwrap();
        let existing_parent = tmp.path().join("commands");
        std::fs::create_dir_all(&existing_parent).unwrap();
        let deleted_leaf = existing_parent.join("gone.md");

        let got = canonicalize_with_deleted_tail(&deleted_leaf);
        let want = existing_parent.canonicalize().unwrap().join("gone.md");
        assert_eq!(got, want);
        // And the normalized result is recognized as content-relevant when the
        // canonical tempdir root is used as claude_dir.
        let claude = tmp.path().canonicalize().unwrap();
        assert!(is_content_relevant(&got, &claude, &claude));
    }
}
