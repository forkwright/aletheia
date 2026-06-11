use axum::body::Body;
use axum::http::{Request, StatusCode};
use tower::ServiceExt;

use super::helpers::*;

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
        .uri("/api/v1/workspace/files/content?path=../theke/USER.md")
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
