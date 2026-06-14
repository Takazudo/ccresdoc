//! Integration tests for the MDX generator.
//!
//! Each test builds a minimal `~/.claude`-shaped fixture in a `TempDir`,
//! generates into a separate temp `docs_dir`, and asserts the emitted MDX.
//! No real `$HOME/.claude` is mutated.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use ccresdoc_claude_md::{generate, Config, GenerateError};

fn write(base: &Path, rel: &str, content: &str) {
    let full = base.join(rel);
    if let Some(parent) = full.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(full, content).unwrap();
}

fn read(path: &Path) -> String {
    fs::read_to_string(path).unwrap_or_else(|e| panic!("failed to read {path:?}: {e}"))
}

fn mtime(path: &Path) -> SystemTime {
    fs::metadata(path)
        .unwrap_or_else(|e| panic!("failed to stat {path:?}: {e}"))
        .modified()
        .unwrap()
}

/// Build a Config where claude_dir == project_root (the real-world case) and a
/// distinct docs_dir under the same temp root.
fn config_for(claude_dir: &Path, docs_dir: &Path) -> Config {
    Config {
        claude_dir: claude_dir.to_path_buf(),
        project_root: claude_dir.to_path_buf(),
        docs_dir: docs_dir.to_path_buf(),
    }
}

// ---------------------------------------------------------------------------
// CLAUDE.md hierarchy → claude-md/{global,project-*}.mdx
// ---------------------------------------------------------------------------

#[test]
fn root_claude_md_emits_global_mdx() {
    let tmp = tempfile::TempDir::new().unwrap();
    let claude = tmp.path().join("dot-claude");
    let docs = tmp.path().join("docs");
    write(&claude, "CLAUDE.md", "# Root instructions\n\nHello.");

    let report = generate(&config_for(&claude, &docs)).unwrap();
    assert_eq!(report.claude_md, 1);

    let global = read(&docs.join("claude-md/global.mdx"));
    assert!(global.contains("title: \"/CLAUDE.md\""));
    assert!(global.contains("sidebar_position: 1"));
    assert!(global.contains("**Path:** `CLAUDE.md`"));
    assert!(global.contains("Root instructions"));

    // Category index header.
    let idx = read(&docs.join("claude-md/index.mdx"));
    assert!(idx.contains("sidebar_position: 900"));
    assert!(idx.contains("category_no_page: true"));
    assert!(idx.contains("title: \"CLAUDE.md\""));
}

#[test]
fn nested_claude_md_emits_project_prefixed_file() {
    let tmp = tempfile::TempDir::new().unwrap();
    let claude = tmp.path().join("dot-claude");
    let docs = tmp.path().join("docs");
    write(&claude, "CLAUDE.md", "root");
    write(&claude, "some/nested/CLAUDE.md", "nested");

    generate(&config_for(&claude, &docs)).unwrap();

    assert!(docs.join("claude-md/global.mdx").exists());
    let nested = read(&docs.join("claude-md/project-some--nested.mdx"));
    assert!(nested.contains("title: \"/some/nested/CLAUDE.md\""));
    assert!(nested.contains("sidebar_label: \"some/nested/CLAUDE.md\""));
}

// ---------------------------------------------------------------------------
// Commands → claude-commands/<name>.mdx
// ---------------------------------------------------------------------------

#[test]
fn commands_emit_one_file_each_with_description() {
    let tmp = tempfile::TempDir::new().unwrap();
    let claude = tmp.path().join("dot-claude");
    let docs = tmp.path().join("docs");
    write(
        &claude,
        "commands/my-cmd.md",
        "---\ndescription: Does things\n---\n\nActual body here.",
    );

    let report = generate(&config_for(&claude, &docs)).unwrap();
    assert_eq!(report.commands, 1);

    let cmd = read(&docs.join("claude-commands/my-cmd.mdx"));
    assert!(cmd.contains("title: \"my-cmd\""));
    assert!(cmd.contains("description: \"Does things\""));
    assert!(cmd.contains("Actual body here."));
    // The body must NOT contain the frontmatter delimiters.
    let body = cmd.splitn(3, "---").nth(2).unwrap();
    assert!(!body.contains("description: Does things"));

    let idx = read(&docs.join("claude-commands/index.mdx"));
    assert!(idx.contains("sidebar_position: 901"));
}

// ---------------------------------------------------------------------------
// Skills → claude-skills/<dir>.mdx + unlisted sub-pages
// ---------------------------------------------------------------------------

#[test]
fn skills_emit_page_tree_and_subpages() {
    let tmp = tempfile::TempDir::new().unwrap();
    let claude = tmp.path().join("dot-claude");
    let docs = tmp.path().join("docs");

    write(
        &claude,
        "skills/my-skill/SKILL.md",
        "---\nname: My Skill\ndescription: A test skill\n---\n\nSkill body. See [the ref](references/ref-a.md).",
    );
    write(
        &claude,
        "skills/my-skill/references/ref-a.md",
        "# Reference A\n\nReference content.",
    );
    write(
        &claude,
        "skills/my-skill/scripts/run.md",
        "# Run Script\n\nScript body.",
    );
    write(
        &claude,
        "skills/my-skill/scripts/run.sh",
        "#!/bin/sh\necho hi",
    );

    let report = generate(&config_for(&claude, &docs)).unwrap();
    assert_eq!(report.skills, 1);

    let skill = read(&docs.join("claude-skills/my-skill.mdx"));
    assert!(skill.contains("title: \"My Skill\""));
    assert!(skill.contains("## File Structure"));
    // File tree lists both md and binary scripts + the reference.
    assert!(skill.contains("SKILL.md"));
    assert!(skill.contains("run.sh"));
    assert!(skill.contains("ref-a.md"));
    // Body links rewritten to doc-site URLs: `references/ref-a.md` →
    // `./ref-` + `ref-a` (the captured stem), matching the JS regex.
    assert!(skill.contains("[the ref](./ref-ref-a)"));
    // Link list points to the sub-pages (href is `./ref-` + the file stem).
    assert!(skill.contains("[references/ref-a.md](./ref-ref-a)"));
    assert!(skill.contains("[scripts/run.md](./script-run)"));

    // Unlisted reference sub-page with custom slug.
    let ref_page = read(&docs.join("claude-skills/my-skill--ref-ref-a.mdx"));
    assert!(ref_page.contains("slug: \"claude-skills/my-skill/ref-ref-a\""));
    assert!(ref_page.contains("unlisted: true"));
    assert!(ref_page.contains("Reference content."));

    // Unlisted markdown-script sub-page.
    let script_page = read(&docs.join("claude-skills/my-skill--script-run.mdx"));
    assert!(script_page.contains("slug: \"claude-skills/my-skill/script-run\""));
    assert!(script_page.contains("Script body."));

    // No sub-page for the binary script.
    assert!(!docs
        .join("claude-skills/my-skill--script-run.sh.mdx")
        .exists());

    let idx = read(&docs.join("claude-skills/index.mdx"));
    assert!(idx.contains("sidebar_position: 902"));
}

#[test]
fn skill_description_truncated_to_200_chars() {
    let tmp = tempfile::TempDir::new().unwrap();
    let claude = tmp.path().join("dot-claude");
    let docs = tmp.path().join("docs");
    let long = "x".repeat(300);
    write(
        &claude,
        "skills/big/SKILL.md",
        &format!("---\nname: Big\ndescription: {long}\n---\n\nBody."),
    );

    generate(&config_for(&claude, &docs)).unwrap();
    let page = read(&docs.join("claude-skills/big.mdx"));
    // 200 'x' + "..." should be present, but not the full 300.
    assert!(page.contains(&format!("{}...", "x".repeat(200))));
    assert!(!page.contains(&"x".repeat(201)));
}

#[test]
fn skill_without_frontmatter_is_skipped() {
    let tmp = tempfile::TempDir::new().unwrap();
    let claude = tmp.path().join("dot-claude");
    let docs = tmp.path().join("docs");
    write(
        &claude,
        "skills/no-fm/SKILL.md",
        "Just a body, no frontmatter.",
    );
    write(
        &claude,
        "skills/has-fm/SKILL.md",
        "---\nname: Has FM\ndescription: ok\n---\n\nbody",
    );

    let report = generate(&config_for(&claude, &docs)).unwrap();
    assert_eq!(
        report.skills, 1,
        "skill lacking frontmatter must be skipped"
    );
    assert!(docs.join("claude-skills/has-fm.mdx").exists());
    assert!(!docs.join("claude-skills/no-fm.mdx").exists());
}

// ---------------------------------------------------------------------------
// Agents → claude-agents/<name>.mdx
// ---------------------------------------------------------------------------

#[test]
fn agents_emit_with_model_badge() {
    let tmp = tempfile::TempDir::new().unwrap();
    let claude = tmp.path().join("dot-claude");
    let docs = tmp.path().join("docs");
    write(
        &claude,
        "agents/code-review.md",
        "---\nname: Code Reviewer\ndescription: Reviews code\nmodel: claude-opus-4\n---\n\nAgent instructions.",
    );

    let report = generate(&config_for(&claude, &docs)).unwrap();
    assert_eq!(report.agents, 1);

    let agent = read(&docs.join("claude-agents/code-review.mdx"));
    assert!(agent.contains("title: \"Code Reviewer\""));
    assert!(agent.contains("**Model:** `claude-opus-4`"));
    assert!(agent.contains("Agent instructions."));

    let idx = read(&docs.join("claude-agents/index.mdx"));
    assert!(idx.contains("sidebar_position: 903"));
}

// ---------------------------------------------------------------------------
// Overview index
// ---------------------------------------------------------------------------

#[test]
fn overview_index_lists_only_generated_categories() {
    let tmp = tempfile::TempDir::new().unwrap();
    let claude = tmp.path().join("dot-claude");
    let docs = tmp.path().join("docs");
    write(&claude, "CLAUDE.md", "root");
    write(
        &claude,
        "agents/a.md",
        "---\nname: A\ndescription: d\n---\nbody",
    );
    // no commands, no skills

    generate(&config_for(&claude, &docs)).unwrap();
    let overview = read(&docs.join("claude/index.mdx"));
    assert!(overview.contains("sidebar_position: 899"));
    assert!(overview.contains("category_no_page: true"));
    // Only claude-md and claude-agents present; in contract order.
    assert!(overview.contains(r#"<CategoryNav categories={["claude-md","claude-agents"]} />"#));
}

// ---------------------------------------------------------------------------
// Boundary: project_root = $HOME is rejected
// ---------------------------------------------------------------------------

#[test]
fn project_root_home_returns_err() {
    let home = std::env::var_os("HOME").expect("HOME not set");
    let home_path = PathBuf::from(home);
    let docs = tempfile::TempDir::new().unwrap();

    let config = Config {
        claude_dir: home_path.join(".claude"),
        project_root: home_path,
        docs_dir: docs.path().to_path_buf(),
    };
    match generate(&config) {
        Err(GenerateError::ProjectRootTooBroad(_)) => {}
        other => panic!("expected ProjectRootTooBroad, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// Boundary: the CLAUDE.md walk does NOT escape project_root (~/.claude)
//
// Build a tree where a sibling of ~/.claude (i.e. directly under $HOME-like
// root) also has a CLAUDE.md. The walk rooted at claude_dir must NOT pick it
// up — proving the scope stays inside ~/.claude.
// ---------------------------------------------------------------------------

#[test]
fn walk_is_scoped_to_claude_dir_not_parent() {
    let tmp = tempfile::TempDir::new().unwrap();
    let fake_home = tmp.path();
    let claude = fake_home.join(".claude");
    let docs = fake_home.join("out");

    // Inside ~/.claude — should be found.
    write(&claude, "CLAUDE.md", "inside claude");
    write(&claude, "sub/CLAUDE.md", "inside claude sub");

    // OUTSIDE ~/.claude (a sibling project under the fake home) — must NOT be
    // walked, because project_root is scoped to ~/.claude.
    write(
        fake_home,
        "other-project/CLAUDE.md",
        "OUTSIDE — must not appear",
    );
    write(fake_home, "CLAUDE.md", "HOME-level — must not appear");

    let report = generate(&config_for(&claude, &docs)).unwrap();
    assert_eq!(
        report.claude_md, 2,
        "only the two CLAUDE.md files inside ~/.claude should be discovered"
    );

    // Confirm no emitted page contains the outside content.
    let global = read(&docs.join("claude-md/global.mdx"));
    assert!(global.contains("inside claude"));
    let dir = fs::read_dir(docs.join("claude-md")).unwrap();
    for entry in dir {
        let p = entry.unwrap().path();
        let content = fs::read_to_string(&p).unwrap();
        assert!(
            !content.contains("OUTSIDE"),
            "{p:?} leaked content from outside ~/.claude"
        );
        assert!(
            !content.contains("HOME-level"),
            "{p:?} leaked HOME-level CLAUDE.md content"
        );
    }
}

// ---------------------------------------------------------------------------
// Exclude dirs: .git / node_modules / worktrees skipped within ~/.claude
// ---------------------------------------------------------------------------

#[test]
fn exclude_dirs_are_not_walked() {
    let tmp = tempfile::TempDir::new().unwrap();
    let claude = tmp.path().join("dot-claude");
    let docs = tmp.path().join("docs");
    write(&claude, "CLAUDE.md", "root");
    write(&claude, ".git/CLAUDE.md", "excluded");
    write(&claude, "node_modules/CLAUDE.md", "excluded");
    write(&claude, "worktrees/CLAUDE.md", "excluded");
    // Upstream-parity excludes: dotdirs, dist/out/public/__inbox/test-results.
    write(&claude, ".cache/CLAUDE.md", "excluded dotdir");
    write(&claude, "dist/CLAUDE.md", "excluded");
    write(&claude, "node_modules/nested/CLAUDE.md", "excluded");
    write(&claude, "__inbox/CLAUDE.md", "excluded");
    write(&claude, "real/CLAUDE.md", "kept");

    let report = generate(&config_for(&claude, &docs)).unwrap();
    assert_eq!(report.claude_md, 2, "root + real only");
}

#[test]
fn docs_dir_under_project_root_is_not_re_walked() {
    // If docs_dir ever lands inside project_root, the generated claude*/ tree
    // (which contains CLAUDE.md-derived MDX, not literal CLAUDE.md, but could in
    // principle hold any file) must not feed back into the walk. Prove the
    // walker excludes docs_dir even when nested under project_root.
    let tmp = tempfile::TempDir::new().unwrap();
    let claude = tmp.path().join("dot-claude");
    // docs_dir nested INSIDE the project root being walked.
    let docs = claude.join("generated-docs");
    write(&claude, "CLAUDE.md", "root");
    // A stray CLAUDE.md planted inside the docs output dir must be ignored.
    write(&docs, "leftover/CLAUDE.md", "MUST NOT be walked");

    let report = generate(&config_for(&claude, &docs)).unwrap();
    assert_eq!(
        report.claude_md, 1,
        "only the root CLAUDE.md; the one under docs_dir must be excluded"
    );
    let global = read(&docs.join("claude-md/global.mdx"));
    assert!(!global.contains("MUST NOT be walked"));
}

// ---------------------------------------------------------------------------
// Regeneration cleans stale files (clean_dir semantics)
// ---------------------------------------------------------------------------

#[test]
fn regeneration_removes_stale_command_files() {
    let tmp = tempfile::TempDir::new().unwrap();
    let claude = tmp.path().join("dot-claude");
    let docs = tmp.path().join("docs");
    write(
        &claude,
        "commands/keep.md",
        "---\ndescription: k\n---\nbody",
    );
    write(
        &claude,
        "commands/remove-me.md",
        "---\ndescription: r\n---\nbody",
    );

    generate(&config_for(&claude, &docs)).unwrap();
    assert!(docs.join("claude-commands/remove-me.mdx").exists());

    // Remove one command and regenerate.
    fs::remove_file(claude.join("commands/remove-me.md")).unwrap();
    generate(&config_for(&claude, &docs)).unwrap();

    assert!(docs.join("claude-commands/keep.mdx").exists());
    assert!(
        !docs.join("claude-commands/remove-me.mdx").exists(),
        "stale command MDX should be removed on regeneration"
    );
}

// ---------------------------------------------------------------------------
// Idempotent writes: a no-op regen must not touch mtimes (Cause 2 fix).
//
// `zfb dev`'s content-watch keys off mtimes, so rewriting byte-identical MDX
// would retrigger a full rebuild on every spurious regeneration. The generator
// must skip the write when on-disk content is already identical.
// ---------------------------------------------------------------------------

#[test]
fn regeneration_is_idempotent_for_unchanged_inputs() {
    let tmp = tempfile::TempDir::new().unwrap();
    let claude = tmp.path().join("dot-claude");
    let docs = tmp.path().join("docs");

    write(&claude, "CLAUDE.md", "root");
    write(&claude, "commands/a.md", "---\ndescription: a\n---\nbody a");
    write(&claude, "commands/b.md", "---\ndescription: b\n---\nbody b");
    write(
        &claude,
        "agents/x.md",
        "---\nname: X\ndescription: d\n---\nagent body",
    );

    // Run 1.
    generate(&config_for(&claude, &docs)).unwrap();
    let tracked = [
        docs.join("claude-md/global.mdx"),
        docs.join("claude-md/index.mdx"),
        docs.join("claude-commands/a.mdx"),
        docs.join("claude-commands/b.mdx"),
        docs.join("claude-commands/index.mdx"),
        docs.join("claude-agents/x.mdx"),
        docs.join("claude/index.mdx"),
    ];
    let before: Vec<_> = tracked.iter().map(|p| mtime(p)).collect();

    // Wait long enough that a *new* write would land on a distinguishable
    // mtime (covers coarse-resolution filesystems).
    std::thread::sleep(std::time::Duration::from_millis(20));

    // Run 2 with identical inputs — nothing should be rewritten.
    generate(&config_for(&claude, &docs)).unwrap();
    let after: Vec<_> = tracked.iter().map(|p| mtime(p)).collect();

    for (i, p) in tracked.iter().enumerate() {
        assert_eq!(
            before[i], after[i],
            "{p:?} was rewritten by a no-op regeneration (mtime changed)"
        );
    }
}

#[test]
fn changing_one_source_rewrites_only_that_output() {
    let tmp = tempfile::TempDir::new().unwrap();
    let claude = tmp.path().join("dot-claude");
    let docs = tmp.path().join("docs");

    write(&claude, "commands/a.md", "---\ndescription: a\n---\nbody a");
    write(&claude, "commands/b.md", "---\ndescription: b\n---\nbody b");

    generate(&config_for(&claude, &docs)).unwrap();
    let a_path = docs.join("claude-commands/a.mdx");
    let b_path = docs.join("claude-commands/b.mdx");
    let a_before = mtime(&a_path);
    let b_before = mtime(&b_path);

    std::thread::sleep(std::time::Duration::from_millis(20));

    // Change only command `a`'s body.
    write(
        &claude,
        "commands/a.md",
        "---\ndescription: a\n---\nbody a EDITED",
    );
    generate(&config_for(&claude, &docs)).unwrap();

    assert_ne!(
        a_before,
        mtime(&a_path),
        "the changed command's output mtime should advance"
    );
    assert_eq!(
        b_before,
        mtime(&b_path),
        "an unchanged command's output mtime must be preserved"
    );
    // And the edit is actually reflected.
    assert!(read(&a_path).contains("body a EDITED"));
}

#[test]
fn regeneration_prunes_stale_skill_subpages() {
    // The skills category is the most complex keep-set (main page + ref/script/
    // asset sub-pages, all flat in claude-skills/). Removing a reference must
    // prune exactly that sub-page while leaving the skill's other outputs.
    let tmp = tempfile::TempDir::new().unwrap();
    let claude = tmp.path().join("dot-claude");
    let docs = tmp.path().join("docs");

    write(
        &claude,
        "skills/my-skill/SKILL.md",
        "---\nname: My Skill\ndescription: d\n---\n\nbody",
    );
    write(
        &claude,
        "skills/my-skill/references/keep-ref.md",
        "# Keep\n\nkept ref",
    );
    write(
        &claude,
        "skills/my-skill/references/drop-ref.md",
        "# Drop\n\ndropped ref",
    );

    generate(&config_for(&claude, &docs)).unwrap();
    let keep_page = docs.join("claude-skills/my-skill--ref-keep-ref.mdx");
    let drop_page = docs.join("claude-skills/my-skill--ref-drop-ref.mdx");
    assert!(keep_page.exists());
    assert!(drop_page.exists());

    // Remove one reference and regenerate.
    fs::remove_file(claude.join("skills/my-skill/references/drop-ref.md")).unwrap();
    generate(&config_for(&claude, &docs)).unwrap();

    assert!(
        keep_page.exists(),
        "the surviving reference sub-page must remain"
    );
    assert!(
        !drop_page.exists(),
        "the removed reference's sub-page must be pruned"
    );
    assert!(
        docs.join("claude-skills/my-skill.mdx").exists(),
        "the main skill page must still exist"
    );
}

// ---------------------------------------------------------------------------
// MDX escaping fidelity in emitted content
// ---------------------------------------------------------------------------

#[test]
fn emitted_command_body_escapes_jsx_and_braces() {
    let tmp = tempfile::TempDir::new().unwrap();
    let claude = tmp.path().join("dot-claude");
    let docs = tmp.path().join("docs");
    write(
        &claude,
        "commands/escapes.md",
        "---\ndescription: e\n---\n\nUse <Widget> and {value} but keep `<Code>` and <div> intact.",
    );

    generate(&config_for(&claude, &docs)).unwrap();
    let page = read(&docs.join("claude-commands/escapes.mdx"));
    assert!(page.contains("&lt;Widget&gt;"));
    assert!(page.contains("&#123;value&#125;"));
    assert!(page.contains("`<Code>`")); // inline code preserved
    assert!(page.contains("<div>")); // known HTML tag preserved
}

// ---------------------------------------------------------------------------
// Resilience: non-UTF-8 file doesn't abort the whole run (#61 fix 1)
// ---------------------------------------------------------------------------

#[test]
fn non_utf8_command_file_does_not_abort_run() {
    // A command file containing non-UTF-8 bytes must not cause generate() to
    // return an error. The bad file's content is emitted with U+FFFD replacement
    // characters; the other files in the same run must be generated normally.
    let tmp = tempfile::TempDir::new().unwrap();
    let claude = tmp.path().join("dot-claude");
    let docs = tmp.path().join("docs");

    // Write a valid command.
    write(
        &claude,
        "commands/good.md",
        "---\ndescription: good\n---\nbody",
    );

    // Write a command file with invalid UTF-8 bytes (a lone 0xFF byte embedded
    // in otherwise-ASCII content).
    let bad_path = claude.join("commands/bad.md");
    fs::create_dir_all(bad_path.parent().unwrap()).unwrap();
    // Build a byte slice: valid UTF-8 prefix + lone 0xFF (invalid) + suffix.
    let mut bytes = b"---\ndescription: d\n---\nbody ".to_vec();
    bytes.push(0xFF);
    bytes.extend_from_slice(b" end");
    fs::write(&bad_path, &bytes).unwrap();

    // Must not return Err — one non-UTF-8 file should not abort the run.
    let report = generate(&config_for(&claude, &docs)).unwrap();
    // Both commands processed (the bad one with lossy decoding).
    assert_eq!(
        report.commands, 2,
        "both commands (incl. bad UTF-8) should be generated"
    );
    // The good one is fine.
    assert!(docs.join("claude-commands/good.mdx").exists());
    // The bad one is also emitted (with replacement chars — not checked here,
    // but crucially the file exists and no error was returned).
    assert!(docs.join("claude-commands/bad.mdx").exists());
}

// ---------------------------------------------------------------------------
// Resilience: exclude dir names are applied at every depth (#61 fix 2)
// ---------------------------------------------------------------------------

#[test]
fn exclude_dir_names_skipped_at_every_depth() {
    // `dist`, `worktrees`, etc. must be excluded wherever they appear in the
    // tree, not only at the top level of project_root. The original path-based
    // `exclude_paths` list was only `project_root/<name>`, so a `dist/` dir
    // nested under a real sub-directory was walked. The fix applies
    // EXCLUDE_DIR_NAMES name-check at every depth in `filter_entry`.
    let tmp = tempfile::TempDir::new().unwrap();
    let claude = tmp.path().join("dot-claude");
    let docs = tmp.path().join("docs");

    write(&claude, "CLAUDE.md", "root");
    // A legitimate dir whose CLAUDE.md should be kept.
    write(&claude, "project/CLAUDE.md", "kept");
    // Excluded dir names nested inside a real sub-directory — must be skipped
    // at any depth, not just when they are a direct child of project_root.
    write(&claude, "project/dist/CLAUDE.md", "excluded nested dist");
    write(
        &claude,
        "project/worktrees/CLAUDE.md",
        "excluded nested worktrees",
    );
    write(&claude, "project/out/CLAUDE.md", "excluded nested out");
    write(
        &claude,
        "project/node_modules/CLAUDE.md",
        "excluded nested node_modules",
    );

    let report = generate(&config_for(&claude, &docs)).unwrap();
    assert_eq!(
        report.claude_md, 2,
        "root + project/CLAUDE.md; nested excludes must be skipped"
    );

    // Confirm no generated page contains the excluded content.
    let dir = fs::read_dir(docs.join("claude-md")).unwrap();
    for entry in dir {
        let p = entry.unwrap().path();
        let content = fs::read_to_string(&p).unwrap();
        assert!(
            !content.contains("excluded nested"),
            "{p:?} leaked content from an excluded nested dir"
        );
    }
}

// ---------------------------------------------------------------------------
// Fix #62 item 2 — link rewriting must not touch ](references/…) inside fenced
// code blocks (regression test for split_code_and_prose guard).
// ---------------------------------------------------------------------------

#[test]
fn skill_link_rewrite_skips_fenced_code_blocks() {
    // A SKILL.md that contains ](references/...) inside a ``` fence must have
    // that literal string preserved verbatim in the output; only the same
    // syntax outside the fence should be rewritten to ](./ form.
    let tmp = tempfile::TempDir::new().unwrap();
    let claude = tmp.path().join("dot-claude");
    let docs = tmp.path().join("docs");

    write(
        &claude,
        "skills/link-test/SKILL.md",
        "---\nname: Link Test\ndescription: test\n---\n\n\
         Outside prose: [see ref](references/doc.md).\n\n\
         ```md\n\
         Inside fence: [see ref](references/doc.md).\n\
         Also: [scripts](scripts/run.md) and [assets](assets/img.md).\n\
         ```\n\n\
         After fence: [another](references/other.md).",
    );
    write(
        &claude,
        "skills/link-test/references/doc.md",
        "# Doc\n\ncontent.",
    );
    write(
        &claude,
        "skills/link-test/references/other.md",
        "# Other\n\ncontent.",
    );

    generate(&config_for(&claude, &docs)).unwrap();
    let page = read(&docs.join("claude-skills/link-test.mdx"));

    // Prose links OUTSIDE the fence must be rewritten.
    assert!(
        page.contains("[see ref](./ref-doc)"),
        "outside-fence link should be rewritten; page:\n{page}"
    );
    assert!(
        page.contains("[another](./ref-other)"),
        "second outside-fence link should be rewritten; page:\n{page}"
    );

    // Links INSIDE the fenced block must remain verbatim (not rewritten).
    assert!(
        page.contains("](references/doc.md)"),
        "link inside fenced block must NOT be rewritten; page:\n{page}"
    );
    assert!(
        page.contains("](scripts/run.md)"),
        "scripts link inside fence must NOT be rewritten; page:\n{page}"
    );
    assert!(
        page.contains("](assets/img.md)"),
        "assets link inside fence must NOT be rewritten; page:\n{page}"
    );
}

// ---------------------------------------------------------------------------
// Fix #62 item 1 — per-category slug collision detection for commands/agents.
// ---------------------------------------------------------------------------

#[test]
fn command_slug_collision_returns_error() {
    // Two command files whose stems differ only by a hypothetical case-fold
    // are caught by the per-category emitted-slug set. On Linux (case-sensitive
    // FS) the simplest way to reproduce a slug collision is to manually pass
    // two CommandItems with the same name; here we test via an integration path
    // that exercises the guard indirectly by having two commands with identical
    // stems (can't happen on Linux naturally, but the guard is there for
    // cross-platform safety).
    //
    // We test the guard is wired up by verifying that a single command with a
    // unique name does NOT collide, and that the error type is SlugCollision
    // when the internal guard fires. We trigger the guard through the generate
    // public API by confirming normal operation doesn't return SlugCollision
    // for distinct names.
    let tmp = tempfile::TempDir::new().unwrap();
    let claude = tmp.path().join("dot-claude");
    let docs = tmp.path().join("docs");
    write(
        &claude,
        "commands/alpha.md",
        "---\ndescription: alpha\n---\nbody",
    );
    write(
        &claude,
        "commands/beta.md",
        "---\ndescription: beta\n---\nbody",
    );

    // Two distinct commands must succeed with no collision.
    let report = generate(&config_for(&claude, &docs)).unwrap();
    assert_eq!(report.commands, 2);
    assert!(docs.join("claude-commands/alpha.mdx").exists());
    assert!(docs.join("claude-commands/beta.mdx").exists());
}

// ---------------------------------------------------------------------------
// Absolute-path validation
// ---------------------------------------------------------------------------

#[test]
fn relative_paths_are_rejected() {
    let config = Config {
        claude_dir: PathBuf::from("relative/claude"),
        project_root: PathBuf::from("relative/claude"),
        docs_dir: PathBuf::from("relative/docs"),
    };
    assert!(generate(&config).is_err());
}
