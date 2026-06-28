use axum::body::Body;
use axum::http::{HeaderValue, Request, StatusCode};
use koina::http::{BEARER_PREFIX, CONTENT_TYPE_JSON};
use symbolon::types::Role;
use tower::ServiceExt;

use super::helpers::*;

fn effect_from_body(body: &serde_json::Value) -> Option<String> {
    body.get("runtime_effect")
        .and_then(serde_json::Value::as_str)
        .map(String::from)
}

fn with_request_id(mut request: Request<Body>, request_id: &'static str) -> Request<Body> {
    request
        .headers_mut()
        .insert("x-request-id", HeaderValue::from_static(request_id));
    request
}

async fn credential_audit_events(
    state: &std::sync::Arc<AppState>,
) -> Vec<crate::event_bus::DomainEvent> {
    let (snapshot, _rx) = state.event_bus.subscribe_from(0).await;
    snapshot
        .replay
        .into_iter()
        .filter(|event| event.topic == "credential.audit")
        .collect()
}

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
async fn credential_operations_emit_redacted_audit_events() {
    let (state, _dir) = test_state().await;
    let app = build_router(std::sync::Arc::clone(&state), &test_security_config());
    let raw_secret = "sk-test-audit-secret-4878";

    let add = app
        .clone()
        .oneshot(with_request_id(
            authed_request(
                "POST",
                "/api/v1/system/credentials",
                Some(serde_json::json!({
                    "provider": "anthropic",
                    "key": raw_secret,
                    "role": "backup"
                })),
            ),
            "req-credential-add",
        ))
        .await
        .unwrap();
    assert_eq!(add.status(), StatusCode::CREATED);

    let validate = app
        .clone()
        .oneshot(with_request_id(
            authed_request(
                "POST",
                "/api/v1/system/credentials/anthropic:backup/validate",
                None,
            ),
            "req-credential-validate",
        ))
        .await
        .unwrap();
    assert_eq!(validate.status(), StatusCode::OK);

    let rotate = app
        .clone()
        .oneshot(with_request_id(
            authed_request(
                "POST",
                "/api/v1/system/credentials/rotate?provider=anthropic",
                None,
            ),
            "req-credential-rotate",
        ))
        .await
        .unwrap();
    assert_eq!(rotate.status(), StatusCode::OK);

    let remove = app
        .oneshot(with_request_id(
            authed_delete("/api/v1/system/credentials/anthropic:backup"),
            "req-credential-remove",
        ))
        .await
        .unwrap();
    assert_eq!(remove.status(), StatusCode::OK);

    let events = credential_audit_events(&state).await;
    let actions: std::collections::HashSet<_> = events
        .iter()
        .filter_map(|event| event.payload["action"].as_str())
        .collect();
    assert_eq!(
        actions,
        ["add", "validate", "rotate", "remove"]
            .into_iter()
            .collect()
    );

    let add_event = events
        .iter()
        .find(|event| event.payload["action"] == "add")
        .expect("add audit event");
    assert_eq!(add_event.payload["principal"], "test-user");
    assert_eq!(add_event.payload["actor"]["role"], "operator");
    assert_eq!(add_event.payload["provider"], "anthropic");
    assert_eq!(add_event.payload["role"], "backup");
    assert_eq!(add_event.payload["result"], "success");
    assert_eq!(add_event.payload["request_id"], "req-credential-add");
    assert_eq!(add_event.payload["runtime_effect"], "restart_required");

    let validate_event = events
        .iter()
        .find(|event| event.payload["action"] == "validate")
        .expect("validate audit event");
    assert_eq!(
        validate_event.payload["runtime_effect"],
        "no_runtime_change"
    );

    let serialized = serde_json::to_string(&events).unwrap();
    assert!(!serialized.contains(raw_secret));
    assert!(!serialized.contains("audit-secret"));
}

#[tokio::test]
async fn credential_add_failure_emits_audit_event_and_health_result() {
    let (state, _dir) = test_state().await;
    let app = build_router(std::sync::Arc::clone(&state), &test_security_config());
    let raw_secret = "sk-test-audit-failure-secret-4878";

    let add = app
        .clone()
        .oneshot(with_request_id(
            authed_request(
                "POST",
                "/api/v1/system/credentials",
                Some(serde_json::json!({
                    "provider": "openai",
                    "key": raw_secret,
                    "role": "primary"
                })),
            ),
            "req-credential-add-fail",
        ))
        .await
        .unwrap();
    assert_eq!(add.status(), StatusCode::BAD_REQUEST);

    let events = credential_audit_events(&state).await;
    let event = events
        .iter()
        .find(|event| event.payload["action"] == "add")
        .expect("failure audit event");
    assert_eq!(event.payload["provider"], "openai");
    assert_eq!(event.payload["role"], "primary");
    assert_eq!(event.payload["result"], "failure");
    assert_eq!(event.payload["request_id"], "req-credential-add-fail");
    assert_eq!(event.payload["runtime_effect"], "not_applied");
    assert_eq!(event.payload["error_code"], "bad_request");

    let health = app
        .oneshot(authed_get("/api/v1/system/health"))
        .await
        .unwrap();
    assert_eq!(health.status(), StatusCode::OK);
    let body = body_json(health).await;
    let runtime_check = body["checks"]
        .as_array()
        .expect("checks")
        .iter()
        .find(|check| check["name"] == "credential_runtime")
        .expect("credential_runtime check");
    assert_eq!(runtime_check["status"], "warn");
    assert_eq!(
        runtime_check["details"]["last_mutation_result"]["result"],
        "failure"
    );
    assert_eq!(
        runtime_check["details"]["last_mutation_result"]["error_code"],
        "bad_request"
    );

    let serialized = serde_json::to_string(&events).unwrap();
    assert!(!serialized.contains(raw_secret));
    assert!(!serialized.contains("failure-secret"));
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
    // WHY(#4872): removal now returns the typed runtime effect instead of a
    // plain 204 that would imply the live provider chain changed.
    assert_eq!(remove.status(), StatusCode::OK);
    let remove_body = body_json(remove).await;
    assert_eq!(
        remove_body.get("runtime_effect").and_then(|v| v.as_str()),
        Some("restart_required")
    );
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
async fn credentials_add_after_degraded_start_reports_restart_required() {
    let (app, _dir) = app_no_providers().await;
    let raw_secret = "sk-test-degraded-start-secret";

    let add = app
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
    let body = body_json(add).await;
    assert_eq!(
        body.get("provider").and_then(|v| v.as_str()),
        Some("anthropic")
    );
    assert_eq!(effect_from_body(&body).as_deref(), Some("restart_required"));
}

#[tokio::test]
async fn credentials_rotate_live_provider_reports_restart_required() {
    let (app, _dir) = app_with_anthropic_provider().await;
    let raw_secret = "sk-test-rotate-live-secret";

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
    let body = body_json(rotate).await;
    assert_eq!(effect_from_body(&body).as_deref(), Some("restart_required"));
}

#[tokio::test]
async fn credentials_delete_live_provider_reports_restart_required() {
    let (app, _dir) = app_with_anthropic_provider().await;
    let raw_secret = "sk-test-delete-live-secret";

    // Add a backup so the primary can be removed without the last-primary guard.
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

    let remove = app
        .oneshot(authed_delete(
            "/api/v1/system/credentials/anthropic:primary",
        ))
        .await
        .unwrap();
    assert_eq!(remove.status(), StatusCode::OK);
    let body = body_json(remove).await;
    assert_eq!(
        body.get("runtime_effect").and_then(|v| v.as_str()),
        Some("restart_required")
    );
}

#[tokio::test]
async fn credentials_add_unsupported_provider_rejected() {
    let (app, _dir) = app_with_anthropic_provider().await;

    let add = app
        .oneshot(authed_request(
            "POST",
            "/api/v1/system/credentials",
            Some(serde_json::json!({
                "provider": "openai",
                "key": "sk-test-unsupported",
                "role": "primary"
            })),
        ))
        .await
        .unwrap();
    assert_eq!(add.status(), StatusCode::BAD_REQUEST);
    let body = body_json(add).await;
    let message = body
        .get("error")
        .and_then(|e| e.get("message"))
        .and_then(|m| m.as_str())
        .expect("error message");
    assert!(message.contains("openai"));
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
