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

use std::path::PathBuf;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use notify::{RecursiveMode, Watcher};
use notify_debouncer_full::new_debouncer;

use crate::error::{GenerateError, Result};
use crate::generate;
use crate::{Config, GenerateReport};

/// Default debounce window for coalescing filesystem-event bursts.
pub const DEFAULT_DEBOUNCE: Duration = Duration::from_millis(300);

/// Outcome of a single watcher-triggered regeneration, passed to the
/// `on_change` callback.
#[derive(Debug)]
pub enum WatchEvent {
    /// A regeneration completed successfully.
    Regenerated(GenerateReport),
    /// A regeneration failed; the watcher keeps running.
    Error(GenerateError),
}

/// A running watch session. Dropping the handle stops the watcher and joins its
/// background thread.
pub struct WatchHandle {
    // Kept alive for the lifetime of the watch; dropping it stops notify.
    _debouncer_alive: Arc<Mutex<()>>,
    stop_tx: Option<mpsc::Sender<()>>,
    join: Option<JoinHandle<()>>,
}

impl WatchHandle {
    /// Stop the watcher and wait for its background thread to finish. Called
    /// automatically on drop, but exposed for explicit shutdown.
    pub fn stop(mut self) {
        self.stop_inner();
    }

    fn stop_inner(&mut self) {
        if let Some(tx) = self.stop_tx.take() {
            let _ = tx.send(());
        }
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
    let (event_tx, event_rx) = mpsc::channel::<()>();
    let (stop_tx, stop_rx) = mpsc::channel::<()>();

    // The debouncer forwards debounced batches; we collapse each batch into a
    // single pulse on event_tx.
    let event_tx_for_debouncer = event_tx.clone();
    let mut debouncer = new_debouncer(debounce, None, move |res| match res {
        Ok(_events) => {
            // Any batch of events => one regeneration pulse. Ignore send errors
            // (worker gone => watch is shutting down).
            let _ = event_tx_for_debouncer.send(());
        }
        Err(errors) => {
            for e in errors {
                log::warn!("watch backend error: {e}");
            }
        }
    })
    .map_err(|e| GenerateError::Watch(e.to_string()))?;

    // Watch claude_dir recursively; also watch project_root if it differs.
    let mut watched: Vec<PathBuf> = vec![config.claude_dir.clone()];
    if config.project_root != config.claude_dir {
        watched.push(config.project_root.clone());
    }
    for path in &watched {
        debouncer
            .watcher()
            .watch(path, RecursiveMode::Recursive)
            .map_err(|e| GenerateError::Watch(format!("failed to watch {path:?}: {e}")))?;
    }

    // Serialize regenerations: the worker thread is the only writer, so a
    // dedicated mutex guards the (single) regeneration critical section. Holding
    // it across generate() guarantees no two regenerations overlap even if a
    // future caller adds more writers.
    let regen_lock = Arc::new(Mutex::new(()));
    let alive = Arc::new(Mutex::new(()));

    let worker_config = config.clone();
    let worker_lock = Arc::clone(&regen_lock);
    // Move the debouncer into the worker thread so it lives exactly as long as
    // the watch session and is dropped (stopping notify) when the thread ends.
    let join = std::thread::Builder::new()
        .name("ccresdoc-claude-md-watch".to_string())
        .spawn(move || {
            // Keep the debouncer alive inside the worker.
            let _debouncer = debouncer;
            loop {
                // Wait for either a change pulse or a stop signal.
                // Poll both channels with a short timeout so stop is responsive.
                if stop_rx.try_recv().is_ok() {
                    break;
                }
                match event_rx.recv_timeout(Duration::from_millis(200)) {
                    Ok(()) => {
                        // Drain any additional queued pulses so a burst that
                        // produced several debounced batches collapses into one
                        // regeneration.
                        while event_rx.try_recv().is_ok() {}

                        let _guard = worker_lock.lock().unwrap_or_else(|p| p.into_inner());
                        match generate::run(&worker_config) {
                            Ok(report) => on_change(WatchEvent::Regenerated(report)),
                            Err(e) => on_change(WatchEvent::Error(e)),
                        }
                    }
                    Err(mpsc::RecvTimeoutError::Timeout) => continue,
                    Err(mpsc::RecvTimeoutError::Disconnected) => break,
                }
            }
        })
        .map_err(|e| GenerateError::Watch(format!("failed to spawn watch thread: {e}")))?;

    Ok(WatchHandle {
        _debouncer_alive: alive,
        stop_tx: Some(stop_tx),
        join: Some(join),
    })
}
