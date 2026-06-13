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
//!   3. spawns `zfb dev --port 4892` (cwd = the writable app project) as a
//!      process-group sidecar,
//!   4. polls readiness on `/` (scaled for the cold first build of ~135
//!      skills) and navigates the WebView to `http://localhost:4892/`.
//!
//! On window close the sidecar process group is SIGTERM→SIGKILL'd so nothing
//! is left holding port 4892.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use std::{env, thread};

use ccresdoc_claude_md::{Config as GenConfig, WatchEvent, WatchHandle, DEFAULT_DEBOUNCE};
use tauri::menu::{MenuBuilder, MenuItemBuilder, SubmenuBuilder};
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder};

const PORT: u16 = 4892;
const DOCS_PATH: &str = "/";
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
fn zfb_platform_package() -> Option<&'static str> {
    // Tauri ships macOS arm64/x64 here; the full map matches the npm wrapper.
    match (env::consts::OS, env::consts::ARCH) {
        ("macos", "aarch64") => Some("@takazudo/zfb-darwin-arm64"),
        ("macos", "x86_64") => Some("@takazudo/zfb-darwin-x64"),
        ("linux", "aarch64") => Some("@takazudo/zfb-linux-arm64-gnu"),
        ("linux", "x86_64") => Some("@takazudo/zfb-linux-x64-gnu"),
        ("windows", "x86_64") => Some("@takazudo/zfb-win32-x64-msvc"),
        _ => None,
    }
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
    /// Bumped at the start of every launch attempt (initial setup + each
    /// retry). A launch thread that finishes after a newer one began sees a
    /// mismatch and skips its navigate/emit so the two cannot race.
    launch_gen: AtomicU64,
}

// ── Helpers ───────────────────────────────────────

fn home_dir() -> String {
    env::var("HOME").expect("HOME not set")
}

/// Absolute `~/.claude`. Passed to the Wave 2 generator as both `claude_dir`
/// and `project_root` — NEVER `$HOME` (the walk must stay scoped to
/// `~/.claude`; the generator rejects `project_root == $HOME`).
fn claude_dir() -> PathBuf {
    PathBuf::from(home_dir()).join(".claude")
}

fn docs_url() -> String {
    format!("http://localhost:{PORT}{DOCS_PATH}")
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
/// the constant `docs_url()`, so they are silently ignored. Shared by the
/// launch-success path, the dev retry path, and the Refresh menu item.
fn navigate_to_docs(app_handle: &AppHandle) {
    if let Some(w) = app_handle.get_webview_window("main") {
        if let Ok(url) = docs_url().parse::<tauri::Url>() {
            let _ = w.navigate(url);
        }
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
/// compile time. An optional `version.txt` beside the bundled `app/` overrides
/// it if present (so a build could emit a per-build token), but the build does
/// NOT currently emit one — so the compiled-in version is what's used. Bump the
/// crate version per release and the workspace refreshes on upgrade.
fn bundled_version_token(resources_app_parent: &Path) -> String {
    let version_file = resources_app_parent.join("version.txt");
    if let Ok(v) = fs::read_to_string(&version_file) {
        let v = v.trim();
        if !v.is_empty() {
            return v.to_string();
        }
    }
    env!("CARGO_PKG_VERSION").to_string()
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

/// Resolve the bundled (read-only) `app/` directory inside `.app` Resources.
///
/// Tauri bundles `../app/**` (a `..` traversal relative to `src-tauri/`) under
/// `Contents/Resources/_up_/app/`. We return the `_up_` parent so callers can
/// also read the sibling `version.txt`.
fn bundled_resources_app_parent(app: &AppHandle) -> tauri::Result<PathBuf> {
    Ok(app.path().resource_dir()?.join("_up_"))
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
    fs::write(&ready_sentinel, &bundled_token).map_err(|e| format!("write ready sentinel: {e}"))?;

    log_to(
        log_path,
        &format!("resolve_workspace: workspace ready (token={bundled_token})"),
    );
    Ok(WorkspaceResolution::AppDataCopy(workspace))
}

/// Copy the bundled `src` tree into `dst`, preserving permissions and symlinks.
///
/// The workspace is large (~636MB; ~413MB is `node_modules` of many small
/// files). A byte-for-byte [`copy_dir_recursive`] of it measured ~41s on cold
/// first launch, which alone blows the 60s acceptance budget. So on macOS we
/// prefer **APFS clonefile** (copy-on-write — near-instant, no data is moved):
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
    match Command::new("/bin/cp").args(args).output() {
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
    Ok(bin)
}

// ── Sidecar (zfb dev) management ──────────────────

/// Spawn `zfb dev --port 4892` with cwd = the writable workspace, in its own
/// process group so the whole tree dies on window close (no orphan on 4892).
fn spawn_zfb_dev(zfb_bin: &Path, workspace: &Path, log_path: &str) -> Result<Sidecar, String> {
    log_to(
        log_path,
        &format!(
            "spawn_zfb_dev: bin={} cwd={}",
            zfb_bin.display(),
            workspace.display()
        ),
    );

    let mut cmd = Command::new(zfb_bin);
    cmd.args(["dev", "--port", &PORT.to_string()])
        .current_dir(workspace)
        .stdout(Stdio::null())
        .stderr(Stdio::null());

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
/// watcher) and SIGTERM→SIGKILL the `zfb dev` process group so nothing is left
/// holding port 4892.
///
/// This MUST run on every app-exit path, not just window close. An app-level
/// Quit (Cmd+Q, Dock → Quit, `osascript 'tell application … to quit'`) can
/// terminate the app WITHOUT reliably emitting `WindowEvent::Destroyed` first,
/// which previously left `zfb dev` orphaned on 4892. So the run-event handler
/// calls this from `WindowEvent::Destroyed` AND `ExitRequested` AND `Exit`.
///
/// It is idempotent: both the sidecar (`Option::take()` on the shared
/// `Mutex<Option<Sidecar>>`) and the watcher (`Option::take()` on the
/// `WatchHandle`) are taken out of shared state, so whichever exit event fires
/// first does the work and any later call is a no-op.
fn teardown(app_handle: &AppHandle, sidecar: &Arc<Mutex<Option<Sidecar>>>, log_path: &str) {
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

fn kill_sidecar(sidecar: &mut Sidecar, log_path: &str) {
    let pid = sidecar.child.id();
    log_to(log_path, &format!("kill_sidecar: pid={pid}"));
    #[cfg(unix)]
    {
        if let Ok(pid) = i32::try_from(pid) {
            // Negative PID → signal the whole process group.
            unsafe { libc::kill(-pid, libc::SIGTERM) };
        }
    }
    thread::sleep(Duration::from_millis(500));
    match sidecar.child.try_wait() {
        Ok(Some(_)) => log_to(log_path, "kill_sidecar: already exited"),
        _ => {
            log_to(log_path, "kill_sidecar: escalating to SIGKILL");
            let _ = sidecar.child.kill();
            let _ = sidecar.child.wait();
        }
    }
}

// ── Port cleanup ─────────────────────────────────

/// List PIDs currently holding :PORT (via `lsof -ti`).
fn pids_on_port() -> Vec<i32> {
    Command::new("/usr/bin/lsof")
        .args(["-ti", &format!(":{PORT}")])
        .output()
        .map(|o| {
            String::from_utf8_lossy(&o.stdout)
                .lines()
                .filter_map(|l| l.trim().parse::<i32>().ok())
                .collect()
        })
        .unwrap_or_default()
}

/// Free :PORT before spawning a fresh sidecar. SIGTERM first; if a holder is
/// slow/deaf to it, escalate to SIGKILL so a stuck process can't make every
/// subsequent spawn (and Retry) fail to bind. Mirrors `kill_sidecar`'s
/// terminate-then-kill escalation.
fn kill_port(log_path: &str) {
    let pids = pids_on_port();
    if pids.is_empty() {
        return;
    }
    for pid in &pids {
        log_to(
            log_path,
            &format!("kill_port: SIGTERM stale pid {pid} on :{PORT}"),
        );
        #[cfg(unix)]
        unsafe {
            libc::kill(*pid, libc::SIGTERM);
        }
    }
    thread::sleep(Duration::from_millis(500));

    let stragglers = pids_on_port();
    for pid in &stragglers {
        log_to(
            log_path,
            &format!("kill_port: SIGKILL straggler pid {pid} on :{PORT}"),
        );
        #[cfg(unix)]
        unsafe {
            libc::kill(*pid, libc::SIGKILL);
        }
    }
    if !stragglers.is_empty() {
        thread::sleep(Duration::from_millis(300));
    }
}

// ── Readiness polling ────────────────────────────

#[derive(Debug)]
enum ReadyResult {
    Ready,
    Timeout,
    /// The sidecar exited before becoming ready — short-circuit the wait.
    SidecarExited {
        code: Option<i32>,
    },
}

fn curl_root() -> String {
    Command::new("/usr/bin/curl")
        .args(["-s", "-o", "/dev/null", "-w", "%{http_code}", &docs_url()])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_else(|_| "err".to_string())
}

/// Poll `GET /` until 200, up to `timeout`. Each tick first checks sidecar
/// liveness via `try_wait` so a crashed `zfb dev` surfaces an error within ~1s
/// rather than burning the whole timeout on the spinner.
fn wait_for_ready(
    timeout: Duration,
    sidecar: &Arc<Mutex<Option<Sidecar>>>,
    log_path: &str,
) -> ReadyResult {
    log_to(log_path, "wait_for_ready: start");
    let start = Instant::now();
    while start.elapsed() < timeout {
        {
            let mut guard = sidecar.lock().unwrap();
            if let Some(ref mut s) = *guard {
                match s.child.try_wait() {
                    Ok(Some(status)) => {
                        let code = status.code();
                        log_to(
                            log_path,
                            &format!("wait_for_ready: sidecar exited early (code={code:?})"),
                        );
                        return ReadyResult::SidecarExited { code };
                    }
                    Ok(None) => {}
                    Err(e) => log_to(log_path, &format!("wait_for_ready: try_wait error: {e}")),
                }
            }
        }

        let code = curl_root();
        log_to(
            log_path,
            &format!("curl /: {code} ({}s)", start.elapsed().as_secs()),
        );
        if code == "200" {
            log_to(log_path, "wait_for_ready: ready");
            return ReadyResult::Ready;
        }
        thread::sleep(Duration::from_secs(1));
    }
    log_to(log_path, "wait_for_ready: TIMEOUT");
    ReadyResult::Timeout
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
        ReadyResult::Ready => return,
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

/// The full node-free boot, runnable from both initial setup and the retry
/// path. Resolves workspace + zfb binary + `~/.claude`, runs `generate()`
/// once, starts `watch()`, spawns `zfb dev`, polls readiness, then navigates.
///
/// `launch_gen` guards against a restart-race: a retry pressed mid-wait bumps
/// the generation so the older launch thread skips its terminal navigate/emit.
fn launch(app_handle: &AppHandle) {
    let log_path = log_path(app_handle);
    let sidecar_arc = app_handle.state::<AppState>().sidecar.clone();

    // Claim a new generation; any in-flight launch is now stale.
    let my_gen = app_handle
        .state::<AppState>()
        .launch_gen
        .fetch_add(1, Ordering::SeqCst)
        + 1;

    log_to(&log_path, "launch: start");

    // 1. Resolve a writable workspace.
    let workspace = match resolve_workspace(app_handle, &log_path) {
        Ok(w) => w.path().to_path_buf(),
        Err(e) => {
            log_to(
                &log_path,
                &format!("launch: workspace resolution failed: {e}"),
            );
            emit_launch_error_str(app_handle, "workspace_unavailable");
            return;
        }
    };

    // 2. Resolve the native zfb binary (missing node_modules → error UI).
    let zfb_bin = match resolve_zfb_binary(&workspace) {
        Ok(b) => b,
        Err(e) => {
            log_to(&log_path, &format!("launch: zfb binary unresolved: {e}"));
            emit_launch_error_str(app_handle, "zfb_binary_missing");
            return;
        }
    };

    // 3. Resolve absolute ~/.claude (missing → error UI).
    let claude = claude_dir();
    if !claude.exists() {
        log_to(
            &log_path,
            &format!("launch: ~/.claude missing at {}", claude.display()),
        );
        emit_launch_error_str(app_handle, "claude_dir_missing");
        return;
    }

    // 4. Boot the Wave 2 generator once, then start the watcher in-process.
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
            emit_launch_error_str(app_handle, "generate_failed");
            return;
        }
    }

    // Start the watcher; keep its handle in AppState so it lives for the
    // process lifetime (dropping it stops the watch). On retry, drop the old
    // watcher FIRST (before constructing the new one) so two watchers never
    // run concurrently on ~/.claude.
    {
        let _ = app_handle
            .state::<AppState>()
            .watch_handle
            .lock()
            .unwrap()
            .take();
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
                *state.watch_handle.lock().unwrap() = Some(handle);
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

    // 5. Clear any stale port holder, then (re)spawn zfb dev.
    {
        let mut guard = sidecar_arc.lock().unwrap();
        if let Some(mut old) = guard.take() {
            kill_sidecar(&mut old, &log_path);
        }
    }
    kill_port(&log_path);
    {
        let mut guard = sidecar_arc.lock().unwrap();
        match spawn_zfb_dev(&zfb_bin, &workspace, &log_path) {
            Ok(s) => *guard = Some(s),
            Err(e) => {
                drop(guard);
                log_to(&log_path, &format!("launch: spawn failed: {e}"));
                emit_launch_error_str(app_handle, "spawn_failed");
                return;
            }
        }
    }

    // 6. Poll readiness on / (scaled for the cold first build).
    let result = wait_for_ready(READY_TIMEOUT, &sidecar_arc, &log_path);

    // 7. Skip navigate/emit if a newer launch superseded this one.
    if app_handle
        .state::<AppState>()
        .launch_gen
        .load(Ordering::SeqCst)
        != my_gen
    {
        log_to(
            &log_path,
            "launch: superseded by a newer launch — skipping navigate/emit",
        );
        return;
    }

    match result {
        ReadyResult::Ready => navigate_to_docs(app_handle),
        ReadyResult::Timeout | ReadyResult::SidecarExited { .. } => {
            emit_launch_error(app_handle, &result);
        }
    }
}

fn apply_zoom(app_handle: &AppHandle, level: f64) {
    let state = app_handle.state::<AppState>();
    *state.zoom.lock().unwrap() = level;
    if let Some(w) = app_handle.get_webview_window("main") {
        let _ = w.eval(format!("document.body.style.zoom = '{level}'"));
    }
}

/// Frontend-callable retry for the loading page's error panel. Spawned on a
/// thread so the IPC call returns immediately. Re-runs the full `launch()`
/// (which claims a new generation, tears down the old sidecar/watcher, and
/// re-spawns) in both dev and prod — the host owns `zfb dev` in both modes.
#[tauri::command]
fn retry_launch(app_handle: AppHandle) {
    log_to(
        &log_path(&app_handle),
        "retry_launch: invoked from frontend",
    );
    thread::spawn(move || launch(&app_handle));
}

// ── Navigation filter ─────────────────────────────

/// Allow in-window navigation only for localhost (the doc site), tauri/asset
/// protocol URLs, and about:blank; open external http(s) links in the OS
/// browser instead of inside the WebView.
fn allow_navigation(url: &tauri::Url) -> bool {
    match url.scheme() {
        "tauri" | "asset" | "about" => true,
        "http" | "https" => matches!(url.host_str(), Some("localhost") | Some("127.0.0.1")),
        _ => false,
    }
}

// ── Main ──────────────────────────────────────────

fn main() {
    let app_state = AppState {
        sidecar: Arc::new(Mutex::new(None)),
        watch_handle: Mutex::new(None),
        zoom: Mutex::new(1.0),
        log_path: Mutex::new(String::new()),
        launch_gen: AtomicU64::new(0),
    };
    let sidecar_for_exit = app_state.sidecar.clone();

    tauri::Builder::default()
        .manage(app_state)
        .invoke_handler(tauri::generate_handler![retry_launch])
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
                "refresh" => navigate_to_docs(app_handle),
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
            WebviewWindowBuilder::new(app, "main", WebviewUrl::App("index.html".into()))
                .title("CCResDoc")
                .inner_size(1200.0, 800.0)
                .on_navigation(allow_navigation)
                .build()?;

            let handle = app.handle().clone();
            thread::spawn(move || launch(&handle));

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
            let is_window_destroyed = matches!(
                &event,
                tauri::RunEvent::WindowEvent {
                    event: tauri::WindowEvent::Destroyed,
                    ..
                }
            );
            let is_app_exit = matches!(
                &event,
                tauri::RunEvent::ExitRequested { .. } | tauri::RunEvent::Exit
            );
            if is_window_destroyed || is_app_exit {
                let log_path = log_path(app_handle);
                teardown(app_handle, &sidecar_for_exit, &log_path);
                // Closing the last window keeps the event loop alive on macOS,
                // so the window-close path explicitly exits; the app-quit paths
                // (ExitRequested/Exit) are already exiting and must not re-enter.
                if is_window_destroyed {
                    app_handle.exit(0);
                }
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
        assert!(DOCS_PATH.starts_with('/'), "DOCS_PATH must start with /");
    }

    #[test]
    fn docs_url_is_root_on_port() {
        assert_eq!(docs_url(), format!("http://localhost:{PORT}/"));
        let url: Result<tauri::Url, _> = docs_url().parse();
        assert!(url.is_ok(), "docs_url should parse: {}", docs_url());
    }

    #[test]
    fn claude_dir_is_absolute_and_not_home() {
        let c = claude_dir();
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

    #[test]
    fn navigation_filter_allows_localhost_only() {
        let ok: tauri::Url = "http://localhost:4892/docs/".parse().unwrap();
        let loop_ok: tauri::Url = "http://127.0.0.1:4892/".parse().unwrap();
        let external: tauri::Url = "https://example.com/".parse().unwrap();
        assert!(allow_navigation(&ok));
        assert!(allow_navigation(&loop_ok));
        assert!(
            !allow_navigation(&external),
            "external links must open in OS browser"
        );
    }

    // ── tauri.conf.json assertions ──────────────────

    #[test]
    fn tauri_conf_devurl_points_to_port_4892_root() {
        let conf = read_tauri_conf();
        let dev_url = conf["build"]["devUrl"]
            .as_str()
            .expect("devUrl must be a string");
        assert_eq!(
            dev_url,
            docs_url(),
            "devUrl should equal http://localhost:{PORT}/"
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
    fn tauri_conf_bundles_app_project_not_dist_only() {
        // The writable workspace copy needs the whole app/ (incl. node_modules),
        // so resources must bundle ../app/** — not just ../app/dist/**.
        let conf = read_tauri_conf();
        let resources = conf["bundle"]["resources"].clone();
        let bundles_app = match &resources {
            serde_json::Value::String(s) => s.contains("app/"),
            serde_json::Value::Array(arr) => arr
                .iter()
                .any(|v| v.as_str().map(|s| s.contains("app/")).unwrap_or(false)),
            _ => false,
        };
        assert!(
            bundles_app,
            "bundle.resources should include ../app/**, got: {resources}"
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

    #[test]
    fn loading_page_wires_launch_error_and_retry_launch() {
        let html_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("frontend")
            .join("index.html");
        let html = std::fs::read_to_string(&html_path).expect("Failed to read frontend/index.html");
        assert!(
            html.contains("\"launch-error\""),
            "frontend/index.html should listen for the launch-error event"
        );
        assert!(
            html.contains("\"retry_launch\""),
            "frontend/index.html should invoke the retry_launch command"
        );
    }
}
