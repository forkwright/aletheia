//! Knowledge endpoint error handling tests.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use tower::ServiceExt;

use super::helpers::*;

/// Error path: `get_fact` with unknown `fact_id` returns 404 Not Found.
#[tokio::test]
async fn get_fact_unknown_id_returns_404() {
    let (app, _dir) = app().await;
    let resp = app
        .oneshot(authed_get("/api/v1/knowledge/facts/nonexistent-fact-id"))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    let body = body_json(resp).await;
    assert_eq!(body["error"]["code"], "not_found");
}

/// Error path: `list_facts` with invalid sort parameter returns 400 Bad Request.
#[tokio::test]
async fn list_facts_invalid_sort_returns_400() {
    let (app, _dir) = app().await;
    let resp = app
        .oneshot(authed_get("/api/v1/knowledge/facts?sort=invalid_field"))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let body = body_json(resp).await;
    assert_eq!(body["error"]["code"], "bad_request");
    assert!(
        body["error"]["message"]
            .as_str()
            .unwrap()
            .contains("invalid sort field")
    );
}

#[tokio::test]
async fn knowledge_read_routes_reject_missing_bearer_token_in_token_mode() {
    let (app, _dir) = app().await;
    let routes = [
        "/api/v1/knowledge/facts",
        "/api/v1/knowledge/facts/nonexistent-fact-id",
        "/api/v1/knowledge/entities",
        "/api/v1/knowledge/entities/some-id",
        "/api/v1/knowledge/entities/some-id/memories",
        "/api/v1/knowledge/entities/some-id/relationships",
        "/api/v1/knowledge/search?q=memory",
        "/api/v1/knowledge/timeline",
        "/api/v1/knowledge/check",
    ];

    for route in routes {
        let resp = app
            .clone()
            .oneshot(Request::get(route).body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED, "{route}");
    }
}

#[tokio::test]
async fn knowledge_entity_merge_route_is_not_shadowed_by_entity_catch_all() {
    let (app, _dir) = app().await;
    let req = authed_request(
        "POST",
        "/api/v1/knowledge/entities/merge",
        Some(serde_json::json!({
            "primary_id": "entity-a",
            "secondary_id": "entity-b",
        })),
    );

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
}

#[tokio::test]
async fn knowledge_read_route_accepts_valid_bearer_token_in_token_mode() {
    let (app, _dir) = app().await;
    let resp = app
        .oneshot(authed_get("/api/v1/knowledge/facts"))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn knowledge_read_route_allows_missing_bearer_token_in_none_mode() {
    let (app, _dir) = app_with_auth_mode("none").await;
    let resp = app
        .oneshot(
            Request::get("/api/v1/knowledge/facts")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
}

/// Error path: `list_facts` with invalid order parameter returns 400 Bad Request.
#[tokio::test]
async fn list_facts_invalid_order_returns_400() {
    let (app, _dir) = app().await;
    let resp = app
        .oneshot(authed_get("/api/v1/knowledge/facts?order=upward"))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let body = body_json(resp).await;
    assert_eq!(body["error"]["code"], "bad_request");
    assert!(
        body["error"]["message"]
            .as_str()
            .unwrap()
            .contains("invalid order")
    );
}

/// Error path: `forget_fact` returns 503 Service Unavailable when knowledge store not enabled.
#[tokio::test]
async fn forget_fact_without_knowledge_store_returns_503() {
    let (app, _dir) = app().await;
    let req = authed_request(
        "POST",
        "/api/v1/knowledge/facts/some-fact-id/forget",
        Some(serde_json::json!({})),
    );
    let resp = app.oneshot(req).await.unwrap();

    assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
    let body = body_json(resp).await;
    assert_eq!(body["error"]["code"], "service_unavailable");
}

/// Error path: `restore_fact` returns 503 Service Unavailable when knowledge store not enabled.
#[tokio::test]
async fn restore_fact_without_knowledge_store_returns_503() {
    let (app, _dir) = app().await;
    let req = authed_request("POST", "/api/v1/knowledge/facts/some-fact-id/restore", None);
    let resp = app.oneshot(req).await.unwrap();

    assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
    let body = body_json(resp).await;
    assert_eq!(body["error"]["code"], "service_unavailable");
}

/// Error path: `update_confidence` returns 503 Service Unavailable when knowledge store not enabled.
#[tokio::test]
async fn update_confidence_without_knowledge_store_returns_503() {
    let (app, _dir) = app().await;
    let req = authed_request(
        "PUT",
        "/api/v1/knowledge/facts/some-fact-id/confidence",
        Some(serde_json::json!({ "confidence": 0.8 })),
    );
    let resp = app.oneshot(req).await.unwrap();

    assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
    let body = body_json(resp).await;
    assert_eq!(body["error"]["code"], "service_unavailable");
}

/// Error path: `update_confidence` with out-of-range confidence returns 400 Bad Request.
#[tokio::test]
async fn update_confidence_out_of_range_returns_400() {
    let (app, _dir) = app().await;
    let req = authed_request(
        "PUT",
        "/api/v1/knowledge/facts/some-fact-id/confidence",
        Some(serde_json::json!({ "confidence": 1.5 })),
    );
    let resp = app.oneshot(req).await.unwrap();

    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let body = body_json(resp).await;
    assert_eq!(body["error"]["code"], "bad_request");
    assert!(
        body["error"]["message"]
            .as_str()
            .unwrap()
            .contains("between 0.0 and 1.0")
    );
}

/// Error path: `update_confidence` with negative confidence returns 400 Bad Request.
#[tokio::test]
async fn update_confidence_negative_returns_400() {
    let (app, _dir) = app().await;
    let req = authed_request(
        "PUT",
        "/api/v1/knowledge/facts/some-fact-id/confidence",
        Some(serde_json::json!({ "confidence": -0.5 })),
    );
    let resp = app.oneshot(req).await.unwrap();

    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let body = body_json(resp).await;
    assert_eq!(body["error"]["code"], "bad_request");
}

/// Error path: `check_graph_health` returns 503 when knowledge store not enabled.
#[tokio::test]
async fn check_graph_health_without_knowledge_store_returns_503() {
    let (app, _dir) = app().await;
    let resp = app
        .oneshot(authed_get("/api/v1/knowledge/check"))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
    let body = body_json(resp).await;
    assert_eq!(body["error"]["code"], "service_unavailable");
}

/// Error path: `entity_relationships` returns 503 when knowledge store not enabled.
#[tokio::test]
async fn entity_relationships_without_knowledge_store_returns_503() {
    let (app, _dir) = app().await;
    let resp = app
        .oneshot(authed_get(
            "/api/v1/knowledge/entities/some-id/relationships",
        ))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
    let body = body_json(resp).await;
    assert_eq!(body["error"]["code"], "service_unavailable");
}

/// Error path: `list_entities` returns empty list when knowledge store not enabled.
#[tokio::test]
async fn list_entities_without_knowledge_store_returns_empty() {
    let (app, _dir) = app().await;
    let resp = app
        .oneshot(authed_get("/api/v1/knowledge/entities"))
        .await
        .unwrap();

    // This endpoint returns empty array when store not available
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;
    assert!(body["entities"].is_array());
}
