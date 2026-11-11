use std::collections::BTreeSet;
use std::fs;
#[cfg(target_os = "macos")]
use std::process::Command;

use serde::Serialize;
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, Manager, State, WebviewWindow};
use tauri_plugin_dialog::DialogExt;

use crate::appearance::{
    persist_appearance_update, resolve_appearance_update, validate_appearance, AppearanceEnvelope,
    AppearanceRejection, AppearanceRequest, APPEARANCE_EVENT,
};
use crate::runtime::{ApplyStatus, RuntimeApplyResult, RuntimePhase, RuntimeSnapshot};
use crate::settings::{
    browser_command_catalog, AppearanceMode, ApplyImpact, CommandCatalog, ContentRevision,
    EffectiveSettings, LoadStatus, SaveError, SaveResult, SettingField, SettingsDiagnostic,
    SettingsDraft, SettingsSnapshot,
};
use crate::settings_window::{open_or_focus_settings, SETTINGS_WINDOW_LABEL};
use crate::{launch, AppState};

pub const MAIN_WINDOW_LABEL: &str = "main";

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandError {
    pub code: &'static str,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<Value>,
}

impl CommandError {
    pub(crate) fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            details: None,
        }
    }

    fn with_details(code: &'static str, message: impl Into<String>, details: Value) -> Self {
        Self {
            code,
            message: message.into(),
            details: Some(details),
        }
    }
}

impl From<SaveError> for CommandError {
    fn from(error: SaveError) -> Self {
        let code = error.code();
        match error {
            SaveError::RevisionConflict { expected, actual } => Self::with_details(
                code,
                "settings changed since they were loaded",
                json!({ "expectedRevision": expected, "actualRevision": actual }),
            ),
            SaveError::UnsupportedVersion(version) => {
                Self::with_details(code, error.to_string(), json!({ "schemaVersion": version }))
            }
            SaveError::Validation(diagnostics) => Self::with_details(
                code,
                "settings are invalid",
                json!({ "diagnostics": diagnostics }),
            ),
            other => Self::new(code, other.to_string()),
        }
    }
}

impl From<AppearanceRejection> for CommandError {
    fn from(rejection: AppearanceRejection) -> Self {
        match rejection {
            AppearanceRejection::ForbiddenOrigin => Self::new(
                rejection.code(),
                "appearance mutation requires the active docs origin",
            ),
            AppearanceRejection::InvalidThemePack(ref theme_pack) => Self::with_details(
                rejection.code(),
                "theme pack is not available",
                json!({ "themePack": theme_pack }),
            ),
        }
    }
}

fn authorize(caller_label: &str, allowed: &[&str]) -> Result<(), CommandError> {
    if allowed.contains(&caller_label) {
        Ok(())
    } else {
        Err(CommandError::with_details(
            "forbidden_window",
            "this command is not available to the caller window",
            json!({ "callerLabel": caller_label }),
        ))
    }
}

fn authorize_settings(window: &WebviewWindow) -> Result<(), CommandError> {
    authorize(window.label(), &[SETTINGS_WINDOW_LABEL])
}

#[tauri::command]
pub(crate) fn retry_launch(window: WebviewWindow, app: AppHandle) -> Result<(), CommandError> {
    authorize(window.label(), &[MAIN_WINDOW_LABEL])?;
    crate::start_launch(&app);
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActionAvailability {
    pub can_save: bool,
    pub can_rebase: bool,
    pub can_replace_malformed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CompleteSettingsSnapshot {
    pub settings: SettingsSnapshot,
    pub runtime: RuntimeSnapshot,
    pub actions: ActionAvailability,
    pub defaults: SettingsDraft,
    pub theme_packs: Vec<String>,
    pub shortcut_catalog: CommandCatalog,
}

fn complete_snapshot(state: &AppState) -> CompleteSettingsSnapshot {
    let settings = state.settings_store.load();
    state
        .browser_bridge
        .reconcile_shortcuts(settings.effective.shortcuts.clone());
    state
        .runtime
        .publish_authoritative_restart_free_settings(settings.clone());
    let actions = action_availability(&settings.status, settings.revision.is_some());
    CompleteSettingsSnapshot {
        settings,
        runtime: state.runtime.snapshot(),
        actions,
        defaults: SettingsDraft::defaults(),
        theme_packs: state.settings_store.available_theme_packs(),
        shortcut_catalog: browser_command_catalog(),
    }
}

fn action_availability(status: &LoadStatus, has_revision: bool) -> ActionAvailability {
    match status {
        LoadStatus::UnsupportedVersion | LoadStatus::Unreadable => ActionAvailability {
            can_save: false,
            can_rebase: false,
            can_replace_malformed: false,
        },
        LoadStatus::Malformed => ActionAvailability {
            can_save: false,
            can_rebase: false,
            can_replace_malformed: true,
        },
        LoadStatus::Valid => ActionAvailability {
            can_save: true,
            can_rebase: has_revision,
            can_replace_malformed: false,
        },
        _ => ActionAvailability {
            can_save: true,
            can_rebase: false,
            can_replace_malformed: false,
        },
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DraftValidation {
    pub effective: EffectiveSettings,
    pub diagnostics: Vec<SettingsDiagnostic>,
    pub valid: bool,
}

fn apply_saved(
    app: &AppHandle,
    state: &AppState,
    saved: SaveResult,
    clear_preview: bool,
) -> RuntimeApplyResult {
    let impact = saved.impact.clone();
    let before = state.runtime.snapshot();
    let effective = saved.snapshot.effective.clone();

    // A successful settings write is independently authoritative even when a
    // later source/port restart fails.
    state
        .browser_bridge
        .reconcile_shortcuts(effective.shortcuts.clone());
    crate::browser_bridge::emit_browser_bootstrap(app);

    let status = if matches!(impact, ApplyImpact::RestartRuntime) {
        let generation = state.runtime.claim_generation();
        state.runtime.publish_starting(saved.snapshot, generation);
        launch(app, generation, effective);
        if state.runtime.snapshot().phase == RuntimePhase::Ready {
            ApplyStatus::Active
        } else {
            ApplyStatus::SavedNotActive
        }
    } else {
        // Appearance-only persistence is not a resource generation. Keep the
        // active watcher callbacks on their existing generation lease while
        // updating only authored/active appearance fields.
        state
            .runtime
            .publish_authoritative_restart_free_settings(saved.snapshot);
        if before.active.is_some() {
            ApplyStatus::SavedNoRestart
        } else {
            ApplyStatus::SavedNotActive
        }
    };

    if clear_preview {
        state.appearance.clear_preview();
    }
    let authoritative = state.settings_store.load();
    let _ = app.emit(APPEARANCE_EVENT, state.appearance.envelope(&authoritative));

    RuntimeApplyResult {
        snapshot: state.runtime.snapshot(),
        impact,
        status,
    }
}

fn save_operation(
    app: &AppHandle,
    state: &AppState,
    operation: impl FnOnce() -> Result<SaveResult, SaveError>,
) -> Result<RuntimeApplyResult, CommandError> {
    state
        .runtime
        .with_serialized_apply(|| operation().map(|saved| apply_saved(app, state, saved, true)))
        .map_err(CommandError::from)
}

fn appearance_save_operation(
    app: &AppHandle,
    state: &AppState,
    operation: impl FnOnce() -> Result<SaveResult, SaveError>,
) -> Result<RuntimeApplyResult, CommandError> {
    state
        .runtime
        .with_serialized_apply(|| operation().map(|saved| apply_saved(app, state, saved, false)))
        .map_err(CommandError::from)
}

#[tauri::command]
pub(crate) fn open_settings_window(
    window: WebviewWindow,
    app: AppHandle,
) -> Result<(), CommandError> {
    authorize(window.label(), &[MAIN_WINDOW_LABEL])?;
    open_or_focus_settings(&app)
        .map_err(|error| CommandError::new("window", format!("open Settings: {error}")))
}

#[tauri::command]
pub(crate) fn get_settings_snapshot(
    window: WebviewWindow,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<CompleteSettingsSnapshot, CommandError> {
    authorize_settings(&window)?;
    let snapshot = state
        .runtime
        .with_serialized_apply(|| complete_snapshot(&state));
    let _ = app.emit(
        APPEARANCE_EVENT,
        state.appearance.envelope(&snapshot.settings),
    );
    crate::browser_bridge::emit_browser_bootstrap(&app);
    Ok(snapshot)
}

fn caller_url(window: &WebviewWindow) -> Result<tauri::Url, CommandError> {
    window
        .url()
        .map_err(|error| CommandError::new("caller_url", error.to_string()))
}

#[tauri::command]
pub(crate) async fn update_appearance(
    window: WebviewWindow,
    app: AppHandle,
    request: AppearanceRequest,
) -> Result<AppearanceEnvelope, CommandError> {
    authorize(window.label(), &[MAIN_WINDOW_LABEL])?;
    let state = app.state::<AppState>();
    let update = resolve_appearance_update(
        &caller_url(&window)?,
        state
            .effective_port
            .load(std::sync::atomic::Ordering::SeqCst),
        request,
        |slug| state.settings_store.supports_theme_pack(slug),
    )?;

    let task_app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let state = task_app.state::<AppState>();
        appearance_save_operation(&task_app, &state, || {
            persist_appearance_update(&state.settings_store, &update)
        })?;
        Ok(state.appearance.envelope(&state.settings_store.load()))
    })
    .await
    .map_err(|error| CommandError::new("task", format!("appearance task failed: {error}")))?
}

#[tauri::command]
pub(crate) fn preview_appearance(
    window: WebviewWindow,
    app: AppHandle,
    mode: AppearanceMode,
    theme_pack: String,
) -> Result<AppearanceEnvelope, CommandError> {
    authorize_settings(&window)?;
    let state = app.state::<AppState>();
    let appearance = validate_appearance(
        |slug| state.settings_store.supports_theme_pack(slug),
        mode,
        theme_pack,
    )?;
    let envelope = state.runtime.with_serialized_apply(|| {
        state.appearance.set_preview(appearance);
        state.appearance.envelope(&state.settings_store.load())
    });
    app.emit(APPEARANCE_EVENT, &envelope)
        .map_err(|error| CommandError::new("event", error.to_string()))?;
    Ok(envelope)
}

#[tauri::command]
pub(crate) fn clear_appearance_preview(
    window: WebviewWindow,
    app: AppHandle,
) -> Result<AppearanceEnvelope, CommandError> {
    authorize_settings(&window)?;
    let state = app.state::<AppState>();
    let envelope = state.runtime.with_serialized_apply(|| {
        state.appearance.clear_preview();
        state.appearance.envelope(&state.settings_store.load())
    });
    app.emit(APPEARANCE_EVENT, &envelope)
        .map_err(|error| CommandError::new("event", error.to_string()))?;
    Ok(envelope)
}

#[tauri::command]
pub(crate) fn validate_settings_draft(
    window: WebviewWindow,
    state: State<'_, AppState>,
    draft: SettingsDraft,
) -> Result<DraftValidation, CommandError> {
    authorize_settings(&window)?;
    let (effective, diagnostics) = state.settings_store.validate(&draft);
    let valid = !diagnostics.iter().any(|diagnostic| diagnostic.blocking);
    Ok(DraftValidation {
        effective,
        diagnostics,
        valid,
    })
}

#[tauri::command]
pub(crate) async fn save_and_apply_settings(
    window: WebviewWindow,
    app: AppHandle,
    draft: SettingsDraft,
    expected_revision: Option<ContentRevision>,
) -> Result<RuntimeApplyResult, CommandError> {
    authorize_settings(&window)?;
    app.state::<AppState>()
        .browser_bridge
        .set_capture_active(false);
    crate::browser_bridge::emit_browser_bootstrap(&app);
    let task_app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let state = task_app.state::<AppState>();
        save_operation(&task_app, &state, || {
            state
                .settings_store
                .save(&draft, expected_revision.as_ref())
        })
    })
    .await
    .map_err(|error| CommandError::new("task", format!("save task failed: {error}")))?
}

#[tauri::command]
pub(crate) async fn rebase_stale_settings(
    window: WebviewWindow,
    app: AppHandle,
    draft: SettingsDraft,
    dirty_fields: BTreeSet<SettingField>,
    stale_revision: ContentRevision,
) -> Result<RuntimeApplyResult, CommandError> {
    authorize_settings(&window)?;
    app.state::<AppState>()
        .browser_bridge
        .set_capture_active(false);
    crate::browser_bridge::emit_browser_bootstrap(&app);
    let task_app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let state = task_app.state::<AppState>();
        save_operation(&task_app, &state, || {
            state
                .settings_store
                .rebase_dirty(&draft, &dirty_fields, &stale_revision)
        })
    })
    .await
    .map_err(|error| CommandError::new("task", format!("rebase task failed: {error}")))?
}

#[tauri::command]
pub(crate) async fn replace_malformed_settings(
    window: WebviewWindow,
    app: AppHandle,
    draft: SettingsDraft,
    expected_revision: ContentRevision,
) -> Result<RuntimeApplyResult, CommandError> {
    authorize_settings(&window)?;
    app.state::<AppState>()
        .browser_bridge
        .set_capture_active(false);
    crate::browser_bridge::emit_browser_bootstrap(&app);
    let task_app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let state = task_app.state::<AppState>();
        save_operation(&task_app, &state, || {
            state
                .settings_store
                .replace_malformed(&draft, &expected_revision)
        })
    })
    .await
    .map_err(|error| CommandError::new("task", format!("replace task failed: {error}")))?
}

#[tauri::command]
pub(crate) async fn pick_source_directory(
    window: WebviewWindow,
) -> Result<Option<String>, CommandError> {
    authorize_settings(&window)?;
    let selected = window.dialog().file().blocking_pick_folder();
    let Some(selected) = selected else {
        return Ok(None);
    };
    let path = selected
        .into_path()
        .map_err(|error| CommandError::new("invalid_path", error.to_string()))?;
    let canonical = fs::canonicalize(&path).map_err(|error| {
        CommandError::new("invalid_path", format!("{}: {error}", path.display()))
    })?;
    if !canonical.is_dir() {
        return Err(CommandError::new(
            "invalid_path",
            "selected path is not a directory",
        ));
    }
    Ok(Some(canonical.to_string_lossy().into_owned()))
}

#[tauri::command]
pub(crate) fn open_config_file(
    window: WebviewWindow,
    state: State<'_, AppState>,
) -> Result<(), CommandError> {
    authorize_settings(&window)?;
    open::that(state.settings_store.path())
        .map_err(|error| CommandError::new("open_failed", error.to_string()))
}

#[tauri::command]
pub(crate) fn reveal_config_file(
    window: WebviewWindow,
    state: State<'_, AppState>,
) -> Result<(), CommandError> {
    authorize_settings(&window)?;
    let path = state.settings_store.path();
    let target = if path.exists() {
        path
    } else {
        path.parent().unwrap_or(path)
    };
    #[cfg(target_os = "macos")]
    {
        let status = Command::new("/usr/bin/open")
            .arg("-R")
            .arg(target)
            .status()
            .map_err(|error| CommandError::new("reveal_failed", error.to_string()))?;
        if !status.success() {
            return Err(CommandError::new(
                "reveal_failed",
                format!("open -R exited with {status}"),
            ));
        }
        Ok(())
    }
    #[cfg(not(target_os = "macos"))]
    {
        open::that(target).map_err(|error| CommandError::new("reveal_failed", error.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn privileged_commands_require_the_fixed_settings_label() {
        assert!(authorize(SETTINGS_WINDOW_LABEL, &[SETTINGS_WINDOW_LABEL]).is_ok());
        let error = authorize(MAIN_WINDOW_LABEL, &[SETTINGS_WINDOW_LABEL]).unwrap_err();
        assert_eq!(error.code, "forbidden_window");
        assert_eq!(error.details.unwrap()["callerLabel"], MAIN_WINDOW_LABEL);
    }

    #[test]
    fn open_settings_is_the_only_command_authorized_for_main() {
        assert!(authorize(MAIN_WINDOW_LABEL, &[MAIN_WINDOW_LABEL]).is_ok());
        assert!(authorize("docs-preview", &[MAIN_WINDOW_LABEL]).is_err());
        assert!(authorize(SETTINGS_WINDOW_LABEL, &[MAIN_WINDOW_LABEL]).is_err());
    }

    #[test]
    fn future_schema_and_malformed_snapshots_do_not_expose_unsafe_actions() {
        let future = action_availability(&LoadStatus::UnsupportedVersion, true);
        assert!(!future.can_save && !future.can_rebase && !future.can_replace_malformed);
        let malformed = action_availability(&LoadStatus::Malformed, true);
        assert!(!malformed.can_save && !malformed.can_rebase && malformed.can_replace_malformed);
        let invalid = action_availability(&LoadStatus::Invalid, true);
        assert!(invalid.can_save && !invalid.can_rebase && !invalid.can_replace_malformed);
    }
}
