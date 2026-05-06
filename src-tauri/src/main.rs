#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::fs;
use std::process::Command;
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use std::{env, thread};

use tauri::menu::{MenuBuilder, MenuItemBuilder, SubmenuBuilder};
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder};

const PORT: u16 = 4892;
const DOCS_PATH: &str = "/";
const IS_DEV: bool = cfg!(debug_assertions);

struct AppState {
    zoom: Mutex<f64>,
}

// ── Helpers ───────────────────────────────────────

fn home_dir() -> String {
    env::var("HOME").expect("HOME not set")
}

fn log(msg: &str) {
    use std::io::Write;
    // In production, log next to the binary in the bundle. In dev, use the
    // cargo workspace directory so the file is always discoverable.
    let path = log_path();
    if let Ok(mut f) = fs::OpenOptions::new().create(true).append(true).open(&path) {
        let secs = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let _ = writeln!(f, "[{secs}] {msg}");
    }
}

fn log_path() -> String {
    // $HOME/.claude/doc/src-tauri/launch.log — same location as the original
    // for continuity; S8 may move this to a proper app-support dir.
    format!("{}/.claude/doc/src-tauri/launch.log", home_dir())
}

fn docs_url() -> String {
    format!("http://localhost:{PORT}{DOCS_PATH}")
}

// ── Port cleanup ─────────────────────────────────

fn kill_port() {
    if let Ok(output) = Command::new("/usr/bin/lsof")
        .args(["-ti", &format!(":{PORT}")])
        .output()
    {
        let pids = String::from_utf8_lossy(&output.stdout);
        for line in pids.trim().lines() {
            if let Ok(pid) = line.trim().parse::<i32>() {
                log(&format!(
                    "kill_port: killing stale pid {pid} on port {PORT}"
                ));
                unsafe { libc::kill(pid, libc::SIGTERM) };
            }
        }
        if !pids.trim().is_empty() {
            thread::sleep(Duration::from_millis(500));
        }
    }
}

// ── Embedded server ──────────────────────────────

/// Spawn the embedded ccresdoc-server on a dedicated tokio runtime thread.
/// The server runs until the process exits (no shutdown signal in normal use).
///
/// `dist_dir` is resolved at runtime:
///   - production: `tauri::path::resource_dir()` + "app/dist"
///   - dev: `$HOME/.claude/app/dist` fallback (actual dev build outputs there)
fn start_embedded_server(dist_dir: std::path::PathBuf) {
    let claude_dir = std::path::PathBuf::from(home_dir()).join(".claude");
    let project_root = claude_dir.clone();

    log(&format!(
        "start_embedded_server: dist_dir={}",
        dist_dir.display()
    ));

    thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("Failed to create tokio runtime");
        rt.block_on(async {
            let config = ccresdoc_server::ServerConfig {
                port: PORT,
                claude_dir,
                project_root,
                dist_dir,
            };
            if let Err(e) =
                ccresdoc_server::serve_with_shutdown(config, std::future::pending()).await
            {
                log(&format!("start_embedded_server: server error: {e}"));
            }
        });
    });
}

// ── Readiness polling ────────────────────────────

fn curl_ready() -> String {
    Command::new("/usr/bin/curl")
        .args([
            "-s",
            "-o",
            "/dev/null",
            "-w",
            "%{http_code}",
            &format!("http://localhost:{PORT}/___ready"),
        ])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_else(|_| "err".to_string())
}

#[derive(Debug)]
enum ReadyResult {
    Ready,
    Timeout,
}

fn wait_for_ready(timeout: Duration) -> ReadyResult {
    log("wait_for_ready: start");
    let start = Instant::now();
    while start.elapsed() < timeout {
        let code = curl_ready();
        log(&format!("curl: {code} ({}s)", start.elapsed().as_secs()));
        if code == "200" {
            log("wait_for_ready: ready");
            return ReadyResult::Ready;
        }
        thread::sleep(Duration::from_secs(1));
    }
    log("wait_for_ready: TIMEOUT");
    ReadyResult::Timeout
}

fn emit_launch_error(app_handle: &AppHandle, result: &ReadyResult) {
    if matches!(result, ReadyResult::Ready) {
        return;
    }
    let reason = match result {
        ReadyResult::Ready => return,
        ReadyResult::Timeout => "timeout",
    };
    let lp = log_path();
    let payload = serde_json::json!({
        "reason": reason,
        "logPath": lp,
    });
    log(&format!("emit_launch_error: reason={reason}"));
    if let Some(w) = app_handle.get_webview_window("main") {
        if let Err(e) = w.emit("launch-error", payload) {
            log(&format!("emit_launch_error: emit failed: {e}"));
        }
    }
}

// ── Refresh ───────────────────────────────────────

/// Refresh simply re-navigates the window to the docs URL.
/// The embedded server is always live — no restart needed.
fn do_refresh(app_handle: &AppHandle) {
    if let Some(w) = app_handle.get_webview_window("main") {
        let _ = w.navigate(
            docs_url()
                .parse()
                .expect("BUG: docs_url produced an invalid URL"),
        );
    }
}

fn apply_zoom(app_handle: &AppHandle, level: f64) {
    let state = app_handle.state::<AppState>();
    *state.zoom.lock().unwrap() = level;
    if let Some(w) = app_handle.get_webview_window("main") {
        let _ = w.eval(&format!("document.body.style.zoom = '{level}'"));
    }
}

#[tauri::command]
fn refresh(app_handle: AppHandle) {
    do_refresh(&app_handle);
}

/// Frontend-callable retry for the loading page's error panel.
///
/// Spawned on a thread so the IPC call returns immediately.
#[tauri::command]
fn retry_launch(app_handle: AppHandle) {
    log("retry_launch: invoked from frontend");
    thread::spawn(move || do_refresh(&app_handle));
}

// ── Main ──────────────────────────────────────────

fn main() {
    if !IS_DEV {
        kill_port();
    }

    tauri::Builder::default()
        .manage(AppState {
            zoom: Mutex::new(1.0),
        })
        .invoke_handler(tauri::generate_handler![refresh, retry_launch])
        .setup(move |app| {
            // Resolve dist_dir for the embedded server.
            // In production: Resources/app/dist (bundled via tauri.conf.json bundle.resources).
            // In dev: fall back to $HOME/.claude/app/dist so `cargo tauri dev` works
            // against a locally-built app/dist/ without requiring the full bundle.
            let dist_dir = if IS_DEV {
                std::path::PathBuf::from(home_dir())
                    .join(".claude")
                    .join("app")
                    .join("dist")
            } else {
                // Tauri bundles resources from "../app/dist/**/*" (relative to src-tauri/).
                // When resources are specified with a ".." traversal, Tauri places them under
                // "_up_/<path>" inside Contents/Resources/ to represent the parent-dir step.
                // So the actual bundle path is Resources/_up_/app/dist/, not Resources/app/dist/.
                app.path()
                    .resource_dir()
                    .expect("resource_dir unavailable in production")
                    .join("_up_")
                    .join("app")
                    .join("dist")
            };

            start_embedded_server(dist_dir);

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
                "refresh" => {
                    let handle = app_handle.clone();
                    thread::spawn(move || do_refresh(&handle));
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

            // Show window immediately with the loading page, then navigate
            // once the embedded server is ready.
            if IS_DEV {
                // In dev mode the server starts concurrently; navigate directly
                // once it's ready rather than showing the bundled loading page.
                let url: tauri::Url = docs_url()
                    .parse()
                    .expect("BUG: docs_url produced an invalid URL");
                WebviewWindowBuilder::new(app, "main", WebviewUrl::External(url))
                    .title("CCResDoc")
                    .inner_size(1200.0, 800.0)
                    .build()?;
            } else {
                // Production: open with loading page first, then navigate once server ready.
                WebviewWindowBuilder::new(app, "main", WebviewUrl::default())
                    .title("CCResDoc")
                    .inner_size(1200.0, 800.0)
                    .build()?;

                let handle = app.handle().clone();
                thread::spawn(move || {
                    let result = wait_for_ready(Duration::from_secs(30));
                    match result {
                        ReadyResult::Ready => {
                            if let Some(w) = handle.get_webview_window("main") {
                                let url: tauri::Url = docs_url()
                                    .parse()
                                    .expect("BUG: docs_url produced an invalid URL");
                                let _ = w.navigate(url);
                            }
                        }
                        ReadyResult::Timeout => {
                            emit_launch_error(&handle, &result);
                        }
                    }
                });
            }

            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app_handle, event| match &event {
            tauri::RunEvent::WindowEvent {
                event: tauri::WindowEvent::Destroyed,
                ..
            } => {
                app_handle.exit(0);
            }
            _ => {}
        });
}

// ── Tests ─────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

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
    fn docs_url_format_is_valid() {
        let url_str = docs_url();
        let url: Result<tauri::Url, _> = url_str.parse();
        assert!(url.is_ok(), "Docs URL should be parseable: {url_str}");
    }

    // ── tauri.conf.json assertions ──────────────────

    #[test]
    fn tauri_conf_devurl_uses_correct_port() {
        let conf = read_tauri_conf();
        let dev_url = conf["build"]["devUrl"]
            .as_str()
            .expect("devUrl must be a string");
        let expected = format!("localhost:{PORT}");
        assert!(
            dev_url.contains(&expected),
            "devUrl '{dev_url}' should reference port {PORT}"
        );
    }

    #[test]
    fn tauri_conf_devurl_points_to_embedded_server() {
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

    /// No `beforeDevCommand` referencing pnpm dev:stable — the embedded server
    /// starts inside the Tauri binary itself.
    #[test]
    fn tauri_conf_no_pnpm_dev_stable() {
        let conf = read_tauri_conf();
        // beforeDevCommand should either be absent or empty string
        let cmd = conf["build"]["beforeDevCommand"].as_str().unwrap_or("");
        assert!(
            !cmd.contains("pnpm dev:stable"),
            "beforeDevCommand must not reference pnpm dev:stable (got '{cmd}')"
        );
    }

    /// `withGlobalTauri` must be true so the bundled loading page can reach
    /// window.__TAURI__.event.listen and window.__TAURI__.core.invoke without a bundler.
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

    /// beforeBuildCommand must run `zfb build` from the `app/` directory so
    /// `cargo tauri build` compiles the frontend before bundling.
    #[test]
    fn tauri_conf_before_build_command_builds_app() {
        let conf = read_tauri_conf();
        let cmd = conf["build"]["beforeBuildCommand"]
            .as_str()
            .expect("beforeBuildCommand must be a string");
        assert!(
            cmd.contains("../app") && cmd.contains("zfb build"),
            "beforeBuildCommand '{cmd}' should run zfb build from ../app"
        );
    }

    /// bundle.resources must include app/dist/** so the dist is bundled into
    /// the .app's Contents/Resources/ folder.
    #[test]
    fn tauri_conf_resources_include_app_dist() {
        let conf = read_tauri_conf();
        let resources = conf["bundle"]["resources"].clone();
        // Accepts either a string or an array of strings.
        let has_app_dist = match &resources {
            serde_json::Value::String(s) => s.contains("app/dist"),
            serde_json::Value::Array(arr) => arr
                .iter()
                .any(|v| v.as_str().map(|s| s.contains("app/dist")).unwrap_or(false)),
            _ => false,
        };
        assert!(
            has_app_dist,
            "bundle.resources should include app/dist/**, got: {resources}"
        );
    }

    // ── Loading page assertions ──────────────────

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
