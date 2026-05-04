//! Manifest JSON builder — produces the shape consumed by ccresdoc-sidebar.js.
//!
//! The manifest must never include shell-related entries (paths starting with `/_shell`).

use chrono::Utc;
use ccresdoc_resources::ResourceTree;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ManifestItem {
    pub slug: String,
    pub label: String,
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ManifestCategory {
    pub slug: String,
    pub label: String,
    pub items: Vec<ManifestItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Manifest {
    pub generated_at: String,
    pub categories: Vec<ManifestCategory>,
}

impl Manifest {
    /// Build a `Manifest` from the walker output.
    ///
    /// Shell paths (`/_shell/…`) are explicitly excluded — they must never
    /// appear in sidebar navigation.
    pub fn build(tree: &ResourceTree) -> Self {
        let generated_at = Utc::now().to_rfc3339();

        // --- CLAUDE.md items ---
        let claude_md_items: Vec<ManifestItem> = tree
            .claude_mds
            .iter()
            .map(|item| ManifestItem {
                slug: item.slug.clone(),
                label: item.display_path.clone(),
                path: format!("/claude-md/{}", item.slug),
            })
            .collect();

        // --- Command items ---
        let command_items: Vec<ManifestItem> = tree
            .commands
            .iter()
            .map(|cmd| ManifestItem {
                slug: cmd.name.clone(),
                label: cmd.name.clone(),
                path: format!("/claude-commands/{}", cmd.name),
            })
            .collect();

        // --- Skill items ---
        let skill_items: Vec<ManifestItem> = tree
            .skills
            .iter()
            .map(|skill| ManifestItem {
                slug: skill.dir.clone(),
                label: skill.name.clone(),
                path: format!("/claude-skills/{}", skill.dir),
            })
            .collect();

        // --- Agent items ---
        let agent_items: Vec<ManifestItem> = tree
            .agents
            .iter()
            .map(|agent| ManifestItem {
                slug: agent.file_slug.clone(),
                label: agent.name.clone(),
                path: format!("/claude-agents/{}", agent.file_slug),
            })
            .collect();

        Manifest {
            generated_at,
            categories: vec![
                ManifestCategory {
                    slug: "claude-md".into(),
                    label: "CLAUDE.md".into(),
                    items: claude_md_items,
                },
                ManifestCategory {
                    slug: "claude-commands".into(),
                    label: "Commands".into(),
                    items: command_items,
                },
                ManifestCategory {
                    slug: "claude-skills".into(),
                    label: "Skills".into(),
                    items: skill_items,
                },
                ManifestCategory {
                    slug: "claude-agents".into(),
                    label: "Agents".into(),
                    items: agent_items,
                },
            ],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ccresdoc_resources::{ClaudeMdItem, CommandItem, ResourceTree};

    fn make_tree() -> ResourceTree {
        ResourceTree {
            claude_mds: vec![ClaudeMdItem {
                display_path: "/CLAUDE.md".into(),
                slug: "root".into(),
                rel_path: "CLAUDE.md".into(),
                raw_content: "# Hello".into(),
            }],
            commands: vec![CommandItem {
                name: "my-cmd".into(),
                description: "does stuff".into(),
                raw_content: "body".into(),
            }],
            skills: vec![],
            agents: vec![],
        }
    }

    #[test]
    fn manifest_has_four_categories() {
        let tree = make_tree();
        let manifest = Manifest::build(&tree);
        assert_eq!(manifest.categories.len(), 4);
    }

    #[test]
    fn manifest_no_shell_entry() {
        let tree = make_tree();
        let manifest = Manifest::build(&tree);
        for cat in &manifest.categories {
            for item in &cat.items {
                assert!(
                    !item.path.starts_with("/_shell"),
                    "manifest item path must not start with /_shell: {}",
                    item.path
                );
            }
        }
    }

    #[test]
    fn manifest_category_slugs() {
        let tree = make_tree();
        let manifest = Manifest::build(&tree);
        let slugs: Vec<&str> = manifest.categories.iter().map(|c| c.slug.as_str()).collect();
        assert_eq!(slugs, ["claude-md", "claude-commands", "claude-skills", "claude-agents"]);
    }

    #[test]
    fn manifest_claude_md_path() {
        let tree = make_tree();
        let manifest = Manifest::build(&tree);
        let cat = manifest.categories.iter().find(|c| c.slug == "claude-md").unwrap();
        assert_eq!(cat.items[0].path, "/claude-md/root");
    }

    #[test]
    fn manifest_serializes_camel_case() {
        let tree = make_tree();
        let manifest = Manifest::build(&tree);
        let json = serde_json::to_string(&manifest).unwrap();
        assert!(json.contains("generatedAt"), "should use camelCase generatedAt");
    }
}
