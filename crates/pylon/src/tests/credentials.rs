use axum::body::Body;
use axum::http::{Request, StatusCode};
use koina::http::{BEARER_PREFIX, CONTENT_TYPE_JSON};
use symbolon::types::Role;
use tower::ServiceExt;

use super::helpers::*;

#[tokio::test]
async fn credentials_reject_non_operator() {
    let (app, _dir) = app().await;

    let resp = app
        .oneshot(authed_get_as("/api/v1/system/credentials", Role::Readonly))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn credentials_ignore_auth_mode_none_anonymous_bypass() {
    let (app, _dir) = app_with_auth_mode("none").await;

    let resp = app
        .oneshot(
            Request::get("/api/v1/system/credentials")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn credentials_list_redacts_secret_material() {
    let (app, _dir) = app().await;

    let resp = app
        .oneshot(authed_get("/api/v1/system/credentials"))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_string(resp).await;
    assert!(body.contains("anthropic:primary"));
    assert!(body.contains("..."));
    assert!(!body.contains("sk-ant-test-key-for-health-checks"));
    assert!(!body.contains("health-checks"));
}

#[tokio::test]
async fn credentials_validate_redacts_secret_material() {
    let (app, _dir) = app().await;

    let resp = app
        .oneshot(authed_request(
            "POST",
            "/api/v1/system/credentials/anthropic:primary/validate",
            None,
        ))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_string(resp).await;
    assert!(body.contains(r#""status":"valid""#));
    assert!(body.contains("last_validated"));
    assert!(!body.contains("sk-ant-test-key-for-health-checks"));
    assert!(!body.contains("health-checks"));
}

#[tokio::test]
async fn credentials_usage_counters_are_unavailable_not_zero() {
    let (app, _dir) = app().await;

    let list = app
        .oneshot(authed_get("/api/v1/system/credentials"))
        .await
        .unwrap();
    assert_eq!(list.status(), StatusCode::OK);
    let body = body_string(list).await;
    assert!(body.contains(r#""usage_counters_available":false"#));
    // WHY: placeholder counters must not be serialized as factual zeros (#4922).
    assert!(!body.contains("\"requests_today\""));
    assert!(!body.contains("\"tokens_today\""));
}

#[tokio::test]
async fn credentials_add_list_remove_roundtrip() {
    let (app, _dir) = app().await;
    let raw_secret = "sk-test-roundtrip-secret-9999";

    let add = app
        .clone()
        .oneshot(authed_request(
            "POST",
            "/api/v1/system/credentials",
            Some(serde_json::json!({
                "provider": "anthropic",
                "key": raw_secret,
                "role": "backup"
            })),
        ))
        .await
        .unwrap();
    assert_eq!(add.status(), StatusCode::CREATED);
    let add_body = body_string(add).await;
    assert!(add_body.contains("anthropic:backup"));
    assert!(!add_body.contains(raw_secret));
    assert!(!add_body.contains("roundtrip"));

    let list = app
        .clone()
        .oneshot(authed_get("/api/v1/system/credentials"))
        .await
        .unwrap();
    assert_eq!(list.status(), StatusCode::OK);
    let list_body = body_string(list).await;
    assert!(list_body.contains("anthropic:backup"));
    assert!(!list_body.contains(raw_secret));
    assert!(!list_body.contains("roundtrip"));

    let remove = app
        .oneshot(authed_delete("/api/v1/system/credentials/anthropic:backup"))
        .await
        .unwrap();
    assert_eq!(remove.status(), StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn credentials_rotate_endpoint_redacts_response() {
    let (app, _dir) = app().await;
    let raw_secret = "sk-test-rotate-secret-2222";

    let add = app
        .clone()
        .oneshot(authed_request(
            "POST",
            "/api/v1/system/credentials",
            Some(serde_json::json!({
                "provider": "anthropic",
                "key": raw_secret,
                "role": "backup"
            })),
        ))
        .await
        .unwrap();
    assert_eq!(add.status(), StatusCode::CREATED);

    let rotate = app
        .oneshot(authed_request(
            "POST",
            "/api/v1/system/credentials/rotate?provider=anthropic",
            None,
        ))
        .await
        .unwrap();
    assert_eq!(rotate.status(), StatusCode::OK);
    let body = body_string(rotate).await;
    assert!(body.contains("anthropic:primary"));
    assert!(body.contains("anthropic:backup"));
    assert!(!body.contains(raw_secret));
    assert!(!body.contains("rotate-secret"));
}

#[tokio::test]
async fn credentials_post_rejects_non_operator() {
    let (app, _dir) = app().await;
    let token = token_for_role(Role::Agent);
    let req = Request::builder()
        .method("POST")
        .uri("/api/v1/system/credentials")
        .header("content-type", CONTENT_TYPE_JSON)
        .header("authorization", format!("{BEARER_PREFIX}{token}"))
        .body(Body::from(
            serde_json::to_vec(&serde_json::json!({
                "provider": "anthropic",
                "key": "sk-test-agent-denied",
                "role": "backup"
            }))
            .unwrap(),
        ))
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();

    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
}
