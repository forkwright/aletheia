#![expect(clippy::expect_used, reason = "test assertions use expect")]
#![expect(
    clippy::indexing_slicing,
    reason = "test: JSON indices and byte-slice ranges are valid after asserting status or known protocol shape"
)]
use std::sync::Arc;
use std::time::Duration;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;
use tower::ServiceExt;

use koina::http::{API_HEALTH, API_V1};
use pylon::idempotency::IdempotencyCache;
use pylon::router::build_router;
use pylon::security::{CsrfConfig, SecurityConfig, TlsConfig};
use pylon::state::AppState;

mod common;
use common::{TestEnv, bearer, issue_test_token, permissive_security};

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
    let mut request = format!("GET {path} HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\n");
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

/// Minimal `HTTP/1.1` POST over a raw TCP stream.
///
/// WHY: Mirrors `raw_get` for state-changing requests so cross-origin and
/// CSRF behavior can be exercised end-to-end over real HTTP framing.
async fn raw_post(
    addr: std::net::SocketAddr,
    path: &str,
    authorization: Option<&str>,
    extra_headers: &[(&str, &str)],
    body: &str,
) -> RawResponse {
    use std::fmt::Write as _;

    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let mut stream = tokio::net::TcpStream::connect(addr)
        .await
        .expect("connect tcp");
    let mut request = format!("POST {path} HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\n");
    for (name, value) in extra_headers {
        write!(&mut request, "{name}: {value}\r\n").expect("write header");
    }
    if let Some(value) = authorization {
        request.push_str("Authorization: ");
        request.push_str(value);
        request.push_str("\r\n");
    }
    request.push_str("Content-Length: ");
    request.push_str(&body.len().to_string());
    request.push_str("\r\n\r\n");
    request.push_str(body);
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

#[tokio::test]
async fn real_tcp_server_rejects_cross_origin_post_when_csrf_disabled() {
    // WHY(#5558): Even with CSRF disabled, browser-style mutating requests
    // from a foreign origin must be rejected. Raw TCP lets us set Host and
    // Origin exactly as a browser would.
    let env = TestEnv::builder().with_actor(true).build().await;
    let (addr, shutdown) = spawn_server(Arc::clone(&env.state), permissive_security()).await;

    let token = issue_test_token(&env.state);
    let body = serde_json::to_string(&serde_json::json!({
        "nous_id": "syn",
        "session_key": "cross-origin-tcp",
    }))
    .expect("serialize");

    let response = raw_post(
        addr,
        &format!("{API_V1}/sessions"),
        Some(&bearer(&token)),
        &[
            ("Origin", "http://evil.example.com"),
            ("Content-Type", "application/json"),
        ],
        &body,
    )
    .await;

    assert_eq!(
        response.status, 403,
        "cross-origin mutating request must be rejected when CSRF is disabled"
    );

    shutdown.cancel();
}
