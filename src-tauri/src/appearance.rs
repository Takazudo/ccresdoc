//! Appearance authority, exact-origin bootstrap, preview, and event payloads.
//!
//! Only mode/theme cross this boundary. The document-start script is installed
//! once on the main WebView and consumes a fresh `window.name` seed immediately
//! before each docs navigation, so fallback ports and restarts do not inherit a
//! different loopback origin's cache.
//!
//! This module has no Tauri dependency, so `crates/ccresdoc-host-domain`
//! compiles and tests it on hosts that cannot build the native app.

use serde::{Deserialize, Serialize};
use std::sync::{Mutex, MutexGuard};

use crate::settings::{
    AppearanceMode, ContentRevision, LoadStatus, SaveError, SettingsSnapshot, SettingsStore,
    DEFAULT_THEME_PACK,
};

pub const APPEARANCE_EVENT: &str = "ccresdoc://appearance";
pub const WINDOW_NAME_PREFIX: &str = "ccresdoc-appearance-v2:";
pub const CACHE_KEY: &str = "ccresdoc-appearance-v2";
/// The previous cache contract. Its presence marks storage from before the
/// schema-v2 clean break; the bootstrap removes it and zudo-doc's preference
/// keys once, then writes only current values.
pub const RETIRED_CACHE_KEY: &str = "ccresdoc-appearance-v1";
/// zudo-doc's own preference keys, kept in step with authoritative values.
pub const MODE_STORAGE_KEY: &str = "zudo-doc-theme";
pub const PACK_STORAGE_KEY: &str = "zudo-doc-theme-pack";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppearanceValue {
    pub mode: AppearanceMode,
    pub theme_pack: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AppearanceSource {
    Authoritative,
    Preview,
    Default,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppearanceEnvelope {
    pub appearance: AppearanceValue,
    pub authoritative: AppearanceValue,
    pub revision: Option<ContentRevision>,
    pub source: AppearanceSource,
    pub authoritative_source: AppearanceSource,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BootstrapSeed {
    pub appearance: AppearanceValue,
    pub revision: Option<ContentRevision>,
    pub authoritative: bool,
    pub available_theme_packs: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AppearanceField {
    Mode,
    ThemePack,
}

/// The docs page's only appearance mutation: persist one field.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(deny_unknown_fields)]
pub struct AppearanceRequest {
    pub mode: AppearanceMode,
    pub theme_pack: String,
    pub field: AppearanceField,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppearanceRejection {
    ForbiddenOrigin,
    InvalidThemePack(String),
}

impl AppearanceRejection {
    pub fn code(&self) -> &'static str {
        match self {
            Self::ForbiddenOrigin => "forbidden_origin",
            Self::InvalidThemePack(_) => "invalid_theme_pack",
        }
    }
}

/// The single field a valid request persists.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppearanceUpdate {
    pub mode: Option<AppearanceMode>,
    pub theme_pack: Option<String>,
}

pub fn authorize_docs_url(url: &url::Url, effective_port: u16) -> Result<(), AppearanceRejection> {
    let host_ok = matches!(url.host_str(), Some("localhost" | "127.0.0.1"));
    if url.scheme() != "http"
        || !host_ok
        || url.port_or_known_default() != Some(effective_port)
        || !url.path().starts_with("/docs/")
    {
        return Err(AppearanceRejection::ForbiddenOrigin);
    }
    Ok(())
}

pub fn validate_appearance(
    supports_theme_pack: impl Fn(&str) -> bool,
    mode: AppearanceMode,
    theme_pack: String,
) -> Result<AppearanceValue, AppearanceRejection> {
    if !supports_theme_pack(&theme_pack) {
        return Err(AppearanceRejection::InvalidThemePack(theme_pack));
    }
    Ok(AppearanceValue { mode, theme_pack })
}

/// Pure request resolution: origin check, then theme-pack validation (for both
/// fields), then the single field to persist. Parity-tested against
/// `fixtures/appearance-transitions.json`.
pub fn resolve_appearance_update(
    caller_url: &url::Url,
    effective_port: u16,
    request: AppearanceRequest,
    supports_theme_pack: impl Fn(&str) -> bool,
) -> Result<AppearanceUpdate, AppearanceRejection> {
    authorize_docs_url(caller_url, effective_port)?;
    let appearance = validate_appearance(supports_theme_pack, request.mode, request.theme_pack)?;
    Ok(match request.field {
        AppearanceField::Mode => AppearanceUpdate {
            mode: Some(appearance.mode),
            theme_pack: None,
        },
        AppearanceField::ThemePack => AppearanceUpdate {
            mode: None,
            theme_pack: Some(appearance.theme_pack),
        },
    })
}

/// Persist a resolved update through the store's revision-guarded merge.
pub fn persist_appearance_update(
    store: &SettingsStore,
    update: &AppearanceUpdate,
) -> Result<crate::settings::SaveResult, SaveError> {
    store.update_appearance(update.mode.clone(), update.theme_pack.as_deref())
}

#[derive(Default)]
struct AppearanceStateInner {
    preview: Option<AppearanceValue>,
}

#[derive(Default)]
pub struct AppearanceState(Mutex<AppearanceStateInner>);

impl AppearanceState {
    fn lock(&self) -> MutexGuard<'_, AppearanceStateInner> {
        self.0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    pub fn set_preview(&self, appearance: AppearanceValue) {
        self.lock().preview = Some(appearance);
    }

    pub fn clear_preview(&self) {
        self.lock().preview = None;
    }

    pub fn preview(&self) -> Option<AppearanceValue> {
        self.lock().preview.clone()
    }

    pub fn envelope(&self, snapshot: &SettingsSnapshot) -> AppearanceEnvelope {
        let authoritative = value_from_snapshot(snapshot);
        let authoritative_source = if snapshot.status == LoadStatus::Missing {
            AppearanceSource::Default
        } else {
            AppearanceSource::Authoritative
        };
        let (appearance, source) = match self.lock().preview.clone() {
            Some(preview) => (preview, AppearanceSource::Preview),
            None => (authoritative.clone(), authoritative_source.clone()),
        };
        AppearanceEnvelope {
            appearance,
            authoritative,
            revision: snapshot.revision.clone(),
            source,
            authoritative_source,
        }
    }
}

pub fn value_from_snapshot(snapshot: &SettingsSnapshot) -> AppearanceValue {
    AppearanceValue {
        mode: snapshot.effective.appearance_mode.clone(),
        theme_pack: snapshot.effective.theme_pack.clone(),
    }
}

pub fn bootstrap_seed(
    snapshot: &SettingsSnapshot,
    available_theme_packs: Vec<String>,
) -> BootstrapSeed {
    BootstrapSeed {
        appearance: value_from_snapshot(snapshot),
        revision: snapshot.revision.clone(),
        // Any existing TOML, including malformed/invalid TOML, is authoritative;
        // its safe effective projection is what the page paints.
        authoritative: snapshot.status != LoadStatus::Missing,
        available_theme_packs,
    }
}

pub fn window_name_script(seed: &BootstrapSeed, url: &str) -> String {
    let payload = serde_json::to_string(seed).expect("appearance seed is serializable");
    let name = serde_json::to_string(&format!("{WINDOW_NAME_PREFIX}{payload}"))
        .expect("window name is serializable");
    let url = serde_json::to_string(url).expect("docs URL is serializable");
    format!("window.name={name};window.location.replace({url});")
}

/// Main-frame document-start bootstrap. This is intentionally self-contained:
/// it runs before page code and never invokes a privileged command. The seed
/// is the only appearance input; storage is written, never read back as a
/// preference.
pub fn initialization_script(initial: &BootstrapSeed) -> String {
    let initial = serde_json::to_string(initial).expect("appearance seed is serializable");
    format!(
        r#"(function(){{
var PREFIX={prefix};var CACHE={cache};var RETIRED={retired};var MODE_KEY={mode_key};var PACK_KEY={pack_key};var FALLBACK={fallback};var seed={initial};
function validSeed(v){{return v&&typeof v==='object'&&v.appearance&&['system','light','dark'].indexOf(v.appearance.mode)!==-1&&typeof v.appearance.themePack==='string'&&Array.isArray(v.availableThemePacks);}}
try{{if(typeof window.name==='string'&&window.name.indexOf(PREFIX)===0){{var fresh=JSON.parse(window.name.slice(PREFIX.length));if(validSeed(fresh))seed=fresh;}}}}catch(e){{}}
var loc=window.location;var docs=(loc.protocol==='http:'&&(loc.hostname==='localhost'||loc.hostname==='127.0.0.1')&&loc.pathname.indexOf('/docs/')===0);if(!docs||!validSeed(seed))return;
var mode=seed.appearance.mode;var pack=seed.appearance.themePack;var source=seed.authoritative?'authoritative':'default';
if(seed.availableThemePacks.indexOf(pack)===-1)pack=FALLBACK;
var effective=mode==='system'?(window.matchMedia('(prefers-color-scheme: dark)').matches?'dark':'light'):mode;
try{{if(localStorage.getItem(RETIRED)!==null){{localStorage.removeItem(RETIRED);localStorage.removeItem(MODE_KEY);localStorage.removeItem(PACK_KEY);}}
localStorage.setItem(MODE_KEY,mode);localStorage.setItem(PACK_KEY,pack);localStorage.setItem(CACHE,JSON.stringify({{mode:mode,themePack:pack,revision:seed.revision}}));}}catch(e){{}}
function paint(r){{r.setAttribute('data-theme',effective);r.style.colorScheme=effective;r.setAttribute('data-theme-pack',pack);r.setAttribute('data-ccresdoc-appearance-ready','true');}}
if(document.documentElement)paint(document.documentElement);else{{var observer=new MutationObserver(function(){{if(document.documentElement){{paint(document.documentElement);observer.disconnect();}}}});observer.observe(document,{{childList:true}});}}
window.__CCRESDOC_APPEARANCE__={{mode:mode,themePack:pack,effectiveMode:effective,revision:seed.revision,source:source,origin:loc.origin}};
}})();"#,
        prefix = serde_json::to_string(WINDOW_NAME_PREFIX).unwrap(),
        cache = serde_json::to_string(CACHE_KEY).unwrap(),
        retired = serde_json::to_string(RETIRED_CACHE_KEY).unwrap(),
        mode_key = serde_json::to_string(MODE_STORAGE_KEY).unwrap(),
        pack_key = serde_json::to_string(PACK_STORAGE_KEY).unwrap(),
        fallback = serde_json::to_string(DEFAULT_THEME_PACK).unwrap(),
    )
}

pub fn bundled_initialization_script(value: &AppearanceValue) -> String {
    let value = serde_json::to_string(value).expect("appearance is serializable");
    format!(
        r#"(function(){{var a={value};var m=a.mode==='system'?(window.matchMedia('(prefers-color-scheme: dark)').matches?'dark':'light'):a.mode;function p(r){{r.setAttribute('data-theme',m);r.style.colorScheme=m;r.setAttribute('data-theme-pack',a.themePack);r.setAttribute('data-ccresdoc-appearance-ready','true');}}if(document.documentElement)p(document.documentElement);else{{var o=new MutationObserver(function(){{if(document.documentElement){{p(document.documentElement);o.disconnect();}}}});o.observe(document,{{childList:true}});}}}})();"#
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::SettingsDraft;
    use serde_json::Value;
    use std::fs;
    use std::path::{Path, PathBuf};

    fn snapshot(status: LoadStatus) -> SettingsSnapshot {
        let store = SettingsStore::with_theme_packs(
            PathBuf::from("/missing/config.toml"),
            PathBuf::from("/home"),
            ["classic", "graphite"],
        );
        let mut value = store.load();
        value.status = status;
        value.authored = SettingsDraft::defaults();
        value
    }

    #[test]
    fn only_a_missing_config_is_non_authoritative() {
        assert!(
            !bootstrap_seed(&snapshot(LoadStatus::Missing), vec!["classic".into()]).authoritative
        );
        for status in [
            LoadStatus::Valid,
            LoadStatus::Invalid,
            LoadStatus::Malformed,
            LoadStatus::Unreadable,
            LoadStatus::UnsupportedVersion,
        ] {
            assert!(bootstrap_seed(&snapshot(status), vec!["classic".into()]).authoritative);
        }
    }

    #[test]
    fn document_start_script_is_exact_origin_and_applies_before_marking_ready() {
        let script = initialization_script(&bootstrap_seed(
            &snapshot(LoadStatus::Missing),
            vec!["classic".into(), "graphite".into()],
        ));
        assert!(script.contains("loc.pathname.indexOf('/docs/')===0"));
        assert!(script.contains("loc.hostname==='localhost'||loc.hostname==='127.0.0.1'"));
        let theme = script
            .find("r.setAttribute('data-theme',effective)")
            .unwrap();
        let ready = script.find("data-ccresdoc-appearance-ready").unwrap();
        assert!(
            theme < ready,
            "theme must be installed before the ready marker"
        );
        assert!(script.contains("new MutationObserver"));
        assert!(!script.contains("invoke("));
    }

    #[test]
    fn document_start_script_never_imports_stored_preferences() {
        let script = initialization_script(&bootstrap_seed(
            &snapshot(LoadStatus::Missing),
            vec!["classic".into()],
        ));
        assert!(!script.contains("legacy"));
        assert!(!script.contains("localStorage.getItem(MODE_KEY)"));
        assert!(!script.contains("localStorage.getItem(PACK_KEY)"));
        assert!(!script.contains("localStorage.clear"));
        // Old keys are cleared only behind the retired-cache marker, and the
        // current values are written after that one-time retirement.
        let gate = script.find("localStorage.getItem(RETIRED)!==null").unwrap();
        let retire = script.find("localStorage.removeItem(RETIRED)").unwrap();
        let write = script.find("localStorage.setItem(CACHE").unwrap();
        assert!(gate < retire && retire < write);
        assert!(script.contains(&format!("var CACHE={CACHE_KEY:?}")));
        assert!(script.contains(&format!("var RETIRED={RETIRED_CACHE_KEY:?}")));
        assert!(script.contains("var FALLBACK=\"classic\""));
    }

    #[test]
    fn navigation_seed_is_assigned_before_location_replace() {
        let seed = bootstrap_seed(&snapshot(LoadStatus::Valid), vec!["classic".into()]);
        let script = window_name_script(&seed, "http://localhost:6000/docs/");
        assert!(
            script.find("window.name=").unwrap() < script.find("window.location.replace").unwrap()
        );
        assert!(script.contains("6000/docs/"));
        assert!(script.contains("ccresdoc-appearance-v2:"));
    }

    #[test]
    fn preview_clear_resolves_against_latest_snapshot() {
        let state = AppearanceState::default();
        let mut first = snapshot(LoadStatus::Valid);
        first.effective.appearance_mode = AppearanceMode::Light;
        state.set_preview(AppearanceValue {
            mode: AppearanceMode::Dark,
            theme_pack: "graphite".into(),
        });
        assert_eq!(state.envelope(&first).appearance.mode, AppearanceMode::Dark);
        let mut latest = first.clone();
        latest.effective.appearance_mode = AppearanceMode::System;
        let during_preview = state.envelope(&latest);
        assert_eq!(during_preview.appearance.mode, AppearanceMode::Dark);
        assert_eq!(during_preview.source, AppearanceSource::Preview);
        assert_eq!(during_preview.authoritative.mode, AppearanceMode::System);
        state.clear_preview();
        let cleared = state.envelope(&latest);
        assert_eq!(cleared.appearance.mode, AppearanceMode::System);
        assert_eq!(cleared.source, AppearanceSource::Authoritative);
    }

    #[test]
    fn appearance_command_requires_the_exact_active_docs_origin() {
        assert!(authorize_docs_url(&"http://localhost:6000/docs/".parse().unwrap(), 6000).is_ok());
        assert!(
            authorize_docs_url(&"http://127.0.0.1:6000/docs/x/".parse().unwrap(), 6000).is_ok()
        );
        for url in [
            "http://localhost:6001/docs/",
            "http://localhost:6000/",
            "https://localhost:6000/docs/",
            "http://example.com:6000/docs/",
        ] {
            assert_eq!(
                authorize_docs_url(&url.parse().unwrap(), 6000),
                Err(AppearanceRejection::ForbiddenOrigin),
                "{url}"
            );
        }
    }

    #[test]
    fn appearance_request_is_persist_only_and_rejects_retired_intents() {
        let valid: AppearanceRequest = serde_json::from_value(serde_json::json!({
            "mode": "dark", "themePack": "classic", "field": "mode"
        }))
        .unwrap();
        assert_eq!(valid.mode, AppearanceMode::Dark);
        for invalid in [
            serde_json::json!({ "mode": "sepia", "themePack": "classic", "field": "mode" }),
            serde_json::json!({ "mode": "dark", "themePack": "classic", "field": "mode", "preferredPort": 1 }),
            serde_json::json!({ "mode": "dark", "themePack": "classic", "field": "mode", "intent": "persist" }),
            serde_json::json!({ "mode": "dark", "themePack": "classic", "field": "mode", "intent": "legacy_candidate" }),
            serde_json::json!({ "claudeDir": "/tmp" }),
        ] {
            assert!(serde_json::from_value::<AppearanceRequest>(invalid).is_err());
        }
    }

    mod transitions {
        use super::*;

        const FIXTURE: &str = include_str!("../fixtures/appearance-transitions.json");

        struct Harness {
            _root: tempfile::TempDir,
            store: SettingsStore,
            appearance: AppearanceState,
        }

        fn config_toml(home: &Path, mode: &str, theme_pack: &str) -> String {
            format!(
                "schema_version = 2\n\n[resources]\nclaude = true\ncodex = false\n\n[source]\nclaude_dir = {:?}\ncodex_dir = \"~/.codex\"\n\n[appearance]\nmode = {mode:?}\ntheme_pack = {theme_pack:?}\n\n[server]\npreferred_port = 5000\nfallback_to_free_port = false\n",
                home.join(".claude").to_string_lossy()
            )
        }

        fn value(value: &Value) -> AppearanceValue {
            serde_json::from_value(value.clone()).unwrap()
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
                    let stored = value(&input["stored"]);
                    fs::write(
                        &path,
                        config_toml(&home, stored.mode.as_str(), &stored.theme_pack),
                    )
                    .unwrap();
                }
                LoadStatus::Invalid => {
                    fs::write(&path, config_toml(&home, "sepia", "classic")).unwrap();
                }
                LoadStatus::Malformed => {
                    fs::write(&path, "schema_version = 2\n[source\nnope").unwrap();
                }
                LoadStatus::UnsupportedVersion => {
                    fs::write(&path, "schema_version = 3\nfuture = true\n").unwrap();
                }
                LoadStatus::Unreadable => {
                    use std::os::unix::fs::PermissionsExt;
                    fs::write(&path, "schema_version = 2\n").unwrap();
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
                assert_eq!(value_from_snapshot(&loaded), value(&input["stored"]));
            } else {
                assert!(input["stored"].is_null());
            }

            let appearance = AppearanceState::default();
            if !input["preview"].is_null() {
                appearance.set_preview(value(&input["preview"]));
            }
            Harness {
                _root: root,
                store,
                appearance,
            }
        }

        /// The command's side-effect path with the Tauri runtime apply
        /// replaced by the event it emits. `apply_saved` emits
        /// `envelope(load())` after a successful save and the command returns
        /// that same emitted value, so one envelope stands for both.
        fn run(
            harness: &Harness,
            caller_url: &url::Url,
            effective_port: u16,
            request: AppearanceRequest,
        ) -> Result<AppearanceEnvelope, &'static str> {
            let update = resolve_appearance_update(caller_url, effective_port, request, |slug| {
                harness.store.supports_theme_pack(slug)
            })
            .map_err(|rejection| rejection.code())?;
            persist_appearance_update(&harness.store, &update).map_err(|error| error.code())?;
            Ok(harness.appearance.envelope(&harness.store.load()))
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

        fn assert_preview_kept(name: &str, harness: &Harness, preview: &Value) {
            assert_eq!(
                harness.appearance.preview(),
                (!preview.is_null()).then(|| value(preview)),
                "{name}: preview"
            );
        }

        #[test]
        fn update_appearance_matches_every_transition_row() {
            let fixture: Value = serde_json::from_str(FIXTURE).unwrap();
            assert_eq!(fixture["schemaVersion"], 2);
            let constants = &fixture["constants"];
            let effective_port =
                u16::try_from(constants["effectivePort"].as_u64().unwrap()).unwrap();
            let caller_path = constants["callerPath"].as_str().unwrap();
            let defaults = value(&constants["defaults"]);
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
                assert!(
                    input.get("candidate").is_none(),
                    "{name}: retired candidate input"
                );
                let parsed = serde_json::from_value::<AppearanceRequest>(row["request"].clone());
                if expect["error"] == "invalid_args" {
                    assert!(parsed.is_err(), "{name}: request must fail deserialization");
                    checked += 1;
                    continue;
                }
                let request = parsed.unwrap_or_else(|error| panic!("{name}: {error}"));

                let harness = harness(input);
                let before = harness.store.load();
                let caller_url: url::Url =
                    format!("{}{caller_path}", input["callerOrigin"].as_str().unwrap())
                        .parse()
                        .unwrap();
                let result = run(&harness, &caller_url, effective_port, request);

                if let Some(code) = expect["error"].as_str() {
                    let error = result
                        .err()
                        .unwrap_or_else(|| panic!("{name}: expected {code}"));
                    assert_eq!(error, code, "{name}: error code");
                    let after = harness.store.load();
                    assert_eq!(after.status, before.status, "{name}: status unchanged");
                    assert_eq!(
                        after.revision, before.revision,
                        "{name}: revision unchanged"
                    );
                    assert_eq!(
                        after.raw_content, before.raw_content,
                        "{name}: content unchanged"
                    );
                    assert_preview_kept(name, &harness, &input["preview"]);
                    checked += 1;
                    continue;
                }

                let envelope = result.unwrap_or_else(|error| panic!("{name}: unexpected {error}"));
                let expected = &expect["envelope"];
                assert_eq!(
                    envelope.appearance,
                    value(&expected["appearance"]),
                    "{name}: appearance"
                );
                assert_eq!(
                    envelope.authoritative,
                    value(&expected["authoritative"]),
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
                assert!(
                    expect["emitsEvent"].as_bool().unwrap(),
                    "{name}: a successful persist always emits"
                );
                assert_preview_kept(name, &harness, &input["preview"]);

                let after = harness.store.load();
                assert_eq!(after.status, LoadStatus::Valid, "{name}: storedAfter");
                assert_eq!(
                    value_from_snapshot(&after),
                    value(&expect["storedAfter"]),
                    "{name}: storedAfter"
                );
                checked += 1;
            }
            assert_eq!(checked, rows.len());
        }
    }
}
