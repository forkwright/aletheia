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
use symbolon::auth::{AuthConfig, AuthFacade};
use symbolon::jwt::{JwtConfig, JwtManager};
use symbolon::types::Role;
use taxis::config::AletheiaConfig;
use taxis::oikos::Oikos;

mod common;
use common::{StateBuilder, issue_token};

#[tokio::test(flavor = "multi_thread")]
async fn router_rejects_missing_authorization_header_in_token_mode() {
    let (state, _jwt, _tmp) = StateBuilder::new().auth_mode("token").build();
    let router = streamable_http_router(state);

    let response = router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(
        response.status(),
        StatusCode::UNAUTHORIZED,
        "POST without Bearer token must be rejected by mcp_auth"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn router_rejects_malformed_bearer_token_in_token_mode() {
    let (state, _jwt, _tmp) = StateBuilder::new().auth_mode("token").build();
    let router = streamable_http_router(state);

    let response = router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header(header::AUTHORIZATION, "Bearer not-a-real-jwt-token")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(
        response.status(),
        StatusCode::UNAUTHORIZED,
        "malformed Bearer token must be rejected"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn router_rejects_missing_bearer_prefix_in_token_mode() {
    let (state, jwt, _tmp) = StateBuilder::new().auth_mode("token").build();
    let token = issue_token(&jwt, "charlie", Role::Operator);
    let router = streamable_http_router(state);

    let response = router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                // WHY: valid token value but missing the "Bearer " prefix must
                // be rejected — the scheme is part of the contract.
                .header(header::AUTHORIZATION, token)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(
        response.status(),
        StatusCode::UNAUTHORIZED,
        "Authorization header without Bearer prefix must be rejected"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn router_rejects_expired_bearer_token() {
    let instance_root = tempfile::tempdir().expect("tempdir");
    let oikos = Arc::new(Oikos::from_root(instance_root.path()));
    let session_store = Arc::new(TokioMutex::new(
        SessionStore::open_in_memory().expect("in-memory session store"),
    ));
    let provider_registry = Arc::new(ProviderRegistry::new());
    let tool_registry = Arc::new(ToolRegistry::new());
    let nous_manager = Arc::new(NousManager::new(
        Arc::clone(&provider_registry),
        Arc::clone(&tool_registry),
        Arc::clone(&oikos),
        None,
        None,
        Some(Arc::clone(&session_store)),
        #[cfg(feature = "knowledge-store")]
        None,
        Arc::new(vec![]),
        None,
        None,
        taxis::config::NousBehaviorConfig::default(),
        taxis::config::ToolLimitsConfig::default(),
    ));

    // WHY: zero access_ttl plus zero clock-skew leeway guarantees that
    // every issued token is already expired by the time validation runs,
    // triggering the expired-token path. The default 30s leeway would
    // otherwise keep the token alive past the 50ms sleep below.
    let jwt_config = JwtConfig {
        signing_key: SecretString::from("expired-token-test-signing-key-bytes!".to_owned()),
        access_ttl: Duration::from_secs(0),
        refresh_ttl: Duration::from_secs(0),
        issuer: "aletheia-diaporeia-tests".to_owned(),
        clock_skew_leeway_secs: 0,
    };
    let jwt_manager = Arc::new(JwtManager::new(jwt_config.clone()));
    let auth_facade = Arc::new(
        AuthFacade::in_memory(AuthConfig { jwt: jwt_config }).expect("in-memory auth facade"),
    );

    let config = Arc::new(RwLock::new(AletheiaConfig::default()));
    let state = Arc::new(DiaporeiaState {
        session_store,
        nous_manager,
        tool_registry,
        oikos,
        auth_facade: Some(auth_facade),
        start_time: Instant::now(),
        config,
        auth_mode: "token".to_owned(),
        none_role: "readonly".to_owned(),
        shutdown: CancellationToken::new(),
        #[cfg(feature = "knowledge-store")]
        knowledge_store: None,
        note_store: None,
        blackboard_store: None,
    });

    let token = issue_token(&jwt_manager, "alice", Role::Admin);
    // Allow the 0-second TTL to elapse in wall-clock time before validation.
    tokio::time::sleep(Duration::from_millis(50)).await;

    let router = streamable_http_router(state);
    let response = router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(
        response.status(),
        StatusCode::UNAUTHORIZED,
        "expired token must be rejected by mcp_auth"
    );
    drop(instance_root);
}

#[tokio::test(flavor = "multi_thread")]
async fn router_rejects_revoked_bearer_token() {
    let (state, jwt, _tmp) = StateBuilder::new().auth_mode("token").build();
    let token = issue_token(&jwt, "alice", Role::Operator);
    state
        .auth_facade
        .as_ref()
        .expect("token-mode state must carry auth facade")
        .revoke(&token)
        .expect("revoke test token");
    let router = streamable_http_router(state);

    let response = router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(
        response.status(),
        StatusCode::UNAUTHORIZED,
        "revoked Bearer token must be rejected by MCP auth"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn router_rejects_token_signed_with_wrong_key() {
    let (state, _state_jwt, _tmp) = StateBuilder::new().auth_mode("token").build();

    // WHY: a fresh JwtManager with a different signing key produces tokens
    // that the state's own validator cannot verify — signature mismatch is
    // the core auth invariant.
    let foreign_jwt = JwtManager::new(JwtConfig {
        signing_key: SecretString::from("a-different-signing-key-never-shared".to_owned()),
        access_ttl: Duration::from_hours(1),
        refresh_ttl: Duration::from_hours(24),
        issuer: "rogue-issuer".to_owned(),
        ..JwtConfig::default()
    });
    let rogue_token = issue_token(&foreign_jwt, "mallory", Role::Admin);

    let router = streamable_http_router(state);
    let response = router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header(header::AUTHORIZATION, format!("Bearer {rogue_token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(
        response.status(),
        StatusCode::UNAUTHORIZED,
        "token signed with foreign key must be rejected"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn router_allows_unauthenticated_requests_in_none_mode() {
    let (state, _jwt, _tmp) = StateBuilder::new()
        .auth_mode("none")
        .none_role("readonly")
        .build();
    let router = streamable_http_router(state);

    // WHY: auth_mode = "none" injects anonymous claims and passes through.
    // Without an `Accept: text/event-stream` header, the downstream MCP
    // service returns 400 Bad Request for GET requests. The 400 proves the
    // middleware passed the request through — a 401 would indicate rejection.
    let response = router
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/mcp")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(
        response.status(),
        StatusCode::BAD_REQUEST,
        "auth_mode=none must pass the request through to the MCP service, \
         which returns 400 for GET without an event-stream Accept header"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn router_rejects_requests_outside_mcp_namespace() {
    let (state, _jwt, _tmp) = StateBuilder::new().auth_mode("none").build();
    let router = streamable_http_router(state);

    // Route scoping: the router only mounts `/mcp`. Other paths must 404.
    let response = router
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/not-mcp")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(
        response.status(),
        StatusCode::NOT_FOUND,
        "paths outside /mcp must return 404"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn router_rejects_delete_without_session_id() {
    let (state, _jwt, _tmp) = StateBuilder::new().auth_mode("none").build();
    let router = streamable_http_router(state);

    // StreamableHttpService's default stateful mode requires a session ID on
    // DELETE. With no `Mcp-Session-Id` header, the downstream service
    // returns 400. This proves the request passed the auth layer in "none"
    // mode and reached the protocol layer.
    let response = router
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri("/mcp")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(
        response.status(),
        StatusCode::BAD_REQUEST,
        "DELETE without session id must be rejected by the MCP service"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn router_auth_rejection_precedes_method_handling() {
    // WHY: the mcp_auth middleware must run before the downstream service
    // sees the request. An unauthenticated DELETE should produce 401, not
    // 405 — confirming ordering of the layer stack.
    let (state, _jwt, _tmp) = StateBuilder::new().auth_mode("token").build();
    let router = streamable_http_router(state);

    let response = router
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri("/mcp")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(
        response.status(),
        StatusCode::UNAUTHORIZED,
        "auth rejection must precede method dispatch"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn router_valid_token_passes_auth_layer_and_reaches_mcp_service() {
    // WHY: a validly-signed Bearer token must clear the auth middleware and
    // reach the downstream MCP service. The service then rejects with 400
    // (missing Accept: text/event-stream + application/json) — proving the
    // request got past auth but failed at protocol negotiation.
    let (state, jwt, _tmp) = StateBuilder::new().auth_mode("token").build();
    let token = issue_token(&jwt, "alice", Role::Operator);
    let router = streamable_http_router(state);

    let response = router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(
        response.status(),
        StatusCode::BAD_REQUEST,
        "valid Bearer token must clear auth and reach the MCP service, \
         which rejects empty POSTs with 400 for missing Accept header"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn router_rejects_bearer_prefix_with_empty_token() {
    // WHY: "Bearer " with no token afterwards must be rejected the same way
    // as a missing token — an empty token is not a valid credential.
    let (state, _jwt, _tmp) = StateBuilder::new().auth_mode("token").build();
    let router = streamable_http_router(state);

    let response = router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header(header::AUTHORIZATION, "Bearer ")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(
        response.status(),
        StatusCode::UNAUTHORIZED,
        "empty token after Bearer prefix must be rejected"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn router_is_cloneable_for_mounting_across_handlers() {
    // axum::Router is Clone. Verify the diaporeia factory returns something
    // we can clone and mount at multiple points without losing behaviour.
    let (state, _jwt, _tmp) = StateBuilder::new().auth_mode("token").build();
    let router_a = streamable_http_router(state);
    let router_b = router_a.clone();

    // Drive each clone independently and verify they both reject unauthenticated.
    let resp_a = router_a
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let resp_b = router_b
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp_a.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(resp_b.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test(flavor = "multi_thread")]
async fn router_in_none_mode_ignores_authorization_header_when_present() {
    // WHY: when auth_mode = "none" the middleware must skip JWT validation
    // entirely and inject anonymous claims, regardless of what the client
    // sends in the Authorization header. A malformed Bearer value would fail
    // in token mode but must be ignored here — the request still reaches the
    // downstream service, which returns 400 for a GET without the expected
    // event-stream Accept header.
    let (state, _jwt, _tmp) = StateBuilder::new().auth_mode("none").build();
    let router = streamable_http_router(state);

    let response = router
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/mcp")
                .header(header::AUTHORIZATION, "Bearer totally-invalid-garbage")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(
        response.status(),
        StatusCode::BAD_REQUEST,
        "auth_mode=none must ignore Authorization and pass through to the \
         MCP service (which returns 400 for GET without Accept: text/event-stream)"
    );
}
