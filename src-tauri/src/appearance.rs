//! Appearance authority, exact-origin bootstrap, preview, and event payloads.
//!
//! Only mode/theme cross this boundary. The document-start script is installed
//! once on the main WebView and consumes a fresh `window.name` seed immediately
//! before each docs navigation, so fallback ports and restarts do not inherit a
//! different loopback origin's cache.

use serde::{Deserialize, Serialize};
use std::sync::{Mutex, MutexGuard};

use crate::settings::{AppearanceMode, ContentRevision, LoadStatus, SettingsSnapshot};

pub const APPEARANCE_EVENT: &str = "ccresdoc://appearance";
pub const WINDOW_NAME_PREFIX: &str = "ccresdoc-appearance-v1:";
pub const CACHE_KEY: &str = "ccresdoc-appearance-v1";

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
    LegacyCandidate,
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LegacyCandidate {
    pub origin: String,
    pub appearance: AppearanceValue,
}

#[derive(Default)]
struct AppearanceStateInner {
    candidate: Option<LegacyCandidate>,
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

    pub fn report_candidate(&self, origin: String, appearance: AppearanceValue) {
        self.lock().candidate = Some(LegacyCandidate { origin, appearance });
    }

    pub fn clear_candidate(&self) {
        self.lock().candidate = None;
    }

    pub fn candidate_for(&self, origin: &str) -> Option<AppearanceValue> {
        self.lock()
            .candidate
            .as_ref()
            .filter(|candidate| candidate.origin == origin)
            .map(|candidate| candidate.appearance.clone())
    }

    pub fn candidate(&self) -> Option<AppearanceValue> {
        self.lock()
            .candidate
            .as_ref()
            .map(|candidate| candidate.appearance.clone())
    }

    pub fn set_preview(&self, appearance: AppearanceValue) {
        self.lock().preview = Some(appearance);
    }

    pub fn clear_preview(&self) {
        self.lock().preview = None;
    }

    pub fn envelope(&self, snapshot: &SettingsSnapshot) -> AppearanceEnvelope {
        let authoritative = value_from_snapshot(snapshot);
        let inner = self.lock();
        let preview = inner.preview.clone();
        let candidate = if snapshot.status == LoadStatus::Missing {
            inner
                .candidate
                .as_ref()
                .map(|value| value.appearance.clone())
        } else {
            None
        };
        let (appearance, source) = if let Some(preview) = preview {
            (preview, AppearanceSource::Preview)
        } else if let Some(candidate) = candidate {
            (candidate, AppearanceSource::LegacyCandidate)
        } else if snapshot.status == LoadStatus::Missing {
            (authoritative.clone(), AppearanceSource::Default)
        } else {
            (authoritative.clone(), AppearanceSource::Authoritative)
        };
        AppearanceEnvelope {
            appearance,
            authoritative,
            revision: snapshot.revision.clone(),
            source,
            authoritative_source: if snapshot.status == LoadStatus::Missing {
                AppearanceSource::Default
            } else {
                AppearanceSource::Authoritative
            },
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
        // Any existing TOML, including malformed/invalid TOML, blocks legacy
        // preference import. Its safe effective projection remains canonical.
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
/// it runs before page code and never invokes a privileged command.
pub fn initialization_script(initial: &BootstrapSeed) -> String {
    let initial = serde_json::to_string(initial).expect("appearance seed is serializable");
    format!(
        r#"(function(){{
var PREFIX={prefix};var CACHE={cache};var seed={initial};
function validSeed(v){{return v&&typeof v==='object'&&v.appearance&&['system','light','dark'].indexOf(v.appearance.mode)!==-1&&typeof v.appearance.themePack==='string'&&Array.isArray(v.availableThemePacks);}}
try{{if(typeof window.name==='string'&&window.name.indexOf(PREFIX)===0){{var fresh=JSON.parse(window.name.slice(PREFIX.length));if(validSeed(fresh))seed=fresh;}}}}catch(e){{}}
var loc=window.location;var docs=(loc.protocol==='http:'&&(loc.hostname==='localhost'||loc.hostname==='127.0.0.1')&&loc.pathname.indexOf('/docs/')===0);if(!docs||!validSeed(seed))return;
var mode=seed.appearance.mode;var pack=seed.appearance.themePack;var source=seed.authoritative?'authoritative':'default';
if(!seed.authoritative){{try{{var lm=localStorage.getItem('zudo-doc-theme');var lp=localStorage.getItem('zudo-doc-theme-pack');var found=false;if(lm==='light'||lm==='dark'){{mode=lm;found=true;}}if(typeof lp==='string'&&seed.availableThemePacks.indexOf(lp)!==-1){{pack=lp;found=true;}}if(found)source='legacy_candidate';}}catch(e){{}}}}
if(seed.availableThemePacks.indexOf(pack)===-1)pack='default';
var effective=mode==='system'?(window.matchMedia('(prefers-color-scheme: dark)').matches?'dark':'light'):mode;
if(seed.authoritative){{try{{if(mode==='system')localStorage.removeItem('zudo-doc-theme');else localStorage.setItem('zudo-doc-theme',mode);localStorage.setItem('zudo-doc-theme-pack',pack);localStorage.setItem(CACHE,JSON.stringify({{mode:mode,themePack:pack,revision:seed.revision}}));}}catch(e){{}}}}
function paint(r){{r.setAttribute('data-theme',effective);r.style.colorScheme=effective;r.setAttribute('data-theme-pack',pack);r.setAttribute('data-ccresdoc-appearance-ready','true');}}
if(document.documentElement)paint(document.documentElement);else{{var observer=new MutationObserver(function(){{if(document.documentElement){{paint(document.documentElement);observer.disconnect();}}}});observer.observe(document,{{childList:true}});}}
window.__CCRESDOC_APPEARANCE__={{mode:mode,themePack:pack,effectiveMode:effective,revision:seed.revision,source:source,origin:loc.origin}};
}})();"#,
        prefix = serde_json::to_string(WINDOW_NAME_PREFIX).unwrap(),
        cache = serde_json::to_string(CACHE_KEY).unwrap(),
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
    use std::path::PathBuf;

    fn snapshot(status: LoadStatus) -> SettingsSnapshot {
        let store = crate::settings::SettingsStore::with_theme_packs(
            PathBuf::from("/missing/config.toml"),
            PathBuf::from("/home"),
            ["default", "paper"],
        );
        let mut value = store.load();
        value.status = status;
        value.authored = SettingsDraft::defaults();
        value
    }

    #[test]
    fn authored_or_invalid_toml_blocks_legacy_while_missing_allows_it() {
        assert!(
            !bootstrap_seed(&snapshot(LoadStatus::Missing), vec!["default".into()]).authoritative
        );
        for status in [
            LoadStatus::Valid,
            LoadStatus::Invalid,
            LoadStatus::Malformed,
            LoadStatus::Unreadable,
        ] {
            assert!(bootstrap_seed(&snapshot(status), vec!["default".into()]).authoritative);
        }
    }

    #[test]
    fn document_start_script_is_exact_origin_and_applies_before_marking_ready() {
        let script = initialization_script(&bootstrap_seed(
            &snapshot(LoadStatus::Missing),
            vec!["default".into(), "paper".into()],
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
    fn navigation_seed_is_assigned_before_location_replace() {
        let seed = bootstrap_seed(&snapshot(LoadStatus::Valid), vec!["default".into()]);
        let script = window_name_script(&seed, "http://localhost:6000/docs/");
        assert!(
            script.find("window.name=").unwrap() < script.find("window.location.replace").unwrap()
        );
        assert!(script.contains("6000/docs/"));
    }

    #[test]
    fn preview_clear_resolves_against_latest_snapshot() {
        let state = AppearanceState::default();
        let mut first = snapshot(LoadStatus::Valid);
        first.effective.appearance_mode = AppearanceMode::Light;
        state.set_preview(AppearanceValue {
            mode: AppearanceMode::Dark,
            theme_pack: "paper".into(),
        });
        assert_eq!(state.envelope(&first).appearance.mode, AppearanceMode::Dark);
        let mut latest = first.clone();
        latest.effective.appearance_mode = AppearanceMode::System;
        let during_preview = state.envelope(&latest);
        assert_eq!(during_preview.appearance.mode, AppearanceMode::Dark);
        assert_eq!(during_preview.authoritative.mode, AppearanceMode::System);
        state.clear_preview();
        assert_eq!(
            state.envelope(&latest).appearance.mode,
            AppearanceMode::System
        );
    }
}
