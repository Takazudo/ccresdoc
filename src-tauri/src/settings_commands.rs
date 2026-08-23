use std::collections::BTreeSet;
use std::fs;
#[cfg(target_os = "macos")]
use std::process::Command;

use serde::Serialize;
use serde_json::{json, Value};
use tauri::{AppHandle, Manager, State, WebviewWindow};
use tauri_plugin_dialog::DialogExt;

use crate::runtime::{ApplyStatus, RuntimeApplyResult, RuntimePhase, RuntimeSnapshot};
use crate::settings::{
    ApplyImpact, ContentRevision, EffectiveSettings, LoadStatus, SaveError, SaveResult,
    SettingField, SettingsDiagnostic, SettingsDraft, SettingsSnapshot,
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
    fn new(code: &'static str, message: impl Into<String>) -> Self {
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
        match error {
            SaveError::RevisionConflict { expected, actual } => Self::with_details(
                "revision_conflict",
                "settings changed since they were loaded",
                json!({ "expectedRevision": expected, "actualRevision": actual }),
            ),
            SaveError::Malformed => Self::new("malformed", error.to_string()),
            SaveError::UnsupportedVersion(version) => Self::with_details(
                "unsupported_version",
                error.to_string(),
                json!({ "schemaVersion": version }),
            ),
            SaveError::Unreadable(_) => Self::new("unreadable", error.to_string()),
            SaveError::Validation(diagnostics) => Self::with_details(
                "validation",
                "settings are invalid",
                json!({ "diagnostics": diagnostics }),
            ),
            SaveError::NotStale => Self::new("not_stale", error.to_string()),
            SaveError::LatestNotValid => Self::new("latest_not_valid", error.to_string()),
            SaveError::ReplacementNotAllowed => {
                Self::new("replacement_not_allowed", error.to_string())
            }
            SaveError::Io(_) => Self::new("io", error.to_string()),
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
}

fn complete_snapshot(state: &AppState) -> CompleteSettingsSnapshot {
    let settings = state.settings_store.load();
    let actions = action_availability(&settings.status, settings.revision.is_some());
    CompleteSettingsSnapshot {
        settings,
        runtime: state.runtime.snapshot(),
        actions,
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

fn apply_saved(app: &AppHandle, state: &AppState, saved: SaveResult) -> RuntimeApplyResult {
    let impact = saved.impact.clone();
    let before = state.runtime.snapshot();
    let effective = saved.snapshot.effective.clone();
    let generation = state.runtime.claim_generation();
    state.runtime.publish_starting(saved.snapshot, generation);

    let status = if matches!(impact, ApplyImpact::RestartRuntime) {
        launch(app, generation, effective, true);
        if state.runtime.snapshot().phase == RuntimePhase::Ready {
            ApplyStatus::Active
        } else {
            ApplyStatus::SavedNotActive
        }
    } else if let Some(mut active) = before.active {
        active.appearance_mode = effective.appearance_mode;
        active.theme_pack = effective.theme_pack;
        let port = active.effective_port;
        let preferred_port = active.preferred_port;
        state.runtime.publish_ready(
            active,
            crate::runtime::PortChoice {
                preferred_port,
                effective_port: port,
                fallback_used: before.fallback_used,
            },
            generation,
        );
        ApplyStatus::SavedNoRestart
    } else {
        if let Some(diagnostic) = before.diagnostic {
            state.runtime.publish_failed(diagnostic, generation);
        } else {
            state.runtime.publish_stopped(generation);
        }
        ApplyStatus::SavedNotActive
    };

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
        .with_serialized_apply(|| operation().map(|saved| apply_saved(app, state, saved)))
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
    state: State<'_, AppState>,
) -> Result<CompleteSettingsSnapshot, CommandError> {
    authorize_settings(&window)?;
    Ok(complete_snapshot(&state))
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
