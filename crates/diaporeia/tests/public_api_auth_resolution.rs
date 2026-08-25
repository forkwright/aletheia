#![expect(
    clippy::unwrap_used,
    reason = "test assertions — panicking on failure is the point"
)]

use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use tower::ServiceExt;

use diaporeia::server::DiaporeiaServer;
use diaporeia::state::DiaporeiaState;

use symbolon::types::Role;

mod common;
use common::{StateBuilder, issue_token};

fn raw_mcp_router(state: &Arc<DiaporeiaState>) -> axum::Router {
    let rate_cfg = state.config.try_read().unwrap().mcp.rate_limit.clone();
    let rate_limiter = Arc::new(diaporeia::rate_limit::RateLimiter::from_config(&rate_cfg));
    let server = DiaporeiaServer::with_state(Arc::clone(state), rate_limiter);

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

fn resource_templates_request(id: u64) -> Body {
    let req = serde_json::json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": "resources/templates/list",
    });
    Body::from(req.to_string())
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

fn assert_mcp_unauthorized(json: &serde_json::Value) {
    assert_eq!(
        json.get("error")
            .and_then(|e| e.get("code"))
            .and_then(serde_json::Value::as_i64),
        Some(-32001)
    );
}

fn assert_mcp_success(json: &serde_json::Value) {
    assert!(
        json.get("result").is_some(),
        "expected successful MCP result, got {json}"
    );
}

fn unsigned_admin_token() -> String {
    let header = koina::base64::encode_url_safe_no_pad(br#"{"alg":"none","typ":"JWT"}"#);
    let payload = koina::base64::encode_url_safe_no_pad(
        serde_json::json!({
            "sub": "mallory",
            "role": "admin",
            "iss": "aletheia-diaporeia-tests",
            "iat": 1,
            "exp": 4_102_444_800_i64,
            "jti": "unsigned-admin",
            "kind": "access",
        })
        .to_string()
        .as_bytes(),
    );
    format!("{header}.{payload}.")
}

#[tokio::test(flavor = "multi_thread")]
async fn missing_auth_facade_in_token_mode_denies_tools_and_resources() {
    let (state, jwt, _tmp) = StateBuilder::new()
        .auth_mode("token")
        .missing_auth_facade()
        .build();
    let token = issue_token(&jwt, "alice", Role::Admin);

    let resources = post_mcp(
        raw_mcp_router(&state),
        &token,
        resource_templates_request(1),
    )
    .await;
    assert_mcp_unauthorized(&resources);

    let tools = post_mcp(raw_mcp_router(&state), &token, config_get_request(2)).await;
    assert_mcp_unauthorized(&tools);
}

#[tokio::test(flavor = "multi_thread")]
async fn valid_signed_token_resolves_role_for_tools_and_resources() {
    let (state, jwt, _tmp) = StateBuilder::new().auth_mode("token").build();
    let token = issue_token(&jwt, "alice", Role::Operator);

    let resources = post_mcp(
        raw_mcp_router(&state),
        &token,
        resource_templates_request(1),
    )
    .await;
    assert_mcp_success(&resources);

    let tools = post_mcp(raw_mcp_router(&state), &token, config_get_request(2)).await;
    assert_mcp_success(&tools);
}

#[tokio::test(flavor = "multi_thread")]
async fn unsigned_admin_token_is_denied_for_tools_and_resources() {
    let (state, _jwt, _tmp) = StateBuilder::new().auth_mode("token").build();
    let token = unsigned_admin_token();

    let resources = post_mcp(
        raw_mcp_router(&state),
        &token,
        resource_templates_request(1),
    )
    .await;
    assert_mcp_unauthorized(&resources);

    let tools = post_mcp(raw_mcp_router(&state), &token, config_get_request(2)).await;
    assert_mcp_unauthorized(&tools);
}
