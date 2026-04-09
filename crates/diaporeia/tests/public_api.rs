//! Integration tests for the `aletheia-diaporeia` public API.
//!
//! These tests exercise diaporeia as an external consumer would: through the
//! publicly re-exported modules (`auth`, `error`, `state`, `server`,
//! `transport`). They do not reach into crate-private items.
//!
//! Coverage targets (issue #2814):
//!
//! 1. `auth::McpClaims` construction, trait implementations, and round-tripping
//!    through the `mcp_auth` middleware.
//! 2. `error::Error` and `error::Result` trait implementations exposed at the
//!    module boundary (`Debug`, `Send`, `Sync`, `From<Error> for rmcp::ErrorData`).
//! 3. `state::DiaporeiaState` construction from real workspace types
//!    (`SessionStore::open_in_memory`, empty `ProviderRegistry`, empty
//!    `ToolRegistry`, `Oikos::from_root`, `JwtManager`).
//! 4. `server::DiaporeiaServer::with_state` construction, `Clone`, and
//!    `Send + Sync` bounds.
//! 5. `transport::streamable_http_router` assembly, auth-layer behaviour for
//!    both `auth_mode = "token"` (Bearer JWT) and `auth_mode = "none"`
//!    (anonymous claims), and HTTP method handling by the downstream MCP
//!    service.

#![expect(
    clippy::unwrap_used,
    reason = "test assertions — panicking on failure is the point"
)]
#![expect(
    clippy::expect_used,
    reason = "test assertions — panicking on failure is the point"
)]

use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use tokio::sync::{Mutex as TokioMutex, RwLock};
use tokio_util::sync::CancellationToken;
use tower::ServiceExt;

use aletheia_diaporeia::auth::McpClaims;
use aletheia_diaporeia::error::{Error, Result as DiaporeiaResult};
use aletheia_diaporeia::server::DiaporeiaServer;
use aletheia_diaporeia::state::DiaporeiaState;
use aletheia_diaporeia::transport::streamable_http_router;

use aletheia_hermeneus::provider::ProviderRegistry;
use aletheia_koina::secret::SecretString;
use aletheia_mneme::store::SessionStore;
use aletheia_nous::manager::NousManager;
use aletheia_organon::registry::ToolRegistry;
use aletheia_symbolon::jwt::{JwtConfig, JwtManager};
use aletheia_symbolon::types::Role;
use aletheia_taxis::config::AletheiaConfig;
use aletheia_taxis::oikos::Oikos;

// WHY: each test must construct an independent state with its own tempdir so
// that tests can run in parallel and in any order. The builder below assembles
// a minimal `DiaporeiaState` with real workspace components (no mocks).
struct StateBuilder {
    auth_mode: String,
    none_role: String,
    signing_key: String,
    instance_root: tempfile::TempDir,
}

impl StateBuilder {
    fn new() -> Self {
        let instance_root = tempfile::tempdir().expect("create instance tempdir");
        Self {
            auth_mode: "token".to_owned(),
            none_role: "readonly".to_owned(),
            signing_key: "integration-test-signing-key-at-least-32-bytes!".to_owned(),
            instance_root,
        }
    }

    fn auth_mode(mut self, mode: &str) -> Self {
        mode.clone_into(&mut self.auth_mode);
        self
    }

    fn none_role(mut self, role: &str) -> Self {
        role.clone_into(&mut self.none_role);
        self
    }

    fn build(self) -> (Arc<DiaporeiaState>, Arc<JwtManager>, tempfile::TempDir) {
        let oikos = Arc::new(Oikos::from_root(self.instance_root.path()));
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
            Arc::new(vec![]),
            None,
            None,
        ));

        let jwt_manager = Arc::new(JwtManager::new(JwtConfig {
            signing_key: SecretString::from(self.signing_key.clone()),
            access_ttl: Duration::from_secs(3600),
            refresh_ttl: Duration::from_secs(86400),
            issuer: "aletheia-diaporeia-tests".to_owned(),
        }));

        let jwt_for_state = if self.auth_mode == "none" {
            None
        } else {
            Some(Arc::clone(&jwt_manager))
        };

        let config = Arc::new(RwLock::new(AletheiaConfig::default()));

        let state = Arc::new(DiaporeiaState {
            session_store,
            nous_manager,
            tool_registry,
            oikos,
            jwt_manager: jwt_for_state,
            start_time: Instant::now(),
            config,
            auth_mode: self.auth_mode,
            none_role: self.none_role,
            shutdown: CancellationToken::new(),
        });

        (state, jwt_manager, self.instance_root)
    }
}

fn issue_token(jwt: &JwtManager, subject: &str, role: Role) -> String {
    jwt.issue_access(subject, role, None)
        .expect("issue test access token")
}

// -------------------------------------------------------------------
// Section 1: McpClaims
// -------------------------------------------------------------------

#[test]
fn mcp_claims_struct_fields_are_publicly_accessible() {
    let claims = McpClaims {
        sub: "alice".to_owned(),
        role: Role::Operator,
        nous_id: Some("syn".to_owned()),
    };

    assert_eq!(claims.sub, "alice");
    assert_eq!(claims.role, Role::Operator);
    assert_eq!(claims.nous_id.as_deref(), Some("syn"));
}

#[test]
fn mcp_claims_allows_none_nous_id_for_unscoped_principals() {
    let claims = McpClaims {
        sub: "admin".to_owned(),
        role: Role::Admin,
        nous_id: None,
    };

    assert!(claims.nous_id.is_none());
    assert_eq!(claims.role, Role::Admin);
}

#[test]
fn mcp_claims_is_clone_debug_send_sync() {
    // Compile-time verification of the trait bounds promised by the public type.
    fn assert_send_sync<T: Send + Sync + Clone + std::fmt::Debug>() {}
    assert_send_sync::<McpClaims>();

    // Runtime verification: clone produces an equal, independently-owned value.
    let original = McpClaims {
        sub: "bob".to_owned(),
        role: Role::Readonly,
        nous_id: Some("charlie".to_owned()),
    };
    let cloned = original.clone();
    assert_eq!(original.sub, cloned.sub);
    assert_eq!(original.role, cloned.role);
    assert_eq!(original.nous_id, cloned.nous_id);

    // Debug formatting must surface the subject so tracing logs are useful.
    let debug = format!("{original:?}");
    assert!(debug.contains("bob"), "Debug output must contain subject: {debug}");
}

#[test]
fn mcp_claims_supports_every_role_variant() {
    // WHY: the RBAC hierarchy is Readonly < Agent < Operator < Admin.
    // McpClaims must accept any variant because the middleware maps whatever
    // the JWT carries directly into the struct. A regression that narrowed
    // the role type would fail this test at compile time.
    for role in [Role::Readonly, Role::Agent, Role::Operator, Role::Admin] {
        let claims = McpClaims {
            sub: format!("subject-{role}"),
            role,
            nous_id: None,
        };
        assert_eq!(claims.role, role);
    }
}

// -------------------------------------------------------------------
// Section 2: Error type
// -------------------------------------------------------------------

#[test]
fn error_type_satisfies_send_sync_std_error() {
    fn assert_traits<T: std::error::Error + Send + Sync + 'static>() {}
    assert_traits::<Error>();
}

#[test]
fn result_alias_refers_to_the_public_error_type() {
    // WHY: the `Result<T>` alias is part of the public `error` module. This
    // test verifies that the alias points at the canonical error type by
    // binding a value to the explicit alias type signature, then chaining
    // through combinators that only resolve when the alias desugars to
    // `std::result::Result<T, diaporeia::error::Error>`.
    //
    // The alias has no constructible `Err` path from outside the crate
    // (snafu builders are `pub(crate)`), so we only exercise `Ok` here.
    // `Error` is already proven `Send + Sync + std::error::Error` in
    // `error_type_satisfies_send_sync_std_error`.
    let value: DiaporeiaResult<&'static str> = Ok("diaporeia");
    let mapped: DiaporeiaResult<usize> = value.map(str::len);
    assert_eq!(mapped.expect("mapped length"), "diaporeia".len());

    // Explicit pattern match on the alias to ensure the Ok arm is reachable.
    let pinned: DiaporeiaResult<i32> = Ok(17);
    match pinned {
        Ok(n) => assert_eq!(n, 17),
        Err(e) => panic!("alias must carry our Error type: {e}"),
    }
}

// -------------------------------------------------------------------
// Section 3: DiaporeiaState construction
// -------------------------------------------------------------------

#[test]
fn state_constructs_from_real_workspace_dependencies() {
    let (state, _jwt, _tmp) = StateBuilder::new().build();

    assert_eq!(state.auth_mode, "token");
    assert!(state.jwt_manager.is_some());
    assert_eq!(state.none_role, "readonly");
    assert!(
        state.start_time.elapsed() < Duration::from_secs(5),
        "start_time should be close to now"
    );
    assert!(!state.shutdown.is_cancelled());
}

#[test]
fn state_omits_jwt_manager_when_auth_mode_is_none() {
    let (state, _jwt, _tmp) = StateBuilder::new()
        .auth_mode("none")
        .none_role("admin")
        .build();

    assert_eq!(state.auth_mode, "none");
    assert!(
        state.jwt_manager.is_none(),
        "auth_mode=none must not carry a signing manager"
    );
    assert_eq!(state.none_role, "admin");
}

#[test]
fn state_is_send_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<DiaporeiaState>();
    assert_send_sync::<Arc<DiaporeiaState>>();
}

#[test]
fn state_shutdown_token_propagates_cancellation() {
    let (state, _jwt, _tmp) = StateBuilder::new().build();
    assert!(!state.shutdown.is_cancelled());

    // Cancel via the shared token and observe the effect on the state's view.
    state.shutdown.cancel();
    assert!(state.shutdown.is_cancelled());
}

// -------------------------------------------------------------------
// Section 4: DiaporeiaServer
// -------------------------------------------------------------------

#[test]
fn server_constructs_from_state() {
    let (state, _jwt, _tmp) = StateBuilder::new().build();

    // WHY: `with_state` performs a blocking read of the config RwLock. Running
    // it in a plain `#[test]` (no tokio runtime entered) avoids the
    // "Cannot block the current thread from within a runtime" panic.
    let server = DiaporeiaServer::with_state(Arc::clone(&state));

    // Cloning the server must produce an independent handle that shares state.
    let _clone = server.clone();
}

#[test]
fn server_is_send_sync_and_clone() {
    fn assert_send_sync<T: Send + Sync + Clone>() {}
    assert_send_sync::<DiaporeiaServer>();
}

#[test]
fn multiple_servers_share_same_state() {
    // WHY: pylon mounts its own DiaporeiaServer and any test/tooling may
    // spawn another from the same state Arc. The with_state contract allows
    // multiple servers to coexist over shared state without construction
    // side effects — each snapshots config once for its own rate limiter,
    // but they all share session store, nous manager, and shutdown token.
    let (state, _jwt, _tmp) = StateBuilder::new().build();
    let initial_strong = Arc::strong_count(&state);

    let server_a = DiaporeiaServer::with_state(Arc::clone(&state));
    let server_b = DiaporeiaServer::with_state(Arc::clone(&state));

    // Both servers hold strong references to the shared state.
    assert!(
        Arc::strong_count(&state) >= initial_strong + 2,
        "each server must retain a strong state reference"
    );

    drop(server_a);
    drop(server_b);

    // After dropping both servers, the strong count returns to the original
    // — server construction does not leak state references.
    assert_eq!(
        Arc::strong_count(&state),
        initial_strong,
        "dropping servers must release their state references"
    );
}

#[test]
fn server_construction_snapshots_config_independently_of_later_mutations() {
    // WHY: `with_state` reads the config RwLock once at construction time to
    // build its rate limiter. Later config mutations via the shared RwLock
    // must not panic or deadlock an already-constructed server — the server
    // owns its own rate limiter after construction.
    let (state, _jwt, _tmp) = StateBuilder::new().build();
    let server = DiaporeiaServer::with_state(Arc::clone(&state));

    // Mutate the shared config after construction. This must not panic or
    // affect the live server's behaviour.
    {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async {
            let mut cfg = state.config.write().await;
            cfg.mcp.rate_limit.enabled = false;
            cfg.mcp.rate_limit.message_requests_per_minute = 1;
        });
    }

    // The server remains alive and cloneable — no poisoning from the mutation.
    let _clone = server.clone();
}

// -------------------------------------------------------------------
// Section 5: streamable_http_router + mcp_auth middleware behaviour
// -------------------------------------------------------------------

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
        Arc::new(vec![]),
        None,
        None,
    ));

    // WHY: zero access_ttl guarantees that every issued token is already
    // expired by the time validation runs, triggering the expired-token path.
    let jwt_manager = Arc::new(JwtManager::new(JwtConfig {
        signing_key: SecretString::from("expired-token-test-signing-key-bytes!".to_owned()),
        access_ttl: Duration::from_secs(0),
        refresh_ttl: Duration::from_secs(0),
        issuer: "aletheia-diaporeia-tests".to_owned(),
    }));

    let config = Arc::new(RwLock::new(AletheiaConfig::default()));
    let state = Arc::new(DiaporeiaState {
        session_store,
        nous_manager,
        tool_registry,
        oikos,
        jwt_manager: Some(Arc::clone(&jwt_manager)),
        start_time: Instant::now(),
        config,
        auth_mode: "token".to_owned(),
        none_role: "readonly".to_owned(),
        shutdown: CancellationToken::new(),
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
async fn router_rejects_token_signed_with_wrong_key() {
    let (state, _state_jwt, _tmp) = StateBuilder::new().auth_mode("token").build();

    // WHY: a fresh JwtManager with a different signing key produces tokens
    // that the state's own validator cannot verify — signature mismatch is
    // the core auth invariant.
    let foreign_jwt = JwtManager::new(JwtConfig {
        signing_key: SecretString::from("a-different-signing-key-never-shared".to_owned()),
        access_ttl: Duration::from_secs(3600),
        refresh_ttl: Duration::from_secs(86400),
        issuer: "rogue-issuer".to_owned(),
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
    // service returns 406 Not Acceptable for GET requests. The 406 proves the
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
        StatusCode::NOT_ACCEPTABLE,
        "auth_mode=none must pass the request through to the MCP service, \
         which returns 406 for GET without an event-stream Accept header"
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
    // reach the downstream MCP service. The service then rejects with 406
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
        StatusCode::NOT_ACCEPTABLE,
        "valid Bearer token must clear auth and reach the MCP service, \
         which rejects empty POSTs with 406 for missing Accept header"
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
    // downstream service, which returns 406 for a GET without the expected
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
        StatusCode::NOT_ACCEPTABLE,
        "auth_mode=none must ignore Authorization and pass through to the \
         MCP service (which returns 406 for GET without Accept: text/event-stream)"
    );
}
