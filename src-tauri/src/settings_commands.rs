use std::collections::BTreeSet;
use std::fs;
#[cfg(target_os = "macos")]
use std::process::Command;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, Manager, State, WebviewWindow};
use tauri_plugin_dialog::DialogExt;

use crate::appearance::{AppearanceEnvelope, AppearanceSource, AppearanceValue, APPEARANCE_EVENT};
use crate::runtime::{ApplyStatus, RuntimeApplyResult, RuntimePhase, RuntimeSnapshot};
use crate::settings::{
    AppearanceMode, ApplyImpact, ContentRevision, EffectiveSettings, LoadStatus, SaveError,
    SaveResult, SettingField, SettingsDiagnostic, SettingsDraft, SettingsSnapshot,
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
    pub defaults: SettingsDraft,
    pub theme_packs: Vec<String>,
}

fn complete_snapshot(state: &AppState) -> CompleteSettingsSnapshot {
    let mut settings = state.settings_store.load();
    state
        .runtime
        .publish_authoritative_appearance(settings.clone());
    // A valid legacy value is a first-save draft candidate only. It never
    // changes file status/revision and disappears when the exact origin does.
    if settings.status == LoadStatus::Missing {
        if let Some(candidate) = state.appearance.candidate() {
            settings.authored.appearance_mode = candidate.mode.as_str().into();
            settings.authored.theme_pack = candidate.theme_pack.clone();
            settings.effective.appearance_mode = candidate.mode;
            settings.effective.theme_pack = candidate.theme_pack;
        }
    }
    let actions = action_availability(&settings.status, settings.revision.is_some());
    CompleteSettingsSnapshot {
        settings,
        runtime: state.runtime.snapshot(),
        actions,
        defaults: SettingsDraft::defaults(),
        theme_packs: state.settings_store.available_theme_packs(),
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
    Ok(snapshot)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AppearanceIntent {
    LegacyCandidate,
    Persist,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(deny_unknown_fields)]
pub struct AppearanceRequest {
    pub mode: AppearanceMode,
    pub theme_pack: String,
    pub intent: AppearanceIntent,
}

fn authorize_docs_url(url: &tauri::Url, effective_port: u16) -> Result<String, CommandError> {
    let host_ok = matches!(url.host_str(), Some("localhost" | "127.0.0.1"));
    if url.scheme() != "http"
        || !host_ok
        || url.port_or_known_default() != Some(effective_port)
        || !url.path().starts_with("/docs/")
    {
        return Err(CommandError::new(
            "forbidden_origin",
            "appearance mutation requires the active docs origin",
        ));
    }
    Ok(format!(
        "{}://{}:{}",
        url.scheme(),
        url.host_str().unwrap(),
        effective_port
    ))
}

fn docs_origin(window: &WebviewWindow, effective_port: u16) -> Result<String, CommandError> {
    let url = window
        .url()
        .map_err(|error| CommandError::new("caller_url", error.to_string()))?;
    authorize_docs_url(&url, effective_port)
}

fn validate_appearance(
    state: &AppState,
    mode: AppearanceMode,
    theme_pack: String,
) -> Result<AppearanceValue, CommandError> {
    if !state.settings_store.supports_theme_pack(&theme_pack) {
        return Err(CommandError::with_details(
            "invalid_theme_pack",
            "theme pack is not available",
            json!({ "themePack": theme_pack }),
        ));
    }
    Ok(AppearanceValue { mode, theme_pack })
}

#[tauri::command]
pub(crate) async fn update_appearance(
    window: WebviewWindow,
    app: AppHandle,
    request: AppearanceRequest,
) -> Result<AppearanceEnvelope, CommandError> {
    authorize(window.label(), &[MAIN_WINDOW_LABEL])?;
    let state = app.state::<AppState>();
    let origin = docs_origin(
        &window,
        state
            .effective_port
            .load(std::sync::atomic::Ordering::SeqCst),
    )?;
    let appearance = validate_appearance(&state, request.mode, request.theme_pack)?;
    if request.intent == AppearanceIntent::LegacyCandidate {
        let latest = state.settings_store.load();
        if latest.status == LoadStatus::Missing {
            state
                .appearance
                .report_candidate(origin, appearance.clone());
            let authoritative = crate::appearance::value_from_snapshot(&latest);
            return Ok(AppearanceEnvelope {
                appearance,
                authoritative,
                revision: None,
                source: AppearanceSource::LegacyCandidate,
                authoritative_source: AppearanceSource::Default,
            });
        }
        let envelope = state.appearance.envelope(&latest);
        let _ = app.emit(APPEARANCE_EVENT, &envelope);
        return Ok(envelope);
    }

    let task_app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let state = task_app.state::<AppState>();
        appearance_save_operation(&task_app, &state, || {
            state
                .settings_store
                .update_appearance(appearance.mode, &appearance.theme_pack)
        })?;
        state.appearance.clear_candidate();
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
    let appearance = validate_appearance(&state, mode, theme_pack)?;
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

    #[test]
    fn appearance_command_requires_the_exact_active_docs_origin() {
        assert_eq!(
            authorize_docs_url(&"http://localhost:6000/docs/".parse().unwrap(), 6000).unwrap(),
            "http://localhost:6000"
        );
        for url in [
            "http://localhost:6001/docs/",
            "http://localhost:6000/",
            "https://localhost:6000/docs/",
            "http://example.com:6000/docs/",
        ] {
            assert_eq!(
                authorize_docs_url(&url.parse().unwrap(), 6000)
                    .unwrap_err()
                    .code,
                "forbidden_origin"
            );
        }
    }

    #[test]
    fn appearance_request_rejects_extra_fields_invalid_modes_and_nonappearance_shape() {
        let valid: AppearanceRequest = serde_json::from_value(json!({
            "mode": "dark", "themePack": "default", "intent": "persist"
        }))
        .unwrap();
        assert_eq!(valid.mode, AppearanceMode::Dark);
        for invalid in [
            json!({ "mode": "sepia", "themePack": "default", "intent": "persist" }),
            json!({ "mode": "dark", "themePack": "default", "intent": "persist", "preferredPort": 1 }),
            json!({ "claudeDir": "/tmp", "intent": "persist" }),
        ] {
            assert!(serde_json::from_value::<AppearanceRequest>(invalid).is_err());
        }
    }
}
