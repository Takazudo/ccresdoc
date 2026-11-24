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
        "# Notebook\n\n[Attack](nested/attack.mdx?view=1#frag) [Nested](nested/README.md?mode=1#top) [Attachment](manual.pdf)\n\n`[literal](nested/attack.mdx)`\n\n```md\n[code](nested/attack.mdx)\n```\n",
    );
    write(&source.join("nested/attack.mdx"), "# Attack\n\n[Home](../README.md#home)\n<script>window.__TAURI__.core.invoke('save_source_editor_draft')</script>\n<img onerror=\"window.__TAURI__.core.invoke('save_source_editor_draft')\">\n{window.__TAURI__.core.invoke('save_source_editor_draft')}\nimport x from './evil.js'\n[click](javascript:alert(1))\n");
    write(
        &source.join("nested/README.md"),
        "# Nested\n\n[Home](../README.md#intro)\n",
    );
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
    assert_eq!(report.pages, 3);
    assert!(docs.join("notes/index.mdx").exists());
    let attack = fs::read_to_string(docs.join("notes/nested/attack.mdx")).unwrap();
    if let Ok(output) = std::env::var("CCRESDOC_MALICIOUS_FIXTURE_OUT") {
        fs::write(output, &attack).unwrap();
    }
    assert!(attack.contains("doc_source_path: \"nested/attack.mdx\""));
    assert!(attack.contains("[Home](../../#home)"));
    assert!(!attack.contains(source.to_str().unwrap()));
    assert!(!attack.contains("<script>"));
    assert!(!attack.contains("<img"));
    assert!(!attack.contains("\nimport x"));
    assert!(!attack.contains("](javascript:"));
    assert!(!docs.join("notes/nested/ignored.bin").exists());
    assert!(docs.join("notes/nested/index.mdx").exists());
    let landing = fs::read_to_string(docs.join("notes/index.mdx")).unwrap();
    assert!(landing.contains("marker-42"));
    assert!(landing.contains("[Attack](./nested/attack/?view=1#frag)"));
    assert!(landing.contains("[Nested](./nested/?mode=1#top)"));
    let nested_landing = fs::read_to_string(docs.join("notes/nested/index.mdx")).unwrap();
    assert!(nested_landing.contains("[Home](../#intro)"));
    assert!(landing.contains("Attachment (`manual.pdf`)"));
    assert!(landing.contains("`[literal](nested/attack.mdx)`"));
    assert!(landing.contains("[code](nested/attack.mdx)"));
}

#[test]
fn builtin_claude_retains_specialized_generation_below_mount() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source");
    let docs = root.path().join("docs");
    write(
        &source.join("CLAUDE.md"),
        "# Root instructions\n\n[Project](project/CLAUDE.md?view=1#part)\n",
    );
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
    assert!(instructions.contains("[Project](./project/?view=1#part)"));
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
    while rx.try_recv().is_ok() {}
    fs::remove_dir_all(&source).unwrap();
    assert!(matches!(
        rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap(),
        SourceWatchEvent::Changed
    ));
    while rx.try_recv().is_ok() {}
    write(&source.join("nested/reappeared.md"), "back");
    assert!(matches!(
        rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap(),
        SourceWatchEvent::Changed
    ));
    while rx.try_recv().is_ok() {}
    write(&source.join("nested/reappeared.md"), "edited");
    assert!(matches!(
        rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap(),
        SourceWatchEvent::Changed
    ));
}

#[cfg(unix)]
#[test]
fn codex_watcher_reconciles_added_and_retargeted_external_skill_links() {
    use std::os::unix::fs::symlink;

    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("codex-home");
    let first = root.path().join("outside-first");
    let second = root.path().join("outside-second");
    fs::create_dir_all(source.join("skills")).unwrap();
    write(&first.join("SKILL.md"), "# First\n");
    write(&second.join("SKILL.md"), "# Second\n");
    let spec = SourceSpec {
        id: "codex-links".into(),
        label: "Codex links".into(),
        root: source.clone(),
        scan_root: source.clone(),
        mount: "codex-links".into(),
        rule: SourceRule::Codex,
    };
    let (tx, rx) = std::sync::mpsc::channel();
    let _watch = watch_source(spec, std::time::Duration::from_millis(75), move |event| {
        let _ = tx.send(event);
    })
    .unwrap();
    let link = source.join("skills/linked");
    symlink(&first, &link).unwrap();
    assert!(matches!(
        rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap(),
        SourceWatchEvent::Changed
    ));
    std::thread::sleep(std::time::Duration::from_millis(250));
    while rx.try_recv().is_ok() {}
    write(&first.join("SKILL.md"), "# First updated\n");
    assert!(matches!(
        rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap(),
        SourceWatchEvent::Changed
    ));
    fs::remove_file(&link).unwrap();
    symlink(&second, &link).unwrap();
    assert!(matches!(
        rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap(),
        SourceWatchEvent::Changed
    ));
    std::thread::sleep(std::time::Duration::from_millis(250));
    while rx.try_recv().is_ok() {}
    write(&second.join("SKILL.md"), "# Second updated\n");
    assert!(matches!(
        rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap(),
        SourceWatchEvent::Changed
    ));
    std::thread::sleep(std::time::Duration::from_millis(250));
    while rx.try_recv().is_ok() {}
    write(&first.join("SKILL.md"), "# First stale target edit\n");
    assert!(rx
        .recv_timeout(std::time::Duration::from_millis(600))
        .is_err());
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
    let broad = SourceSpec {
        id: "broad".into(),
        root: Path::new("/").to_path_buf(),
        scan_root: Path::new("/").to_path_buf(),
        ..spec
    };
    assert!(watch_source(broad, std::time::Duration::from_millis(50), |_| {}).is_err());
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
    assert!(skill.contains("title: \"Helpers\""));
    assert!(skill.contains("description: \"Example\""));
    assert!(!skill.contains("\nname: Helpers\n"));
    assert!(skill.contains("**Display name:** Helper Display"));
    assert!(skill.contains("**Invocation:** explicit only"));
    assert!(skill.contains("[references/guide.md](./references-guide/)"));
    let guide = fs::read_to_string(docs.join("my-skills/helpers/references-guide.mdx")).unwrap();
    assert!(guide.contains("unlisted: true"));
    assert!(guide.contains("doc_source_path: \"helpers/references/guide.md\""));
    assert!(!docs.join("my-skills/helpers/assets/private.bin").exists());
}

#[test]
fn custom_skill_excludes_nested_files_and_escapes_attacker_filenames() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source");
    let docs = root.path().join("docs");
    write(&source.join("helpers/SKILL.md"), "# Helpers\n");
    write(&source.join("helpers/references/public.md"), "# Public\n");
    write(
        &source.join("helpers/references/visible.mdx"),
        "# Visible\n",
    );
    write(
        &source.join("helpers/references/private.md"),
        "PRIVATE_MARKDOWN\n",
    );
    write(
        &source.join("helpers/references/private.mdx"),
        "PRIVATE_MDX\n",
    );
    write(
        &source.join("helpers/assets/private.bin"),
        "PRIVATE_BINARY\n",
    );
    write(
        &source.join("helpers/agents/openai.yaml"),
        "interface:\n  display_name: PRIVATE_DISPLAY\n",
    );
    let link_attack = "bad](javascript:alert(1))<Widget>{run}.md";
    write(
        &source.join("helpers/references").join(link_attack),
        "# Safe\n",
    );
    let fence_attack = "break\n```\n<img src=x onerror=run()>.md";
    write(
        &source.join("helpers/references").join(fence_attack),
        "# Still safe\n",
    );

    let spec = SourceSpec {
        id: "skills-filtered".into(),
        label: "Filtered skills".into(),
        root: source.clone(),
        scan_root: source,
        mount: "filtered".into(),
        rule: SourceRule::Custom {
            format: SourceFormat::Skill,
            include: vec!["*/SKILL.md".into()],
            exclude: vec![
                "helpers/references/private.*".into(),
                "helpers/assets/private.bin".into(),
                "helpers/agents/openai.yaml".into(),
            ],
        },
    };
    let report = generate_source(&spec, &docs, "marker").unwrap();
    assert_eq!(report.pages, 5);
    let package = fs::read_to_string(docs.join("filtered/helpers/index.mdx")).unwrap();
    assert!(package.contains("[references/public.md](./references-public/)"));
    assert!(package.contains("[references/visible.mdx](./references-visible/)"));
    assert!(docs.join("filtered/helpers/references-public.mdx").exists());
    assert!(docs
        .join("filtered/helpers/references-visible.mdx")
        .exists());
    assert!(!docs
        .join("filtered/helpers/references-private.mdx")
        .exists());
    for private in [
        "private.md",
        "private.mdx",
        "private.bin",
        "PRIVATE_DISPLAY",
    ] {
        assert!(
            !package.contains(private),
            "excluded name leaked: {private}"
        );
    }
    assert!(!package.contains("PRIVATE_MARKDOWN"));
    assert!(!package.contains("PRIVATE_MDX"));
    assert!(package.contains("[references/bad\\](javascript:alert(1))&lt;Widget&gt;&#123;run&#125;.md](./references-bad-javascript-alert-1-widget-run/)"));
    assert!(!package.contains("[references/bad](javascript:"));
    assert!(package.contains("## File Structure\n\n````\n"));
    assert!(package.contains("\n````\n\n- ["));
    assert!(!package.contains("\n```\n<img src=x onerror=run()>\n```"));
}

#[test]
fn custom_skill_broad_include_selects_package_contents_once() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source");
    let docs = root.path().join("docs");
    write(&source.join("helpers/SKILL.md"), "# Helpers\n");
    write(&source.join("helpers/README.md"), "# Read me\n");
    write(&source.join("helpers/references/guide.md"), "# Guide\n");
    write(
        &source.join("helpers/references/private.md"),
        "PRIVATE_CONTENT\n",
    );
    let spec = SourceSpec {
        id: "skills-broad".into(),
        label: "Broad skills".into(),
        root: source.clone(),
        scan_root: source,
        mount: "broad".into(),
        rule: SourceRule::Custom {
            format: SourceFormat::Skill,
            include: vec!["**/*.md".into()],
            exclude: vec!["helpers/references/private.md".into()],
        },
    };
    let report = generate_source(&spec, &docs, "marker").unwrap();
    assert_eq!(report.pages, 3);
    let package = fs::read_to_string(docs.join("broad/helpers/index.mdx")).unwrap();
    assert!(package.contains("[README.md](./readme/)"));
    assert!(package.contains("[references/guide.md](./references-guide/)"));
    assert!(!package.contains("private.md"));
    assert!(docs.join("broad/helpers/references-guide.mdx").exists());
    assert!(docs.join("broad/helpers/readme.mdx").exists());
    assert!(!docs.join("broad/helpers/references-private.mdx").exists());
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
    let reference = fs::read_to_string(
        output
            .path()
            .join("codex-two/skills/fixture-skill/ref-contract.mdx"),
    )
    .unwrap();
    assert!(reference.contains("doc_source_path: \"fixture-skill/references/contract.md\""));
}

#[test]
fn code_source_fence_outgrows_attacker_backticks() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source");
    let docs = root.path().join("docs");
    write(
        &source.join("attack.txt"),
        "   ```\n<script>window.__TAURI__.core.invoke('save_source_editor_draft')</script>\n",
    );
    let spec = SourceSpec {
        id: "code-one".into(),
        label: "Code".into(),
        root: source.clone(),
        scan_root: source,
        mount: "code-one".into(),
        rule: SourceRule::Custom {
            format: SourceFormat::Code,
            include: vec!["*.txt".into()],
            exclude: vec![],
        },
    };
    generate_source(&spec, &docs, "code-marker").unwrap();
    let body = fs::read_to_string(docs.join("code-one/attack-txt.mdx")).unwrap();
    assert!(body.contains("\n````\n   ```\n<script>"));
    assert!(body.ends_with("\n````\n"));
}
