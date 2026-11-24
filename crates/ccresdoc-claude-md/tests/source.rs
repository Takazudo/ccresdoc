use std::fs;
use std::path::Path;

use ccresdoc_claude_md::{
    generate_source, watch_source, SourceFormat, SourceRule, SourceSpec, SourceWatchEvent,
};

fn write(path: &Path, text: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

#[test]
fn custom_source_is_mount_scoped_and_imported_mdx_cannot_run_native_calls() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source");
    let docs = root.path().join("docs");
    write(
        &source.join("README.md"),
        "# Notebook\n\n[Attack](nested/attack.mdx?view=1#frag) [Attachment](manual.pdf)\n",
    );
    write(&source.join("nested/attack.mdx"), "# Attack\n\n<script>window.__TAURI__.core.invoke('save_source_editor_draft')</script>\n<img onerror=\"window.__TAURI__.core.invoke('save_source_editor_draft')\">\n{window.__TAURI__.core.invoke('save_source_editor_draft')}\nimport x from './evil.js'\n[click](javascript:alert(1))\n");
    write(&source.join("nested/ignored.bin"), "SECRET_BINARY");
    let spec = SourceSpec {
        id: "notebook".into(),
        label: "Notebook".into(),
        root: source.clone(),
        scan_root: source.clone(),
        mount: "notes".into(),
        rule: SourceRule::Custom {
            format: SourceFormat::Markdown,
            include: vec!["**/*.md".into(), "**/*.mdx".into()],
            exclude: vec![],
        },
    };
    let report = generate_source(&spec, &docs, "marker-42").unwrap();
    assert_eq!(report.pages, 2);
    assert!(docs.join("notes/index.mdx").exists());
    let attack = fs::read_to_string(docs.join("notes/nested/attack.mdx")).unwrap();
    assert!(attack.contains("doc_source_path: \"nested/attack.mdx\""));
    assert!(!attack.contains(source.to_str().unwrap()));
    assert!(!attack.contains("<script>"));
    assert!(!attack.contains("<img"));
    assert!(!attack.contains("\nimport x"));
    assert!(!attack.contains("](javascript:"));
    assert!(!docs.join("notes/nested/ignored.bin").exists());
    assert!(docs.join("notes/nested/index.mdx").exists());
    let landing = fs::read_to_string(docs.join("notes/index.mdx")).unwrap();
    assert!(landing.contains("marker-42"));
    assert!(landing.contains("[Attack](/docs/notes/nested/attack/?view=1#frag)"));
    assert!(landing.contains("Attachment (`manual.pdf`)"));
}

#[test]
fn builtin_claude_retains_specialized_generation_below_mount() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source");
    let docs = root.path().join("docs");
    write(&source.join("CLAUDE.md"), "# Root instructions\n");
    write(&source.join("commands/check.md"), "# Check\n");
    write(&source.join("commands/nested/audit.md"), "# Audit\n");
    write(
        &source.join("project/CLAUDE.md"),
        "# Project instructions\n",
    );
    let spec = SourceSpec {
        id: "claude-two".into(),
        label: "Second Claude".into(),
        root: source.clone(),
        scan_root: source,
        mount: "claude-two".into(),
        rule: SourceRule::ClaudeCode,
    };
    let report = generate_source(&spec, &docs, "marker-43").unwrap();
    assert!(report.pages >= 2);
    let instructions = fs::read_to_string(docs.join("claude-two/instructions/index.mdx")).unwrap();
    assert!(instructions.contains("Root instructions"));
    assert!(instructions.contains("doc_source_mapping: \"instructions\""));
    assert!(instructions.contains("doc_source_path: \"CLAUDE.md\""));
    assert!(docs.join("claude-two/commands/check.mdx").exists());
    assert!(docs.join("claude-two/commands/nested/audit.mdx").exists());
    assert!(docs.join("claude-two/commands/nested/index.mdx").exists());
    let nested =
        fs::read_to_string(docs.join("claude-two/instructions/project/index.mdx")).unwrap();
    assert!(nested.contains("doc_source_path: \"project/CLAUDE.md\""));
    let landing = fs::read_to_string(docs.join("claude-two/index.mdx")).unwrap();
    assert!(landing.contains("<CategoryNav category=\"claude-two\" />"));
    assert!(landing.contains("marker-43"));
}

#[test]
fn watcher_observes_missing_root_appearance_and_nested_edit() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("appears-later");
    let (tx, rx) = std::sync::mpsc::channel();
    let spec = SourceSpec {
        id: "later".into(),
        label: "Later".into(),
        root: source.clone(),
        scan_root: source.clone(),
        mount: "later".into(),
        rule: SourceRule::Custom {
            format: SourceFormat::Markdown,
            include: vec!["**/*.md".into()],
            exclude: vec![],
        },
    };
    let _watch = watch_source(spec, std::time::Duration::from_millis(100), move |event| {
        let _ = tx.send(event);
    })
    .unwrap();
    write(&source.join("nested/first.md"), "first");
    assert!(matches!(
        rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap(),
        SourceWatchEvent::Changed
    ));
    write(&source.join("nested/first.md"), "second");
    assert!(matches!(
        rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap(),
        SourceWatchEvent::Changed
    ));
    fs::rename(
        source.join("nested/first.md"),
        source.join("nested/renamed.md"),
    )
    .unwrap();
    assert!(matches!(
        rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap(),
        SourceWatchEvent::Changed
    ));
}

#[test]
fn source_id_cannot_escape_candidate_scratch() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source");
    fs::create_dir(&source).unwrap();
    let docs = root.path().join("docs");
    let spec = SourceSpec {
        id: "../escape".into(),
        label: "Bad".into(),
        root: source.clone(),
        scan_root: source,
        mount: "safe".into(),
        rule: SourceRule::ClaudeCode,
    };
    assert!(generate_source(&spec, &docs, "marker").is_err());
    assert!(!root.path().join("escape").exists());
}

#[test]
fn custom_skill_renders_package_structure_metadata_and_flat_unlisted_page() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source");
    let docs = root.path().join("docs");
    write(
        &source.join("helpers/SKILL.md"),
        "---\nname: Helpers\ndescription: Example\n---\n\n# Helpers\n",
    );
    write(&source.join("helpers/references/guide.md"), "# Guide\n");
    write(&source.join("helpers/agents/openai.yaml"), "interface:\n  display_name: Helper Display\n  short_description: Useful helper\npolicy:\n  allow_implicit_invocation: false\n");
    write(&source.join("helpers/assets/private.bin"), "BINARY_SECRET");
    let spec = SourceSpec {
        id: "skills-one".into(),
        label: "Skills".into(),
        root: source.clone(),
        scan_root: source,
        mount: "my-skills".into(),
        rule: SourceRule::Custom {
            format: SourceFormat::Skill,
            include: vec!["*/SKILL.md".into()],
            exclude: vec![],
        },
    };
    let report = generate_source(&spec, &docs, "skill-marker").unwrap();
    assert_eq!(report.pages, 2);
    let skill = fs::read_to_string(docs.join("my-skills/helpers/index.mdx")).unwrap();
    assert!(skill.contains("## File Structure"));
    assert!(skill.contains("**Display name:** Helper Display"));
    assert!(skill.contains("**Invocation:** explicit only"));
    assert!(skill.contains("[references/guide.md](./references-guide)"));
    let guide = fs::read_to_string(docs.join("my-skills/helpers/references-guide.mdx")).unwrap();
    assert!(guide.contains("unlisted: true"));
    assert!(guide.contains("doc_source_path: \"helpers/references/guide.md\""));
    assert!(!docs.join("my-skills/helpers/assets/private.bin").exists());
}

#[test]
fn codex_preset_keeps_specialized_pages_and_mapping_relative_provenance() {
    let output = tempfile::tempdir().unwrap();
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/codex-representative");
    let spec = SourceSpec {
        id: "codex-two".into(),
        label: "Second Codex".into(),
        root: source.clone(),
        scan_root: source,
        mount: "codex-two".into(),
        rule: SourceRule::Codex,
    };
    generate_source(&spec, output.path(), "codex-marker").unwrap();
    let root = fs::read_to_string(output.path().join("codex-two/instructions/index.mdx")).unwrap();
    assert!(root.contains("doc_source_path: \"AGENTS.md\""));
    assert!(output
        .path()
        .join("codex-two/instructions/project/agents-override.mdx")
        .exists());
    let config =
        fs::read_to_string(output.path().join("codex-two/config/config-toml.mdx")).unwrap();
    assert!(config.contains("| `model`"));
    assert!(config.contains("doc_source_mapping: \"config\""));
    assert!(config.contains("doc_source_path: \"config.toml\""));
    let agent = fs::read_to_string(output.path().join("codex-two/agents/reviewer.mdx")).unwrap();
    assert!(agent.contains("**Reasoning effort:**"));
    assert!(agent.contains("doc_source_path: \"reviewer.toml\""));
    assert!(output
        .path()
        .join("codex-two/skills/fixture-skill/index.mdx")
        .exists());
}
