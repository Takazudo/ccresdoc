//! Versioned, human-editable settings and a revision-guarded durable TOML store.
//!
//! This module intentionally has no Tauri dependencies. The host and settings
//! window can share the serializable domain types without coupling storage to
//! a window or application lifecycle.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::ffi::{OsStr, OsString};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use thiserror::Error;
use toml_edit::{value, Array, ArrayOfTables, DocumentMut, Item, Table, Value};

#[path = "settings/source_model.rs"]
mod source_model;
pub use source_model::{
    builtin_presets, DocumentSource, EffectiveSource, PresetRef, PublicFormat, RuleSnapshot,
    ScanRootChoice, SourceCatalog, UserPreset, BUILTIN_RULE_VERSION, MANAGEMENT_MOUNT,
};

pub const CURRENT_SCHEMA_VERSION: i64 = 3;
pub const MIGRATED_SCHEMA_VERSION: i64 = 2;
/// The only earlier schema the startup retirement recognizes.
pub const RETIRED_SCHEMA_VERSION: i64 = 1;
pub const DEFAULT_PORT: u16 = 4892;
pub const DEFAULT_THEME_PACK: &str = "classic";
pub const COMMAND_CATALOG_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandMenuMetadata {
    pub name: String,
    pub order: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserCommandMetadata {
    pub command_id: String,
    pub label: String,
    pub group: String,
    pub menu: CommandMenuMetadata,
    pub default_bindings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandCatalog {
    pub version: u32,
    pub commands: Vec<BrowserCommandMetadata>,
}

pub fn browser_command_catalog() -> CommandCatalog {
    let catalog: CommandCatalog = serde_json::from_str(include_str!(
        "../../app/src/browser-chrome/command-catalog.json"
    ))
    .expect("bundled browser command catalog must be valid JSON");
    assert_eq!(
        catalog.version, COMMAND_CATALOG_VERSION,
        "browser command catalog version must match the Rust consumer"
    );
    catalog
}

pub fn default_shortcut_entries() -> Vec<ShortcutEntry> {
    browser_command_catalog()
        .commands
        .into_iter()
        .map(|command| ShortcutEntry {
            command_id: command.command_id,
            bindings: command.default_bindings,
        })
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShortcutEntry {
    /// Command IDs are opaque data. In particular, serde must never apply case
    /// conversion to IDs containing underscores.
    pub command_id: String,
    pub bindings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NormalizedShortcut {
    modifiers: BTreeSet<ShortcutModifier>,
    key: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum ShortcutModifier {
    Mod,
    Ctrl,
    Alt,
    Shift,
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
#[error("{message}")]
pub struct ShortcutParseError {
    message: String,
}

impl NormalizedShortcut {
    pub fn as_portable_string(&self) -> String {
        let mut components = Vec::new();
        for modifier in [
            ShortcutModifier::Mod,
            ShortcutModifier::Ctrl,
            ShortcutModifier::Alt,
            ShortcutModifier::Shift,
        ] {
            if self.modifiers.contains(&modifier) {
                components.push(match modifier {
                    ShortcutModifier::Mod => "Mod",
                    ShortcutModifier::Ctrl => "Ctrl",
                    ShortcutModifier::Alt => "Alt",
                    ShortcutModifier::Shift => "Shift",
                });
            }
        }
        components.push(&self.key);
        components.join("+")
    }

    /// Tauri accepts `CmdOrCtrl` as its platform-neutral Command/Control
    /// modifier. Storage and frontend display continue to use neutral `Mod`.
    pub fn to_tauri_accelerator(&self) -> String {
        self.as_portable_string().replacen("Mod", "CmdOrCtrl", 1)
    }

    fn conflicts_with(&self, other: &Self) -> bool {
        if self.key != other.key {
            return false;
        }
        let exact = self.modifiers == other.modifiers;
        let non_macos = resolved_modifiers(&self.modifiers, false)
            == resolved_modifiers(&other.modifiers, false);
        let macos =
            resolved_modifiers(&self.modifiers, true) == resolved_modifiers(&other.modifiers, true);
        exact || non_macos || macos
    }
}

fn resolved_modifiers(
    modifiers: &BTreeSet<ShortcutModifier>,
    macos: bool,
) -> BTreeSet<&'static str> {
    modifiers
        .iter()
        .map(|modifier| match modifier {
            ShortcutModifier::Mod if macos => "cmd",
            ShortcutModifier::Mod | ShortcutModifier::Ctrl => "ctrl",
            ShortcutModifier::Alt => "alt",
            ShortcutModifier::Shift => "shift",
        })
        .collect()
}

pub fn normalize_shortcut_binding(raw: &str) -> Result<NormalizedShortcut, ShortcutParseError> {
    if raw.is_empty() || raw.trim() != raw || raw.chars().any(char::is_whitespace) {
        return Err(shortcut_parse_error(
            "binding must be one shortcut without surrounding whitespace or a chord",
        ));
    }
    let components = raw.split('+').collect::<Vec<_>>();
    if components.is_empty() || components.iter().any(|part| part.is_empty()) {
        return Err(shortcut_parse_error(
            "binding must contain one key and valid modifiers",
        ));
    }
    let (raw_key, raw_modifiers) = components
        .split_last()
        .expect("non-empty components checked above");
    let mut modifiers = BTreeSet::new();
    for raw_modifier in raw_modifiers {
        let modifier = match raw_modifier.to_ascii_lowercase().as_str() {
            "mod" => ShortcutModifier::Mod,
            "ctrl" | "control" => ShortcutModifier::Ctrl,
            "alt" | "option" => ShortcutModifier::Alt,
            "shift" => ShortcutModifier::Shift,
            _ => {
                return Err(shortcut_parse_error(format!(
                    "unknown modifier '{raw_modifier}'; use Mod, Ctrl, Alt, or Shift"
                )))
            }
        };
        if !modifiers.insert(modifier) {
            return Err(shortcut_parse_error(format!(
                "modifier '{raw_modifier}' appears more than once"
            )));
        }
    }
    if modifiers.contains(&ShortcutModifier::Mod) && modifiers.contains(&ShortcutModifier::Ctrl) {
        return Err(shortcut_parse_error(
            "Mod and Ctrl cannot be combined because they are the same modifier off macOS",
        ));
    }
    let key = normalize_shortcut_key(raw_key)?;
    if modifiers.is_empty() && (raw_key.chars().count() == 1 || key == "Space") {
        return Err(shortcut_parse_error(
            "bare printable keys are not supported; add a modifier",
        ));
    }
    Ok(NormalizedShortcut { modifiers, key })
}

fn normalize_shortcut_key(raw: &str) -> Result<String, ShortcutParseError> {
    if raw.chars().count() == 1 {
        let character = raw.chars().next().expect("one character");
        if character.is_ascii_alphabetic() {
            return Ok(character.to_ascii_uppercase().to_string());
        }
        if character.is_ascii_digit() || ",./;'[]\\-=`".contains(character) {
            return Ok(character.to_string());
        }
        return Err(shortcut_parse_error(format!("unsupported key '{raw}'")));
    }
    let lower = raw.to_ascii_lowercase();
    let normalized = match lower.as_str() {
        "escape" | "esc" => "Escape".into(),
        "enter" | "return" => "Enter".into(),
        "tab" => "Tab".into(),
        "space" => "Space".into(),
        "backspace" => "Backspace".into(),
        "delete" | "del" => "Delete".into(),
        "insert" => "Insert".into(),
        "home" => "Home".into(),
        "end" => "End".into(),
        "pageup" => "PageUp".into(),
        "pagedown" => "PageDown".into(),
        "arrowup" | "up" => "ArrowUp".into(),
        "arrowdown" | "down" => "ArrowDown".into(),
        "arrowleft" | "left" => "ArrowLeft".into(),
        "arrowright" | "right" => "ArrowRight".into(),
        _ if lower
            .strip_prefix('f')
            .and_then(|number| number.parse::<u8>().ok())
            .is_some_and(|number| (1..=24).contains(&number)) =>
        {
            lower.to_ascii_uppercase()
        }
        _ => return Err(shortcut_parse_error(format!("unsupported key '{raw}'"))),
    };
    Ok(normalized)
}

fn shortcut_parse_error(message: impl Into<String>) -> ShortcutParseError {
    ShortcutParseError {
        message: message.into(),
    }
}

pub fn bundled_theme_pack_slugs() -> Vec<String> {
    serde_json::from_str(include_str!("../../app/src/config/theme-pack-slugs.json"))
        .expect("bundled theme-pack slug catalog must be valid JSON")
}

static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigEnvironment {
    pub override_path: Option<OsString>,
    pub xdg_config_home: Option<OsString>,
    pub home: Option<OsString>,
}

impl ConfigEnvironment {
    pub fn from_process() -> Self {
        Self {
            override_path: std::env::var_os("CCRESDOC_CONFIG"),
            xdg_config_home: std::env::var_os("XDG_CONFIG_HOME"),
            home: std::env::var_os("HOME"),
        }
    }
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum ConfigPathError {
    #[error("CCRESDOC_CONFIG is set but empty")]
    EmptyOverride,
    #[error("XDG_CONFIG_HOME is set but empty")]
    EmptyXdgConfigHome,
    #[error("HOME is missing or empty")]
    MissingHome,
}

pub fn resolve_config_path() -> Result<PathBuf, ConfigPathError> {
    resolve_config_path_from(&ConfigEnvironment::from_process())
}

pub fn resolve_config_path_from(env: &ConfigEnvironment) -> Result<PathBuf, ConfigPathError> {
    if let Some(path) = env.override_path.as_deref() {
        if path.is_empty() {
            return Err(ConfigPathError::EmptyOverride);
        }
        return Ok(PathBuf::from(path));
    }
    if let Some(root) = env.xdg_config_home.as_deref() {
        if root.is_empty() {
            return Err(ConfigPathError::EmptyXdgConfigHome);
        }
        return Ok(PathBuf::from(root).join("ccresdoc/config.toml"));
    }
    let home = env
        .home
        .as_deref()
        .filter(|home| !home.is_empty())
        .ok_or(ConfigPathError::MissingHome)?;
    Ok(PathBuf::from(home).join(".config/ccresdoc/config.toml"))
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AppearanceMode {
    System,
    Light,
    Dark,
}

impl AppearanceMode {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "system" => Some(Self::System),
            "light" => Some(Self::Light),
            "dark" => Some(Self::Dark),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::System => "system",
            Self::Light => "light",
            Self::Dark => "dark",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SettingsDraft {
    pub schema_version: i64,
    pub claude_resources: bool,
    pub codex_resources: bool,
    pub claude_dir: String,
    pub codex_dir: String,
    pub appearance_mode: String,
    pub theme_pack: String,
    pub preferred_port: i64,
    pub fallback_to_free_port: bool,
    #[serde(default = "default_shortcut_entries")]
    pub shortcuts: Vec<ShortcutEntry>,
}

impl SettingsDraft {
    pub fn defaults() -> Self {
        Self {
            schema_version: CURRENT_SCHEMA_VERSION,
            claude_resources: true,
            codex_resources: false,
            claude_dir: "~/.claude".into(),
            codex_dir: "~/.codex".into(),
            appearance_mode: "system".into(),
            theme_pack: DEFAULT_THEME_PACK.into(),
            preferred_port: i64::from(DEFAULT_PORT),
            fallback_to_free_port: true,
            shortcuts: default_shortcut_entries(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EffectiveSettings {
    pub claude_resources: bool,
    pub codex_resources: bool,
    pub claude_dir: Option<PathBuf>,
    pub codex_dir: Option<PathBuf>,
    pub appearance_mode: AppearanceMode,
    pub theme_pack: String,
    pub preferred_port: u16,
    pub effective_port: u16,
    pub fallback_to_free_port: bool,
    pub shortcuts: Vec<ShortcutEntry>,
    #[serde(default)]
    pub sources: Vec<EffectiveSource>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActiveState {
    pub uses_authored_settings: bool,
    pub source_is_authored: bool,
    pub preferred_port: u16,
    pub effective_port: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LoadStatus {
    Missing,
    Valid,
    Invalid,
    Unreadable,
    Malformed,
    UnsupportedVersion,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticKind {
    Unreadable,
    MalformedSyntax,
    UnsupportedSchemaVersion,
    InvalidType,
    InvalidAppearanceMode,
    InvalidPort,
    InvalidSourcePath,
    UnreadableSourcePath,
    ThemePackUnavailable,
    InvalidShortcut,
    ShortcutConflict,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceLocation {
    pub line: usize,
    pub column: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SettingsDiagnostic {
    pub kind: DiagnosticKind,
    pub field: Option<String>,
    pub message: String,
    pub blocking: bool,
    pub location: Option<SourceLocation>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ContentRevision(pub String);

impl ContentRevision {
    pub fn from_bytes(bytes: &[u8]) -> Self {
        let digest = Sha256::digest(bytes);
        Self(format!("sha256:{digest:x}"))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SettingsSnapshot {
    pub config_path: PathBuf,
    pub file_exists: bool,
    pub status: LoadStatus,
    pub revision: Option<ContentRevision>,
    /// Lossless for every valid TOML file. Invalid UTF-8 is represented with
    /// replacement characters here, while its revision still hashes exact bytes.
    pub raw_content: Option<String>,
    pub authored: SettingsDraft,
    pub source_catalog: SourceCatalog,
    pub effective: EffectiveSettings,
    pub active: ActiveState,
    pub validation: Vec<SettingsDiagnostic>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SettingField {
    ClaudeResources,
    CodexResources,
    ClaudeDir,
    CodexDir,
    AppearanceMode,
    ThemePack,
    PreferredPort,
    FallbackToFreePort,
    Shortcuts,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApplyImpact {
    None,
    AppearanceOnly,
    RestartRuntime,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SaveResult {
    pub snapshot: SettingsSnapshot,
    pub impact: ApplyImpact,
    pub rebased: bool,
}

#[derive(Debug, Error)]
pub enum SaveError {
    #[error("settings changed since they were loaded")]
    RevisionConflict {
        expected: Option<ContentRevision>,
        actual: Option<ContentRevision>,
    },
    #[error("settings TOML is malformed; explicit replacement is required")]
    Malformed,
    #[error("schema version {0} is unsupported and cannot be overwritten")]
    UnsupportedVersion(i64),
    #[error("settings file is unreadable: {0}")]
    Unreadable(String),
    #[error("settings are invalid")]
    Validation(Vec<SettingsDiagnostic>),
    #[error("confirmed rebase requires a stale revision")]
    NotStale,
    #[error("the latest settings are not valid and cannot be rebased")]
    LatestNotValid,
    #[error("explicit replacement is only allowed for malformed TOML")]
    ReplacementNotAllowed,
    #[error("I/O error while writing settings: {0}")]
    Io(#[from] io::Error),
}

impl SaveError {
    /// Stable wire code shared by the command boundary and its parity tests.
    pub fn code(&self) -> &'static str {
        match self {
            Self::RevisionConflict { .. } => "revision_conflict",
            Self::Malformed => "malformed",
            Self::UnsupportedVersion(_) => "unsupported_version",
            Self::Unreadable(_) => "unreadable",
            Self::Validation(_) => "validation",
            Self::NotStale => "not_stale",
            Self::LatestNotValid => "latest_not_valid",
            Self::ReplacementNotAllowed => "replacement_not_allowed",
            Self::Io(_) => "io",
        }
    }
}

/// Result of the explicit startup retirement of a schema-v1 config.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum V1Retirement {
    /// No file exists at the active config path.
    Absent,
    /// The file is not a positively recognized schema-v1 CCResDoc config
    /// (current, future, malformed, unreadable or foreign); it is untouched.
    NotRecognized,
    /// The file changed between recognition and removal; it was restored.
    Changed,
    /// The recognized file was removed. Nothing is written in its place.
    Retired(ContentRevision),
}

const V1_SECTIONS: [&str; 5] = ["resources", "source", "appearance", "server", "shortcuts"];

fn recognized_v1(bytes: &[u8]) -> bool {
    let Ok(text) = std::str::from_utf8(bytes) else {
        return false;
    };
    let Ok(doc) = text.parse::<DocumentMut>() else {
        return false;
    };
    if integer_at(&doc, &["schema_version"]) != Some(RETIRED_SCHEMA_VERSION) {
        return false;
    }
    let recognized = doc.iter().all(|(key, item)| {
        key == "schema_version" || (V1_SECTIONS.contains(&key) && item.is_table_like())
    });
    recognized
}

#[derive(Debug)]
enum ReadState {
    Missing,
    Present {
        bytes: Vec<u8>,
        doc: Result<Box<DocumentMut>, ParseFailure>,
    },
    Unreadable(io::Error),
}

#[derive(Debug)]
struct ParseFailure {
    message: String,
    offset: Option<usize>,
}

pub struct SettingsStore {
    path: PathBuf,
    home: PathBuf,
    available_theme_packs: BTreeSet<String>,
    #[cfg(test)]
    before_replace: Option<BeforeReplaceHook>,
}

#[cfg(test)]
type BeforeReplaceHook = std::sync::Arc<dyn Fn(&Path) + Send + Sync>;

impl SettingsStore {
    pub fn new(path: PathBuf, home: PathBuf) -> Self {
        Self::with_theme_packs(path, home, [DEFAULT_THEME_PACK])
    }

    pub fn with_theme_packs<I, S>(path: PathBuf, home: PathBuf, packs: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let mut available_theme_packs = packs.into_iter().map(Into::into).collect::<BTreeSet<_>>();
        available_theme_packs.insert(DEFAULT_THEME_PACK.into());
        Self {
            path,
            home,
            available_theme_packs,
            #[cfg(test)]
            before_replace: None,
        }
    }

    #[cfg(test)]
    fn with_before_replace(mut self, hook: impl Fn(&Path) + Send + Sync + 'static) -> Self {
        self.before_replace = Some(std::sync::Arc::new(hook));
        self
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn available_theme_packs(&self) -> Vec<String> {
        self.available_theme_packs.iter().cloned().collect()
    }

    pub fn supports_theme_pack(&self, slug: &str) -> bool {
        self.available_theme_packs.contains(slug)
    }

    pub fn load(&self) -> SettingsSnapshot {
        match self.read_state() {
            ReadState::Missing => {
                self.default_snapshot(LoadStatus::Missing, false, None, None, vec![])
            }
            ReadState::Unreadable(error) => self.default_snapshot(
                LoadStatus::Unreadable,
                true,
                None,
                None,
                vec![diagnostic(
                    DiagnosticKind::Unreadable,
                    None,
                    error.to_string(),
                    true,
                )],
            ),
            ReadState::Present {
                bytes,
                doc: Err(error),
            } => {
                let location = error.offset.map(|offset| byte_location(&bytes, offset));
                self.default_snapshot(
                    LoadStatus::Malformed,
                    true,
                    Some(ContentRevision::from_bytes(&bytes)),
                    Some(String::from_utf8_lossy(&bytes).into_owned()),
                    vec![SettingsDiagnostic {
                        kind: DiagnosticKind::MalformedSyntax,
                        field: None,
                        message: error.message,
                        blocking: true,
                        location,
                    }],
                )
            }
            ReadState::Present {
                bytes,
                doc: Ok(doc),
            } => self.snapshot_from_doc(bytes, &doc),
        }
    }

    /// Merge only appearance fields into the freshest readable document.
    /// The caller serializes this with Settings Save; the bounded retry also
    /// protects against an editor replacing the file between read and rename.
    pub fn update_appearance(
        &self,
        mode: Option<AppearanceMode>,
        theme_pack: Option<&str>,
    ) -> Result<SaveResult, SaveError> {
        if mode.is_none() && theme_pack.is_none() {
            return Err(SaveError::Validation(vec![diagnostic(
                DiagnosticKind::InvalidAppearanceMode,
                Some("appearance"),
                "an appearance field is required".into(),
                true,
            )]));
        }
        if theme_pack.is_some_and(|slug| !self.supports_theme_pack(slug)) {
            let slug = theme_pack.expect("checked as some");
            return Err(SaveError::Validation(vec![diagnostic(
                DiagnosticKind::ThemePackUnavailable,
                Some("appearance.theme_pack"),
                format!("theme pack '{slug}' is unavailable"),
                true,
            )]));
        }
        let mut last_conflict = None;
        for _ in 0..4 {
            let latest = self.load();
            match latest.status {
                LoadStatus::Missing | LoadStatus::Valid => {}
                LoadStatus::Malformed => return Err(SaveError::Malformed),
                LoadStatus::UnsupportedVersion => {
                    return Err(SaveError::UnsupportedVersion(
                        latest.authored.schema_version,
                    ))
                }
                LoadStatus::Unreadable => {
                    return Err(SaveError::Unreadable("config cannot be read".into()))
                }
                LoadStatus::Invalid => return Err(SaveError::LatestNotValid),
            }
            let mut draft = latest.authored.clone();
            if let Some(mode) = &mode {
                draft.appearance_mode = mode.as_str().into();
            }
            if let Some(theme_pack) = theme_pack {
                draft.theme_pack = theme_pack.into();
            }
            let fields = [SettingField::AppearanceMode, SettingField::ThemePack]
                .into_iter()
                .collect();
            match self.save_app_fields(&draft, &fields, latest.revision.as_ref()) {
                Ok(result) => return Ok(result),
                Err(error @ SaveError::RevisionConflict { .. }) => last_conflict = Some(error),
                Err(error) => return Err(error),
            }
        }
        Err(last_conflict.expect("a retry exits only after revision conflicts"))
    }

    /// Validate and normalize a draft without touching the settings file.
    /// The native command boundary uses this so Rust remains authoritative for
    /// source-path, port, appearance, and theme-pack validation.
    pub fn validate(&self, draft: &SettingsDraft) -> (EffectiveSettings, Vec<SettingsDiagnostic>) {
        let (effective, mut diagnostics) = self.validate_and_project(draft);
        if draft.schema_version != CURRENT_SCHEMA_VERSION {
            diagnostics.insert(
                0,
                diagnostic(
                    DiagnosticKind::UnsupportedSchemaVersion,
                    Some("schema_version"),
                    format!("schema version {} is unsupported", draft.schema_version),
                    true,
                ),
            );
        }
        (effective, diagnostics)
    }

    pub fn save(
        &self,
        draft: &SettingsDraft,
        expected_revision: Option<&ContentRevision>,
    ) -> Result<SaveResult, SaveError> {
        let before = self.load();
        self.ensure_revision(expected_revision, before.revision.as_ref())?;
        match before.status {
            LoadStatus::Malformed => return Err(SaveError::Malformed),
            LoadStatus::UnsupportedVersion => {
                return Err(SaveError::UnsupportedVersion(schema_from_raw(
                    &before.raw_content,
                )))
            }
            LoadStatus::Unreadable => {
                return Err(SaveError::Unreadable(first_message(&before.validation)))
            }
            _ => {}
        }
        let draft = self.normalized_draft(draft)?;

        let mut doc = match self.read_state() {
            ReadState::Missing => DocumentMut::new(),
            ReadState::Present { doc: Ok(doc), .. } => *doc,
            ReadState::Present { doc: Err(_), .. } => return Err(SaveError::Malformed),
            ReadState::Unreadable(error) => return Err(SaveError::Unreadable(error.to_string())),
        };
        merge_fields(&mut doc, &draft, &SettingField::all());
        if schema_from_raw(&before.raw_content) == MIGRATED_SCHEMA_VERSION
            || before.status == LoadStatus::Missing
        {
            merge_source_catalog(&mut doc, &catalog_from_legacy(&draft));
        }
        let impact = impact_between(&before.authored, &draft, &SettingField::all());
        self.write_document(&doc, expected_revision)?;
        Ok(SaveResult {
            snapshot: self.load(),
            impact,
            rebased: false,
        })
    }

    pub fn rebase_dirty(
        &self,
        draft: &SettingsDraft,
        dirty_fields: &BTreeSet<SettingField>,
        stale_revision: &ContentRevision,
    ) -> Result<SaveResult, SaveError> {
        let latest = self.load();
        if latest.revision.as_ref() == Some(stale_revision) {
            return Err(SaveError::NotStale);
        }
        if latest.status != LoadStatus::Valid {
            return Err(SaveError::LatestNotValid);
        }
        match self.read_state() {
            ReadState::Present {
                bytes,
                doc: Ok(mut doc),
            } => {
                let mut candidate = latest.authored.clone();
                copy_dirty(&mut candidate, draft, dirty_fields);
                let candidate = self.normalized_draft(&candidate)?;
                let impact = impact_between(&latest.authored, &candidate, dirty_fields);
                merge_fields(&mut doc, &candidate, dirty_fields);
                if schema_from_raw(&latest.raw_content) == MIGRATED_SCHEMA_VERSION {
                    merge_source_catalog(&mut doc, &catalog_from_legacy(&candidate));
                }
                let latest_revision = ContentRevision::from_bytes(&bytes);
                self.write_document(&doc, Some(&latest_revision))?;
                Ok(SaveResult {
                    snapshot: self.load(),
                    impact,
                    rebased: true,
                })
            }
            _ => Err(SaveError::LatestNotValid),
        }
    }

    pub fn replace_malformed(
        &self,
        draft: &SettingsDraft,
        expected_revision: &ContentRevision,
    ) -> Result<SaveResult, SaveError> {
        let before = self.load();
        self.ensure_revision(Some(expected_revision), before.revision.as_ref())?;
        if before.status != LoadStatus::Malformed {
            return Err(SaveError::ReplacementNotAllowed);
        }
        let draft = self.normalized_draft(draft)?;
        let mut doc = DocumentMut::new();
        merge_fields(&mut doc, &draft, &SettingField::all());
        merge_source_catalog(&mut doc, &SourceCatalog::defaults());
        self.write_document(&doc, Some(expected_revision))?;
        Ok(SaveResult {
            snapshot: self.load(),
            impact: ApplyImpact::RestartRuntime,
            rebased: false,
        })
    }

    /// Persist a recognized v2 document as v3 without changing any authored
    /// app fields, comments or unknown keys. A second call is a no-op.
    pub fn migrate_v2(&self) -> Result<SettingsSnapshot, SaveError> {
        let before = self.load();
        if schema_from_raw(&before.raw_content) != MIGRATED_SCHEMA_VERSION {
            return Ok(before);
        }
        // A v2 source may be temporarily absent. Preserve its authored path
        // during migration so the document manager can repair it on first
        // launch; malformed fields and unrelated invalid app settings still
        // keep the old bytes protected.
        if before.status != LoadStatus::Valid
            && !before.validation.iter().all(|diagnostic| {
                !diagnostic.blocking
                    || matches!(
                        diagnostic.kind,
                        DiagnosticKind::InvalidSourcePath | DiagnosticKind::UnreadableSourcePath
                    )
            })
        {
            return Err(SaveError::LatestNotValid);
        }
        let Some(revision) = before.revision.as_ref() else {
            return Err(SaveError::Unreadable("v2 config has no revision".into()));
        };
        let mut doc = match self.read_state() {
            ReadState::Present { doc: Ok(doc), .. } => *doc,
            ReadState::Present { doc: Err(_), .. } => return Err(SaveError::Malformed),
            _ => return Err(SaveError::Unreadable("v2 config cannot be read".into())),
        };
        set_value_preserving_decor(&mut doc["schema_version"], value(CURRENT_SCHEMA_VERSION));
        merge_source_catalog(&mut doc, &before.source_catalog);
        self.write_document(&doc, Some(revision))?;
        Ok(self.load())
    }

    /// Native Settings writes only the fields its own form owns. Source and
    /// preset arrays are copied from the freshest revision, never from a stale
    /// whole-settings draft.
    pub fn save_app_fields(
        &self,
        draft: &SettingsDraft,
        fields: &BTreeSet<SettingField>,
        expected_revision: Option<&ContentRevision>,
    ) -> Result<SaveResult, SaveError> {
        if fields.iter().any(|field| {
            matches!(
                field,
                SettingField::ClaudeResources
                    | SettingField::CodexResources
                    | SettingField::ClaudeDir
                    | SettingField::CodexDir
            )
        }) {
            return Err(SaveError::Validation(vec![diagnostic(
                DiagnosticKind::InvalidType,
                Some("resources"),
                "source fields must use the source catalog transaction".into(),
                true,
            )]));
        }
        let before = self.load();
        self.ensure_revision(expected_revision, before.revision.as_ref())?;
        self.ensure_writable_status(&before)?;
        if before.validation.iter().any(|diagnostic| {
            diagnostic.blocking
                && diagnostic.kind == DiagnosticKind::InvalidType
                && diagnostic.field.as_deref().is_some_and(|field| {
                    field.starts_with("document_sources") || field.starts_with("user_presets")
                })
        }) {
            return Err(SaveError::LatestNotValid);
        }
        let mut candidate = before.authored.clone();
        copy_dirty(&mut candidate, draft, fields);
        let diagnostics = self.validate_app_fields(&candidate);
        let blocking = diagnostics
            .into_iter()
            .filter(|d| d.blocking)
            .collect::<Vec<_>>();
        if !blocking.is_empty() {
            return Err(SaveError::Validation(blocking));
        }
        let (effective, _) = self.validate_and_project(&candidate);
        candidate.shortcuts = effective.shortcuts;
        let impact = impact_between(&before.authored, &candidate, fields);
        let mut doc = self.read_document_for_write()?;
        let write_fields = if before.status == LoadStatus::Missing {
            SettingField::app_fields()
        } else {
            fields.clone()
        };
        merge_fields(&mut doc, &candidate, &write_fields);
        if schema_from_raw(&before.raw_content) == MIGRATED_SCHEMA_VERSION {
            merge_source_catalog(&mut doc, &catalog_from_legacy(&candidate));
        } else if before.status == LoadStatus::Missing {
            merge_source_catalog(&mut doc, &before.source_catalog);
        }
        self.write_document(&doc, expected_revision)?;
        Ok(SaveResult {
            snapshot: self.load(),
            impact,
            rebased: false,
        })
    }

    pub fn validate_source_catalog(&self, catalog: &SourceCatalog) -> Vec<SettingsDiagnostic> {
        self.project_source_catalog(catalog, true).1
    }

    pub fn validate_app_fields(&self, draft: &SettingsDraft) -> Vec<SettingsDiagnostic> {
        let (_, mut diagnostics) = self.validate_and_project(draft);
        diagnostics.retain(|diagnostic| {
            !diagnostic
                .field
                .as_deref()
                .is_some_and(|field| field.starts_with("source."))
        });
        if draft.schema_version != CURRENT_SCHEMA_VERSION {
            diagnostics.push(diagnostic(
                DiagnosticKind::UnsupportedSchemaVersion,
                Some("schema_version"),
                format!("schema version {} is unsupported", draft.schema_version),
                true,
            ));
        }
        diagnostics
    }

    fn project_source_catalog(
        &self,
        catalog: &SourceCatalog,
        strict_paths: bool,
    ) -> (Vec<EffectiveSource>, Vec<SettingsDiagnostic>) {
        let mut diagnostics = Vec::new();
        let mut ids = BTreeSet::new();
        let mut mounts = BTreeSet::new();
        let mut result = Vec::new();
        for source in &catalog.sources {
            let field = format!("sources.{}", source.id);
            if !valid_identity(&source.id) || !ids.insert(source.id.clone()) {
                diagnostics.push(diagnostic(
                    DiagnosticKind::InvalidType,
                    Some(&field),
                    "source ID must be unique and contain letters, digits, underscores or hyphens"
                        .into(),
                    true,
                ));
            }
            if source.label.trim().is_empty() {
                diagnostics.push(diagnostic(
                    DiagnosticKind::InvalidType,
                    Some(&format!("{field}.label")),
                    "source label is required".into(),
                    true,
                ));
            }
            if !valid_mount(&source.mount) || !mounts.insert(source.mount.clone()) {
                diagnostics.push(diagnostic(
                    DiagnosticKind::InvalidType,
                    Some(&format!("{field}.mount")),
                    "mount must be a unique lowercase route segment outside app-owned namespaces"
                        .into(),
                    true,
                ));
            }
            if let RuleSnapshot::Custom {
                include, exclude, ..
            } = &source.rules
            {
                for (kind, patterns) in [("include", include), ("exclude", exclude)] {
                    for pattern in patterns {
                        if pattern.trim().is_empty()
                            || pattern.starts_with('/')
                            || pattern.contains("..")
                            || pattern.contains('\\')
                        {
                            diagnostics.push(diagnostic(
                                DiagnosticKind::InvalidType,
                                Some(&format!("{field}.rules.{kind}")),
                                "patterns must be relative and cannot traverse folders".into(),
                                true,
                            ));
                        }
                    }
                }
                if include.is_empty() {
                    diagnostics.push(diagnostic(
                        DiagnosticKind::InvalidType,
                        Some(&format!("{field}.rules.include")),
                        "at least one include pattern is required".into(),
                        true,
                    ));
                }
            }
            if !source.enabled {
                continue;
            }
            let Some(root) = self.project_catalog_path(
                &source.root,
                strict_paths,
                &format!("{field}.root"),
                &mut diagnostics,
            ) else {
                continue;
            };
            let scan_root = match &source.scan_root {
                ScanRootChoice::Same => root.clone(),
                ScanRootChoice::Separate { path } => {
                    let Some(scan_root) = self.project_catalog_path(
                        path,
                        strict_paths,
                        &format!("{field}.scan_root"),
                        &mut diagnostics,
                    ) else {
                        continue;
                    };
                    scan_root
                }
            };
            result.push(EffectiveSource {
                id: source.id.clone(),
                label: source.label.clone(),
                root,
                scan_root,
                mount: source.mount.clone(),
                rules: source.rules.clone(),
            });
        }
        let mut preset_ids = BTreeSet::new();
        for preset in &catalog.presets {
            if !valid_identity(&preset.id)
                || preset.id.starts_with("builtin-")
                || !preset_ids.insert(preset.id.clone())
                || preset.name.trim().is_empty()
                || preset.version == 0
            {
                diagnostics.push(diagnostic(
                    DiagnosticKind::InvalidType,
                    Some(&format!("presets.{}", preset.id)),
                    "preset needs a unique non-built-in ID, name and positive version".into(),
                    true,
                ));
            }
        }
        (result, diagnostics)
    }

    fn project_catalog_path(
        &self,
        raw: &str,
        strict: bool,
        field: &str,
        diagnostics: &mut Vec<SettingsDiagnostic>,
    ) -> Option<PathBuf> {
        match normalize_source(raw, &self.home) {
            Ok(path) => Some(path),
            Err((kind, message)) if !strict && path_is_missing(raw, &self.home) => {
                let path = expand_source_path(raw, &self.home)?;
                diagnostics.push(diagnostic(kind, Some(field), message, false));
                Some(path)
            }
            Err((kind, message)) => {
                diagnostics.push(diagnostic(kind, Some(field), message, true));
                None
            }
        }
    }

    pub fn save_source_catalog(
        &self,
        catalog: &SourceCatalog,
        expected_revision: Option<&ContentRevision>,
    ) -> Result<SaveResult, SaveError> {
        let before = self.load();
        self.ensure_revision(expected_revision, before.revision.as_ref())?;
        self.ensure_writable_status(&before)?;
        if before.validation.iter().any(|diagnostic| {
            diagnostic.blocking
                && diagnostic.kind == DiagnosticKind::InvalidType
                && diagnostic.field.as_deref().is_some_and(|field| {
                    field.starts_with("document_sources") || field.starts_with("user_presets")
                })
        }) {
            return Err(SaveError::LatestNotValid);
        }
        let diagnostics = self.validate_source_catalog(catalog);
        let blocking = diagnostics
            .into_iter()
            .filter(|d| d.blocking)
            .collect::<Vec<_>>();
        if !blocking.is_empty() {
            return Err(SaveError::Validation(blocking));
        }
        let mut doc = self.read_document_for_write()?;
        merge_source_catalog(&mut doc, catalog);
        set_value_preserving_decor(&mut doc["schema_version"], value(CURRENT_SCHEMA_VERSION));
        self.write_document(&doc, expected_revision)?;
        Ok(SaveResult {
            snapshot: self.load(),
            impact: if before.source_catalog == *catalog {
                ApplyImpact::None
            } else {
                ApplyImpact::RestartRuntime
            },
            rebased: false,
        })
    }

    pub fn rebase_source_catalog(
        &self,
        draft: &SourceCatalog,
        base: &SourceCatalog,
        stale_revision: &ContentRevision,
    ) -> Result<SaveResult, SaveError> {
        let latest = self.load();
        if latest.revision.as_ref() == Some(stale_revision) {
            return Err(SaveError::NotStale);
        }
        if latest.status != LoadStatus::Valid {
            return Err(SaveError::LatestNotValid);
        }
        let merged =
            merge_catalog_three_way(base, draft, &latest.source_catalog).ok_or_else(|| {
                SaveError::RevisionConflict {
                    expected: Some(stale_revision.clone()),
                    actual: latest.revision.clone(),
                }
            })?;
        let mut saved = self.save_source_catalog(&merged, latest.revision.as_ref())?;
        saved.rebased = true;
        Ok(saved)
    }

    fn ensure_writable_status(&self, snapshot: &SettingsSnapshot) -> Result<(), SaveError> {
        match snapshot.status {
            LoadStatus::Missing | LoadStatus::Valid | LoadStatus::Invalid => Ok(()),
            LoadStatus::Malformed => Err(SaveError::Malformed),
            LoadStatus::UnsupportedVersion => Err(SaveError::UnsupportedVersion(schema_from_raw(
                &snapshot.raw_content,
            ))),
            LoadStatus::Unreadable => {
                Err(SaveError::Unreadable(first_message(&snapshot.validation)))
            }
        }
    }

    fn read_document_for_write(&self) -> Result<DocumentMut, SaveError> {
        match self.read_state() {
            ReadState::Missing => Ok(DocumentMut::new()),
            ReadState::Present { doc: Ok(doc), .. } => Ok(*doc),
            ReadState::Present { doc: Err(_), .. } => Err(SaveError::Malformed),
            ReadState::Unreadable(error) => Err(SaveError::Unreadable(error.to_string())),
        }
    }

    /// Explicit, bounded startup operation: remove a positively recognized
    /// schema-v1 config at the active path so v2 defaults load. It never scans
    /// other paths, never converts values and never writes a new config; every
    /// other document is left exactly as it is.
    pub fn retire_recognized_v1(&self) -> io::Result<V1Retirement> {
        match fs::symlink_metadata(&self.path) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Ok(V1Retirement::Absent)
            }
            Err(_) => return Ok(V1Retirement::NotRecognized),
            Ok(metadata) if !metadata.file_type().is_file() => {
                return Ok(V1Retirement::NotRecognized)
            }
            Ok(_) => {}
        }
        let bytes = match fs::read(&self.path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Ok(V1Retirement::Absent)
            }
            Err(_) => return Ok(V1Retirement::NotRecognized),
        };
        if !recognized_v1(&bytes) {
            return Ok(V1Retirement::NotRecognized);
        }
        let revision = ContentRevision::from_bytes(&bytes);

        #[cfg(test)]
        if let Some(hook) = &self.before_replace {
            hook(&self.path);
        }

        // Move the file aside atomically, then recheck the bytes that were
        // actually moved. A file that changed after recognition goes back.
        let parent = self.path.parent().unwrap_or_else(|| Path::new("."));
        let (aside, reservation) = create_unique_temp(
            parent,
            self.path
                .file_name()
                .unwrap_or_else(|| OsStr::new("config.toml")),
        )?;
        drop(reservation);
        if let Err(error) = fs::rename(&self.path, &aside) {
            let _ = fs::remove_file(&aside);
            return if error.kind() == io::ErrorKind::NotFound {
                Ok(V1Retirement::Changed)
            } else {
                Err(error)
            };
        }
        let moved = match fs::read(&aside) {
            Ok(moved) => moved,
            Err(error) => {
                self.restore_aside(&aside, parent)?;
                return Err(error);
            }
        };
        if ContentRevision::from_bytes(&moved) == revision {
            fs::remove_file(&aside)?;
            sync_directory(parent)?;
            return Ok(V1Retirement::Retired(revision));
        }
        self.restore_aside(&aside, parent)?;
        Ok(V1Retirement::Changed)
    }

    /// Put a moved-aside file back at the active path. If the active path is
    /// occupied, the moved file stays where it is and the error names it.
    fn restore_aside(&self, aside: &Path, parent: &Path) -> io::Result<()> {
        match fs::hard_link(aside, &self.path) {
            Ok(()) => {
                fs::remove_file(aside)?;
                sync_directory(parent)
            }
            Err(error) => Err(io::Error::new(
                error.kind(),
                format!(
                    "a settings file was preserved at {} because it could not be restored to {}: {error}",
                    aside.display(),
                    self.path.display()
                ),
            )),
        }
    }

    fn read_state(&self) -> ReadState {
        #[cfg(unix)]
        if let Ok(metadata) = fs::metadata(&self.path) {
            use std::os::unix::fs::PermissionsExt;
            if metadata.is_file() && metadata.permissions().mode() & 0o444 == 0 {
                return ReadState::Unreadable(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "settings file has no readable permission bits",
                ));
            }
        }
        match fs::read(&self.path) {
            Ok(bytes) => {
                let doc =
                    match std::str::from_utf8(&bytes) {
                        Ok(text) => text.parse::<DocumentMut>().map(Box::new).map_err(|error| {
                            ParseFailure {
                                offset: error.span().map(|span| span.start),
                                message: error.to_string(),
                            }
                        }),
                        Err(error) => Err(ParseFailure {
                            message: format!("settings are not valid UTF-8: {error}"),
                            offset: Some(error.valid_up_to()),
                        }),
                    };
                ReadState::Present { bytes, doc }
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => ReadState::Missing,
            Err(error) => ReadState::Unreadable(error),
        }
    }

    fn default_snapshot(
        &self,
        status: LoadStatus,
        file_exists: bool,
        revision: Option<ContentRevision>,
        raw_content: Option<String>,
        validation: Vec<SettingsDiagnostic>,
    ) -> SettingsSnapshot {
        let authored = SettingsDraft::defaults();
        let mut effective = self.project_defaults();
        if status != LoadStatus::Missing {
            effective.sources.clear();
        }
        SettingsSnapshot {
            config_path: self.path.clone(),
            file_exists,
            status,
            revision,
            raw_content,
            authored,
            source_catalog: SourceCatalog::defaults(),
            active: ActiveState {
                uses_authored_settings: false,
                source_is_authored: false,
                preferred_port: effective.preferred_port,
                effective_port: effective.effective_port,
            },
            effective,
            validation,
        }
    }

    fn snapshot_from_doc(&self, bytes: Vec<u8>, doc: &DocumentMut) -> SettingsSnapshot {
        let raw_content = Some(String::from_utf8_lossy(&bytes).into_owned());
        let revision = Some(ContentRevision::from_bytes(&bytes));
        let version = integer_at(doc, &["schema_version"]);
        if let Some(version) = version {
            if version != CURRENT_SCHEMA_VERSION && version != MIGRATED_SCHEMA_VERSION {
                return self.default_snapshot(
                    LoadStatus::UnsupportedVersion,
                    true,
                    revision,
                    raw_content,
                    vec![diagnostic(
                        DiagnosticKind::UnsupportedSchemaVersion,
                        Some("schema_version"),
                        if version < CURRENT_SCHEMA_VERSION {
                            format!(
                                "schema version {version} is from an earlier CCResDoc and was kept because it has entries this version does not recognize; move or delete {} to start from schema {CURRENT_SCHEMA_VERSION} defaults",
                                self.path.display()
                            )
                        } else {
                            format!("schema version {version} is unsupported")
                        },
                        true,
                    )],
                );
            }
        } else if doc.get("schema_version").is_some() {
            return self.default_snapshot(
                LoadStatus::Invalid,
                true,
                revision,
                raw_content,
                vec![invalid_type("schema_version", "an integer")],
            );
        }

        let mut authored = SettingsDraft::defaults();
        let mut diagnostics = Vec::new();
        if validate_section(doc, "resources", &mut diagnostics) {
            read_bool(
                doc,
                &["resources", "claude"],
                &mut authored.claude_resources,
                &mut diagnostics,
            );
            read_bool(
                doc,
                &["resources", "codex"],
                &mut authored.codex_resources,
                &mut diagnostics,
            );
        }
        if validate_section(doc, "source", &mut diagnostics) {
            read_string(
                doc,
                &["source", "claude_dir"],
                &mut authored.claude_dir,
                &mut diagnostics,
            );
            read_string(
                doc,
                &["source", "codex_dir"],
                &mut authored.codex_dir,
                &mut diagnostics,
            );
        }
        if validate_section(doc, "appearance", &mut diagnostics) {
            read_string(
                doc,
                &["appearance", "mode"],
                &mut authored.appearance_mode,
                &mut diagnostics,
            );
            read_string(
                doc,
                &["appearance", "theme_pack"],
                &mut authored.theme_pack,
                &mut diagnostics,
            );
        }
        if validate_section(doc, "server", &mut diagnostics) {
            read_integer(
                doc,
                &["server", "preferred_port"],
                &mut authored.preferred_port,
                &mut diagnostics,
            );
            read_bool(
                doc,
                &["server", "fallback_to_free_port"],
                &mut authored.fallback_to_free_port,
                &mut diagnostics,
            );
        }
        if validate_section(doc, "shortcuts", &mut diagnostics) {
            read_shortcut_entries(doc, &mut authored.shortcuts, &mut diagnostics);
        }

        let (effective, mut semantic) = self.validate_and_project(&authored);
        if version == Some(CURRENT_SCHEMA_VERSION) {
            semantic.retain(|diagnostic| {
                !diagnostic
                    .field
                    .as_deref()
                    .is_some_and(|field| field.starts_with("source."))
            });
        }
        let mut source_catalog = if version == Some(MIGRATED_SCHEMA_VERSION) {
            let mut catalog = SourceCatalog::defaults();
            catalog.sources[0].enabled = authored.claude_resources;
            catalog.sources[0].root.clone_from(&authored.claude_dir);
            catalog.sources[1].enabled = authored.codex_resources;
            catalog.sources[1].root.clone_from(&authored.codex_dir);
            catalog
        } else {
            read_source_catalog(doc, &mut diagnostics)
        };
        let (sources, mut source_diagnostics) = self.project_source_catalog(&source_catalog, false);
        semantic.append(&mut source_diagnostics);
        let mut effective = effective;
        effective.sources = sources;
        diagnostics.append(&mut semantic);
        let blocking = diagnostics.iter().any(|d| d.blocking);
        let status = if blocking {
            LoadStatus::Invalid
        } else {
            LoadStatus::Valid
        };
        let uses_authored = !blocking;
        let mut active_effective = if uses_authored {
            effective.clone()
        } else {
            self.project_defaults()
        };
        if !uses_authored {
            active_effective.sources.clear();
        }
        SettingsSnapshot {
            config_path: self.path.clone(),
            file_exists: true,
            status,
            revision,
            raw_content,
            authored,
            source_catalog: std::mem::take(&mut source_catalog),
            active: ActiveState {
                uses_authored_settings: uses_authored,
                source_is_authored: uses_authored,
                preferred_port: active_effective.preferred_port,
                effective_port: active_effective.effective_port,
            },
            effective: active_effective,
            validation: diagnostics,
        }
    }

    fn project_defaults(&self) -> EffectiveSettings {
        EffectiveSettings {
            claude_resources: true,
            codex_resources: false,
            claude_dir: Some(self.home.join(".claude")),
            codex_dir: None,
            appearance_mode: AppearanceMode::System,
            theme_pack: DEFAULT_THEME_PACK.into(),
            preferred_port: DEFAULT_PORT,
            effective_port: DEFAULT_PORT,
            fallback_to_free_port: true,
            shortcuts: default_shortcut_entries(),
            sources: self
                .project_source_catalog(&SourceCatalog::defaults(), false)
                .0,
        }
    }

    fn validate_and_project(
        &self,
        draft: &SettingsDraft,
    ) -> (EffectiveSettings, Vec<SettingsDiagnostic>) {
        let mut diagnostics = Vec::new();
        let mode = AppearanceMode::parse(&draft.appearance_mode).unwrap_or_else(|| {
            diagnostics.push(diagnostic(
                DiagnosticKind::InvalidAppearanceMode,
                Some("appearance.mode"),
                "mode must be system, light, or dark".into(),
                true,
            ));
            AppearanceMode::System
        });
        let port = u16::try_from(draft.preferred_port)
            .ok()
            .filter(|p| *p != 0)
            .unwrap_or_else(|| {
                diagnostics.push(diagnostic(
                    DiagnosticKind::InvalidPort,
                    Some("server.preferred_port"),
                    "port must be in 1..=65535".into(),
                    true,
                ));
                DEFAULT_PORT
            });
        let claude_dir = self.project_source(
            draft.claude_resources,
            &draft.claude_dir,
            "source.claude_dir",
            &mut diagnostics,
        );
        let codex_dir = self.project_source(
            draft.codex_resources,
            &draft.codex_dir,
            "source.codex_dir",
            &mut diagnostics,
        );
        let theme_pack = if self.available_theme_packs.contains(&draft.theme_pack) {
            draft.theme_pack.clone()
        } else {
            diagnostics.push(diagnostic(
                DiagnosticKind::ThemePackUnavailable,
                Some("appearance.theme_pack"),
                format!(
                    "theme pack '{}' is unavailable; using default",
                    draft.theme_pack
                ),
                false,
            ));
            DEFAULT_THEME_PACK.into()
        };
        let (shortcuts, mut shortcut_diagnostics) = validate_shortcut_entries(&draft.shortcuts);
        diagnostics.append(&mut shortcut_diagnostics);
        (
            EffectiveSettings {
                claude_resources: draft.claude_resources,
                codex_resources: draft.codex_resources,
                claude_dir,
                codex_dir,
                appearance_mode: mode,
                theme_pack,
                preferred_port: port,
                effective_port: port,
                fallback_to_free_port: draft.fallback_to_free_port,
                shortcuts,
                sources: Vec::new(),
            },
            diagnostics,
        )
    }

    fn project_source(
        &self,
        enabled: bool,
        raw: &str,
        field: &'static str,
        diagnostics: &mut Vec<SettingsDiagnostic>,
    ) -> Option<PathBuf> {
        if !enabled {
            return None;
        }
        // This store can enforce filesystem validity and the HOME boundary.
        // Generator output-overlap validation stays at runtime where both the
        // selected source and writable workspace output paths are available.
        match normalize_source(raw, &self.home) {
            Ok(path) => Some(path),
            Err((kind, message)) => {
                diagnostics.push(diagnostic(kind, Some(field), message, true));
                None
            }
        }
    }

    fn ensure_valid_draft(&self, draft: &SettingsDraft) -> Result<(), SaveError> {
        if draft.schema_version != CURRENT_SCHEMA_VERSION {
            return Err(SaveError::UnsupportedVersion(draft.schema_version));
        }
        let (_, diagnostics) = self.validate_and_project(draft);
        let blocking = diagnostics
            .into_iter()
            .filter(|d| d.blocking)
            .collect::<Vec<_>>();
        if blocking.is_empty() {
            Ok(())
        } else {
            Err(SaveError::Validation(blocking))
        }
    }

    fn normalized_draft(&self, draft: &SettingsDraft) -> Result<SettingsDraft, SaveError> {
        self.ensure_valid_draft(draft)?;
        let (effective, _) = self.validate_and_project(draft);
        let known = browser_command_catalog()
            .commands
            .into_iter()
            .map(|command| command.command_id)
            .collect::<BTreeSet<_>>();
        let mut normalized = draft.clone();
        normalized.shortcuts = effective.shortcuts;
        normalized.shortcuts.extend(
            draft
                .shortcuts
                .iter()
                .filter(|entry| !known.contains(&entry.command_id))
                .cloned(),
        );
        Ok(normalized)
    }

    fn ensure_revision(
        &self,
        expected: Option<&ContentRevision>,
        actual: Option<&ContentRevision>,
    ) -> Result<(), SaveError> {
        if expected == actual {
            Ok(())
        } else {
            Err(SaveError::RevisionConflict {
                expected: expected.cloned(),
                actual: actual.cloned(),
            })
        }
    }

    fn write_document(
        &self,
        doc: &DocumentMut,
        expected: Option<&ContentRevision>,
    ) -> Result<(), SaveError> {
        let use_crlf = fs::read(&self.path)
            .ok()
            .is_some_and(|bytes| bytes.windows(2).any(|pair| pair == b"\r\n"));
        let mut rendered = doc.to_string();
        while rendered.ends_with('\n') {
            rendered.pop();
        }
        rendered.push('\n');
        if use_crlf {
            rendered = rendered.replace('\n', "\r\n");
        }

        let parent = self.path.parent().unwrap_or_else(|| Path::new("."));
        fs::create_dir_all(parent)?;
        let existing_permissions = fs::metadata(&self.path).ok().map(|m| m.permissions());
        let (temp_path, mut temp) = create_unique_temp(
            parent,
            self.path
                .file_name()
                .unwrap_or_else(|| OsStr::new("config.toml")),
        )?;
        let mut cleanup = TempCleanup(Some(temp_path.clone()));

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = existing_permissions
                .as_ref()
                .map(PermissionsExt::mode)
                .unwrap_or(0o600);
            temp.set_permissions(fs::Permissions::from_mode(mode))?;
        }
        #[cfg(not(unix))]
        if let Some(permissions) = existing_permissions {
            temp.set_permissions(permissions)?;
        }

        temp.write_all(rendered.as_bytes())?;
        temp.flush()?;
        temp.sync_all()?;
        drop(temp);

        #[cfg(test)]
        if let Some(hook) = &self.before_replace {
            hook(&self.path);
        }

        let actual = match fs::read(&self.path) {
            Ok(bytes) => Some(ContentRevision::from_bytes(&bytes)),
            Err(error) if error.kind() == io::ErrorKind::NotFound => None,
            Err(error) => return Err(SaveError::Unreadable(error.to_string())),
        };
        self.ensure_revision(expected, actual.as_ref())?;
        fs::rename(&temp_path, &self.path)?;
        cleanup.0 = None;
        sync_directory(parent)?;
        Ok(())
    }
}

impl SettingField {
    fn app_fields() -> BTreeSet<Self> {
        [
            Self::AppearanceMode,
            Self::ThemePack,
            Self::PreferredPort,
            Self::FallbackToFreePort,
            Self::Shortcuts,
        ]
        .into_iter()
        .collect()
    }
    fn all() -> BTreeSet<Self> {
        [
            Self::ClaudeResources,
            Self::CodexResources,
            Self::ClaudeDir,
            Self::CodexDir,
            Self::AppearanceMode,
            Self::ThemePack,
            Self::PreferredPort,
            Self::FallbackToFreePort,
            Self::Shortcuts,
        ]
        .into_iter()
        .collect()
    }
}

fn normalize_source(raw: &str, home: &Path) -> Result<PathBuf, (DiagnosticKind, String)> {
    let expanded = if raw == "~" {
        if !home.is_absolute() {
            return Err((
                DiagnosticKind::InvalidSourcePath,
                "HOME must be absolute to expand ~ without using the working directory".into(),
            ));
        }
        home.to_path_buf()
    } else if let Some(rest) = raw.strip_prefix("~/") {
        if !home.is_absolute() {
            return Err((
                DiagnosticKind::InvalidSourcePath,
                "HOME must be absolute to expand ~ without using the working directory".into(),
            ));
        }
        home.join(rest)
    } else {
        let path = PathBuf::from(raw);
        if !path.is_absolute() {
            return Err((
                DiagnosticKind::InvalidSourcePath,
                "source path must be absolute or start with ~/".into(),
            ));
        }
        path
    };
    let metadata = fs::metadata(&expanded).map_err(|error| {
        (
            DiagnosticKind::InvalidSourcePath,
            format!("source directory is unavailable: {error}"),
        )
    })?;
    if !metadata.is_dir() {
        return Err((
            DiagnosticKind::InvalidSourcePath,
            "source path is not a directory".into(),
        ));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = metadata.permissions().mode();
        if mode & 0o444 == 0 || mode & 0o111 == 0 {
            return Err((
                DiagnosticKind::UnreadableSourcePath,
                "source directory is not readable/searchable".into(),
            ));
        }
    }
    fs::read_dir(&expanded).map_err(|error| {
        (
            DiagnosticKind::UnreadableSourcePath,
            format!("source directory is unreadable: {error}"),
        )
    })?;
    fs::canonicalize(&expanded)
        .map_err(|error| {
            (
                DiagnosticKind::InvalidSourcePath,
                format!("source directory cannot be normalized: {error}"),
            )
        })
        .and_then(|canonical| {
            let canonical_home = fs::canonicalize(home).map_err(|error| {
                (
                    DiagnosticKind::InvalidSourcePath,
                    format!("HOME cannot be normalized: {error}"),
                )
            })?;
            if canonical == canonical_home {
                Err((
                    DiagnosticKind::InvalidSourcePath,
                    "source directory must be narrower than HOME".into(),
                ))
            } else {
                Ok(canonical)
            }
        })
}

fn expand_source_path(raw: &str, home: &Path) -> Option<PathBuf> {
    let path = if let Some(rest) = raw.strip_prefix("~/") {
        home.join(rest)
    } else if raw == "~" {
        home.to_path_buf()
    } else {
        if raw.starts_with('~') {
            return None;
        }
        PathBuf::from(raw)
    };
    if !path.is_absolute()
        || path
            .components()
            .any(|component| matches!(component, std::path::Component::ParentDir))
    {
        return None;
    }
    let home = home.canonicalize().unwrap_or_else(|_| home.to_path_buf());
    if path == home || home.starts_with(&path) {
        return None;
    }
    Some(path)
}

fn path_is_missing(raw: &str, home: &Path) -> bool {
    expand_source_path(raw, home).is_some_and(|path| {
        std::fs::metadata(path).is_err_and(|error| error.kind() == io::ErrorKind::NotFound)
    })
}

fn item_at<'a>(doc: &'a DocumentMut, path: &[&str]) -> Option<&'a Item> {
    let mut item = doc.as_item();
    for component in path {
        item = item.get(component)?;
    }
    Some(item)
}

fn integer_at(doc: &DocumentMut, path: &[&str]) -> Option<i64> {
    item_at(doc, path)?.as_integer()
}

fn validate_section(
    doc: &DocumentMut,
    section: &str,
    diagnostics: &mut Vec<SettingsDiagnostic>,
) -> bool {
    match doc.get(section) {
        None => true,
        Some(item) if item.is_table() || item.as_inline_table().is_some() => true,
        Some(_) => {
            diagnostics.push(invalid_type(section, "a table"));
            false
        }
    }
}

fn read_string(
    doc: &DocumentMut,
    path: &[&str],
    target: &mut String,
    diagnostics: &mut Vec<SettingsDiagnostic>,
) {
    if let Some(item) = item_at(doc, path) {
        if let Some(value) = item.as_str() {
            *target = value.into();
        } else {
            diagnostics.push(invalid_type(&path.join("."), "a string"));
        }
    }
}

fn read_integer(
    doc: &DocumentMut,
    path: &[&str],
    target: &mut i64,
    diagnostics: &mut Vec<SettingsDiagnostic>,
) {
    if let Some(item) = item_at(doc, path) {
        if let Some(value) = item.as_integer() {
            *target = value;
        } else {
            diagnostics.push(invalid_type(&path.join("."), "an integer"));
        }
    }
}

fn read_bool(
    doc: &DocumentMut,
    path: &[&str],
    target: &mut bool,
    diagnostics: &mut Vec<SettingsDiagnostic>,
) {
    if let Some(item) = item_at(doc, path) {
        if let Some(value) = item.as_bool() {
            *target = value;
        } else {
            diagnostics.push(invalid_type(&path.join("."), "a boolean"));
        }
    }
}

fn read_shortcut_entries(
    doc: &DocumentMut,
    target: &mut Vec<ShortcutEntry>,
    diagnostics: &mut Vec<SettingsDiagnostic>,
) {
    let known = browser_command_catalog()
        .commands
        .into_iter()
        .map(|command| command.command_id)
        .collect::<BTreeSet<_>>();
    let mut authored = BTreeMap::<String, Result<Vec<String>, ()>>::new();
    let Some(section) = doc.get("shortcuts") else {
        return;
    };
    if let Some(table) = section.as_table() {
        for (command_id, item) in table.iter() {
            authored.insert(command_id.into(), shortcut_array_from_item(item));
        }
    } else if let Some(table) = section.as_inline_table() {
        for (command_id, item) in table.iter() {
            authored.insert(command_id.into(), shortcut_array_from_value(item));
        }
    }

    for entry in target.iter_mut() {
        if let Some(value) = authored.remove(&entry.command_id) {
            match value {
                Ok(bindings) => entry.bindings = bindings,
                Err(()) => diagnostics.push(invalid_type(
                    &format!("shortcuts.{}", entry.command_id),
                    "an array of strings",
                )),
            }
        }
    }
    for (command_id, bindings) in authored {
        if known.contains(&command_id) {
            continue;
        }
        if let Ok(bindings) = bindings {
            target.push(ShortcutEntry {
                command_id,
                bindings,
            });
        }
        // Unknown values belong to a future writer. Their exact TOML remains
        // in the lossless document and Rust deliberately does not diagnose or
        // rewrite them.
    }
}

fn shortcut_array_from_item(item: &Item) -> Result<Vec<String>, ()> {
    item.as_array()
        .ok_or(())?
        .iter()
        .map(|value| value.as_str().map(str::to_owned).ok_or(()))
        .collect()
}

fn shortcut_array_from_value(item: &toml_edit::Value) -> Result<Vec<String>, ()> {
    item.as_array()
        .ok_or(())?
        .iter()
        .map(|value| value.as_str().map(str::to_owned).ok_or(()))
        .collect()
}

fn validate_shortcut_entries(
    entries: &[ShortcutEntry],
) -> (Vec<ShortcutEntry>, Vec<SettingsDiagnostic>) {
    let catalog = browser_command_catalog();
    let mut supplied = BTreeMap::<&str, &ShortcutEntry>::new();
    let mut diagnostics = Vec::new();
    for entry in entries {
        if supplied.insert(&entry.command_id, entry).is_some() {
            diagnostics.push(shortcut_diagnostic(
                DiagnosticKind::InvalidShortcut,
                &entry.command_id,
                format!(
                    "shortcut command '{}' appears more than once",
                    entry.command_id
                ),
            ));
        }
    }

    let reserved = reserved_shortcuts();
    let mut claimed = Vec::<(String, String, NormalizedShortcut)>::new();
    let mut effective = Vec::new();
    for command in &catalog.commands {
        let bindings = supplied
            .get(command.command_id.as_str())
            .map(|entry| entry.bindings.as_slice())
            .unwrap_or(command.default_bindings.as_slice());
        let mut normalized = Vec::<NormalizedShortcut>::new();
        for raw in bindings {
            match normalize_shortcut_binding(raw) {
                Ok(binding) => {
                    if normalized
                        .iter()
                        .any(|existing| existing.conflicts_with(&binding))
                    {
                        diagnostics.push(shortcut_diagnostic(
                            DiagnosticKind::InvalidShortcut,
                            &command.command_id,
                            format!("'{raw}' duplicates another binding for {}", command.label),
                        ));
                        continue;
                    }
                    for (reserved_name, reserved_binding) in &reserved {
                        if binding.conflicts_with(reserved_binding) {
                            diagnostics.push(shortcut_diagnostic(
                                DiagnosticKind::ShortcutConflict,
                                &command.command_id,
                                format!(
                                    "{} conflicts with reserved action {reserved_name} ({})",
                                    command.label,
                                    reserved_binding.as_portable_string()
                                ),
                            ));
                        }
                    }
                    for (other_id, other_label, other_binding) in &claimed {
                        if binding.conflicts_with(other_binding) {
                            diagnostics.push(shortcut_diagnostic(
                                DiagnosticKind::ShortcutConflict,
                                &command.command_id,
                                format!(
                                    "{} conflicts with {other_label} ({other_id}) on {}",
                                    command.label,
                                    binding.as_portable_string()
                                ),
                            ));
                            diagnostics.push(shortcut_diagnostic(
                                DiagnosticKind::ShortcutConflict,
                                other_id,
                                format!(
                                    "{other_label} conflicts with {} ({}) on {}",
                                    command.label,
                                    command.command_id,
                                    binding.as_portable_string()
                                ),
                            ));
                        }
                    }
                    claimed.push((
                        command.command_id.clone(),
                        command.label.clone(),
                        binding.clone(),
                    ));
                    normalized.push(binding);
                }
                Err(error) => diagnostics.push(shortcut_diagnostic(
                    DiagnosticKind::InvalidShortcut,
                    &command.command_id,
                    format!("invalid binding '{raw}' for {}: {error}", command.label),
                )),
            }
        }
        effective.push(ShortcutEntry {
            command_id: command.command_id.clone(),
            bindings: normalized
                .iter()
                .map(NormalizedShortcut::as_portable_string)
                .collect(),
        });
    }
    (effective, diagnostics)
}

fn reserved_shortcuts() -> Vec<(&'static str, NormalizedShortcut)> {
    [
        ("Settings", "Mod+,"),
        ("Undo", "Mod+Z"),
        ("Redo", "Mod+Shift+Z"),
        ("Redo", "Mod+Y"),
        ("Cut", "Mod+X"),
        ("Copy", "Mod+C"),
        ("Paste", "Mod+V"),
        ("Select All", "Mod+A"),
        ("Actual Size", "Mod+0"),
        ("Zoom In", "Mod+="),
        ("Zoom Out", "Mod+-"),
        ("Toggle Developer Tools", "Mod+Alt+I"),
        ("Minimize", "Mod+M"),
        ("Hide", "Mod+H"),
        ("Hide Others", "Mod+Alt+H"),
        ("Quit", "Mod+Q"),
    ]
    .into_iter()
    .map(|(name, binding)| {
        (
            name,
            normalize_shortcut_binding(binding).expect("reserved shortcuts are valid"),
        )
    })
    .collect()
}

fn shortcut_diagnostic(
    kind: DiagnosticKind,
    command_id: &str,
    message: String,
) -> SettingsDiagnostic {
    diagnostic(
        kind,
        Some(&format!("shortcuts.{command_id}")),
        message,
        true,
    )
}

fn invalid_type(field: &str, expected: &str) -> SettingsDiagnostic {
    diagnostic(
        DiagnosticKind::InvalidType,
        Some(field),
        format!("{field} must be {expected}"),
        true,
    )
}

fn diagnostic(
    kind: DiagnosticKind,
    field: Option<&str>,
    message: String,
    blocking: bool,
) -> SettingsDiagnostic {
    SettingsDiagnostic {
        kind,
        field: field.map(str::to_owned),
        message,
        blocking,
        location: None,
    }
}

fn byte_location(bytes: &[u8], offset: usize) -> SourceLocation {
    let prefix = &bytes[..offset.min(bytes.len())];
    let line = prefix.iter().filter(|byte| **byte == b'\n').count() + 1;
    let column = prefix
        .iter()
        .rev()
        .take_while(|byte| **byte != b'\n')
        .count()
        + 1;
    SourceLocation { line, column }
}

fn ensure_table<'a>(doc: &'a mut DocumentMut, name: &str) -> &'a mut Table {
    if !doc.get(name).is_some_and(Item::is_table) {
        let decor = doc
            .get(name)
            .and_then(Item::as_value)
            .map(|value| value.decor().clone());
        let mut table = Table::new();
        if let Some(decor) = decor {
            *table.decor_mut() = decor;
        }
        doc[name] = Item::Table(table);
    }
    doc[name].as_table_mut().expect("table inserted above")
}

fn set_section_value(doc: &mut DocumentMut, section: &str, key: &str, replacement: Item) {
    if let Some(inline) = doc.get_mut(section).and_then(Item::as_inline_table_mut) {
        let Item::Value(mut replacement) = replacement else {
            unreachable!("settings fields are TOML values")
        };
        if let Some(existing) = inline.get(key) {
            *replacement.decor_mut() = existing.decor().clone();
        }
        inline.insert(key, replacement);
    } else {
        set_value_preserving_decor(&mut ensure_table(doc, section)[key], replacement);
    }
}

fn merge_fields(doc: &mut DocumentMut, draft: &SettingsDraft, fields: &BTreeSet<SettingField>) {
    set_value_preserving_decor(&mut doc["schema_version"], value(CURRENT_SCHEMA_VERSION));
    if fields.contains(&SettingField::ClaudeResources) {
        set_section_value(doc, "resources", "claude", value(draft.claude_resources));
    }
    if fields.contains(&SettingField::CodexResources) {
        set_section_value(doc, "resources", "codex", value(draft.codex_resources));
    }
    if fields.contains(&SettingField::ClaudeDir) {
        set_section_value(doc, "source", "claude_dir", value(&draft.claude_dir));
    }
    if fields.contains(&SettingField::CodexDir) {
        set_section_value(doc, "source", "codex_dir", value(&draft.codex_dir));
    }
    if fields.contains(&SettingField::AppearanceMode) {
        set_section_value(doc, "appearance", "mode", value(&draft.appearance_mode));
    }
    if fields.contains(&SettingField::ThemePack) {
        set_section_value(doc, "appearance", "theme_pack", value(&draft.theme_pack));
    }
    if fields.contains(&SettingField::PreferredPort) {
        set_section_value(doc, "server", "preferred_port", value(draft.preferred_port));
    }
    if fields.contains(&SettingField::FallbackToFreePort) {
        set_section_value(
            doc,
            "server",
            "fallback_to_free_port",
            value(draft.fallback_to_free_port),
        );
    }
    if fields.contains(&SettingField::Shortcuts) {
        merge_shortcuts(doc, &draft.shortcuts);
    }
}

fn valid_identity(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 80
        && value.starts_with(|c: char| c.is_ascii_alphanumeric())
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-'))
}

fn catalog_from_legacy(draft: &SettingsDraft) -> SourceCatalog {
    let mut catalog = SourceCatalog::defaults();
    catalog.sources[0].enabled = draft.claude_resources;
    catalog.sources[0].root.clone_from(&draft.claude_dir);
    catalog.sources[1].enabled = draft.codex_resources;
    catalog.sources[1].root.clone_from(&draft.codex_dir);
    catalog
}

fn valid_mount(mount: &str) -> bool {
    !mount.is_empty()
        && mount.len() <= 80
        && mount.starts_with(|c: char| c.is_ascii_lowercase() || c.is_ascii_digit())
        && mount
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '.' | '_' | '-'))
        && !matches!(
            mount,
            "ccresdoc"
                | "index"
                | "welcome"
                | "claude-md"
                | "claude-commands"
                | "claude-skills"
                | "claude-agents"
                | "codex-agents-md"
                | "codex-config"
                | "codex-agents"
                | "codex-hooks"
                | "codex-rules"
                | "codex-skills"
        )
}

fn string_array(values: &[String]) -> Item {
    let mut array = Array::new();
    for value in values {
        array.push(value.as_str());
    }
    Item::Value(Value::Array(array))
}

fn rule_table(rules: &RuleSnapshot) -> Table {
    let mut table = Table::new();
    match rules {
        RuleSnapshot::ClaudeCode { definition_version } => {
            table["kind"] = value("claude_code");
            table["definition_version"] = value(i64::from(*definition_version));
        }
        RuleSnapshot::Codex { definition_version } => {
            table["kind"] = value("codex");
            table["definition_version"] = value(i64::from(*definition_version));
        }
        RuleSnapshot::Custom {
            definition_version,
            format,
            include,
            exclude,
        } => {
            table["kind"] = value("custom");
            table["definition_version"] = value(i64::from(*definition_version));
            table["format"] = value(match format {
                PublicFormat::Markdown => "markdown",
                PublicFormat::Code => "code",
                PublicFormat::Skill => "skill",
            });
            table["include"] = string_array(include);
            table["exclude"] = string_array(exclude);
        }
    }
    table
}

fn merge_source_catalog(doc: &mut DocumentMut, catalog: &SourceCatalog) {
    let mut sources = ArrayOfTables::new();
    for source in &catalog.sources {
        let mut table = Table::new();
        table["id"] = value(&source.id);
        table["label"] = value(&source.label);
        table["enabled"] = value(source.enabled);
        table["root"] = value(&source.root);
        table["mount"] = value(&source.mount);
        let mut scan = Table::new();
        match &source.scan_root {
            ScanRootChoice::Same => scan["kind"] = value("same"),
            ScanRootChoice::Separate { path } => {
                scan["kind"] = value("separate");
                scan["path"] = value(path);
            }
        }
        table["scan_root"] = Item::Table(scan);
        if let Some(origin) = &source.origin_preset {
            let mut preset = Table::new();
            preset["id"] = value(&origin.id);
            preset["name"] = value(&origin.name);
            preset["version"] = value(i64::from(origin.version));
            table["origin_preset"] = Item::Table(preset);
        }
        table["rules"] = Item::Table(rule_table(&source.rules));
        sources.push(table);
    }
    doc["document_sources"] = Item::ArrayOfTables(sources);
    let mut presets = ArrayOfTables::new();
    for preset in &catalog.presets {
        let mut table = Table::new();
        table["id"] = value(&preset.id);
        table["name"] = value(&preset.name);
        table["description"] = value(&preset.description);
        table["version"] = value(i64::from(preset.version));
        table["rules"] = Item::Table(rule_table(&preset.rules));
        presets.push(table);
    }
    doc["user_presets"] = Item::ArrayOfTables(presets);
    set_section_value(doc, "document_source_catalog", "format_version", value(1));
}

fn read_source_catalog(
    doc: &DocumentMut,
    diagnostics: &mut Vec<SettingsDiagnostic>,
) -> SourceCatalog {
    let Some(source_item) = doc.get("document_sources") else {
        return SourceCatalog::defaults();
    };
    let Some(source_tables) = source_item.as_array_of_tables() else {
        diagnostics.push(invalid_type("document_sources", "an array of tables"));
        return SourceCatalog::defaults();
    };
    let mut catalog = SourceCatalog::default();
    for (index, table) in source_tables.iter().enumerate() {
        match parse_document_source(table) {
            Some(source) => catalog.sources.push(source),
            None => diagnostics.push(invalid_type(
                &format!("document_sources[{index}]"),
                "a complete source table",
            )),
        }
    }
    if let Some(preset_item) = doc.get("user_presets") {
        if let Some(preset_tables) = preset_item.as_array_of_tables() {
            for (index, table) in preset_tables.iter().enumerate() {
                match parse_user_preset(table) {
                    Some(preset) => catalog.presets.push(preset),
                    None => diagnostics.push(invalid_type(
                        &format!("user_presets[{index}]"),
                        "a complete preset table",
                    )),
                }
            }
        } else {
            diagnostics.push(invalid_type("user_presets", "an array of tables"));
        }
    }
    catalog
}

fn table_string(table: &Table, key: &str) -> Option<String> {
    table.get(key)?.as_str().map(str::to_owned)
}
fn table_u32(table: &Table, key: &str) -> Option<u32> {
    u32::try_from(table.get(key)?.as_integer()?).ok()
}
fn table_strings(table: &Table, key: &str) -> Option<Vec<String>> {
    table
        .get(key)?
        .as_array()?
        .iter()
        .map(|value| value.as_str().map(str::to_owned))
        .collect()
}
fn parse_rules(table: &Table) -> Option<RuleSnapshot> {
    let version = table_u32(table, "definition_version")?;
    match table_string(table, "kind")?.as_str() {
        "claude_code" => Some(RuleSnapshot::ClaudeCode {
            definition_version: version,
        }),
        "codex" => Some(RuleSnapshot::Codex {
            definition_version: version,
        }),
        "custom" => Some(RuleSnapshot::Custom {
            definition_version: version,
            format: match table_string(table, "format")?.as_str() {
                "markdown" => PublicFormat::Markdown,
                "code" => PublicFormat::Code,
                "skill" => PublicFormat::Skill,
                _ => return None,
            },
            include: table_strings(table, "include")?,
            exclude: table_strings(table, "exclude")?,
        }),
        _ => None,
    }
}
fn parse_document_source(table: &Table) -> Option<DocumentSource> {
    let scan = table.get("scan_root")?.as_table()?;
    let scan_root = match table_string(scan, "kind")?.as_str() {
        "same" => ScanRootChoice::Same,
        "separate" => ScanRootChoice::Separate {
            path: table_string(scan, "path")?,
        },
        _ => return None,
    };
    let origin_preset = match table.get("origin_preset") {
        None => None,
        Some(item) => {
            let table = item.as_table()?;
            Some(PresetRef {
                id: table_string(table, "id")?,
                name: table_string(table, "name")?,
                version: table_u32(table, "version")?,
            })
        }
    };
    Some(DocumentSource {
        id: table_string(table, "id")?,
        label: table_string(table, "label")?,
        enabled: table.get("enabled")?.as_bool()?,
        root: table_string(table, "root")?,
        scan_root,
        mount: table_string(table, "mount")?,
        origin_preset,
        rules: parse_rules(table.get("rules")?.as_table()?)?,
    })
}
fn parse_user_preset(table: &Table) -> Option<UserPreset> {
    Some(UserPreset {
        id: table_string(table, "id")?,
        name: table_string(table, "name")?,
        description: table_string(table, "description")?,
        version: table_u32(table, "version")?,
        rules: parse_rules(table.get("rules")?.as_table()?)?,
    })
}

fn merge_catalog_three_way(
    base: &SourceCatalog,
    draft: &SourceCatalog,
    latest: &SourceCatalog,
) -> Option<SourceCatalog> {
    Some(SourceCatalog {
        sources: merge_entries_by_id(&base.sources, &draft.sources, &latest.sources, |entry| {
            &entry.id
        })?,
        presets: merge_entries_by_id(&base.presets, &draft.presets, &latest.presets, |entry| {
            &entry.id
        })?,
    })
}

fn merge_entries_by_id<T: Clone + PartialEq>(
    base: &[T],
    draft: &[T],
    latest: &[T],
    id: impl Fn(&T) -> &str,
) -> Option<Vec<T>> {
    let base_map = base
        .iter()
        .map(|entry| (id(entry).to_owned(), entry))
        .collect::<BTreeMap<_, _>>();
    let draft_map = draft
        .iter()
        .map(|entry| (id(entry).to_owned(), entry))
        .collect::<BTreeMap<_, _>>();
    let latest_map = latest
        .iter()
        .map(|entry| (id(entry).to_owned(), entry))
        .collect::<BTreeMap<_, _>>();
    let keys = base_map
        .keys()
        .chain(draft_map.keys())
        .chain(latest_map.keys())
        .cloned()
        .collect::<BTreeSet<_>>();
    let mut merged = latest.to_vec();
    for key in keys {
        let base_entry = base_map.get(&key).copied();
        let draft_entry = draft_map.get(&key).copied();
        let latest_entry = latest_map.get(&key).copied();
        if draft_entry == base_entry {
            continue;
        }
        if latest_entry != base_entry && latest_entry != draft_entry {
            return None;
        }
        match (
            merged.iter().position(|entry| id(entry) == key),
            draft_entry,
        ) {
            (Some(index), Some(entry)) => merged[index] = entry.clone(),
            (Some(index), None) => {
                merged.remove(index);
            }
            (None, Some(entry)) => merged.push(entry.clone()),
            (None, None) => {}
        }
    }
    Some(merged)
}

fn merge_shortcuts(doc: &mut DocumentMut, entries: &[ShortcutEntry]) {
    let defaults = default_shortcut_entries()
        .into_iter()
        .map(|entry| (entry.command_id, entry.bindings))
        .collect::<BTreeMap<_, _>>();
    let has_custom_data = entries
        .iter()
        .any(|entry| defaults.get(&entry.command_id) != Some(&entry.bindings));
    if doc.get("shortcuts").is_none() && !has_custom_data {
        return;
    }
    for entry in entries {
        let known = defaults.contains_key(&entry.command_id);
        let already_present = item_at(doc, &["shortcuts", &entry.command_id]).is_some();
        if known || !already_present {
            set_section_value(
                doc,
                "shortcuts",
                &entry.command_id,
                shortcut_array_item(&entry.bindings),
            );
        }
    }
}

fn shortcut_array_item(bindings: &[String]) -> Item {
    let mut array = Array::new();
    for binding in bindings {
        array.push(binding.as_str());
    }
    Item::Value(Value::Array(array))
}

fn set_value_preserving_decor(target: &mut Item, mut replacement: Item) {
    let decor = target.as_value().map(|value| value.decor().clone());
    if let (Some(decor), Some(value)) = (decor, replacement.as_value_mut()) {
        *value.decor_mut() = decor;
    }
    *target = replacement;
}

fn copy_dirty(target: &mut SettingsDraft, source: &SettingsDraft, fields: &BTreeSet<SettingField>) {
    for field in fields {
        match field {
            SettingField::ClaudeResources => target.claude_resources = source.claude_resources,
            SettingField::CodexResources => target.codex_resources = source.codex_resources,
            SettingField::ClaudeDir => target.claude_dir.clone_from(&source.claude_dir),
            SettingField::CodexDir => target.codex_dir.clone_from(&source.codex_dir),
            SettingField::AppearanceMode => {
                target.appearance_mode.clone_from(&source.appearance_mode)
            }
            SettingField::ThemePack => target.theme_pack.clone_from(&source.theme_pack),
            SettingField::PreferredPort => target.preferred_port = source.preferred_port,
            SettingField::FallbackToFreePort => {
                target.fallback_to_free_port = source.fallback_to_free_port
            }
            SettingField::Shortcuts => target.shortcuts.clone_from(&source.shortcuts),
        }
    }
}

fn impact_between(
    before: &SettingsDraft,
    after: &SettingsDraft,
    fields: &BTreeSet<SettingField>,
) -> ApplyImpact {
    let restart = (fields.contains(&SettingField::ClaudeResources)
        && before.claude_resources != after.claude_resources)
        || (fields.contains(&SettingField::CodexResources)
            && before.codex_resources != after.codex_resources)
        || (fields.contains(&SettingField::ClaudeDir) && before.claude_dir != after.claude_dir)
        || (fields.contains(&SettingField::CodexDir) && before.codex_dir != after.codex_dir)
        || (fields.contains(&SettingField::PreferredPort)
            && before.preferred_port != after.preferred_port)
        || (fields.contains(&SettingField::FallbackToFreePort)
            && before.fallback_to_free_port != after.fallback_to_free_port);
    if restart {
        return ApplyImpact::RestartRuntime;
    }
    let appearance = (fields.contains(&SettingField::AppearanceMode)
        && before.appearance_mode != after.appearance_mode)
        || (fields.contains(&SettingField::ThemePack) && before.theme_pack != after.theme_pack);
    if appearance {
        ApplyImpact::AppearanceOnly
    } else {
        ApplyImpact::None
    }
}

fn create_unique_temp(parent: &Path, file_name: &OsStr) -> io::Result<(PathBuf, File)> {
    for _ in 0..128 {
        let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let candidate = parent.join(format!(
            ".{}.{}.{}.tmp",
            file_name.to_string_lossy(),
            std::process::id(),
            sequence
        ));
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&candidate)
        {
            Ok(file) => return Ok((candidate, file)),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "could not allocate a unique settings temporary file",
    ))
}

struct TempCleanup(Option<PathBuf>);
impl Drop for TempCleanup {
    fn drop(&mut self) {
        if let Some(path) = self.0.take() {
            let _ = fs::remove_file(path);
        }
    }
}

fn sync_directory(path: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        File::open(path)?.sync_all()
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        Ok(())
    }
}

fn first_message(diagnostics: &[SettingsDiagnostic]) -> String {
    diagnostics
        .first()
        .map(|d| d.message.clone())
        .unwrap_or_else(|| "unknown error".into())
}

fn schema_from_raw(raw: &Option<String>) -> i64 {
    raw.as_deref()
        .and_then(|s| s.parse::<DocumentMut>().ok())
        .and_then(|d| integer_at(&d, &["schema_version"]))
        .unwrap_or(-1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use filetime::{set_file_mtime, FileTime};
    use std::sync::Arc;
    use tempfile::TempDir;

    fn fixture() -> (TempDir, SettingsStore) {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir_all(root.path().join("home/.claude")).unwrap();
        let store = SettingsStore::new(
            root.path().join("config/config.toml"),
            root.path().join("home"),
        );
        (root, store)
    }

    fn valid_toml(home: &Path) -> String {
        format!("schema_version = 2\n\n[resources]\nclaude = true\ncodex = false\n\n[source]\nclaude_dir = {:?}\ncodex_dir = \"~/.codex\"\n\n[appearance]\nmode = \"dark\"\ntheme_pack = \"classic\"\n\n[server]\npreferred_port = 5000\nfallback_to_free_port = false\n", home.join(".claude").to_string_lossy())
    }

    #[test]
    fn resolution_order_and_missing_home() {
        let env = ConfigEnvironment {
            override_path: Some("/override.toml".into()),
            xdg_config_home: Some("/xdg".into()),
            home: Some("/home/me".into()),
        };
        assert_eq!(
            resolve_config_path_from(&env).unwrap(),
            PathBuf::from("/override.toml")
        );
        let env = ConfigEnvironment {
            override_path: None,
            ..env
        };
        assert_eq!(
            resolve_config_path_from(&env).unwrap(),
            PathBuf::from("/xdg/ccresdoc/config.toml")
        );
        let env = ConfigEnvironment {
            xdg_config_home: None,
            ..env
        };
        assert_eq!(
            resolve_config_path_from(&env).unwrap(),
            PathBuf::from("/home/me/.config/ccresdoc/config.toml")
        );
        let env = ConfigEnvironment { home: None, ..env };
        assert_eq!(
            resolve_config_path_from(&env),
            Err(ConfigPathError::MissingHome)
        );
    }

    #[test]
    fn bundled_theme_catalog_exposes_supported_nondefault_packs() {
        let packs = bundled_theme_pack_slugs();
        assert!(packs.iter().any(|slug| slug == DEFAULT_THEME_PACK));
        assert!(!packs.iter().any(|slug| slug == "default"));
        assert!(packs.len() > 1);
        assert!(packs.contains(&"eink".to_string()));
    }

    #[test]
    fn missing_load_does_not_create_any_paths() {
        let (_root, store) = fixture();
        let snapshot = store.load();
        assert_eq!(snapshot.status, LoadStatus::Missing);
        assert!(!snapshot.file_exists);
        assert!(!store.path().exists());
        assert!(!store.path().parent().unwrap().exists());
    }

    #[test]
    fn isolated_contract_reports_authored_source_and_preferred_port_without_writing_missing_config()
    {
        let root = tempfile::tempdir().unwrap();
        let home = root.path().join("home");
        let source = root.path().join("isolated-source");
        fs::create_dir_all(home.join(".claude")).unwrap();
        fs::create_dir_all(&source).unwrap();
        let path = root.path().join("override/config.toml");
        let store = SettingsStore::new(path.clone(), home.clone());

        let missing = store.load();
        assert_eq!(missing.config_path, path);
        assert_eq!(missing.status, LoadStatus::Missing);
        assert!(!missing.file_exists);
        assert_eq!(missing.effective.claude_dir, Some(home.join(".claude")));
        assert_eq!(missing.effective.codex_dir, None);
        assert_eq!(missing.active.preferred_port, DEFAULT_PORT);
        assert_eq!(missing.active.effective_port, DEFAULT_PORT);
        assert!(
            !path.exists(),
            "path discovery must not create config bytes"
        );

        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(
            &path,
            format!(
                "schema_version = 2\n[source]\nclaude_dir = {:?}\n[server]\npreferred_port = 53001\nfallback_to_free_port = true\n",
                source.to_string_lossy()
            ),
        )
        .unwrap();
        let authored = store.load();
        assert_eq!(authored.status, LoadStatus::Valid);
        assert!(authored.active.uses_authored_settings);
        assert!(authored.active.source_is_authored);
        assert_eq!(authored.authored.preferred_port, 53001);
        assert_eq!(authored.effective.preferred_port, 53001);
        assert_eq!(authored.effective.effective_port, 53001);
        assert_eq!(
            authored.effective.claude_dir,
            Some(fs::canonicalize(source).unwrap())
        );
        assert!(authored.effective.fallback_to_free_port);
    }

    #[test]
    fn valid_partial_and_round_trip() {
        let (root, store) = fixture();
        fs::create_dir_all(store.path().parent().unwrap()).unwrap();
        fs::write(
            store.path(),
            "schema_version = 2\n\n[appearance]\nmode = \"light\"\n",
        )
        .unwrap();
        let loaded = store.load();
        assert_eq!(loaded.status, LoadStatus::Valid);
        assert_eq!(loaded.authored.appearance_mode, "light");
        assert_eq!(loaded.authored.preferred_port, 4892);
        let result = store
            .save(&loaded.authored, loaded.revision.as_ref())
            .unwrap();
        assert_eq!(result.snapshot.status, LoadStatus::Valid);
        assert_eq!(
            result.snapshot.effective.claude_dir,
            Some(fs::canonicalize(root.path().join("home/.claude")).unwrap())
        );
        assert!(fs::read_to_string(store.path()).unwrap().ends_with('\n'));
    }

    #[test]
    fn partial_and_missing_documents_inherit_resource_defaults_without_rewrite() {
        let (_root, store) = fixture();
        let missing = store.load();
        assert!(missing.authored.claude_resources);
        assert!(!missing.authored.codex_resources);
        assert_eq!(missing.authored.claude_dir, "~/.claude");
        assert_eq!(missing.authored.codex_dir, "~/.codex");
        assert!(!store.path().exists());

        fs::create_dir_all(store.path().parent().unwrap()).unwrap();
        let partial = "schema_version = 2\n[appearance]\nmode = \"dark\"\n";
        fs::write(store.path(), partial).unwrap();
        let loaded = store.load();
        assert_eq!(loaded.status, LoadStatus::Valid);
        assert!(loaded.authored.claude_resources);
        assert!(!loaded.authored.codex_resources);
        assert_eq!(fs::read_to_string(store.path()).unwrap(), partial);
    }

    fn v1_config(home: &Path) -> String {
        format!(
            "# kept by hand\nschema_version = 1\n\n[resources]\nclaude = true\ncodex = false\n\n[source]\nclaude_dir = {:?}\n\n[appearance]\nmode = \"dark\"\ntheme_pack = \"default\"\n\n[server]\npreferred_port = 5000\n",
            home.join(".claude").to_string_lossy()
        )
    }

    #[test]
    fn ordinary_load_protects_a_v1_config_until_explicit_retirement() {
        let (root, store) = fixture();
        fs::create_dir_all(store.path().parent().unwrap()).unwrap();
        let v1 = v1_config(&root.path().join("home"));
        fs::write(store.path(), &v1).unwrap();
        let loaded = store.load();
        assert_eq!(loaded.status, LoadStatus::UnsupportedVersion);
        assert!(matches!(
            store.save(&SettingsDraft::defaults(), loaded.revision.as_ref()),
            Err(SaveError::UnsupportedVersion(1))
        ));
        assert_eq!(fs::read_to_string(store.path()).unwrap(), v1);
    }

    #[test]
    fn recognized_v1_is_retired_to_v2_defaults_without_writing_a_config() {
        let (root, store) = fixture();
        let home = root.path().join("home");
        let source = home.join(".claude");
        fs::create_dir_all(&source).unwrap();
        fs::write(source.join("CLAUDE.md"), "user source data\n").unwrap();
        fs::create_dir_all(store.path().parent().unwrap()).unwrap();
        let sibling = store.path().with_file_name("config.toml.bak");
        fs::write(&sibling, v1_config(&home)).unwrap();
        fs::write(store.path(), v1_config(&home)).unwrap();
        let revision = ContentRevision::from_bytes(v1_config(&home).as_bytes());

        assert_eq!(
            store.retire_recognized_v1().unwrap(),
            V1Retirement::Retired(revision)
        );
        assert!(
            !store.path().exists(),
            "no v2 config is created before Save"
        );
        let loaded = store.load();
        assert_eq!(loaded.status, LoadStatus::Missing);
        assert_eq!(loaded.authored, SettingsDraft::defaults());
        assert_eq!(loaded.effective.theme_pack, DEFAULT_THEME_PACK);
        // Only the active path is retired: no directory scan, no source data.
        assert_eq!(fs::read_to_string(&sibling).unwrap(), v1_config(&home));
        assert_eq!(
            fs::read_to_string(source.join("CLAUDE.md")).unwrap(),
            "user source data\n"
        );
        let mut entries = fs::read_dir(store.path().parent().unwrap())
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .collect::<Vec<_>>();
        entries.sort();
        assert_eq!(entries, ["config.toml.bak"]);
        // A second startup finds nothing to do.
        assert_eq!(store.retire_recognized_v1().unwrap(), V1Retirement::Absent);
    }

    #[test]
    fn v2_save_after_retirement_persists_across_relaunch() {
        let (root, store) = fixture();
        fs::create_dir_all(root.path().join("home/.claude")).unwrap();
        fs::create_dir_all(store.path().parent().unwrap()).unwrap();
        fs::write(store.path(), v1_config(&root.path().join("home"))).unwrap();
        assert!(matches!(
            store.retire_recognized_v1().unwrap(),
            V1Retirement::Retired(_)
        ));
        let mut draft = store.load().authored;
        draft.appearance_mode = "light".into();
        draft.theme_pack = "graphite".into();
        let saved = store.save(&draft, None).unwrap();
        assert_eq!(saved.snapshot.status, LoadStatus::Valid);
        assert!(fs::read_to_string(store.path())
            .unwrap()
            .contains("schema_version = 3"));

        // Relaunch: retirement leaves the saved v2 config alone.
        let relaunched = SettingsStore::with_theme_packs(
            store.path().to_path_buf(),
            root.path().join("home"),
            ["classic", "graphite"],
        );
        assert_eq!(
            relaunched.retire_recognized_v1().unwrap(),
            V1Retirement::NotRecognized
        );
        let loaded = relaunched.load();
        assert_eq!(loaded.status, LoadStatus::Valid);
        assert_eq!(loaded.effective.appearance_mode, AppearanceMode::Light);
        assert_eq!(loaded.effective.theme_pack, "graphite");
    }

    #[test]
    fn retirement_protects_malformed_future_unreadable_and_foreign_documents() {
        let (root, store) = fixture();
        fs::create_dir_all(store.path().parent().unwrap()).unwrap();
        for content in [
            "schema_version = 1\n[source\nnope".to_string(),
            "schema_version = 4\nfuture = true\n".to_string(),
            "schema_version = 2\n[appearance]\nmode = \"dark\"\n".to_string(),
            "[appearance]\nmode = \"dark\"\n".to_string(),
            "schema_version = 1\n[unrelated]\nkey = 1\n".to_string(),
            "schema_version = 1\nname = \"another tool\"\n".to_string(),
            "schema_version = \"1\"\n".to_string(),
        ] {
            fs::write(store.path(), &content).unwrap();
            assert_eq!(
                store.retire_recognized_v1().unwrap(),
                V1Retirement::NotRecognized,
                "{content:?}"
            );
            assert_eq!(fs::read_to_string(store.path()).unwrap(), content);
        }

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::write(store.path(), v1_config(&root.path().join("home"))).unwrap();
            fs::set_permissions(store.path(), fs::Permissions::from_mode(0o000)).unwrap();
            // A process that cannot read the file cannot recognize it and
            // leaves it in place; root can read it, so it is a genuine v1.
            match store.retire_recognized_v1().unwrap() {
                V1Retirement::NotRecognized => {
                    assert!(store.path().exists());
                    fs::set_permissions(store.path(), fs::Permissions::from_mode(0o600)).unwrap();
                }
                V1Retirement::Retired(_) => assert!(!store.path().exists()),
                other => panic!("unexpected {other:?}"),
            }

            let target = root.path().join("linked-v1.toml");
            fs::write(&target, v1_config(&root.path().join("home"))).unwrap();
            fs::remove_file(store.path()).ok();
            std::os::unix::fs::symlink(&target, store.path()).unwrap();
            assert_eq!(
                store.retire_recognized_v1().unwrap(),
                V1Retirement::NotRecognized
            );
            assert!(target.exists() && store.path().exists());
        }
    }

    #[test]
    fn an_unrecognized_v1_document_explains_how_to_start_over() {
        let (_root, store) = fixture();
        fs::create_dir_all(store.path().parent().unwrap()).unwrap();
        let content = "schema_version = 1\nhand_added = \"keep\"\n";
        fs::write(store.path(), content).unwrap();
        assert_eq!(
            store.retire_recognized_v1().unwrap(),
            V1Retirement::NotRecognized
        );
        let loaded = store.load();
        assert_eq!(loaded.status, LoadStatus::UnsupportedVersion);
        let message = &loaded.validation[0].message;
        assert!(message.contains("earlier CCResDoc"), "{message}");
        assert!(
            message.contains(&store.path().display().to_string()),
            "{message}"
        );
        assert_eq!(fs::read_to_string(store.path()).unwrap(), content);
    }

    #[test]
    fn retirement_restores_a_file_that_changed_after_recognition() {
        let (root, store) = fixture();
        fs::create_dir_all(store.path().parent().unwrap()).unwrap();
        fs::write(store.path(), v1_config(&root.path().join("home"))).unwrap();
        let replacement = "schema_version = 2\n[appearance]\nmode = \"light\"\n";
        let store = store.with_before_replace(move |path| {
            fs::write(path, replacement).unwrap();
        });
        assert_eq!(store.retire_recognized_v1().unwrap(), V1Retirement::Changed);
        assert_eq!(fs::read_to_string(store.path()).unwrap(), replacement);
        assert_eq!(
            fs::read_dir(store.path().parent().unwrap())
                .unwrap()
                .count(),
            1,
            "the aside file is removed after restoring"
        );
    }

    #[test]
    fn all_resource_selection_combinations_project_only_enabled_paths() {
        let (root, store) = fixture();
        fs::create_dir_all(root.path().join("home/.codex")).unwrap();
        for (claude, codex) in [(true, false), (false, true), (true, true), (false, false)] {
            let draft = SettingsDraft {
                claude_resources: claude,
                codex_resources: codex,
                ..SettingsDraft::defaults()
            };
            let (effective, diagnostics) = store.validate(&draft);
            assert!(diagnostics.iter().all(|diagnostic| !diagnostic.blocking));
            assert_eq!(effective.claude_resources, claude);
            assert_eq!(effective.codex_resources, codex);
            assert_eq!(effective.claude_dir.is_some(), claude);
            assert_eq!(effective.codex_dir.is_some(), codex);
        }
    }

    #[test]
    fn disabled_sources_preserve_invalid_authored_paths_without_validation() {
        let (_root, store) = fixture();
        let draft = SettingsDraft {
            claude_resources: false,
            codex_resources: false,
            claude_dir: "relative/claude".into(),
            codex_dir: "/missing/codex".into(),
            ..SettingsDraft::defaults()
        };
        let (effective, diagnostics) = store.validate(&draft);
        assert!(diagnostics.iter().all(|diagnostic| !diagnostic.blocking));
        assert_eq!(effective.claude_dir, None);
        assert_eq!(effective.codex_dir, None);

        for (enabled, field) in [
            (
                SettingsDraft {
                    claude_resources: true,
                    ..draft.clone()
                },
                "source.claude_dir",
            ),
            (
                SettingsDraft {
                    codex_resources: true,
                    ..draft.clone()
                },
                "source.codex_dir",
            ),
        ] {
            assert!(store.validate(&enabled).1.iter().any(|diagnostic| {
                diagnostic.blocking && diagnostic.field.as_deref() == Some(field)
            }));
        }
    }

    #[test]
    fn enabled_source_cannot_be_home() {
        let (_root, store) = fixture();
        let draft = SettingsDraft {
            claude_dir: "~".into(),
            ..SettingsDraft::defaults()
        };
        assert!(store.validate(&draft).1.iter().any(|diagnostic| {
            diagnostic.blocking
                && diagnostic.field.as_deref() == Some("source.claude_dir")
                && diagnostic.message.contains("narrower than HOME")
        }));
    }

    #[test]
    fn regular_and_inline_resource_tables_are_losslessly_updated() {
        let (root, store) = fixture();
        fs::create_dir_all(store.path().parent().unwrap()).unwrap();
        fs::create_dir_all(root.path().join("home/.codex")).unwrap();
        for original in [
            "schema_version = 2\n[resources]\nclaude = true # keep claude\ncodex = false\nfuture = \"keep\"\n[source]\nclaude_dir = \"~/.claude\"\ncodex_dir = \"~/.codex\"\n",
            "schema_version = 2\nresources = { claude = true, codex = false, future = \"keep\" } # resources note\nsource = { claude_dir = \"~/.claude\", codex_dir = \"~/.codex\", future = \"keep\" } # source note\n",
        ] {
            fs::write(store.path(), original).unwrap();
            let loaded = store.load();
            assert_eq!(loaded.raw_content.as_deref(), Some(original));
            let mut draft = loaded.authored;
            draft.codex_resources = true;
            store.save(&draft, loaded.revision.as_ref()).unwrap();
            let raw = fs::read_to_string(store.path()).unwrap();
            assert!(raw.contains("codex = true"));
            assert!(raw.contains("future = \"keep\""));
            assert!(raw.contains("keep claude") || raw.contains("resources note"));
        }
    }

    #[test]
    fn malformed_has_location_and_requires_explicit_replacement() {
        let (_root, store) = fixture();
        fs::create_dir_all(store.path().parent().unwrap()).unwrap();
        fs::write(store.path(), "schema_version = 2\n[source\nnope").unwrap();
        let loaded = store.load();
        assert_eq!(loaded.status, LoadStatus::Malformed);
        assert!(loaded.validation[0].location.is_some());
        assert!(matches!(
            store.save(&loaded.authored, loaded.revision.as_ref()),
            Err(SaveError::Malformed)
        ));
        let replaced = store
            .replace_malformed(&loaded.authored, loaded.revision.as_ref().unwrap())
            .unwrap();
        assert_eq!(replaced.snapshot.status, LoadStatus::Valid);
    }

    #[cfg(unix)]
    #[test]
    fn unreadable_config_is_distinct_and_untouched() {
        use std::os::unix::fs::PermissionsExt;
        let (_root, store) = fixture();
        fs::create_dir_all(store.path().parent().unwrap()).unwrap();
        fs::write(store.path(), "schema_version = 2\n").unwrap();
        fs::set_permissions(store.path(), fs::Permissions::from_mode(0o000)).unwrap();
        let loaded = store.load();
        assert_eq!(loaded.status, LoadStatus::Unreadable);
        assert!(loaded.file_exists);
        assert!(matches!(
            store.save(&loaded.authored, None),
            Err(SaveError::Unreadable(_))
        ));
        assert_eq!(fs::metadata(store.path()).unwrap().len(), 19);
        fs::set_permissions(store.path(), fs::Permissions::from_mode(0o600)).unwrap();
    }

    #[test]
    fn future_version_cannot_be_saved_or_replaced() {
        let (_root, store) = fixture();
        fs::create_dir_all(store.path().parent().unwrap()).unwrap();
        fs::write(store.path(), "schema_version = 4\nfuture = true\n").unwrap();
        let loaded = store.load();
        assert_eq!(loaded.status, LoadStatus::UnsupportedVersion);
        assert!(matches!(
            store.save(&loaded.authored, loaded.revision.as_ref()),
            Err(SaveError::UnsupportedVersion(4))
        ));
        assert!(matches!(
            store.replace_malformed(&loaded.authored, loaded.revision.as_ref().unwrap()),
            Err(SaveError::ReplacementNotAllowed)
        ));
    }

    #[test]
    fn semantic_errors_can_be_repaired() {
        let (_root, store) = fixture();
        fs::create_dir_all(store.path().parent().unwrap()).unwrap();
        fs::write(store.path(), "schema_version = 2\n[source]\nclaude_dir = \"relative\"\n[appearance]\nmode = \"sepia\"\n[server]\npreferred_port = 0\n").unwrap();
        let loaded = store.load();
        assert_eq!(loaded.status, LoadStatus::Invalid);
        assert!(!loaded.active.uses_authored_settings);
        let mut repaired = loaded.authored.clone();
        repaired.claude_dir = "~/.claude".into();
        repaired.appearance_mode = "system".into();
        repaired.preferred_port = 4892;
        assert_eq!(
            store
                .save(&repaired, loaded.revision.as_ref())
                .unwrap()
                .snapshot
                .status,
            LoadStatus::Valid
        );
    }

    #[test]
    fn invalid_types_paths_and_unknown_theme_are_diagnostic() {
        let (root, store) = fixture();
        fs::create_dir_all(store.path().parent().unwrap()).unwrap();
        fs::write(root.path().join("plain-file"), "x").unwrap();
        fs::write(store.path(), format!("schema_version = 2\n[source]\nclaude_dir = {:?}\n[appearance]\nmode = 7\ntheme_pack = \"lost\"\n[server]\npreferred_port = 70000\n", root.path().join("plain-file").to_string_lossy())).unwrap();
        let loaded = store.load();
        assert_eq!(loaded.status, LoadStatus::Invalid);
        assert!(loaded
            .validation
            .iter()
            .any(|d| d.kind == DiagnosticKind::InvalidType));
        assert!(loaded
            .validation
            .iter()
            .any(|d| d.kind == DiagnosticKind::InvalidSourcePath));
        assert!(loaded
            .validation
            .iter()
            .any(|d| d.kind == DiagnosticKind::InvalidPort));
        assert!(loaded
            .validation
            .iter()
            .any(|d| d.kind == DiagnosticKind::ThemePackUnavailable));
    }

    #[test]
    fn invalid_section_type_is_diagnostic_and_repairable() {
        let (_root, store) = fixture();
        fs::create_dir_all(store.path().parent().unwrap()).unwrap();
        fs::write(
            store.path(),
            "schema_version = 2\nsource = \"not a table\" # keep note\n",
        )
        .unwrap();
        let loaded = store.load();
        assert_eq!(loaded.status, LoadStatus::Invalid);
        assert!(loaded.validation.iter().any(|diagnostic| {
            diagnostic.kind == DiagnosticKind::InvalidType
                && diagnostic.field.as_deref() == Some("source")
        }));
        store
            .save(&loaded.authored, loaded.revision.as_ref())
            .unwrap();
        assert!(fs::read_to_string(store.path())
            .unwrap()
            .contains("# keep note"));
    }

    #[test]
    fn inline_tables_retain_unknown_keys_and_comments_on_save() {
        let (_root, store) = fixture();
        fs::create_dir_all(store.path().parent().unwrap()).unwrap();
        fs::write(
            store.path(),
            "schema_version = 2\nsource = { claude_dir = \"~/.claude\", future = \"keep\" } # source note\nappearance = { mode = \"system\", theme_pack = \"classic\" }\nserver = { preferred_port = 4892, fallback_to_free_port = true }\n",
        )
        .unwrap();
        let loaded = store.load();
        assert_eq!(loaded.status, LoadStatus::Valid);
        let mut draft = loaded.authored;
        draft.appearance_mode = "dark".into();
        store.save(&draft, loaded.revision.as_ref()).unwrap();
        let raw = fs::read_to_string(store.path()).unwrap();
        assert!(raw.contains("future = \"keep\""));
        assert!(raw.contains("# source note"));
        assert!(raw.contains("mode = \"dark\""));
    }

    #[test]
    fn unknown_theme_is_valid_authored_data_with_effective_fallback() {
        let (_root, store) = fixture();
        fs::create_dir_all(store.path().parent().unwrap()).unwrap();
        fs::write(
            store.path(),
            "schema_version = 2\n[appearance]\ntheme_pack = \"uninstalled-pack\"\n",
        )
        .unwrap();
        let loaded = store.load();
        assert_eq!(loaded.status, LoadStatus::Valid);
        assert!(loaded.active.uses_authored_settings);
        assert_eq!(loaded.authored.theme_pack, "uninstalled-pack");
        assert_eq!(loaded.effective.theme_pack, DEFAULT_THEME_PACK);
        assert!(loaded.validation.iter().any(|diagnostic| {
            diagnostic.kind == DiagnosticKind::ThemePackUnavailable && !diagnostic.blocking
        }));
    }

    #[test]
    fn tilde_and_symlink_are_normalized() {
        let (root, store) = fixture();
        #[cfg(unix)]
        std::os::unix::fs::symlink(
            root.path().join("home/.claude"),
            root.path().join("home/link"),
        )
        .unwrap();
        fs::create_dir_all(store.path().parent().unwrap()).unwrap();
        fs::write(
            store.path(),
            "schema_version = 2\n[source]\nclaude_dir = \"~/link\"\n",
        )
        .unwrap();
        assert_eq!(
            store.load().effective.claude_dir,
            Some(fs::canonicalize(root.path().join("home/.claude")).unwrap())
        );
    }

    #[cfg(unix)]
    #[test]
    fn unreadable_source_is_rejected_even_when_tests_run_as_root() {
        use std::os::unix::fs::PermissionsExt;
        let (root, store) = fixture();
        let locked = root.path().join("locked");
        fs::create_dir(&locked).unwrap();
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o000)).unwrap();
        fs::create_dir_all(store.path().parent().unwrap()).unwrap();
        fs::write(
            store.path(),
            format!(
                "schema_version = 2\n[source]\nclaude_dir = {:?}\n",
                locked.to_string_lossy()
            ),
        )
        .unwrap();
        assert!(store
            .load()
            .validation
            .iter()
            .any(|d| d.kind == DiagnosticKind::UnreadableSourcePath));
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o700)).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn unreadable_disabled_source_is_nonblocking_until_enabled() {
        use std::os::unix::fs::PermissionsExt;
        let (root, store) = fixture();
        let locked = root.path().join("locked-codex");
        fs::create_dir(&locked).unwrap();
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o000)).unwrap();
        let mut draft = SettingsDraft {
            codex_dir: locked.to_string_lossy().into_owned(),
            ..SettingsDraft::defaults()
        };
        assert!(store
            .validate(&draft)
            .1
            .iter()
            .all(|diagnostic| diagnostic.field.as_deref() != Some("source.codex_dir")));
        draft.codex_resources = true;
        assert!(store.validate(&draft).1.iter().any(|diagnostic| {
            diagnostic.blocking
                && diagnostic.kind == DiagnosticKind::UnreadableSourcePath
                && diagnostic.field.as_deref() == Some("source.codex_dir")
        }));
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o700)).unwrap();
    }

    #[test]
    fn exact_bytes_detect_conflict_even_with_same_mtime() {
        let (root, store) = fixture();
        fs::create_dir_all(store.path().parent().unwrap()).unwrap();
        let first = valid_toml(root.path().join("home").as_path());
        fs::write(store.path(), &first).unwrap();
        let loaded = store.load();
        let stamp = FileTime::from_last_modification_time(&fs::metadata(store.path()).unwrap());
        let second = first.replace("mode = \"dark\"", "mode = \"lite\"");
        assert_eq!(first.len(), second.len());
        fs::write(store.path(), second).unwrap();
        set_file_mtime(store.path(), stamp).unwrap();
        assert!(matches!(
            store.save(&loaded.authored, loaded.revision.as_ref()),
            Err(SaveError::RevisionConflict { .. })
        ));
    }

    #[test]
    fn save_preserves_comments_unknown_keys_permissions_and_newline() {
        #[cfg(unix)]
        use std::os::unix::fs::PermissionsExt;
        let (root, store) = fixture();
        fs::create_dir_all(store.path().parent().unwrap()).unwrap();
        let content = format!("# human note\nschema_version = 2\nunknown = \"keep\"\n[source]\nclaude_dir = {:?}\n[appearance]\nmode = \"system\" # inline\ntheme_pack = \"classic\"\n[server]\npreferred_port = 4892\nfallback_to_free_port = true\n", root.path().join("home/.claude").to_string_lossy());
        fs::write(store.path(), content).unwrap();
        #[cfg(unix)]
        fs::set_permissions(store.path(), fs::Permissions::from_mode(0o640)).unwrap();
        let loaded = store.load();
        let mut draft = loaded.authored;
        draft.appearance_mode = "dark".into();
        store.save(&draft, loaded.revision.as_ref()).unwrap();
        let raw = fs::read_to_string(store.path()).unwrap();
        assert!(raw.contains("# human note"));
        assert!(raw.contains("unknown = \"keep\""));
        assert!(raw.contains("mode = \"dark\" # inline"));
        assert!(raw.ends_with('\n'));
        #[cfg(unix)]
        assert_eq!(
            fs::metadata(store.path()).unwrap().permissions().mode() & 0o777,
            0o640
        );
    }

    #[test]
    fn first_write_is_restrictive_and_leaves_no_temp_files() {
        #[cfg(unix)]
        use std::os::unix::fs::PermissionsExt;
        let (_root, store) = fixture();
        store.save(&SettingsDraft::defaults(), None).unwrap();
        #[cfg(unix)]
        assert_eq!(
            fs::metadata(store.path()).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert_eq!(
            fs::read_dir(store.path().parent().unwrap())
                .unwrap()
                .count(),
            1
        );
    }

    #[cfg(unix)]
    #[test]
    fn save_atomically_replaces_instead_of_truncating_in_place() {
        use std::io::Read;
        let (_root, store) = fixture();
        fs::create_dir_all(store.path().parent().unwrap()).unwrap();
        fs::write(store.path(), "schema_version = 2\n# old inode\n").unwrap();
        let loaded = store.load();
        let mut old_handle = File::open(store.path()).unwrap();
        let mut draft = loaded.authored;
        draft.appearance_mode = "dark".into();
        store.save(&draft, loaded.revision.as_ref()).unwrap();
        let mut old_bytes = String::new();
        old_handle.read_to_string(&mut old_bytes).unwrap();
        assert_eq!(old_bytes, "schema_version = 2\n# old inode\n");
        assert!(fs::read_to_string(store.path())
            .unwrap()
            .contains("mode = \"dark\""));
    }

    #[test]
    fn temporary_file_names_are_unique() {
        let root = tempfile::tempdir().unwrap();
        let (first_path, first) =
            create_unique_temp(root.path(), OsStr::new("config.toml")).unwrap();
        let (second_path, second) =
            create_unique_temp(root.path(), OsStr::new("config.toml")).unwrap();
        assert_ne!(first_path, second_path);
        drop((first, second));
        fs::remove_file(first_path).unwrap();
        fs::remove_file(second_path).unwrap();
    }

    #[test]
    fn dirty_rebase_preserves_latest_untouched_fields_comments_and_unknowns() {
        let (root, store) = fixture();
        fs::create_dir_all(store.path().parent().unwrap()).unwrap();
        fs::write(store.path(), valid_toml(root.path().join("home").as_path())).unwrap();
        let stale = store.load();
        let external = fs::read_to_string(store.path())
            .unwrap()
            .replace("preferred_port = 5000", "# external\npreferred_port = 6000")
            + "extra = true\n";
        fs::write(store.path(), external).unwrap();
        let mut draft = stale.authored.clone();
        draft.appearance_mode = "light".into();
        let dirty = [SettingField::AppearanceMode].into_iter().collect();
        let result = store
            .rebase_dirty(&draft, &dirty, stale.revision.as_ref().unwrap())
            .unwrap();
        assert!(result.rebased);
        let raw = fs::read_to_string(store.path()).unwrap();
        assert!(raw.contains("preferred_port = 6000"));
        assert!(raw.contains("# external"));
        assert!(raw.contains("extra = true"));
        assert!(raw.contains("mode = \"light\""));
    }

    #[test]
    fn dirty_rebase_keeps_external_resource_fields_independent() {
        let (root, store) = fixture();
        fs::create_dir_all(store.path().parent().unwrap()).unwrap();
        let external_codex = root.path().join("external-codex");
        fs::create_dir(&external_codex).unwrap();
        fs::write(store.path(), valid_toml(root.path().join("home").as_path())).unwrap();
        let stale = store.load();
        let external = fs::read_to_string(store.path())
            .unwrap()
            .replace("claude = true", "claude = false # external toggle")
            .replace(
                "codex_dir = \"~/.codex\"",
                &format!("codex_dir = {:?}", external_codex.to_string_lossy()),
            );
        fs::write(store.path(), external).unwrap();

        let mut draft = stale.authored.clone();
        draft.codex_resources = true;
        draft.claude_dir = "/draft/claude".into();
        let dirty = [SettingField::CodexResources, SettingField::ClaudeDir]
            .into_iter()
            .collect();
        let result = store
            .rebase_dirty(&draft, &dirty, stale.revision.as_ref().unwrap())
            .unwrap();
        assert_eq!(result.impact, ApplyImpact::RestartRuntime);
        let raw = fs::read_to_string(store.path()).unwrap();
        assert!(raw.contains("claude = false # external toggle"));
        assert!(raw.contains("codex = true"));
        assert!(raw.contains("claude_dir = \"/draft/claude\""));
        assert!(raw.contains(&external_codex.to_string_lossy().to_string()));
    }

    #[test]
    fn resource_fields_have_restart_impact_and_no_ops_do_not() {
        let draft = SettingsDraft::defaults();
        for field in [
            SettingField::ClaudeResources,
            SettingField::CodexResources,
            SettingField::ClaudeDir,
            SettingField::CodexDir,
        ] {
            let mut changed = draft.clone();
            match field {
                SettingField::ClaudeResources => changed.claude_resources = false,
                SettingField::CodexResources => changed.codex_resources = true,
                SettingField::ClaudeDir => changed.claude_dir = "/changed/claude".into(),
                SettingField::CodexDir => changed.codex_dir = "/changed/codex".into(),
                _ => unreachable!(),
            }
            assert_eq!(
                impact_between(&draft, &changed, &[field].into_iter().collect()),
                ApplyImpact::RestartRuntime
            );
            assert_eq!(
                impact_between(&draft, &draft, &[field].into_iter().collect()),
                ApplyImpact::None
            );
        }
    }

    #[test]
    fn save_preserves_crlf_newline_style() {
        let (_root, store) = fixture();
        fs::create_dir_all(store.path().parent().unwrap()).unwrap();
        fs::write(
            store.path(),
            "schema_version = 2\r\n[resources]\r\nclaude = true\r\ncodex = false\r\n",
        )
        .unwrap();
        let loaded = store.load();
        let mut draft = loaded.authored;
        draft.appearance_mode = "dark".into();
        store.save(&draft, loaded.revision.as_ref()).unwrap();
        let bytes = fs::read(store.path()).unwrap();
        assert!(bytes.windows(2).any(|pair| pair == b"\r\n"));
        assert!(!bytes.windows(3).any(|window| window == b"\r\r\n"));
        assert!(!bytes
            .iter()
            .enumerate()
            .any(|(index, byte)| *byte == b'\n' && (index == 0 || bytes[index - 1] != b'\r')));
    }

    #[test]
    fn second_conflict_during_rebase_is_safe_and_temp_is_cleaned() {
        let (root, base_store) = fixture();
        fs::create_dir_all(base_store.path().parent().unwrap()).unwrap();
        fs::write(
            base_store.path(),
            valid_toml(root.path().join("home").as_path()),
        )
        .unwrap();
        let stale = base_store.load();
        fs::write(
            base_store.path(),
            fs::read_to_string(base_store.path())
                .unwrap()
                .replace("5000", "6000"),
        )
        .unwrap();
        let path = base_store.path().to_path_buf();
        let hook_path = path.clone();
        let fired = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let fired_hook = fired.clone();
        let store = SettingsStore::new(path.clone(), root.path().join("home")).with_before_replace(
            move |_| {
                if !fired_hook.swap(true, Ordering::SeqCst) {
                    fs::write(
                        &hook_path,
                        fs::read_to_string(&hook_path)
                            .unwrap()
                            .replace("6000", "7000"),
                    )
                    .unwrap();
                }
            },
        );
        let mut draft = stale.authored.clone();
        draft.appearance_mode = "light".into();
        let dirty = [SettingField::AppearanceMode].into_iter().collect();
        assert!(matches!(
            store.rebase_dirty(&draft, &dirty, stale.revision.as_ref().unwrap()),
            Err(SaveError::RevisionConflict { .. })
        ));
        assert!(fs::read_to_string(&path).unwrap().contains("7000"));
        assert_eq!(fs::read_dir(path.parent().unwrap()).unwrap().count(), 1);
    }

    #[test]
    fn appearance_update_first_write_and_retry_preserve_latest_nonappearance_data() {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir_all(root.path().join("home/.claude")).unwrap();
        let path = root.path().join("config/config.toml");
        let store = SettingsStore::with_theme_packs(
            path.clone(),
            root.path().join("home"),
            ["classic", "graphite"],
        );
        let first = store
            .update_appearance(Some(AppearanceMode::Dark), Some("graphite"))
            .unwrap();
        assert_eq!(first.impact, ApplyImpact::AppearanceOnly);
        assert_eq!(first.snapshot.authored.appearance_mode, "dark");
        assert_eq!(first.snapshot.authored.theme_pack, "graphite");

        let mut raw = fs::read_to_string(&path).unwrap();
        raw.push_str("\n# external edit\nfuture_key = \"keep\"\n");
        fs::write(&path, raw).unwrap();
        let hook_path = path.clone();
        let fired = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let fired_hook = fired.clone();
        let racing = SettingsStore::with_theme_packs(
            path.clone(),
            root.path().join("home"),
            ["classic", "graphite"],
        )
        .with_before_replace(move |_| {
            if !fired_hook.swap(true, Ordering::SeqCst) {
                let latest = fs::read_to_string(&hook_path).unwrap();
                fs::write(
                    &hook_path,
                    latest.replace("preferred_port = 4892", "preferred_port = 6001"),
                )
                .unwrap();
            }
        });
        let saved = racing
            .update_appearance(Some(AppearanceMode::Light), Some("classic"))
            .unwrap();
        assert_eq!(saved.snapshot.authored.preferred_port, 6001);
        let final_raw = fs::read_to_string(&path).unwrap();
        assert!(final_raw.contains("future_key = \"keep\""));
        assert!(final_raw.contains("mode = \"light\""));
    }

    #[test]
    fn mode_only_quick_update_preserves_unavailable_authored_theme() {
        let (root, store) = fixture();
        fs::create_dir_all(store.path().parent().unwrap()).unwrap();
        fs::write(
            store.path(),
            format!(
                "schema_version = 2\n[source]\nclaude_dir = {:?}\n[appearance]\nmode = \"system\"\ntheme_pack = \"not-installed\"\n[server]\npreferred_port = 4892\nfallback_to_free_port = true\n",
                root.path().join("home/.claude").to_string_lossy()
            ),
        )
        .unwrap();
        let result = store
            .update_appearance(Some(AppearanceMode::Dark), None)
            .unwrap();
        assert_eq!(result.snapshot.authored.appearance_mode, "dark");
        assert_eq!(result.snapshot.authored.theme_pack, "not-installed");
        assert_eq!(result.snapshot.effective.theme_pack, DEFAULT_THEME_PACK);
    }

    #[test]
    fn rebase_requires_staleness_and_a_latest_valid_supported_document() {
        let (root, store) = fixture();
        fs::create_dir_all(store.path().parent().unwrap()).unwrap();
        fs::write(store.path(), valid_toml(root.path().join("home").as_path())).unwrap();
        let current = store.load();
        let dirty = [SettingField::AppearanceMode].into_iter().collect();
        assert!(matches!(
            store.rebase_dirty(
                &current.authored,
                &dirty,
                current.revision.as_ref().unwrap()
            ),
            Err(SaveError::NotStale)
        ));

        let stale_revision = current.revision.unwrap();
        fs::write(store.path(), "schema_version = 2\n[broken\n").unwrap();
        assert!(matches!(
            store.rebase_dirty(&current.authored, &dirty, &stale_revision),
            Err(SaveError::LatestNotValid)
        ));
        fs::write(store.path(), "schema_version = 4\n").unwrap();
        assert!(matches!(
            store.rebase_dirty(&current.authored, &dirty, &stale_revision),
            Err(SaveError::LatestNotValid)
        ));
    }

    #[test]
    fn command_catalog_defaults_and_native_conversion_share_one_contract() {
        let catalog = browser_command_catalog();
        assert_eq!(catalog.version, COMMAND_CATALOG_VERSION);
        assert_eq!(catalog.commands.len(), 8);
        assert_eq!(default_shortcut_entries()[0].bindings, ["Mod+["]);
        for command in catalog.commands {
            for binding in command.default_bindings {
                let normalized = normalize_shortcut_binding(&binding).unwrap();
                assert_eq!(normalized.as_portable_string(), binding);
                assert_eq!(
                    normalized.to_tauri_accelerator(),
                    binding.replacen("Mod", "CmdOrCtrl", 1)
                );
            }
        }
    }

    #[test]
    fn shortcut_parser_normalizes_and_rejects_v1_ambiguities() {
        assert_eq!(
            normalize_shortcut_binding("shift+mod+r")
                .unwrap()
                .as_portable_string(),
            "Mod+Shift+R"
        );
        assert_eq!(
            normalize_shortcut_binding("F12")
                .unwrap()
                .as_portable_string(),
            "F12"
        );
        for invalid in [
            "K",
            "Space",
            "Mod",
            "Mod+",
            "Mod+K Mod+C",
            "Mod+K, Mod+C",
            "Meta+K",
            "Mod+Mod+K",
            "Mod+Ctrl+K",
            "Mod+Unknown",
        ] {
            assert!(
                normalize_shortcut_binding(invalid).is_err(),
                "{invalid} must be rejected"
            );
        }
    }

    #[test]
    fn shortcut_validation_names_command_and_reserved_conflicts() {
        let (_root, store) = fixture();
        let mut draft = SettingsDraft::defaults();
        draft
            .shortcuts
            .iter_mut()
            .find(|entry| entry.command_id == "home")
            .unwrap()
            .bindings = vec!["Mod+F".into(), "Ctrl+K".into()];
        let (_, diagnostics) = store.validate(&draft);
        assert!(diagnostics.iter().any(|diagnostic| {
            diagnostic.kind == DiagnosticKind::ShortcutConflict
                && diagnostic.field.as_deref() == Some("shortcuts.home")
                && diagnostic.message.contains("Find in Page")
        }));
        assert!(diagnostics.iter().any(|diagnostic| {
            diagnostic.kind == DiagnosticKind::ShortcutConflict
                && diagnostic.message.contains("Search Documentation")
        }));

        draft
            .shortcuts
            .iter_mut()
            .find(|entry| entry.command_id == "home")
            .unwrap()
            .bindings = vec!["Mod+,".into()];
        assert!(store
            .validate(&draft)
            .1
            .iter()
            .any(|diagnostic| { diagnostic.message.contains("reserved action Settings") }));

        draft
            .shortcuts
            .iter_mut()
            .find(|entry| entry.command_id == "home")
            .unwrap()
            .bindings = vec!["Mod+Alt+I".into()];
        assert!(store.validate(&draft).1.iter().any(|diagnostic| {
            diagnostic
                .message
                .contains("reserved action Toggle Developer Tools")
        }));
    }

    #[test]
    fn optional_shortcuts_persist_normalized_known_values_and_preserve_unknown_data() {
        let (root, store) = fixture();
        fs::create_dir_all(store.path().parent().unwrap()).unwrap();
        let original = format!(
            "{}\n[shortcuts]\nback = [\"alt+left\"] # known\nfuture_command_v2 = {{ chord = [\"Mod+K\", \"Mod+C\"] }} # untouched\nfuture_array_id = [\"future+syntax\"]\n",
            valid_toml(root.path().join("home").as_path())
        );
        fs::write(store.path(), &original).unwrap();
        let loaded = store.load();
        assert_eq!(loaded.status, LoadStatus::Valid);
        assert!(loaded.authored.shortcuts.iter().any(|entry| {
            entry.command_id == "future_array_id" && entry.bindings == ["future+syntax"]
        }));
        let result = store
            .save(&loaded.authored, loaded.revision.as_ref())
            .unwrap();
        assert_eq!(result.impact, ApplyImpact::None);
        let saved = fs::read_to_string(store.path()).unwrap();
        assert!(saved.contains("back = [\"Alt+ArrowLeft\"] # known"));
        assert!(
            saved.contains("future_command_v2 = { chord = [\"Mod+K\", \"Mod+C\"] } # untouched")
        );
        assert!(saved.contains("future_array_id = [\"future+syntax\"]"));
        assert!(result.snapshot.authored.shortcuts.iter().any(|entry| {
            entry.command_id == "future_array_id" && entry.bindings == ["future+syntax"]
        }));
    }

    #[test]
    fn shortcut_rebase_and_malformed_replacement_keep_opaque_ids_restart_free() {
        let (root, store) = fixture();
        fs::create_dir_all(store.path().parent().unwrap()).unwrap();
        fs::write(store.path(), valid_toml(root.path().join("home").as_path())).unwrap();
        let stale = store.load();
        let mut draft = stale.authored.clone();
        draft.shortcuts.push(ShortcutEntry {
            command_id: "future_command_id".into(),
            bindings: vec!["future+binding".into()],
        });
        draft
            .shortcuts
            .iter_mut()
            .find(|entry| entry.command_id == "home")
            .unwrap()
            .bindings = vec!["Alt+Home".into()];
        fs::write(
            store.path(),
            format!(
                "{}\n[future]\nexternal = \"preserve\"\n",
                valid_toml(root.path().join("home").as_path())
            ),
        )
        .unwrap();
        let result = store
            .rebase_dirty(
                &draft,
                &[SettingField::Shortcuts].into_iter().collect(),
                stale.revision.as_ref().unwrap(),
            )
            .unwrap();
        assert_eq!(result.impact, ApplyImpact::None);
        let rebased = fs::read_to_string(store.path()).unwrap();
        assert!(rebased.contains("external = \"preserve\""));
        assert!(rebased.contains("future_command_id = [\"future+binding\"]"));

        fs::write(store.path(), "schema_version = 2\n[broken\n").unwrap();
        let malformed = store.load();
        let replaced = store
            .replace_malformed(&draft, malformed.revision.as_ref().unwrap())
            .unwrap();
        assert!(replaced.snapshot.authored.shortcuts.iter().any(|entry| {
            entry.command_id == "future_command_id" && entry.bindings == ["future+binding"]
        }));
    }

    #[test]
    fn invalid_known_shortcut_toml_is_field_addressable_but_unknown_values_are_ignored() {
        let (root, store) = fixture();
        fs::create_dir_all(store.path().parent().unwrap()).unwrap();
        fs::write(
            store.path(),
            format!(
                "{}\n[shortcuts]\nback = \"Mod+[\"\nunknown_future = 42\n",
                valid_toml(root.path().join("home").as_path())
            ),
        )
        .unwrap();
        let loaded = store.load();
        assert_eq!(loaded.status, LoadStatus::Invalid);
        assert!(loaded.validation.iter().any(|diagnostic| {
            diagnostic.field.as_deref() == Some("shortcuts.back")
                && diagnostic.kind == DiagnosticKind::InvalidType
        }));
        assert!(!loaded
            .validation
            .iter()
            .any(|diagnostic| { diagnostic.field.as_deref() == Some("shortcuts.unknown_future") }));
    }

    #[test]
    fn wire_types_have_stable_serialization_fixture() {
        let (_root, store) = fixture();
        let snapshot = store.load();
        let wire = serde_json::to_value(&snapshot).unwrap();
        assert_eq!(wire["status"], "missing");
        assert_eq!(wire["authored"]["appearance_mode"], "system");
        assert_eq!(wire["authored"]["claude_resources"], true);
        assert_eq!(wire["authored"]["codex_resources"], false);
        assert_eq!(wire["authored"]["codex_dir"], "~/.codex");
        assert_eq!(wire["effective"]["claude_resources"], true);
        assert_eq!(wire["effective"]["codex_resources"], false);
        assert_eq!(wire["effective"]["codex_dir"], serde_json::Value::Null);
        assert_eq!(wire["effective"]["preferred_port"], 4892);
        assert_eq!(wire["active"]["effective_port"], 4892);
        assert_eq!(wire["effective"]["shortcuts"][0]["commandId"], "back");
        assert_eq!(
            serde_json::to_value(SettingField::ClaudeDir).unwrap(),
            "claude_dir"
        );
        assert_eq!(
            serde_json::to_value(SettingField::CodexResources).unwrap(),
            "codex_resources"
        );
        assert_eq!(
            serde_json::to_value(ApplyImpact::RestartRuntime).unwrap(),
            "restart_runtime"
        );
        let diagnostic = diagnostic(
            DiagnosticKind::InvalidPort,
            Some("server.preferred_port"),
            "bad".into(),
            true,
        );
        assert_eq!(
            serde_json::to_value(diagnostic).unwrap()["kind"],
            "invalid_port"
        );
        let result = SaveResult {
            snapshot,
            impact: ApplyImpact::None,
            rebased: false,
        };
        assert_eq!(serde_json::to_value(result).unwrap()["rebased"], false);
        assert_eq!(
            serde_json::to_value(SettingsDraft::defaults()).unwrap(),
            serde_json::json!({
                "schema_version": 3,
                "claude_resources": true,
                "codex_resources": false,
                "claude_dir": "~/.claude",
                "codex_dir": "~/.codex",
                "appearance_mode": "system",
                "theme_pack": "classic",
                "preferred_port": 4892,
                "fallback_to_free_port": true,
                "shortcuts": default_shortcut_entries()
            })
        );
        let validation = vec![SettingsDiagnostic {
            kind: DiagnosticKind::MalformedSyntax,
            field: None,
            message: "bad TOML".into(),
            blocking: true,
            location: Some(SourceLocation { line: 2, column: 3 }),
        }];
        let validation_wire = serde_json::to_value(validation).unwrap();
        assert_eq!(validation_wire[0]["location"]["line"], 2);

        let opaque = ShortcutEntry {
            command_id: "future_command_id".into(),
            bindings: vec!["future+syntax".into()],
        };
        let opaque_wire = serde_json::to_value(&opaque).unwrap();
        assert_eq!(opaque_wire["commandId"], "future_command_id");
        assert_eq!(
            serde_json::from_value::<ShortcutEntry>(opaque_wire).unwrap(),
            opaque
        );

        let mut legacy_wire = serde_json::to_value(SettingsDraft::defaults()).unwrap();
        legacy_wire.as_object_mut().unwrap().remove("shortcuts");
        assert_eq!(
            serde_json::from_value::<SettingsDraft>(legacy_wire)
                .unwrap()
                .shortcuts,
            default_shortcut_entries()
        );
    }

    #[test]
    fn v2_migration_is_atomic_idempotent_and_preserves_disabled_paths() {
        let (_root, store) = fixture();
        fs::create_dir_all(store.path().parent().unwrap()).unwrap();
        let old = "# owner note\nschema_version = 2\ncustom = 'keep'\n[resources]\nclaude = false\ncodex = false\n[source]\nclaude_dir = '/missing/claude'\ncodex_dir = '/missing/codex'\n[appearance]\nmode = 'dark'\n[server]\npreferred_port = 5900\n";
        fs::write(store.path(), old).unwrap();
        let migrated = store.migrate_v2().unwrap();
        assert_eq!(migrated.authored.schema_version, 3);
        assert_eq!(migrated.source_catalog.sources.len(), 2);
        assert_eq!(migrated.source_catalog.sources[0].root, "/missing/claude");
        assert_eq!(migrated.source_catalog.sources[1].root, "/missing/codex");
        assert!(!migrated.source_catalog.sources[0].enabled);
        assert_eq!(migrated.authored.appearance_mode, "dark");
        assert_eq!(migrated.authored.preferred_port, 5900);
        let first = fs::read(store.path()).unwrap();
        assert!(String::from_utf8_lossy(&first).contains("# owner note"));
        assert!(String::from_utf8_lossy(&first).contains("custom = 'keep'"));
        store.migrate_v2().unwrap();
        assert_eq!(fs::read(store.path()).unwrap(), first);
    }

    #[test]
    fn v2_migration_preserves_enabled_missing_root_for_native_recovery() {
        let (_root, store) = fixture();
        fs::create_dir_all(store.path().parent().unwrap()).unwrap();
        let old =
            "schema_version = 2\n[source]\nclaude_dir = '/missing/ccresdoc-migration-recovery'\n";
        fs::write(store.path(), old).unwrap();
        assert_eq!(store.load().status, LoadStatus::Invalid);
        let migrated = store.migrate_v2().unwrap();
        assert_eq!(migrated.authored.schema_version, CURRENT_SCHEMA_VERSION);
        assert_eq!(
            migrated.source_catalog.sources[0].root,
            "/missing/ccresdoc-migration-recovery"
        );
        assert!(migrated.source_catalog.sources[0].enabled);
        let once = fs::read(store.path()).unwrap();
        store.migrate_v2().unwrap();
        assert_eq!(fs::read(store.path()).unwrap(), once);
    }

    #[test]
    fn scoped_app_write_preserves_source_table_bytes_and_source_rebase_is_per_id() {
        let (root, store) = fixture();
        let first = store
            .save_source_catalog(&SourceCatalog::defaults(), None)
            .unwrap()
            .snapshot;
        let before = fs::read_to_string(store.path()).unwrap();
        let mut app = first.authored.clone();
        app.appearance_mode = "dark".into();
        let fields = [SettingField::AppearanceMode].into_iter().collect();
        let saved = store
            .save_app_fields(&app, &fields, first.revision.as_ref())
            .unwrap()
            .snapshot;
        let after = fs::read_to_string(store.path()).unwrap();
        assert_eq!(
            before.split("[[document_sources]]").nth(1),
            after.split("[[document_sources]]").nth(1)
        );
        let base = saved.source_catalog.clone();
        let mut draft = base.clone();
        draft.sources[0].label = "Local Claude".into();
        let mut concurrent = base.clone();
        concurrent.sources[1].label = "Work Codex".into();
        let current = store
            .save_source_catalog(&concurrent, saved.revision.as_ref())
            .unwrap()
            .snapshot;
        let merged = store
            .rebase_source_catalog(&draft, &base, saved.revision.as_ref().unwrap())
            .unwrap();
        assert!(merged.rebased);
        assert_eq!(
            merged.snapshot.source_catalog.sources[0].label,
            "Local Claude"
        );
        assert_eq!(
            merged.snapshot.source_catalog.sources[1].label,
            "Work Codex"
        );
        let mut collision = base.clone();
        collision.sources[1].label = "Conflicting Codex".into();
        assert!(matches!(
            store.rebase_source_catalog(&collision, &base, saved.revision.as_ref().unwrap()),
            Err(SaveError::RevisionConflict { .. })
        ));
        assert!(root.path().join("home/.claude").exists());
        assert_eq!(current.source_catalog.sources[1].label, "Work Codex");
    }

    #[test]
    fn persisted_source_that_disappears_keeps_identity_for_recovery_watch() {
        let (root, store) = fixture();
        let disappearing = root.path().join("disappearing");
        fs::create_dir(&disappearing).unwrap();
        let mut catalog = SourceCatalog::defaults();
        catalog.sources[0].root = disappearing.to_string_lossy().into_owned();
        store.save_source_catalog(&catalog, None).unwrap();
        fs::remove_dir(&disappearing).unwrap();
        let loaded = store.load();
        assert_eq!(loaded.status, LoadStatus::Valid);
        assert_eq!(loaded.effective.sources[0].id, "legacy-claude");
        assert_eq!(loaded.effective.sources[0].root, disappearing);
        assert!(loaded
            .validation
            .iter()
            .any(
                |diagnostic| diagnostic.field.as_deref() == Some("sources.legacy-claude.root")
                    && !diagnostic.blocking
            ));
        assert!(store
            .validate_source_catalog(&catalog)
            .iter()
            .any(|diagnostic| diagnostic.blocking));
    }

    #[test]
    fn protected_configs_never_fall_back_to_scanning_default_sources() {
        let (_root, store) = fixture();
        fs::create_dir_all(store.path().parent().unwrap()).unwrap();
        for (raw, status) in [
            ("schema_version = 4\n", LoadStatus::UnsupportedVersion),
            ("schema_version = 3\n[broken\n", LoadStatus::Malformed),
            (
                "schema_version = 3\n[[document_sources]]\nid = 'bad'\n",
                LoadStatus::Invalid,
            ),
        ] {
            fs::write(store.path(), raw).unwrap();
            let snapshot = store.load();
            assert_eq!(snapshot.status, status);
            assert!(snapshot.effective.sources.is_empty());
            assert_eq!(fs::read_to_string(store.path()).unwrap(), raw);
        }
    }
}
