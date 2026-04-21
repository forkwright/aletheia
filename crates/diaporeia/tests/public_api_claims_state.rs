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

use diaporeia::auth::McpClaims;
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

// Split: McpClaims + Error + DiaporeiaState + DiaporeiaServer construction.

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
    assert!(
        debug.contains("bob"),
        "Debug output must contain subject: {debug}"
    );
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
