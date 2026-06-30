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

/// Build a JSON-RPC tool call request body.
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

/// Extract the MCP error code from a JSON-RPC tool call response.
async fn extract_error_code(response: axum::response::Response) -> Option<i64> {
    let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .ok()?;
    let json: serde_json::Value = serde_json::from_slice(&body_bytes).ok()?;
    json.get("error")?.get("code")?.as_i64()
}

/// Extract the text result from a JSON-RPC tool call response.
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

/// Create a session for `nous_id` and return its generated session id.
async fn create_session(state: &Arc<DiaporeiaState>, nous_id: &str, session_key: &str) -> String {
    let store = state.session_store.lock().await;
    let session_id = koina::id::SessionId::new().to_string();
    store
        .create_session(&session_id, nous_id, session_key, None, Some("test-model"))
        .expect("create session");
    session_id
}

#[cfg(feature = "knowledge-store")]
fn insert_test_fact(store: &mneme::knowledge_store::KnowledgeStore, nous_id: &str, content: &str) {
    use jiff::Timestamp;
    use mneme::id::FactId;
    use mneme::knowledge::{
        EpistemicTier, Fact, FactAccess, FactLifecycle, FactProvenance, FactSensitivity,
        FactTemporal, Visibility, default_stability_hours, far_future,
    };

    let now = Timestamp::now();
    let fact_id = format!("f-test-{}", koina::uuid::uuid_v4());
    let fact = Fact {
        id: FactId::new(&fact_id).expect("valid fact id"),
        nous_id: nous_id.to_owned(),
        fact_type: "knowledge".to_owned(),
        content: content.to_owned(),
        scope: None,
        project_id: None,
        sensitivity: FactSensitivity::Public,
        visibility: Visibility::Private,
        temporal: FactTemporal {
            valid_from: now,
            valid_to: far_future(),
            recorded_at: now,
        },
        provenance: FactProvenance {
            confidence: 0.9,
            tier: EpistemicTier::Verified,
            source_session_id: None,
            stability_hours: default_stability_hours("knowledge"),
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
    };
    store.insert_fact(&fact).expect("insert fact");
}

#[tokio::test(flavor = "multi_thread")]
async fn agent_session_list_is_forced_to_caller_scope() {
    let (state, jwt, _tmp) = StateBuilder::new().build();
    let syn_session = create_session(&state, "syn", "main").await;
    let _demiurge_session = create_session(&state, "demiurge", "main").await;

    let token = issue_token_with_nous_id(&jwt, "agent-syn", Role::Agent, "syn");
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
                .body(tool_call_request(1, "session_list", &serde_json::json!({})))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let text = extract_tool_text(response)
        .await
        .expect("tool response text");
    assert!(
        text.contains(&syn_session),
        "scoped agent must see its own session"
    );
    assert!(
        !text.contains("demiurge"),
        "scoped agent must not see sibling sessions"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn agent_session_list_rejects_contradictory_nous_id() {
    let (state, jwt, _tmp) = StateBuilder::new().build();
    let _syn_session = create_session(&state, "syn", "main").await;
    let _demiurge_session = create_session(&state, "demiurge", "main").await;

    let token = issue_token_with_nous_id(&jwt, "agent-syn", Role::Agent, "syn");
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
                    "session_list",
                    &serde_json::json!({"nous_id": "demiurge"}),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        extract_error_code(response).await,
        Some(-32001),
        "contradictory nous_id must return MCP unauthorized"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn agent_session_history_rejects_cross_agent_session() {
    let (state, jwt, _tmp) = StateBuilder::new().build();
    let _syn_session = create_session(&state, "syn", "main").await;
    let demiurge_session = create_session(&state, "demiurge", "main").await;

    let token = issue_token_with_nous_id(&jwt, "agent-syn", Role::Agent, "syn");
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
                    "session_history",
                    &serde_json::json!({"session_id": demiurge_session}),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        extract_error_code(response).await,
        Some(-32001),
        "cross-agent session history must return MCP unauthorized"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn scoped_operator_session_create_rejects_contradictory_nous_id() {
    let (state, jwt, _tmp) = StateBuilder::new().build();

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
                .body(tool_call_request(
                    1,
                    "session_create",
                    &serde_json::json!({"nous_id": "demiurge", "session_key": "main"}),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        extract_error_code(response).await,
        Some(-32001),
        "scoped operator must not create sessions for a sibling agent"
    );
}

#[cfg(feature = "knowledge-store")]
#[tokio::test(flavor = "multi_thread")]
async fn agent_knowledge_recall_is_scoped_to_caller_nous() {
    let store = mneme::knowledge_store::KnowledgeStore::open_mem().expect("open memory store");
    insert_test_fact(&store, "syn", "syn secret about Rust ownership");
    insert_test_fact(&store, "demiurge", "demiurge secret about Rust ownership");

    let (state, jwt, _tmp) = StateBuilder::new()
        .knowledge_graph_enabled()
        .knowledge_store(store)
        .build();

    let token = issue_token_with_nous_id(&jwt, "agent-syn", Role::Agent, "syn");
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
                    "knowledge_recall",
                    &serde_json::json!({"query": "Rust ownership"}),
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
        text.contains("syn secret"),
        "scoped recall must return own fact"
    );
    assert!(
        !text.contains("demiurge secret"),
        "scoped recall must not return sibling fact"
    );
}

#[cfg(feature = "knowledge-store")]
#[tokio::test(flavor = "multi_thread")]
async fn agent_knowledge_recall_rejects_contradictory_nous_id() {
    let store = mneme::knowledge_store::KnowledgeStore::open_mem().expect("open memory store");
    insert_test_fact(&store, "demiurge", "demiurge secret about Rust ownership");

    let (state, jwt, _tmp) = StateBuilder::new()
        .knowledge_graph_enabled()
        .knowledge_store(store)
        .build();

    let token = issue_token_with_nous_id(&jwt, "agent-syn", Role::Agent, "syn");
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
                    "knowledge_recall",
                    &serde_json::json!({"query": "Rust ownership", "nous_id": "demiurge"}),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        extract_error_code(response).await,
        Some(-32001),
        "contradictory recall scope must return MCP unauthorized"
    );
}

#[cfg(feature = "knowledge-store")]
#[tokio::test(flavor = "multi_thread")]
async fn agent_knowledge_get_rejects_cross_agent_fact() {
    let store = mneme::knowledge_store::KnowledgeStore::open_mem().expect("open memory store");
    insert_test_fact(&store, "demiurge", "demiurge confidential fact");
    // Find the generated fact id by recalling it.
    let results = store
        .search_text_for_recall("demiurge confidential fact", 10)
        .expect("search");
    let fact_id = results.first().expect("one fact").source_id.clone();

    let (state, jwt, _tmp) = StateBuilder::new()
        .knowledge_graph_enabled()
        .knowledge_store(store)
        .build();

    let token = issue_token_with_nous_id(&jwt, "agent-syn", Role::Agent, "syn");
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
                    "knowledge_get",
                    &serde_json::json!({"fact_id": fact_id}),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        extract_error_code(response).await,
        Some(rmcp::model::ErrorCode::INVALID_PARAMS.0.into()),
        "cross-agent fact get must return invalid params (not found)"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn scoped_operator_knowledge_insert_rejects_contradictory_nous_id() {
    let (state, jwt, _tmp) = StateBuilder::new().knowledge_graph_enabled().build();

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
                .body(tool_call_request(
                    1,
                    "knowledge_insert",
                    &serde_json::json!({
                        "content": "cross-agent fact",
                        "nous_id": "demiurge"
                    }),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        extract_error_code(response).await,
        Some(-32001),
        "scoped operator must not insert facts for a sibling agent"
    );
}

#[cfg(feature = "knowledge-store")]
#[tokio::test(flavor = "multi_thread")]
async fn scoped_operator_knowledge_forget_rejects_cross_agent_fact_id() {
    let store = mneme::knowledge_store::KnowledgeStore::open_mem().expect("open memory store");
    insert_test_fact(&store, "demiurge", "demiurge fact to protect from forget");
    let results = store
        .search_text_for_recall("demiurge fact to protect from forget", 10)
        .expect("search");
    let fact_id = results.first().expect("one fact").source_id.clone();

    let (state, jwt, _tmp) = StateBuilder::new()
        .knowledge_graph_enabled()
        .knowledge_store(store)
        .build();

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
                .body(tool_call_request(
                    1,
                    "knowledge_forget",
                    &serde_json::json!({"fact_id": fact_id}),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        extract_error_code(response).await,
        Some(-32001),
        "scoped operator must not mutate sibling facts by ID"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn agent_memory_note_rejects_cross_session_session_id() {
    let note_store = Arc::new(TokioMutex::new(
        SessionStore::open_in_memory().expect("open store"),
    ));
    let adapter = Arc::new(nous::adapters::SessionNoteAdapter(Arc::clone(&note_store)));

    let (state, jwt, _tmp) = StateBuilder::new().note_store(adapter).build();

    let token = issue_token_with_nous_id(&jwt, "agent-syn", Role::Agent, "syn");
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
                    "memory_note",
                    &serde_json::json!({
                        "session_id": "victim-session",
                        "action": "add",
                        "content": "planted note",
                    }),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        extract_error_code(response).await,
        Some(-32001),
        "cross-session memory_note must return MCP unauthorized"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn agent_memory_blackboard_rejects_cross_agent_author() {
    let bb_store = Arc::new(TokioMutex::new(
        SessionStore::open_in_memory().expect("open store"),
    ));
    let adapter = Arc::new(nous::adapters::SessionBlackboardAdapter(Arc::clone(
        &bb_store,
    )));

    let (state, jwt, _tmp) = StateBuilder::new().blackboard_store(adapter).build();

    let token = issue_token_with_nous_id(&jwt, "agent-syn", Role::Agent, "syn");
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
                    "memory_blackboard",
                    &serde_json::json!({
                        "action": "write",
                        "key": "shared-key",
                        "value": "planted value",
                        "author": "demiurge",
                        "ttl_seconds": 60,
                    }),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        extract_error_code(response).await,
        Some(-32001),
        "cross-agent blackboard author must return MCP unauthorized"
    );
}
