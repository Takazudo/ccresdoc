//! Live smoke test: generate MDX against the real `$HOME/.claude`, writing into
//! a TEMP output dir. Skips gracefully when `$HOME/.claude` is absent (CI).
//!
//! This is the acceptance smoke check: generating against the real `~/.claude`
//! produces browsable MDX for the CLAUDE.md hierarchy, commands, skills and
//! agents, with the contract's category index pages.

use std::path::PathBuf;

use ccresdoc_claude_md::{generate, Config};

#[test]
fn live_generate_against_real_claude_dir() {
    let home = match std::env::var_os("HOME") {
        Some(h) => PathBuf::from(h),
        None => {
            eprintln!("SKIP: HOME not set");
            return;
        }
    };
    let claude = home.join(".claude");
    if !claude.exists() {
        eprintln!("SKIP: $HOME/.claude does not exist");
        return;
    }

    let out = tempfile::TempDir::new().unwrap();
    let config = Config {
        claude_dir: claude.clone(),
        project_root: claude.clone(),
        docs_dir: out.path().to_path_buf(),
    };

    let report = generate(&config).expect("live generate failed");
    eprintln!(
        "live generate counts: claude_md={}, commands={}, skills={}, agents={}",
        report.claude_md, report.commands, report.skills, report.agents
    );

    // All four families should produce content on a real dev box.
    assert!(report.claude_md > 0, "expected >=1 CLAUDE.md");
    assert!(report.commands > 0, "expected >=1 command");
    assert!(report.skills > 0, "expected >=1 skill");
    assert!(report.agents > 0, "expected >=1 agent");

    // Contract category index pages exist with the right positions.
    let overview = std::fs::read_to_string(out.path().join("claude/index.mdx")).unwrap();
    assert!(overview.contains("sidebar_position: 899"));
    assert!(overview.contains("<CategoryNav categories="));

    for (sub, pos) in [
        ("claude-md", "900"),
        ("claude-commands", "901"),
        ("claude-skills", "902"),
        ("claude-agents", "903"),
    ] {
        let idx = out.path().join(sub).join("index.mdx");
        assert!(idx.exists(), "{sub}/index.mdx must exist");
        let content = std::fs::read_to_string(&idx).unwrap();
        assert!(
            content.contains(&format!("sidebar_position: {pos}")),
            "{sub}/index.mdx must have sidebar_position {pos}"
        );
        assert!(content.contains("category_no_page: true"));
    }

    // Sanity: the global CLAUDE.md page exists.
    assert!(
        out.path().join("claude-md/global.mdx").exists(),
        "claude-md/global.mdx (the ~/.claude/CLAUDE.md page) must exist"
    );
}
