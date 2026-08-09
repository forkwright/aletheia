use axum::body::Body;
use axum::http::{Request, StatusCode};
use koina::http::{BEARER_PREFIX, CONTENT_TYPE_JSON};
use symbolon::types::Role;
use tower::ServiceExt;

use super::helpers::*;

fn effect_from_body(body: &serde_json::Value) -> Option<String> {
    body.get("runtime_effect")
        .and_then(serde_json::Value::as_str)
        .map(String::from)
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
    // WHY(#4875): use a provider this crate has no live-check strategy for
    // (unlike "anthropic"/"claude"), so validation stays network-free and
    // deterministic while still exercising the full add -> validate ->
    // redaction path. The live Anthropic/OpenAI round-trip paths are covered
    // directly in symbolon's own tests (crates/symbolon/src/credential/admin.rs,
    // provider_validation_tests).
    let (app, _dir) = app_with_provider_name("acme-test-provider").await;
    let raw_secret = "sk-test-validate-redaction-marker";

    let add = app
        .clone()
        .oneshot(authed_request(
            "POST",
            "/api/v1/system/credentials",
            Some(serde_json::json!({
                "provider": "acme-test-provider",
                "key": raw_secret,
                "role": "primary"
            })),
        ))
        .await
        .unwrap();
    assert_eq!(add.status(), StatusCode::CREATED);

    let resp = app
        .oneshot(authed_request(
            "POST",
            "/api/v1/system/credentials/acme-test-provider:primary/validate",
            None,
        ))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_string(resp).await;
    // WHY: an unrecognized provider has no live-check strategy, so `status`
    // falls back to local inspection (file loaded, not locally expired).
    assert!(body.contains(r#""status":"valid""#));
    assert!(body.contains(r#""validation_state":"unknown""#));
    assert!(body.contains(r#""provider_verified":false"#));
    assert!(body.contains("last_validated"));
    assert!(!body.contains(raw_secret));
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
async fn credentials_add_rejects_short_secret_without_storing() {
    let (app, dir) = app().await;
    let raw_secret = "abcd1234";

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

    assert_eq!(add.status(), StatusCode::BAD_REQUEST);
    let body = body_json(add).await;
    let message = body
        .get("error")
        .and_then(|e| e.get("message"))
        .and_then(|m| m.as_str())
        .expect("error message");
    assert!(message.contains("at least"));
    assert!(!message.contains(raw_secret));
    assert!(
        !dir.path()
            .join("config/credentials/anthropic.backup.json")
            .exists(),
        "invalid credential must not be persisted"
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

// ── audit events (#4878) ──
//
// WHY: add/validate/rotate/remove previously left no audit trail at all --
// no actor, no outcome, nothing to subscribe to. These tests drive each
// endpoint against a subscribed event_bus receiver and assert the emitted
// payload's shape directly, on both success and failure paths, and that no
// raw credential value ever reaches the payload.

async fn next_event(
    events: &mut tokio::sync::broadcast::Receiver<crate::event_bus::DomainEvent>,
) -> crate::event_bus::DomainEvent {
    tokio::time::timeout(std::time::Duration::from_secs(2), events.recv())
        .await
        .expect("audit event was not published within the timeout")
        .expect("event_bus receiver closed unexpectedly")
}

#[tokio::test]
async fn add_credential_publishes_success_audit_event() {
    let (state, _dir) = test_state().await;
    let mut events = state.event_bus.subscribe();
    let app = build_router(std::sync::Arc::clone(&state), &test_security_config());
    let raw_secret = "sk-test-audit-add-success-marker";

    let resp = app
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
    assert_eq!(resp.status(), StatusCode::CREATED);

    let event = next_event(&mut events).await;
    assert_eq!(event.topic, "credential.mutation");
    assert_eq!(event.payload["action"], "add");
    assert_eq!(event.payload["result"], "ok");
    assert_eq!(event.payload["provider"], "anthropic");
    assert_eq!(event.payload["credential_role"], "backup");
    assert_eq!(event.payload["actor"], "test-user");
    assert_eq!(event.payload["actor_role"], "operator");
    assert_eq!(event.payload["error_code"], serde_json::Value::Null);
    assert!(
        event.payload["request_id"]
            .as_str()
            .is_some_and(|s| !s.is_empty())
    );
    assert_eq!(event.payload["runtime_effect"], "restart_required");

    // SECURITY: the audit payload must never carry the raw credential value.
    let serialized = event.payload.to_string();
    assert!(!serialized.contains(raw_secret));
}

#[tokio::test]
async fn add_credential_publishes_failure_audit_event_on_duplicate() {
    let (state, _dir) = test_state().await;
    let app = build_router(std::sync::Arc::clone(&state), &test_security_config());

    let first = app
        .clone()
        .oneshot(authed_request(
            "POST",
            "/api/v1/system/credentials",
            Some(serde_json::json!({
                "provider": "anthropic",
                "key": "sk-test-audit-dup-first",
                "role": "backup"
            })),
        ))
        .await
        .unwrap();
    assert_eq!(first.status(), StatusCode::CREATED);

    let mut events = state.event_bus.subscribe();
    let raw_secret = "sk-test-audit-dup-second-marker";
    let second = app
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
    assert_eq!(second.status(), StatusCode::CONFLICT);

    let event = next_event(&mut events).await;
    assert_eq!(event.topic, "credential.mutation");
    assert_eq!(event.payload["action"], "add");
    assert_eq!(
        event.payload["result"], "error",
        "a failed add must still be audited, not silently dropped"
    );
    assert_eq!(event.payload["error_code"], "conflict");
    assert_eq!(event.payload["provider"], "anthropic");
    assert_eq!(event.payload["runtime_effect"], serde_json::Value::Null);

    let serialized = event.payload.to_string();
    assert!(!serialized.contains(raw_secret));
}

#[tokio::test]
async fn validate_credential_publishes_audit_event_with_validation_state() {
    let (state, _dir) = state_with_provider_name("acme-audit-provider").await;
    let app = build_router(std::sync::Arc::clone(&state), &test_security_config());
    let raw_secret = "sk-test-audit-validate-marker";

    let add = app
        .clone()
        .oneshot(authed_request(
            "POST",
            "/api/v1/system/credentials",
            Some(serde_json::json!({
                "provider": "acme-audit-provider",
                "key": raw_secret,
                "role": "primary"
            })),
        ))
        .await
        .unwrap();
    assert_eq!(add.status(), StatusCode::CREATED);

    let mut events = state.event_bus.subscribe();
    let resp = app
        .oneshot(authed_request(
            "POST",
            "/api/v1/system/credentials/acme-audit-provider:primary/validate",
            None,
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_string(resp).await;
    assert!(body.contains(r#""validation_state":"unknown""#));
    assert!(!body.contains(raw_secret));

    let event = next_event(&mut events).await;
    assert_eq!(event.topic, "credential.validation");
    assert_eq!(event.payload["action"], "validate");
    assert_eq!(event.payload["result"], "ok");
    assert_eq!(event.payload["provider"], "acme-audit-provider");
    assert_eq!(event.payload["credential_role"], "primary");
    assert_eq!(event.payload["validation_state"], "unknown");
    assert_eq!(event.payload["runtime_effect"], serde_json::Value::Null);

    let serialized = event.payload.to_string();
    assert!(!serialized.contains(raw_secret));
}

#[tokio::test]
async fn rotate_credentials_publishes_audit_event_with_null_credential_role() {
    let (state, _dir) = test_state().await;
    let app = build_router(std::sync::Arc::clone(&state), &test_security_config());

    app.clone()
        .oneshot(authed_request(
            "POST",
            "/api/v1/system/credentials",
            Some(serde_json::json!({
                "provider": "anthropic",
                "key": "sk-test-audit-rotate-backup",
                "role": "backup"
            })),
        ))
        .await
        .unwrap();

    let mut events = state.event_bus.subscribe();
    let resp = app
        .oneshot(authed_request(
            "POST",
            "/api/v1/system/credentials/rotate?provider=anthropic",
            None,
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let event = next_event(&mut events).await;
    assert_eq!(event.topic, "credential.mutation");
    assert_eq!(event.payload["action"], "rotate");
    assert_eq!(event.payload["result"], "ok");
    assert_eq!(
        event.payload["credential_role"],
        serde_json::Value::Null,
        "rotate swaps both roles -- no single role is the subject"
    );
}

#[tokio::test]
async fn remove_credential_publishes_audit_event() {
    let (state, _dir) = test_state().await;
    let app = build_router(std::sync::Arc::clone(&state), &test_security_config());

    app.clone()
        .oneshot(authed_request(
            "POST",
            "/api/v1/system/credentials",
            Some(serde_json::json!({
                "provider": "anthropic",
                "key": "sk-test-audit-remove-backup",
                "role": "backup"
            })),
        ))
        .await
        .unwrap();

    let mut events = state.event_bus.subscribe();
    let resp = app
        .oneshot(authed_request(
            "DELETE",
            "/api/v1/system/credentials/anthropic:backup",
            None,
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let event = next_event(&mut events).await;
    assert_eq!(event.topic, "credential.mutation");
    assert_eq!(event.payload["action"], "remove");
    assert_eq!(event.payload["result"], "ok");
    assert_eq!(event.payload["credential_role"], "backup");
    assert_eq!(event.payload["provider"], "anthropic");
}
