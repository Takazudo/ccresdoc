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
    pub shortcut_catalog: CommandCatalog,
}

fn complete_snapshot(state: &AppState) -> CompleteSettingsSnapshot {
    let mut settings = state.settings_store.load();
    state
        .browser_bridge
        .reconcile_shortcuts(settings.effective.shortcuts.clone());
    state
        .runtime
        .publish_authoritative_restart_free_settings(settings.clone());
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AppearanceIntent {
    LegacyCandidate,
    Persist,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AppearanceField {
    Mode,
    ThemePack,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(deny_unknown_fields)]
pub struct AppearanceRequest {
    pub mode: AppearanceMode,
    pub theme_pack: String,
    pub intent: AppearanceIntent,
    pub field: AppearanceField,
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

fn caller_url(window: &WebviewWindow) -> Result<tauri::Url, CommandError> {
    window
        .url()
        .map_err(|error| CommandError::new("caller_url", error.to_string()))
}

fn validate_appearance(
    supports_theme_pack: impl Fn(&str) -> bool,
    mode: AppearanceMode,
    theme_pack: String,
) -> Result<AppearanceValue, CommandError> {
    if !supports_theme_pack(&theme_pack) {
        return Err(CommandError::with_details(
            "invalid_theme_pack",
            "theme pack is not available",
            json!({ "themePack": theme_pack }),
        ));
    }
    Ok(AppearanceValue { mode, theme_pack })
}

/// What `update_appearance` does once the caller is authorized and the
/// request is valid. Parity-tested against `fixtures/appearance-transitions.json`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum AppearanceDecision {
    /// Resolve against the latest load status with `legacy_candidate_outcome`.
    LegacyCandidate {
        origin: String,
        appearance: AppearanceValue,
    },
    /// Merge exactly one field through `SettingsStore::update_appearance`, then
    /// `apply_saved` (which emits the event), then `clear_candidate`.
    Persist {
        mode: Option<AppearanceMode>,
        theme_pack: Option<String>,
    },
}

/// Pure request resolution: origin check, then theme-pack validation (for
/// both intents and both fields), then intent/field branching.
pub(crate) fn resolve_appearance_update(
    caller_url: &tauri::Url,
    effective_port: u16,
    request: AppearanceRequest,
    supports_theme_pack: impl Fn(&str) -> bool,
) -> Result<AppearanceDecision, CommandError> {
    let origin = authorize_docs_url(caller_url, effective_port)?;
    let appearance = validate_appearance(supports_theme_pack, request.mode, request.theme_pack)?;
    Ok(match request.intent {
        AppearanceIntent::LegacyCandidate => {
            AppearanceDecision::LegacyCandidate { origin, appearance }
        }
        AppearanceIntent::Persist => match request.field {
            AppearanceField::Mode => AppearanceDecision::Persist {
                mode: Some(appearance.mode),
                theme_pack: None,
            },
            AppearanceField::ThemePack => AppearanceDecision::Persist {
                mode: None,
                theme_pack: Some(appearance.theme_pack),
            },
        },
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum LegacyCandidateOutcome {
    /// Missing config: report the candidate and return this envelope; no event.
    ReportCandidate(AppearanceEnvelope),
    /// Present config: return and emit `AppearanceState::envelope(latest)`;
    /// the request value is ignored and any earlier candidate is kept.
    EmitCurrent,
}

pub(crate) fn legacy_candidate_outcome(
    latest: &SettingsSnapshot,
    appearance: &AppearanceValue,
) -> LegacyCandidateOutcome {
    if latest.status != LoadStatus::Missing {
        return LegacyCandidateOutcome::EmitCurrent;
    }
    LegacyCandidateOutcome::ReportCandidate(AppearanceEnvelope {
        appearance: appearance.clone(),
        authoritative: crate::appearance::value_from_snapshot(latest),
        revision: None,
        source: AppearanceSource::LegacyCandidate,
        authoritative_source: AppearanceSource::Default,
    })
}

#[tauri::command]
pub(crate) async fn update_appearance(
    window: WebviewWindow,
    app: AppHandle,
    request: AppearanceRequest,
) -> Result<AppearanceEnvelope, CommandError> {
    authorize(window.label(), &[MAIN_WINDOW_LABEL])?;
    let state = app.state::<AppState>();
    let decision = resolve_appearance_update(
        &caller_url(&window)?,
        state
            .effective_port
            .load(std::sync::atomic::Ordering::SeqCst),
        request,
        |slug| state.settings_store.supports_theme_pack(slug),
    )?;
    let (mode, theme_pack) = match decision {
        AppearanceDecision::LegacyCandidate { origin, appearance } => {
            let latest = state.settings_store.load();
            return Ok(match legacy_candidate_outcome(&latest, &appearance) {
                LegacyCandidateOutcome::ReportCandidate(envelope) => {
                    state.appearance.report_candidate(origin, appearance);
                    envelope
                }
                LegacyCandidateOutcome::EmitCurrent => {
                    let envelope = state.appearance.envelope(&latest);
                    let _ = app.emit(APPEARANCE_EVENT, &envelope);
                    envelope
                }
            });
        }
        AppearanceDecision::Persist { mode, theme_pack } => (mode, theme_pack),
    };

    let task_app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let state = task_app.state::<AppState>();
        appearance_save_operation(&task_app, &state, || {
            state
                .settings_store
                .update_appearance(mode, theme_pack.as_deref())
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
            "mode": "dark", "themePack": "default", "intent": "persist", "field": "mode"
        }))
        .unwrap();
        assert_eq!(valid.mode, AppearanceMode::Dark);
        for invalid in [
            json!({ "mode": "sepia", "themePack": "default", "intent": "persist", "field": "mode" }),
            json!({ "mode": "dark", "themePack": "default", "intent": "persist", "field": "mode", "preferredPort": 1 }),
            json!({ "claudeDir": "/tmp", "intent": "persist" }),
        ] {
            assert!(serde_json::from_value::<AppearanceRequest>(invalid).is_err());
        }
    }

    mod appearance_transitions {
        use super::*;
        use crate::appearance::{value_from_snapshot, AppearanceState};
        use crate::settings::SettingsStore;
        use std::path::Path;

        const FIXTURE: &str = include_str!("../fixtures/appearance-transitions.json");

        struct Harness {
            _root: tempfile::TempDir,
            store: SettingsStore,
            appearance: AppearanceState,
        }

        fn config_toml(home: &Path, mode: &str, theme_pack: &str) -> String {
            format!(
                "schema_version = 1\n\n[resources]\nclaude = true\ncodex = false\n\n[source]\nclaude_dir = {:?}\ncodex_dir = \"~/.codex\"\n\n[appearance]\nmode = {mode:?}\ntheme_pack = {theme_pack:?}\n\n[server]\npreferred_port = 5000\nfallback_to_free_port = false\n",
                home.join(".claude").to_string_lossy()
            )
        }

        fn harness(input: &Value) -> Harness {
            let root = tempfile::tempdir().unwrap();
            let home = root.path().join("home");
            fs::create_dir_all(home.join(".claude")).unwrap();
            let packs: Vec<String> =
                serde_json::from_value(input["availableThemePacks"].clone()).unwrap();
            let store = SettingsStore::with_theme_packs(
                root.path().join("config/config.toml"),
                home.clone(),
                packs,
            );
            let status: LoadStatus = serde_json::from_value(input["configStatus"].clone()).unwrap();
            let path = store.path().to_path_buf();
            if status != LoadStatus::Missing {
                fs::create_dir_all(path.parent().unwrap()).unwrap();
            }
            match status {
                LoadStatus::Missing => {}
                LoadStatus::Valid => {
                    let stored: AppearanceValue =
                        serde_json::from_value(input["stored"].clone()).unwrap();
                    fs::write(
                        &path,
                        config_toml(&home, stored.mode.as_str(), &stored.theme_pack),
                    )
                    .unwrap();
                }
                LoadStatus::Invalid => {
                    fs::write(&path, config_toml(&home, "sepia", "default")).unwrap();
                }
                LoadStatus::Malformed => {
                    fs::write(&path, "schema_version = 1\n[source\nnope").unwrap();
                }
                LoadStatus::UnsupportedVersion => {
                    fs::write(&path, "schema_version = 2\nfuture = true\n").unwrap();
                }
                LoadStatus::Unreadable => {
                    use std::os::unix::fs::PermissionsExt;
                    fs::write(&path, "schema_version = 1\n").unwrap();
                    fs::set_permissions(&path, fs::Permissions::from_mode(0o000)).unwrap();
                }
            }

            let loaded = store.load();
            assert_eq!(loaded.status, status, "config status setup");
            assert_eq!(
                loaded.revision.is_some(),
                !input["revision"].is_null(),
                "input.revision must be null exactly when the backend computes none"
            );
            if status == LoadStatus::Valid {
                assert_eq!(value_from_snapshot(&loaded), stored_value(&input["stored"]));
            } else {
                assert!(input["stored"].is_null());
            }

            let appearance = AppearanceState::default();
            if !input["preview"].is_null() {
                appearance.set_preview(serde_json::from_value(input["preview"].clone()).unwrap());
            }
            if !input["candidate"].is_null() {
                appearance.report_candidate(
                    input["candidate"]["origin"].as_str().unwrap().to_string(),
                    serde_json::from_value(input["candidate"]["appearance"].clone()).unwrap(),
                );
            }
            Harness {
                _root: root,
                store,
                appearance,
            }
        }

        fn stored_value(value: &Value) -> AppearanceValue {
            serde_json::from_value(value.clone()).unwrap()
        }

        /// Test double for the command's side-effect path: the same store and
        /// appearance-state calls in the same order, with the Tauri runtime
        /// apply replaced by the event it emits (`apply_saved` emits
        /// `envelope(load())` before `clear_candidate`).
        fn run(
            harness: &Harness,
            caller_url: &tauri::Url,
            effective_port: u16,
            request: AppearanceRequest,
        ) -> Result<(AppearanceEnvelope, Option<AppearanceEnvelope>), CommandError> {
            let decision =
                resolve_appearance_update(caller_url, effective_port, request, |slug| {
                    harness.store.supports_theme_pack(slug)
                })?;
            match decision {
                AppearanceDecision::LegacyCandidate { origin, appearance } => {
                    let latest = harness.store.load();
                    Ok(match legacy_candidate_outcome(&latest, &appearance) {
                        LegacyCandidateOutcome::ReportCandidate(envelope) => {
                            harness.appearance.report_candidate(origin, appearance);
                            (envelope, None)
                        }
                        LegacyCandidateOutcome::EmitCurrent => {
                            let envelope = harness.appearance.envelope(&latest);
                            (envelope.clone(), Some(envelope))
                        }
                    })
                }
                AppearanceDecision::Persist { mode, theme_pack } => {
                    harness
                        .store
                        .update_appearance(mode, theme_pack.as_deref())
                        .map_err(CommandError::from)?;
                    let emitted = harness.appearance.envelope(&harness.store.load());
                    harness.appearance.clear_candidate();
                    let envelope = harness.appearance.envelope(&harness.store.load());
                    Ok((envelope, Some(emitted)))
                }
            }
        }

        fn assert_revision(
            name: &str,
            expected: &Value,
            actual: &Option<ContentRevision>,
            before: &Option<ContentRevision>,
        ) {
            match expected.as_str() {
                None => assert!(expected.is_null() && actual.is_none(), "{name}: revision"),
                Some("rev:stored") => assert_eq!(actual, before, "{name}: revision unchanged"),
                Some("rev:after-save") => {
                    assert!(actual.is_some(), "{name}: revision after save");
                    assert_ne!(actual, before, "{name}: revision changed by save");
                }
                Some(other) => panic!("{name}: unknown revision placeholder {other}"),
            }
        }

        fn assert_candidate(name: &str, harness: &Harness, expected: &Value) {
            if expected.is_null() {
                assert_eq!(
                    harness.appearance.candidate(),
                    None,
                    "{name}: candidateAfter"
                );
                return;
            }
            let appearance = stored_value(&expected["appearance"]);
            assert_eq!(
                harness
                    .appearance
                    .candidate_for(expected["origin"].as_str().unwrap()),
                Some(appearance),
                "{name}: candidateAfter (origin and appearance)"
            );
        }

        fn assert_preview_kept(name: &str, harness: &Harness, preview: &Value) {
            let envelope = harness.appearance.envelope(&harness.store.load());
            if preview.is_null() {
                assert_ne!(
                    envelope.source,
                    AppearanceSource::Preview,
                    "{name}: preview"
                );
            } else {
                assert_eq!(
                    envelope.source,
                    AppearanceSource::Preview,
                    "{name}: preview"
                );
                assert_eq!(
                    envelope.appearance,
                    stored_value(preview),
                    "{name}: preview"
                );
            }
        }

        #[test]
        fn update_appearance_matches_every_transition_row() {
            let fixture: Value = serde_json::from_str(FIXTURE).unwrap();
            assert_eq!(fixture["schemaVersion"], 1);
            let constants = &fixture["constants"];
            let effective_port =
                u16::try_from(constants["effectivePort"].as_u64().unwrap()).unwrap();
            let caller_path = constants["callerPath"].as_str().unwrap();
            let defaults = stored_value(&constants["defaults"]);
            let draft = SettingsDraft::defaults();
            assert_eq!(
                (defaults.mode.as_str(), defaults.theme_pack.as_str()),
                (draft.appearance_mode.as_str(), draft.theme_pack.as_str())
            );
            let rows = fixture["rows"].as_array().unwrap();
            assert!(!rows.is_empty());

            let mut checked = 0;
            for row in rows {
                let name = row["name"].as_str().unwrap();
                assert!(
                    matches!(row["parity"].as_str(), Some("rust+stub" | "rust-only")),
                    "{name}: parity"
                );
                let input = &row["input"];
                let expect = &row["expect"];
                let parsed = serde_json::from_value::<AppearanceRequest>(row["request"].clone());
                if expect["error"] == "invalid_args" {
                    assert!(parsed.is_err(), "{name}: request must fail deserialization");
                    checked += 1;
                    continue;
                }
                let request = parsed.unwrap_or_else(|error| panic!("{name}: {error}"));

                let harness = harness(input);
                let before = harness.store.load();
                let caller_url: tauri::Url =
                    format!("{}{caller_path}", input["callerOrigin"].as_str().unwrap())
                        .parse()
                        .unwrap();
                let result = run(&harness, &caller_url, effective_port, request);

                if let Some(code) = expect["error"].as_str() {
                    let error = result
                        .err()
                        .unwrap_or_else(|| panic!("{name}: expected {code}"));
                    assert_eq!(error.code, code, "{name}: error code");
                    let after = harness.store.load();
                    assert_eq!(
                        after.status, before.status,
                        "{name}: stored status unchanged"
                    );
                    assert_eq!(
                        after.revision, before.revision,
                        "{name}: revision unchanged"
                    );
                    assert_eq!(
                        after.raw_content, before.raw_content,
                        "{name}: content unchanged"
                    );
                    assert_candidate(name, &harness, &input["candidate"]);
                    assert_preview_kept(name, &harness, &input["preview"]);
                    checked += 1;
                    continue;
                }

                let (envelope, emitted) =
                    result.unwrap_or_else(|error| panic!("{name}: unexpected {error:?}"));
                let expected = &expect["envelope"];
                assert_eq!(
                    envelope.appearance,
                    stored_value(&expected["appearance"]),
                    "{name}: appearance"
                );
                assert_eq!(
                    envelope.authoritative,
                    stored_value(&expected["authoritative"]),
                    "{name}: authoritative"
                );
                assert_eq!(
                    envelope.source,
                    serde_json::from_value(expected["source"].clone()).unwrap(),
                    "{name}: source"
                );
                assert_eq!(
                    envelope.authoritative_source,
                    serde_json::from_value(expected["authoritativeSource"].clone()).unwrap(),
                    "{name}: authoritativeSource"
                );
                assert_revision(
                    name,
                    &expected["revision"],
                    &envelope.revision,
                    &before.revision,
                );

                assert_eq!(
                    emitted.is_some(),
                    expect["emitsEvent"].as_bool().unwrap(),
                    "{name}: emitsEvent"
                );
                if let Some(payload) = emitted {
                    assert_eq!(
                        payload, envelope,
                        "{name}: event payload equals the envelope"
                    );
                }
                assert_candidate(name, &harness, &expect["candidateAfter"]);
                assert_preview_kept(name, &harness, &input["preview"]);

                let after = harness.store.load();
                if expect["storedAfter"].is_null() {
                    assert_eq!(after.status, LoadStatus::Missing, "{name}: storedAfter");
                } else {
                    assert_eq!(after.status, LoadStatus::Valid, "{name}: storedAfter");
                    assert_eq!(
                        value_from_snapshot(&after),
                        stored_value(&expect["storedAfter"]),
                        "{name}: storedAfter"
                    );
                }
                checked += 1;
            }
            assert_eq!(checked, rows.len());
        }
    }
}
