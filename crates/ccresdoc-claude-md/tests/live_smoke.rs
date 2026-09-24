//! Live smoke test: generate MDX against the real `$HOME/.claude`, writing into
//! a TEMP output dir. Skips gracefully when `$HOME/.claude` is absent (CI).
//!
//! This is the acceptance smoke check: generating against the real `~/.claude`
//! produces browsable MDX for the resources actually present, with category
//! index pages only for populated families. The representative fixture in
//! `generate.rs` requires and checks all four families independently of HOME.

use std::path::{Path, PathBuf};

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

    // Commands and agents are optional. Every direct regular .md source must
    // still produce a page; merely accepting a zero report would hide drops.
    for (source, category, count) in [
        ("commands", "claude-commands", report.commands),
        ("agents", "claude-agents", report.agents),
    ] {
        let sources = markdown_sources(&claude.join(source));
        assert_eq!(count, sources.len(), "{source}: source/report mismatch");
        for source in sources {
            let page = out
                .path()
                .join(category)
                .join(source.file_name().unwrap())
                .with_extension("mdx");
            assert!(page.is_file(), "missing generated page: {page:?}");
        }
    }

    // Detail category index pages exist with the right positions. The routed
    // `claude/` landing is coordinator-owned and is intentionally not emitted.
    assert!(!out.path().join("claude/index.mdx").exists());
    for (sub, pos, count) in [
        ("claude-md", "900", report.claude_md),
        ("claude-commands", "901", report.commands),
        ("claude-skills", "902", report.skills),
        ("claude-agents", "903", report.agents),
    ] {
        let idx = out.path().join(sub).join("index.mdx");
        if count == 0 {
            assert!(!idx.exists(), "empty {sub} must not have an index");
            continue;
        }
        assert!(idx.exists(), "{sub}/index.mdx must exist");
        let content = std::fs::read_to_string(&idx).unwrap();
        assert!(
            content.contains(&format!("sidebar_position: {pos}")),
            "{sub}/index.mdx must have sidebar_position {pos}"
        );
        assert!(content.contains("category_no_page: true"));
    }

    // A global page exists exactly when the developer has a root CLAUDE.md.
    assert_eq!(
        out.path().join("claude-md/global.mdx").exists(),
        claude.join("CLAUDE.md").is_file(),
        "global page must match ~/.claude/CLAUDE.md availability"
    );
}

fn markdown_sources(dir: &Path) -> Vec<PathBuf> {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Vec::new(),
        Err(err) => panic!("cannot inspect {dir:?}: {err}"),
    };
    entries
        .map(|entry| entry.expect("cannot read source entry"))
        .filter(|entry| {
            entry
                .file_type()
                .expect("cannot inspect source type")
                .is_file()
                && entry
                    .file_name()
                    .to_str()
                    .is_some_and(|name| name.ends_with(".md"))
        })
        .map(|entry| entry.path())
        .collect()
}
