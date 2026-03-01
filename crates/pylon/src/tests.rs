//! Integration tests for the pylon HTTP gateway.

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};
    use std::time::Instant;

    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;

    use aletheia_hermeneus::provider::{LlmProvider, ProviderConfig, ProviderRegistry};
    use aletheia_hermeneus::types::*;
    use aletheia_mneme::store::SessionStore;
    use aletheia_nous::config::NousConfig;
    use aletheia_nous::session::SessionManager;
    use aletheia_organon::registry::ToolRegistry;
    use aletheia_taxis::oikos::Oikos;

    use crate::router::build_router;
    use crate::state::AppState;

    // --- Mock Provider ---

    struct MockProvider {
        response: CompletionResponse,
    }

    impl MockProvider {
        fn new() -> Self {
            Self {
                response: CompletionResponse {
                    id: "msg_test".to_owned(),
                    model: "mock-model".to_owned(),
                    stop_reason: StopReason::EndTurn,
                    content: vec![ContentBlock::Text {
                        text: "Hello from mock!".to_owned(),
                    }],
                    usage: Usage {
                        input_tokens: 10,
                        output_tokens: 5,
                        ..Usage::default()
                    },
                },
            }
        }

        fn with_thinking() -> Self {
            Self {
                response: CompletionResponse {
                    id: "msg_think".to_owned(),
                    model: "mock-model".to_owned(),
                    stop_reason: StopReason::EndTurn,
                    content: vec![
                        ContentBlock::Thinking {
                            thinking: "Let me think...".to_owned(),
                        },
                        ContentBlock::Text {
                            text: "The answer is 42.".to_owned(),
                        },
                    ],
                    usage: Usage {
                        input_tokens: 20,
                        output_tokens: 10,
                        ..Usage::default()
                    },
                },
            }
        }
    }

    impl LlmProvider for MockProvider {
        fn complete(&self, _request: &CompletionRequest) -> aletheia_hermeneus::error::Result<CompletionResponse> {
            Ok(self.response.clone())
        }

        fn supported_models(&self) -> &[&str] {
            &["mock-model", "claude-opus-4-20250514"]
        }

        #[expect(clippy::unnecessary_literal_bound, reason = "trait requires &str")]
        fn name(&self) -> &str {
            "mock"
        }
    }

    // --- Test Helpers ---

    fn test_state() -> Arc<AppState> {
        test_state_with_provider(true)
    }

    fn test_state_with_provider(with_provider: bool) -> Arc<AppState> {
        let store = SessionStore::open_in_memory().expect("in-memory store");
        let nous_config = NousConfig {
            id: "syn".to_owned(),
            ..NousConfig::default()
        };
        let session_manager = SessionManager::new(nous_config);

        let mut provider_registry = ProviderRegistry::new();
        if with_provider {
            provider_registry.register(Box::new(MockProvider::new()));
        }

        let tool_registry = ToolRegistry::new();
        let oikos = Oikos::from_root("/tmp/aletheia-test");

        Arc::new(AppState {
            session_store: Mutex::new(store),
            session_manager,
            provider_registry,
            tool_registry,
            oikos,
            start_time: Instant::now(),
        })
    }

    fn app() -> axum::Router {
        build_router(test_state())
    }

    fn app_no_providers() -> axum::Router {
        build_router(test_state_with_provider(false))
    }

    fn json_request(method: &str, uri: &str, body: Option<serde_json::Value>) -> Request<Body> {
        let builder = Request::builder()
            .method(method)
            .uri(uri)
            .header("content-type", "application/json");

        match body {
            Some(b) => builder
                .body(Body::from(serde_json::to_vec(&b).unwrap()))
                .unwrap(),
            None => builder.body(Body::empty()).unwrap(),
        }
    }

    async fn body_json(response: axum::response::Response) -> serde_json::Value {
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    async fn body_string(response: axum::response::Response) -> String {
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        String::from_utf8(bytes.to_vec()).unwrap()
    }

    async fn create_test_session(app: &axum::Router) -> serde_json::Value {
        let req = json_request(
            "POST",
            "/api/sessions",
            Some(serde_json::json!({
                "nous_id": "syn",
                "session_key": "test-session"
            })),
        );
        let resp = app.clone().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        body_json(resp).await
    }

    // --- Health Tests ---

    #[tokio::test]
    async fn health_returns_200() {
        let resp = app()
            .oneshot(Request::get("/api/health").body(Body::empty()).unwrap())
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body = body_json(resp).await;
        assert_eq!(body["status"], "healthy");
        assert!(body["version"].is_string());
        assert!(body["uptime_seconds"].is_number());
        assert!(body["checks"].is_array());
    }

    #[tokio::test]
    async fn health_degraded_without_providers() {
        let resp = app_no_providers()
            .oneshot(Request::get("/api/health").body(Body::empty()).unwrap())
            .await
            .unwrap();

        let body = body_json(resp).await;
        assert_eq!(body["status"], "degraded");
    }

    // --- Session CRUD Tests ---

    #[tokio::test]
    async fn create_session_returns_201() {
        let session = create_test_session(&app()).await;
        assert!(session["id"].is_string());
        assert_eq!(session["nous_id"], "syn");
        assert_eq!(session["session_key"], "test-session");
        assert_eq!(session["status"], "active");
    }

    #[tokio::test]
    async fn get_session_returns_created_session() {
        let router = app();
        let created = create_test_session(&router).await;
        let id = created["id"].as_str().unwrap();

        let resp = router
            .clone()
            .oneshot(
                Request::get(&format!("/api/sessions/{id}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body = body_json(resp).await;
        assert_eq!(body["id"], id);
        assert_eq!(body["nous_id"], "syn");
    }

    #[tokio::test]
    async fn get_unknown_session_returns_404() {
        let resp = app()
            .oneshot(
                Request::get("/api/sessions/nonexistent")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
        let body = body_json(resp).await;
        assert_eq!(body["error"]["code"], "session_not_found");
    }

    #[tokio::test]
    async fn close_session_returns_204() {
        let router = app();
        let created = create_test_session(&router).await;
        let id = created["id"].as_str().unwrap();

        let resp = router
            .clone()
            .oneshot(
                Request::delete(&format!("/api/sessions/{id}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::NO_CONTENT);
    }

    #[tokio::test]
    async fn get_closed_session_shows_archived() {
        let router = app();
        let created = create_test_session(&router).await;
        let id = created["id"].as_str().unwrap();

        // Close
        router
            .clone()
            .oneshot(
                Request::delete(&format!("/api/sessions/{id}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        // Get again
        let resp = router
            .clone()
            .oneshot(
                Request::get(&format!("/api/sessions/{id}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body = body_json(resp).await;
        assert_eq!(body["status"], "archived");
    }

    #[tokio::test]
    async fn close_unknown_session_returns_404() {
        let resp = app()
            .oneshot(
                Request::delete("/api/sessions/nonexistent")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    // --- History Tests ---

    #[tokio::test]
    async fn history_empty_for_new_session() {
        let router = app();
        let created = create_test_session(&router).await;
        let id = created["id"].as_str().unwrap();

        let resp = router
            .clone()
            .oneshot(
                Request::get(&format!("/api/sessions/{id}/history"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body = body_json(resp).await;
        assert!(body["messages"].as_array().unwrap().is_empty());
    }

    #[tokio::test]
    async fn history_unknown_session_returns_404() {
        let resp = app()
            .oneshot(
                Request::get("/api/sessions/nonexistent/history")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn history_with_limit() {
        let state = test_state();
        let router = build_router(Arc::clone(&state));

        let created = create_test_session(&router).await;
        let id = created["id"].as_str().unwrap();

        // Directly insert messages
        {
            let store = state.session_store.lock().unwrap();
            for i in 1..=5 {
                store
                    .append_message(
                        id,
                        aletheia_mneme::types::Role::User,
                        &format!("message {i}"),
                        None,
                        None,
                        10,
                    )
                    .unwrap();
            }
        }

        let resp = router
            .clone()
            .oneshot(
                Request::get(&format!("/api/sessions/{id}/history?limit=3"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        let body = body_json(resp).await;
        assert_eq!(body["messages"].as_array().unwrap().len(), 3);
    }

    // --- SSE Message Tests ---

    #[tokio::test]
    async fn send_message_returns_sse_content_type() {
        let state = test_state();
        let router = build_router(Arc::clone(&state));
        let created = create_test_session(&router).await;
        let id = created["id"].as_str().unwrap();

        let req = json_request(
            "POST",
            &format!("/api/sessions/{id}/messages"),
            Some(serde_json::json!({ "content": "Hello!" })),
        );

        let resp = router.clone().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let content_type = resp
            .headers()
            .get("content-type")
            .unwrap()
            .to_str()
            .unwrap();
        assert!(content_type.contains("text/event-stream"));
    }

    #[tokio::test]
    async fn send_message_stream_contains_events() {
        let state = test_state();
        let router = build_router(Arc::clone(&state));
        let created = create_test_session(&router).await;
        let id = created["id"].as_str().unwrap();

        let req = json_request(
            "POST",
            &format!("/api/sessions/{id}/messages"),
            Some(serde_json::json!({ "content": "Hello!" })),
        );

        let resp = router.clone().oneshot(req).await.unwrap();
        let body = body_string(resp).await;

        assert!(body.contains("event: text_delta"), "should contain text_delta event");
        assert!(body.contains("Hello from mock!"), "should contain mock response text");
        assert!(body.contains("event: message_complete"), "should contain message_complete event");
    }

    #[tokio::test]
    async fn send_message_unknown_session_returns_404() {
        let req = json_request(
            "POST",
            "/api/sessions/nonexistent/messages",
            Some(serde_json::json!({ "content": "Hello!" })),
        );

        let resp = app().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn send_empty_message_returns_400() {
        let router = app();
        let created = create_test_session(&router).await;
        let id = created["id"].as_str().unwrap();

        let req = json_request(
            "POST",
            &format!("/api/sessions/{id}/messages"),
            Some(serde_json::json!({ "content": "" })),
        );

        let resp = router.clone().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn send_message_stores_in_history() {
        let state = test_state();
        let router = build_router(Arc::clone(&state));
        let created = create_test_session(&router).await;
        let id = created["id"].as_str().unwrap();

        // Send a message
        let req = json_request(
            "POST",
            &format!("/api/sessions/{id}/messages"),
            Some(serde_json::json!({ "content": "Hello!" })),
        );
        let resp = router.clone().oneshot(req).await.unwrap();
        // Consume the SSE body to ensure the blocking task completes
        let _ = body_string(resp).await;

        // Check history
        let resp = router
            .clone()
            .oneshot(
                Request::get(&format!("/api/sessions/{id}/history"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        let body = body_json(resp).await;
        let messages = body["messages"].as_array().unwrap();
        assert!(messages.len() >= 2, "should have user + assistant messages");

        assert_eq!(messages[0]["role"], "user");
        assert_eq!(messages[0]["content"], "Hello!");
    }

    // --- Error Format Tests ---

    #[tokio::test]
    async fn error_response_has_consistent_structure() {
        let resp = app()
            .oneshot(
                Request::get("/api/sessions/nonexistent")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        let body = body_json(resp).await;
        assert!(body["error"].is_object());
        assert!(body["error"]["code"].is_string());
        assert!(body["error"]["message"].is_string());
    }

    #[tokio::test]
    async fn malformed_create_body_returns_400() {
        let req = Request::builder()
            .method("POST")
            .uri("/api/sessions")
            .header("content-type", "application/json")
            .body(Body::from(r#"{"invalid": true}"#))
            .unwrap();

        let resp = app().oneshot(req).await.unwrap();
        // Axum returns 422 for deserialization failures
        assert!(
            resp.status() == StatusCode::BAD_REQUEST
                || resp.status() == StatusCode::UNPROCESSABLE_ENTITY
        );
    }

    #[tokio::test]
    async fn malformed_send_body_returns_error() {
        let router = app();
        let created = create_test_session(&router).await;
        let id = created["id"].as_str().unwrap();

        let req = Request::builder()
            .method("POST")
            .uri(&format!("/api/sessions/{id}/messages"))
            .header("content-type", "application/json")
            .body(Body::from(r#"{"wrong_field": "abc"}"#))
            .unwrap();

        let resp = router.clone().oneshot(req).await.unwrap();
        assert!(
            resp.status() == StatusCode::BAD_REQUEST
                || resp.status() == StatusCode::UNPROCESSABLE_ENTITY
        );
    }

    // --- Nous Tests ---

    #[tokio::test]
    async fn list_nous_returns_agents() {
        let resp = app()
            .oneshot(
                Request::get("/api/nous").body(Body::empty()).unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body = body_json(resp).await;
        let agents = body["nous"].as_array().unwrap();
        assert_eq!(agents.len(), 1);
        assert_eq!(agents[0]["id"], "syn");
    }

    #[tokio::test]
    async fn get_nous_status() {
        let resp = app()
            .oneshot(
                Request::get("/api/nous/syn").body(Body::empty()).unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body = body_json(resp).await;
        assert_eq!(body["id"], "syn");
        assert!(body["context_window"].is_number());
        assert!(body["max_output_tokens"].is_number());
    }

    #[tokio::test]
    async fn get_unknown_nous_returns_404() {
        let resp = app()
            .oneshot(
                Request::get("/api/nous/nonexistent")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
        let body = body_json(resp).await;
        assert_eq!(body["error"]["code"], "nous_not_found");
    }

    #[tokio::test]
    async fn get_nous_tools() {
        let resp = app()
            .oneshot(
                Request::get("/api/nous/syn/tools")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body = body_json(resp).await;
        assert!(body["tools"].is_array());
    }

    // --- Concurrent access ---

    #[tokio::test]
    async fn concurrent_session_creation() {
        let state = test_state();
        let mut handles = Vec::new();

        for i in 0..5 {
            let router = build_router(Arc::clone(&state));
            handles.push(tokio::spawn(async move {
                let req = json_request(
                    "POST",
                    "/api/sessions",
                    Some(serde_json::json!({
                        "nous_id": "syn",
                        "session_key": format!("concurrent-{i}")
                    })),
                );
                let resp = router.oneshot(req).await.unwrap();
                resp.status()
            }));
        }

        for handle in handles {
            let status = handle.await.unwrap();
            assert_eq!(status, StatusCode::CREATED);
        }
    }

    // --- SSE with no provider ---

    #[tokio::test]
    async fn send_message_no_provider_returns_error() {
        let state = test_state_with_provider(false);
        let router = build_router(Arc::clone(&state));
        let created = create_test_session(&router).await;
        let id = created["id"].as_str().unwrap();

        let req = json_request(
            "POST",
            &format!("/api/sessions/{id}/messages"),
            Some(serde_json::json!({ "content": "Hello!" })),
        );

        let resp = router.clone().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }
}
