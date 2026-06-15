use axum::body::Body;
use axum::http::{Request, StatusCode};
use tower::ServiceExt;

use super::helpers::*;

/// Workspace-relative path of the seed README under the resolved workspace
/// root (`<instance>/nous/workspace`).
const SEED_MARKDOWN: &str = "README.md";

/// Absolute path to the resolved workspace root inside the instance tempdir.
fn workspace_root(instance_dir: &tempfile::TempDir) -> std::path::PathBuf {
    instance_dir.path().join("theke")
}

/// Build a workspace test app whose request body limit exceeds the handler's
/// 10 MiB write cap.
///
/// WHY: the default test `SecurityConfig` body limit is 1 MiB, which would
/// reject an over-cap write at the middleware layer before the handler's own
/// 413 size check runs. Tests that must exercise the *handler* cap need the
/// transport limit raised above it.
async fn app_with_large_body_limit() -> (axum::Router, tempfile::TempDir) {
    let (state, dir) = test_state().await;
    let security = SecurityConfig {
        body_limit_bytes: 32 * 1024 * 1024,
        csrf: crate::security::CsrfConfig {
            enabled: false,
            disable_acknowledged: true,
            ..crate::security::CsrfConfig::default()
        },
        ..SecurityConfig::default()
    };
    (build_router(state, &security), dir)
}

#[tokio::test]
async fn workspace_root_listing_includes_seed_files() {
    let (app, _dir) = app().await;
    let resp = app
        .oneshot(authed_get("/api/v1/workspace/files"))
        .await
        .expect("response");

    assert_eq!(resp.status(), StatusCode::OK);
    let entries = body_json(resp).await;
    let paths: Vec<String> = entries
        .as_array()
        .expect("array")
        .iter()
        .map(|entry| entry["path"].as_str().unwrap_or_default().to_owned())
        .collect();
    assert!(paths.contains(&"README.md".to_owned()));
    assert!(paths.contains(&"src".to_owned()));
}

#[tokio::test]
async fn workspace_file_content_returns_raw_text() {
    let (app, _dir) = app().await;
    let resp = app
        .oneshot(authed_get("/api/v1/workspace/files/content?path=README.md"))
        .await
        .expect("response");

    assert_eq!(resp.status(), StatusCode::OK);
    let content_type = resp
        .headers()
        .get(axum::http::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();
    assert!(content_type.starts_with("text/"));

    let body = body_string(resp).await;
    assert!(body.contains("Workspace root fixture."));
}

#[tokio::test]
async fn workspace_path_traversal_is_rejected() {
    let (app, _dir) = app().await;
    let req = Request::builder()
        .method("GET")
        .uri("/api/v1/workspace/files/content?path=../nous/syn/SOUL.md")
        .header("authorization", format!("Bearer {}", default_token()))
        .body(Body::empty())
        .expect("request");

    let resp = app.oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn workspace_git_status_is_empty_when_not_a_repo() {
    let (app, _dir) = app().await;
    let resp = app
        .oneshot(authed_get("/api/v1/workspace/git-status"))
        .await
        .expect("response");

    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(body_json(resp).await, serde_json::json!([]));
}

#[tokio::test]
async fn workspace_write_round_trips_with_get_content() {
    let (app, _dir) = app().await;
    let new_content = "# Notes\n\nUpdated by the editor.\n";

    let write = authed_request(
        "PUT",
        "/api/v1/workspace/files/content",
        Some(serde_json::json!({ "path": SEED_MARKDOWN, "content": new_content })),
    );
    let resp = app.clone().oneshot(write).await.expect("write response");
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;
    assert_eq!(body["path"], SEED_MARKDOWN);
    assert_eq!(
        body["size"].as_u64().expect("size"),
        u64::try_from(new_content.len()).expect("test content length fits u64")
    );
    assert!(body["mtime_ms"].as_i64().expect("mtime_ms") > 0);

    let read = authed_get("/api/v1/workspace/files/content?path=README.md");
    let resp = app.oneshot(read).await.expect("read response");
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(body_string(resp).await, new_content);
}

#[tokio::test]
async fn workspace_write_rejects_non_operator_token() {
    let (app, dir) = app().await;

    let write = authed_request_as(
        "PUT",
        "/api/v1/workspace/files/content",
        Some(serde_json::json!({ "path": "readonly-write.md", "content": "blocked\n" })),
        symbolon::types::Role::Agent,
    );
    let resp = app.oneshot(write).await.expect("write response");
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    assert!(
        !workspace_root(&dir).join("readonly-write.md").exists(),
        "non-operator write must not create files"
    );
}

#[tokio::test]
async fn workspace_write_creates_new_file() {
    let (app, dir) = app().await;

    let write = authed_request(
        "PUT",
        "/api/v1/workspace/files/content",
        Some(serde_json::json!({ "path": "fresh-note.md", "content": "brand new\n" })),
    );
    let resp = app.oneshot(write).await.expect("write response");
    assert_eq!(resp.status(), StatusCode::OK);

    let on_disk = std::fs::read_to_string(workspace_root(&dir).join("fresh-note.md"))
        .expect("new file should exist on disk");
    assert_eq!(on_disk, "brand new\n");
}

#[tokio::test]
async fn workspace_write_rejects_path_traversal() {
    let (app, _dir) = app().await;

    let write = authed_request(
        "PUT",
        "/api/v1/workspace/files/content",
        Some(serde_json::json!({ "path": "../theke/USER.md", "content": "pwned" })),
    );
    let resp = app.oneshot(write).await.expect("write response");
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn workspace_write_rejects_oversize_content() {
    // WHY: the transport body limit is raised above the handler cap so this
    // exercises the handler's own 413 size check, not the middleware limit.
    let (app, _dir) = app_with_large_body_limit().await;
    let oversize = "a".repeat(10 * 1024 * 1024 + 1);

    let write = authed_request(
        "PUT",
        "/api/v1/workspace/files/content",
        Some(serde_json::json!({ "path": "huge.md", "content": oversize })),
    );
    let resp = app.oneshot(write).await.expect("write response");
    assert_eq!(resp.status(), StatusCode::PAYLOAD_TOO_LARGE);
}

#[tokio::test]
async fn workspace_write_accepts_large_under_cap_content() {
    // WHY: a multi-megabyte note (under the 10 MiB cap) must succeed when the
    // transport limit allows it — confirming the handler cap, not an
    // incidental limit, is the binding constraint.
    let (app, dir) = app_with_large_body_limit().await;
    let big = "x".repeat(2 * 1024 * 1024);

    let write = authed_request(
        "PUT",
        "/api/v1/workspace/files/content",
        Some(serde_json::json!({ "path": "big.md", "content": big.clone() })),
    );
    let resp = app.oneshot(write).await.expect("write response");
    assert_eq!(resp.status(), StatusCode::OK);

    let on_disk =
        std::fs::read_to_string(workspace_root(&dir).join("big.md")).expect("file on disk");
    assert_eq!(on_disk.len(), big.len());
}

#[tokio::test]
async fn workspace_write_rejects_disallowed_extension() {
    let (app, _dir) = app().await;

    let write = authed_request(
        "PUT",
        "/api/v1/workspace/files/content",
        Some(serde_json::json!({ "path": "evil.sh", "content": "#!/bin/sh\nrm -rf /\n" })),
    );
    let resp = app.oneshot(write).await.expect("write response");
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn workspace_write_rejects_directory_target() {
    let (app, _dir) = app().await;

    // WHY: `src` is a seed directory; the handler must reject it before ever
    // attempting an atomic write so a directory is never clobbered.
    let write = authed_request(
        "PUT",
        "/api/v1/workspace/files/content",
        Some(serde_json::json!({ "path": "src", "content": "nope" })),
    );
    let resp = app.oneshot(write).await.expect("write response");
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn workspace_write_leaves_no_orphan_tmp_files() {
    let (app, dir) = app().await;

    let write = authed_request(
        "PUT",
        "/api/v1/workspace/files/content",
        Some(serde_json::json!({ "path": SEED_MARKDOWN, "content": "clean write\n" })),
    );
    let resp = app.oneshot(write).await.expect("write response");
    assert_eq!(resp.status(), StatusCode::OK);

    // WHY: the atomic write stages to a hidden `.<name>.<ulid>.tmp` sibling and
    // renames it into place. After a successful write no temp artifact may
    // remain in the operator's vault.
    let orphans: Vec<String> = std::fs::read_dir(workspace_root(&dir))
        .expect("read workspace root")
        .filter_map(std::result::Result::ok)
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| {
            std::path::Path::new(name)
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("tmp"))
        })
        .collect();
    assert!(
        orphans.is_empty(),
        "atomic write left orphan temp files: {orphans:?}"
    );
}

#[tokio::test]
async fn workspace_write_mtime_mismatch_conflicts() {
    let (app, _dir) = app().await;

    let write = authed_request(
        "PUT",
        "/api/v1/workspace/files/content",
        Some(serde_json::json!({
            "path": SEED_MARKDOWN,
            "content": "stale edit\n",
            "if_match_mtime_ms": 1_i64,
        })),
    );
    let resp = app.oneshot(write).await.expect("write response");
    assert_eq!(resp.status(), StatusCode::CONFLICT);
}

#[tokio::test]
async fn workspace_open_rejects_path_traversal() {
    let (app, _dir) = app().await;

    let open = authed_request(
        "POST",
        "/api/v1/workspace/open",
        Some(serde_json::json!({ "path": "../theke/USER.md" })),
    );
    let resp = app.oneshot(open).await.expect("open response");
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn workspace_open_rejects_non_operator_token() {
    let (app, _dir) = app().await;

    let open = authed_request_as(
        "POST",
        "/api/v1/workspace/open",
        Some(serde_json::json!({ "path": SEED_MARKDOWN })),
        symbolon::types::Role::Readonly,
    );
    let resp = app.oneshot(open).await.expect("open response");
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn workspace_open_rejects_missing_file() {
    let (app, _dir) = app().await;

    let open = authed_request(
        "POST",
        "/api/v1/workspace/open",
        Some(serde_json::json!({ "path": "does-not-exist.pdf" })),
    );
    let resp = app.oneshot(open).await.expect("open response");
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn workspace_open_rejects_directory_target() {
    let (app, _dir) = app().await;

    let open = authed_request(
        "POST",
        "/api/v1/workspace/open",
        Some(serde_json::json!({ "path": "src" })),
    );
    let resp = app.oneshot(open).await.expect("open response");
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}
