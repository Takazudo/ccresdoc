#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

//! CCResDoc — thin sidecar host (Wave 3 / #44).
//!
//! Runtime is **node-free**: the host resolves a writable app-project, the
//! native `zfb` binary (NOT the Node-shebang `node_modules/.bin/zfb` wrapper),
//! and the settings-selected Claude/Codex paths, then:
//!
//!   1. generates a complete selected-resource candidate outside the served
//!      tree and journal-promotes only the managed namespaces,
//!   2. starts exactly the enabled in-process watcher set,
//!   3. selects a settings-driven loopback port and spawns native `zfb dev` as a
//!      process-group sidecar,
//!   4. polls the neutral shell plus both selection overviews for the exact
//!      transition marker before publishing/navigating the runtime.
//!
//! On main-window close the sidecar process group is SIGTERM→SIGKILL'd so
//! nothing is left holding its effective port. Closing Settings only hides it.

pub mod appearance;
mod menu;
pub mod runtime;
mod search_index_publication;
pub mod settings;
pub mod settings_commands;
pub mod settings_window;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
#[cfg(unix)]
use std::sync::atomic::AtomicI32;
use std::sync::atomic::{AtomicBool, AtomicU16, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use std::{env, thread};

use ccresdoc_claude_md::{
    generate_codex, watch_codex, CodexConfig, CodexWatchEvent, CodexWatchHandle,
    Config as GenConfig, WatchEvent, WatchHandle, DEFAULT_DEBOUNCE,
};
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder};

use runtime::{
    NavigationDecision, PortBoundary, PortChoice, ReadyResult, RuntimeDiagnostic,
    RuntimeDiagnosticKind, SystemPortBoundary,
};
use search_index_publication::publish_search_index;
use settings::{EffectiveSettings, SettingsStore};
use settings_window::{
    lifecycle_action, open_or_focus_settings, LifecycleAction, SETTINGS_MENU_ID,
    SETTINGS_WINDOW_LABEL,
};
const LOADING_URL: &str = "tauri://localhost/index.html";
const IS_DEV: bool = cfg!(debug_assertions);

/// Cold first launch must walk + render ~135 skills (plus commands/agents/
/// CLAUDE.md) and then let `zfb dev` build the whole site once. That is far
/// slower than a warm relaunch, so the readiness window is generous; the
/// loading page stays informative (spinner + "still building" hint) meanwhile.
const READY_TIMEOUT: Duration = Duration::from_secs(300);

/// Sentinel filename written into the writable workspace once a copy fully
/// completes. Its presence + matching version token is what marks the
/// workspace "ready"; a partial/interrupted copy lacks it and is re-copied.
const WORKSPACE_READY_FILE: &str = ".ccresdoc-workspace-ready";
const EPHEMERAL_WEBVIEW_ENV: &str = "CCRESDOC_EPHEMERAL_WEBVIEW";

/// Process-exit backstop for the one native sidecar group CCResDoc can own at
/// a time. AppKit's Apple-event quit path can bypass Tauri run events on macOS,
/// so normal `teardown` clears this only after the exact group is gone; an
/// `atexit` hook consumes it otherwise. It is never populated from a port or
/// process scan.
#[cfg(unix)]
static OWNED_SIDECAR_PROCESS_GROUP: AtomicI32 = AtomicI32::new(0);
#[cfg(unix)]
const SIDECAR_OWNERSHIP_EXITING: i32 = -1;
#[cfg(unix)]
static SIDECAR_OWNERSHIP_TRANSITION: Mutex<()> = Mutex::new(());

/// Maps `std::env::consts::OS`-`ARCH` to the zfb platform package name.
/// Mirrors `@takazudo/zfb/bin/zfb.mjs` exactly (biome's pattern). The native
/// binary lives at `<pkgDir>/zfb` (`zfb.exe` on Windows) — NEVER the
/// `node_modules/.bin/zfb` Node-shebang wrapper, which would require Node.
fn zfb_platform_package_for(os: &str, arch: &str) -> Option<&'static str> {
    match (os, arch) {
        ("macos", "aarch64") => Some("@takazudo/zfb-darwin-arm64"),
        ("macos", "x86_64") => Some("@takazudo/zfb-darwin-x64"),
        ("linux", "aarch64") => Some("@takazudo/zfb-linux-arm64-gnu"),
        ("linux", "x86_64") => Some("@takazudo/zfb-linux-x64-gnu"),
        ("windows", "x86_64") => Some("@takazudo/zfb-win32-x64-msvc"),
        _ => None,
    }
}

fn zfb_platform_package() -> Option<&'static str> {
    zfb_platform_package_for(env::consts::OS, env::consts::ARCH)
}

fn zfb_binary_name() -> &'static str {
    if cfg!(windows) {
        "zfb.exe"
    } else {
        "zfb"
    }
}

// ── Shared state ──────────────────────────────────

struct Sidecar {
    child: Child,
    #[cfg(unix)]
    process_group_id: i32,
}

struct ResourceWatchers {
    claude: Option<WatchHandle>,
    codex: Option<CodexWatchHandle>,
    /// Serializes coordinator-owned overview publications across both source
    /// watchers. Detail namespaces remain disjoint and generator-owned.
    publication: Arc<Mutex<()>>,
}

struct ResourceRuntime {
    generation: u64,
    marker: String,
    selection: runtime::ResourceSelection,
    effective: EffectiveSettings,
    counts: runtime::ResourceCounts,
    workspace: PathBuf,
    sidecar: Sidecar,
    watchers: ResourceWatchers,
}

#[derive(Debug, Clone)]
struct PreviousRuntime {
    marker: String,
    effective: EffectiveSettings,
    counts: runtime::ResourceCounts,
    workspace: PathBuf,
}

struct AppState {
    /// Cohesive ownership: a published runtime always owns its exact sidecar,
    /// selected watcher set, generation marker, workspace, and settings.
    resources: Arc<Mutex<Option<ResourceRuntime>>>,
    zoom: Mutex<f64>,
    /// Filled in during setup() (app_data_dir/ccresdoc.log).
    log_path: Mutex<String>,
    runtime: Arc<runtime::ApplyCoordinator>,
    settings_store: SettingsStore,
    appearance: appearance::AppearanceState,
    /// Read by the navigation callback without consulting Tauri state.
    effective_port: Arc<AtomicU16>,
    /// Set before exit teardown. Publication handshakes with this flag so a
    /// watcher or child spawned concurrently with exit cannot escape tracking.
    shutting_down: AtomicBool,
}

// ── Helpers ───────────────────────────────────────

/// `$HOME`, or `None` if it is unset/empty. Returning `Option` instead of
/// panicking lets the launch thread surface a dedicated error in the UI rather
/// than aborting the process (a missing/empty `HOME` is a recoverable launch
/// failure, not a crash).
fn home_dir() -> Option<String> {
    env::var("HOME").ok().filter(|h| !h.is_empty())
}

fn ephemeral_webview_enabled_value(value: Option<&str>) -> bool {
    value == Some("1")
}

pub(crate) fn ephemeral_webview_enabled() -> bool {
    ephemeral_webview_enabled_value(env::var(EPHEMERAL_WEBVIEW_ENV).ok().as_deref())
}

fn resource_publication_allowed(shutting_down: bool, current: u64, expected: u64) -> bool {
    !shutting_down && current == expected
}

/// Atomically recheck the launch lease and install an owned runtime relative
/// to teardown's resource slot. Teardown sets `shutting_down`/generation before
/// taking this same lock, so either it sees and stops the published value or a
/// stale publisher receives its value back for local rollback.
fn publish_owned_resource_if_current<T>(
    slot: &Mutex<Option<T>>,
    shutting_down: &AtomicBool,
    generation: &AtomicU64,
    expected: u64,
    value: T,
) -> Result<(), T> {
    let mut slot = slot.lock().unwrap();
    if !resource_publication_allowed(
        shutting_down.load(Ordering::SeqCst),
        generation.load(Ordering::SeqCst),
        expected,
    ) {
        return Err(value);
    }
    *slot = Some(value);
    Ok(())
}

fn watcher_publication_allowed(app_handle: &AppHandle, expected: u64) -> bool {
    let state = app_handle.state::<AppState>();
    if state.shutting_down.load(Ordering::SeqCst) {
        return false;
    }
    let allowed = state.resources.lock().unwrap().as_ref().map_or_else(
        || state.runtime.generation().load(Ordering::SeqCst) == expected,
        |runtime| runtime.generation == expected,
    );
    allowed
}

/// The log path resolved in setup(), read out of shared state.
fn log_path(app_handle: &AppHandle) -> String {
    app_handle
        .state::<AppState>()
        .log_path
        .lock()
        .unwrap()
        .clone()
}

/// Navigate the main window to the doc site. Parse errors are impossible for
/// the runtime-generated docs URL, so they are silently ignored. Shared by the
/// launch-success path, the dev retry path, and the Refresh menu item.
fn navigate_to_docs(app_handle: &AppHandle) {
    if let Some(w) = app_handle.get_webview_window("main") {
        let state = app_handle.state::<AppState>();
        let port = state.effective_port.load(Ordering::SeqCst);
        if let Ok(url) = runtime::docs_url(port).parse::<tauri::Url>() {
            // One WebKit task updates the appearance-only navigation seed and
            // starts navigation. The installed initialization script consumes
            // it at DocumentStart on the exact destination origin.
            state.appearance.clear_candidate();
            let snapshot = state.settings_store.load();
            let seed =
                appearance::bootstrap_seed(&snapshot, state.settings_store.available_theme_packs());
            let script = appearance::window_name_script(&seed, url.as_str());
            let _ = w.eval(script);
        }
    }
}

/// A freshly built main WebView already carries a current static DocumentStart
/// seed, so reopen can use native navigation without racing an eval against the
/// loading document's creation.
#[cfg(target_os = "macos")]
fn navigate_fresh_main_to_docs(app_handle: &AppHandle) {
    let state = app_handle.state::<AppState>();
    state.appearance.clear_candidate();
    let port = state.effective_port.load(Ordering::SeqCst);
    if let (Some(window), Ok(url)) = (
        app_handle.get_webview_window("main"),
        runtime::docs_url(port).parse::<tauri::Url>(),
    ) {
        let _ = window.navigate(url);
    }
}

/// Restore the bundled loading surface before a Refresh begins. Retry already
/// runs from this page's error panel, so both paths converge on the same launch
/// lease and semantic-readiness classifier.
fn navigate_to_loading(app_handle: &AppHandle) {
    app_handle.state::<AppState>().appearance.clear_candidate();
    if let Some(w) = app_handle.get_webview_window("main") {
        if let Ok(url) = LOADING_URL.parse::<tauri::Url>() {
            let _ = w.navigate(url);
        }
    }
}

/// Build a `Command` for an external tool, preferring the macOS absolute path
/// but falling back to a bare name (resolved via `PATH`) when that absolute
/// path does not exist. macOS ships `cp` at `/bin/cp`; on other Unixes the
/// layout can differ, so we let `PATH` resolve the bare name. This keeps
/// current macOS behavior while making the host portable for local dev/CI.
#[cfg(target_os = "macos")]
fn tool_command(abs_path: &str, bare_name: &str) -> Command {
    if Path::new(abs_path).exists() {
        Command::new(abs_path)
    } else {
        Command::new(bare_name)
    }
}

fn log_to(path: &str, msg: &str) {
    use std::io::Write;
    if path.is_empty() {
        return;
    }
    if let Ok(mut f) = fs::OpenOptions::new().create(true).append(true).open(path) {
        let secs = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let _ = writeln!(f, "[{secs}] {msg}");
    }
}

// ── Bundle version token (writable-workspace refresh gate) ─

/// The version token used to decide whether the writable workspace copy is
/// stale. The effective token is the app's Cargo package version, embedded at
/// compile time. The package build emits a verified content-derived
/// `version.txt` beside the bundled `app/`; the Cargo version is only the
/// defensive fallback for a malformed or missing staging token.
fn bundled_version_token(resources_app_parent: &Path) -> String {
    let version_file = resources_app_parent.join("version.txt");
    if let Ok(v) = fs::read_to_string(&version_file) {
        let v = v.trim();
        if is_valid_version_token(v) {
            return v.to_string();
        }
    }
    env!("CARGO_PKG_VERSION").to_string()
}

/// A `version.txt` override must look like a sane token before it is trusted to
/// gate the workspace-refresh decision: a single non-empty line, bounded
/// length, and limited to version-ish characters (alphanumerics plus
/// `. _ - +`, the chars that show up in semver / build identifiers). Junk
/// (multi-line, control chars, absurd length) is rejected so a corrupt file
/// cannot wedge the refresh logic; the caller then falls back to the
/// compiled-in `CARGO_PKG_VERSION`.
fn is_valid_version_token(v: &str) -> bool {
    !v.is_empty()
        && v.len() <= 64
        && !v.contains(['\n', '\r'])
        && v.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | '+'))
}

/// Write the ready sentinel and fsync it (plus its parent dir) so the "ready"
/// marker is durable only once the bytes it implies are also durable. We sync
/// the file's own data, then fsync the containing directory so the new
/// directory entry itself survives a crash. Dir fsync is best-effort (some
/// platforms reject `O_RDONLY` dir sync); failing it does not fail the write.
fn write_sentinel_durable(sentinel: &Path, dir: &Path, token: &str) -> std::io::Result<()> {
    use std::io::Write;
    let mut f = fs::File::create(sentinel)?;
    f.write_all(token.as_bytes())?;
    f.sync_all()?;
    // Best-effort: persist the directory entry for the sentinel too.
    if let Ok(d) = fs::File::open(dir) {
        let _ = d.sync_all();
    }
    Ok(())
}

// ── Workspace resolution ──────────────────────────

/// How the writable app-project root was resolved.
#[derive(Debug)]
enum WorkspaceResolution {
    /// `cargo tauri dev` — use the repo `app/` directly (already writable,
    /// already has `node_modules` from the dev `pnpm install`).
    DevRepo(PathBuf),
    /// Bundled `.app` — a versioned copy of the read-only bundled `app/` placed
    /// in the app-data dir (writable; `zfb dev` writes `dist/`, `.zfb/`,
    /// `.zfb-build/`, and the generated `claude*/` MDX there).
    AppDataCopy(PathBuf),
}

impl WorkspaceResolution {
    fn path(&self) -> &Path {
        match self {
            WorkspaceResolution::DevRepo(p) | WorkspaceResolution::AppDataCopy(p) => p,
        }
    }
}

/// Resolve the bundled (read-only) staged runtime workspace inside `.app`
/// Resources. The build hook creates this deliberately pruned tree; bundling
/// the repository `app/` directly would ship TypeScript/Vitest and every
/// optional platform binary.
fn bundled_resources_app_parent(app: &AppHandle) -> tauri::Result<PathBuf> {
    Ok(app.path().resource_dir()?.join("runtime-workspace"))
}

/// Resolve a **writable** app-project root.
///
/// - Dev: the repo `app/` (sibling of `src-tauri/`, found via `CARGO_MANIFEST_DIR`).
/// - Bundled: copy the read-only bundled `app/` into the app-data dir, with a
///   **versioned refresh** (re-copy when the bundled token differs from the
///   one recorded in the copy, or when the previous copy never completed).
fn resolve_workspace(app: &AppHandle, log_path: &str) -> Result<WorkspaceResolution, String> {
    if IS_DEV {
        // src-tauri/ sibling: ../app
        let repo_app = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .map(|p| p.join("app"))
            .ok_or_else(|| "could not resolve repo app/ dir in dev".to_string())?;
        log_to(
            log_path,
            &format!("resolve_workspace: DEV repo app = {}", repo_app.display()),
        );
        return Ok(WorkspaceResolution::DevRepo(repo_app));
    }

    let resources_parent =
        bundled_resources_app_parent(app).map_err(|e| format!("resource_dir unavailable: {e}"))?;
    let bundled_app = resources_parent.join("app");
    if !bundled_app.exists() {
        return Err(format!(
            "bundled app/ missing at {} (build did not stage it)",
            bundled_app.display()
        ));
    }

    let app_data = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("app_data_dir unavailable: {e}"))?;
    fs::create_dir_all(&app_data).map_err(|e| format!("create app_data dir: {e}"))?;
    let workspace = app_data.join("app-workspace");

    let bundled_token = bundled_version_token(&resources_parent);
    let ready_sentinel = workspace.join(WORKSPACE_READY_FILE);
    let recorded_token = fs::read_to_string(&ready_sentinel)
        .ok()
        .map(|s| s.trim().to_string());

    let up_to_date = recorded_token.as_deref() == Some(bundled_token.as_str());
    if workspace.exists() && up_to_date {
        log_to(
            log_path,
            &format!(
                "resolve_workspace: reusing workspace {} (token={bundled_token})",
                workspace.display()
            ),
        );
        return Ok(WorkspaceResolution::AppDataCopy(workspace));
    }

    log_to(
        log_path,
        &format!(
            "resolve_workspace: (re)copying bundled app -> {} (bundled_token={bundled_token}, recorded={recorded_token:?})",
            workspace.display()
        ),
    );

    // Remove any partial/stale copy, then copy fresh. The sentinel is written
    // LAST so an interrupted copy is detected (missing sentinel ⇒ not ready).
    if workspace.exists() {
        fs::remove_dir_all(&workspace).map_err(|e| format!("clear stale workspace: {e}"))?;
    }
    copy_workspace(&bundled_app, &workspace, log_path)
        .map_err(|e| format!("copy bundled app into workspace: {e}"))?;
    // The sentinel is written LAST (after the copy succeeds) so a partial copy
    // is detected as not-ready. The bundled app/ has no sentinel of its own, so
    // a fast `cp` cannot drag a stale "ready" marker into a partial dest; still,
    // writing it here unconditionally after success keeps the invariant.
    //
    // fsync the sentinel (and its parent dir) before treating the workspace as
    // ready: a crash between `write` and the OS flushing its page cache could
    // otherwise leave a "ready" sentinel durably on disk over a workspace tree
    // whose file contents had not yet been flushed — exactly the partial-but-
    // marked-ready state the sentinel exists to prevent.
    write_sentinel_durable(&ready_sentinel, &workspace, &bundled_token)
        .map_err(|e| format!("write ready sentinel: {e}"))?;

    log_to(
        log_path,
        &format!("resolve_workspace: workspace ready (token={bundled_token})"),
    );
    Ok(WorkspaceResolution::AppDataCopy(workspace))
}

/// Copy the bundled `src` tree into `dst`, preserving permissions and symlinks.
///
/// The workspace contains a large native binary plus many package files. A
/// byte-for-byte [`copy_dir_recursive`] of the former unpruned tree measured
/// ~41s on cold first launch, which alone blows the 60s acceptance budget. So
/// on macOS we prefer **APFS clonefile** (copy-on-write — near-instant, no data
/// is moved):
///
///   1. `cp -Rc src/. dst` — `-c` uses `clonefile(2)`, `-R` recurses,
///      symlinks are copied as symlinks and permissions preserved (matching
///      [`copy_dir_recursive`]'s semantics). The `src/.` form copies the
///      CONTENTS of `src` into `dst` (so `dst/node_modules/…`, NOT
///      `dst/app/node_modules/…`).
///   2. If that fails (clonefile only works within one APFS volume — a
///      cross-volume app-data dir returns non-zero), fall back to `cp -R`
///      (still a fast native copy).
///   3. If `cp` is unavailable/fails entirely, fall back to the portable
///      [`copy_dir_recursive`] byte copy.
///
/// On non-macOS we always use [`copy_dir_recursive`].
///
/// `dst` is expected to be freshly created/empty (the caller removes any stale
/// copy first); the sentinel is written by the caller AFTER this returns Ok.
fn copy_workspace(src: &Path, dst: &Path, log_path: &str) -> std::io::Result<()> {
    #[cfg(target_os = "macos")]
    {
        // `cp -Rc src/. dst` clones the CONTENTS of src into dst via clonefile.
        let src_contents = format!("{}/.", src.display());
        if run_cp(
            dst,
            &["-Rc", &src_contents, &dst.to_string_lossy()],
            log_path,
        ) {
            log_to(log_path, "copy_workspace: used clonefile (cp -Rc)");
            return Ok(());
        }
        log_to(
            log_path,
            "copy_workspace: cp -Rc failed (cross-volume?) — falling back to cp -R",
        );
        if run_cp(
            dst,
            &["-R", &src_contents, &dst.to_string_lossy()],
            log_path,
        ) {
            log_to(log_path, "copy_workspace: used native copy (cp -R)");
            return Ok(());
        }
        log_to(
            log_path,
            "copy_workspace: cp -R failed — falling back to byte copy",
        );
        // Start the byte-copy fallback from a clean dest so a partially-written
        // failed `cp` cannot leave stray files behind.
        let _ = fs::remove_dir_all(dst);
    }
    log_to(
        log_path,
        "copy_workspace: using byte copy (copy_dir_recursive)",
    );
    copy_dir_recursive(src, dst)
}

/// Run `/bin/cp` with the given args; returns true on a zero exit status. `dst`
/// is wiped and recreated empty first so each attempt starts clean — a failed
/// `cp` (e.g. cross-volume `-Rc`) cannot leave a partial tree for the next
/// fallback hop, and the `src/. → dst` form writes contents INTO an existing
/// `dst` rather than erroring. `cp`'s stderr is logged on failure so a field
/// diagnosis can see WHY the fast path was rejected (cross-volume, perms, …).
#[cfg(target_os = "macos")]
fn run_cp(dst: &Path, args: &[&str], log_path: &str) -> bool {
    let _ = fs::remove_dir_all(dst);
    if fs::create_dir_all(dst).is_err() {
        return false;
    }
    match tool_command("/bin/cp", "cp").args(args).output() {
        Ok(out) if out.status.success() => true,
        Ok(out) => {
            let stderr = String::from_utf8_lossy(&out.stderr);
            log_to(
                log_path,
                &format!(
                    "run_cp: cp {args:?} failed ({}): {}",
                    out.status,
                    stderr.trim()
                ),
            );
            false
        }
        Err(e) => {
            log_to(log_path, &format!("run_cp: cp {args:?} spawn error: {e}"));
            false
        }
    }
}

/// Recursively copy `src` into `dst`, preserving Unix permissions (the native
/// `zfb` binary and `node_modules/.bin` shims must stay executable). Symlinks
/// are recreated as symlinks (pnpm's `node_modules` is symlink-heavy).
///
/// Cross-platform fallback for [`copy_workspace`]; used directly on non-macOS
/// and when the macOS `cp` fast paths fail.
fn copy_dir_recursive(src: &Path, dst: &Path) -> std::io::Result<()> {
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let from = entry.path();
        let to = dst.join(entry.file_name());

        if file_type.is_symlink() {
            let target = fs::read_link(&from)?;
            // Best-effort: replace any pre-existing entry at `to`.
            let _ = fs::remove_file(&to);
            #[cfg(unix)]
            std::os::unix::fs::symlink(&target, &to)?;
            #[cfg(windows)]
            {
                // Windows symlink kind depends on the target; fall back to a
                // file symlink (node_modules layout is dir-symlink-heavy, but
                // Tauri targets macOS here so this branch is rarely taken).
                let _ = std::os::windows::fs::symlink_file(&target, &to);
            }
        } else if file_type.is_dir() {
            copy_dir_recursive(&from, &to)?;
        } else {
            fs::copy(&from, &to)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                if let Ok(meta) = fs::metadata(&from) {
                    let _ = fs::set_permissions(
                        &to,
                        fs::Permissions::from_mode(meta.permissions().mode()),
                    );
                }
            }
        }
    }
    Ok(())
}

// ── zfb binary resolution ─────────────────────────

/// Resolve the **native** zfb binary inside the workspace's `node_modules`.
///
/// Path: `<workspace>/node_modules/@takazudo/zfb-<platform>/zfb`. This is the
/// platform package's binary (`main: "zfb"`), NOT the `node_modules/.bin/zfb`
/// Node-shebang wrapper — running the wrapper would require Node at runtime,
/// defeating the node-free goal.
fn resolve_zfb_binary(workspace: &Path) -> Result<PathBuf, String> {
    let pkg = zfb_platform_package().ok_or_else(|| {
        format!(
            "unsupported platform: {}-{}",
            env::consts::OS,
            env::consts::ARCH
        )
    })?;
    let bin = workspace
        .join("node_modules")
        .join(pkg)
        .join(zfb_binary_name());
    if !bin.exists() {
        return Err(format!(
            "native zfb binary missing at {} — node_modules not installed or platform package absent",
            bin.display()
        ));
    }
    if !bin.is_file() {
        return Err(format!("native zfb path is not a file: {}", bin.display()));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = fs::metadata(&bin)
            .map_err(|e| format!("inspect native zfb binary {}: {e}", bin.display()))?
            .permissions()
            .mode();
        if mode & 0o111 == 0 {
            return Err(format!(
                "native zfb binary is not executable: {}",
                bin.display()
            ));
        }
    }
    Ok(bin)
}

// ── Sidecar (zfb dev) management ──────────────────

/// Build the native zfb command. `ZFB_DEV_BOOT_LAZY` is removed explicitly:
/// Finder and terminal launches may inherit it, but readiness must come from a
/// fresh sidecar build over the privacy-scoped source tree, not staged `dist/`.
fn zfb_dev_command(zfb_bin: &Path, workspace: &Path, port: u16) -> Command {
    let mut cmd = Command::new(zfb_bin);
    cmd.args(["dev", "--host", "127.0.0.1", "--port", &port.to_string()])
        .current_dir(workspace)
        .env_remove("ZFB_DEV_BOOT_LAZY");
    cmd
}

/// Spawn `zfb dev` on the selected port with cwd = the writable workspace, in
/// its own process group so the whole owned tree dies on window close.
fn spawn_zfb_dev(
    zfb_bin: &Path,
    workspace: &Path,
    port: u16,
    log_path: &str,
) -> Result<Sidecar, String> {
    log_to(
        log_path,
        &format!(
            "spawn_zfb_dev: bin={} cwd={} port={port}",
            zfb_bin.display(),
            workspace.display()
        ),
    );

    let sidecar_log = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path)
        .map_err(|e| format!("open sidecar log {log_path}: {e}"))?;
    let sidecar_stderr = sidecar_log
        .try_clone()
        .map_err(|e| format!("clone sidecar log {log_path}: {e}"))?;

    let mut cmd = zfb_dev_command(zfb_bin, workspace, port);
    cmd.stdout(Stdio::from(sidecar_log))
        .stderr(Stdio::from(sidecar_stderr));

    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }

    // Serialize the tiny spawn-to-PGID-publication window with the process-exit
    // hook. If exit wins, no new child is created; if spawn wins, the hook waits
    // until the exact new PGID has been published and then consumes it.
    #[cfg(unix)]
    let _ownership_transition = SIDECAR_OWNERSHIP_TRANSITION
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    #[cfg(unix)]
    if OWNED_SIDECAR_PROCESS_GROUP.load(Ordering::SeqCst) == SIDECAR_OWNERSHIP_EXITING {
        return Err("refusing to start zfb dev while process exit is in progress".to_string());
    }

    let mut child = cmd.spawn().map_err(|e| {
        log_to(log_path, &format!("spawn_zfb_dev: spawn failed: {e}"));
        format!("failed to spawn zfb dev in {}: {e}", workspace.display())
    })?;
    log_to(log_path, &format!("spawn_zfb_dev: pid={}", child.id()));
    #[cfg(unix)]
    let process_group_id = match i32::try_from(child.id()) {
        Ok(pid) if pid > 0 => pid,
        _ => {
            let _ = child.kill();
            let _ = child.wait();
            return Err("spawned zfb dev with an invalid process-group id".to_string());
        }
    };
    let sidecar = Sidecar {
        child,
        #[cfg(unix)]
        process_group_id,
    };
    #[cfg(unix)]
    let sidecar = {
        let mut sidecar = sidecar;
        if let Err(existing) = claim_owned_process_group(process_group_id) {
            kill_sidecar(&mut sidecar, log_path);
            return Err(if existing == SIDECAR_OWNERSHIP_EXITING {
                "refusing to start zfb dev while process exit is in progress".to_string()
            } else {
                format!(
                    "refusing to replace live owned process group {existing} with {process_group_id}"
                )
            });
        }
        sidecar
    };
    Ok(sidecar)
}

/// Tear down the live sidecar + watcher: drop the `WatchHandle` (stops the
/// watcher) and SIGTERM→SIGKILL the `zfb dev` process group so no owned child
/// remains.
///
/// This MUST run on every app-exit path, not just window close. An app-level
/// Quit (Cmd+Q, Dock → Quit, `osascript 'tell application … to quit'`) can
/// terminate the app WITHOUT reliably emitting `WindowEvent::Destroyed` first,
/// which previously left `zfb dev` orphaned. So the run-event handler
/// calls this from `WindowEvent::Destroyed` AND `ExitRequested` AND `Exit`.
///
/// It is idempotent: the cohesive runtime is removed with `Option::take()`, so
/// whichever exit event fires first does the work and any later call is a
/// no-op. A process-exit hook owns the same exact PGID as a final backstop on
/// macOS, where LaunchServices can finish application termination without
/// giving the run-event callback a usable teardown window.
fn teardown(
    app_handle: &AppHandle,
    resources: &Arc<Mutex<Option<ResourceRuntime>>>,
    log_path: &str,
    shutting_down: bool,
) {
    let state = app_handle.state::<AppState>();
    if shutting_down {
        state.shutting_down.store(true, Ordering::SeqCst);
    }
    let stopped_generation = state.runtime.claim_generation();
    state.runtime.publish_stopped(stopped_generation);
    stop_owned_runtime_resources(app_handle, resources, log_path);
}

/// `libc::kill(target, sig)` with its return value checked and logged on
/// failure (the bare call drops it, so a failed signal — e.g. `ESRCH` for a
/// already-dead target, or `EPERM` — was previously invisible). Returns whether
/// the signal was delivered. `target` is the raw argument (a negative value
/// signals the process group); the caller is responsible for only passing a
/// target it has confirmed is still live (so a recycled PID/PGID is not hit).
#[cfg(unix)]
fn signal_checked(target: i32, sig: i32, log_path: &str, what: &str) -> bool {
    // SAFETY: `kill(2)` is a plain syscall with no memory contract; we only
    // pass an integer pid/pgid and signal number.
    let rc = unsafe { libc::kill(target, sig) };
    if rc != 0 {
        let err = std::io::Error::last_os_error();
        log_to(
            log_path,
            &format!("{what}: kill({target}, {sig}) failed: {err}"),
        );
        false
    } else {
        true
    }
}

/// Poll `try_wait` up to `max` (in `step` increments), returning `true` as soon
/// as the child is reaped. Unlike a fixed `sleep`, this returns immediately once
/// the child exits — important on the main event loop (`ExitRequested`/`Exit`),
/// where a blanket `sleep(500ms)` would stall the loop even when the child has
/// already gone.
fn wait_reaped(child: &mut Child, max: Duration, step: Duration) -> bool {
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => return true,
            Ok(None) => {}
            // try_wait errored (e.g. already reaped elsewhere) — stop polling.
            Err(_) => return true,
        }
        if start.elapsed() >= max {
            return false;
        }
        thread::sleep(step);
    }
}

#[cfg(unix)]
fn process_group_exists(process_group_id: i32) -> bool {
    if process_group_id <= 0 {
        return false;
    }
    // SAFETY: signal 0 performs an existence/permission check only. The
    // negative id addresses the process group created for this exact child.
    if unsafe { libc::kill(-process_group_id, 0) } == 0 {
        return true;
    }
    std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

#[cfg(unix)]
fn claim_owned_process_group(process_group_id: i32) -> Result<(), i32> {
    let existing = OWNED_SIDECAR_PROCESS_GROUP.load(Ordering::SeqCst);
    if existing > 0 && !process_group_exists(existing) {
        let _ = OWNED_SIDECAR_PROCESS_GROUP.compare_exchange(
            existing,
            0,
            Ordering::SeqCst,
            Ordering::SeqCst,
        );
    }
    OWNED_SIDECAR_PROCESS_GROUP
        .compare_exchange(0, process_group_id, Ordering::SeqCst, Ordering::SeqCst)
        .map(|_| ())
}

#[cfg(unix)]
fn release_owned_process_group(process_group_id: i32) {
    let _ = OWNED_SIDECAR_PROCESS_GROUP.compare_exchange(
        process_group_id,
        0,
        Ordering::SeqCst,
        Ordering::SeqCst,
    );
}

/// `applicationWillTerminate` can end a macOS app without Tauri dispatching a
/// usable run event. libc's normal process-exit hook runs inside that native
/// termination path. It has no discovery step: the only target is the PGID
/// stored immediately after this host created it with `process_group(0)`.
#[cfg(unix)]
extern "C" fn stop_owned_sidecar_at_process_exit() {
    let _ownership_transition = SIDECAR_OWNERSHIP_TRANSITION
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    // Keep the slot terminal while exit handlers run. A late background launch
    // may still reach `spawn` when AppKit bypassed Tauri's shutdown flag; its
    // subsequent claim must fail so that exact new child is torn down too.
    let process_group_id =
        OWNED_SIDECAR_PROCESS_GROUP.swap(SIDECAR_OWNERSHIP_EXITING, Ordering::SeqCst);
    if process_group_id <= 0 {
        return;
    }

    if process_group_exists(process_group_id) {
        // SAFETY: the negative id is the exact group claimed at sidecar spawn.
        let _ = unsafe { libc::kill(-process_group_id, libc::SIGTERM) };
    }
    if wait_process_group_gone_at_exit(process_group_id, Duration::from_millis(1000)) {
        return;
    }
    if process_group_exists(process_group_id) {
        // SAFETY: the group is still allocated, so the stored PGID still names
        // this exact owned group rather than a recycled identifier.
        let _ = unsafe { libc::kill(-process_group_id, libc::SIGKILL) };
    }
    let _ = wait_process_group_gone_at_exit(process_group_id, Duration::from_millis(1000));
}

#[cfg(unix)]
fn wait_process_group_gone_at_exit(process_group_id: i32, max: Duration) -> bool {
    let start = Instant::now();
    loop {
        let mut status = 0;
        // SAFETY: the group leader is the direct child whose PID equals PGID.
        // WNOHANG makes this a bounded reap attempt during process exit.
        let _ = unsafe { libc::waitpid(process_group_id, &mut status, libc::WNOHANG) };
        if !process_group_exists(process_group_id) {
            return true;
        }
        if start.elapsed() >= max {
            return false;
        }
        thread::sleep(Duration::from_millis(25));
    }
}

#[cfg(unix)]
fn register_sidecar_process_exit_hook() -> Result<(), String> {
    // SAFETY: the callback has C ABI, no captures, and remains valid for the
    // entire process lifetime.
    let result = unsafe { libc::atexit(stop_owned_sidecar_at_process_exit) };
    if result == 0 {
        Ok(())
    } else {
        Err(format!(
            "register sidecar process-exit hook: status {result}"
        ))
    }
}

/// Reap the direct child while waiting for every member of its exact process
/// group to disappear. The group, rather than the listening port or group
/// leader alone, is the lifecycle contract.
#[cfg(unix)]
fn wait_owned_process_group_gone(
    child: &mut Child,
    process_group_id: i32,
    max: Duration,
    step: Duration,
) -> bool {
    let start = Instant::now();
    loop {
        let _ = child.try_wait();
        if !process_group_exists(process_group_id) {
            return true;
        }
        if start.elapsed() >= max {
            return false;
        }
        thread::sleep(step);
    }
}

fn kill_sidecar(sidecar: &mut Sidecar, log_path: &str) {
    let pid = sidecar.child.id();
    #[cfg(unix)]
    let process_group_id = sidecar.process_group_id;
    #[cfg(unix)]
    log_to(
        log_path,
        &format!("kill_sidecar: pid={pid} pgid={process_group_id}"),
    );
    #[cfg(not(unix))]
    log_to(log_path, &format!("kill_sidecar: pid={pid}"));

    #[cfg(unix)]
    {
        // Signal the stored group even if its leader exited between readiness
        // and teardown. A surviving group member keeps the PGID allocated, so
        // this still targets the exact app-owned tree rather than a port owner.
        if process_group_exists(process_group_id) {
            signal_checked(-process_group_id, libc::SIGTERM, log_path, "kill_sidecar");
        }

        let gone_after_term = wait_owned_process_group_gone(
            &mut sidecar.child,
            process_group_id,
            Duration::from_millis(1000),
            Duration::from_millis(50),
        );
        if gone_after_term {
            log_to(log_path, "kill_sidecar: group exited after SIGTERM");
        } else {
            log_to(log_path, "kill_sidecar: escalating group to SIGKILL");
            if process_group_exists(process_group_id) {
                signal_checked(-process_group_id, libc::SIGKILL, log_path, "kill_sidecar");
            }
            if !wait_owned_process_group_gone(
                &mut sidecar.child,
                process_group_id,
                Duration::from_millis(1000),
                Duration::from_millis(50),
            ) {
                log_to(
                    log_path,
                    &format!(
                        "kill_sidecar: process group {process_group_id} survived SIGKILL timeout"
                    ),
                );
            }
        }

        // A child that moved itself out of the group is still represented by
        // the exact `Child` handle. Bound the final reap, then terminate only
        // that owned PID as a defensive fallback.
        if !wait_reaped(
            &mut sidecar.child,
            Duration::from_millis(250),
            Duration::from_millis(25),
        ) {
            let _ = sidecar.child.kill();
            let _ = sidecar.child.wait();
        }
        if !process_group_exists(process_group_id) {
            release_owned_process_group(process_group_id);
        }
    }

    #[cfg(not(unix))]
    {
        if !wait_reaped(
            &mut sidecar.child,
            Duration::from_millis(1000),
            Duration::from_millis(50),
        ) {
            let _ = sidecar.child.kill();
        }
        let _ = sidecar.child.wait();
    }
}

fn stop_owned_runtime_resources(
    app_handle: &AppHandle,
    resources: &Arc<Mutex<Option<ResourceRuntime>>>,
    log_path: &str,
) {
    let state = app_handle.state::<AppState>();
    let mut owned = resources.lock().ok().and_then(|mut slot| slot.take());
    if let Some(ref mut owned) = owned {
        log_to(
            log_path,
            &format!(
                "stop runtime[{}]: claude={} codex={}",
                owned.generation, owned.selection.claude, owned.selection.codex
            ),
        );
        // Never hold the resource slot while joining: an in-flight callback
        // may be completing its final generation ownership check.
        let watchers = std::mem::replace(
            &mut owned.watchers,
            ResourceWatchers {
                claude: None,
                codex: None,
                publication: Arc::new(Mutex::new(())),
            },
        );
        drop(watchers);
        kill_sidecar(&mut owned.sidecar, log_path);
    }
    state.effective_port.store(0, Ordering::SeqCst);
}

// ── Error emission ────────────────────────────────

fn emit_launch_error_str(app_handle: &AppHandle, reason: &str) {
    let log_path = log_path(app_handle);
    let payload = serde_json::json!({
        "reason": reason,
        "logPath": log_path,
    });
    log_to(&log_path, &format!("emit_launch_error: reason={reason}"));
    if let Some(w) = app_handle.get_webview_window("main") {
        if let Err(e) = w.emit("launch-error", payload) {
            log_to(&log_path, &format!("emit_launch_error: emit failed: {e}"));
        }
    } else {
        log_to(&log_path, "emit_launch_error: no main window to emit to");
    }
}

fn emit_launch_error(app_handle: &AppHandle, result: &ReadyResult) {
    let reason = match result {
        ReadyResult::Ready | ReadyResult::Superseded => return,
        ReadyResult::Timeout => "timeout",
        ReadyResult::SidecarExited { code } => {
            log_to(
                &log_path(app_handle),
                &format!("emit_launch_error: zfb dev exit code = {code:?}"),
            );
            "sidecar_exited"
        }
    };
    emit_launch_error_str(app_handle, reason);
}

// ── Launch (boot + retry) ─────────────────────────

fn launch_is_current(app_handle: &AppHandle, generation: u64) -> bool {
    app_handle
        .state::<AppState>()
        .runtime
        .generation()
        .load(Ordering::SeqCst)
        == generation
}

fn emit_launch_error_if_current(app_handle: &AppHandle, generation: u64, reason: &str) {
    if launch_is_current(app_handle, generation) {
        emit_launch_error_str(app_handle, reason);
    } else {
        log_to(
            &log_path(app_handle),
            &format!("launch[{generation}]: superseded — suppressing error {reason}"),
        );
    }
}

fn publish_launch_failure(
    state: &AppState,
    generation: u64,
    desired: &EffectiveSettings,
    kind: RuntimeDiagnosticKind,
    attempted_port: Option<u16>,
    message: String,
) {
    state.runtime.publish_failed(
        RuntimeDiagnostic {
            kind,
            preferred_port: desired.preferred_port,
            attempted_port,
            message,
        },
        generation,
    );
}

#[derive(Debug)]
struct CandidateTree {
    root: PathBuf,
    marker: String,
    selection: runtime::ResourceSelection,
    counts: runtime::ResourceCounts,
}

impl Drop for CandidateTree {
    fn drop(&mut self) {
        let _ = runtime::remove_exact(&self.root);
        if let Some(parent) = self.root.parent() {
            let _ = fs::remove_dir(parent);
        }
    }
}

fn write_overview(
    docs_dir: &Path,
    kind: &str,
    enabled: bool,
    categories: &[&str],
    marker: &str,
) -> Result<(), String> {
    let dir = docs_dir.join(kind);
    fs::create_dir_all(&dir).map_err(|error| format!("create {kind} overview: {error}"))?;
    fs::write(
        dir.join("index.mdx"),
        runtime::overview_mdx(kind, enabled, categories, marker),
    )
    .map_err(|error| format!("write {kind} overview: {error}"))
}

/// A rollback and its shared-index repair are one coordinator operation. The
/// publication is attempted even when journal restoration reports an error so
/// the index describes the best tree that is actually left on disk.
fn rollback_and_republish(
    journal: runtime::ManagedTreeJournal,
    workspace: &Path,
    docs_dir: &Path,
) -> Result<(), String> {
    let rollback = journal
        .rollback()
        .map_err(|error| format!("managed-tree restore failed: {error}"));
    let publication = publish_search_index(workspace, docs_dir)
        .map_err(|error| format!("restored search-index publication failed: {error}"));
    match (rollback, publication) {
        (Ok(()), Ok(())) => Ok(()),
        (Err(error), Ok(())) | (Ok(()), Err(error)) => Err(error),
        (Err(restore), Err(publication)) => Err(format!("{restore}; {publication}")),
    }
}

/// Generate a complete selected-resource candidate away from the served docs
/// tree. Disabled paths are never dereferenced or passed to a generator.
fn build_candidate(
    workspace: &Path,
    desired: &EffectiveSettings,
    generation: u64,
    log_path: &str,
) -> Result<CandidateTree, String> {
    let selection = runtime::ResourceSelection::from_effective(desired);
    let marker = runtime::transition_marker(generation)
        .map_err(|error| format!("create transition marker: {error}"))?;
    let transitions = workspace.join(".ccresdoc-resource-transitions");
    let root = transitions.join(format!("candidate-{marker}"));
    runtime::remove_exact(&root).map_err(|error| format!("clear candidate: {error}"))?;
    fs::create_dir_all(&root).map_err(|error| format!("create candidate: {error}"))?;
    let candidate = CandidateTree {
        root,
        marker,
        selection,
        counts: runtime::ResourceCounts::default(),
    };
    let mut candidate = candidate;

    let mut claude_categories = Vec::new();
    if selection.claude {
        let source = desired
            .claude_dir
            .as_ref()
            .ok_or_else(|| "Claude is enabled without an effective source".to_string())?;
        let config = GenConfig {
            claude_dir: source.clone(),
            project_root: source.clone(),
            docs_dir: candidate.root.clone(),
        };
        let report = ccresdoc_claude_md::generate(&config)
            .map_err(|error| format!("Claude candidate generation failed: {error}"))?;
        candidate.counts.claude_md = report.claude_md;
        candidate.counts.claude_commands = report.commands;
        candidate.counts.claude_skills = report.skills;
        candidate.counts.claude_agents = report.agents;
        if report.claude_md > 0 {
            claude_categories.push("claude-md");
        }
        if report.commands > 0 {
            claude_categories.push("claude-commands");
        }
        if report.skills > 0 {
            claude_categories.push("claude-skills");
        }
        if report.agents > 0 {
            claude_categories.push("claude-agents");
        }
        log_to(
            log_path,
            &format!(
                "launch[{generation}]: Claude candidate — claude_md={} commands={} skills={} agents={}",
                report.claude_md, report.commands, report.skills, report.agents
            ),
        );
    }

    let mut codex_categories = Vec::new();
    if selection.codex {
        let source = desired
            .codex_dir
            .as_ref()
            .ok_or_else(|| "Codex is enabled without an effective source".to_string())?;
        let config = CodexConfig {
            codex_dir: source.clone(),
            project_root: source.clone(),
            docs_dir: candidate.root.clone(),
        };
        let report = generate_codex(&config)
            .map_err(|error| format!("Codex candidate generation failed: {error}"))?;
        candidate.counts.codex_agents_md = report.agents_md;
        candidate.counts.codex_config = report.config;
        candidate.counts.codex_agents = report.agents;
        candidate.counts.codex_hooks = report.hooks;
        candidate.counts.codex_rules = report.rules;
        candidate.counts.codex_skills = report.skills;
        candidate.counts.codex_warnings = report.warnings.len();
        if report.agents_md > 0 {
            codex_categories.push("codex-agents-md");
        }
        if report.config > 0 {
            codex_categories.push("codex-config");
        }
        if report.agents > 0 {
            codex_categories.push("codex-agents");
        }
        if report.hooks > 0 {
            codex_categories.push("codex-hooks");
        }
        if report.rules > 0 {
            codex_categories.push("codex-rules");
        }
        if report.skills > 0 {
            codex_categories.push("codex-skills");
        }
        log_to(
            log_path,
            &format!(
                "launch[{generation}]: Codex candidate — agents_md={} config={} agents={} hooks={} rules={} skills={} warnings={}",
                report.agents_md,
                report.config,
                report.agents,
                report.hooks,
                report.rules,
                report.skills,
                report.warnings.len()
            ),
        );
    }

    write_overview(
        &candidate.root,
        "claude",
        selection.claude,
        &claude_categories,
        &candidate.marker,
    )?;
    write_overview(
        &candidate.root,
        "codex",
        selection.codex,
        &codex_categories,
        &candidate.marker,
    )?;
    Ok(candidate)
}

fn start_resource_watchers(
    app_handle: &AppHandle,
    desired: &EffectiveSettings,
    workspace: &Path,
    docs_dir: &Path,
    generation: u64,
    marker: &str,
    log_path: &str,
) -> Result<ResourceWatchers, String> {
    let mut watchers = ResourceWatchers {
        claude: None,
        codex: None,
        publication: Arc::new(Mutex::new(())),
    };
    if desired.claude_resources {
        let source = desired
            .claude_dir
            .as_ref()
            .ok_or_else(|| "Claude is enabled without an effective source".to_string())?;
        let callback_app = app_handle.clone();
        let callback_log = log_path.to_string();
        let callback_docs = docs_dir.to_path_buf();
        let callback_workspace = workspace.to_path_buf();
        let callback_marker = marker.to_string();
        let publication = watchers.publication.clone();
        watchers.claude = Some(
            ccresdoc_claude_md::watch(
                GenConfig {
                    claude_dir: source.clone(),
                    project_root: source.clone(),
                    docs_dir: docs_dir.to_path_buf(),
                },
                DEFAULT_DEBOUNCE,
                move |event| {
                    if !watcher_publication_allowed(&callback_app, generation) {
                        return;
                    }
                    match event {
                        WatchEvent::Regenerated(report) => {
                            let _publication = publication.lock().unwrap();
                            if !watcher_publication_allowed(&callback_app, generation) {
                                return;
                            }
                            let mut categories = Vec::new();
                            if report.claude_md > 0 {
                                categories.push("claude-md");
                            }
                            if report.commands > 0 {
                                categories.push("claude-commands");
                            }
                            if report.skills > 0 {
                                categories.push("claude-skills");
                            }
                            if report.agents > 0 {
                                categories.push("claude-agents");
                            }
                            if let Err(error) = write_overview(
                                &callback_docs,
                                "claude",
                                true,
                                &categories,
                                &callback_marker,
                            ) {
                                log_to(&callback_log, &format!("watch[{generation}]: Claude overview error: {error}"));
                            }
                            if let Err(error) =
                                publish_search_index(&callback_workspace, &callback_docs)
                            {
                                log_to(
                                    &callback_log,
                                    &format!(
                                        "watch[{generation}]: shared search-index error after Claude regeneration: {error}"
                                    ),
                                );
                            }
                            let counts = {
                                let state = callback_app.state::<AppState>();
                                let mut resources = state.resources.lock().unwrap();
                                resources.as_mut().and_then(|runtime| {
                                    (runtime.generation == generation).then(|| {
                                        runtime.counts.claude_md = report.claude_md;
                                        runtime.counts.claude_commands = report.commands;
                                        runtime.counts.claude_skills = report.skills;
                                        runtime.counts.claude_agents = report.agents;
                                        runtime.counts.clone()
                                    })
                                })
                            };
                            if let Some(counts) = counts {
                                callback_app
                                    .state::<AppState>()
                                    .runtime
                                    .publish_generated(counts, generation);
                            }
                            log_to(&callback_log, &format!(
                                "watch[{generation}]: Claude regenerated — claude_md={} commands={} skills={} agents={}",
                                report.claude_md, report.commands, report.skills, report.agents
                            ));
                        }
                        WatchEvent::Error(error) => log_to(
                            &callback_log,
                            &format!("watch[{generation}]: Claude error: {error}"),
                        ),
                    }
                },
            )
            .map_err(|error| format!("start Claude watcher: {error}"))?,
        );
    }
    if desired.codex_resources {
        let source = desired
            .codex_dir
            .as_ref()
            .ok_or_else(|| "Codex is enabled without an effective source".to_string())?;
        let callback_app = app_handle.clone();
        let callback_log = log_path.to_string();
        let callback_docs = docs_dir.to_path_buf();
        let callback_workspace = workspace.to_path_buf();
        let callback_marker = marker.to_string();
        let publication = watchers.publication.clone();
        watchers.codex = Some(
            watch_codex(
                CodexConfig {
                    codex_dir: source.clone(),
                    project_root: source.clone(),
                    docs_dir: docs_dir.to_path_buf(),
                },
                DEFAULT_DEBOUNCE,
                move |event| {
                    if !watcher_publication_allowed(&callback_app, generation) {
                        return;
                    }
                    match event {
                        CodexWatchEvent::Regenerated(report) => {
                            let _publication = publication.lock().unwrap();
                            if !watcher_publication_allowed(&callback_app, generation) {
                                return;
                            }
                            let mut categories = Vec::new();
                            if report.agents_md > 0 {
                                categories.push("codex-agents-md");
                            }
                            if report.config > 0 {
                                categories.push("codex-config");
                            }
                            if report.agents > 0 {
                                categories.push("codex-agents");
                            }
                            if report.hooks > 0 {
                                categories.push("codex-hooks");
                            }
                            if report.rules > 0 {
                                categories.push("codex-rules");
                            }
                            if report.skills > 0 {
                                categories.push("codex-skills");
                            }
                            if let Err(error) = write_overview(
                                &callback_docs,
                                "codex",
                                true,
                                &categories,
                                &callback_marker,
                            ) {
                                log_to(&callback_log, &format!("watch[{generation}]: Codex overview error: {error}"));
                            }
                            if let Err(error) =
                                publish_search_index(&callback_workspace, &callback_docs)
                            {
                                log_to(
                                    &callback_log,
                                    &format!(
                                        "watch[{generation}]: shared search-index error after Codex regeneration: {error}"
                                    ),
                                );
                            }
                            let counts = {
                                let state = callback_app.state::<AppState>();
                                let mut resources = state.resources.lock().unwrap();
                                resources.as_mut().and_then(|runtime| {
                                    (runtime.generation == generation).then(|| {
                                        runtime.counts.codex_agents_md = report.agents_md;
                                        runtime.counts.codex_config = report.config;
                                        runtime.counts.codex_agents = report.agents;
                                        runtime.counts.codex_hooks = report.hooks;
                                        runtime.counts.codex_rules = report.rules;
                                        runtime.counts.codex_skills = report.skills;
                                        runtime.counts.codex_warnings = report.warnings.len();
                                        runtime.counts.clone()
                                    })
                                })
                            };
                            if let Some(counts) = counts {
                                callback_app
                                    .state::<AppState>()
                                    .runtime
                                    .publish_generated(counts, generation);
                            }
                            log_to(&callback_log, &format!(
                                "watch[{generation}]: Codex regenerated — agents_md={} config={} agents={} hooks={} rules={} skills={} warnings={}",
                                report.agents_md,
                                report.config,
                                report.agents,
                                report.hooks,
                                report.rules,
                                report.skills,
                                report.warnings.len()
                            ));
                        }
                        CodexWatchEvent::Error { source, error } => log_to(
                            &callback_log,
                            &format!("watch[{generation}]: Codex {source:?} error: {error}"),
                        ),
                    }
                },
            )
            .map_err(|error| format!("start Codex watcher: {error}"))?,
        );
    }
    Ok(watchers)
}

fn relaunch_previous_runtime(
    app_handle: &AppHandle,
    previous: PreviousRuntime,
    generation: u64,
    log_path: &str,
) -> Result<(ResourceRuntime, PortChoice), String> {
    let docs_dir = previous.workspace.join("src").join("content").join("docs");
    let selection = runtime::ResourceSelection::from_effective(&previous.effective);
    let watchers = start_resource_watchers(
        app_handle,
        &previous.effective,
        &previous.workspace,
        &docs_dir,
        generation,
        &previous.marker,
        log_path,
    )?;
    let zfb_bin = resolve_zfb_binary(&previous.workspace)?;
    let port = previous.effective.effective_port;
    let mut sidecar = match spawn_zfb_dev(&zfb_bin, &previous.workspace, port, log_path) {
        Ok(sidecar) => sidecar,
        Err(error) => {
            drop(watchers);
            return Err(error);
        }
    };
    let state = app_handle.state::<AppState>();
    let ready = runtime::wait_for_ready(
        port,
        READY_TIMEOUT,
        (state.runtime.generation(), generation),
        || match sidecar.child.try_wait() {
            Ok(Some(status)) => Some(status.code()),
            _ => None,
        },
        |port| {
            runtime::probe_resource_readiness(
                port,
                selection,
                &previous.marker,
                Duration::from_secs(1),
            )
        },
    );
    if ready != ReadyResult::Ready {
        drop(watchers);
        kill_sidecar(&mut sidecar, log_path);
        return Err(format!("restored runtime readiness failed: {ready:?}"));
    }
    let choice = PortChoice {
        preferred_port: previous.effective.preferred_port,
        effective_port: port,
        fallback_used: previous.effective.preferred_port != port,
    };
    Ok((
        ResourceRuntime {
            generation,
            marker: previous.marker,
            selection,
            effective: previous.effective,
            counts: previous.counts,
            workspace: previous.workspace,
            sidecar,
            watchers,
        },
        choice,
    ))
}

fn recover_previous_runtime(
    app_handle: &AppHandle,
    resources: &Arc<Mutex<Option<ResourceRuntime>>>,
    previous: Option<PreviousRuntime>,
    generation: u64,
    log_path: &str,
) -> Result<(), String> {
    let previous = previous.ok_or_else(|| "no previous active runtime".to_string())?;
    let active = previous.effective.clone();
    let counts = previous.counts.clone();
    let (runtime, choice) = relaunch_previous_runtime(app_handle, previous, generation, log_path)?;
    let state = app_handle.state::<AppState>();
    if let Err(mut unpublished) = publish_owned_resource_if_current(
        resources,
        &state.shutting_down,
        state.runtime.generation(),
        generation,
        runtime,
    ) {
        drop(unpublished.watchers);
        kill_sidecar(&mut unpublished.sidecar, log_path);
        return Err("restored runtime superseded before publication".to_string());
    }
    state
        .effective_port
        .store(choice.effective_port, Ordering::SeqCst);
    state.runtime.publish_ready(active, choice, generation);
    state.runtime.publish_generated(counts, generation);
    Ok(())
}

/// The full node-free boot, runnable from initial setup, Refresh, and Retry.
/// Resolves workspace + zfb binary, builds/promotes the selected candidate,
/// starts its exact watcher set, spawns `zfb dev`, polls readiness, then
/// publishes and navigates.
///
/// The runtime coordinator's generation guards against stale terminal work,
/// while its serialized apply lease prevents interleaved replacement.
fn launch(app_handle: &AppHandle, my_gen: u64, desired: EffectiveSettings) {
    let log_path = log_path(app_handle);
    let resources_arc = app_handle.state::<AppState>().resources.clone();
    let state = app_handle.state::<AppState>();

    if !launch_is_current(app_handle, my_gen) {
        log_to(
            &log_path,
            &format!("launch[{my_gen}]: superseded before acquiring launch lock"),
        );
        return;
    }

    log_to(&log_path, &format!("launch[{my_gen}]: start"));

    // 1. Resolve a writable workspace.
    let workspace = match resolve_workspace(app_handle, &log_path) {
        Ok(w) => w.path().to_path_buf(),
        Err(e) => {
            log_to(
                &log_path,
                &format!("launch: workspace resolution failed: {e}"),
            );
            publish_launch_failure(
                &state,
                my_gen,
                &desired,
                RuntimeDiagnosticKind::WorkspaceUnavailable,
                None,
                e,
            );
            emit_launch_error_if_current(app_handle, my_gen, "workspace_unavailable");
            return;
        }
    };

    // 2. Resolve the native zfb binary (missing node_modules → error UI).
    let zfb_bin = match resolve_zfb_binary(&workspace) {
        Ok(b) => b,
        Err(e) => {
            log_to(&log_path, &format!("launch: zfb binary unresolved: {e}"));
            publish_launch_failure(
                &state,
                my_gen,
                &desired,
                RuntimeDiagnosticKind::ZfbBinaryMissing,
                None,
                e,
            );
            emit_launch_error_if_current(app_handle, my_gen, "zfb_binary_missing");
            return;
        }
    };

    // 3. Select before disturbing the previous working runtime whenever the
    // requested port is not the port owned by that runtime.
    let previous = state.runtime.snapshot().active;
    let mut ports = SystemPortBoundary;
    let initial_choice = if previous
        .as_ref()
        .is_some_and(|active| active.effective_port == desired.preferred_port)
    {
        Ok(PortChoice {
            preferred_port: desired.preferred_port,
            effective_port: desired.preferred_port,
            fallback_used: false,
        })
    } else {
        runtime::choose_port(
            &mut ports,
            desired.preferred_port,
            desired.fallback_to_free_port,
        )
    };
    let mut choice = match initial_choice {
        Ok(choice) => choice,
        Err(error) => {
            let diagnostic = RuntimeDiagnostic {
                kind: if matches!(error, runtime::PortError::PreferredOccupied { .. }) {
                    RuntimeDiagnosticKind::PreferredPortOccupied
                } else {
                    RuntimeDiagnosticKind::SpawnFailed
                },
                preferred_port: desired.preferred_port,
                attempted_port: Some(desired.preferred_port),
                message: error.to_string(),
            };
            state.runtime.publish_failed(diagnostic, my_gen);
            emit_launch_error_if_current(app_handle, my_gen, "preferred_port_occupied");
            return;
        }
    };

    // 4. Build the entire four-state candidate outside the served tree. This
    // is the only point at which selected sources are inspected.
    let candidate = match build_candidate(&workspace, &desired, my_gen, &log_path) {
        Ok(candidate) => candidate,
        Err(error) => {
            publish_launch_failure(
                &state,
                my_gen,
                &desired,
                RuntimeDiagnosticKind::GenerateFailed,
                None,
                error,
            );
            emit_launch_error_if_current(app_handle, my_gen, "generate_failed");
            return;
        }
    };

    if !launch_is_current(app_handle, my_gen) {
        log_to(
            &log_path,
            &format!("launch[{my_gen}]: superseded after generation"),
        );
        return;
    }

    // 5. Cutover begins here. Capture metadata, then synchronously stop/join
    // only the cohesive app-owned watcher/sidecar set.
    let previous_runtime = resources_arc.lock().unwrap().take();
    let previous_metadata = previous_runtime.as_ref().map(|runtime| PreviousRuntime {
        marker: runtime.marker.clone(),
        // The settings snapshot is authoritative for restart-free appearance
        // changes; the owned runtime retains the source/port identity.
        effective: previous
            .clone()
            .unwrap_or_else(|| runtime.effective.clone()),
        counts: runtime.counts.clone(),
        workspace: runtime.workspace.clone(),
    });
    if let Some(mut old) = previous_runtime {
        drop(old.watchers);
        kill_sidecar(&mut old.sidecar, &log_path);
    }
    state.effective_port.store(0, Ordering::SeqCst);

    let docs_dir = workspace.join("src").join("content").join("docs");
    let backup_dir = workspace
        .join(".ccresdoc-resource-transitions")
        .join(format!("backup-{}", candidate.marker));
    let journal =
        match runtime::ManagedTreeJournal::promote(&docs_dir, &candidate.root, &backup_dir) {
            Ok(journal) => journal,
            Err(error) => {
                let mut diagnostic = RuntimeDiagnostic {
                    kind: RuntimeDiagnosticKind::PromotionFailed,
                    preferred_port: desired.preferred_port,
                    attempted_port: None,
                    message: format!("candidate promotion/restore failed: {error}"),
                };
                if let Err(publication) = publish_search_index(&workspace, &docs_dir) {
                    diagnostic.kind = RuntimeDiagnosticKind::RestoreFailed;
                    diagnostic.message.push_str(&format!(
                        "; restored search-index publication failed: {publication}"
                    ));
                }
                if previous_metadata.is_some() {
                    if let Err(relaunch) = recover_previous_runtime(
                        app_handle,
                        &resources_arc,
                        previous_metadata,
                        my_gen,
                        &log_path,
                    ) {
                        diagnostic.kind = RuntimeDiagnosticKind::RelaunchFailed;
                        diagnostic
                            .message
                            .push_str(&format!("; previous relaunch failed: {relaunch}"));
                        state.runtime.clear_active(my_gen);
                    }
                } else {
                    state.runtime.clear_active(my_gen);
                }
                state.runtime.publish_failed(diagnostic, my_gen);
                emit_launch_error_if_current(app_handle, my_gen, "promotion_failed");
                return;
            }
        };

    // The old sidecar is stopped, so promote + publication form an
    // unobservable cutover. The new sidecar and watchers never start against a
    // tree whose shared index still describes the previous selection.
    if let Err(error) = publish_search_index(&workspace, &docs_dir) {
        let rollback = rollback_and_republish(journal, &workspace, &docs_dir);
        let mut kind = RuntimeDiagnosticKind::PromotionFailed;
        let mut message = format!("candidate search-index publication failed: {error}");
        if let Err(restore) = rollback {
            kind = RuntimeDiagnosticKind::RestoreFailed;
            message.push_str(&format!("; {restore}"));
        }
        if kind != RuntimeDiagnosticKind::RestoreFailed && previous_metadata.is_some() {
            if let Err(relaunch) = recover_previous_runtime(
                app_handle,
                &resources_arc,
                previous_metadata,
                my_gen,
                &log_path,
            ) {
                kind = RuntimeDiagnosticKind::RelaunchFailed;
                message.push_str(&format!("; previous relaunch failed: {relaunch}"));
                state.runtime.clear_active(my_gen);
            }
        } else if kind == RuntimeDiagnosticKind::RestoreFailed {
            state.runtime.clear_active(my_gen);
        }
        publish_launch_failure(&state, my_gen, &desired, kind, None, message);
        emit_launch_error_if_current(app_handle, my_gen, "promotion_failed");
        return;
    }

    // Enabled watcher startup is part of activation. A partial second-watcher
    // failure drops/joins the first before the tree journal is rolled back.
    let watchers = match start_resource_watchers(
        app_handle,
        &desired,
        &workspace,
        &docs_dir,
        my_gen,
        &candidate.marker,
        &log_path,
    ) {
        Ok(watchers) => watchers,
        Err(error) => {
            let rollback = rollback_and_republish(journal, &workspace, &docs_dir);
            let mut kind = RuntimeDiagnosticKind::WatchFailed;
            let mut message = match rollback {
                Ok(()) => error,
                Err(restore) => {
                    kind = RuntimeDiagnosticKind::RestoreFailed;
                    format!("{error}; {restore}")
                }
            };
            if kind != RuntimeDiagnosticKind::RestoreFailed && previous_metadata.is_some() {
                if let Err(relaunch) = recover_previous_runtime(
                    app_handle,
                    &resources_arc,
                    previous_metadata,
                    my_gen,
                    &log_path,
                ) {
                    kind = RuntimeDiagnosticKind::RelaunchFailed;
                    message.push_str(&format!("; previous relaunch failed: {relaunch}"));
                    state.runtime.clear_active(my_gen);
                }
            } else if kind == RuntimeDiagnosticKind::RestoreFailed {
                state.runtime.clear_active(my_gen);
            }
            publish_launch_failure(&state, my_gen, &desired, kind, None, message);
            emit_launch_error_if_current(app_handle, my_gen, "watch_failed");
            return;
        }
    };

    // 5/6. Spawn and probe. If the released preflight socket was stolen,
    // retry with a fresh OS-assigned loopback candidate, boundedly.
    let mut result = ReadyResult::Timeout;
    let mut candidate_sidecar = None;
    let mut bind_retry_exhausted = false;
    let mut spawn_failure = None;
    for attempt in 0..runtime::MAX_BIND_ATTEMPTS {
        if attempt > 0 {
            match runtime::choose_port(&mut ports, desired.preferred_port, true) {
                Ok(next) if next.effective_port != desired.preferred_port => choice = next,
                Ok(_) => match ports.fallback_candidate() {
                    Ok(port) => {
                        choice = PortChoice {
                            preferred_port: desired.preferred_port,
                            effective_port: port,
                            fallback_used: true,
                        }
                    }
                    Err(error) => {
                        log_to(&log_path, &format!("fallback allocation failed: {error}"));
                        break;
                    }
                },
                Err(error) => {
                    log_to(&log_path, &format!("fallback allocation failed: {error}"));
                    break;
                }
            }
        }
        match spawn_zfb_dev(&zfb_bin, &workspace, choice.effective_port, &log_path) {
            Ok(sidecar) => candidate_sidecar = Some(sidecar),
            Err(error) => {
                log_to(&log_path, &format!("launch: spawn failed: {error}"));
                spawn_failure = Some(error);
                result = ReadyResult::SidecarExited { code: None };
                break;
            }
        }
        result = runtime::wait_for_ready(
            choice.effective_port,
            READY_TIMEOUT,
            (state.runtime.generation(), my_gen),
            || {
                candidate_sidecar
                    .as_mut()
                    .and_then(|sidecar| match sidecar.child.try_wait() {
                        Ok(Some(status)) => Some(status.code()),
                        _ => None,
                    })
            },
            |port| {
                runtime::probe_resource_readiness(
                    port,
                    candidate.selection,
                    &candidate.marker,
                    Duration::from_secs(1),
                )
            },
        );
        if result != (ReadyResult::SidecarExited { code: None })
            && !matches!(result, ReadyResult::SidecarExited { .. })
        {
            break;
        }
        if let Some(mut sidecar) = candidate_sidecar.take() {
            kill_sidecar(&mut sidecar, &log_path);
        }
        if !desired.fallback_to_free_port
            || ports.is_available(choice.effective_port).unwrap_or(true)
        {
            break;
        }
        bind_retry_exhausted = attempt + 1 == runtime::MAX_BIND_ATTEMPTS;
    }

    // 7. Skip navigate/emit if a newer launch superseded this one.
    if !launch_is_current(app_handle, my_gen) {
        log_to(
            &log_path,
            "launch: superseded by a newer launch — skipping navigate/emit",
        );
        drop(watchers);
        if let Some(mut sidecar) = candidate_sidecar {
            kill_sidecar(&mut sidecar, &log_path);
        }
        match rollback_and_republish(journal, &workspace, &docs_dir) {
            Err(error) => log_to(
                &log_path,
                &format!("launch: supersession restore failed: {error}"),
            ),
            Ok(()) if !state.shutting_down.load(Ordering::SeqCst) => {
                let handoff_generation = state.runtime.generation().load(Ordering::SeqCst);
                if previous_metadata.is_some() {
                    if let Err(error) = recover_previous_runtime(
                        app_handle,
                        &resources_arc,
                        previous_metadata,
                        handoff_generation,
                        &log_path,
                    ) {
                        log_to(
                            &log_path,
                            &format!("launch: supersession relaunch failed: {error}"),
                        );
                        state.runtime.clear_active(handoff_generation);
                    }
                }
            }
            Ok(()) => {}
        }
        return;
    }

    match result {
        ReadyResult::Ready => {
            let sidecar = candidate_sidecar.expect("ready sidecar remains owned");
            let marker = candidate.marker.clone();
            let mut active_effective = desired.clone();
            active_effective.effective_port = choice.effective_port;
            let ready_runtime = ResourceRuntime {
                generation: my_gen,
                marker,
                selection: candidate.selection,
                effective: active_effective,
                counts: candidate.counts.clone(),
                workspace: workspace.clone(),
                sidecar,
                watchers,
            };
            if let Err(mut unpublished) = publish_owned_resource_if_current(
                &resources_arc,
                &state.shutting_down,
                state.runtime.generation(),
                my_gen,
                ready_runtime,
            ) {
                let shutting_down = state.shutting_down.load(Ordering::SeqCst);
                let handoff_generation = state.runtime.generation().load(Ordering::SeqCst);
                drop(unpublished.watchers);
                kill_sidecar(&mut unpublished.sidecar, &log_path);
                match rollback_and_republish(journal, &workspace, &docs_dir) {
                    Err(error) => log_to(
                        &log_path,
                        &format!("launch: publication restore failed: {error}"),
                    ),
                    Ok(()) if !shutting_down && previous_metadata.is_some() => {
                        if let Err(error) = recover_previous_runtime(
                            app_handle,
                            &resources_arc,
                            previous_metadata,
                            handoff_generation,
                            &log_path,
                        ) {
                            log_to(
                                &log_path,
                                &format!("launch: publication relaunch failed: {error}"),
                            );
                            state.runtime.clear_active(handoff_generation);
                        }
                    }
                    Ok(()) => {}
                }
                return;
            }
            state
                .effective_port
                .store(choice.effective_port, Ordering::SeqCst);
            state.runtime.publish_ready(desired, choice, my_gen);
            state
                .runtime
                .publish_generated(candidate.counts.clone(), my_gen);
            if let Err(error) = journal.commit() {
                log_to(
                    &log_path,
                    &format!("launch: backup cleanup failed: {error}"),
                );
            }
            navigate_to_docs(app_handle)
        }
        ReadyResult::Timeout | ReadyResult::SidecarExited { .. } => {
            drop(watchers);
            if let Some(mut sidecar) = candidate_sidecar {
                kill_sidecar(&mut sidecar, &log_path);
            }
            let kind = if spawn_failure.is_some() {
                RuntimeDiagnosticKind::SpawnFailed
            } else if bind_retry_exhausted {
                RuntimeDiagnosticKind::BindRetryExhausted
            } else if matches!(result, ReadyResult::Timeout) {
                RuntimeDiagnosticKind::Timeout
            } else {
                RuntimeDiagnosticKind::SidecarExited
            };
            let mut diagnostic = RuntimeDiagnostic {
                kind,
                preferred_port: desired.preferred_port,
                attempted_port: Some(choice.effective_port),
                message: spawn_failure
                    .clone()
                    .unwrap_or_else(|| format!("{result:?}")),
            };
            let restore_result = rollback_and_republish(journal, &workspace, &docs_dir);
            let had_previous = previous_metadata.is_some();
            let recovery = match restore_result {
                Ok(()) if had_previous => recover_previous_runtime(
                    app_handle,
                    &resources_arc,
                    previous_metadata,
                    my_gen,
                    &log_path,
                ),
                Ok(()) => Ok(()),
                Err(error) => Err(error),
            };
            if let Err(error) = recovery {
                log_to(&log_path, &format!("launch: recovery failed: {error}"));
                diagnostic.kind = if error.starts_with("managed-tree restore failed") {
                    RuntimeDiagnosticKind::RestoreFailed
                } else {
                    RuntimeDiagnosticKind::RelaunchFailed
                };
                diagnostic.message.push_str(&format!("; {error}"));
                state.runtime.clear_active(my_gen);
            } else if !had_previous {
                state.runtime.clear_active(my_gen);
            }
            state.runtime.publish_failed(diagnostic, my_gen);
            if spawn_failure.is_some() {
                emit_launch_error_if_current(app_handle, my_gen, "spawn_failed");
            } else {
                emit_launch_error(app_handle, &result);
            }
        }
        ReadyResult::Superseded => {}
    }
}

fn start_launch(app_handle: &AppHandle) {
    let state = app_handle.state::<AppState>();
    if state.shutting_down.load(Ordering::SeqCst) {
        return;
    }
    let generation = state.runtime.claim_generation();
    let authored = state.runtime.snapshot().authored;
    let desired = authored.effective.clone();
    state.runtime.publish_starting(authored, generation);
    let coordinator = state.runtime.clone();
    let handle = app_handle.clone();
    thread::spawn(move || {
        coordinator.with_serialized_apply(|| launch(&handle, generation, desired))
    });
}

/// The JS that applies a zoom level to the page body. Used both by `apply_zoom`
/// (menu actions) and by the `on_page_load` handler that re-applies the stored
/// zoom after every navigation — `document.body.style.zoom` is page-scoped, so
/// a `navigate_to_docs` (Refresh, launch, retry) would otherwise reset it to 1.
/// Guards on `document.body` existing so it is harmless if eval'd before the
/// body is parsed.
fn zoom_script(level: f64) -> String {
    format!("if (document.body) {{ document.body.style.zoom = '{level}'; }}")
}

fn apply_zoom(app_handle: &AppHandle, level: f64) {
    let state = app_handle.state::<AppState>();
    *state.zoom.lock().unwrap() = level;
    if let Some(w) = app_handle.get_webview_window("main") {
        let _ = w.eval(zoom_script(level));
    }
}

/// Re-apply the stored zoom to the main window's current page. Called after a
/// page finishes loading (via `on_page_load`) so a navigation does not lose the
/// user's chosen zoom. No-op at the default 1.0 level.
fn reapply_zoom(app_handle: &AppHandle) {
    let level = *app_handle.state::<AppState>().zoom.lock().unwrap();
    if let Some(w) = app_handle.get_webview_window("main") {
        let _ = w.eval(zoom_script(level));
    }
}

// ── Navigation filter ─────────────────────────────

/// Allow in-window navigation only for the pinned doc-site origin
/// (`localhost:PORT` / `127.0.0.1:PORT`), tauri/asset protocol URLs, and
/// about:blank. Any other http(s) URL is opened in the OS browser and rejected
/// for in-window navigation.
fn allow_navigation(url: &tauri::Url, effective_port: u16) -> bool {
    match runtime::navigation_decision(url, (effective_port != 0).then_some(effective_port)) {
        NavigationDecision::Allow => true,
        NavigationDecision::OpenExternal => {
            if let Err(e) = open::that(url.as_str()) {
                eprintln!("allow_navigation: failed to open {url} in OS browser: {e}");
            }
            false
        }
        NavigationDecision::Reject => false,
    }
}

fn create_main_window(
    app: &AppHandle,
    navigation_port: Arc<AtomicU16>,
    appearance_seed: appearance::BootstrapSeed,
) -> Result<(), tauri::Error> {
    let mut builder = WebviewWindowBuilder::new(app, "main", WebviewUrl::App("index.html".into()))
        .title("CCResDoc")
        .inner_size(1200.0, 800.0)
        .initialization_script(appearance::initialization_script(&appearance_seed))
        .on_navigation(move |url| allow_navigation(url, navigation_port.load(Ordering::SeqCst)))
        .on_page_load(|window, payload| {
            if let tauri::webview::PageLoadEvent::Finished = payload.event() {
                reapply_zoom(window.app_handle());
            }
        });
    if ephemeral_webview_enabled() {
        builder = builder.incognito(true);
    }
    builder.build()?;
    Ok(())
}

// ── Main ──────────────────────────────────────────

fn main() {
    #[cfg(unix)]
    register_sidecar_process_exit_hook().expect("sidecar process-exit hook must register");

    let home = match home_dir() {
        Some(home) => PathBuf::from(home),
        None => PathBuf::from("/"),
    };
    let config_path = settings::resolve_config_path()
        .unwrap_or_else(|_| home.join(".config/ccresdoc/config.toml"));
    let settings_store =
        SettingsStore::with_theme_packs(config_path, home, settings::bundled_theme_pack_slugs());
    let settings_snapshot = settings_store.load();
    let appearance_seed =
        appearance::bootstrap_seed(&settings_snapshot, settings_store.available_theme_packs());
    let runtime = Arc::new(runtime::ApplyCoordinator::new(settings_snapshot));
    let effective_port = Arc::new(AtomicU16::new(0));
    let app_state = AppState {
        resources: Arc::new(Mutex::new(None)),
        zoom: Mutex::new(1.0),
        log_path: Mutex::new(String::new()),
        runtime,
        settings_store,
        appearance: appearance::AppearanceState::default(),
        effective_port: effective_port.clone(),
        shutting_down: AtomicBool::new(false),
    };
    let resources_for_exit = app_state.resources.clone();
    let navigation_port = effective_port.clone();
    #[cfg(target_os = "macos")]
    let reopen_navigation_port = navigation_port.clone();

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(app_state)
        .invoke_handler(tauri::generate_handler![
            settings_commands::retry_launch,
            settings_commands::open_settings_window,
            settings_commands::get_settings_snapshot,
            settings_commands::update_appearance,
            settings_commands::preview_appearance,
            settings_commands::clear_appearance_preview,
            settings_commands::validate_settings_draft,
            settings_commands::save_and_apply_settings,
            settings_commands::rebase_stale_settings,
            settings_commands::replace_malformed_settings,
            settings_commands::pick_source_directory,
            settings_commands::open_config_file,
            settings_commands::reveal_config_file,
        ])
        .setup(move |app| {
            // Resolve the log path under the app-data dir (always writable).
            let app_data = app
                .path()
                .app_data_dir()
                .unwrap_or_else(|_| PathBuf::from("/tmp"));
            let _ = fs::create_dir_all(&app_data);
            let log_path = app_data.join("ccresdoc.log").to_string_lossy().into_owned();
            {
                let state = app.state::<AppState>();
                *state.log_path.lock().unwrap() = log_path.clone();
            }
            log_to(&log_path, "setup: starting CCResDoc");

            // ── Menu ──
            app.set_menu(menu::build(app)?)?;

            app.on_menu_event(|app_handle, event| match event.id().as_ref() {
                SETTINGS_MENU_ID => {
                    if let Err(error) = open_or_focus_settings(app_handle) {
                        log_to(
                            &self::log_path(app_handle),
                            &format!("open Settings failed: {error}"),
                        );
                    }
                }
                "refresh" => {
                    navigate_to_loading(app_handle);
                    start_launch(app_handle);
                }
                "devtools" => {
                    if let Some(w) = app_handle.get_webview_window("main") {
                        if w.is_devtools_open() {
                            w.close_devtools();
                        } else {
                            w.open_devtools();
                        }
                    }
                }
                "actual_size" => apply_zoom(app_handle, 1.0),
                "zoom_in" => {
                    let state = app_handle.state::<AppState>();
                    let z = (*state.zoom.lock().unwrap() + 0.1).min(3.0);
                    apply_zoom(app_handle, z);
                }
                "zoom_out" => {
                    let state = app_handle.state::<AppState>();
                    let z = (*state.zoom.lock().unwrap() - 0.1).max(0.1);
                    apply_zoom(app_handle, z);
                }
                _ => {}
            });

            // ── Window ──
            // Open immediately with the bundled loading page (anti-white-flash),
            // then a background thread does the node-free boot and navigates.
            // Use App("index.html") (the bundled frontendDist page) explicitly —
            // NOT WebviewUrl::default(), which in dev resolves to `devUrl`
            // (:4892) and would show connection-refused before zfb dev binds.
            // The host owns `zfb dev` in BOTH dev and prod, so the loading page
            // + readiness-navigate flow must run in both modes.
            create_main_window(
                app.handle(),
                navigation_port.clone(),
                appearance_seed.clone(),
            )?;

            start_launch(app.handle());

            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(move |app_handle, event| {
            // Main-window destruction stops owned runtime resources but leaves
            // the macOS app available for Dock reopen. Settings close is
            // intercepted and hidden. App-level Quit still tears down through
            // ExitRequested/Exit; take-once cleanup makes repeated events safe.
            let action = match &event {
                tauri::RunEvent::WindowEvent { label, event, .. } => {
                    let kind = match event {
                        tauri::WindowEvent::CloseRequested { .. } => "close_requested",
                        tauri::WindowEvent::Destroyed => "destroyed",
                        _ => "other",
                    };
                    lifecycle_action(kind, Some(label))
                }
                tauri::RunEvent::ExitRequested { .. } => lifecycle_action("exit_requested", None),
                tauri::RunEvent::Exit => lifecycle_action("exit", None),
                #[cfg(target_os = "macos")]
                tauri::RunEvent::Reopen { .. } => lifecycle_action("reopen", None),
                _ => LifecycleAction::Ignore,
            };
            match action {
                LifecycleAction::HideSettings => {
                    if let tauri::RunEvent::WindowEvent {
                        event: tauri::WindowEvent::CloseRequested { api, .. },
                        ..
                    } = &event
                    {
                        api.prevent_close();
                        let state = app_handle.state::<AppState>();
                        state.appearance.clear_preview();
                        let authoritative = state.settings_store.load();
                        let _ = app_handle.emit(
                            appearance::APPEARANCE_EVENT,
                            state.appearance.envelope(&authoritative),
                        );
                        if let Some(window) = app_handle.get_webview_window(SETTINGS_WINDOW_LABEL) {
                            let _ = window.eval(
                                "window.dispatchEvent(new Event('ccresdoc-settings-native-close'))",
                            );
                            let _ = window.hide();
                        }
                        if let Some(main) = app_handle.get_webview_window("main") {
                            let _ = main.set_focus();
                        }
                    }
                }
                LifecycleAction::StopForMainClose => {
                    let log_path = log_path(app_handle);
                    teardown(app_handle, &resources_for_exit, &log_path, false);
                }
                LifecycleAction::Shutdown => {
                    let log_path = log_path(app_handle);
                    teardown(app_handle, &resources_for_exit, &log_path, true);
                }
                LifecycleAction::ReopenMain => {
                    #[cfg(target_os = "macos")]
                    if let Some(main) = app_handle.get_webview_window("main") {
                        let _ = main.show();
                        if main.is_minimized().unwrap_or(false) {
                            let _ = main.unminimize();
                        }
                        let _ = main.set_focus();
                    } else {
                        let state = app_handle.state::<AppState>();
                        let seed = appearance::bootstrap_seed(
                            &state.settings_store.load(),
                            state.settings_store.available_theme_packs(),
                        );
                        match create_main_window(app_handle, reopen_navigation_port.clone(), seed) {
                            Ok(()) => {
                                if app_handle.state::<AppState>().runtime.snapshot().phase
                                    == runtime::RuntimePhase::Ready
                                {
                                    navigate_fresh_main_to_docs(app_handle);
                                } else {
                                    start_launch(app_handle);
                                }
                            }
                            Err(error) => log_to(
                                &log_path(app_handle),
                                &format!("reopen main window failed: {error}"),
                            ),
                        }
                    }
                }
                LifecycleAction::Ignore => {}
            }
        });
}

// ── Tests ─────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn read_tauri_conf() -> serde_json::Value {
        let conf_path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tauri.conf.json");
        let raw = std::fs::read_to_string(&conf_path).expect("Failed to read tauri.conf.json");
        serde_json::from_str(&raw).expect("Failed to parse tauri.conf.json")
    }

    #[test]
    fn docs_path_starts_with_slash() {
        assert!(
            runtime::DOCS_PATH.starts_with('/'),
            "DOCS_PATH must start with /"
        );
    }

    #[test]
    fn stale_or_shutdown_launches_cannot_publish_owned_resources() {
        assert!(resource_publication_allowed(false, 7, 7));
        assert!(!resource_publication_allowed(false, 8, 7));
        assert!(!resource_publication_allowed(true, 7, 7));
    }

    #[test]
    fn owned_resource_publication_rechecks_the_lease_inside_the_slot_lock() {
        let slot = Mutex::new(None);
        let shutting_down = AtomicBool::new(false);
        let generation = AtomicU64::new(7);

        assert_eq!(
            publish_owned_resource_if_current(&slot, &shutting_down, &generation, 6, "stale",),
            Err("stale")
        );
        assert!(slot.lock().unwrap().is_none());

        shutting_down.store(true, Ordering::SeqCst);
        assert_eq!(
            publish_owned_resource_if_current(&slot, &shutting_down, &generation, 7, "shutdown",),
            Err("shutdown")
        );
        assert!(slot.lock().unwrap().is_none());

        shutting_down.store(false, Ordering::SeqCst);
        assert_eq!(
            publish_owned_resource_if_current(&slot, &shutting_down, &generation, 7, "current",),
            Ok(())
        );
        assert_eq!(*slot.lock().unwrap(), Some("current"));
    }

    #[test]
    fn ephemeral_webview_is_explicitly_opted_in() {
        assert!(!ephemeral_webview_enabled_value(None));
        assert!(!ephemeral_webview_enabled_value(Some("true")));
        assert!(ephemeral_webview_enabled_value(Some("1")));
    }

    #[test]
    fn docs_url_is_canonical_docs_shell_on_port() {
        assert_eq!(runtime::DOCS_PATH, "/docs/");
        let docs_url = runtime::docs_url(settings::DEFAULT_PORT);
        assert_eq!(
            docs_url,
            format!("http://localhost:{}/docs/", settings::DEFAULT_PORT)
        );
        let url: Result<tauri::Url, _> = docs_url.parse();
        assert!(url.is_ok(), "docs_url should parse: {docs_url}");
    }

    #[test]
    fn candidate_generation_covers_four_states_without_touching_live_tree() {
        let temp = tempfile::TempDir::new().unwrap();
        let workspace = temp.path().join("workspace");
        let live_docs = workspace.join("src/content/docs");
        fs::create_dir_all(&live_docs).unwrap();
        fs::write(live_docs.join("foreign.mdx"), "served-before-cutover").unwrap();
        let claude = temp.path().join("claude");
        let codex = temp.path().join("codex");
        fs::create_dir_all(&claude).unwrap();
        fs::create_dir_all(&codex).unwrap();
        fs::write(claude.join("CLAUDE.md"), "# Claude").unwrap();
        fs::write(codex.join("AGENTS.md"), "# Codex").unwrap();

        for (generation, claude_enabled, codex_enabled) in [
            (1, false, false),
            (2, true, false),
            (3, false, true),
            (4, true, true),
        ] {
            let effective = EffectiveSettings {
                claude_resources: claude_enabled,
                codex_resources: codex_enabled,
                claude_dir: claude_enabled.then(|| claude.clone()),
                codex_dir: codex_enabled.then(|| codex.clone()),
                appearance_mode: settings::AppearanceMode::System,
                theme_pack: "default".into(),
                preferred_port: settings::DEFAULT_PORT,
                effective_port: settings::DEFAULT_PORT,
                fallback_to_free_port: true,
            };
            let candidate = build_candidate(&workspace, &effective, generation, "").unwrap();
            assert_eq!(candidate.root.join("claude-md").exists(), claude_enabled);
            assert_eq!(
                candidate.root.join("codex-agents-md").exists(),
                codex_enabled
            );
            assert_eq!(candidate.counts.claude_md > 0, claude_enabled);
            assert_eq!(candidate.counts.codex_agents_md > 0, codex_enabled);
            let claude_overview =
                fs::read_to_string(candidate.root.join("claude/index.mdx")).unwrap();
            let codex_overview =
                fs::read_to_string(candidate.root.join("codex/index.mdx")).unwrap();
            assert!(claude_overview.contains(if claude_enabled {
                "data-ccresdoc-state=\"enabled\""
            } else {
                "data-ccresdoc-state=\"disabled\""
            }));
            assert!(codex_overview.contains(if codex_enabled {
                "data-ccresdoc-state=\"enabled\""
            } else {
                "data-ccresdoc-state=\"disabled\""
            }));
            assert!(claude_overview.contains(&candidate.marker));
            assert!(codex_overview.contains(&candidate.marker));
            assert_eq!(
                fs::read_to_string(live_docs.join("foreign.mdx")).unwrap(),
                "served-before-cutover"
            );
        }
    }

    #[test]
    fn partial_second_generator_failure_discards_candidate_and_preserves_live_tree() {
        let temp = tempfile::TempDir::new().unwrap();
        let workspace = temp.path().join("workspace");
        let live_docs = workspace.join("src/content/docs");
        let claude = temp.path().join("claude");
        fs::create_dir_all(&live_docs).unwrap();
        fs::create_dir_all(&claude).unwrap();
        fs::write(claude.join("CLAUDE.md"), "# generated first").unwrap();
        fs::write(live_docs.join("served.mdx"), "previous").unwrap();
        let effective = EffectiveSettings {
            claude_resources: true,
            codex_resources: true,
            claude_dir: Some(claude),
            // The candidate is below workspace, so Codex rejects this source
            // before walking it under the source↔docs ancestry contract.
            codex_dir: Some(workspace.clone()),
            appearance_mode: settings::AppearanceMode::System,
            theme_pack: "default".into(),
            preferred_port: settings::DEFAULT_PORT,
            effective_port: settings::DEFAULT_PORT,
            fallback_to_free_port: true,
        };
        let error = build_candidate(&workspace, &effective, 9, "").unwrap_err();
        assert!(error.contains("Codex candidate generation failed"));
        assert_eq!(
            fs::read_to_string(live_docs.join("served.mdx")).unwrap(),
            "previous"
        );
        let transitions = workspace.join(".ccresdoc-resource-transitions");
        assert!(
            !transitions.exists() || fs::read_dir(&transitions).unwrap().next().is_none(),
            "failed candidate should be cleaned exactly"
        );
    }

    #[test]
    fn promoted_and_rolled_back_trees_republish_matching_search_indexes() {
        let temp = tempfile::TempDir::new().unwrap();
        let workspace = temp.path().join("workspace");
        let docs = workspace.join("src/content/docs");
        let candidate = workspace.join("candidate");
        let backup = workspace.join("backup");
        fs::create_dir_all(docs.join("claude-md")).unwrap();
        fs::write(
            docs.join("claude-md/old.mdx"),
            "---\ntitle: Old\n---\n\nold body\n",
        )
        .unwrap();
        publish_search_index(&workspace, &docs).unwrap();
        assert!(
            fs::read_to_string(workspace.join("public/docs/search-index.json"))
                .unwrap()
                .contains("claude:claude-md/old")
        );

        fs::create_dir_all(candidate.join("codex-config")).unwrap();
        fs::write(
            candidate.join("codex-config/new.mdx"),
            "---\ntitle: New\n---\n\nnew body\n",
        )
        .unwrap();
        let journal = runtime::ManagedTreeJournal::promote(&docs, &candidate, &backup).unwrap();
        publish_search_index(&workspace, &docs).unwrap();
        let promoted = fs::read_to_string(workspace.join("public/docs/search-index.json")).unwrap();
        assert!(promoted.contains("codex:codex-config/new"));
        assert!(!promoted.contains("claude:claude-md/old"));

        rollback_and_republish(journal, &workspace, &docs).unwrap();
        let restored = fs::read_to_string(workspace.join("public/docs/search-index.json")).unwrap();
        assert!(restored.contains("claude:claude-md/old"));
        assert!(!restored.contains("codex:codex-config/new"));
    }

    #[test]
    fn appearance_bootstrap_is_document_start_not_page_load_eval() {
        let source = include_str!("main.rs");
        assert!(source.contains(".initialization_script(appearance::initialization_script"));
        assert!(source.contains("appearance::window_name_script"));
        let page_load = source
            .split(".on_page_load")
            .nth(1)
            .and_then(|tail| tail.split(".build()?").next())
            .unwrap();
        assert!(page_load.contains("reapply_zoom"));
        assert!(!page_load.contains("appearance"));
    }

    #[test]
    fn zfb_command_removes_inherited_boot_lazy() {
        let command = zfb_dev_command(Path::new("/tmp/native-zfb"), Path::new("/tmp/app"), 53003);
        let args = command
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        assert_eq!(args, ["dev", "--host", "127.0.0.1", "--port", "53003"]);
        let boot_lazy = command
            .get_envs()
            .find(|(key, _)| *key == "ZFB_DEV_BOOT_LAZY");
        assert!(
            matches!(boot_lazy, Some((_, None))),
            "ZFB_DEV_BOOT_LAZY must be explicitly removed from the child environment"
        );
    }

    #[test]
    fn claude_dir_is_absolute_and_not_home() {
        let c = PathBuf::from(home_dir().expect("HOME should resolve")).join(".claude");
        assert!(c.is_absolute(), "claude_dir must be absolute");
        assert!(
            c.ends_with(".claude"),
            "claude_dir must end with .claude, not be $HOME"
        );
    }

    #[test]
    fn zfb_platform_package_resolves_on_supported_targets() {
        // On any host this crate compiles for here, the map must hit.
        let pkg = zfb_platform_package();
        assert!(
            pkg.is_some(),
            "no zfb platform package for {}-{}",
            env::consts::OS,
            env::consts::ARCH
        );
        assert!(pkg.unwrap().starts_with("@takazudo/zfb-"));
    }

    #[test]
    fn zfb_platform_package_preserves_the_five_published_targets() {
        assert_eq!(
            zfb_platform_package_for("macos", "aarch64"),
            Some("@takazudo/zfb-darwin-arm64")
        );
        assert_eq!(
            zfb_platform_package_for("macos", "x86_64"),
            Some("@takazudo/zfb-darwin-x64")
        );
        assert_eq!(
            zfb_platform_package_for("linux", "aarch64"),
            Some("@takazudo/zfb-linux-arm64-gnu")
        );
        assert_eq!(
            zfb_platform_package_for("linux", "x86_64"),
            Some("@takazudo/zfb-linux-x64-gnu")
        );
        assert_eq!(
            zfb_platform_package_for("windows", "x86_64"),
            Some("@takazudo/zfb-win32-x64-msvc")
        );
        assert_eq!(zfb_platform_package_for("windows", "aarch64"), None);
    }

    #[test]
    fn zfb_binary_name_is_not_the_node_wrapper() {
        // Must be the bare platform binary, never `.bin/zfb` (Node shebang).
        let name = zfb_binary_name();
        assert!(name == "zfb" || name == "zfb.exe");
    }

    #[test]
    fn resolve_zfb_binary_errors_when_node_modules_absent() {
        let tmp = std::env::temp_dir().join("ccresdoc-test-no-nm");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        let res = resolve_zfb_binary(&tmp);
        assert!(res.is_err(), "missing node_modules should error");
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[cfg(unix)]
    #[test]
    fn resolve_zfb_binary_rejects_a_non_executable_file() {
        use std::os::unix::fs::PermissionsExt;

        let tmp =
            std::env::temp_dir().join(format!("ccresdoc-test-nonexec-zfb-{}", std::process::id()));
        let binary = tmp
            .join("node_modules")
            .join(zfb_platform_package().unwrap())
            .join(zfb_binary_name());
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(binary.parent().unwrap()).unwrap();
        std::fs::write(&binary, b"not executable").unwrap();
        std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o644)).unwrap();

        let error = resolve_zfb_binary(&tmp).expect_err("non-executable binary must fail");
        assert!(
            error.contains("not executable"),
            "unexpected error: {error}"
        );
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn navigation_filter_allows_localhost_only() {
        let ok: tauri::Url = "http://localhost:4892/docs/".parse().unwrap();
        let loop_ok: tauri::Url = "http://127.0.0.1:4892/".parse().unwrap();
        let external: tauri::Url = "https://example.com/".parse().unwrap();
        assert!(allow_navigation(&ok, 4892));
        assert!(allow_navigation(&loop_ok, 4892));
        assert!(
            !allow_navigation(&external, 4892),
            "external links must open in OS browser"
        );
    }

    // ── tauri.conf.json assertions ──────────────────

    #[test]
    fn tauri_conf_devurl_points_to_canonical_docs_shell() {
        let conf = read_tauri_conf();
        let dev_url = conf["build"]["devUrl"]
            .as_str()
            .expect("devUrl must be a string");
        assert_eq!(
            dev_url,
            runtime::docs_url(settings::DEFAULT_PORT),
            "devUrl should equal the canonical docs URL"
        );
    }

    #[test]
    fn tauri_conf_enables_global_tauri() {
        let conf = read_tauri_conf();
        let flag = conf["app"]["withGlobalTauri"].as_bool();
        assert_eq!(
            flag,
            Some(true),
            "app.withGlobalTauri must be true for the bundled loading page"
        );
    }

    #[test]
    fn tauri_conf_keeps_product_and_identifier() {
        let conf = read_tauri_conf();
        assert_eq!(conf["productName"].as_str(), Some("CCResDoc"));
        assert_eq!(conf["identifier"].as_str(), Some("com.takazudo.ccresdoc"));
    }

    #[test]
    fn tauri_conf_has_real_icon() {
        let conf = read_tauri_conf();
        let icons = conf["bundle"]["icon"]
            .as_array()
            .expect("bundle.icon must be an array");
        assert!(!icons.is_empty(), "bundle.icon must be populated (was [])");
    }

    #[test]
    fn tauri_conf_bundles_only_the_staged_runtime_workspace() {
        let conf = read_tauri_conf();
        let resources = conf["bundle"]["resources"].clone();
        let bundles_runtime = match &resources {
            serde_json::Value::String(s) => s.contains("runtime-workspace/"),
            serde_json::Value::Array(arr) => arr.iter().any(|v| {
                v.as_str()
                    .map(|s| s.contains("runtime-workspace/"))
                    .unwrap_or(false)
            }),
            _ => false,
        };
        assert!(
            bundles_runtime,
            "bundle.resources should include runtime-workspace/**, got: {resources}"
        );
        assert!(
            !resources.to_string().contains("../app"),
            "bundle.resources must not ship the unpruned app tree: {resources}"
        );
    }

    // ── copy_workspace / copy_dir_recursive ─────────

    /// Build a small source tree: a file, a nested subdir with a file, and a
    /// symlink. Returns the temp dir root (caller removes it).
    fn make_sample_tree(root: &Path) {
        std::fs::create_dir_all(root.join("sub")).unwrap();
        std::fs::write(root.join("top.txt"), b"top-contents").unwrap();
        std::fs::write(root.join("sub").join("nested.txt"), b"nested-contents").unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink("top.txt", root.join("link-to-top")).unwrap();
    }

    /// Assert `dst` mirrors the tree `make_sample_tree` created in `src`:
    /// identical file contents, nested structure, and a preserved symlink.
    fn assert_tree_copied(dst: &Path) {
        assert_eq!(
            std::fs::read(dst.join("top.txt")).unwrap(),
            b"top-contents",
            "top file contents must match"
        );
        assert_eq!(
            std::fs::read(dst.join("sub").join("nested.txt")).unwrap(),
            b"nested-contents",
            "nested file contents must match"
        );
        #[cfg(unix)]
        {
            let link = dst.join("link-to-top");
            let meta =
                std::fs::symlink_metadata(&link).expect("symlink entry must exist in the copy");
            assert!(
                meta.file_type().is_symlink(),
                "link-to-top must be preserved AS a symlink, not dereferenced into a regular file"
            );
            assert_eq!(
                std::fs::read_link(&link).unwrap(),
                Path::new("top.txt"),
                "symlink target must be preserved"
            );
        }
    }

    /// `copy_workspace` produces a faithful copy regardless of which path it
    /// took (clonefile/native `cp` on macOS, byte copy elsewhere or on `cp`
    /// failure). This exercises the macOS fast path on macOS and the portable
    /// fallback on other platforms.
    #[test]
    fn copy_workspace_preserves_files_and_symlinks() {
        let base =
            std::env::temp_dir().join(format!("ccresdoc-test-copyws-{}", std::process::id()));
        let src = base.join("src");
        let dst = base.join("dst");
        let _ = std::fs::remove_dir_all(&base);
        make_sample_tree(&src);

        copy_workspace(&src, &dst, "").expect("copy_workspace should succeed");
        assert_tree_copied(&dst);

        let _ = std::fs::remove_dir_all(&base);
    }

    /// The portable fallback copies an identical tree (file contents, nested
    /// dirs, symlink preserved as a symlink) — this is the path used on
    /// non-macOS and whenever the macOS `cp` fast paths fail.
    #[test]
    fn copy_dir_recursive_preserves_files_and_symlinks() {
        let base =
            std::env::temp_dir().join(format!("ccresdoc-test-copyrec-{}", std::process::id()));
        let src = base.join("src");
        let dst = base.join("dst");
        let _ = std::fs::remove_dir_all(&base);
        make_sample_tree(&src);

        copy_dir_recursive(&src, &dst).expect("copy_dir_recursive should succeed");
        assert_tree_copied(&dst);

        let _ = std::fs::remove_dir_all(&base);
    }

    /// `Option::take()` on the shared sidecar state yields the value exactly
    /// once; a second take is `None`. This is the take-once idempotency that
    /// makes `teardown` safe to call from whichever exit event fires first
    /// (Destroyed / ExitRequested / Exit) — the first wins, later calls no-op.
    #[test]
    fn shared_sidecar_take_is_once() {
        let slot: Arc<Mutex<Option<u32>>> = Arc::new(Mutex::new(Some(7)));
        let first = slot.lock().unwrap().take();
        let second = slot.lock().unwrap().take();
        assert_eq!(first, Some(7), "first take yields the value");
        assert_eq!(second, None, "second take is a no-op");
    }

    #[cfg(unix)]
    #[test]
    fn owned_process_group_teardown_reaps_the_group() {
        use std::os::unix::process::CommandExt;

        let mut command = Command::new("/bin/sh");
        command.args(["-c", "sleep 30 & wait"]).process_group(0);
        let child = command.spawn().expect("spawn owned process group");
        let pgid = i32::try_from(child.id()).unwrap();
        let mut sidecar = Sidecar {
            child,
            process_group_id: pgid,
        };
        kill_sidecar(&mut sidecar, "");
        // SAFETY: signal 0 only checks existence; the negative id addresses
        // precisely the process group created above.
        let rc = unsafe { libc::kill(-pgid, 0) };
        assert_eq!(rc, -1, "the app-owned process group must be gone");
        assert_eq!(
            std::io::Error::last_os_error().raw_os_error(),
            Some(libc::ESRCH)
        );
    }

    #[cfg(unix)]
    #[test]
    fn owned_process_group_teardown_escalates_and_waits_for_stubborn_members() {
        use std::os::unix::process::CommandExt;

        let mut command = Command::new("/bin/sh");
        command
            .args(["-c", "trap '' TERM HUP; while :; do sleep 1; done"])
            .process_group(0);
        let child = command.spawn().expect("spawn stubborn owned process group");
        let pgid = i32::try_from(child.id()).unwrap();
        let mut sidecar = Sidecar {
            child,
            process_group_id: pgid,
        };
        thread::sleep(Duration::from_millis(100));
        assert!(process_group_exists(pgid));
        kill_sidecar(&mut sidecar, "");
        assert!(
            !process_group_exists(pgid),
            "the exact app-owned process group must be absent before teardown returns"
        );
    }

    #[cfg(unix)]
    #[test]
    fn process_exit_hook_reaps_owned_group_without_a_run_event() {
        use std::os::unix::process::CommandExt;

        const HELPER_ENV: &str = "CCRESDOC_TEST_PROCESS_EXIT_HELPER";
        const PGID_FILE_ENV: &str = "CCRESDOC_TEST_PROCESS_EXIT_PGID_FILE";
        if std::env::var_os(HELPER_ENV).is_some() {
            register_sidecar_process_exit_hook().unwrap();
            let mut command = Command::new("/bin/sh");
            command.args(["-c", "sleep 30 & wait"]).process_group(0);
            let child = command.spawn().expect("spawn exit-hook sidecar group");
            let pgid = i32::try_from(child.id()).unwrap();
            claim_owned_process_group(pgid).unwrap();
            std::fs::write(std::env::var_os(PGID_FILE_ENV).unwrap(), pgid.to_string()).unwrap();
            std::process::exit(0);
        }

        let pgid_file =
            std::env::temp_dir().join(format!("ccresdoc-process-exit-pgid-{}", std::process::id()));
        let _ = std::fs::remove_file(&pgid_file);
        let output = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "tests::process_exit_hook_reaps_owned_group_without_a_run_event",
                "--nocapture",
            ])
            .env(HELPER_ENV, "1")
            .env(PGID_FILE_ENV, &pgid_file)
            .output()
            .expect("run process-exit hook helper");
        assert!(
            output.status.success(),
            "helper failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let pgid = std::fs::read_to_string(&pgid_file)
            .unwrap()
            .parse::<i32>()
            .unwrap();
        let _ = std::fs::remove_file(&pgid_file);
        assert!(
            !process_group_exists(pgid),
            "the process-exit hook must remove the exact owned group"
        );
    }

    #[test]
    fn loading_page_wires_launch_error_and_retry_launch() {
        let html_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("frontend")
            .join("index.html");
        let html = std::fs::read_to_string(&html_path).expect("Failed to read frontend/index.html");
        let adapter = std::fs::read_to_string(
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("frontend")
                .join("settings-backend.mjs"),
        )
        .expect("Failed to read settings backend adapter");
        assert!(
            html.contains("\"launch-error\""),
            "frontend/index.html should listen for the launch-error event"
        );
        assert!(
            adapter.contains("\"retry_launch\""),
            "the centralized adapter should invoke retry_launch"
        );
        assert!(
            html.contains("openSettings") && html.matches("Settings…").count() >= 2,
            "loading and error states should expose the Settings recovery action"
        );
        assert!(
            html.contains("settings-backend.mjs"),
            "bundled pages must use the centralized backend adapter"
        );
    }

    #[test]
    fn settings_menu_contract_is_native_and_stable() {
        assert_eq!(settings_window::SETTINGS_MENU_ID, "open_settings");
        assert_eq!(settings_window::SETTINGS_ACCELERATOR, "CmdOrCtrl+,");
    }

    #[test]
    fn every_custom_command_has_generated_acl_and_per_window_grants() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let build = std::fs::read_to_string(root.join("build.rs")).unwrap();
        let main: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(root.join("capabilities/default.json")).unwrap(),
        )
        .unwrap();
        let settings: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(root.join("capabilities/settings.json")).unwrap(),
        )
        .unwrap();
        let commands = [
            "retry_launch",
            "open_settings_window",
            "get_settings_snapshot",
            "update_appearance",
            "preview_appearance",
            "clear_appearance_preview",
            "validate_settings_draft",
            "save_and_apply_settings",
            "rebase_stale_settings",
            "replace_malformed_settings",
            "pick_source_directory",
            "open_config_file",
            "reveal_config_file",
        ];
        for command in commands {
            assert!(
                build.contains(&format!("\"{command}\"")),
                "missing {command}"
            );
            let permission = std::fs::read_to_string(
                root.join("permissions")
                    .join("autogenerated")
                    .join(format!("{command}.toml")),
            )
            .unwrap_or_else(|_| panic!("generated permission missing for {command}"));
            let dashed = command.replace('_', "-");
            assert!(permission.contains(&format!("allow-{dashed}")));
            assert!(permission.contains(&format!("deny-{dashed}")));
            assert!(permission.contains(&format!("commands.allow = [\"{command}\"]")));
        }
        assert!(build.contains("AppManifest::new().commands(COMMANDS)"));

        assert_eq!(main["windows"], serde_json::json!(["main"]));
        assert_eq!(settings["windows"], serde_json::json!(["settings"]));
        let main_permissions = main["permissions"].as_array().unwrap();
        let settings_permissions = settings["permissions"].as_array().unwrap();
        assert!(main_permissions.contains(&serde_json::json!("allow-open-settings-window")));
        assert!(main_permissions.contains(&serde_json::json!("allow-retry-launch")));
        assert!(main_permissions.contains(&serde_json::json!("allow-update-appearance")));
        for privileged in [
            "allow-get-settings-snapshot",
            "allow-preview-appearance",
            "allow-clear-appearance-preview",
            "allow-save-and-apply-settings",
            "allow-rebase-stale-settings",
            "allow-replace-malformed-settings",
            "allow-pick-source-directory",
            "allow-open-config-file",
            "allow-reveal-config-file",
        ] {
            assert!(!main_permissions.contains(&serde_json::json!(privileged)));
            assert!(settings_permissions.contains(&serde_json::json!(privileged)));
        }
        assert!(!settings_permissions.contains(&serde_json::json!("allow-update-appearance")));
        assert!(settings_permissions.contains(&serde_json::json!("core:event:allow-listen")));
        assert!(settings_permissions.contains(&serde_json::json!("core:event:allow-unlisten")));
        assert!(!settings_permissions.contains(&serde_json::json!("core:event:default")));
        assert!(!settings_permissions.contains(&serde_json::json!("core:event:allow-emit")));
        assert!(!settings.to_string().contains('*'));
        assert!(!settings.to_string().to_ascii_lowercase().contains("test"));
    }

    #[test]
    fn production_manifests_exclude_settings_test_identity_drivers_and_fixtures() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let mut files = vec![
            root.join("tauri.conf.json"),
            root.join("capabilities/default.json"),
            root.join("capabilities/settings.json"),
            root.join("build.rs"),
        ];
        files.extend(
            std::fs::read_dir(root.join("permissions/autogenerated"))
                .unwrap()
                .map(|entry| entry.unwrap().path()),
        );
        for file in files {
            let source = std::fs::read_to_string(&file).unwrap();
            for forbidden in [
                "test-bundle",
                "ccresdoc.settings-test",
                "CCRESDOC_TEST",
                "settings-test",
                "test-macos-settings",
                "test-driver",
                "native-driver",
                "computer-use",
                "fixture-claude",
                "/tmp/ccresdoc-settings-smoke",
            ] {
                assert!(
                    !source.contains(forbidden),
                    "test-only marker {forbidden:?} leaked into {}",
                    file.display()
                );
            }
        }
        let conf = read_tauri_conf();
        assert_eq!(conf["identifier"], "com.takazudo.ccresdoc");
        assert_eq!(conf["app"]["windows"], serde_json::json!([]));
    }

    #[test]
    fn csp_and_remote_capability_are_dynamic_loopback_only() {
        let conf = read_tauri_conf();
        let csp = conf["app"]["security"]["csp"]
            .as_str()
            .expect("CSP must be non-null");
        assert!(csp.contains("http://localhost:*"));
        assert!(csp.contains("http://127.0.0.1:*"));
        assert!(csp.contains("ws://localhost:*"));
        assert!(!csp.contains("0.0.0.0") && !csp.contains("*://"));

        let capability: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(
                PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("capabilities/default.json"),
            )
            .unwrap(),
        )
        .unwrap();
        for pattern in capability["remote"]["urls"].as_array().unwrap() {
            let pattern = pattern.as_str().unwrap();
            assert!(
                pattern.starts_with("http://localhost:")
                    || pattern.starts_with("http://127.0.0.1:")
            );
        }
    }

    #[test]
    fn bundled_settings_shell_has_loading_and_fatal_states() {
        let frontend = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("frontend");
        let html = std::fs::read_to_string(frontend.join("settings.html")).unwrap();
        let css = std::fs::read_to_string(frontend.join("settings.css")).unwrap();
        assert!(html.contains("settings-loading") && html.contains("settings-fatal"));
        assert!(html.contains("role=\"alert\"") && html.contains("aria-live=\"polite\""));
        assert!(css.contains("button:focus-visible"));
        assert!(css.contains("@media (hover: hover)"));

        let index = std::fs::read_to_string(frontend.join("index.html")).unwrap();
        let shell = std::fs::read_to_string(frontend.join("settings-shell.mjs")).unwrap();
        let adapter = std::fs::read_to_string(frontend.join("settings-backend.mjs")).unwrap();
        assert!(!index.contains("core.invoke") && !shell.contains("core.invoke"));
        assert_eq!(adapter.matches("invoke(command, args)").count(), 1);
    }
}
