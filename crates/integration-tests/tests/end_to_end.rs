//! End-to-end integration tests: HTTP → pipeline → provider → persistence.

#![expect(clippy::expect_used, reason = "test assertions")]
#![expect(
    clippy::indexing_slicing,
    reason = "integration tests: index-based assertions on known-length slices"
)]

use std::sync::{Arc, Mutex};

use axum::body::Body;
use axum::http::{Request, StatusCode};
use tower::ServiceExt;

use hermeneus::provider::LlmProvider;
use hermeneus::test_utils::MockProvider;
use hermeneus::types::*;
use integration_tests::harness::{TestHarness, body_json, body_string};
use mneme::embedding::EmbeddingProvider;
use mneme::id::{EmbeddingId, FactId};
use mneme::knowledge::{
    EmbeddedChunk, EpistemicTier, Fact, FactAccess, FactLifecycle, FactProvenance, FactSensitivity,
    FactTemporal, Visibility,
};

// --- Mock Providers ---

struct CapturingMockProvider {
    response: CompletionResponse,
    captured: Arc<Mutex<Vec<CompletionRequest>>>,
}

impl CapturingMockProvider {
    fn new(captured: Arc<Mutex<Vec<CompletionRequest>>>) -> Self {
        Self {
            response: CompletionResponse {
                id: "msg_test".to_owned(),
                model: "mock-model".to_owned(),
                stop_reason: StopReason::EndTurn,
                content: vec![ContentBlock::Text {
                    text: "Hello from mock!".to_owned(),
                    citations: None,
                }],
                usage: Usage {
                    input_tokens: 10,
                    output_tokens: 5,
                    ..Usage::default()
                },
                cost_usd: None,
                duration_ms: None,
            },
            captured,
        }
    }

    fn response_for(&self, request: &CompletionRequest) -> CompletionResponse {
        let system = request.system.as_deref().unwrap_or_default();
        let text = if system.contains("search query expansion engine") {
            r#"["What does the Aletheia recall substrate store?","Aletheia recall substrate stores operator calibration facts"]"#
        } else if system.contains("Select up to") {
            r#"["fact-http-recall-1"]"#
        } else {
            return self.response.clone();
        };
        CompletionResponse {
            content: vec![ContentBlock::Text {
                text: text.to_owned(),
                citations: None,
            }],
            ..self.response.clone()
        }
    }
}

impl LlmProvider for CapturingMockProvider {
    fn complete<'a>(
        &'a self,
        request: &'a CompletionRequest,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<Output = hermeneus::error::Result<CompletionResponse>>
                + Send
                + 'a,
        >,
    > {
        Box::pin(async {
            #[expect(
                clippy::expect_used,
                reason = "test mock: poisoned lock means a test bug"
            )]
            self.captured
                .lock()
                .expect("lock poisoned")
                .push(request.clone());
            Ok(self.response_for(request))
        })
    }

    fn supported_models(&self) -> &[&str] {
        &["mock-model"]
    }

    #[expect(clippy::unnecessary_literal_bound, reason = "trait requires &str")]
    fn name(&self) -> &str {
        "mock-capturing"
    }
}

async fn build_capturing() -> (TestHarness, Arc<Mutex<Vec<CompletionRequest>>>) {
    let captured = Arc::new(Mutex::new(Vec::new()));
    let provider = CapturingMockProvider::new(Arc::clone(&captured));
    let harness = TestHarness::build_with_provider(Box::new(provider)).await;
    (harness, captured)
}

#[cfg(feature = "knowledge-store")]
async fn build_capturing_with_knowledge_store() -> (TestHarness, Arc<Mutex<Vec<CompletionRequest>>>)
{
    let captured = Arc::new(Mutex::new(Vec::new()));
    let provider = CapturingMockProvider::new(Arc::clone(&captured));
    let harness = TestHarness::build_with_provider_and_knowledge_store(Box::new(provider)).await;
    (harness, captured)
}

#[cfg(feature = "knowledge-store")]
fn recall_fact(id: &str, content: &str) -> Fact {
    let now = "2026-03-01T00:00:00Z".parse().expect("valid timestamp");
    Fact {
        id: FactId::new(id).expect("valid fact id"),
        nous_id: "test-nous".to_owned(),
        content: content.to_owned(),
        fact_type: "observation".to_owned(),
        scope: None,
        temporal: FactTemporal {
            valid_from: now,
            valid_to: mneme::knowledge::far_future(),
            recorded_at: now,
        },
        provenance: FactProvenance {
            confidence: 0.95,
            tier: EpistemicTier::Verified,
            source_session_id: Some("session-http-recall".to_owned()),
            stability_hours: mneme::knowledge::default_stability_hours("observation"),
        },
        lifecycle: FactLifecycle {
            superseded_by: None,
            is_forgotten: false,
            forgotten_at: None,
            forget_reason: None,
        },
        access: FactAccess {
            access_count: 0,
            last_accessed_at: None,
        },
        sensitivity: FactSensitivity::Public,
        visibility: Visibility::Private,
    }
}

#[cfg(feature = "knowledge-store")]
fn recall_embedding(embedder: &dyn EmbeddingProvider, fact: &Fact) -> EmbeddedChunk {
    EmbeddedChunk {
        id: EmbeddingId::new(format!("emb-{}", fact.id)).expect("valid embedding id"),
        content: fact.content.clone(),
        source_type: "fact".to_owned(),
        source_id: fact.id.to_string(),
        nous_id: fact.nous_id.clone(),
        embedding: embedder.embed(&fact.content).expect("embed fixture"),
        created_at: fact.temporal.recorded_at,
    }
}

// --- Tests ---

#[tokio::test]
async fn http_create_session_send_message_get_history() {
    let harness = TestHarness::build().await;
    let router = harness.router();

    let session = harness.create_session(&router).await;
    let id = session["id"].as_str().expect("session id");

    let req = harness.authed_request(
        "POST",
        &format!("/api/v1/sessions/{id}/messages"),
        Some(serde_json::json!({ "content": "hello" })),
    );
    let resp = router.clone().oneshot(req).await.expect("send message");
    assert_eq!(resp.status(), StatusCode::OK);

    let body = body_string(resp).await;
    assert!(
        body.contains("event: text_delta"),
        "SSE stream should contain text_delta event"
    );
    assert!(
        body.contains("Hello from mock!"),
        "SSE stream should contain mock response text"
    );
    assert!(
        body.contains("event: message_complete"),
        "SSE stream should contain message_complete event"
    );

    let resp = router
        .clone()
        .oneshot(harness.authed_get(&format!("/api/v1/sessions/{id}/history")))
        .await
        .expect("get history");
    assert_eq!(resp.status(), StatusCode::OK);

    let history = body_json(resp).await;
    let messages = history["messages"].as_array().expect("messages array");
    assert!(
        messages.len() >= 2,
        "history should contain user + assistant messages, got {}",
        messages.len()
    );
    assert_eq!(messages[0]["role"], "user");
    assert_eq!(messages[0]["content"], "hello");
}

#[tokio::test]
async fn session_persists_across_turns() {
    let harness = TestHarness::build().await;
    let router = harness.router();

    let session = harness.create_session(&router).await;
    let id = session["id"].as_str().expect("session id");

    let req = harness.authed_request(
        "POST",
        &format!("/api/v1/sessions/{id}/messages"),
        Some(serde_json::json!({ "content": "first" })),
    );
    let resp = router.clone().oneshot(req).await.expect("first turn");
    let _ = body_string(resp).await;

    let req = harness.authed_request(
        "POST",
        &format!("/api/v1/sessions/{id}/messages"),
        Some(serde_json::json!({ "content": "second" })),
    );
    let resp = router.clone().oneshot(req).await.expect("second turn");
    let _ = body_string(resp).await;

    let resp = router
        .clone()
        .oneshot(harness.authed_get(&format!("/api/v1/sessions/{id}/history")))
        .await
        .expect("get history");

    let history = body_json(resp).await;
    let messages = history["messages"].as_array().expect("messages array");
    assert!(
        messages.len() >= 4,
        "should have at least 4 messages (2 user + 2 assistant), got {}",
        messages.len()
    );

    assert_eq!(messages[0]["role"], "user");
    assert_eq!(messages[0]["content"], "first");
    assert_eq!(messages[1]["role"], "assistant");
    assert_eq!(messages[2]["role"], "user");
    assert_eq!(messages[2]["content"], "second");
    assert_eq!(messages[3]["role"], "assistant");
}

#[tokio::test]
async fn nous_status_reflects_configuration() {
    let harness = TestHarness::build().await;
    let router = harness.router();

    let resp = router
        .clone()
        .oneshot(harness.authed_get("/api/v1/nous"))
        .await
        .expect("list nous");
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;
    let agents = body["nous"].as_array().expect("nous array");
    assert_eq!(agents.len(), 1);
    assert_eq!(agents[0]["id"], "test-nous");

    let resp = router
        .clone()
        .oneshot(harness.authed_get("/api/v1/nous/test-nous"))
        .await
        .expect("get status");
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;
    assert_eq!(body["id"], "test-nous");
    assert!(body["context_window"].is_number());
    assert!(body["max_output_tokens"].is_number());

    let resp = router
        .clone()
        .oneshot(harness.authed_get("/api/v1/nous/nonexistent"))
        .await
        .expect("get nonexistent");
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn bootstrap_assembles_from_oikos() {
    let (harness, captured) = build_capturing().await;
    let router = harness.router();

    let session = harness.create_session(&router).await;
    let id = session["id"].as_str().expect("session id");

    let req = harness.authed_request(
        "POST",
        &format!("/api/v1/sessions/{id}/messages"),
        Some(serde_json::json!({ "content": "trigger bootstrap" })),
    );
    let resp = router.clone().oneshot(req).await.expect("send message");
    let _ = body_string(resp).await;

    #[expect(
        clippy::expect_used,
        reason = "test assertion: poisoned lock means a test bug"
    )]
    let requests = captured.lock().expect("lock poisoned");
    assert!(
        !requests.is_empty(),
        "mock provider should have received at least one request"
    );

    let system_prompt = requests
        .last()
        .expect("at least one provider request")
        .system
        .as_ref()
        .expect("system prompt should be present");
    assert!(
        system_prompt.contains("You are a test agent"),
        "system prompt should contain SOUL.md content, got: {system_prompt}"
    );
    assert!(
        system_prompt.contains("Test user"),
        "system prompt should contain USER.md content, got: {system_prompt}"
    );
}

#[cfg(feature = "knowledge-store")]
#[tokio::test]
async fn http_harness_wires_real_knowledge_store_and_embedding_provider() {
    let (harness, captured) = build_capturing_with_knowledge_store().await;
    let router = harness.router();
    assert!(
        harness.state.embedding_provider.is_some(),
        "specific invariant: harness must wire an embedding provider into AppState"
    );
    assert!(
        harness.state.nous_manager.knowledge_store().is_some(),
        "specific invariant: harness must wire a knowledge store into NousManager"
    );

    let fact = recall_fact(
        "fact-http-recall-1",
        "Aletheia recall substrate stores operator calibration facts",
    );
    let store = harness.knowledge_store();
    store.insert_fact(&fact).expect("insert recall fact");
    store
        .insert_embedding(&recall_embedding(
            harness.embedding_provider().as_ref(),
            &fact,
        ))
        .expect("insert recall embedding");
    let direct_results = store
        .search_vectors(
            harness
                .embedding_provider()
                .embed(&fact.content)
                .expect("embed"),
            5,
            50,
        )
        .expect("direct vector search");
    assert!(
        direct_results
            .iter()
            .any(|result| result.source_id == "fact-http-recall-1"),
        "specific invariant: harness knowledge_store must expose real vector search results"
    );

    let session = harness.create_session(&router).await;
    let id = session["id"].as_str().expect("session id");
    let body = harness
        .send_message(
            &router,
            id,
            "What does the Aletheia recall substrate store?",
        )
        .await;
    assert!(
        body.contains("event: message_complete"),
        "happy path should still complete after recall executes: {body}"
    );

    let requests = captured
        .lock()
        .expect("captured provider requests lock should not be poisoned");
    assert!(
        !requests.is_empty(),
        "specific invariant: HTTP message path should reach the provider while knowledge_store is wired"
    );
}

#[tokio::test]
async fn unknown_session_returns_404() {
    let harness = TestHarness::build().await;
    let router = harness.router();

    let resp = router
        .clone()
        .oneshot(harness.authed_get("/api/v1/sessions/nonexistent"))
        .await
        .expect("get unknown session");
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);

    let req = harness.authed_request(
        "POST",
        "/api/v1/sessions/nonexistent/messages",
        Some(serde_json::json!({ "content": "hello" })),
    );
    let resp = router.clone().oneshot(req).await.expect("send to unknown");
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn health_returns_status() {
    let harness = TestHarness::build().await;
    let router = harness.router();

    let resp = router
        .oneshot(
            Request::get("/api/health")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("health check");
    assert_eq!(resp.status(), StatusCode::OK);

    // WHY: the in-memory test harness doesn't wire real provider reachability,
    // a config file, or credential files, so the comprehensive health checks
    // added in #2918 push the overall status from "healthy" → "degraded".
    // The invariant for this test is that the endpoint returns 200 with a
    // recognized status string, not that the test harness produces "healthy".
    let body = body_json(resp).await;
    let status = body["status"]
        .as_str()
        .expect("health response should have status field");
    assert!(
        matches!(status, "healthy" | "degraded" | "unhealthy"),
        "health status must be one of healthy/degraded/unhealthy, got: {status}"
    );
}

#[tokio::test]
async fn unauthenticated_request_rejected() {
    let harness = TestHarness::build().await;
    let router = harness.router();

    let req = Request::builder()
        .method("POST")
        .uri("/api/v1/sessions")
        .header("content-type", "application/json")
        .body(Body::from(
            serde_json::to_vec(&serde_json::json!({
                "nous_id": "test-nous",
                "session_key": "unauthed"
            }))
            .expect("serialize"),
        ))
        .expect("request");

    let resp = router.oneshot(req).await.expect("unauthenticated request");
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn close_session_archives_it() {
    let harness = TestHarness::build().await;
    let router = harness.router();

    let session = harness.create_session(&router).await;
    let id = session["id"].as_str().expect("session id");

    let token = harness.auth_token();
    let req = Request::delete(format!("/api/v1/sessions/{id}"))
        .header("authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .expect("request");
    let resp = router.clone().oneshot(req).await.expect("close session");
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);

    // DELETE archives the session; subsequent GET must return 404 (#1251).
    let resp = router
        .clone()
        .oneshot(harness.authed_get(&format!("/api/v1/sessions/{id}")))
        .await
        .expect("get after delete");
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn send_message_stores_both_roles_in_history() {
    let harness = TestHarness::build().await;
    let router = harness.router();

    let session = harness.create_session(&router).await;
    let id = session["id"].as_str().expect("session id");

    let req = harness.authed_request(
        "POST",
        &format!("/api/v1/sessions/{id}/messages"),
        Some(serde_json::json!({ "content": "test message" })),
    );
    let resp = router.clone().oneshot(req).await.expect("send message");
    let _ = body_string(resp).await;

    let resp = router
        .clone()
        .oneshot(harness.authed_get(&format!("/api/v1/sessions/{id}/history")))
        .await
        .expect("get history");
    let history = body_json(resp).await;
    let messages = history["messages"].as_array().expect("messages array");

    assert_eq!(messages.len(), 2, "should have exactly user + assistant");
    assert_eq!(messages[0]["role"], "user");
    assert_eq!(messages[0]["content"], "test message");
    assert_eq!(messages[1]["role"], "assistant");
    assert_eq!(messages[1]["content"], "Hello from mock!");
}

#[tokio::test]
async fn provider_failure_returns_sse_error_event() {
    let harness = TestHarness::build_with_provider(Box::new(
        MockProvider::error("simulated provider failure").models(&["mock-model"]),
    ))
    .await;
    let router = harness.router();

    let session = harness.create_session(&router).await;
    let id = session["id"].as_str().expect("session id");

    let req = harness.authed_request(
        "POST",
        &format!("/api/v1/sessions/{id}/messages"),
        Some(serde_json::json!({ "content": "hello" })),
    );
    let resp = router.clone().oneshot(req).await.expect("send message");
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "streaming endpoint returns 200 even on provider error"
    );

    let body = body_string(resp).await;
    assert!(
        body.contains("event: error"),
        "SSE stream should contain error event, got: {body}"
    );
    assert!(
        body.contains("provider_unavailable"),
        "error event should have provider_unavailable code (ApiRequest errors map to provider_unavailable via nous UserFacingError), got: {body}"
    );
    assert!(
        body.contains("event: message_complete"),
        "SSE stream should contain message_complete even on error, got: {body}"
    );
}

#[tokio::test]
async fn oversized_payload_returns_413() {
    let harness = TestHarness::build().await;
    let security = pylon::security::SecurityConfig {
        body_limit_bytes: 100,
        csrf: pylon::security::CsrfConfig {
            enabled: false,
            ..pylon::security::CsrfConfig::default()
        },
        ..pylon::security::SecurityConfig::default()
    };
    let router = harness.router_with_security(&security);

    let big_body = "x".repeat(200);
    let req = harness.authed_request(
        "POST",
        "/api/v1/sessions",
        Some(serde_json::json!({
            "nous_id": "test-nous",
            "session_key": "413-test",
            "extra": big_body
        })),
    );
    let resp = router.oneshot(req).await.expect("oneshot");
    assert_eq!(resp.status(), StatusCode::PAYLOAD_TOO_LARGE);
}

#[tokio::test]
async fn rate_limited_request_returns_429() {
    let harness = TestHarness::build().await;
    let security = pylon::security::SecurityConfig {
        csrf: pylon::security::CsrfConfig {
            enabled: false,
            ..pylon::security::CsrfConfig::default()
        },
        rate_limit: pylon::security::RateLimitConfig {
            enabled: false,
            per_user: taxis::config::PerUserRateLimitConfig {
                enabled: true,
                default_rpm: 60,
                default_burst: 2,
                llm_rpm: 60,
                llm_burst: 1,
                tool_rpm: 60,
                tool_burst: 1,
                stale_after_secs: 600,
            },
            ..pylon::security::RateLimitConfig::default()
        },
        ..pylon::security::SecurityConfig::default()
    };
    let router = harness.router_with_security(&security);

    let session = harness.create_session(&router).await;
    let id = session["id"].as_str().expect("session id");

    // Reuse the same token so all requests hash to the same per-user bucket.
    let token = harness.auth_token();

    // First LLM request should succeed (burst = 1).
    let req = harness.authed_request_with_token(
        &token,
        "POST",
        &format!("/api/v1/sessions/{id}/messages"),
        Some(serde_json::json!({ "content": "first" })),
    );
    let resp = router.clone().oneshot(req).await.expect("first message");
    assert_eq!(resp.status(), StatusCode::OK);
    let _ = body_string(resp).await;

    // Second LLM request should be rate-limited (burst = 1).
    let req = harness.authed_request_with_token(
        &token,
        "POST",
        &format!("/api/v1/sessions/{id}/messages"),
        Some(serde_json::json!({ "content": "second" })),
    );
    let resp = router.clone().oneshot(req).await.expect("second message");
    assert_eq!(resp.status(), StatusCode::TOO_MANY_REQUESTS);
}
