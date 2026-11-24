//! Persisted document-source definitions. Paths belong to source instances;
//! reusable presets deliberately contain only reading rules.

use serde::{Deserialize, Serialize};

pub const BUILTIN_RULE_VERSION: u32 = 1;
pub const MANAGEMENT_MOUNT: &str = "ccresdoc";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct SourceCatalog {
    #[serde(default)]
    pub sources: Vec<DocumentSource>,
    #[serde(default)]
    pub presets: Vec<UserPreset>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocumentSource {
    pub id: String,
    pub label: String,
    pub enabled: bool,
    /// Authored absolute path or ~/ path. Retained when disabled.
    pub root: String,
    pub scan_root: ScanRootChoice,
    /// Single lowercase route segment. Editing a label never changes this.
    pub mount: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin_preset: Option<PresetRef>,
    pub rules: RuleSnapshot,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ScanRootChoice {
    Same,
    Separate { path: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PresetRef {
    pub id: String,
    pub name: String,
    pub version: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UserPreset {
    pub id: String,
    pub name: String,
    pub description: String,
    pub version: u32,
    pub rules: RuleSnapshot,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PublicFormat {
    Markdown,
    Code,
    Skill,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RuleSnapshot {
    ClaudeCode {
        definition_version: u32,
    },
    Codex {
        definition_version: u32,
    },
    Custom {
        definition_version: u32,
        format: PublicFormat,
        include: Vec<String>,
        exclude: Vec<String>,
    },
}

impl SourceCatalog {
    pub fn defaults() -> Self {
        Self {
            sources: vec![
                DocumentSource {
                    id: "legacy-claude".into(),
                    label: "Claude Code".into(),
                    enabled: true,
                    root: "~/.claude".into(),
                    scan_root: ScanRootChoice::Same,
                    mount: "claude".into(),
                    origin_preset: Some(PresetRef {
                        id: "builtin-claude-code".into(),
                        name: "Claude Code".into(),
                        version: BUILTIN_RULE_VERSION,
                    }),
                    rules: RuleSnapshot::ClaudeCode {
                        definition_version: BUILTIN_RULE_VERSION,
                    },
                },
                DocumentSource {
                    id: "legacy-codex".into(),
                    label: "Codex".into(),
                    enabled: false,
                    root: "~/.codex".into(),
                    scan_root: ScanRootChoice::Same,
                    mount: "codex".into(),
                    origin_preset: Some(PresetRef {
                        id: "builtin-codex".into(),
                        name: "Codex".into(),
                        version: BUILTIN_RULE_VERSION,
                    }),
                    rules: RuleSnapshot::Codex {
                        definition_version: BUILTIN_RULE_VERSION,
                    },
                },
            ],
            presets: Vec::new(),
        }
    }
}

pub fn builtin_presets() -> Vec<UserPreset> {
    vec![
        UserPreset {
            id: "builtin-claude-code".into(),
            name: "Claude Code".into(),
            description: "Claude Code instructions, commands, agents and skills".into(),
            version: BUILTIN_RULE_VERSION,
            rules: RuleSnapshot::ClaudeCode {
                definition_version: BUILTIN_RULE_VERSION,
            },
        },
        UserPreset {
            id: "builtin-codex".into(),
            name: "Codex".into(),
            description: "Codex instructions, configuration, agents, hooks, rules and skills"
                .into(),
            version: BUILTIN_RULE_VERSION,
            rules: RuleSnapshot::Codex {
                definition_version: BUILTIN_RULE_VERSION,
            },
        },
        UserPreset {
            id: "builtin-markdown".into(),
            name: "Markdown".into(),
            description: "Markdown files in a folder".into(),
            version: BUILTIN_RULE_VERSION,
            rules: RuleSnapshot::Custom {
                definition_version: BUILTIN_RULE_VERSION,
                format: PublicFormat::Markdown,
                include: vec!["**/*.md".into(), "**/*.mdx".into()],
                exclude: Vec::new(),
            },
        },
        UserPreset {
            id: "builtin-code".into(),
            name: "Code and text".into(),
            description: "Selected text and code files in a folder".into(),
            version: BUILTIN_RULE_VERSION,
            rules: RuleSnapshot::Custom {
                definition_version: BUILTIN_RULE_VERSION,
                format: PublicFormat::Code,
                include: vec!["**/*.txt".into(), "**/*.ts".into(), "**/*.rs".into()],
                exclude: Vec::new(),
            },
        },
    ]
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EffectiveSource {
    pub id: String,
    pub label: String,
    pub root: std::path::PathBuf,
    pub scan_root: std::path::PathBuf,
    pub mount: String,
    pub rules: RuleSnapshot,
}
