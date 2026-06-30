#![expect(
    clippy::unwrap_used,
    reason = "test assertions — panicking on failure is the point"
)]
#![expect(
    clippy::expect_used,
    reason = "test assertions — panicking on failure is the point"
)]

use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use tower::ServiceExt;

use diaporeia::server::DiaporeiaServer;
use diaporeia::state::DiaporeiaState;

use symbolon::types::Role;
use taxis::config::NousDefinition;

mod common;
use common::{StateBuilder, issue_token, issue_token_with_nous_id};

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

/// Build a JSON-RPC `resources/list` request body.
fn resources_list_request(id: u64) -> Body {
    let req = serde_json::json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": "resources/list",
    });
    Body::from(req.to_string())
}

/// Build a JSON-RPC `resources/read` request body.
fn resources_read_request(id: u64, uri: &str) -> Body {
    let req = serde_json::json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": "resources/read",
        "params": {
            "uri": uri,
        },
    });
    Body::from(req.to_string())
}

/// Extract the MCP resource list from a JSON-RPC response.
async fn extract_resources(response: axum::response::Response) -> Vec<serde_json::Value> {
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    json.get("result")
        .and_then(|r| r.get("resources"))
        .and_then(|r| r.as_array())
        .cloned()
        .unwrap_or_default()
}

/// Extract the MCP error code from a JSON-RPC response.
async fn extract_error_code(response: axum::response::Response) -> Option<i64> {
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .ok()?;
    let json: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    json.get("error")?.get("code")?.as_i64()
}

/// Add an agent to the in-memory config and create its workspace files under
/// the Oikos instance root.
async fn add_agent_with_workspace(state: &Arc<DiaporeiaState>, id: &str, files: &[&str]) {
    let root = state.oikos.root();
    let agent_dir = root.join("nous").join(id);
    std::fs::create_dir_all(&agent_dir).expect("create agent dir");
    for file in files {
        tokio::fs::write(agent_dir.join(file), format!("# {id} {file}\n"))
            .await
            .expect("write workspace file");
    }

    let mut config = state.config.write().await;
    config.agents.list.push(NousDefinition {
        id: id.to_owned(),
        name: Some(id.to_owned()),
        ..NousDefinition::default()
    });
}

#[tokio::test(flavor = "multi_thread")]
async fn resources_list_includes_config_and_existing_workspace_files() {
    let (state, jwt, _tmp) = StateBuilder::new().build();
    add_agent_with_workspace(&state, "syn", &["SOUL.md", "TOOLS.md"]).await;

    let token = issue_token(&jwt, "operator-1", Role::Operator);
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
                .body(resources_list_request(1))
                .unwrap(),
        )
        .await
        .unwrap();

    let resources = extract_resources(response).await;
    let uris: Vec<&str> = resources
        .iter()
        .filter_map(|r| r.get("uri").and_then(|u| u.as_str()))
        .collect();

    assert!(
        uris.contains(&"aletheia://config"),
        "config resource must be listed"
    );
    assert!(
        uris.contains(&"aletheia://nous/syn/soul"),
        "existing soul file must be listed"
    );
    assert!(
        uris.contains(&"aletheia://nous/syn/tools"),
        "existing tools file must be listed"
    );
    assert!(
        !uris.contains(&"aletheia://nous/syn/memory"),
        "missing memory file must not be listed"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn scoped_operator_resources_list_filters_workspace_files_to_scope() {
    let (state, jwt, _tmp) = StateBuilder::new().build();
    add_agent_with_workspace(&state, "syn", &["SOUL.md"]).await;
    add_agent_with_workspace(&state, "demiurge", &["SOUL.md"]).await;

    let token = issue_token_with_nous_id(&jwt, "operator-syn", Role::Operator, "syn");
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
                .body(resources_list_request(1))
                .unwrap(),
        )
        .await
        .unwrap();

    let resources = extract_resources(response).await;
    let uris: Vec<&str> = resources
        .iter()
        .filter_map(|r| r.get("uri").and_then(|u| u.as_str()))
        .collect();

    assert!(
        uris.contains(&"aletheia://nous/syn/soul"),
        "scoped operator must see in-scope workspace files"
    );
    assert!(
        !uris.contains(&"aletheia://nous/demiurge/soul"),
        "scoped operator must not be shown sibling workspace files"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn scoped_operator_resources_read_rejects_cross_agent_workspace_file() {
    let (state, jwt, _tmp) = StateBuilder::new().build();
    add_agent_with_workspace(&state, "demiurge", &["SOUL.md"]).await;

    let token = issue_token_with_nous_id(&jwt, "operator-syn", Role::Operator, "syn");
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
                .body(resources_read_request(1, "aletheia://nous/demiurge/soul"))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        extract_error_code(response).await,
        Some(-32001),
        "cross-agent resource read must return MCP unauthorized"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn resources_list_rejects_non_operator() {
    let (state, jwt, _tmp) = StateBuilder::new().build();

    let token = issue_token(&jwt, "readonly-1", Role::Readonly);
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
                .body(resources_list_request(1))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(
        json.get("error")
            .and_then(|e| e.get("code"))
            .and_then(serde_json::Value::as_i64),
        Some(-32001),
        "Readonly callers must not list resources"
    );
}
