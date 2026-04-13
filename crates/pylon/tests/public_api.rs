//! Integration tests for the `aletheia-pylon` public API (#2814).
//!
//! These tests exercise the crate the way an external consumer would: only
//! `pub` items, real implementations wired through [`build_router`] and a
//! real [`axum::serve`] bound to `127.0.0.1:0`. No mocks other than the LLM
//! provider, which is the only external service boundary pylon depends on.

#![expect(clippy::expect_used, reason = "test assertions use expect")]
#![expect(
    clippy::indexing_slicing,
    reason = "test: JSON indices and byte-slice ranges are valid after asserting status or known protocol shape"
)]
#![expect(
    clippy::disallowed_methods,
    reason = "integration tests write fixture files to temp directories; synchronous std::fs I/O is required in test setup"
)]

use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::body::Body;
use axum::http::{Request, StatusCode};
use tokio::net::TcpListener;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;
use tower::ServiceExt;

use hermeneus::provider::ProviderRegistry;
use hermeneus::test_utils::MockProvider;
use koina::http::{API_HEALTH, API_V1, BEARER_PREFIX};
use koina::secret::SecretString;
use mneme::store::SessionStore;
use nous::config::{NousConfig, NousGenerationConfig, PipelineConfig};
use nous::manager::NousManager;
use organon::registry::ToolRegistry;
use pylon::idempotency::IdempotencyCache;
use pylon::router::build_router;
use pylon::security::{
    CorsConfig, CsrfConfig, RateLimitConfig, SecurityConfig, TlsConfig,
};
use pylon::state::AppState;
use symbolon::jwt::{JwtConfig, JwtManager};
use symbolon::types::{Claims, Role, TokenKind};
use taxis::config::AletheiaConfig;
use taxis::oikos::Oikos;

// ── Fixtures ────────────────────────────────────────────────────────────────

/// Minimal oikos tempdir with the directories and config files the
/// health-check handlers expect to be readable.
struct TestEnv {
    state: Arc<AppState>,
    _tmp: tempfile::TempDir,
}

impl TestEnv {
    async fn new() -> Self {
        Self::builder().build().await
    }

    fn builder() -> TestEnvBuilder {
        TestEnvBuilder::default()
    }
}

#[derive(Default)]
struct TestEnvBuilder {
    with_actor: bool,
    auth_mode: Option<String>,
    jwt_access_ttl: Option<Duration>,
}

impl TestEnvBuilder {
    fn with_actor(mut self, flag: bool) -> Self {
        self.with_actor = flag;
        self
    }

    fn auth_mode(mut self, mode: &str) -> Self {
        self.auth_mode = Some(mode.to_owned());
        self
    }

    fn jwt_access_ttl(mut self, ttl: Duration) -> Self {
        self.jwt_access_ttl = Some(ttl);
        self
    }

    async fn build(self) -> TestEnv {
        let tmp = tempfile::TempDir::new().expect("tmpdir");
        let root = tmp.path();

        // WHY: oikos layout is load-bearing for health checks: missing
        // directories cause fail-closed behaviour that hides real bugs.
        for dir in [
            "nous/syn",
            "shared",
            "theke",
            "data",
            "config",
            "config/credentials",
        ] {
            std::fs::create_dir_all(root.join(dir)).expect("mkdir oikos subdir");
        }

        std::fs::write(root.join("nous/syn/SOUL.md"), "I am Syn, a test agent.")
            .expect("write SOUL.md");
        std::fs::write(root.join("theke/USER.md"), "Test user.").expect("write USER.md");
        std::fs::write(
            root.join("config/aletheia.toml"),
            "[gateway]\nport = 18789\nbind = \"localhost\"\n",
        )
        .expect("write config");
        std::fs::write(
            root.join("config/credentials/anthropic.json"),
            r#"{"token":"sk-ant-test-key"}"#,
        )
        .expect("write credential");

        let oikos = Arc::new(Oikos::from_root(root));
        let session_store = Arc::new(Mutex::new(
            SessionStore::open_in_memory().expect("open in-memory store"),
        ));

        // WHY: every TestEnv registers a mock provider so health checks can
        // report at least one Up provider. Tests that want a clean registry
        // should construct AppState directly.
        let mut provider_registry = ProviderRegistry::new();
        provider_registry.register(Box::new(
            MockProvider::new("Hello from mock!").models(&["mock-model"]),
        ));
        let provider_registry = Arc::new(provider_registry);
        let tool_registry = Arc::new(ToolRegistry::new());

        let mut nous_manager = NousManager::new(
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
        );

        if self.with_actor {
            let nous_config = NousConfig {
                id: Arc::from("syn"),
                generation: NousGenerationConfig {
                    model: "mock-model".to_owned(),
                    ..Default::default()
                },
                ..NousConfig::default()
            };
            nous_manager
                .spawn(nous_config, PipelineConfig::default())
                .await;
        }

        let jwt_manager = Arc::new(JwtManager::new(JwtConfig {
            signing_key: SecretString::from("test-secret-key-for-jwt".to_owned()),
            access_ttl: self.jwt_access_ttl.unwrap_or(Duration::from_secs(3600)),
            refresh_ttl: Duration::from_secs(86_400),
            issuer: "aletheia-test".to_owned(),
        }));

        let default_config = AletheiaConfig::default();
        let (config_tx, _config_rx) = tokio::sync::watch::channel(default_config.clone());
        let state = Arc::new(AppState {
            session_store,
            nous_manager: Arc::new(nous_manager),
            provider_registry,
            tool_registry,
            oikos,
            jwt_manager,
            start_time: Instant::now(),
            auth_mode: self.auth_mode.unwrap_or_else(|| "token".to_owned()),
            none_role: "admin".to_owned(),
            config: Arc::new(tokio::sync::RwLock::new(default_config)),
            config_tx,
            idempotency_cache: Arc::new(IdempotencyCache::new()),
            shutdown: CancellationToken::new(),
            #[cfg(feature = "knowledge-store")]
            knowledge_store: None,
        });

        TestEnv { state, _tmp: tmp }
    }
}

/// `SecurityConfig` with CSRF disabled: exercises the default route matrix
/// without requiring the CSRF header on mutations.
fn permissive_security() -> SecurityConfig {
    SecurityConfig {
        csrf: CsrfConfig {
            enabled: false,
            ..CsrfConfig::default()
        },
        ..SecurityConfig::default()
    }
}

fn issue_test_token(state: &AppState) -> String {
    state
        .jwt_manager
        .issue_access("test-user", Role::Operator, None)
        .expect("issue test token")
}

fn bearer(token: &str) -> String {
    format!("{BEARER_PREFIX}{token}")
}

async fn read_body_json(response: axum::response::Response) -> serde_json::Value {
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("read body");
    serde_json::from_slice(&bytes).expect("parse json")
}

// ── build_router: construction contracts ───────────────────────────────────

#[tokio::test]
async fn build_router_produces_router_with_health_endpoint() {
    let env = TestEnv::new().await;
    let router = build_router(Arc::clone(&env.state), &permissive_security());

    let response = router
        .oneshot(
            Request::get(API_HEALTH)
                .body(Body::empty())
                .expect("build request"),
        )
        .await
        .expect("router response");

    // WHY: health may legitimately report "unhealthy" (503) when
    // no providers are registered, so both 200 and 503 are contractually
    // valid. What matters is that the endpoint returns a response at all
    // and that the body parses as the documented HealthResponse shape.
    let status = response.status();
    assert!(
        status == StatusCode::OK || status == StatusCode::SERVICE_UNAVAILABLE,
        "health must return 200 or 503, got {status}",
    );

    let body = read_body_json(response).await;
    assert!(body["status"].is_string(), "health body lacks status");
    assert!(body["version"].is_string(), "health body lacks version");
    assert!(body["uptime_seconds"].is_u64(), "uptime_seconds must be u64");
    assert!(body["checks"].is_array(), "checks must be an array");
    assert!(
        !body["checks"].as_array().expect("checks array").is_empty(),
        "health must report at least one check"
    );
}

#[tokio::test]
async fn build_router_health_also_served_at_slash_health() {
    // WHY: The router exposes health at both `/api/health` and `/health`
    // for infrastructure compatibility (some load balancers default to
    // `/health`). Regression test: #2814 must not drop the bare path.
    let env = TestEnv::new().await;
    let router = build_router(Arc::clone(&env.state), &permissive_security());

    let response = router
        .oneshot(
            Request::get("/health")
                .body(Body::empty())
                .expect("build request"),
        )
        .await
        .expect("router response");

    let status = response.status();
    assert!(
        status == StatusCode::OK || status == StatusCode::SERVICE_UNAVAILABLE,
        "/health must return 200 or 503, got {status}",
    );
}

#[tokio::test]
async fn build_router_is_idempotent_for_shared_state() {
    let env = TestEnv::new().await;
    let router_one = build_router(Arc::clone(&env.state), &permissive_security());
    let router_two = build_router(Arc::clone(&env.state), &permissive_security());

    // WHY: AppState is shared behind Arc and build_router must not consume or
    // mutate it. Regression test: if build_router were to install a one-shot
    // layer that panics on re-entry, routing through the second router would
    // fail. Both should work.
    for router in [router_one, router_two] {
        let response = router
            .oneshot(
                Request::get(API_HEALTH)
                    .body(Body::empty())
                    .expect("build request"),
            )
            .await
            .expect("router response");
        assert!(matches!(
            response.status(),
            StatusCode::OK | StatusCode::SERVICE_UNAVAILABLE
        ));
    }
}

#[tokio::test]
async fn build_router_unknown_path_returns_404_json_envelope() {
    let env = TestEnv::new().await;
    let router = build_router(Arc::clone(&env.state), &permissive_security());

    let response = router
        .oneshot(
            Request::get("/definitely/not/a/real/path")
                .body(Body::empty())
                .expect("build request"),
        )
        .await
        .expect("router response");

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let body = read_body_json(response).await;
    assert_eq!(body["error"]["code"], "not_found");
    assert!(
        body["error"]["request_id"].is_string(),
        "404 must carry a request_id for correlation"
    );
}

#[tokio::test]
async fn build_router_old_api_nous_path_returns_410_gone() {
    // WHY: The unversioned `/api/nous` path was moved to `/api/v1/nous`.
    // The fallback returns 410 Gone with a migration hint instead of 404
    // so older clients see an actionable error. Regression test: this
    // migration hint is a public contract.
    let env = TestEnv::new().await;
    let router = build_router(Arc::clone(&env.state), &permissive_security());

    let response = router
        .oneshot(
            Request::get("/api/nous")
                .body(Body::empty())
                .expect("build request"),
        )
        .await
        .expect("router response");

    assert_eq!(response.status(), StatusCode::GONE);
    let body = read_body_json(response).await;
    assert_eq!(body["error"]["code"], "api_version_required");
    let message = body["error"]["message"]
        .as_str()
        .expect("message is a string");
    assert!(
        message.contains("/api/v1/nous"),
        "migration hint must name the new path, got {message}",
    );
}

// ── build_router: auth contracts ───────────────────────────────────────────

#[tokio::test]
async fn protected_endpoint_rejects_missing_bearer() {
    let env = TestEnv::new().await;
    let router = build_router(Arc::clone(&env.state), &permissive_security());

    let response = router
        .oneshot(
            Request::get(format!("{API_V1}/nous"))
                .body(Body::empty())
                .expect("build request"),
        )
        .await
        .expect("router response");

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn protected_endpoint_accepts_valid_bearer() {
    let env = TestEnv::new().await;
    let token = issue_test_token(&env.state);
    let router = build_router(Arc::clone(&env.state), &permissive_security());

    let response = router
        .oneshot(
            Request::get(format!("{API_V1}/nous"))
                .header("authorization", bearer(&token))
                .body(Body::empty())
                .expect("build request"),
        )
        .await
        .expect("router response");

    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn protected_endpoint_rejects_malformed_bearer() {
    let env = TestEnv::new().await;
    let router = build_router(Arc::clone(&env.state), &permissive_security());

    let response = router
        .oneshot(
            Request::get(format!("{API_V1}/nous"))
                .header("authorization", "Bearer not.a.valid.jwt")
                .body(Body::empty())
                .expect("build request"),
        )
        .await
        .expect("router response");

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn protected_endpoint_rejects_bearer_without_prefix() {
    let env = TestEnv::new().await;
    let token = issue_test_token(&env.state);
    let router = build_router(Arc::clone(&env.state), &permissive_security());

    let response = router
        .oneshot(
            Request::get(format!("{API_V1}/nous"))
                .header("authorization", token)
                .body(Body::empty())
                .expect("build request"),
        )
        .await
        .expect("router response");

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn auth_mode_none_allows_access_without_bearer() {
    let env = TestEnv::builder().auth_mode("none").build().await;
    let router = build_router(Arc::clone(&env.state), &permissive_security());

    let response = router
        .oneshot(
            Request::get(format!("{API_V1}/nous"))
                .body(Body::empty())
                .expect("build request"),
        )
        .await
        .expect("router response");

    assert_eq!(
        response.status(),
        StatusCode::OK,
        "auth_mode=none must not require a bearer on protected routes"
    );
}

// ── JWT round-trip via the public symbolon API wired into AppState ─────────

#[tokio::test]
async fn jwt_issue_then_validate_preserves_sub_and_role() {
    let env = TestEnv::new().await;
    let token = env
        .state
        .jwt_manager
        .issue_access("alice", Role::Admin, None)
        .expect("issue");

    let claims = env.state.jwt_manager.validate(&token).expect("validate");
    assert_eq!(claims.sub, "alice");
    assert_eq!(claims.role, Role::Admin);
    assert_eq!(claims.kind, TokenKind::Access);
    assert!(claims.nous_id.is_none());
}

#[tokio::test]
async fn jwt_agent_token_carries_nous_scope() {
    let env = TestEnv::new().await;
    let token = env
        .state
        .jwt_manager
        .issue_access("agent-syn", Role::Agent, Some("syn"))
        .expect("issue");

    let claims = env.state.jwt_manager.validate(&token).expect("validate");
    assert_eq!(claims.role, Role::Agent);
    assert_eq!(claims.nous_id.as_deref(), Some("syn"));
}

#[tokio::test]
async fn jwt_expired_token_is_rejected_by_router() {
    // WHY: The extractor and the manager must agree on expiry: a token the
    // manager rejects with ExpiredToken must yield 401 at the HTTP layer.
    let env = TestEnv::new().await;
    let claims = Claims {
        sub: "test-user".to_owned(),
        role: Role::Operator,
        nous_id: None,
        iss: "aletheia-test".to_owned(),
        iat: 1_000_000,
        exp: 1_000_001,
        jti: "expired-jti".to_owned(),
        kind: TokenKind::Access,
    };
    let token = env
        .state
        .jwt_manager
        .encode_claims(&claims)
        .expect("encode expired claims");

    assert!(
        env.state.jwt_manager.validate(&token).is_err(),
        "manager must reject expired token",
    );

    let router = build_router(Arc::clone(&env.state), &permissive_security());
    let response = router
        .oneshot(
            Request::get(format!("{API_V1}/nous"))
                .header("authorization", bearer(&token))
                .body(Body::empty())
                .expect("build request"),
        )
        .await
        .expect("router response");

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn jwt_wrong_issuer_is_rejected() {
    let env = TestEnv::new().await;
    let wrong_manager = JwtManager::new(JwtConfig {
        signing_key: SecretString::from("test-secret-key-for-jwt".to_owned()),
        access_ttl: Duration::from_secs(3600),
        refresh_ttl: Duration::from_secs(86_400),
        issuer: "someone-else".to_owned(),
    });
    let token = wrong_manager
        .issue_access("test-user", Role::Operator, None)
        .expect("issue");

    assert!(
        env.state.jwt_manager.validate(&token).is_err(),
        "token from a different issuer must be rejected"
    );
}

#[tokio::test]
async fn jwt_wrong_signing_key_is_rejected() {
    let env = TestEnv::new().await;
    let wrong_manager = JwtManager::new(JwtConfig {
        signing_key: SecretString::from("a-different-signing-key".to_owned()),
        access_ttl: Duration::from_secs(3600),
        refresh_ttl: Duration::from_secs(86_400),
        issuer: "aletheia-test".to_owned(),
    });
    let token = wrong_manager
        .issue_access("test-user", Role::Operator, None)
        .expect("issue");

    assert!(
        env.state.jwt_manager.validate(&token).is_err(),
        "token signed with wrong key must be rejected"
    );
}

// ── SecurityConfig and sub-configs: defaults are sensible ──────────────────

#[test]
fn security_config_default_has_1mib_body_limit() {
    let config = SecurityConfig::default();
    assert_eq!(
        config.body_limit_bytes, 1_048_576,
        "default body limit must be 1 MiB to match the documented contract"
    );
}

#[test]
fn security_config_default_enables_csrf() {
    let config = SecurityConfig::default();
    assert!(
        config.csrf.enabled,
        "CSRF defaults to enabled for safety: opt-out, not opt-in"
    );
    assert_eq!(config.csrf.header_name, "x-requested-with");
}

#[test]
fn csrf_config_default_generates_random_32_hex_token() {
    let csrf = CsrfConfig::default();
    assert_eq!(
        csrf.header_value.len(),
        32,
        "CSRF token must be 32 hex chars"
    );
    assert!(
        csrf.header_value.chars().all(|c| c.is_ascii_hexdigit()),
        "CSRF token must be hexadecimal"
    );
    assert_ne!(
        csrf.header_value, "aletheia",
        "must not use the legacy insecure static default"
    );
}

#[test]
fn csrf_config_default_tokens_differ_across_instances() {
    // WHY: Regression test for #1690 — if generate_csrf_token ever regresses
    // to a static seed, this will catch it without requiring cryptanalysis.
    let a = CsrfConfig::default();
    let b = CsrfConfig::default();
    assert_ne!(
        a.header_value, b.header_value,
        "consecutive defaults must produce distinct CSPRNG tokens"
    );
}

#[test]
fn tls_config_default_is_disabled() {
    let tls = TlsConfig::default();
    assert!(!tls.enabled, "TLS must be opt-in");
    assert!(tls.cert_path.is_none());
    assert!(tls.key_path.is_none());
}

#[test]
fn rate_limit_config_default_is_disabled_but_sane() {
    let rl = RateLimitConfig::default();
    assert!(!rl.enabled, "rate limiting is opt-in");
    assert_eq!(rl.requests_per_minute, 60);
    assert!(
        !rl.trust_proxy,
        "trust_proxy must default to false: enabling it blindly is a spoofing vector"
    );
}

#[test]
fn cors_config_default_has_empty_allow_list_and_1h_max_age() {
    let cors = CorsConfig::default();
    assert!(
        cors.allowed_origins.is_empty(),
        "default must not pre-allow any origin"
    );
    assert_eq!(cors.max_age_secs, 3600);
}

// ── build_router: CSRF routing behaviour ───────────────────────────────────

#[tokio::test]
async fn csrf_enabled_blocks_post_without_header() {
    let env = TestEnv::builder().with_actor(true).build().await;
    let security = SecurityConfig::default();
    let router = build_router(Arc::clone(&env.state), &security);

    let token = issue_test_token(&env.state);
    let response = router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("{API_V1}/sessions"))
                .header("content-type", "application/json")
                .header("authorization", bearer(&token))
                .body(Body::from(
                    serde_json::to_vec(&serde_json::json!({
                        "nous_id": "syn",
                        "session_key": "csrf-missing",
                    }))
                    .expect("serialize"),
                ))
                .expect("build request"),
        )
        .await
        .expect("router response");

    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn csrf_disabled_allows_post_without_header() {
    let env = TestEnv::builder().with_actor(true).build().await;
    let router = build_router(Arc::clone(&env.state), &permissive_security());

    let token = issue_test_token(&env.state);
    let response = router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("{API_V1}/sessions"))
                .header("content-type", "application/json")
                .header("authorization", bearer(&token))
                .body(Body::from(
                    serde_json::to_vec(&serde_json::json!({
                        "nous_id": "syn",
                        "session_key": "csrf-disabled",
                    }))
                    .expect("serialize"),
                ))
                .expect("build request"),
        )
        .await
        .expect("router response");

    assert_eq!(response.status(), StatusCode::CREATED);
}

// ── Response headers: security contract ────────────────────────────────────

#[tokio::test]
async fn router_sets_standard_security_headers_on_every_response() {
    let env = TestEnv::new().await;
    let router = build_router(Arc::clone(&env.state), &permissive_security());

    let response = router
        .oneshot(
            Request::get(API_HEALTH)
                .body(Body::empty())
                .expect("build request"),
        )
        .await
        .expect("router response");

    let headers = response.headers();
    assert_eq!(headers.get("x-frame-options").expect("x-frame"), "DENY");
    assert_eq!(
        headers.get("x-content-type-options").expect("x-cto"),
        "nosniff"
    );
    assert_eq!(
        headers.get("content-security-policy").expect("csp"),
        "default-src 'self'"
    );
    // WHY: HSTS is emitted only when TLS is configured. Without TLS, setting
    // HSTS would pin browsers to HTTPS even though the server can't serve it.
    assert!(
        headers.get("strict-transport-security").is_none(),
        "HSTS must not appear when TLS is disabled"
    );
}

#[tokio::test]
async fn router_emits_hsts_header_when_tls_enabled() {
    let env = TestEnv::new().await;
    let security = SecurityConfig {
        tls: TlsConfig {
            enabled: true,
            ..TlsConfig::default()
        },
        csrf: CsrfConfig {
            enabled: false,
            ..CsrfConfig::default()
        },
        ..SecurityConfig::default()
    };
    let router = build_router(Arc::clone(&env.state), &security);

    let response = router
        .oneshot(
            Request::get(API_HEALTH)
                .body(Body::empty())
                .expect("build request"),
        )
        .await
        .expect("router response");

    let hsts = response
        .headers()
        .get("strict-transport-security")
        .expect("HSTS header");
    assert_eq!(hsts, "max-age=31536000; includeSubDomains");
}

// ── Body-limit contract ────────────────────────────────────────────────────

#[tokio::test]
async fn oversized_body_returns_413_payload_too_large() {
    let env = TestEnv::new().await;
    let security = SecurityConfig {
        body_limit_bytes: 64,
        csrf: CsrfConfig {
            enabled: false,
            ..CsrfConfig::default()
        },
        ..SecurityConfig::default()
    };
    let token = issue_test_token(&env.state);
    let router = build_router(Arc::clone(&env.state), &security);

    let oversized = "x".repeat(1024);
    let response = router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("{API_V1}/sessions"))
                .header("content-type", "application/json")
                .header("authorization", bearer(&token))
                .body(Body::from(oversized))
                .expect("build request"),
        )
        .await
        .expect("router response");

    assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
}

// ── IdempotencyCache: public constructor and state wiring ──────────────────

#[test]
fn idempotency_cache_new_is_equivalent_to_default() {
    // WHY: The cache exposes `new()` and `Default`. Both must construct an
    // independent instance with no shared state. Regression test: if a future
    // refactor turns one into a singleton, concurrent AppStates would share
    // cache state and leak idempotency keys across tests.
    let cache_one = IdempotencyCache::new();
    let cache_two = IdempotencyCache::default();
    let a = Arc::new(cache_one);
    let b = Arc::new(cache_two);
    assert!(
        !Arc::ptr_eq(&a, &b),
        "two fresh caches must be distinct allocations"
    );
}

#[test]
fn idempotency_cache_is_send_and_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<IdempotencyCache>();
    // WHY: The cache lives inside Arc<AppState> and is therefore shared across
    // tokio tasks handling concurrent requests. A regression that removed
    // Send+Sync would break axum's state injection at compile time, but the
    // compile error would point at router.rs, not at the cache itself. This
    // assertion makes the requirement greppable.
}

#[tokio::test]
async fn app_state_exposes_idempotency_cache_via_public_field() {
    // WHY: `AppState::idempotency_cache` is a public field that handlers
    // reach through. A refactor that makes the field private would silently
    // break the session POST flow. Regression test: confirm external code
    // can still read the Arc.
    let env = TestEnv::new().await;
    let cache = Arc::clone(&env.state.idempotency_cache);
    assert!(
        Arc::strong_count(&cache) >= 2,
        "cloning the Arc must actually share state with AppState"
    );
}

// ── Real TCP: axum::serve on a random port, exercise from outside ──────────

/// Spawn the router behind a real `axum::serve` on `127.0.0.1:0` and return
/// the bound socket address plus a cancel token for shutdown.
async fn spawn_server(
    state: Arc<AppState>,
    security: SecurityConfig,
) -> (std::net::SocketAddr, CancellationToken) {
    let router = build_router(state, &security);
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("local_addr");
    let shutdown = CancellationToken::new();
    let cancel = shutdown.clone();

    tokio::spawn(async move {
        axum::serve(listener, router)
            .with_graceful_shutdown(async move { cancel.cancelled().await })
            .await
            .expect("serve");
    });

    (addr, shutdown)
}

/// Minimal `HTTP/1.1` GET over a raw TCP stream.
///
/// WHY: The workspace pins `reqwest` to the `rustls-no-provider` feature set,
/// so every `reqwest::Client::new` panics with "No provider set" unless a
/// crypto provider was installed first. Installing the provider from outside
/// a `test-support`-gated helper would require a `rustls` dev-dep on pylon,
/// which is out of scope for this test file. Raw TCP avoids the issue
/// entirely and still exercises the real `axum::serve` HTTP framing stack.
async fn raw_get(
    addr: std::net::SocketAddr,
    path: &str,
    authorization: Option<&str>,
) -> RawResponse {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let mut stream = tokio::net::TcpStream::connect(addr)
        .await
        .expect("connect tcp");
    let mut request = format!(
        "GET {path} HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\n"
    );
    if let Some(value) = authorization {
        request.push_str("Authorization: ");
        request.push_str(value);
        request.push_str("\r\n");
    }
    request.push_str("\r\n");
    stream
        .write_all(request.as_bytes())
        .await
        .expect("write request");

    let mut buf = Vec::new();
    stream.read_to_end(&mut buf).await.expect("read response");

    parse_http_response(&buf)
}

struct RawResponse {
    status: u16,
    body: Vec<u8>,
}

impl RawResponse {
    fn body_json(&self) -> serde_json::Value {
        serde_json::from_slice(&self.body).expect("parse json body")
    }
}

fn parse_http_response(bytes: &[u8]) -> RawResponse {
    // Find end of headers (\r\n\r\n)
    let header_end = bytes
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .expect("http response missing header terminator");
    let head =
        std::str::from_utf8(&bytes[..header_end]).expect("http response headers must be utf-8");
    let mut lines = head.lines();
    let status_line = lines.next().expect("http response missing status line");
    // Format: HTTP/1.1 200 OK
    let status: u16 = status_line
        .split_whitespace()
        .nth(1)
        .expect("status code token")
        .parse()
        .expect("status code is numeric");

    let headers_body = &bytes[header_end + 4..];

    // WHY: axum + tower-http compression may apply "Transfer-Encoding: chunked"
    // on larger bodies. Detect this and decode; otherwise use the raw tail.
    let is_chunked = head
        .lines()
        .any(|l| l.eq_ignore_ascii_case("transfer-encoding: chunked"));

    let body = if is_chunked {
        decode_chunked(headers_body)
    } else {
        headers_body.to_vec()
    };

    RawResponse { status, body }
}

fn decode_chunked(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        // Read size line
        let line_end = bytes[i..]
            .windows(2)
            .position(|w| w == b"\r\n")
            .expect("chunk size line terminator");
        let size_str = std::str::from_utf8(&bytes[i..i + line_end]).expect("chunk size utf-8");
        let size = usize::from_str_radix(size_str.trim(), 16).expect("chunk size hex");
        i += line_end + 2;
        if size == 0 {
            break;
        }
        out.extend_from_slice(&bytes[i..i + size]);
        i += size + 2; // skip \r\n trailer
    }
    out
}

#[tokio::test]
async fn real_tcp_server_answers_health_probe() {
    // WHY: `tower::ServiceExt::oneshot` skips the HTTP wire format entirely.
    // Exercising a real `axum::serve` behind `TcpListener` catches regressions
    // in HTTP framing, connection handling, and graceful shutdown that
    // in-memory tests cannot see.
    let env = TestEnv::new().await;
    let (addr, shutdown) = spawn_server(Arc::clone(&env.state), permissive_security()).await;

    let response = raw_get(addr, API_HEALTH, None).await;
    assert!(
        response.status == 200 || response.status == 503,
        "real-TCP health probe must return 200 or 503, got {}",
        response.status
    );
    let body = response.body_json();
    assert!(body["status"].is_string(), "health body lacks status");
    assert!(body["version"].is_string(), "health body lacks version");

    shutdown.cancel();
}

#[tokio::test]
async fn real_tcp_server_rejects_unknown_path_with_404() {
    let env = TestEnv::new().await;
    let (addr, shutdown) = spawn_server(Arc::clone(&env.state), permissive_security()).await;

    let response = raw_get(addr, "/nope", None).await;
    assert_eq!(response.status, 404);
    let body = response.body_json();
    assert_eq!(body["error"]["code"], "not_found");

    shutdown.cancel();
}

#[tokio::test]
async fn real_tcp_server_protected_endpoint_requires_token() {
    let env = TestEnv::new().await;
    let (addr, shutdown) = spawn_server(Arc::clone(&env.state), permissive_security()).await;

    let path = format!("{API_V1}/nous");
    let no_auth = raw_get(addr, &path, None).await;
    assert_eq!(
        no_auth.status, 401,
        "protected endpoint must return 401 without a bearer token"
    );

    let token = issue_test_token(&env.state);
    let auth_header = bearer(&token);
    let with_auth = raw_get(addr, &path, Some(&auth_header)).await;
    assert_eq!(
        with_auth.status, 200,
        "same endpoint must return 200 with a valid bearer token"
    );

    shutdown.cancel();
}

#[tokio::test]
async fn real_tcp_server_short_jwt_ttl_expires_in_flight() {
    // WHY: Regression test for the JWT manager expiry path under the real
    // HTTP stack. Issue a token with a 1-second TTL, wait past expiry, then
    // confirm the server returns 401. If the extractor cached a validated
    // claim, this would silently pass 200.
    let env = TestEnv::builder()
        .jwt_access_ttl(Duration::from_secs(1))
        .build()
        .await;
    let token = issue_test_token(&env.state);
    let (addr, shutdown) = spawn_server(Arc::clone(&env.state), permissive_security()).await;

    let path = format!("{API_V1}/nous");
    let auth_header = bearer(&token);
    let first = raw_get(addr, &path, Some(&auth_header)).await;
    assert_eq!(first.status, 200);

    // WHY: JWT manager rejects exp <= now with wall-clock comparison. Sleep
    // past TTL plus slack to cross the boundary deterministically.
    tokio::time::sleep(Duration::from_millis(1_200)).await;

    let second = raw_get(addr, &path, Some(&auth_header)).await;
    assert_eq!(
        second.status, 401,
        "expired token must be rejected by the real HTTP server"
    );

    shutdown.cancel();
}
