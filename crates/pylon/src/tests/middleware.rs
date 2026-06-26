#![expect(
    clippy::indexing_slicing,
    reason = "test: vec/JSON indices valid after asserting len or known structure"
)]
use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::routing::post;
use tower::ServiceExt;

use super::helpers::*;

#[tokio::test]
async fn security_headers_present_on_response() {
    let (app, _dir) = app().await;
    let resp = app
        .oneshot(Request::get("/api/health").body(Body::empty()).unwrap())
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(resp.headers().get("x-frame-options").unwrap(), "DENY");
    assert_eq!(
        resp.headers().get("x-content-type-options").unwrap(),
        "nosniff"
    );
    assert_eq!(resp.headers().get("x-xss-protection").unwrap(), "0");
    assert_eq!(
        resp.headers().get("referrer-policy").unwrap(),
        "strict-origin-when-cross-origin"
    );
    assert_eq!(
        resp.headers().get("content-security-policy").unwrap(),
        "default-src 'self'"
    );
    assert!(resp.headers().get("strict-transport-security").is_none());
}

#[tokio::test]
async fn hsts_header_present_when_tls_enabled() {
    let (state, _dir) = test_state().await;
    let security = SecurityConfig {
        tls: crate::security::TlsConfig {
            enabled: true,
            ..crate::security::TlsConfig::default()
        },
        ..SecurityConfig::default()
    };
    let router = build_router(state, &security);

    let resp = router
        .oneshot(Request::get("/api/health").body(Body::empty()).unwrap())
        .await
        .unwrap();

    assert_eq!(
        resp.headers().get("strict-transport-security").unwrap(),
        "max-age=31536000; includeSubDomains"
    );
}

#[tokio::test]
async fn oversized_body_returns_413() {
    let (state, _dir) = test_state().await;
    let security = SecurityConfig {
        body_limit_bytes: 100,
        csrf: crate::security::CsrfConfig {
            enabled: false,
            disable_acknowledged: true,
            ..crate::security::CsrfConfig::default()
        },
        ..SecurityConfig::default()
    };
    let router = build_router(state, &security);

    let big_body = "x".repeat(200);
    let token = default_token();
    let req = Request::builder()
        .method("POST")
        .uri("/api/v1/sessions")
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {token}"))
        .body(Body::from(big_body))
        .unwrap();

    let resp = router.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::PAYLOAD_TOO_LARGE);
}

#[tokio::test]
async fn csrf_rejects_post_without_header() {
    let (state, _dir) = test_state().await;
    let security = SecurityConfig {
        csrf: crate::security::CsrfConfig {
            enabled: true,
            ..crate::security::CsrfConfig::default()
        },
        ..SecurityConfig::default()
    };
    let router = build_router(state, &security);

    let req = authed_request(
        "POST",
        "/api/v1/sessions",
        Some(serde_json::json!({
            "nous_id": "syn",
            "session_key": "csrf-test"
        })),
    );

    let resp = router.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn csrf_allows_post_with_correct_header() {
    let (state, _dir) = test_state().await;
    let security = SecurityConfig {
        csrf: crate::security::CsrfConfig {
            enabled: true,
            ..crate::security::CsrfConfig::default()
        },
        ..SecurityConfig::default()
    };
    let csrf_token = security.csrf.header_value.expose_secret().to_owned();
    let router = build_router(state, &security);

    let token = default_token();
    let req = Request::builder()
        .method("POST")
        .uri("/api/v1/sessions")
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {token}"))
        .header("x-requested-with", csrf_token)
        .body(Body::from(
            serde_json::to_vec(&serde_json::json!({
                "nous_id": "syn",
                "session_key": "csrf-test"
            }))
            .unwrap(),
        ))
        .unwrap();

    let resp = router.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
}

#[tokio::test]
async fn csrf_allows_get_without_header() {
    let (state, _dir) = test_state().await;
    let security = SecurityConfig {
        csrf: crate::security::CsrfConfig {
            enabled: true,
            ..crate::security::CsrfConfig::default()
        },
        ..SecurityConfig::default()
    };
    let router = build_router(state, &security);

    let resp = router.oneshot(authed_get("/api/v1/nous")).await.unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn csrf_rejects_wrong_header_value() {
    let (state, _dir) = test_state().await;
    let security = SecurityConfig {
        csrf: crate::security::CsrfConfig {
            enabled: true,
            ..crate::security::CsrfConfig::default()
        },
        ..SecurityConfig::default()
    };
    let router = build_router(state, &security);

    let token = default_token();
    let req = Request::builder()
        .method("POST")
        .uri("/api/v1/sessions")
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {token}"))
        .header("x-requested-with", "wrong-value")
        .body(Body::from(
            serde_json::to_vec(&serde_json::json!({
                "nous_id": "syn",
                "session_key": "csrf-wrong"
            }))
            .unwrap(),
        ))
        .unwrap();

    let resp = router.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn csrf_allows_delete_with_correct_header() {
    let (state, _dir) = test_state().await;
    let security = SecurityConfig {
        csrf: crate::security::CsrfConfig {
            enabled: true,
            ..crate::security::CsrfConfig::default()
        },
        ..SecurityConfig::default()
    };
    let csrf_token = security.csrf.header_value.expose_secret().to_owned();
    let router = build_router(Arc::clone(&state), &security);

    let token = default_token();

    let create_req = Request::builder()
        .method("POST")
        .uri("/api/v1/sessions")
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {token}"))
        .header("x-requested-with", csrf_token.clone())
        .body(Body::from(
            serde_json::to_vec(&serde_json::json!({
                "nous_id": "syn",
                "session_key": "csrf-delete"
            }))
            .unwrap(),
        ))
        .unwrap();

    let resp = router.clone().oneshot(create_req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
    let session = body_json(resp).await;
    let id = session["id"].as_str().unwrap();

    let delete_req = Request::builder()
        .method("DELETE")
        .uri(format!("/api/v1/sessions/{id}"))
        .header("authorization", format!("Bearer {token}"))
        .header("x-requested-with", csrf_token)
        .body(Body::empty())
        .unwrap();

    let resp = router.clone().oneshot(delete_req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn cors_permissive_when_no_origins_configured() {
    let (state, _dir) = test_state().await;
    let security = test_security_config(); // empty origins = permissive
    let router = build_router(state, &security);

    let req = Request::builder()
        .method("OPTIONS")
        .uri("/api/health")
        .header("origin", "http://evil.example.com")
        .header("access-control-request-method", "GET")
        .body(Body::empty())
        .unwrap();

    let resp = router.oneshot(req).await.unwrap();
    assert!(resp.status().is_success() || resp.status() == StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn csrf_middleware_without_state_rejects_post() {
    let router = axum::Router::new()
        .route("/write", post(|| async { StatusCode::OK }))
        .layer(axum::middleware::from_fn(
            crate::middleware::require_csrf_header,
        ));

    let resp = router
        .oneshot(Request::post("/write").body(Body::empty()).unwrap())
        .await
        .unwrap();

    assert_eq!(
        resp.status(),
        StatusCode::FORBIDDEN,
        "missing CsrfState must not bypass CSRF protection"
    );
}

#[tokio::test]
async fn cors_rejects_unlisted_origin() {
    let (state, _dir) = test_state().await;
    let security = SecurityConfig {
        cors: crate::security::CorsConfig {
            allowed_origins: vec!["http://localhost:3000".to_owned()],
            ..crate::security::CorsConfig::default()
        },
        ..SecurityConfig::default()
    };
    let router = build_router(state, &security);

    let req = Request::builder()
        .method("OPTIONS")
        .uri("/api/health")
        .header("origin", "http://evil.example.com")
        .header("access-control-request-method", "GET")
        .body(Body::empty())
        .unwrap();

    let resp = router.oneshot(req).await.unwrap();
    let allow_origin = resp.headers().get("access-control-allow-origin");
    assert!(allow_origin.is_none() || allow_origin.unwrap() != "http://evil.example.com");
}

#[tokio::test]
async fn cors_permissive_allows_browser_api_headers() {
    let (state, _dir) = test_state().await;
    let security = test_security_config();
    let router = build_router(state, &security);

    let req = Request::builder()
        .method("OPTIONS")
        .uri("/api/health")
        .header("origin", "http://localhost:3000")
        .header("access-control-request-method", "POST")
        .header(
            "access-control-request-headers",
            "idempotency-key, last-event-id",
        )
        .body(Body::empty())
        .unwrap();

    let resp = router.oneshot(req).await.unwrap();
    assert!(resp.status().is_success() || resp.status() == StatusCode::NO_CONTENT);
    let allow_headers = resp
        .headers()
        .get("access-control-allow-headers")
        .expect("allow-headers must be present");
    let allowed = allow_headers.to_str().unwrap();
    assert!(
        allowed.contains("idempotency-key"),
        "idempotency-key must be allowed"
    );
    assert!(
        allowed.contains("last-event-id"),
        "last-event-id must be allowed"
    );
}

#[tokio::test]
async fn cors_explicit_origin_allows_browser_api_headers() {
    let (state, _dir) = test_state().await;
    let security = SecurityConfig {
        cors: crate::security::CorsConfig {
            allowed_origins: vec!["http://localhost:3000".to_owned()],
            ..crate::security::CorsConfig::default()
        },
        csrf: crate::security::CsrfConfig {
            enabled: false,
            ..crate::security::CsrfConfig::default()
        },
        ..SecurityConfig::default()
    };
    let router = build_router(state, &security);

    let req = Request::builder()
        .method("OPTIONS")
        .uri("/api/health")
        .header("origin", "http://localhost:3000")
        .header("access-control-request-method", "POST")
        .header(
            "access-control-request-headers",
            "idempotency-key, last-event-id",
        )
        .body(Body::empty())
        .unwrap();

    let resp = router.oneshot(req).await.unwrap();
    assert!(resp.status().is_success() || resp.status() == StatusCode::NO_CONTENT);
    let allow_headers = resp
        .headers()
        .get("access-control-allow-headers")
        .expect("allow-headers must be present");
    let allowed = allow_headers.to_str().unwrap();
    assert!(
        allowed.contains("idempotency-key"),
        "idempotency-key must be allowed"
    );
    assert!(
        allowed.contains("last-event-id"),
        "last-event-id must be allowed"
    );
}

#[tokio::test]
async fn cors_permissive_allows_idempotency_key_preflight_on_messages() {
    let (state, _dir) = test_state().await;
    let security = test_security_config();
    let router = build_router(state, &security);

    let req = Request::builder()
        .method("OPTIONS")
        .uri("/api/v1/sessions/sess-01/messages")
        .header("origin", "http://localhost:3000")
        .header("access-control-request-method", "POST")
        .header("access-control-request-headers", "idempotency-key")
        .body(Body::empty())
        .unwrap();

    let resp = router.oneshot(req).await.unwrap();
    assert!(resp.status().is_success() || resp.status() == StatusCode::NO_CONTENT);
    let allow_methods = resp
        .headers()
        .get("access-control-allow-methods")
        .expect("allow-methods must be present");
    assert!(
        allow_methods.to_str().unwrap().contains("POST"),
        "POST must be allowed for message send preflight"
    );
    let allow_headers = resp
        .headers()
        .get("access-control-allow-headers")
        .expect("allow-headers must be present");
    let allowed = allow_headers.to_str().unwrap();
    assert!(
        allowed.contains("idempotency-key"),
        "idempotency-key must be allowed on message send route"
    );
}

#[tokio::test]
async fn cors_explicit_origin_allows_idempotency_key_preflight_on_messages() {
    let (state, _dir) = test_state().await;
    let security = SecurityConfig {
        cors: crate::security::CorsConfig {
            allowed_origins: vec!["http://localhost:3000".to_owned()],
            ..crate::security::CorsConfig::default()
        },
        csrf: crate::security::CsrfConfig {
            enabled: false,
            ..crate::security::CsrfConfig::default()
        },
        ..SecurityConfig::default()
    };
    let router = build_router(state, &security);

    let req = Request::builder()
        .method("OPTIONS")
        .uri("/api/v1/sessions/sess-01/messages")
        .header("origin", "http://localhost:3000")
        .header("access-control-request-method", "POST")
        .header("access-control-request-headers", "idempotency-key")
        .body(Body::empty())
        .unwrap();

    let resp = router.oneshot(req).await.unwrap();
    assert!(resp.status().is_success() || resp.status() == StatusCode::NO_CONTENT);
    assert_eq!(
        resp.headers()
            .get("access-control-allow-origin")
            .map(|v| v.to_str().unwrap()),
        Some("http://localhost:3000")
    );
    let allow_methods = resp
        .headers()
        .get("access-control-allow-methods")
        .expect("allow-methods must be present");
    assert!(
        allow_methods.to_str().unwrap().contains("POST"),
        "POST must be allowed for message send preflight"
    );
    let allow_headers = resp
        .headers()
        .get("access-control-allow-headers")
        .expect("allow-headers must be present");
    let allowed = allow_headers.to_str().unwrap();
    assert!(
        allowed.contains("idempotency-key"),
        "idempotency-key must be allowed on message send route"
    );
}

#[tokio::test]
async fn cors_permissive_allows_last_event_id_preflight_on_turn_events() {
    let (state, _dir) = test_state().await;
    let security = test_security_config();
    let router = build_router(state, &security);

    let req = Request::builder()
        .method("OPTIONS")
        .uri("/api/v1/sessions/sess-01/turns/turn-01/events")
        .header("origin", "http://localhost:3000")
        .header("access-control-request-method", "GET")
        .header("access-control-request-headers", "last-event-id")
        .body(Body::empty())
        .unwrap();

    let resp = router.oneshot(req).await.unwrap();
    assert!(resp.status().is_success() || resp.status() == StatusCode::NO_CONTENT);
    let allow_methods = resp
        .headers()
        .get("access-control-allow-methods")
        .expect("allow-methods must be present");
    assert!(
        allow_methods.to_str().unwrap().contains("GET"),
        "GET must be allowed for turn reconnect preflight"
    );
    let allow_headers = resp
        .headers()
        .get("access-control-allow-headers")
        .expect("allow-headers must be present");
    let allowed = allow_headers.to_str().unwrap();
    assert!(
        allowed.contains("last-event-id"),
        "last-event-id must be allowed on turn reconnect route"
    );
}

#[tokio::test]
async fn cors_explicit_origin_allows_last_event_id_preflight_on_turn_events() {
    let (state, _dir) = test_state().await;
    let security = SecurityConfig {
        cors: crate::security::CorsConfig {
            allowed_origins: vec!["http://localhost:3000".to_owned()],
            ..crate::security::CorsConfig::default()
        },
        csrf: crate::security::CsrfConfig {
            enabled: false,
            ..crate::security::CsrfConfig::default()
        },
        ..SecurityConfig::default()
    };
    let router = build_router(state, &security);

    let req = Request::builder()
        .method("OPTIONS")
        .uri("/api/v1/sessions/sess-01/turns/turn-01/events")
        .header("origin", "http://localhost:3000")
        .header("access-control-request-method", "GET")
        .header("access-control-request-headers", "last-event-id")
        .body(Body::empty())
        .unwrap();

    let resp = router.oneshot(req).await.unwrap();
    assert!(resp.status().is_success() || resp.status() == StatusCode::NO_CONTENT);
    assert_eq!(
        resp.headers()
            .get("access-control-allow-origin")
            .map(|v| v.to_str().unwrap()),
        Some("http://localhost:3000")
    );
    let allow_methods = resp
        .headers()
        .get("access-control-allow-methods")
        .expect("allow-methods must be present");
    assert!(
        allow_methods.to_str().unwrap().contains("GET"),
        "GET must be allowed for turn reconnect preflight"
    );
    let allow_headers = resp
        .headers()
        .get("access-control-allow-headers")
        .expect("allow-headers must be present");
    let allowed = allow_headers.to_str().unwrap();
    assert!(
        allowed.contains("last-event-id"),
        "last-event-id must be allowed on turn reconnect route"
    );
}

#[test]
fn security_config_default_values() {
    let config = SecurityConfig::default();
    assert!(config.cors.allowed_origins.is_empty());
    assert_eq!(config.cors.max_age_secs, 3600);
    assert_eq!(config.body_limit_bytes, 1_048_576);
    assert!(config.csrf.enabled);
    assert!(!config.csrf.disable_acknowledged);
    assert_eq!(config.csrf.header_name, "x-requested-with");
    assert_eq!(
        config.csrf.header_value.expose_secret(),
        "aletheia",
        "default CSRF header value must match the documented bootstrap header"
    );
    assert!(!config.tls.enabled);
    assert!(config.tls.cert_path.is_none());
    assert!(config.tls.key_path.is_none());
}

#[test]
fn security_config_from_gateway() {
    use taxis::config::GatewayConfig;

    let gw = GatewayConfig::default();
    let config = SecurityConfig::from_gateway(&gw);
    assert!(!config.tls.enabled);
    assert!(config.csrf.enabled);
    assert!(!config.csrf.disable_acknowledged);
    assert_eq!(config.csrf.header_value.expose_secret(), "aletheia");
    assert_eq!(config.cors.max_age_secs, 3600);
}

#[tokio::test]
async fn request_id_present_in_error_responses() {
    let (app, _dir) = app().await;
    let resp = app
        .oneshot(authed_get("/api/v1/sessions/nonexistent"))
        .await
        .unwrap();

    let body = body_json(resp).await;
    let request_id = body["error"]["request_id"].as_str().unwrap();
    assert!(!request_id.is_empty());
    assert!(request_id.len() >= 20, "request_id should be a ULID");
}

#[tokio::test]
async fn error_response_has_consistent_structure() {
    let (app, _dir) = app().await;
    let resp = app
        .oneshot(authed_get("/api/v1/sessions/nonexistent"))
        .await
        .unwrap();

    let body = body_json(resp).await;
    assert!(body["error"].is_object());
    assert!(body["error"]["code"].is_string());
    assert!(body["error"]["message"].is_string());
    assert!(
        body["error"]["request_id"].is_string(),
        "error response must include request_id"
    );
}

#[tokio::test]
async fn malformed_create_body_returns_400() {
    let (app, _dir) = app().await;
    let token = default_token();
    let req = Request::builder()
        .method("POST")
        .uri("/api/v1/sessions")
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {token}"))
        .body(Body::from(r#"{"invalid": true}"#))
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert!(
        resp.status() == StatusCode::BAD_REQUEST
            || resp.status() == StatusCode::UNPROCESSABLE_ENTITY
    );
}

#[tokio::test]
async fn malformed_send_body_returns_error() {
    let (router, _dir) = app().await;
    let created = create_test_session(&router).await;
    let id = created["id"].as_str().unwrap();

    let token = default_token();
    let req = Request::builder()
        .method("POST")
        .uri(format!("/api/v1/sessions/{id}/messages"))
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {token}"))
        .body(Body::from(r#"{"wrong_field": "abc"}"#))
        .unwrap();

    let resp = router.clone().oneshot(req).await.unwrap();
    assert!(
        resp.status() == StatusCode::BAD_REQUEST
            || resp.status() == StatusCode::UNPROCESSABLE_ENTITY
    );
}

#[tokio::test]
async fn unknown_route_returns_404() {
    let (app, _dir) = app().await;
    let resp = app
        .oneshot(authed_get("/api/nonexistent"))
        .await
        .expect("response");

    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    let body = body_json(resp).await;
    assert_eq!(body["error"]["code"], "not_found");
    assert!(body["error"]["request_id"].is_string());
}

#[tokio::test]
async fn old_api_nous_path_returns_gone() {
    let (app, _dir) = app().await;
    let resp = app
        .oneshot(authed_get("/api/nous"))
        .await
        .expect("response");

    assert_eq!(resp.status(), StatusCode::GONE);
    let body = body_json(resp).await;
    assert_eq!(body["error"]["code"], "api_version_required");
    assert!(
        body["error"]["message"]
            .as_str()
            .unwrap()
            .contains("/api/v1/nous")
    );
}

#[tokio::test]
async fn fallback_404_returns_json_error() {
    let (app, _dir) = app().await;
    let resp = app
        .oneshot(
            Request::get("/totally/unknown/path")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("response");

    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    let body = body_json(resp).await;
    assert_eq!(body["error"]["code"], "not_found");
    assert!(
        body["error"]["message"]
            .as_str()
            .unwrap()
            .contains("/totally/unknown/path")
    );
    assert!(body["error"]["request_id"].is_string());
}

#[tokio::test]
async fn put_on_sessions_returns_405() {
    let (app, _dir) = app().await;
    let req = Request::builder()
        .method("PUT")
        .uri("/api/v1/sessions")
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {}", default_token()))
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::METHOD_NOT_ALLOWED);
}

#[tokio::test]
async fn delete_on_nous_returns_405() {
    let (app, _dir) = app().await;
    let req = Request::builder()
        .method("DELETE")
        .uri("/api/v1/nous")
        .header("authorization", format!("Bearer {}", default_token()))
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::METHOD_NOT_ALLOWED);
}

#[tokio::test]
async fn post_on_health_returns_405() {
    let (app, _dir) = app().await;
    let req = Request::builder()
        .method("POST")
        .uri("/api/health")
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::METHOD_NOT_ALLOWED);
}

#[tokio::test]
async fn deprecation_header_present_for_registered_route() {
    let (app, _dir) = app().await;
    let resp = app
        .oneshot(Request::get("/health").body(Body::empty()).unwrap())
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let deprecation = resp
        .headers()
        .get("deprecation")
        .expect("Deprecation header must be present");
    assert!(
        deprecation.to_str().unwrap().starts_with('@'),
        "Deprecation header must be a timestamp prefixed with @"
    );
    assert!(
        resp.headers().get("sunset").is_some(),
        "Sunset header must be present"
    );
}

#[tokio::test]
async fn non_deprecated_route_has_no_headers() {
    let (app, _dir) = app().await;
    let resp = app
        .oneshot(Request::get("/api/health").body(Body::empty()).unwrap())
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    assert!(
        resp.headers().get("deprecation").is_none(),
        "non-deprecated route must not have Deprecation header"
    );
    assert!(
        resp.headers().get("sunset").is_none(),
        "non-deprecated route must not have Sunset header"
    );
    assert!(
        resp.headers().get("link").is_none(),
        "non-deprecated route must not have Link header"
    );
}

#[tokio::test]
async fn link_header_included_when_set() {
    let (app, _dir) = app().await;
    let resp = app
        .oneshot(Request::get("/health").body(Body::empty()).unwrap())
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let link = resp
        .headers()
        .get("link")
        .expect("Link header must be present for deprecated route with link");
    assert!(
        link.to_str().unwrap().contains("rel=\"deprecation\""),
        "Link header must contain rel=\"deprecation\""
    );
}

#[tokio::test]
async fn sunset_header_format_rfc8594() {
    let (app, _dir) = app().await;
    let resp = app
        .oneshot(Request::get("/health").body(Body::empty()).unwrap())
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let sunset = resp
        .headers()
        .get("sunset")
        .expect("Sunset header must be present");
    let sunset_str = sunset.to_str().unwrap();

    // RFC 7231 HTTP-date ends with "GMT"
    let (datetime_part, tz) = sunset_str.rsplit_once(' ').expect("valid HTTP-date format");
    assert_eq!(tz, "GMT", "Sunset header must use GMT timezone");

    // Verify the datetime portion parses with the expected format
    let parsed = jiff::civil::DateTime::strptime("%a, %d %b %Y %H:%M:%S", datetime_part);
    assert!(
        parsed.is_ok(),
        "Sunset header must be a valid RFC 7231 HTTP-date"
    );
}

#[tokio::test]
async fn etag_set_on_200_get_response() {
    let (app, _dir) = app().await;
    let resp = app.oneshot(authed_get("/api/v1/nous")).await.unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let etag = resp
        .headers()
        .get("etag")
        .expect("ETag header must be present on GET 200");
    assert!(
        etag.to_str().unwrap().starts_with('"'),
        "ETag must be a strong quoted string"
    );
}

#[tokio::test]
async fn if_none_match_returns_304_on_match() {
    let (app, _dir) = app().await;

    // First request to capture the ETag.
    let first = app
        .clone()
        .oneshot(authed_get("/api/v1/nous"))
        .await
        .unwrap();
    assert_eq!(first.status(), StatusCode::OK);
    let etag = first
        .headers()
        .get("etag")
        .unwrap()
        .to_str()
        .unwrap()
        .to_owned();

    // Replay with If-None-Match.
    let req = Request::get("/api/v1/nous")
        .header("authorization", format!("Bearer {}", default_token()))
        .header("if-none-match", &etag)
        .body(Body::empty())
        .unwrap();
    let second = app.oneshot(req).await.unwrap();
    assert_eq!(second.status(), StatusCode::NOT_MODIFIED);
    assert_eq!(body_string(second).await, "", "304 must have empty body");
}

#[tokio::test]
async fn if_none_match_returns_200_on_mismatch() {
    let (app, _dir) = app().await;

    let req = Request::get("/api/v1/nous")
        .header("authorization", format!("Bearer {}", default_token()))
        .header("if-none-match", "\"stale-etag-value\"")
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let etag = resp.headers().get("etag").expect("ETag must be present");
    assert_ne!(etag.to_str().unwrap(), "\"stale-etag-value\"");
}

#[tokio::test]
async fn etag_stable_for_identical_body() {
    let (app, _dir) = app().await;

    let req1 = authed_get("/api/v1/nous");
    let resp1 = app.clone().oneshot(req1).await.unwrap();
    assert_eq!(resp1.status(), StatusCode::OK);
    let etag1 = resp1
        .headers()
        .get("etag")
        .unwrap()
        .to_str()
        .unwrap()
        .to_owned();

    let req2 = authed_get("/api/v1/nous");
    let resp2 = app.oneshot(req2).await.unwrap();
    assert_eq!(resp2.status(), StatusCode::OK);
    let etag2 = resp2
        .headers()
        .get("etag")
        .unwrap()
        .to_str()
        .unwrap()
        .to_owned();

    assert_eq!(etag1, etag2, "identical body must produce identical ETag");
}

#[tokio::test]
async fn sse_endpoint_unaffected() {
    let (app, _dir) = app().await;
    let resp = app.oneshot(authed_get("/api/v1/events")).await.unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    assert!(
        resp.headers().get("etag").is_none(),
        "SSE endpoint must not have ETag header"
    );
    let ct = resp
        .headers()
        .get("content-type")
        .expect("SSE must have content-type");
    assert!(ct.to_str().unwrap().starts_with("text/event-stream"));
}

/// Build a router with per-IP rate limiting enabled and the given trust-proxy flag.
async fn app_with_ip_rate_limit(trust_proxy: bool) -> (axum::Router, tempfile::TempDir) {
    let (state, dir) = test_state().await;
    let security = SecurityConfig {
        csrf: crate::security::CsrfConfig {
            enabled: false,
            disable_acknowledged: true,
            ..crate::security::CsrfConfig::default()
        },
        rate_limit: crate::security::RateLimitConfig {
            enabled: true,
            requests_per_minute: 1,
            trust_proxy,
            ..crate::security::RateLimitConfig::default()
        },
        ..SecurityConfig::default()
    };
    (build_router(state, &security), dir)
}

/// Build a plain GET /api/health request with an optional X-Forwarded-For header.
fn anon_health_request(x_forwarded_for: Option<&str>) -> Request<Body> {
    let mut builder = Request::get("/api/health");
    if let Some(xff) = x_forwarded_for {
        builder = builder.header("x-forwarded-for", xff);
    }
    builder.body(Body::empty()).unwrap()
}

#[tokio::test]
async fn rate_limit_ignores_spoofed_forwarded_header_when_trust_proxy_false() {
    let (app, _dir) = app_with_ip_rate_limit(false).await;

    // Without trust_proxy, two requests with different spoofed X-Forwarded-For
    // headers must still share the "peer" bucket and the second must be limited.
    let first = app.clone().oneshot(anon_health_request(Some("1.2.3.4"))).await.unwrap();
    assert_eq!(first.status(), StatusCode::OK);

    let second = app.oneshot(anon_health_request(Some("5.6.7.8"))).await.unwrap();
    assert_eq!(
        second.status(),
        StatusCode::TOO_MANY_REQUESTS,
        "spoofed X-Forwarded-For must not bypass per-IP rate limiting when trust_proxy=false"
    );
}

#[tokio::test]
async fn rate_limit_uses_forwarded_header_when_trust_proxy_true() {
    let (app, _dir) = app_with_ip_rate_limit(true).await;

    // With trust_proxy, each distinct X-Forwarded-For value gets its own bucket.
    let first = app.clone().oneshot(anon_health_request(Some("1.2.3.4"))).await.unwrap();
    assert_eq!(first.status(), StatusCode::OK);

    let second = app.clone().oneshot(anon_health_request(Some("5.6.7.8"))).await.unwrap();
    assert_eq!(second.status(), StatusCode::OK);

    // A repeat from the first IP should now hit its own bucket's limit.
    let third = app.oneshot(anon_health_request(Some("1.2.3.4"))).await.unwrap();
    assert_eq!(
        third.status(),
        StatusCode::TOO_MANY_REQUESTS,
        "repeated X-Forwarded-For IP must be rate limited when trust_proxy=true"
    );
}
