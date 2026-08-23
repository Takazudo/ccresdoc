#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

//! CCResDoc — thin sidecar host (Wave 3 / #44).
//!
//! Runtime is **node-free**: the host resolves a writable app-project, the
//! native `zfb` binary (NOT the Node-shebang `node_modules/.bin/zfb` wrapper),
//! and the absolute `~/.claude` path, then:
//!
//!   1. boots the Wave 2 generator (`ccresdoc_claude_md::generate`) once,
//!   2. starts the Wave 2 watcher (`::watch`) in-process so edits under
//!      `~/.claude` regenerate the MDX tree and `zfb dev`'s content-watch HMRs,
//!   3. selects a settings-driven loopback port and spawns `zfb dev` as a
//!      process-group sidecar,
//!   4. polls semantic readiness on `/docs/` (scaled for the cold first build
//!      of ~135 skills) and navigates the WebView there only after generated
//!      resource navigation is present.
//!
//! On window close the sidecar process group is SIGTERM→SIGKILL'd so nothing
//! is left holding its effective port.

pub mod runtime;
pub mod settings;
pub mod settings_commands;
pub mod settings_window;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU16, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use std::{env, thread};

use ccresdoc_claude_md::{Config as GenConfig, WatchEvent, WatchHandle, DEFAULT_DEBOUNCE};
use tauri::menu::{MenuBuilder, MenuItemBuilder, SubmenuBuilder};
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder};

use runtime::{
    NavigationDecision, PortBoundary, PortChoice, ReadyResult, RuntimeDiagnostic,
    RuntimeDiagnosticKind, RuntimePhase, SystemPortBoundary,
};
use settings::{EffectiveSettings, SettingsStore};
use settings_window::{
    lifecycle_action, open_or_focus_settings, LifecycleAction, SETTINGS_ACCELERATOR,
    SETTINGS_MENU_ID, SETTINGS_WINDOW_LABEL,
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
}

struct AppState {
    sidecar: Arc<Mutex<Option<Sidecar>>>,
    /// Kept alive for the process lifetime; dropping it stops the watcher.
    watch_handle: Mutex<Option<WatchHandle>>,
    zoom: Mutex<f64>,
    /// Filled in during setup() (app_data_dir/ccresdoc.log).
    log_path: Mutex<String>,
    runtime: Arc<runtime::ApplyCoordinator>,
    settings_store: SettingsStore,
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
        let port = app_handle
            .state::<AppState>()
            .effective_port
            .load(Ordering::SeqCst);
        if let Ok(url) = runtime::docs_url(port).parse::<tauri::Url>() {
            let _ = w.navigate(url);
        }
    }
}

/// Restore the bundled loading surface before a Refresh begins. Retry already
/// runs from this page's error panel, so both paths converge on the same launch
/// lease and semantic-readiness classifier.
fn navigate_to_loading(app_handle: &AppHandle) {
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
/// Finder and terminal launches may inherit it, but boot-lazy is allowed to
/// serve staged `dist/` before the freshly generated resource tree is ready.
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

    let child = cmd.spawn().map_err(|e| {
        log_to(log_path, &format!("spawn_zfb_dev: spawn failed: {e}"));
        format!("failed to spawn zfb dev in {}: {e}", workspace.display())
    })?;
    log_to(log_path, &format!("spawn_zfb_dev: pid={}", child.id()));
    Ok(Sidecar { child })
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
/// It is idempotent: both the sidecar (`Option::take()` on the shared
/// `Mutex<Option<Sidecar>>`) and the watcher (`Option::take()` on the
/// `WatchHandle`) are taken out of shared state, so whichever exit event fires
/// first does the work and any later call is a no-op.
fn teardown(
    app_handle: &AppHandle,
    sidecar: &Arc<Mutex<Option<Sidecar>>>,
    log_path: &str,
    shutting_down: bool,
) {
    let state = app_handle.state::<AppState>();
    if shutting_down {
        state.shutting_down.store(true, Ordering::SeqCst);
    }
    let stopped_generation = state.runtime.claim_generation();
    state.runtime.publish_stopped(stopped_generation);
    state.effective_port.store(0, Ordering::SeqCst);
    let _ = app_handle
        .state::<AppState>()
        .watch_handle
        .lock()
        .unwrap()
        .take();
    if let Ok(mut g) = sidecar.lock() {
        if let Some(mut s) = g.take() {
            kill_sidecar(&mut s, log_path);
        }
    }
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

fn kill_sidecar(sidecar: &mut Sidecar, log_path: &str) {
    let pid = sidecar.child.id();
    log_to(log_path, &format!("kill_sidecar: pid={pid}"));

    // Only signal the process GROUP while the group leader (our direct child)
    // is still alive: once it has exited, the PID/PGID can be recycled, and
    // signalling `-pid` could then hit an unrelated group. If the child is
    // already gone there is nothing to SIGTERM — fall through to reap it.
    #[cfg(unix)]
    {
        let already_exited = matches!(sidecar.child.try_wait(), Ok(Some(_)));
        if !already_exited {
            if let Ok(pid) = i32::try_from(pid) {
                // Negative PID → signal the whole process group.
                signal_checked(-pid, libc::SIGTERM, log_path, "kill_sidecar");
            }
        }
    }

    // Bounded poll instead of a flat 500ms sleep so we return as soon as the
    // child is reaped (this can run on the main event loop during exit).
    let reaped = wait_reaped(
        &mut sidecar.child,
        Duration::from_millis(1000),
        Duration::from_millis(50),
    );
    if reaped {
        log_to(log_path, "kill_sidecar: exited after SIGTERM");
    } else {
        log_to(log_path, "kill_sidecar: escalating to SIGKILL");
    }

    // The direct child can exit before one of its descendants. While any
    // descendant remains, the app-created PGID remains allocated and cannot
    // be reassigned, so a signal-0 check followed by SIGKILL targets only that
    // exact owned group. This replaces the unsafe former port-owner sweep.
    #[cfg(unix)]
    if let Ok(pid) = i32::try_from(pid) {
        // SAFETY: signal 0 performs an existence check only.
        if unsafe { libc::kill(-pid, 0) } == 0 {
            signal_checked(-pid, libc::SIGKILL, log_path, "kill_sidecar");
        }
    }
    if !reaped {
        #[cfg(not(unix))]
        {
            let _ = sidecar.child.kill();
        }
        let _ = sidecar.child.wait();
    }
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

/// The full node-free boot, runnable from initial setup, Refresh, and Retry.
/// Resolves workspace + zfb binary + `~/.claude`, runs `generate()`
/// once, starts `watch()`, spawns `zfb dev`, polls readiness, then navigates.
///
/// The runtime coordinator's generation guards against stale terminal work,
/// while its serialized apply lease prevents interleaved replacement.
fn launch(app_handle: &AppHandle, my_gen: u64, desired: EffectiveSettings, allow_recovery: bool) {
    let log_path = log_path(app_handle);
    let sidecar_arc = app_handle.state::<AppState>().sidecar.clone();
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
            emit_launch_error_if_current(app_handle, my_gen, "workspace_unavailable");
            return;
        }
    };

    // 2. Resolve the native zfb binary (missing node_modules → error UI).
    let zfb_bin = match resolve_zfb_binary(&workspace) {
        Ok(b) => b,
        Err(e) => {
            log_to(&log_path, &format!("launch: zfb binary unresolved: {e}"));
            emit_launch_error_if_current(app_handle, my_gen, "zfb_binary_missing");
            return;
        }
    };

    // 3. Use the normalized source from the typed settings snapshot.
    let claude = desired.claude_dir.clone();
    if !claude.exists() {
        log_to(
            &log_path,
            &format!("launch: ~/.claude missing at {}", claude.display()),
        );
        emit_launch_error_if_current(app_handle, my_gen, "claude_dir_missing");
        return;
    }

    // Select before disturbing the previous working runtime whenever the
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

    // 4. Boot the Wave 2 generator once. The old watcher/server remain alive
    // until this succeeds, preserving the previous working runtime.
    //    docs_dir is the workspace's zudo-doc content root.
    let gen_config = GenConfig {
        claude_dir: claude.clone(),
        project_root: claude.clone(),
        docs_dir: workspace.join("src").join("content").join("docs"),
    };

    match ccresdoc_claude_md::generate(&gen_config) {
        Ok(report) => log_to(
            &log_path,
            &format!(
                "launch: generate ok — claude_md={} commands={} skills={} agents={}",
                report.claude_md, report.commands, report.skills, report.agents
            ),
        ),
        Err(e) => {
            log_to(&log_path, &format!("launch: generate failed: {e}"));
            state.runtime.publish_failed(
                RuntimeDiagnostic {
                    kind: RuntimeDiagnosticKind::GenerateFailed,
                    preferred_port: desired.preferred_port,
                    attempted_port: None,
                    message: e.to_string(),
                },
                my_gen,
            );
            emit_launch_error_if_current(app_handle, my_gen, "generate_failed");
            return;
        }
    }

    if !launch_is_current(app_handle, my_gen) {
        log_to(
            &log_path,
            &format!("launch[{my_gen}]: superseded after generation"),
        );
        return;
    }

    // The replacement transition begins here. Drop only app-owned resources;
    // no port-owner discovery or signalling is permitted.
    let _ = state.watch_handle.lock().unwrap().take();
    let mut old_sidecar = sidecar_arc.lock().unwrap().take();
    if let Some(ref mut old) = old_sidecar {
        kill_sidecar(old, &log_path);
    }
    state.effective_port.store(0, Ordering::SeqCst);

    // Start the watcher; keep its handle in AppState so it lives for the
    // process lifetime.
    {
        let watch_log = log_path.clone();
        match ccresdoc_claude_md::watch(gen_config, DEFAULT_DEBOUNCE, move |event| match event {
            WatchEvent::Regenerated(report) => log_to(
                &watch_log,
                &format!(
                    "watch: regenerated — claude_md={} commands={} skills={} agents={}",
                    report.claude_md, report.commands, report.skills, report.agents
                ),
            ),
            WatchEvent::Error(e) => log_to(&watch_log, &format!("watch: regeneration error: {e}")),
        }) {
            Ok(handle) => {
                let state = app_handle.state::<AppState>();
                let mut slot = state.watch_handle.lock().unwrap();
                if state.shutting_down.load(Ordering::SeqCst) {
                    drop(slot);
                    drop(handle);
                    log_to(
                        &log_path,
                        "launch: dropping watcher created during shutdown",
                    );
                    return;
                }
                *slot = Some(handle);
                log_to(&log_path, "launch: watcher started");
            }
            Err(e) => {
                // Non-fatal: one-shot content is already on disk, so the site
                // still serves; only live updates are lost.
                log_to(
                    &log_path,
                    &format!("launch: watch failed (continuing without live updates): {e}"),
                );
            }
        }
    }

    if !launch_is_current(app_handle, my_gen) {
        log_to(
            &log_path,
            &format!("launch[{my_gen}]: superseded after watcher setup"),
        );
        return;
    }

    // 5/6. Spawn and probe. If the released preflight socket was stolen,
    // retry with a fresh OS-assigned loopback candidate, boundedly.
    let mut result = ReadyResult::Timeout;
    let mut bind_retry_exhausted = false;
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
            Ok(mut sidecar) => {
                let mut slot = sidecar_arc.lock().unwrap();
                if state.shutting_down.load(Ordering::SeqCst) {
                    drop(slot);
                    kill_sidecar(&mut sidecar, &log_path);
                    return;
                }
                *slot = Some(sidecar);
            }
            Err(error) => {
                log_to(&log_path, &format!("launch: spawn failed: {error}"));
                result = ReadyResult::SidecarExited { code: None };
                break;
            }
        }
        result = runtime::wait_for_ready(
            choice.effective_port,
            READY_TIMEOUT,
            (state.runtime.generation(), my_gen),
            || {
                let mut guard = sidecar_arc.lock().unwrap();
                guard
                    .as_mut()
                    .and_then(|sidecar| match sidecar.child.try_wait() {
                        Ok(Some(status)) => Some(status.code()),
                        _ => None,
                    })
            },
            |port| match runtime::probe_docs(port, Duration::from_secs(1)) {
                Ok((status, body)) => runtime::classify_readiness(status, &body),
                Err(_) => runtime::ReadinessState::HttpUnavailable,
            },
        );
        if result != (ReadyResult::SidecarExited { code: None })
            && !matches!(result, ReadyResult::SidecarExited { .. })
        {
            break;
        }
        let _ = sidecar_arc.lock().unwrap().take();
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
        return;
    }

    match result {
        ReadyResult::Ready => {
            state
                .effective_port
                .store(choice.effective_port, Ordering::SeqCst);
            state.runtime.publish_ready(desired, choice, my_gen);
            navigate_to_docs(app_handle)
        }
        ReadyResult::Timeout | ReadyResult::SidecarExited { .. } => {
            let kind = if bind_retry_exhausted {
                RuntimeDiagnosticKind::BindRetryExhausted
            } else if matches!(result, ReadyResult::Timeout) {
                RuntimeDiagnosticKind::Timeout
            } else {
                RuntimeDiagnosticKind::SidecarExited
            };
            let diagnostic = RuntimeDiagnostic {
                kind,
                preferred_port: desired.preferred_port,
                attempted_port: Some(choice.effective_port),
                message: format!("{result:?}"),
            };
            // A failure after replacement began has stopped the previous
            // process. Restore that exact effective runtime when possible;
            // then re-publish the new authored settings as saved-not-active.
            if let Some(previous) = previous.filter(|_| allow_recovery) {
                log_to(&log_path, "launch: attempting previous-runtime recovery");
                launch(app_handle, my_gen, previous, false);
                if state.runtime.snapshot().phase != RuntimePhase::Ready {
                    state.runtime.clear_active(my_gen);
                }
            } else if !allow_recovery {
                state.runtime.clear_active(my_gen);
            }
            state.runtime.publish_failed(diagnostic, my_gen);
            emit_launch_error(app_handle, &result);
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
        coordinator.with_serialized_apply(|| launch(&handle, generation, desired, true))
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
) -> Result<(), tauri::Error> {
    WebviewWindowBuilder::new(app, "main", WebviewUrl::App("index.html".into()))
        .title("CCResDoc")
        .inner_size(1200.0, 800.0)
        .on_navigation(move |url| allow_navigation(url, navigation_port.load(Ordering::SeqCst)))
        .on_page_load(|window, payload| {
            if let tauri::webview::PageLoadEvent::Finished = payload.event() {
                reapply_zoom(window.app_handle());
            }
        })
        .build()?;
    Ok(())
}

// ── Main ──────────────────────────────────────────

fn main() {
    let home = match home_dir() {
        Some(home) => PathBuf::from(home),
        None => PathBuf::from("/"),
    };
    let config_path = settings::resolve_config_path()
        .unwrap_or_else(|_| home.join(".config/ccresdoc/config.toml"));
    let settings_store = SettingsStore::new(config_path, home);
    let settings_snapshot = settings_store.load();
    let runtime = Arc::new(runtime::ApplyCoordinator::new(settings_snapshot));
    let effective_port = Arc::new(AtomicU16::new(0));
    let app_state = AppState {
        sidecar: Arc::new(Mutex::new(None)),
        watch_handle: Mutex::new(None),
        zoom: Mutex::new(1.0),
        log_path: Mutex::new(String::new()),
        runtime,
        settings_store,
        effective_port: effective_port.clone(),
        shutting_down: AtomicBool::new(false),
    };
    let sidecar_for_exit = app_state.sidecar.clone();
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
            let app_menu = SubmenuBuilder::new(app, "CCResDoc")
                .about(None)
                .separator()
                .item(
                    &MenuItemBuilder::with_id(SETTINGS_MENU_ID, "Settings…")
                        .accelerator(SETTINGS_ACCELERATOR)
                        .build(app)?,
                )
                .separator()
                .quit()
                .build()?;

            let edit_menu = SubmenuBuilder::new(app, "Edit")
                .undo()
                .redo()
                .separator()
                .cut()
                .copy()
                .paste()
                .select_all()
                .build()?;

            let view_menu = SubmenuBuilder::new(app, "View")
                .item(
                    &MenuItemBuilder::with_id("refresh", "Refresh")
                        .accelerator("CmdOrCtrl+R")
                        .build(app)?,
                )
                .item(
                    &MenuItemBuilder::with_id("devtools", "Toggle Developer Tools")
                        .accelerator("CmdOrCtrl+Alt+I")
                        .build(app)?,
                )
                .separator()
                .item(
                    &MenuItemBuilder::with_id("actual_size", "Actual Size")
                        .accelerator("CmdOrCtrl+0")
                        .build(app)?,
                )
                .item(
                    &MenuItemBuilder::with_id("zoom_in", "Zoom In")
                        .accelerator("CmdOrCtrl+=")
                        .build(app)?,
                )
                .item(
                    &MenuItemBuilder::with_id("zoom_out", "Zoom Out")
                        .accelerator("CmdOrCtrl+-")
                        .build(app)?,
                )
                .build()?;

            let menu = MenuBuilder::new(app)
                .item(&app_menu)
                .item(&edit_menu)
                .item(&view_menu)
                .build()?;

            app.set_menu(menu)?;

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
            create_main_window(app.handle(), navigation_port.clone())?;

            start_launch(app.handle());

            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(move |app_handle, event| {
            // Tear down on EVERY exit path. Window close fires
            // `WindowEvent::Destroyed`; an app-level Quit (Cmd+Q, Dock → Quit,
            // `osascript` quit) fires `ExitRequested` (before exit) then `Exit`
            // (last) but NOT necessarily `Destroyed` — handling all three (and
            // relying on `teardown`'s take-once idempotency) guarantees the
            // sidecar process group is killed exactly once regardless of which
            // event the platform delivers, so nothing is orphaned on 4892.
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
                        if let Some(window) = app_handle.get_webview_window(SETTINGS_WINDOW_LABEL) {
                            let _ = window.hide();
                        }
                    }
                }
                LifecycleAction::StopForMainClose => {
                    let log_path = log_path(app_handle);
                    teardown(app_handle, &sidecar_for_exit, &log_path, false);
                }
                LifecycleAction::Shutdown => {
                    let log_path = log_path(app_handle);
                    teardown(app_handle, &sidecar_for_exit, &log_path, true);
                }
                LifecycleAction::ReopenMain => {
                    #[cfg(target_os = "macos")]
                    if let Some(main) = app_handle.get_webview_window("main") {
                        let _ = main.show();
                        if main.is_minimized().unwrap_or(false) {
                            let _ = main.unminimize();
                        }
                        let _ = main.set_focus();
                    } else if create_main_window(app_handle, reopen_navigation_port.clone()).is_ok()
                    {
                        if app_handle.state::<AppState>().runtime.snapshot().phase
                            == RuntimePhase::Ready
                        {
                            navigate_to_docs(app_handle);
                        } else {
                            start_launch(app_handle);
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
        let mut sidecar = Sidecar { child };
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
        for privileged in [
            "allow-get-settings-snapshot",
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
        assert!(!settings.to_string().contains('*'));
        assert!(!settings.to_string().to_ascii_lowercase().contains("test"));
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
