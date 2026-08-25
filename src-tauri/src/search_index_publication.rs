use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

use ccresdoc_claude_md::{search_index_json, SearchIndexNamespace, SearchIndexPage};

use crate::runtime::{CLAUDE_NAMESPACES, CODEX_NAMESPACES};

const SEARCH_INDEX_RELATIVE_PATH: &str = "public/docs/search-index.json";
static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Default)]
struct Frontmatter {
    title: String,
    description: String,
    search_exclude: bool,
    draft: bool,
    unlisted: bool,
    category_no_page: bool,
}

/// Collect every generator-owned page currently present in the served tree.
///
/// The generic `claude/` and `codex/` landings are coordinator-authored status
/// pages, so only the exact detail namespaces participate. Reading the actual
/// tree (rather than a settings snapshot) also makes rollback publication
/// describe whatever the journal successfully restored.
pub(crate) fn collect_generated_pages(docs_dir: &Path) -> Result<Vec<SearchIndexPage>, String> {
    let mut pages = Vec::new();
    for (namespace, names) in [
        (SearchIndexNamespace::Claude, &CLAUDE_NAMESPACES[1..]),
        (SearchIndexNamespace::Codex, &CODEX_NAMESPACES[1..]),
    ] {
        for name in names {
            let root = docs_dir.join(name);
            if root.exists() {
                collect_namespace(&root, docs_dir, namespace, &mut pages)?;
            }
        }
    }
    Ok(pages)
}

fn collect_namespace(
    directory: &Path,
    docs_dir: &Path,
    namespace: SearchIndexNamespace,
    pages: &mut Vec<SearchIndexPage>,
) -> Result<(), String> {
    let mut entries = fs::read_dir(directory)
        .map_err(|error| format!("read generated namespace {}: {error}", directory.display()))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("read generated namespace {}: {error}", directory.display()))?;
    entries.sort_by_key(|entry| entry.file_name());

    for entry in entries {
        let path = entry.path();
        let file_type = entry
            .file_type()
            .map_err(|error| format!("inspect generated page {}: {error}", path.display()))?;
        if file_type.is_dir() {
            collect_namespace(&path, docs_dir, namespace, pages)?;
        } else if file_type.is_file() && path.extension().is_some_and(|ext| ext == "mdx") {
            pages.push(read_page(&path, docs_dir, namespace)?);
        }
    }
    Ok(())
}

fn read_page(
    path: &Path,
    docs_dir: &Path,
    namespace: SearchIndexNamespace,
) -> Result<SearchIndexPage, String> {
    let source = fs::read_to_string(path)
        .map_err(|error| format!("read generated page {}: {error}", path.display()))?;
    let (frontmatter, body) = split_frontmatter(&source).ok_or_else(|| {
        format!(
            "generated page lacks closed frontmatter: {}",
            path.display()
        )
    })?;
    let frontmatter = parse_generated_frontmatter(frontmatter);
    let relative = path
        .strip_prefix(docs_dir)
        .map_err(|_| format!("generated page escaped docs root: {}", path.display()))?;
    let mut slug_path = relative.to_path_buf();
    slug_path.set_extension("");
    let slug = slash_path(&slug_path)
        .ok_or_else(|| format!("generated page path is not UTF-8: {}", path.display()))?;

    Ok(SearchIndexPage {
        namespace,
        slug,
        title: frontmatter.title,
        body: body.to_owned(),
        description: frontmatter.description,
        search_exclude: frontmatter.search_exclude,
        draft: frontmatter.draft,
        unlisted: frontmatter.unlisted,
        category_no_page: frontmatter.category_no_page,
    })
}

fn split_frontmatter(source: &str) -> Option<(&str, &str)> {
    let source = source.strip_prefix("---\n")?;
    source.split_once("\n---\n")
}

/// Parse the fixed scalar subset emitted by this repository's two generators.
/// Keeping collection on that producer-owned format avoids adding a second
/// YAML dependency to the Tauri host merely to recover six known keys.
fn parse_generated_frontmatter(source: &str) -> Frontmatter {
    let mut frontmatter = Frontmatter::default();
    for line in source.lines() {
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let value = value.trim();
        match key.trim() {
            "title" => frontmatter.title = generated_scalar(value),
            "description" => frontmatter.description = generated_scalar(value),
            "search_exclude" => frontmatter.search_exclude = value == "true",
            "draft" => frontmatter.draft = value == "true",
            "unlisted" => frontmatter.unlisted = value == "true",
            "category_no_page" => frontmatter.category_no_page = value == "true",
            _ => {}
        }
    }
    frontmatter
}

fn generated_scalar(value: &str) -> String {
    let Some(quoted) = value
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
    else {
        return value.to_owned();
    };
    let mut decoded = String::with_capacity(quoted.len());
    let mut chars = quoted.chars();
    while let Some(character) = chars.next() {
        if character != '\\' {
            decoded.push(character);
            continue;
        }
        match chars.next() {
            Some('n') => decoded.push('\n'),
            Some('r') => decoded.push('\r'),
            Some('t') => decoded.push('\t'),
            Some('"') => decoded.push('"'),
            Some('\\') => decoded.push('\\'),
            Some(other) => {
                decoded.push('\\');
                decoded.push(other);
            }
            None => decoded.push('\\'),
        }
    }
    decoded
}

fn slash_path(path: &Path) -> Option<String> {
    let parts = path
        .components()
        .map(|component| component.as_os_str().to_str())
        .collect::<Option<Vec<_>>>()?;
    Some(parts.join("/"))
}

/// Rebuild and atomically replace the shared runtime index.
pub(crate) fn publish_search_index(workspace: &Path, docs_dir: &Path) -> Result<(), String> {
    let pages = collect_generated_pages(docs_dir)?;
    let json = search_index_json(pages)
        .map_err(|error| format!("serialize generated search index: {error}"))?;
    atomic_replace(&workspace.join(SEARCH_INDEX_RELATIVE_PATH), json.as_bytes())
        .map_err(|error| format!("publish search index: {error}"))
}

fn atomic_replace(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let parent = path.parent().ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::InvalidInput, "index path has no parent")
    })?;
    fs::create_dir_all(parent)?;

    let mut last_collision = None;
    for _ in 0..100 {
        let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let temp = parent.join(format!(
            ".search-index.json.tmp-{}-{sequence}",
            std::process::id()
        ));
        match OpenOptions::new().write(true).create_new(true).open(&temp) {
            Ok(mut file) => {
                let result = (|| {
                    file.write_all(bytes)?;
                    file.sync_all()?;
                    drop(file);
                    fs::rename(&temp, path)
                })();
                if result.is_err() {
                    let _ = fs::remove_file(&temp);
                }
                return result;
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                last_collision = Some(error);
            }
            Err(error) => return Err(error),
        }
    }
    Err(last_collision.unwrap_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            "could not reserve search-index temp file",
        )
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;
    use std::sync::Arc;
    use std::thread;

    fn page(root: &Path, relative: &str, frontmatter: &str, body: &str) {
        let path = root.join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, format!("---\n{frontmatter}\n---\n\n{body}\n")).unwrap();
    }

    #[test]
    fn collection_covers_both_namespaces_and_exclusion_schema() {
        let temp = tempfile::TempDir::new().unwrap();
        let docs = temp.path().join("docs");
        page(
            &docs,
            "claude-md/global.mdx",
            "title: Claude\ndescription: Claude description",
            "# Claude body",
        );
        page(
            &docs,
            "codex-skills/tool/index.mdx",
            "title: Codex\ndescription: Codex description",
            "Codex body",
        );
        page(
            &docs,
            "codex-skills/tool/private.mdx",
            "title: Private\nunlisted: true",
            "hidden",
        );
        page(&docs, "claude/index.mdx", "title: Landing", "ignored");

        let json = search_index_json(collect_generated_pages(&docs).unwrap()).unwrap();
        let rows = serde_json::from_str::<Vec<Value>>(&json).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0]["id"], "claude:claude-md/global");
        assert_eq!(rows[1]["id"], "codex:codex-skills/tool");
        for row in rows {
            let keys = row.as_object().unwrap().keys().cloned().collect::<Vec<_>>();
            assert_eq!(keys, ["body", "description", "id", "title", "url"]);
        }
    }

    #[test]
    fn selection_is_derived_from_exact_namespaces_and_empty_is_overwritten() {
        let temp = tempfile::TempDir::new().unwrap();
        let workspace = temp.path();
        let docs = workspace.join("src/content/docs");
        page(&docs, "claude-md/a.mdx", "title: A", "a");
        publish_search_index(workspace, &docs).unwrap();
        assert!(
            fs::read_to_string(workspace.join(SEARCH_INDEX_RELATIVE_PATH))
                .unwrap()
                .contains("claude:claude-md/a")
        );

        fs::remove_dir_all(docs.join("claude-md")).unwrap();
        page(&docs, "codex-config/b.mdx", "title: B", "b");
        publish_search_index(workspace, &docs).unwrap();
        let selected = fs::read_to_string(workspace.join(SEARCH_INDEX_RELATIVE_PATH)).unwrap();
        assert!(!selected.contains("claude:"));
        assert!(selected.contains("codex:codex-config/b"));

        fs::remove_dir_all(docs.join("codex-config")).unwrap();
        publish_search_index(workspace, &docs).unwrap();
        assert_eq!(
            fs::read_to_string(workspace.join(SEARCH_INDEX_RELATIVE_PATH)).unwrap(),
            "[]"
        );
    }

    #[test]
    fn atomic_replacement_never_exposes_partial_json() {
        let temp = tempfile::TempDir::new().unwrap();
        let target = Arc::new(temp.path().join("search-index.json"));
        atomic_replace(&target, b"[]").unwrap();
        let writer_target = target.clone();
        let writer = thread::spawn(move || {
            for iteration in 0..300 {
                let body = "x".repeat(if iteration % 2 == 0 { 8 } else { 32_000 });
                let json = format!(r#"[{{"iteration":{iteration},"body":"{body}"}}]"#);
                atomic_replace(&writer_target, json.as_bytes()).unwrap();
            }
        });
        while !writer.is_finished() {
            let bytes = fs::read(&*target).unwrap();
            serde_json::from_slice::<Value>(&bytes).expect("target must always be complete JSON");
        }
        writer.join().unwrap();
        assert!(
            fs::read_dir(temp.path()).unwrap().all(|entry| !entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .contains(".tmp-")),
            "atomic publication must not leave temp files"
        );
    }

    #[test]
    fn publication_does_not_modify_workspace_ready_sentinel() {
        let temp = tempfile::TempDir::new().unwrap();
        let workspace = temp.path();
        let docs = workspace.join("src/content/docs");
        fs::create_dir_all(&docs).unwrap();
        let sentinel = workspace.join(crate::WORKSPACE_READY_FILE);
        fs::write(&sentinel, "digest-token").unwrap();

        publish_search_index(workspace, &docs).unwrap();

        assert_eq!(fs::read_to_string(sentinel).unwrap(), "digest-token");
        let allowlist = include_str!("../../scripts/runtime-workspace-files.mjs");
        assert!(!allowlist.contains(SEARCH_INDEX_RELATIVE_PATH));
    }
}
