//! Versioned, human-editable settings and a revision-guarded durable TOML store.
//!
//! This module intentionally has no Tauri dependencies. The host and settings
//! window can share the serializable domain types without coupling storage to
//! a window or application lifecycle.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::ffi::{OsStr, OsString};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use thiserror::Error;
use toml_edit::{value, DocumentMut, Item, Table};

pub const CURRENT_SCHEMA_VERSION: i64 = 1;
pub const DEFAULT_PORT: u16 = 4892;
pub const DEFAULT_THEME_PACK: &str = "default";

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
    fn parse(value: &str) -> Option<Self> {
        match value {
            "system" => Some(Self::System),
            "light" => Some(Self::Light),
            "dark" => Some(Self::Dark),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SettingsDraft {
    pub schema_version: i64,
    pub claude_dir: String,
    pub appearance_mode: String,
    pub theme_pack: String,
    pub preferred_port: i64,
    pub fallback_to_free_port: bool,
}

impl SettingsDraft {
    pub fn defaults() -> Self {
        Self {
            schema_version: CURRENT_SCHEMA_VERSION,
            claude_dir: "~/.claude".into(),
            appearance_mode: "system".into(),
            theme_pack: DEFAULT_THEME_PACK.into(),
            preferred_port: i64::from(DEFAULT_PORT),
            fallback_to_free_port: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EffectiveSettings {
    pub claude_dir: PathBuf,
    pub appearance_mode: AppearanceMode,
    pub theme_pack: String,
    pub preferred_port: u16,
    pub effective_port: u16,
    pub fallback_to_free_port: bool,
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
    pub effective: EffectiveSettings,
    pub active: ActiveState,
    pub validation: Vec<SettingsDiagnostic>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SettingField {
    ClaudeDir,
    AppearanceMode,
    ThemePack,
    PreferredPort,
    FallbackToFreePort,
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
        self.ensure_valid_draft(draft)?;

        let mut doc = match self.read_state() {
            ReadState::Missing => DocumentMut::new(),
            ReadState::Present { doc: Ok(doc), .. } => *doc,
            ReadState::Present { doc: Err(_), .. } => return Err(SaveError::Malformed),
            ReadState::Unreadable(error) => return Err(SaveError::Unreadable(error.to_string())),
        };
        merge_fields(&mut doc, draft, &SettingField::all());
        let impact = impact_between(&before.authored, draft, &SettingField::all());
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
                self.ensure_valid_draft(&candidate)?;
                let impact = impact_between(&latest.authored, &candidate, dirty_fields);
                merge_fields(&mut doc, &candidate, dirty_fields);
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
        self.ensure_valid_draft(draft)?;
        let mut doc = DocumentMut::new();
        merge_fields(&mut doc, draft, &SettingField::all());
        self.write_document(&doc, Some(expected_revision))?;
        Ok(SaveResult {
            snapshot: self.load(),
            impact: ApplyImpact::RestartRuntime,
            rebased: false,
        })
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
        let effective = self.project_defaults();
        SettingsSnapshot {
            config_path: self.path.clone(),
            file_exists,
            status,
            revision,
            raw_content,
            authored,
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
            if version != CURRENT_SCHEMA_VERSION {
                return self.default_snapshot(
                    LoadStatus::UnsupportedVersion,
                    true,
                    revision,
                    raw_content,
                    vec![diagnostic(
                        DiagnosticKind::UnsupportedSchemaVersion,
                        Some("schema_version"),
                        format!("schema version {version} is unsupported"),
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
        if validate_section(doc, "source", &mut diagnostics) {
            read_string(
                doc,
                &["source", "claude_dir"],
                &mut authored.claude_dir,
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

        let (effective, mut semantic) = self.validate_and_project(&authored);
        diagnostics.append(&mut semantic);
        let blocking = diagnostics.iter().any(|d| d.blocking);
        let status = if blocking {
            LoadStatus::Invalid
        } else {
            LoadStatus::Valid
        };
        let uses_authored = !blocking;
        let active_effective = if uses_authored {
            effective.clone()
        } else {
            self.project_defaults()
        };
        SettingsSnapshot {
            config_path: self.path.clone(),
            file_exists: true,
            status,
            revision,
            raw_content,
            authored,
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
            claude_dir: self.home.join(".claude"),
            appearance_mode: AppearanceMode::System,
            theme_pack: DEFAULT_THEME_PACK.into(),
            preferred_port: DEFAULT_PORT,
            effective_port: DEFAULT_PORT,
            fallback_to_free_port: true,
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
        let source = match normalize_source(&draft.claude_dir, &self.home) {
            Ok(path) => path,
            Err((kind, message)) => {
                diagnostics.push(diagnostic(kind, Some("source.claude_dir"), message, true));
                self.home.join(".claude")
            }
        };
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
        (
            EffectiveSettings {
                claude_dir: source,
                appearance_mode: mode,
                theme_pack,
                preferred_port: port,
                effective_port: port,
                fallback_to_free_port: draft.fallback_to_free_port,
            },
            diagnostics,
        )
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
        let mut rendered = doc.to_string();
        while rendered.ends_with('\n') {
            rendered.pop();
        }
        rendered.push('\n');

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
    fn all() -> BTreeSet<Self> {
        [
            Self::ClaudeDir,
            Self::AppearanceMode,
            Self::ThemePack,
            Self::PreferredPort,
            Self::FallbackToFreePort,
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
        if metadata.permissions().mode() & 0o500 == 0 {
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
    fs::canonicalize(&expanded).map_err(|error| {
        (
            DiagnosticKind::InvalidSourcePath,
            format!("source directory cannot be normalized: {error}"),
        )
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
    if fields.contains(&SettingField::ClaudeDir) {
        set_section_value(doc, "source", "claude_dir", value(&draft.claude_dir));
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
            SettingField::ClaudeDir => target.claude_dir.clone_from(&source.claude_dir),
            SettingField::AppearanceMode => {
                target.appearance_mode.clone_from(&source.appearance_mode)
            }
            SettingField::ThemePack => target.theme_pack.clone_from(&source.theme_pack),
            SettingField::PreferredPort => target.preferred_port = source.preferred_port,
            SettingField::FallbackToFreePort => {
                target.fallback_to_free_port = source.fallback_to_free_port
            }
        }
    }
}

fn impact_between(
    before: &SettingsDraft,
    after: &SettingsDraft,
    fields: &BTreeSet<SettingField>,
) -> ApplyImpact {
    let restart = (fields.contains(&SettingField::ClaudeDir)
        && before.claude_dir != after.claude_dir)
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
        format!("schema_version = 1\n\n[source]\nclaude_dir = {:?}\n\n[appearance]\nmode = \"dark\"\ntheme_pack = \"default\"\n\n[server]\npreferred_port = 5000\nfallback_to_free_port = false\n", home.join(".claude").to_string_lossy())
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
    fn missing_load_does_not_create_any_paths() {
        let (_root, store) = fixture();
        let snapshot = store.load();
        assert_eq!(snapshot.status, LoadStatus::Missing);
        assert!(!snapshot.file_exists);
        assert!(!store.path().exists());
        assert!(!store.path().parent().unwrap().exists());
    }

    #[test]
    fn valid_partial_and_round_trip() {
        let (root, store) = fixture();
        fs::create_dir_all(store.path().parent().unwrap()).unwrap();
        fs::write(
            store.path(),
            "schema_version = 1\n\n[appearance]\nmode = \"light\"\n",
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
            fs::canonicalize(root.path().join("home/.claude")).unwrap()
        );
        assert!(fs::read_to_string(store.path()).unwrap().ends_with('\n'));
    }

    #[test]
    fn malformed_has_location_and_requires_explicit_replacement() {
        let (_root, store) = fixture();
        fs::create_dir_all(store.path().parent().unwrap()).unwrap();
        fs::write(store.path(), "schema_version = 1\n[source\nnope").unwrap();
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
        fs::write(store.path(), "schema_version = 1\n").unwrap();
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
        fs::write(store.path(), "schema_version = 2\nfuture = true\n").unwrap();
        let loaded = store.load();
        assert_eq!(loaded.status, LoadStatus::UnsupportedVersion);
        assert!(matches!(
            store.save(&loaded.authored, loaded.revision.as_ref()),
            Err(SaveError::UnsupportedVersion(2))
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
        fs::write(store.path(), "schema_version = 1\n[source]\nclaude_dir = \"relative\"\n[appearance]\nmode = \"sepia\"\n[server]\npreferred_port = 0\n").unwrap();
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
        fs::write(store.path(), format!("schema_version = 1\n[source]\nclaude_dir = {:?}\n[appearance]\nmode = 7\ntheme_pack = \"lost\"\n[server]\npreferred_port = 70000\n", root.path().join("plain-file").to_string_lossy())).unwrap();
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
            "schema_version = 1\nsource = \"not a table\" # keep note\n",
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
            "schema_version = 1\nsource = { claude_dir = \"~/.claude\", future = \"keep\" } # source note\nappearance = { mode = \"system\", theme_pack = \"default\" }\nserver = { preferred_port = 4892, fallback_to_free_port = true }\n",
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
            "schema_version = 1\n[appearance]\ntheme_pack = \"uninstalled-pack\"\n",
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
            "schema_version = 1\n[source]\nclaude_dir = \"~/link\"\n",
        )
        .unwrap();
        assert_eq!(
            store.load().effective.claude_dir,
            fs::canonicalize(root.path().join("home/.claude")).unwrap()
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
                "schema_version = 1\n[source]\nclaude_dir = {:?}\n",
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
        let content = format!("# human note\nschema_version = 1\nunknown = \"keep\"\n[source]\nclaude_dir = {:?}\n[appearance]\nmode = \"system\" # inline\ntheme_pack = \"default\"\n[server]\npreferred_port = 4892\nfallback_to_free_port = true\n", root.path().join("home/.claude").to_string_lossy());
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
        fs::write(store.path(), "schema_version = 1\n# old inode\n").unwrap();
        let loaded = store.load();
        let mut old_handle = File::open(store.path()).unwrap();
        let mut draft = loaded.authored;
        draft.appearance_mode = "dark".into();
        store.save(&draft, loaded.revision.as_ref()).unwrap();
        let mut old_bytes = String::new();
        old_handle.read_to_string(&mut old_bytes).unwrap();
        assert_eq!(old_bytes, "schema_version = 1\n# old inode\n");
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
        fs::write(store.path(), "schema_version = 1\n[broken\n").unwrap();
        assert!(matches!(
            store.rebase_dirty(&current.authored, &dirty, &stale_revision),
            Err(SaveError::LatestNotValid)
        ));
        fs::write(store.path(), "schema_version = 2\n").unwrap();
        assert!(matches!(
            store.rebase_dirty(&current.authored, &dirty, &stale_revision),
            Err(SaveError::LatestNotValid)
        ));
    }

    #[test]
    fn wire_types_have_stable_serialization_fixture() {
        let (_root, store) = fixture();
        let snapshot = store.load();
        let wire = serde_json::to_value(&snapshot).unwrap();
        assert_eq!(wire["status"], "missing");
        assert_eq!(wire["authored"]["appearance_mode"], "system");
        assert_eq!(wire["effective"]["preferred_port"], 4892);
        assert_eq!(wire["active"]["effective_port"], 4892);
        assert_eq!(
            serde_json::to_value(SettingField::ClaudeDir).unwrap(),
            "claude_dir"
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
                "schema_version": 1,
                "claude_dir": "~/.claude",
                "appearance_mode": "system",
                "theme_pack": "default",
                "preferred_port": 4892,
                "fallback_to_free_port": true
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
    }
}
