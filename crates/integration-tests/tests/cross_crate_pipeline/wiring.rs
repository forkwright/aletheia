//! Additional wiring and provider inspection integration tests.
use super::*;

// ===========================================================================
// Additional wiring tests
// ===========================================================================

#[tokio::test]
async fn capturing_provider_receives_tool_definitions() {
    let captured = Arc::new(Mutex::new(Vec::new()));
    let provider = CapturingMockProvider::new("ok", Arc::clone(&captured));
    let harness = TestHarness::build_with_provider_and_tools(Box::new(provider), true).await;
    let router = harness.router();

    let session = harness.create_session(&router).await;
    let id = session["id"].as_str().expect("session id");

    let _ = harness.send_message(&router, id, "test tools").await;

    #[expect(
        clippy::expect_used,
        reason = "test assertion: poisoned lock means a test bug"
    )]
    let requests = captured.lock().expect("lock poisoned");
    assert!(!requests.is_empty());

    // When tools are registered, LLM should receive tool definitions
    assert!(
        !requests[0].tools.is_empty(),
        "LLM request should include tool definitions when tools are registered"
    );

    // Verify that known builtins appear
    let tool_names: Vec<&str> = requests[0].tools.iter().map(|t| t.name.as_str()).collect();
    assert!(
        tool_names.contains(&"note"),
        "tool definitions should include 'note', got: {tool_names:?}"
    );
}

#[tokio::test]
async fn health_endpoint_no_auth_required() {
    let harness = TestHarness::build().await;
    let router = harness.router();

    let req = Request::get("/api/health")
        .body(Body::empty())
        .expect("request");
    let resp = router.oneshot(req).await.expect("health check");
    assert_eq!(resp.status(), StatusCode::OK);

    let body = body_json(resp).await;
    assert_eq!(body["status"], "healthy");
}

#[tokio::test]
async fn nous_list_and_status_endpoints() {
    let harness = TestHarness::build().await;
    let router = harness.router();

    // List nous agents
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

    // Get agent status
    let resp = router
        .clone()
        .oneshot(harness.authed_get("/api/v1/nous/test-nous"))
        .await
        .expect("get status");
    assert_eq!(resp.status(), StatusCode::OK);

    // Get agent tools
    let resp = router
        .clone()
        .oneshot(harness.authed_get("/api/v1/nous/test-nous/tools"))
        .await
        .expect("get tools");
    assert_eq!(resp.status(), StatusCode::OK);

    // Nonexistent agent
    let resp = router
        .clone()
        .oneshot(harness.authed_get("/api/v1/nous/nonexistent"))
        .await
        .expect("get nonexistent");
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}
