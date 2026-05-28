#![expect(
    clippy::unwrap_used,
    reason = "test assertions — panicking on failure is the point"
)]
#![expect(
    clippy::expect_used,
    reason = "test assertions — panicking on failure is the point"
)]
#![expect(
    unused_imports,
    reason = "split public_api_*.rs files share the same import block"
)]

use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use tokio::sync::{Mutex as TokioMutex, RwLock};
use tokio_util::sync::CancellationToken;
use tower::ServiceExt;

use diaporeia::error::{Error, Result as DiaporeiaResult};
use diaporeia::server::DiaporeiaServer;
use diaporeia::state::DiaporeiaState;
use diaporeia::transport::streamable_http_router;

use hermeneus::provider::ProviderRegistry;
use koina::secret::SecretString;
use mneme::store::SessionStore;
use nous::manager::NousManager;
use organon::registry::ToolRegistry;
use symbolon::jwt::{JwtConfig, JwtManager};
use symbolon::types::Role;
use taxis::config::AletheiaConfig;
use taxis::oikos::Oikos;

mod common;
use common::{StateBuilder, issue_token};

// Split: Repomix MCP tool end-to-end tests.

// Section 6: Repomix MCP tools — end-to-end via socket
// -------------------------------------------------------------------

/// Build a test router in stateless+json mode.
fn test_router(state: &Arc<DiaporeiaState>) -> axum::Router {
    let rate_cfg = state.config.try_read().unwrap().mcp.rate_limit.clone();
    let server = DiaporeiaServer::with_state(Arc::clone(state), &rate_cfg);

    let auth_state = Arc::clone(state);
    let service = rmcp::transport::streamable_http_server::tower::StreamableHttpService::new(
        move || Ok(server.clone()),
        rmcp::transport::streamable_http_server::session::local::LocalSessionManager::default()
            .into(),
        rmcp::transport::streamable_http_server::StreamableHttpServerConfig::default()
            .with_stateful_mode(false)
            .with_json_response(true),
    );

    axum::Router::new()
        .nest_service("/mcp", service)
        .layer(axum::middleware::from_fn(move |req, next| {
            diaporeia::auth::mcp_auth(Arc::clone(&auth_state), req, next)
        }))
}

/// Helper to build a JSON-RPC tool call request body.
fn tool_call_request(id: u64, name: &str, arguments: &serde_json::Value) -> Body {
    let req = serde_json::json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": "tools/call",
        "params": {
            "name": name,
            "arguments": arguments,
        }
    });
    Body::from(req.to_string())
}

/// Helper to extract the text result from a JSON-RPC tool call response.
async fn extract_tool_text(response: axum::response::Response) -> Option<String> {
    if response.status() != StatusCode::OK {
        return None;
    }
    let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .ok()?;
    let json: serde_json::Value = serde_json::from_slice(&body_bytes).ok()?;
    let content = json.get("result")?.get("content")?.as_array()?;
    content.first()?.get("text")?.as_str().map(String::from)
}

#[tokio::test(flavor = "multi_thread")]
async fn repomix_templates_list_via_http_socket() {
    let (state, jwt, _tmp) = StateBuilder::new()
        .auth_mode("token")
        .repomix_enabled()
        .build();
    let token = issue_token(&jwt, "alice", Role::Agent);

    let router = test_router(&state);

    let response = router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header(header::HOST, "localhost")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .header(header::CONTENT_TYPE, "application/json")
                .header(header::ACCEPT, "application/json, text/event-stream")
                .body(tool_call_request(
                    1,
                    "repomix_templates_list",
                    &serde_json::json!({}),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let text = extract_tool_text(response)
        .await
        .expect("tool response text");
    assert!(
        text.contains("single_crate"),
        "must list single_crate template: {text}"
    );
    assert!(
        text.contains("crate_with_deps"),
        "must list crate_with_deps template: {text}"
    );
    assert!(
        text.contains("cross_crate"),
        "must list cross_crate template: {text}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn repomix_template_get_via_http_socket() {
    let (state, jwt, _tmp) = StateBuilder::new()
        .auth_mode("token")
        .repomix_enabled()
        .build();
    let token = issue_token(&jwt, "alice", Role::Agent);

    let router = test_router(&state);

    let response = router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header(header::HOST, "localhost")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .header(header::CONTENT_TYPE, "application/json")
                .header(header::ACCEPT, "application/json, text/event-stream")
                .body(tool_call_request(
                    1,
                    "repomix_template_get",
                    &serde_json::json!({"name": "single_crate"}),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let text = extract_tool_text(response)
        .await
        .expect("tool response text");
    assert!(
        text.contains("single_crate"),
        "must return single_crate template: {text}"
    );
    assert!(
        text.contains("description"),
        "must include description: {text}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn repomix_pack_rejects_agent_role() {
    // WHY: repomix_pack requires Operator; Agent must get -32001 unauthorized.
    let (state, jwt, _tmp) = StateBuilder::new()
        .auth_mode("token")
        .repomix_enabled()
        .build();
    let token = issue_token(&jwt, "alice", Role::Agent);

    let router = test_router(&state);

    let response = router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header(header::HOST, "localhost")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .header(header::CONTENT_TYPE, "application/json")
                .header(header::ACCEPT, "application/json, text/event-stream")
                .body(tool_call_request(
                    1,
                    "repomix_pack",
                    &serde_json::json!({
                        "crate_names": ["diaporeia"],
                        "template": "single_crate",
                    }),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
    let error = json.get("error").expect("error field must be present");
    assert_eq!(
        error.get("code").unwrap().as_i64(),
        Some(-32001),
        "Agent calling repomix_pack must get -32001 unauthorized"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn repomix_pack_allows_operator_role() {
    let (state, jwt, _tmp) = StateBuilder::new()
        .auth_mode("token")
        .repomix_enabled()
        .build();
    let token = issue_token(&jwt, "bob", Role::Operator);

    let router = test_router(&state);

    let response = router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header(header::HOST, "localhost")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .header(header::CONTENT_TYPE, "application/json")
                .header(header::ACCEPT, "application/json, text/event-stream")
                .body(tool_call_request(
                    1,
                    "repomix_pack",
                    &serde_json::json!({
                        "crate_names": ["diaporeia"],
                        "template": "single_crate",
                    }),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let text = extract_tool_text(response)
        .await
        .expect("tool response text");
    // The workspace root detection will fail because we're in a temp dir with no Cargo.toml.
    // The tool should return a repomix pack error (internal error) rather than unauthorized.
    assert!(
        text.contains("could not detect workspace root") || text.contains("packed_context"),
        "Operator must reach the pack logic, got: {text}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn repomix_tools_reject_when_disabled() {
    let (state, jwt, _tmp) = StateBuilder::new().auth_mode("token").build();
    let token = issue_token(&jwt, "alice", Role::Agent);

    let router = test_router(&state);

    let response = router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header(header::HOST, "localhost")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .header(header::CONTENT_TYPE, "application/json")
                .header(header::ACCEPT, "application/json, text/event-stream")
                .body(tool_call_request(
                    1,
                    "repomix_templates_list",
                    &serde_json::json!({}),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
    let error = json.get("error").expect("error field must be present");
    assert_eq!(
        error.get("code").unwrap().as_i64(),
        Some(-32003),
        "Disabled repomix must return -32003"
    );
}
