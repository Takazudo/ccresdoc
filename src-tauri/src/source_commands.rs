//! Narrow document-source editor commands. The ordinary docs renderer never
//! receives the settings file or catalog in an SSR payload. Every editor call
//! is authorized against the owned main WebView and current runtime.

use std::fs;
use std::sync::atomic::Ordering;

use serde::Serialize;
use tauri::{AppHandle, Manager, State, WebviewWindow};
use tauri_plugin_dialog::DialogExt;

use crate::runtime::{RuntimeApplyResult, RuntimePhase};
use crate::settings::{
    builtin_presets, ContentRevision, LoadStatus, SaveError, SettingsDiagnostic, SourceCatalog,
    UserPreset,
};
use crate::settings_commands::{CommandError, MAIN_WINDOW_LABEL};
use crate::settings_window::SETTINGS_WINDOW_LABEL;
use crate::AppState;

pub const EDITOR_PATH: &str = "/docs/ccresdoc/document-sources/";

#[tauri::command]
pub(crate) fn open_document_source_editor(
    window: WebviewWindow,
    app: AppHandle,
) -> Result<(), CommandError> {
    if window.label() != SETTINGS_WINDOW_LABEL {
        return Err(CommandError::new(
            "forbidden_window",
            "this action requires Settings",
        ));
    }
    let state = app.state::<AppState>();
    let port = state
        .resources
        .lock()
        .unwrap()
        .as_ref()
        .map(|runtime| runtime.effective.effective_port)
        .ok_or_else(|| {
            CommandError::new(
                "runtime_unavailable",
                "the document server is not running; repair a source here and retry",
            )
        })?;
    let main = app
        .get_webview_window(MAIN_WINDOW_LABEL)
        .ok_or_else(|| CommandError::new("window", "main window is unavailable"))?;
    let url = format!("http://localhost:{port}{EDITOR_PATH}")
        .parse::<tauri::Url>()
        .map_err(|error| CommandError::new("url", error.to_string()))?;
    let already_open = main.url().is_ok_and(|current| {
        current.path() == EDITOR_PATH
            && crate::browser_bridge::validate_active_docs_url(&current, port).is_ok()
    });
    if !already_open {
        main.navigate(url)
            .map_err(|error| CommandError::new("window", format!("open source editor: {error}")))?;
    }
    main.show()
        .map_err(|error| CommandError::new("window", error.to_string()))?;
    main.set_focus()
        .map_err(|error| CommandError::new("window", error.to_string()))?;
    Ok(())
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceEditorSnapshot {
    pub catalog: SourceCatalog,
    pub builtin_presets: Vec<UserPreset>,
    pub revision: Option<ContentRevision>,
    pub load_status: LoadStatus,
    pub validation: Vec<SettingsDiagnostic>,
    pub runtime_phase: RuntimePhase,
    pub runtime_generation: u64,
    pub pending_apply: bool,
    pub active_source_ids: Vec<String>,
    pub runtime_error: Option<String>,
}

fn snapshot(state: &AppState) -> SourceEditorSnapshot {
    let settings = state.settings_store.load();
    let runtime = state.runtime.snapshot();
    let pending_apply =
        settings.revision != runtime.authored.revision || runtime.phase != RuntimePhase::Ready;
    let owned_generation = state
        .resources
        .lock()
        .unwrap()
        .as_ref()
        .map_or(runtime.generation, |owned| owned.generation);
    SourceEditorSnapshot {
        catalog: settings.source_catalog,
        builtin_presets: builtin_presets(),
        revision: settings.revision,
        load_status: settings.status,
        validation: settings.validation,
        runtime_phase: runtime.phase,
        runtime_generation: owned_generation,
        pending_apply,
        active_source_ids: runtime.active.map_or_else(Vec::new, |active| {
            active.sources.into_iter().map(|source| source.id).collect()
        }),
        runtime_error: runtime.diagnostic.map(|diagnostic| diagnostic.message),
    }
}

fn authorize_editor(
    window: &WebviewWindow,
    state: &AppState,
    expected_generation: Option<u64>,
) -> Result<u64, CommandError> {
    if window.label() != MAIN_WINDOW_LABEL {
        return Err(CommandError::new(
            "forbidden_window",
            "source editing requires the main window",
        ));
    }
    let (generation, port) = state
        .resources
        .lock()
        .unwrap()
        .as_ref()
        .map(|runtime| (runtime.generation, runtime.effective.effective_port))
        .ok_or_else(|| {
            CommandError::new("runtime_unavailable", "the document runtime is not active")
        })?;
    let coordinator = state.runtime.snapshot();
    let current_generation = state.runtime.generation().load(Ordering::SeqCst);
    if !owned_generation_is_current(generation, current_generation, coordinator.phase)
        || state
            .browser_bridge
            .bootstrap()
            .is_none_or(|bootstrap| bootstrap.runtime_generation != generation)
    {
        return Err(CommandError::new(
            "stale_generation",
            "reload the editor for the current document runtime",
        ));
    }
    let url = window
        .url()
        .map_err(|error| CommandError::new("caller_url", error.to_string()))?;
    validate_editor_scope(window.label(), &url, port, generation, expected_generation)?;
    Ok(generation)
}

fn owned_generation_is_current(owned: u64, coordinator: u64, phase: RuntimePhase) -> bool {
    owned == coordinator || phase == RuntimePhase::SavedNotActive
}

fn validate_editor_scope(
    label: &str,
    url: &tauri::Url,
    port: u16,
    generation: u64,
    expected_generation: Option<u64>,
) -> Result<(), CommandError> {
    if label != MAIN_WINDOW_LABEL {
        return Err(CommandError::new(
            "forbidden_window",
            "source editing requires the main window",
        ));
    }
    if expected_generation.is_some_and(|expected| expected != generation) {
        return Err(CommandError::new(
            "stale_generation",
            "reload the editor for the current document runtime",
        ));
    }
    crate::browser_bridge::validate_active_docs_url(url, port)?;
    // The article's normal TOC adds a fragment without leaving the trusted
    // editor document. Queries remain outside the editor command contract.
    if url.path() != EDITOR_PATH || url.query().is_some() {
        return Err(CommandError::new(
            "forbidden_route",
            "source editing requires the app-owned document article",
        ));
    }
    Ok(())
}

fn require_same_revision(
    state: &AppState,
    expected: Option<&ContentRevision>,
) -> Result<(), CommandError> {
    let actual = state.settings_store.load().revision;
    if actual.as_ref() != expected {
        return Err(CommandError::from(SaveError::RevisionConflict {
            expected: expected.cloned(),
            actual,
        }));
    }
    Ok(())
}

#[tauri::command]
pub(crate) async fn get_source_editor_snapshot(
    window: WebviewWindow,
    app: AppHandle,
) -> Result<SourceEditorSnapshot, CommandError> {
    authorize_editor(&window, &app.state::<AppState>(), None)?;
    let task_app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let state = task_app.state::<AppState>();
        state.runtime.with_serialized_apply(|| {
            authorize_editor(&window, &state, None)?;
            Ok(snapshot(&state))
        })
    })
    .await
    .map_err(|error| CommandError::new("task", format!("source snapshot failed: {error}")))?
}

#[tauri::command]
pub(crate) async fn validate_source_editor_draft(
    window: WebviewWindow,
    app: AppHandle,
    catalog: SourceCatalog,
    expected_generation: u64,
) -> Result<Vec<SettingsDiagnostic>, CommandError> {
    authorize_editor(&window, &app.state::<AppState>(), Some(expected_generation))?;
    let task_app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let state = task_app.state::<AppState>();
        authorize_editor(&window, &state, Some(expected_generation))?;
        Ok(state.settings_store.validate_source_catalog(&catalog))
    })
    .await
    .map_err(|error| CommandError::new("task", format!("source validation failed: {error}")))?
}

#[tauri::command]
pub(crate) async fn save_source_editor_draft(
    window: WebviewWindow,
    app: AppHandle,
    catalog: SourceCatalog,
    expected_revision: Option<ContentRevision>,
    expected_generation: u64,
) -> Result<SourceEditorSnapshot, CommandError> {
    authorize_editor(&window, &app.state::<AppState>(), Some(expected_generation))?;
    let task_app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let state = task_app.state::<AppState>();
        state.runtime.with_serialized_apply(|| {
            authorize_editor(&window, &state, Some(expected_generation))?;
            state
                .settings_store
                .save_source_catalog(&catalog, expected_revision.as_ref())
                .map_err(CommandError::from)?;
            Ok(snapshot(&state))
        })
    })
    .await
    .map_err(|error| CommandError::new("task", format!("source save failed: {error}")))?
}

#[tauri::command]
pub(crate) async fn rebase_source_editor_draft(
    window: WebviewWindow,
    app: AppHandle,
    catalog: SourceCatalog,
    base_catalog: SourceCatalog,
    stale_revision: ContentRevision,
    expected_generation: u64,
) -> Result<SourceEditorSnapshot, CommandError> {
    authorize_editor(&window, &app.state::<AppState>(), Some(expected_generation))?;
    let task_app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let state = task_app.state::<AppState>();
        state.runtime.with_serialized_apply(|| {
            authorize_editor(&window, &state, Some(expected_generation))?;
            state
                .settings_store
                .rebase_source_catalog(&catalog, &base_catalog, &stale_revision)
                .map_err(CommandError::from)?;
            Ok(snapshot(&state))
        })
    })
    .await
    .map_err(|error| CommandError::new("task", format!("source rebase failed: {error}")))?
}

#[tauri::command]
pub(crate) async fn apply_saved_sources(
    window: WebviewWindow,
    app: AppHandle,
    expected_revision: Option<ContentRevision>,
    expected_generation: u64,
) -> Result<RuntimeApplyResult, CommandError> {
    {
        let state = app.state::<AppState>();
        authorize_editor(&window, &state, Some(expected_generation))?;
    }
    let task_app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let state = task_app.state::<AppState>();
        authorize_editor(&window, &state, Some(expected_generation))?;
        require_same_revision(&state, expected_revision.as_ref())?;
        crate::settings_commands::apply_persisted_sources(
            &task_app,
            &state,
            expected_revision.as_ref(),
        )
    })
    .await
    .map_err(|error| CommandError::new("task", format!("apply task failed: {error}")))?
}

#[tauri::command]
pub(crate) async fn pick_document_source_directory(
    window: WebviewWindow,
    state: State<'_, AppState>,
    expected_generation: u64,
) -> Result<Option<String>, CommandError> {
    authorize_editor(&window, &state, Some(expected_generation))?;
    let selected = window.dialog().file().blocking_pick_folder();
    let Some(selected) = selected else {
        return Ok(None);
    };
    authorize_editor(&window, &state, Some(expected_generation))?;
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

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RecoveryAction {
    Disable { source_id: String },
    ReplaceRoot { source_id: String, root: String },
}

/// Bounded Settings-only repair remains available when the sidecar cannot run.
/// It changes one selected source in the same revision-aware settings store.
#[tauri::command]
pub(crate) async fn recover_document_source(
    window: WebviewWindow,
    app: AppHandle,
    action: RecoveryAction,
    expected_revision: Option<ContentRevision>,
) -> Result<RuntimeApplyResult, CommandError> {
    if window.label() != SETTINGS_WINDOW_LABEL {
        return Err(CommandError::new(
            "forbidden_window",
            "source recovery requires Settings",
        ));
    }
    let task_app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let state = task_app.state::<AppState>();
        state.runtime.with_serialized_apply(|| {
            require_same_revision(&state, expected_revision.as_ref())?;
            let mut catalog = state.settings_store.load().source_catalog;
            let source_id = match &action {
                RecoveryAction::Disable { source_id }
                | RecoveryAction::ReplaceRoot { source_id, .. } => source_id,
            }
            .clone();
            let source = catalog
                .sources
                .iter_mut()
                .find(|source| source.id == source_id)
                .ok_or_else(|| {
                    CommandError::new("unknown_source", "that source no longer exists")
                })?;
            match action {
                RecoveryAction::Disable { .. } => source.enabled = false,
                RecoveryAction::ReplaceRoot { root, .. } => source.root = root,
            }
            let saved = state
                .settings_store
                .save_source_catalog_for_recovery(&catalog, expected_revision.as_ref(), &source_id)
                .map_err(CommandError::from)?;
            Ok(crate::settings_commands::apply_saved(&task_app, &state, saved, true).0)
        })
    })
    .await
    .map_err(|error| CommandError::new("task", format!("recovery task failed: {error}")))?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failed_apply_keeps_the_previous_owned_editor_authorized() {
        assert!(owned_generation_is_current(
            4,
            5,
            RuntimePhase::SavedNotActive
        ));
        assert!(!owned_generation_is_current(4, 5, RuntimePhase::Starting));
        assert!(!owned_generation_is_current(4, 5, RuntimePhase::Ready));
    }

    #[test]
    fn editor_scope_rejects_wrong_window_origin_port_route_and_generation() {
        let url =
            tauri::Url::parse("http://localhost:4892/docs/ccresdoc/document-sources/").unwrap();
        assert!(validate_editor_scope("main", &url, 4892, 7, Some(7)).is_ok());
        let toc_url = tauri::Url::parse(
            "http://localhost:4892/docs/ccresdoc/document-sources/#how-changes-take-effect",
        )
        .unwrap();
        assert!(validate_editor_scope("main", &toc_url, 4892, 7, Some(7)).is_ok());
        assert_eq!(
            validate_editor_scope("settings", &url, 4892, 7, Some(7))
                .unwrap_err()
                .code,
            "forbidden_window"
        );
        assert_eq!(
            validate_editor_scope("main", &url, 4892, 7, Some(6))
                .unwrap_err()
                .code,
            "stale_generation"
        );
        for other in [
            "http://evil.example:4892/docs/ccresdoc/document-sources/",
            "http://localhost:4893/docs/ccresdoc/document-sources/",
            "https://localhost:4892/docs/ccresdoc/document-sources/",
        ] {
            assert_eq!(
                validate_editor_scope("main", &tauri::Url::parse(other).unwrap(), 4892, 7, Some(7))
                    .unwrap_err()
                    .code,
                "forbidden_docs_url"
            );
        }
        for other in [
            "http://localhost:4892/docs/ccresdoc/document-sources",
            "http://localhost:4892/docs/claude/",
            "http://localhost:4892/docs/ccresdoc/document-sources/?untrusted=1",
        ] {
            assert_eq!(
                validate_editor_scope("main", &tauri::Url::parse(other).unwrap(), 4892, 7, Some(7))
                    .unwrap_err()
                    .code,
                "forbidden_route"
            );
        }
    }
}
