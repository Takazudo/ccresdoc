use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, WebviewWindow};

use crate::menu::BrowserMenuHandles;
use crate::runtime::DOCS_PATH;
use crate::settings::{browser_command_catalog, normalize_shortcut_binding, ShortcutEntry};
use crate::settings_commands::{CommandError, MAIN_WINDOW_LABEL};
use crate::AppState;

pub const BROWSER_COMMAND_EVENT: &str = "ccresdoc://browser-command";
pub const BROWSER_BOOTSTRAP_EVENT: &str = "ccresdoc://browser-bootstrap";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BrowserCommandOrigin {
    NativeMenu,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserCommandEnvelope {
    pub command_id: String,
    pub origin: BrowserCommandOrigin,
    pub invocation_id: u64,
    pub runtime_generation: u64,
    /// Native host-only menu effects are already executed by Rust. The page
    /// observes the same bus envelope but must not execute them a second time.
    pub host_handled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeOwnedBinding {
    pub command_id: String,
    pub binding: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserHostCapabilities {
    pub reload_documentation: bool,
    pub open_in_default_browser: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserBootstrap {
    pub shortcut_entries: Vec<ShortcutEntry>,
    pub native_owned_bindings: Vec<NativeOwnedBinding>,
    pub host_capabilities: BrowserHostCapabilities,
    pub runtime_generation: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NavigationStateUpdate {
    pub can_go_back: bool,
    pub can_go_forward: bool,
    pub current_stable_path: String,
    pub runtime_generation: u64,
}

#[derive(Default)]
struct BridgeState {
    handles: Option<BrowserMenuHandles>,
    shortcuts: Vec<ShortcutEntry>,
    native_owned: Vec<NativeOwnedBinding>,
    capture_active: bool,
    page_available: bool,
    runtime_generation: Option<u64>,
    can_go_back: bool,
    can_go_forward: bool,
    current_stable_path: Option<String>,
}

pub struct BrowserBridge {
    state: Mutex<BridgeState>,
    invocation_sequence: AtomicU64,
}

impl BrowserBridge {
    pub fn new(shortcuts: Vec<ShortcutEntry>) -> Self {
        Self {
            state: Mutex::new(BridgeState {
                shortcuts,
                ..BridgeState::default()
            }),
            invocation_sequence: AtomicU64::new(0),
        }
    }

    pub fn install_handles(&self, handles: BrowserMenuHandles) {
        let mut state = self.state.lock().unwrap();
        state.handles = Some(handles);
        reconcile_locked(&mut state);
    }

    pub fn reconcile_shortcuts(&self, shortcuts: Vec<ShortcutEntry>) {
        let mut state = self.state.lock().unwrap();
        state.shortcuts = shortcuts;
        reconcile_locked(&mut state);
    }

    pub fn activate(&self, generation: u64) {
        let mut state = self.state.lock().unwrap();
        state.page_available = true;
        state.runtime_generation = Some(generation);
        state.can_go_back = false;
        state.can_go_forward = false;
        state.current_stable_path = Some(DOCS_PATH.into());
        reconcile_locked(&mut state);
    }

    pub fn deactivate(&self) {
        let mut state = self.state.lock().unwrap();
        state.page_available = false;
        state.runtime_generation = None;
        state.can_go_back = false;
        state.can_go_forward = false;
        state.current_stable_path = None;
        reconcile_locked(&mut state);
    }

    pub fn set_capture_active(&self, active: bool) {
        let mut state = self.state.lock().unwrap();
        state.capture_active = active;
        reconcile_locked(&mut state);
    }

    pub fn bootstrap(&self) -> Option<BrowserBootstrap> {
        let state = self.state.lock().unwrap();
        let generation = state.runtime_generation.filter(|_| state.page_available)?;
        Some(BrowserBootstrap {
            shortcut_entries: state.shortcuts.clone(),
            native_owned_bindings: state.native_owned.clone(),
            host_capabilities: BrowserHostCapabilities {
                reload_documentation: true,
                open_in_default_browser: true,
            },
            runtime_generation: generation,
        })
    }

    pub fn update_navigation(&self, update: NavigationStateUpdate) -> bool {
        if !valid_stable_docs_path(&update.current_stable_path) {
            return false;
        }
        let mut state = self.state.lock().unwrap();
        if !state.page_available || state.runtime_generation != Some(update.runtime_generation) {
            return false;
        }
        state.can_go_back = update.can_go_back;
        state.can_go_forward = update.can_go_forward;
        state.current_stable_path = Some(update.current_stable_path);
        update_enabled_locked(&state);
        true
    }

    pub fn native_envelope(&self, command_id: &str) -> Option<BrowserCommandEnvelope> {
        if !browser_command_catalog()
            .commands
            .iter()
            .any(|command| command.command_id == command_id)
        {
            return None;
        }
        let state = self.state.lock().unwrap();
        let generation = state.runtime_generation.filter(|_| state.page_available)?;
        Some(BrowserCommandEnvelope {
            command_id: command_id.into(),
            origin: BrowserCommandOrigin::NativeMenu,
            invocation_id: self.invocation_sequence.fetch_add(1, Ordering::SeqCst) + 1,
            runtime_generation: generation,
            host_handled: matches!(
                command_id,
                "reload-documentation" | "open-in-default-browser"
            ),
        })
    }
}

fn desired_primary_bindings(shortcuts: &[ShortcutEntry]) -> BTreeMap<&str, (&str, String)> {
    shortcuts
        .iter()
        .filter_map(|entry| {
            let binding = entry.bindings.first()?;
            let accelerator = normalize_shortcut_binding(binding)
                .ok()?
                .to_tauri_accelerator();
            // Tauri's string setter treats a parse failure as `None`; parse
            // with its menu backend first so "success" cannot mean removal.
            accelerator.parse::<muda::accelerator::Accelerator>().ok()?;
            Some((entry.command_id.as_str(), (binding.as_str(), accelerator)))
        })
        .collect()
}

fn reconcile_locked(state: &mut BridgeState) {
    state.native_owned.clear();
    let Some(handles) = state.handles.as_ref() else {
        return;
    };
    let desired = desired_primary_bindings(&state.shortcuts);
    let command_ids = handles
        .iter()
        .map(|(command_id, _)| command_id.to_string())
        .collect::<Vec<_>>();
    let native_owned = arbitrate_native_ownership(
        command_ids.iter().map(String::as_str),
        &desired,
        state.page_available && !state.capture_active,
        |command_id, accelerator| {
            let Some(item) = handles
                .iter()
                .find_map(|(candidate, item)| (candidate == command_id).then_some(item))
            else {
                return false;
            };
            item.set_accelerator(accelerator).is_ok()
        },
    );
    state.native_owned = native_owned;
    update_enabled_locked(state);
}

fn arbitrate_native_ownership<'a>(
    command_ids: impl Iterator<Item = &'a str>,
    desired: &BTreeMap<&str, (&str, String)>,
    accelerators_enabled: bool,
    mut mutate: impl FnMut(&str, Option<&str>) -> bool,
) -> Vec<NativeOwnedBinding> {
    let mut owned = Vec::new();
    for command_id in command_ids {
        // Removal must succeed before replacement. A failure leaves ownership
        // unclaimed so the WebView remains the safe fallback.
        if !mutate(command_id, None) || !accelerators_enabled {
            continue;
        }
        let Some((portable, accelerator)) = desired.get(command_id) else {
            continue;
        };
        if mutate(command_id, Some(accelerator)) {
            owned.push(NativeOwnedBinding {
                command_id: command_id.into(),
                binding: (*portable).into(),
            });
        }
    }
    owned.sort_by(|left, right| left.command_id.cmp(&right.command_id));
    owned
}

fn update_enabled_locked(state: &BridgeState) {
    let Some(handles) = state.handles.as_ref() else {
        return;
    };
    for (command_id, item) in handles.iter() {
        let enabled = state.page_available
            && match command_id {
                "back" => state.can_go_back,
                "forward" => state.can_go_forward,
                _ => true,
            };
        let _ = item.set_enabled(enabled);
    }
}

pub fn valid_stable_docs_path(path: &str) -> bool {
    if !path.starts_with(DOCS_PATH) || path.contains(['\\', '#', '?']) || path.starts_with("//") {
        return false;
    }
    let Ok(base) = tauri::Url::parse("http://localhost/docs/") else {
        return false;
    };
    base.join(path).is_ok_and(|url| {
        url.scheme() == "http"
            && url.host_str() == Some("localhost")
            && url.path().starts_with(DOCS_PATH)
            && url.query().is_none()
            && url.fragment().is_none()
    })
}

pub fn validate_active_docs_url(url: &tauri::Url, effective_port: u16) -> Result<(), CommandError> {
    let host_ok = matches!(url.host_str(), Some("localhost" | "127.0.0.1"));
    if url.scheme() != "http"
        || !host_ok
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port() != Some(effective_port)
        || !url.path().starts_with(DOCS_PATH)
        || url.fragment().is_some()
    {
        return Err(CommandError::new(
            "forbidden_docs_url",
            "the live page is not on the active loopback documentation server",
        ));
    }
    Ok(())
}

fn authorize_active_main(window: &WebviewWindow, state: &AppState) -> Result<u64, CommandError> {
    if window.label() != MAIN_WINDOW_LABEL {
        return Err(CommandError::new(
            "forbidden_window",
            "this command is only available to the main documentation window",
        ));
    }
    let (generation, port) = state
        .resources
        .lock()
        .unwrap()
        .as_ref()
        .map(|runtime| (runtime.generation, runtime.effective.effective_port))
        .ok_or_else(|| {
            CommandError::new(
                "runtime_unavailable",
                "the documentation server is not active",
            )
        })?;
    if state
        .browser_bridge
        .bootstrap()
        .is_none_or(|bootstrap| bootstrap.runtime_generation != generation)
    {
        return Err(CommandError::new(
            "stale_generation",
            "the browser bridge does not match the owned documentation server",
        ));
    }
    let url = window
        .url()
        .map_err(|error| CommandError::new("caller_url", error.to_string()))?;
    validate_active_docs_url(&url, port)?;
    Ok(generation)
}

#[tauri::command]
pub(crate) fn get_browser_bootstrap(
    window: WebviewWindow,
    state: tauri::State<'_, AppState>,
) -> Result<BrowserBootstrap, CommandError> {
    let generation = authorize_active_main(&window, &state)?;
    let bootstrap = state.browser_bridge.bootstrap().ok_or_else(|| {
        CommandError::new("runtime_unavailable", "the browser bridge is not active")
    })?;
    if bootstrap.runtime_generation != generation {
        return Err(CommandError::new(
            "stale_generation",
            "the browser bridge does not match the active runtime generation",
        ));
    }
    Ok(bootstrap)
}

#[tauri::command]
pub(crate) fn update_browser_navigation_state(
    window: WebviewWindow,
    state: tauri::State<'_, AppState>,
    update: NavigationStateUpdate,
) -> Result<(), CommandError> {
    let generation = authorize_active_main(&window, &state)?;
    if update.runtime_generation != generation || !state.browser_bridge.update_navigation(update) {
        return Err(CommandError::new(
            "stale_navigation_state",
            "navigation state does not match the active runtime generation",
        ));
    }
    Ok(())
}

#[tauri::command]
pub(crate) fn set_shortcut_capture_active(
    window: WebviewWindow,
    app: AppHandle,
    active: bool,
) -> Result<(), CommandError> {
    if window.label() != crate::settings_window::SETTINGS_WINDOW_LABEL {
        return Err(CommandError::new(
            "forbidden_window",
            "shortcut capture is only available to Settings",
        ));
    }
    app.state::<AppState>()
        .browser_bridge
        .set_capture_active(active);
    emit_browser_bootstrap(&app);
    Ok(())
}

#[tauri::command]
pub(crate) async fn open_current_page_in_default_browser(
    window: WebviewWindow,
    app: AppHandle,
) -> Result<(), CommandError> {
    let state = app.state::<AppState>();
    authorize_active_main(&window, &state)?;
    let url = app
        .get_webview_window(MAIN_WINDOW_LABEL)
        .ok_or_else(|| CommandError::new("window_unavailable", "main window is unavailable"))?
        .url()
        .map_err(|error| CommandError::new("caller_url", error.to_string()))?;
    let port = state
        .resources
        .lock()
        .unwrap()
        .as_ref()
        .map(|runtime| runtime.effective.effective_port)
        .ok_or_else(|| CommandError::new("runtime_unavailable", "the active server has no port"))?;
    validate_active_docs_url(&url, port)?;
    tauri::async_runtime::spawn_blocking(move || open::that(url.as_str()))
        .await
        .map_err(|error| CommandError::new("task", format!("open task failed: {error}")))?
        .map_err(|error| CommandError::new("open_failed", error.to_string()))
}

#[tauri::command]
pub(crate) fn reload_documentation(
    window: WebviewWindow,
    app: AppHandle,
) -> Result<(), CommandError> {
    authorize_active_main(&window, &app.state::<AppState>())?;
    crate::navigate_to_loading(&app);
    crate::start_launch(&app);
    Ok(())
}

pub fn emit_native_command(app: &AppHandle, command_id: &str) -> Option<BrowserCommandEnvelope> {
    let envelope = app
        .state::<AppState>()
        .browser_bridge
        .native_envelope(command_id)?;
    let _ = app.emit_to(MAIN_WINDOW_LABEL, BROWSER_COMMAND_EVENT, &envelope);
    Some(envelope)
}

pub fn emit_browser_bootstrap(app: &AppHandle) {
    if let Some(bootstrap) = app.state::<AppState>().browser_bridge.bootstrap() {
        let _ = app.emit_to(MAIN_WINDOW_LABEL, BROWSER_BOOTSTRAP_EVENT, bootstrap);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stable_paths_are_docs_only_and_cannot_change_url_components() {
        for valid in ["/docs/", "/docs/claude/", "/docs/a%20b/"] {
            assert!(valid_stable_docs_path(valid), "{valid}");
        }
        for invalid in [
            "/",
            "/docs",
            "//evil.test/docs/",
            "/docs/../admin/",
            "/docs/a?next=1",
            "/docs/a#fragment",
            "/docs\\evil",
        ] {
            assert!(!valid_stable_docs_path(invalid), "{invalid}");
        }
    }

    #[test]
    fn external_open_validation_pins_scheme_authority_port_and_docs_path() {
        for valid in [
            "http://localhost:6000/docs/",
            "http://127.0.0.1:6000/docs/codex/?q=one",
        ] {
            validate_active_docs_url(&valid.parse().unwrap(), 6000).unwrap();
        }
        for invalid in [
            "https://localhost:6000/docs/",
            "http://localhost:6001/docs/",
            "http://example.com:6000/docs/",
            "http://user@localhost:6000/docs/",
            "http://localhost:6000/",
            "http://localhost:6000/docs/#fragment",
            "http://localhost/docs/",
        ] {
            assert_eq!(
                validate_active_docs_url(&invalid.parse().unwrap(), 6000)
                    .unwrap_err()
                    .code,
                "forbidden_docs_url",
                "{invalid}"
            );
        }
        assert!(validate_active_docs_url(&"http://localhost/docs/".parse().unwrap(), 80).is_err());
    }

    #[test]
    fn primary_binding_selection_ignores_alternates_and_invalid_values() {
        let shortcuts = [
            ShortcutEntry {
                command_id: "back".into(),
                bindings: vec!["Mod+[".into(), "Alt+ArrowLeft".into()],
            },
            ShortcutEntry {
                command_id: "home".into(),
                bindings: vec!["not valid".into()],
            },
        ];
        let desired = desired_primary_bindings(&shortcuts);
        assert_eq!(desired["back"], ("Mod+[", "CmdOrCtrl+[".into()));
        assert!(!desired.contains_key("home"));
    }

    #[test]
    fn ownership_requires_successful_removal_and_installation() {
        let shortcuts = [ShortcutEntry {
            command_id: "back".into(),
            bindings: vec!["Mod+[".into()],
        }];
        let desired = desired_primary_bindings(&shortcuts);
        let ids = ["back"];

        let failed_remove =
            arbitrate_native_ownership(ids.into_iter(), &desired, true, |_, accelerator| {
                accelerator.is_some()
            });
        assert!(failed_remove.is_empty());

        let failed_install =
            arbitrate_native_ownership(ids.into_iter(), &desired, true, |_, accelerator| {
                accelerator.is_none()
            });
        assert!(failed_install.is_empty());

        let suspended = arbitrate_native_ownership(ids.into_iter(), &desired, false, |_, _| true);
        assert!(suspended.is_empty());

        let owned = arbitrate_native_ownership(ids.into_iter(), &desired, true, |_, _| true);
        assert_eq!(
            owned,
            [NativeOwnedBinding {
                command_id: "back".into(),
                binding: "Mod+[".into(),
            }]
        );
    }

    #[test]
    fn every_configured_primary_is_accepted_by_the_native_menu_parser() {
        let defaults = crate::settings::default_shortcut_entries();
        let desired = desired_primary_bindings(&defaults);
        assert_eq!(desired.len(), 5);
        for command in [
            "back",
            "forward",
            "reload-documentation",
            "find-in-page",
            "search-documentation",
        ] {
            assert!(desired.contains_key(command), "{command}");
        }
    }

    #[test]
    fn stale_navigation_updates_and_unknown_menu_commands_are_ignored() {
        let bridge = BrowserBridge::new(crate::settings::default_shortcut_entries());
        bridge.activate(7);
        assert!(!bridge.update_navigation(NavigationStateUpdate {
            can_go_back: true,
            can_go_forward: true,
            current_stable_path: "/docs/claude/".into(),
            runtime_generation: 6,
        }));
        assert!(bridge.update_navigation(NavigationStateUpdate {
            can_go_back: true,
            can_go_forward: false,
            current_stable_path: "/docs/claude/".into(),
            runtime_generation: 7,
        }));
        assert!(bridge.native_envelope("future-command").is_none());
        let first = bridge.native_envelope("back").unwrap();
        let second = bridge.native_envelope("back").unwrap();
        assert_eq!(first.runtime_generation, 7);
        assert!(!first.host_handled);
        assert_ne!(first.invocation_id, second.invocation_id);
        assert!(
            bridge
                .native_envelope("reload-documentation")
                .unwrap()
                .host_handled
        );
    }
}
