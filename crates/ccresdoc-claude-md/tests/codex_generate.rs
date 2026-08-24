use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::Duration;

use ccresdoc_claude_md::{
    generate_codex, watch_codex, CodexConfig, CodexSource, CodexWatchEvent, GenerateError,
};

fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/codex-representative")
}

fn config(source: &Path, docs: &Path) -> CodexConfig {
    CodexConfig {
        codex_dir: source.to_path_buf(),
        project_root: source.to_path_buf(),
        docs_dir: docs.to_path_buf(),
    }
}

fn write(root: &Path, relative: &str, content: &str) {
    let path = root.join(relative);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, content).unwrap();
}

fn read(path: impl AsRef<Path>) -> String {
    fs::read_to_string(path).unwrap()
}

#[test]
fn representative_fixture_emits_every_detail_category() {
    let output = tempfile::TempDir::new().unwrap();
    let report = generate_codex(&config(&fixture(), output.path())).unwrap();

    assert_eq!(
        (
            report.agents_md,
            report.config,
            report.agents,
            report.hooks,
            report.rules,
            report.skills,
        ),
        (2, 2, 1, 2, 1, 1)
    );
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);

    for (category, position) in [
        ("codex-agents-md", 905),
        ("codex-config", 906),
        ("codex-agents", 907),
        ("codex-hooks", 908),
        ("codex-rules", 909),
        ("codex-skills", 910),
    ] {
        let index = read(output.path().join(category).join("index.mdx"));
        assert!(index.contains(&format!("sidebar_position: {position}")));
        assert!(index.contains("category_no_page: true"));
        assert!(index.contains("generated: true"));
    }

    let instructions = read(output.path().join("codex-agents-md/root.mdx"));
    assert!(instructions.contains("sidebar_position: 1"));
    assert!(instructions.contains("&lt;CodexWidget&gt;"));
    assert!(instructions.contains("&#123;safeMdx&#125;"));
    assert!(instructions.contains("`local`"));
    assert!(instructions.contains("`[inline](./literal.md)`"));
    assert!(instructions.contains("[fenced](./literal.md)"));
    assert!(output
        .path()
        .join("codex-agents-md/project--override.mdx")
        .exists());

    let config_page = read(output.path().join("codex-config/config-toml.mdx"));
    assert!(config_page.contains("| `model` | `gpt-5` |"));
    assert!(config_page.contains("`[sandbox_workspace_write]`"));
    assert!(config_page.contains("```toml"));

    let agent = read(output.path().join("codex-agents/reviewer.mdx"));
    assert!(agent.contains("**Reasoning effort:** `high`"));
    assert!(agent.contains("**Sandbox:** `read-only`"));
    assert!(agent.contains("&lt;ReviewerWidget&gt; and &#123;risks&#125;"));

    let hooks = read(output.path().join("codex-hooks/hooks-json.mdx"));
    assert!(hooks.contains("`repo\\|worktree`"));
    assert!(hooks.contains("`./hooks/start.sh \\| tee log`"));
    let script = read(output.path().join("codex-hooks/start-sh.mdx"));
    assert!(script.contains("description: \"Prepare the session\""));
    assert!(script.contains("```bash"));

    let rules = read(output.path().join("codex-rules/default.mdx"));
    assert!(rules.contains("`git status\\|diff`"));
    assert_eq!(rules.matches("`read only`").count(), 1);

    let skill = read(output.path().join("codex-skills/fixture-skill/index.mdx"));
    assert!(skill.contains("**Display name:** Fixture Display"));
    assert!(skill.contains("**Invocation:** explicit only (`$fixture-skill`)"));
    assert!(skill.contains("[the contract](./ref-contract)"));
    assert!(skill.contains("`[literal](references/contract.md)`"));
    assert!(skill.contains("[literal](references/contract.md)\n```"));
    assert!(skill.contains("run.sh"));
    assert!(output
        .path()
        .join("codex-skills/fixture-skill/ref-contract.mdx")
        .exists());

    assert!(
        !output.path().join("codex/index.mdx").exists(),
        "the detail generator must not own the coordinator landing"
    );
}

#[test]
fn malformed_sources_warn_and_are_skipped() {
    let source = tempfile::TempDir::new().unwrap();
    let output = tempfile::TempDir::new().unwrap();
    write(source.path(), "config.toml", "broken = [");
    write(source.path(), "agents/bad.toml", "name = [");
    write(source.path(), "hooks.json", "{");
    write(source.path(), "skills/bad/SKILL.md", "not frontmatter");
    fs::create_dir_all(source.path().join("rules")).unwrap();
    fs::write(source.path().join("rules/non-utf8.rules"), [0xff, 0xfe]).unwrap();

    let report = generate_codex(&config(source.path(), output.path())).unwrap();
    assert_eq!(
        (report.config, report.agents, report.hooks, report.skills),
        (0, 0, 0, 0)
    );
    assert!(report
        .warnings
        .iter()
        .any(|w| w.source == CodexSource::Config));
    assert!(report
        .warnings
        .iter()
        .any(|w| w.source == CodexSource::Agents));
    assert!(report
        .warnings
        .iter()
        .any(|w| w.source == CodexSource::Hooks));
    assert!(report
        .warnings
        .iter()
        .any(|w| w.source == CodexSource::Skills));
    assert!(report
        .warnings
        .iter()
        .any(|w| w.source == CodexSource::Rules && w.message.contains("UTF-8")));
}

#[test]
fn instruction_walk_excludes_codex_runtime_state_directories() {
    let source = tempfile::TempDir::new().unwrap();
    let output = tempfile::TempDir::new().unwrap();
    write(source.path(), "AGENTS.md", "root instructions");
    for directory in [
        "sessions",
        "archived_sessions",
        "shell_snapshots",
        "history",
        "log",
        "logs",
        "tmp",
    ] {
        write(
            source.path(),
            &format!("{directory}/captured/AGENTS.md"),
            "private session instructions",
        );
    }

    let report = generate_codex(&config(source.path(), output.path())).unwrap();
    assert_eq!(report.agents_md, 1);
    let page = read(output.path().join("codex-agents-md/root.mdx"));
    assert!(page.contains("root instructions"));
    assert!(!page.contains("private session instructions"));
}

#[test]
fn regeneration_is_idempotent_prunes_stale_and_preserves_overview() {
    let source = tempfile::TempDir::new().unwrap();
    let output = tempfile::TempDir::new().unwrap();
    write(source.path(), "config.toml", "model = \"gpt-5\"\n");
    write(output.path(), "codex/index.mdx", "coordinator owned");
    generate_codex(&config(source.path(), output.path())).unwrap();
    let generated = output.path().join("codex-config/config-toml.mdx");
    let first = fs::metadata(&generated).unwrap().modified().unwrap();
    std::thread::sleep(Duration::from_millis(25));
    generate_codex(&config(source.path(), output.path())).unwrap();
    assert_eq!(first, fs::metadata(&generated).unwrap().modified().unwrap());

    fs::remove_file(source.path().join("config.toml")).unwrap();
    generate_codex(&config(source.path(), output.path())).unwrap();
    assert!(!output.path().join("codex-config").exists());
    assert_eq!(
        read(output.path().join("codex/index.mdx")),
        "coordinator owned"
    );
}

#[test]
fn normalized_collisions_and_reserved_slugs_fail_before_overwrite() {
    let source = tempfile::TempDir::new().unwrap();
    let output = tempfile::TempDir::new().unwrap();
    write(source.path(), "é.config.toml", "model = \"a\"");
    write(source.path(), "e\u{301}.config.toml", "model = \"b\"");
    assert!(matches!(
        generate_codex(&config(source.path(), output.path())),
        Err(GenerateError::SlugCollision(_))
    ));

    fs::remove_file(source.path().join("é.config.toml")).unwrap();
    fs::remove_file(source.path().join("e\u{301}.config.toml")).unwrap();
    write(source.path(), "straße.config.toml", "model = \"a\"");
    write(source.path(), "strasse.config.toml", "model = \"b\"");
    assert!(matches!(
        generate_codex(&config(source.path(), output.path())),
        Err(GenerateError::SlugCollision(_))
    ));
    fs::remove_file(source.path().join("straße.config.toml")).unwrap();
    fs::remove_file(source.path().join("strasse.config.toml")).unwrap();
    write(
        source.path(),
        "rules/index.rules",
        "prefix_rule(pattern='x')",
    );
    assert!(matches!(
        generate_codex(&config(source.path(), output.path())),
        Err(GenerateError::ReservedSlug(_))
    ));
}

#[test]
fn source_output_overlap_is_rejected() {
    let source = tempfile::TempDir::new().unwrap();
    let docs = source.path().join("generated/docs");
    let error = generate_codex(&config(source.path(), &docs)).unwrap_err();
    assert!(matches!(error, GenerateError::InvalidConfig(_)));
}

#[cfg(unix)]
#[test]
fn instruction_walk_never_follows_symlinks() {
    use std::os::unix::fs::symlink;

    let source = tempfile::TempDir::new().unwrap();
    let outside = tempfile::TempDir::new().unwrap();
    let output = tempfile::TempDir::new().unwrap();
    write(source.path(), "AGENTS.md", "root");
    write(outside.path(), "AGENTS.md", "outside secret");
    symlink(outside.path(), source.path().join("linked-project")).unwrap();
    symlink(
        source.path().join("broken-target"),
        source.path().join("broken-link"),
    )
    .unwrap();

    let report = generate_codex(&config(source.path(), output.path())).unwrap();
    assert_eq!(report.agents_md, 1);
    assert!(!read(output.path().join("codex-agents-md/root.mdx")).contains("outside secret"));
}

#[cfg(unix)]
#[test]
fn managed_output_symlink_is_rejected() {
    use std::os::unix::fs::symlink;

    let source = tempfile::TempDir::new().unwrap();
    let output = tempfile::TempDir::new().unwrap();
    let outside = tempfile::TempDir::new().unwrap();
    write(source.path(), "config.toml", "model = \"gpt-5\"");
    symlink(outside.path(), output.path().join("codex-config")).unwrap();
    assert!(matches!(
        generate_codex(&config(source.path(), output.path())),
        Err(GenerateError::InvalidConfig(_))
    ));
    assert!(fs::read_dir(outside.path()).unwrap().next().is_none());
}

#[cfg(unix)]
#[test]
fn only_direct_skill_package_symlinks_may_escape() {
    use std::os::unix::fs::symlink;

    let source = tempfile::TempDir::new().unwrap();
    let target = tempfile::TempDir::new().unwrap();
    let nested_target = tempfile::TempDir::new().unwrap();
    let output = tempfile::TempDir::new().unwrap();
    write(
        target.path(),
        "SKILL.md",
        "---\nname: Linked\ndescription: safe direct target\n---\nbody",
    );
    write(nested_target.path(), "hidden.md", "# Must not render");
    fs::create_dir_all(source.path().join("skills")).unwrap();
    symlink(target.path(), source.path().join("skills/linked")).unwrap();
    symlink(nested_target.path(), target.path().join("references")).unwrap();

    let report = generate_codex(&config(source.path(), output.path())).unwrap();
    assert_eq!(report.skills, 1);
    assert!(output.path().join("codex-skills/linked/index.mdx").exists());
    assert!(!output
        .path()
        .join("codex-skills/linked/ref-hidden.mdx")
        .exists());
}

#[test]
fn watcher_regenerates_relevant_content_and_ignores_churn() {
    let source = tempfile::TempDir::new().unwrap();
    let output = tempfile::TempDir::new().unwrap();
    write(source.path(), "AGENTS.md", "root");
    let (tx, rx) = mpsc::channel();
    let handle = watch_codex(
        config(source.path(), output.path()),
        Duration::from_millis(120),
        move |event| {
            let _ = tx.send(event);
        },
    )
    .unwrap();

    while rx.recv_timeout(Duration::from_millis(300)).is_ok() {}
    write(source.path(), "sessions/noise.json", "{}");
    assert!(rx.recv_timeout(Duration::from_millis(500)).is_err());

    write(source.path(), "config.toml", "model = \"gpt-5\"\n");
    match rx.recv_timeout(Duration::from_secs(10)).unwrap() {
        CodexWatchEvent::Regenerated(report) => assert_eq!(report.config, 1),
        CodexWatchEvent::Error { error, .. } => panic!("watch error: {error}"),
    }
    handle.stop();
}
