#![expect(clippy::expect_used, reason = "test assertions use expect")]
#![expect(
    clippy::indexing_slicing,
    reason = "test: JSON indices and byte-slice ranges are valid after asserting status or known protocol shape"
)]
use std::sync::Arc;

use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use tower::ServiceExt;

use koina::http::{API_HEALTH, API_V1};
use pylon::router::build_router;
use skene::api::routes::planning::{project_verification_path, project_verification_refresh_path};
use symbolon::types::Role;

mod common;
use common::{
    TestEnv, bearer, issue_test_token, issue_test_token_as, issue_test_token_scoped,
    permissive_security, read_body_json,
};

// ── build_router: construction contracts ───────────────────────────────────

#[tokio::test]
async fn build_router_produces_router_with_health_endpoint() {
    let env = TestEnv::new().await;
    let router = build_router(Arc::clone(&env.state), &permissive_security());

    let response = router
        .oneshot(
            Request::get(API_HEALTH)
                .body(Body::empty())
                .expect("build request"),
        )
        .await
        .expect("router response");

    assert_eq!(response.status(), StatusCode::OK);

    let body = read_body_json(response).await;
    assert!(
        body.as_object().expect("health response object").len() == 1,
        "public health must not expose diagnostic fields"
    );
    assert_eq!(body["status"], "healthy");
}

#[tokio::test]
async fn build_router_health_also_served_at_slash_health() {
    // WHY: The router exposes health at both `/api/health` and `/health`
    // for infrastructure compatibility (some load balancers default to
    // `/health`). Regression test: #2814 must not drop the bare path.
    let env = TestEnv::new().await;
    let router = build_router(Arc::clone(&env.state), &permissive_security());

    let response = router
        .oneshot(
            Request::get("/health")
                .body(Body::empty())
                .expect("build request"),
        )
        .await
        .expect("router response");

    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn build_router_is_idempotent_for_shared_state() {
    let env = TestEnv::new().await;
    let router_one = build_router(Arc::clone(&env.state), &permissive_security());
    let router_two = build_router(Arc::clone(&env.state), &permissive_security());

    // WHY: AppState is shared behind Arc and build_router must not consume or
    // mutate it. Regression test: if build_router were to install a one-shot
    // layer that panics on re-entry, routing through the second router would
    // fail. Both should work.
    for router in [router_one, router_two] {
        let response = router
            .oneshot(
                Request::get(API_HEALTH)
                    .body(Body::empty())
                    .expect("build request"),
            )
            .await
            .expect("router response");
        assert_eq!(response.status(), StatusCode::OK);
    }
}

#[tokio::test]
async fn build_router_unknown_path_returns_404_json_envelope() {
    let env = TestEnv::new().await;
    let router = build_router(Arc::clone(&env.state), &permissive_security());

    let response = router
        .oneshot(
            Request::get("/definitely/not/a/real/path")
                .body(Body::empty())
                .expect("build request"),
        )
        .await
        .expect("router response");

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let body = read_body_json(response).await;
    assert_eq!(body["error"]["code"], "not_found");
    assert!(
        body["error"]["request_id"].is_string(),
        "404 must carry a request_id for correlation"
    );
}

#[tokio::test]
async fn build_router_old_api_nous_path_returns_410_gone() {
    // WHY: The unversioned `/api/nous` path was moved to `/api/v1/nous`.
    // The fallback returns 410 Gone with a migration hint instead of 404
    // so older clients see an actionable error. Regression test: this
    // migration hint is a public contract.
    let env = TestEnv::new().await;
    let router = build_router(Arc::clone(&env.state), &permissive_security());

    let response = router
        .oneshot(
            Request::get("/api/nous")
                .body(Body::empty())
                .expect("build request"),
        )
        .await
        .expect("router response");

    assert_eq!(response.status(), StatusCode::GONE);
    let body = read_body_json(response).await;
    assert_eq!(body["error"]["code"], "api_version_required");
    let message = body["error"]["message"]
        .as_str()
        .expect("message is a string");
    assert!(
        message.contains("/api/v1/nous"),
        "migration hint must name the new path, got {message}",
    );
}

// ── build_router: auth contracts ───────────────────────────────────────────

#[tokio::test]
async fn protected_endpoint_rejects_missing_bearer() {
    let env = TestEnv::new().await;
    let router = build_router(Arc::clone(&env.state), &permissive_security());

    let response = router
        .oneshot(
            Request::get(format!("{API_V1}/nous"))
                .body(Body::empty())
                .expect("build request"),
        )
        .await
        .expect("router response");

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn protected_endpoint_accepts_valid_bearer() {
    let env = TestEnv::new().await;
    let token = issue_test_token(&env.state);
    let router = build_router(Arc::clone(&env.state), &permissive_security());

    let response = router
        .oneshot(
            Request::get(format!("{API_V1}/nous"))
                .header("authorization", bearer(&token))
                .body(Body::empty())
                .expect("build request"),
        )
        .await
        .expect("router response");

    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn detailed_health_rejects_missing_bearer() {
    let env = TestEnv::new().await;
    let router = build_router(Arc::clone(&env.state), &permissive_security());

    let response = router
        .oneshot(
            Request::get(format!("{API_V1}/system/health"))
                .body(Body::empty())
                .expect("build request"),
        )
        .await
        .expect("router response");

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn detailed_health_rejects_readonly_bearer() {
    let env = TestEnv::new().await;
    let token = issue_test_token_as(&env.state, Role::Readonly);
    let router = build_router(Arc::clone(&env.state), &permissive_security());

    let response = router
        .oneshot(
            Request::get(format!("{API_V1}/system/health"))
                .header("authorization", bearer(&token))
                .body(Body::empty())
                .expect("build request"),
        )
        .await
        .expect("router response");

    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn detailed_health_accepts_operator_bearer() {
    let env = TestEnv::new().await;
    let token = issue_test_token(&env.state);
    let router = build_router(Arc::clone(&env.state), &permissive_security());

    let response = router
        .oneshot(
            Request::get(format!("{API_V1}/system/health"))
                .header("authorization", bearer(&token))
                .body(Body::empty())
                .expect("build request"),
        )
        .await
        .expect("router response");

    assert!(matches!(
        response.status(),
        StatusCode::OK | StatusCode::SERVICE_UNAVAILABLE
    ));
    let body = read_body_json(response).await;
    assert!(body["checks"].is_array(), "operator health exposes checks");
    assert!(
        body["data_dir"].is_string(),
        "operator health exposes data_dir"
    );
}

#[tokio::test]
async fn proskenion_planning_fetch_urls_match_pylon_routes() {
    let env = TestEnv::new().await;
    let token = issue_test_token(&env.state);
    let router = build_router(Arc::clone(&env.state), &permissive_security());

    for (method, path) in [
        (Method::GET, project_verification_path("some-project")),
        (
            Method::POST,
            project_verification_refresh_path("some-project"),
        ),
    ] {
        let response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method(method)
                    .uri(path)
                    .header("authorization", bearer(&token))
                    .body(Body::empty())
                    .expect("build request"),
            )
            .await
            .expect("router response");

        let body = read_body_json(response).await;
        assert_eq!(body["error"]["code"], "not_found");
        assert!(
            body["error"]["message"]
                .as_str()
                .expect("error message")
                .contains("planning/projects/some-project"),
            "proskenion planning URL must reach the planning handler"
        );
    }
}

#[tokio::test]
async fn legacy_planning_fetch_url_is_not_served() {
    let env = TestEnv::new().await;
    let token = issue_test_token(&env.state);
    let router = build_router(Arc::clone(&env.state), &permissive_security());

    let response = router
        .oneshot(
            Request::get("/api/planning/projects/some-project/verification")
                .header("authorization", bearer(&token))
                .body(Body::empty())
                .expect("build request"),
        )
        .await
        .expect("router response");

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn protected_endpoint_rejects_malformed_bearer() {
    let env = TestEnv::new().await;
    let router = build_router(Arc::clone(&env.state), &permissive_security());

    let response = router
        .oneshot(
            Request::get(format!("{API_V1}/nous"))
                .header("authorization", "Bearer not.a.valid.jwt")
                .body(Body::empty())
                .expect("build request"),
        )
        .await
        .expect("router response");

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn protected_endpoint_rejects_bearer_without_prefix() {
    let env = TestEnv::new().await;
    let token = issue_test_token(&env.state);
    let router = build_router(Arc::clone(&env.state), &permissive_security());

    let response = router
        .oneshot(
            Request::get(format!("{API_V1}/nous"))
                .header("authorization", token)
                .body(Body::empty())
                .expect("build request"),
        )
        .await
        .expect("router response");

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn auth_mode_none_allows_access_without_bearer() {
    let env = TestEnv::builder().auth_mode("none").build().await;
    let router = build_router(Arc::clone(&env.state), &permissive_security());

    let response = router
        .oneshot(
            Request::get(format!("{API_V1}/nous"))
                .body(Body::empty())
                .expect("build request"),
        )
        .await
        .expect("router response");

    assert_eq!(
        response.status(),
        StatusCode::OK,
        "auth_mode=none must not require a bearer on protected routes"
    );
}

#[tokio::test]
async fn knowledge_write_routes_reject_missing_bearer_token() {
    let env = TestEnv::new().await;
    let router = build_router(Arc::clone(&env.state), &permissive_security());

    for route in knowledge_write_routes() {
        let response = router
            .clone()
            .oneshot(route.request(None))
            .await
            .expect("router response");

        assert_eq!(
            response.status(),
            StatusCode::UNAUTHORIZED,
            "{} {}",
            route.method,
            route.path
        );
    }
}

#[tokio::test]
async fn knowledge_write_routes_reject_invalid_bearer_token() {
    let env = TestEnv::new().await;
    let router = build_router(Arc::clone(&env.state), &permissive_security());

    for route in knowledge_write_routes() {
        let response = router
            .clone()
            .oneshot(route.request(Some("Bearer not.a.valid.jwt".to_owned())))
            .await
            .expect("router response");

        assert_eq!(
            response.status(),
            StatusCode::UNAUTHORIZED,
            "{} {}",
            route.method,
            route.path
        );
    }
}

#[tokio::test]
async fn knowledge_write_routes_reject_valid_bearer_with_readonly_role() {
    let env = TestEnv::new().await;
    let token = issue_test_token_as(&env.state, Role::Readonly);
    let router = build_router(Arc::clone(&env.state), &permissive_security());

    for route in knowledge_write_routes() {
        let response = router
            .clone()
            .oneshot(route.request(Some(bearer(&token))))
            .await
            .expect("router response");

        assert_eq!(
            response.status(),
            StatusCode::FORBIDDEN,
            "{} {}",
            route.method,
            route.path
        );
    }
}

// ── scoped tokens must not reach agents outside their scope ────────────────

/// Per-agent routes that take an `{id}` path parameter and therefore must
/// reject a token whose `nous_id` scope does not match the requested agent.
fn nous_per_agent_routes() -> [(Method, &'static str); 3] {
    [
        (Method::GET, "/api/v1/nous/syn"),
        (Method::GET, "/api/v1/nous/syn/tools"),
        (Method::POST, "/api/v1/nous/syn/recover"),
    ]
}

#[tokio::test]
async fn nous_routes_reject_token_scoped_to_a_different_agent() {
    // WHY: a JWT scoped to another nous must not be able to read status,
    // enumerate tools, or trigger recovery on `syn`. Without
    // `require_nous_access` on these handlers, an Operator token scoped to
    // one agent could affect every other agent in the system.
    let env = TestEnv::builder().with_actor(true).build().await;
    let token = issue_test_token_scoped(&env.state, Role::Operator, "other-agent");
    let router = build_router(Arc::clone(&env.state), &permissive_security());

    for (method, path) in nous_per_agent_routes() {
        let response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method(method.clone())
                    .uri(path)
                    .header("authorization", bearer(&token))
                    .body(Body::empty())
                    .expect("build request"),
            )
            .await
            .expect("router response");

        assert_eq!(
            response.status(),
            StatusCode::FORBIDDEN,
            "{method} {path} must reject cross-agent scoped tokens",
        );
        let body = read_body_json(response).await;
        assert_eq!(
            body["error"]["code"], "forbidden",
            "{method} {path} must use the forbidden error envelope"
        );
    }
}

#[tokio::test]
async fn nous_routes_admit_token_scoped_to_matching_agent() {
    // WHY: the scope check must be additive — a token scoped to `syn`
    // still reaches handlers for `/api/v1/nous/syn/...`. Asserts the new
    // `require_nous_access` calls do not break the happy path.
    let env = TestEnv::builder().with_actor(true).build().await;
    let token = issue_test_token_scoped(&env.state, Role::Operator, "syn");
    let router = build_router(Arc::clone(&env.state), &permissive_security());

    let response = router
        .clone()
        .oneshot(
            Request::get("/api/v1/nous/syn/tools")
                .header("authorization", bearer(&token))
                .body(Body::empty())
                .expect("build request"),
        )
        .await
        .expect("router response");

    assert_eq!(
        response.status(),
        StatusCode::OK,
        "matching scope must reach the handler"
    );
}

#[tokio::test]
async fn nous_list_hides_other_agents_from_scoped_token() {
    // WHY: a token scoped to a single nous_id should not be able to
    // enumerate other agents via GET /api/v1/nous, even if those agents
    // are public. The list filters by the caller's scope (#enumeration).
    let env = TestEnv::new().await;
    let token = issue_test_token_scoped(&env.state, Role::Operator, "syn");
    let router = build_router(Arc::clone(&env.state), &permissive_security());

    let response = router
        .clone()
        .oneshot(
            Request::get(format!("{API_V1}/nous"))
                .header("authorization", bearer(&token))
                .body(Body::empty())
                .expect("build request"),
        )
        .await
        .expect("router response");

    assert_eq!(response.status(), StatusCode::OK);
    let body = read_body_json(response).await;
    let entries = body["nous"].as_array().expect("nous list must be an array");
    for entry in entries {
        let id = entry["id"].as_str().expect("entry must have id");
        assert_eq!(
            id, "syn",
            "scoped token must only see its own agent; saw `{id}`"
        );
    }
}

#[tokio::test]
async fn insights_agent_metrics_admit_matching_scoped_agent_token() {
    let env = TestEnv::builder().with_actor(true).build().await;
    let token = issue_test_token_scoped(&env.state, Role::Agent, "syn");
    let router = build_router(Arc::clone(&env.state), &permissive_security());

    let response = router
        .oneshot(
            Request::get("/api/v1/metrics/agents/syn")
                .header("authorization", bearer(&token))
                .body(Body::empty())
                .expect("build request"),
        )
        .await
        .expect("router response");

    assert_eq!(response.status(), StatusCode::OK);
    let body = read_body_json(response).await;
    assert_eq!(body["agent_id"], "syn");
    assert!(
        body.get("agents").is_none(),
        "scoped per-agent metrics must not return the global agent list"
    );
}

#[tokio::test]
async fn insights_agent_metrics_reject_cross_agent_scoped_token() {
    let env = TestEnv::builder().with_actor(true).build().await;
    let token = issue_test_token_scoped(&env.state, Role::Agent, "other-agent");
    let router = build_router(Arc::clone(&env.state), &permissive_security());

    let response = router
        .oneshot(
            Request::get("/api/v1/metrics/agents/syn")
                .header("authorization", bearer(&token))
                .body(Body::empty())
                .expect("build request"),
        )
        .await
        .expect("router response");

    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    let body = read_body_json(response).await;
    assert_eq!(body["error"]["code"], "forbidden");
}

#[tokio::test]
async fn insights_aggregate_routes_reject_scoped_operator_token() {
    let env = TestEnv::new().await;
    let token = issue_test_token_scoped(&env.state, Role::Operator, "syn");
    let router = build_router(Arc::clone(&env.state), &permissive_security());

    for path in insights_aggregate_routes() {
        let response = router
            .clone()
            .oneshot(
                Request::get(path)
                    .header("authorization", bearer(&token))
                    .body(Body::empty())
                    .expect("build request"),
            )
            .await
            .expect("router response");

        assert_eq!(
            response.status(),
            StatusCode::FORBIDDEN,
            "{path} must reject scoped tokens for aggregate telemetry"
        );
        let body = read_body_json(response).await;
        assert_eq!(body["error"]["code"], "forbidden");
    }
}

#[tokio::test]
async fn knowledge_write_routes_with_operator_bearer_reach_handlers() {
    let env = TestEnv::new().await;
    let token = issue_test_token(&env.state);
    let router = build_router(Arc::clone(&env.state), &permissive_security());

    for route in knowledge_write_routes() {
        let response = router
            .clone()
            .oneshot(route.request(Some(bearer(&token))))
            .await
            .expect("router response");

        assert_eq!(
            response.status(),
            KnowledgeWriteRoute::expected_authorized_status(),
            "{} {}",
            route.method,
            route.path
        );
    }
}

#[derive(Clone)]
struct KnowledgeWriteRoute {
    method: Method,
    path: &'static str,
    body: Option<serde_json::Value>,
}

impl KnowledgeWriteRoute {
    fn request(&self, authorization: Option<String>) -> Request<Body> {
        let mut builder = Request::builder()
            .method(self.method.clone())
            .uri(self.path);
        if self.body.is_some() {
            builder = builder.header("content-type", "application/json");
        }
        if let Some(header) = authorization {
            builder = builder.header("authorization", header);
        }

        match &self.body {
            Some(body) => builder
                .body(Body::from(
                    serde_json::to_vec(&body).expect("serialize json body"),
                ))
                .expect("build request"),
            None => builder.body(Body::empty()).expect("build request"),
        }
    }

    fn expected_authorized_status() -> StatusCode {
        StatusCode::SERVICE_UNAVAILABLE
    }
}

/// Read-only `/api/v1/...` routes that must reject anonymous requests.
///
/// These handlers do not run inside a `route_layer` like knowledge does, and
/// they do not perform write actions, but they read agent telemetry and
/// planning state that should not be exposed without a verified bearer.
fn unauthenticated_read_routes() -> [(Method, &'static str); 8] {
    [
        (Method::GET, "/api/v1/metrics/agents"),
        (Method::GET, "/api/v1/metrics/agents/syn"),
        (Method::GET, "/api/v1/metrics/quality"),
        (Method::GET, "/api/v1/metrics/tokens"),
        (Method::GET, "/api/v1/metrics/costs"),
        (Method::GET, "/api/v1/journal"),
        (
            Method::GET,
            "/api/v1/planning/projects/some-project/verification",
        ),
        (
            Method::POST,
            "/api/v1/planning/projects/some-project/verification/refresh",
        ),
    ]
}

fn insights_aggregate_routes() -> [&'static str; 5] {
    [
        "/api/v1/metrics/agents",
        "/api/v1/metrics/quality",
        "/api/v1/metrics/tokens",
        "/api/v1/metrics/costs",
        "/api/v1/journal",
    ]
}

#[tokio::test]
async fn insights_and_planning_routes_reject_missing_bearer_token() {
    // WHY: regression guard — without Claims behind a `route_layer`, anonymous
    // callers could read per-agent token/cost metrics and planning state.
    // Every handler under `/api/v1/metrics/*`, `/api/v1/journal`, and
    // `/api/v1/planning/projects/{id}/verification[/refresh]` must reject
    // anonymous requests with 401.
    let env = TestEnv::new().await;
    let router = build_router(Arc::clone(&env.state), &permissive_security());

    for (method, path) in unauthenticated_read_routes() {
        let response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method(method.clone())
                    .uri(path)
                    .body(Body::empty())
                    .expect("build request"),
            )
            .await
            .expect("router response");

        assert_eq!(
            response.status(),
            StatusCode::UNAUTHORIZED,
            "{method} {path} must reject anonymous requests"
        );
    }
}

#[tokio::test]
async fn insights_and_planning_routes_reject_invalid_bearer_token() {
    let env = TestEnv::new().await;
    let router = build_router(Arc::clone(&env.state), &permissive_security());

    for (method, path) in unauthenticated_read_routes() {
        let response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method(method.clone())
                    .uri(path)
                    .header("authorization", "Bearer not.a.valid.jwt")
                    .body(Body::empty())
                    .expect("build request"),
            )
            .await
            .expect("router response");

        assert_eq!(
            response.status(),
            StatusCode::UNAUTHORIZED,
            "{method} {path} must reject invalid bearer"
        );
    }
}

#[tokio::test]
async fn insights_aggregate_routes_admit_unscoped_operator_token() {
    let env = TestEnv::new().await;
    let token = issue_test_token(&env.state);
    let router = build_router(Arc::clone(&env.state), &permissive_security());

    for path in insights_aggregate_routes() {
        let response = router
            .clone()
            .oneshot(
                Request::get(path)
                    .header("authorization", bearer(&token))
                    .body(Body::empty())
                    .expect("build request"),
            )
            .await
            .expect("router response");

        assert_eq!(
            response.status(),
            StatusCode::OK,
            "{path} must succeed with an unscoped operator bearer"
        );
    }
}

fn knowledge_write_routes() -> [KnowledgeWriteRoute; 7] {
    [
        KnowledgeWriteRoute {
            method: Method::POST,
            path: "/api/v1/knowledge/facts/import",
            body: Some(serde_json::json!({ "facts": [] })),
        },
        KnowledgeWriteRoute {
            method: Method::POST,
            path: "/api/v1/knowledge/ingest",
            body: Some(serde_json::json!({
                "content": "alice remembers the deployment window",
                "format": "text",
                "nous_id": "syn"
            })),
        },
        KnowledgeWriteRoute {
            method: Method::POST,
            path: "/api/v1/knowledge/ingest/webhook",
            body: Some(serde_json::json!({
                "nous_id": "syn",
                "facts": [],
                "source": "acme.corp"
            })),
        },
        KnowledgeWriteRoute {
            method: Method::POST,
            path: "/api/v1/knowledge/facts/some-fact-id/forget",
            body: Some(serde_json::json!({})),
        },
        KnowledgeWriteRoute {
            method: Method::POST,
            path: "/api/v1/knowledge/facts/some-fact-id/restore",
            body: None,
        },
        KnowledgeWriteRoute {
            method: Method::PUT,
            path: "/api/v1/knowledge/facts/some-fact-id/confidence",
            body: Some(serde_json::json!({ "confidence": 0.8 })),
        },
        KnowledgeWriteRoute {
            method: Method::PUT,
            path: "/api/v1/knowledge/facts/some-fact-id/sensitivity",
            body: Some(serde_json::json!({ "sensitivity": "public" })),
        },
    ]
}
