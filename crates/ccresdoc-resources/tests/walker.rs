//! Integration tests for the ccresdoc-resources walker.
//!
//! Each test builds a minimal fixture tree in a `tempfile::TempDir` and
//! asserts walker output.  No real `$HOME/.claude` is touched.

use std::fs;
use std::os::unix::fs::symlink;
use std::path::Path;

use ccresdoc_resources::{walk_claude_dir, ResourceError};

// ---------------------------------------------------------------------------
// Helper: write file, creating parent dirs as needed
// ---------------------------------------------------------------------------

fn write(base: &Path, rel: &str, content: &str) {
    let full = base.join(rel);
    if let Some(parent) = full.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(full, content).unwrap();
}

// ---------------------------------------------------------------------------
// Test 1: root CLAUDE.md is found and slug is "root"
// ---------------------------------------------------------------------------

#[test]
fn test_root_claude_md() {
    let tmp = tempfile::TempDir::new().unwrap();
    let claude_dir = tmp.path();

    write(claude_dir, "CLAUDE.md", "# Root instructions\n\nHello.");

    let tree = walk_claude_dir(claude_dir, claude_dir).unwrap();

    assert_eq!(tree.claude_mds.len(), 1);
    let item = &tree.claude_mds[0];
    assert_eq!(item.slug, "root");
    assert_eq!(item.display_path, "/CLAUDE.md");
    assert_eq!(item.rel_path, "CLAUDE.md");
    assert!(item.raw_content.contains("Root instructions"));
}

// ---------------------------------------------------------------------------
// Test 2: nested CLAUDE.md produces correct slug and display_path
// ---------------------------------------------------------------------------

#[test]
fn test_nested_claude_md_slug() {
    let tmp = tempfile::TempDir::new().unwrap();
    let claude_dir = tmp.path();

    write(claude_dir, "CLAUDE.md", "root");
    write(claude_dir, "some/nested/CLAUDE.md", "nested");

    let tree = walk_claude_dir(claude_dir, claude_dir).unwrap();

    // root must come first
    assert_eq!(tree.claude_mds[0].slug, "root");

    let nested = tree.claude_mds
        .iter()
        .find(|i| i.slug != "root")
        .expect("nested item not found");
    assert_eq!(nested.slug, "some--nested");
    assert_eq!(nested.display_path, "/some/nested/CLAUDE.md");
}

// ---------------------------------------------------------------------------
// Test 3: commands — frontmatter description extracted, body is remainder
// ---------------------------------------------------------------------------

#[test]
fn test_commands() {
    let tmp = tempfile::TempDir::new().unwrap();
    let claude_dir = tmp.path();

    write(
        claude_dir,
        "commands/my-cmd.md",
        "---\ndescription: Does things\n---\n\nActual body here.",
    );

    let tree = walk_claude_dir(claude_dir, claude_dir).unwrap();

    assert_eq!(tree.commands.len(), 1);
    let cmd = &tree.commands[0];
    assert_eq!(cmd.name, "my-cmd");
    assert_eq!(cmd.description, "Does things");
    assert!(cmd.raw_content.contains("Actual body here."));
    // body must NOT contain the frontmatter delimiters
    assert!(!cmd.raw_content.contains("---"));
}

// ---------------------------------------------------------------------------
// Test 4: skills — SKILL.md, one reference, one .md script, one binary script
// ---------------------------------------------------------------------------

#[test]
fn test_skills_full() {
    let tmp = tempfile::TempDir::new().unwrap();
    let claude_dir = tmp.path();

    // SKILL.md
    write(
        claude_dir,
        "skills/my-skill/SKILL.md",
        "---\nname: My Skill\ndescription: A test skill\n---\n\nSkill body.",
    );
    // reference
    write(
        claude_dir,
        "skills/my-skill/references/ref-a.md",
        "# Reference A\n\nReference content.",
    );
    // markdown script
    write(
        claude_dir,
        "skills/my-skill/scripts/run.md",
        "# Run Script\n\nScript body.",
    );
    // binary script (non-markdown)
    write(claude_dir, "skills/my-skill/scripts/run.sh", "#!/bin/sh\necho hi");

    let tree = walk_claude_dir(claude_dir, claude_dir).unwrap();

    assert_eq!(tree.skills.len(), 1);
    let skill = &tree.skills[0];

    assert_eq!(skill.name, "My Skill");
    assert_eq!(skill.dir, "my-skill");
    assert_eq!(skill.description, "A test skill");
    assert!(skill.raw_content.contains("Skill body."));

    // references
    assert_eq!(skill.references.len(), 1);
    assert_eq!(skill.references[0].name, "ref-a");
    assert_eq!(skill.references[0].title, "Reference A");
    assert!(skill.references[0].raw_content.contains("Reference content."));

    // script files: .md and .sh
    assert_eq!(skill.script_files.len(), 2);
    let md_script = skill.script_files.iter().find(|f| f.filename == "run.md").unwrap();
    assert!(md_script.is_markdown);
    assert_eq!(md_script.title.as_deref(), Some("Run Script"));
    assert!(md_script.raw_content.as_deref().unwrap().contains("Script body."));

    let sh_script = skill.script_files.iter().find(|f| f.filename == "run.sh").unwrap();
    assert!(!sh_script.is_markdown);
    assert!(sh_script.raw_content.is_none());
}

// ---------------------------------------------------------------------------
// Test 5: agents — frontmatter name/model/description extracted
// ---------------------------------------------------------------------------

#[test]
fn test_agents() {
    let tmp = tempfile::TempDir::new().unwrap();
    let claude_dir = tmp.path();

    write(
        claude_dir,
        "agents/code-review.md",
        "---\nname: Code Reviewer\ndescription: Reviews code\nmodel: claude-opus-4\n---\n\nAgent instructions.",
    );

    let tree = walk_claude_dir(claude_dir, claude_dir).unwrap();

    assert_eq!(tree.agents.len(), 1);
    let agent = &tree.agents[0];
    assert_eq!(agent.name, "Code Reviewer");
    assert_eq!(agent.file_slug, "code-review");
    assert_eq!(agent.description, "Reviews code");
    assert_eq!(agent.model, "claude-opus-4");
    assert!(agent.raw_content.contains("Agent instructions."));
}

// ---------------------------------------------------------------------------
// Test 6: broken symlinks do NOT panic
// ---------------------------------------------------------------------------

#[test]
fn test_broken_symlink_no_panic() {
    let tmp = tempfile::TempDir::new().unwrap();
    let claude_dir = tmp.path();

    write(claude_dir, "CLAUDE.md", "root");

    // Create a broken symlink inside claude_dir
    let broken = claude_dir.join("broken-link");
    symlink("/this/does/not/exist/at/all", &broken).unwrap();

    // Must not panic
    let tree = walk_claude_dir(claude_dir, claude_dir).unwrap();

    // The CLAUDE.md should still be found
    assert!(!tree.claude_mds.is_empty());
}

// ---------------------------------------------------------------------------
// Test 7: project_root = $HOME returns Err
// ---------------------------------------------------------------------------

#[test]
fn test_project_root_home_returns_err() {
    let home = std::env::var_os("HOME").expect("HOME not set");
    let home_path = std::path::PathBuf::from(home);

    // claude_dir doesn't need to exist for this guard to fire
    let result = walk_claude_dir(&home_path.join(".claude"), &home_path);

    match result {
        Err(ResourceError::ProjectRootTooBoard(_)) => { /* expected */ }
        other => panic!("expected ProjectRootTooBoard, got {:?}", other),
    }
}

// ---------------------------------------------------------------------------
// Test 8: exclude dirs — .git, node_modules, worktrees skipped
// ---------------------------------------------------------------------------

#[test]
fn test_exclude_dirs_not_walked() {
    let tmp = tempfile::TempDir::new().unwrap();
    let claude_dir = tmp.path();

    write(claude_dir, "CLAUDE.md", "root");
    // These should be skipped
    write(claude_dir, ".git/CLAUDE.md", "should be excluded");
    write(claude_dir, "node_modules/CLAUDE.md", "should be excluded");
    write(claude_dir, "worktrees/CLAUDE.md", "should be excluded");
    // This should be found
    write(claude_dir, "real-subdir/CLAUDE.md", "subdir");

    let tree = walk_claude_dir(claude_dir, claude_dir).unwrap();

    assert_eq!(tree.claude_mds.len(), 2, "expected root + real-subdir only");
    assert!(tree.claude_mds.iter().all(|i| !i.rel_path.starts_with(".git")));
    assert!(tree.claude_mds.iter().all(|i| !i.rel_path.starts_with("node_modules")));
    assert!(tree.claude_mds.iter().all(|i| !i.rel_path.starts_with("worktrees")));
}

// ---------------------------------------------------------------------------
// Test 9: sorting — commands and agents sorted by name; root-first for claude_mds
// ---------------------------------------------------------------------------

#[test]
fn test_sorting() {
    let tmp = tempfile::TempDir::new().unwrap();
    let claude_dir = tmp.path();

    // Two CLAUDE.mds
    write(claude_dir, "CLAUDE.md", "root");
    write(claude_dir, "zzz/CLAUDE.md", "z subdir");
    write(claude_dir, "aaa/CLAUDE.md", "a subdir");

    // Commands out of alphabetical order
    write(claude_dir, "commands/zebra.md", "---\ndescription: z\n---\nz body");
    write(claude_dir, "commands/alpha.md", "---\ndescription: a\n---\na body");

    // Agents out of order
    write(claude_dir, "agents/z-agent.md", "---\nname: Z Agent\n---\nbody");
    write(claude_dir, "agents/a-agent.md", "---\nname: A Agent\n---\nbody");

    let tree = walk_claude_dir(claude_dir, claude_dir).unwrap();

    // claude_mds: root first
    assert_eq!(tree.claude_mds[0].slug, "root");
    // then alphabetical by display_path
    assert!(tree.claude_mds[1].display_path < tree.claude_mds[2].display_path);

    // commands: alphabetical
    assert_eq!(tree.commands[0].name, "alpha");
    assert_eq!(tree.commands[1].name, "zebra");

    // agents: alphabetical by name field
    assert_eq!(tree.agents[0].name, "A Agent");
    assert_eq!(tree.agents[1].name, "Z Agent");
}

// ---------------------------------------------------------------------------
// Test 10: skill name falls back to dir name when frontmatter has no `name`
// ---------------------------------------------------------------------------

#[test]
fn test_skill_name_fallback() {
    let tmp = tempfile::TempDir::new().unwrap();
    let claude_dir = tmp.path();

    // SKILL.md without a `name` field
    write(
        claude_dir,
        "skills/my-dir/SKILL.md",
        "---\ndescription: no name field\n---\n\nBody.",
    );

    let tree = walk_claude_dir(claude_dir, claude_dir).unwrap();

    assert_eq!(tree.skills.len(), 1);
    assert_eq!(tree.skills[0].name, "my-dir"); // fallback to dir name
}
