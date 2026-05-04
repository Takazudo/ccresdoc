//! Integration tests for ccresdoc-server.
//!
//! Uses `axum::Router` + `tower::ServiceExt::oneshot` (no port binding).

use std::path::PathBuf;

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use http_body_util::BodyExt;
use tower::ServiceExt;

// ---------------------------------------------------------------------------
// Fixture helpers
// ---------------------------------------------------------------------------

/// Create a minimal fixture directory tree under a tempdir.
///
/// Structure:
///   <root>/
///     CLAUDE.md
///     commands/
///       my-cmd.md
///     skills/
///       my-skill/
///         SKILL.md
///         references/
///           ref-one.md
///     agents/
///       my-agent.md
///     dist/
///       index.html
///       404.html
///       ccresdoc-sidebar.js
///       assets/
///         style.css
///       _shell/
///         index.html  (contains both sentinels)
fn make_fixture() -> (tempfile::TempDir, PathBuf, PathBuf) {
    let dir = tempfile::TempDir::new().expect("tempdir");
    let root = dir.path().to_path_buf();

    // CLAUDE.md
    std::fs::write(root.join("CLAUDE.md"), "# Root\n\nHello world.").unwrap();

    // commands/
    let cmd_dir = root.join("commands");
    std::fs::create_dir_all(&cmd_dir).unwrap();
    std::fs::write(
        cmd_dir.join("my-cmd.md"),
        "---\ndescription: does stuff\n---\n# My Command\n\nBody here.",
    )
    .unwrap();

    // skills/my-skill/
    let skill_dir = root.join("skills").join("my-skill");
    std::fs::create_dir_all(skill_dir.join("references")).unwrap();
    std::fs::write(
        skill_dir.join("SKILL.md"),
        "---\nname: My Skill\ndescription: skill desc\n---\n# My Skill\n\nSkill body.",
    )
    .unwrap();
    std::fs::write(
        skill_dir.join("references").join("ref-one.md"),
        "# Reference One\n\nRef body.",
    )
    .unwrap();

    // agents/
    let agents_dir = root.join("agents");
    std::fs::create_dir_all(&agents_dir).unwrap();
    std::fs::write(
        agents_dir.join("my-agent.md"),
        "---\nname: My Agent\ndescription: agent desc\nmodel: claude-opus-4-5\n---\n# My Agent\n\nAgent body.",
    )
    .unwrap();

    // dist/
    let dist_dir = root.join("dist");
    std::fs::create_dir_all(dist_dir.join("assets")).unwrap();
    std::fs::create_dir_all(dist_dir.join("_shell")).unwrap();

    std::fs::write(
        dist_dir.join("index.html"),
        "<html><body>Home</body></html>",
    )
    .unwrap();
    std::fs::write(
        dist_dir.join("404.html"),
        "<html><body>404 Not Found</body></html>",
    )
    .unwrap();
    std::fs::write(dist_dir.join("ccresdoc-sidebar.js"), "// sidebar").unwrap();
    std::fs::write(dist_dir.join("assets").join("style.css"), "body{}").unwrap();

    // Shell with sentinels
    let shell_html = format!(
        "<html><head><title>{}</title></head><body class=\"ccresdoc-shell\"><main>{}</main></body></html>",
        "\u{2603}CCRESDOC_TITLE_SLOT\u{2603}",
        "\u{2603}CCRESDOC_CONTENT_SLOT\u{2603}",
    );
    std::fs::write(dist_dir.join("_shell").join("index.html"), &shell_html).unwrap();

    (dir, root.clone(), dist_dir)
}

/// Build the router from fixtures.
fn make_router(root: &PathBuf, dist_dir: &PathBuf) -> axum::Router {
    use ccresdoc_server::routes_for_test;
    use ccresdoc_server::ServerConfig;

    let config = ServerConfig {
        port: 0, // unused in oneshot mode
        claude_dir: root.clone(),
        project_root: root.clone(),
        dist_dir: dist_dir.clone(),
    };
    routes_for_test(config).expect("build router")
}

async fn body_string(resp: axum::response::Response) -> String {
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    String::from_utf8_lossy(&bytes).into_owned()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn home_returns_200_with_html() {
    let (_dir, root, dist) = make_fixture();
    let router = make_router(&root, &dist);

    let req = Request::builder().uri("/").body(Body::empty()).unwrap();
    let resp = router.oneshot(req).await.unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_string(resp).await;
    assert!(body.contains("Home"), "expected body to contain 'Home'");
}

#[tokio::test]
async fn claude_md_root_returns_200_with_substituted_content() {
    let (_dir, root, dist) = make_fixture();
    let router = make_router(&root, &dist);

    let req = Request::builder()
        .uri("/claude-md/root")
        .body(Body::empty())
        .unwrap();
    let resp = router.oneshot(req).await.unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_string(resp).await;

    // Title should be non-empty and contain page content
    assert!(
        body.contains("<title>") || body.contains("CCResDoc"),
        "expected title in rendered HTML"
    );
    // Rendered markdown content should be present
    assert!(
        body.contains("Hello world") || body.contains("<p>"),
        "expected rendered markdown content"
    );
    // Layout chrome from shell should be present
    assert!(body.contains("ccresdoc-shell"), "expected shell chrome");
    // Sentinels must NOT be present
    assert!(
        !body.contains("\u{2603}CCRESDOC_TITLE_SLOT\u{2603}"),
        "sentinel TITLE must be substituted"
    );
    assert!(
        !body.contains("\u{2603}CCRESDOC_CONTENT_SLOT\u{2603}"),
        "sentinel CONTENT must be substituted"
    );
}

#[tokio::test]
async fn claude_md_root_title_is_non_empty() {
    let (_dir, root, dist) = make_fixture();
    let router = make_router(&root, &dist);

    let req = Request::builder()
        .uri("/claude-md/root")
        .body(Body::empty())
        .unwrap();
    let resp = router.oneshot(req).await.unwrap();
    let body = body_string(resp).await;

    // title tag must contain something meaningful
    if let Some(start) = body.find("<title>") {
        let after = &body[start + 7..];
        if let Some(end) = after.find("</title>") {
            let title = &after[..end];
            assert!(!title.trim().is_empty(), "title must not be empty");
            assert!(title.contains("CCResDoc"), "title must contain CCResDoc");
        }
    }
}

#[tokio::test]
async fn skill_ref_returns_200() {
    let (_dir, root, dist) = make_fixture();
    let router = make_router(&root, &dist);

    let req = Request::builder()
        .uri("/claude-skills/my-skill/ref-ref-one")
        .body(Body::empty())
        .unwrap();
    let resp = router.oneshot(req).await.unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_string(resp).await;
    assert!(body.contains("Ref body") || body.contains("Reference One"));
}

#[tokio::test]
async fn missing_slug_returns_404() {
    let (_dir, root, dist) = make_fixture();
    let router = make_router(&root, &dist);

    let req = Request::builder()
        .uri("/claude-md/does-not-exist")
        .body(Body::empty())
        .unwrap();
    let resp = router.oneshot(req).await.unwrap();

    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    let body = body_string(resp).await;
    assert!(body.contains("404"), "expected 404 page content");
}

#[tokio::test]
async fn api_manifest_parseable_json() {
    let (_dir, root, dist) = make_fixture();
    let router = make_router(&root, &dist);

    let req = Request::builder()
        .uri("/api/manifest.json")
        .body(Body::empty())
        .unwrap();
    let resp = router.oneshot(req).await.unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_string(resp).await;
    let manifest: serde_json::Value = serde_json::from_str(&body).expect("must be valid JSON");
    assert!(manifest["generatedAt"].is_string());
    assert!(manifest["categories"].is_array());
    let cats = manifest["categories"].as_array().unwrap();
    assert_eq!(cats.len(), 4, "must have exactly 4 categories");
}

#[tokio::test]
async fn manifest_no_shell_entry() {
    let (_dir, root, dist) = make_fixture();
    let router = make_router(&root, &dist);

    let req = Request::builder()
        .uri("/api/manifest.json")
        .body(Body::empty())
        .unwrap();
    let resp = router.oneshot(req).await.unwrap();
    let body = body_string(resp).await;
    let manifest: serde_json::Value = serde_json::from_str(&body).unwrap();

    for cat in manifest["categories"].as_array().unwrap() {
        for item in cat["items"].as_array().unwrap() {
            let path = item["path"].as_str().unwrap_or("");
            assert!(
                !path.starts_with("/_shell"),
                "manifest must not contain /_shell paths, got: {}",
                path
            );
        }
    }
}

#[tokio::test]
async fn shell_path_returns_404() {
    let (_dir, root, dist) = make_fixture();

    let req_shell = Request::builder()
        .uri("/_shell")
        .body(Body::empty())
        .unwrap();
    let req_shell_sub = Request::builder()
        .uri("/_shell/index.html")
        .body(Body::empty())
        .unwrap();

    let router = make_router(&root, &dist);
    let resp = router.oneshot(req_shell).await.unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND, "/_shell must be 404");

    let router2 = make_router(&root, &dist);
    let resp2 = router2.oneshot(req_shell_sub).await.unwrap();
    assert_eq!(
        resp2.status(),
        StatusCode::NOT_FOUND,
        "/_shell/index.html must be 404"
    );
}

#[tokio::test]
async fn shell_path_not_200_with_shell_content() {
    // Even if the file exists on disk, /_shell must never return 200
    let (_dir, root, dist) = make_fixture();
    let router = make_router(&root, &dist);

    let req = Request::builder()
        .uri("/_shell")
        .body(Body::empty())
        .unwrap();
    let resp = router.oneshot(req).await.unwrap();
    let status = resp.status();
    let body = body_string(resp).await;

    assert_ne!(status, StatusCode::OK, "/_shell must not return 200");
    // Must not return the shell template with un-replaced sentinels
    assert!(
        !body.contains("\u{2603}CCRESDOC_TITLE_SLOT\u{2603}"),
        "/_shell response must not contain title sentinel"
    );
}

#[tokio::test]
async fn dynamic_page_contains_layout_and_content_and_no_sentinels() {
    let (_dir, root, dist) = make_fixture();
    let router = make_router(&root, &dist);

    let req = Request::builder()
        .uri("/claude-commands/my-cmd")
        .body(Body::empty())
        .unwrap();
    let resp = router.oneshot(req).await.unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_string(resp).await;

    // Layout chrome
    assert!(
        body.contains("ccresdoc-shell"),
        "must contain layout chrome"
    );
    // Rendered markdown content
    assert!(
        body.contains("Body here") || body.contains("<p>"),
        "must contain rendered markdown"
    );
    // No sentinels
    assert!(
        !body.contains("\u{2603}CCRESDOC_TITLE_SLOT\u{2603}"),
        "title sentinel must be replaced"
    );
    assert!(
        !body.contains("\u{2603}CCRESDOC_CONTENT_SLOT\u{2603}"),
        "content sentinel must be replaced"
    );
}

#[tokio::test]
async fn ready_endpoint_returns_200() {
    let (_dir, root, dist) = make_fixture();
    let router = make_router(&root, &dist);

    let req = Request::builder()
        .uri("/___ready")
        .body(Body::empty())
        .unwrap();
    let resp = router.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn assets_returns_css() {
    let (_dir, root, dist) = make_fixture();
    let router = make_router(&root, &dist);

    let req = Request::builder()
        .uri("/assets/style.css")
        .body(Body::empty())
        .unwrap();
    let resp = router.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let ct = resp
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    assert!(
        ct.contains("css"),
        "content-type should be css, got: {}",
        ct
    );
}
