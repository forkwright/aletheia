//! WHY(#5182, #4843): MCP rate limiting must survive session churn and be
//! keyed by authenticated principal, not reset every time a client opens a
//! new session against the same transport bind.

#![expect(
    clippy::unwrap_used,
    reason = "test assertions — panicking on failure is the point"
)]

use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use tower::ServiceExt;

use diaporeia::rate_limit::RateLimiter;
use diaporeia::server::DiaporeiaServer;

use symbolon::types::Role;

mod common;
use common::{StateBuilder, issue_token};

/// Mount a single, already-constructed server behind its own stateless router.
///
/// Each call simulates a distinct MCP session: production transport wiring
/// (`transport.rs`) constructs a fresh `DiaporeiaServer` per streamable-HTTP
/// session, sharing one `Arc<RateLimiter>` across all of them.
fn router_for(server: DiaporeiaServer) -> axum::Router {
    let service = rmcp::transport::streamable_http_server::tower::StreamableHttpService::new(
        move || Ok(server.clone()),
        rmcp::transport::streamable_http_server::session::local::LocalSessionManager::default()
            .into(),
        rmcp::transport::streamable_http_server::StreamableHttpServerConfig::default()
            .with_legacy_session_mode(false)
            .with_json_response(true),
    );
    axum::Router::new().nest_service("/mcp", service)
}

fn config_get_request(id: u64) -> Body {
    let req = serde_json::json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": "tools/call",
        "params": {
            "name": "config_get",
            "arguments": {},
        },
    });
    Body::from(req.to_string())
}

async fn post_mcp(router: axum::Router, token: &str, body: Body) -> serde_json::Value {
    let response = router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header(header::HOST, "localhost")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .header(header::CONTENT_TYPE, "application/json")
                .header(header::ACCEPT, "application/json, text/event-stream")
                .body(body)
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

fn assert_mcp_success(json: &serde_json::Value) {
    assert!(
        json.get("result").is_some(),
        "expected successful MCP result, got {json}"
    );
}

fn assert_mcp_rate_limited(json: &serde_json::Value) {
    assert_eq!(
        json.get("error")
            .and_then(|e| e.get("code"))
            .and_then(serde_json::Value::as_i64),
        Some(-32000),
        "expected rate-limit error, got {json}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn quota_persists_across_new_server_instances_sharing_one_limiter() {
    // WHY(#5182, #4843): this is the exact regression described in both
    // issues — the streamable HTTP transport builds a fresh `DiaporeiaServer`
    // per session. Before the fix, `with_state` built its own `RateLimiter`
    // internally from a config snapshot, so `server_b` below would have
    // gotten a brand-new, un-exhausted budget: a client could reset its
    // quota simply by opening a new session. `with_state` now takes an
    // already-built `Arc<RateLimiter>` — the only way to share budget across
    // sessions — so this test exercises the production wiring pattern
    // directly (`transport.rs` builds one limiter per bind and clones the
    // `Arc` into every session's server).
    let (state, jwt, _tmp) = StateBuilder::new().auth_mode("token").build();
    {
        let mut cfg = state.config.write().await;
        cfg.mcp.rate_limit.enabled = true;
        cfg.mcp.rate_limit.read_requests_per_minute = 1;
    }
    let token = issue_token(&jwt, "alice", Role::Operator);

    let rate_cfg = state.config.read().await.mcp.rate_limit.clone();
    let shared_limiter = Arc::new(RateLimiter::from_config(&rate_cfg));

    let server_a = DiaporeiaServer::with_state(Arc::clone(&state), Arc::clone(&shared_limiter));
    let server_b = DiaporeiaServer::with_state(Arc::clone(&state), Arc::clone(&shared_limiter));

    // "Session A" consumes the single available cheap-tier token.
    let first = post_mcp(router_for(server_a), &token, config_get_request(1)).await;
    assert_mcp_success(&first);

    // "Session B" is a fresh `DiaporeiaServer` (as a new streamable-HTTP
    // session would get) but shares the same `Arc<RateLimiter>` — the same
    // principal's quota must already be exhausted.
    let second = post_mcp(router_for(server_b), &token, config_get_request(2)).await;
    assert_mcp_rate_limited(&second);
}

#[tokio::test(flavor = "multi_thread")]
async fn distinct_principals_do_not_share_a_budget_across_sessions() {
    // WHY(#5182): budgets are keyed by authenticated principal — a second,
    // distinct caller sharing the same limiter must not be throttled by the
    // first caller's exhausted quota.
    let (state, jwt, _tmp) = StateBuilder::new().auth_mode("token").build();
    {
        let mut cfg = state.config.write().await;
        cfg.mcp.rate_limit.enabled = true;
        cfg.mcp.rate_limit.read_requests_per_minute = 1;
    }
    let alice = issue_token(&jwt, "alice", Role::Operator);
    let bob = issue_token(&jwt, "bob", Role::Operator);

    let rate_cfg = state.config.read().await.mcp.rate_limit.clone();
    let shared_limiter = Arc::new(RateLimiter::from_config(&rate_cfg));

    let server_a = DiaporeiaServer::with_state(Arc::clone(&state), Arc::clone(&shared_limiter));
    let server_b = DiaporeiaServer::with_state(Arc::clone(&state), Arc::clone(&shared_limiter));

    let alice_result = post_mcp(router_for(server_a), &alice, config_get_request(1)).await;
    assert_mcp_success(&alice_result);

    let bob_result = post_mcp(router_for(server_b), &bob, config_get_request(2)).await;
    assert_mcp_success(&bob_result);
}
