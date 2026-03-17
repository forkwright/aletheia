//! Custom middleware layers for pylon.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axum::body::Body;
use axum::extract::Request;
use axum::http::{Method, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use tracing::{Instrument, debug, warn};

use aletheia_taxis::config::PerUserRateLimitConfig;

use aletheia_koina::http::CONTENT_TYPE_JSON;

use crate::error::{ErrorBody, ErrorResponse};

/// CSRF protection state stored as a router extension.
#[derive(Debug, Clone)]
pub struct CsrfState {
    /// HTTP header name to check (e.g. `"x-requested-with"`).
    pub header_name: String,
    /// Expected header value (e.g. `"aletheia"`).
    pub header_value: String,
}

/// Middleware that requires a custom header on state-changing requests.
///
/// GET, HEAD, and OPTIONS are exempt. POST, PUT, DELETE, and PATCH must
/// include the configured header with the expected value.
pub async fn require_csrf_header(request: Request, next: Next) -> Response {
    let is_safe = matches!(
        *request.method(),
        Method::GET | Method::HEAD | Method::OPTIONS
    );

    if is_safe {
        return next.run(request).await;
    }

    let csrf = request.extensions().get::<CsrfState>().cloned();

    if let Some(csrf) = csrf {
        let header_value = request
            .headers()
            .get(&csrf.header_name)
            .and_then(|v| v.to_str().ok());

        match header_value {
            Some(v) if v == csrf.header_value => next.run(request).await,
            _ => (
                StatusCode::FORBIDDEN,
                axum::Json(ErrorResponse {
                    error: ErrorBody {
                        code: "csrf_rejected".to_owned(),
                        message: "missing or invalid CSRF header".to_owned(),
                        details: None,
                    },
                }),
            )
                .into_response(),
        }
    } else {
        next.run(request).await
    }
}

/// Request ID stored in request extensions for tracing and error responses.
#[derive(Debug, Clone)]
pub struct RequestId(pub String);

impl std::fmt::Display for RequestId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl AsRef<str> for RequestId {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl From<String> for RequestId {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl From<RequestId> for String {
    fn from(id: RequestId) -> Self {
        id.0
    }
}

/// Middleware that generates a ULID request ID and stores it in request extensions.
pub async fn inject_request_id(mut request: Request, next: Next) -> Response {
    let id = ulid::Ulid::new().to_string();
    request.extensions_mut().insert(RequestId(id));
    next.run(request).await
}

/// Middleware that enriches 4xx/5xx JSON error responses with `request_id`.
///
/// Must be placed inside the compression layer so the body is uncompressed.
pub async fn enrich_error_response(request: Request, next: Next) -> Response {
    let request_id = request
        .extensions()
        .get::<RequestId>()
        .map(std::string::ToString::to_string);

    let response = next.run(request).await;

    let Some(rid) = request_id else {
        return response;
    };

    if !response.status().is_client_error() && !response.status().is_server_error() {
        return response;
    }

    let is_json = response
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .is_some_and(|ct| ct.contains(CONTENT_TYPE_JSON));

    if !is_json {
        return response;
    }

    let (parts, body) = response.into_parts();
    let Ok(bytes) = axum::body::to_bytes(body, 64 * 1024).await else {
        return Response::from_parts(parts, Body::empty());
    };

    let Ok(mut json) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
        return Response::from_parts(parts, Body::from(bytes));
    };

    if let Some(error) = json.get_mut("error").and_then(|e| e.as_object_mut()) {
        error.insert(String::from("request_id"), serde_json::Value::String(rid));
        let new_bytes = serde_json::to_vec(&json).unwrap_or_else(|e| {
            warn!(error = %e, "failed to re-serialize error response body");
            Vec::new()
        });
        return Response::from_parts(parts, Body::from(new_bytes));
    }

    Response::from_parts(parts, Body::from(bytes))
}

/// Middleware that records HTTP request metrics (count + duration).
pub async fn record_http_metrics(request: Request, next: Next) -> Response {
    let method = request.method().to_string();
    let path = crate::metrics::normalize_path(request.uri().path());
    let start = std::time::Instant::now();

    let response = next.run(request).await;

    let status = response.status().as_u16();
    let duration = start.elapsed().as_secs_f64();
    crate::metrics::record_request(&method, &path, status, duration);

    response
}

/// Per-IP sliding-window rate limiter.
///
/// Each IP gets a fixed window of `window` duration. The window resets when
/// expired. Uses `std::sync::Mutex` (not tokio): the critical section is
/// short and contains no `.await` points.
pub struct RateLimiter {
    max_requests: u32,
    window: Duration,
    state: Mutex<HashMap<String, (Instant, u32)>>,
    /// When true, `X-Forwarded-For` / `X-Real-IP` headers are trusted for
    /// client IP resolution. Enable only when pylon sits behind a trusted
    /// reverse proxy that strips these headers from untrusted clients.
    trust_proxy: bool,
}

impl RateLimiter {
    pub fn new(requests_per_minute: u32) -> Self {
        Self {
            max_requests: requests_per_minute,
            window: Duration::from_secs(60),
            state: Mutex::new(HashMap::new()),
            trust_proxy: false,
        }
    }

    #[must_use]
    pub fn with_trust_proxy(mut self, trust_proxy: bool) -> Self {
        self.trust_proxy = trust_proxy;
        self
    }

    /// Check whether the given client key is within the rate limit.
    ///
    /// Returns `None` if the request is allowed, or `Some(retry_after_secs)`
    /// if the limit has been exceeded.
    pub fn check(&self, client: &str) -> Option<u64> {
        let now = Instant::now();
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);

        if let Some((window_start, count)) = state.get_mut(client) {
            if now.duration_since(*window_start) >= self.window {
                // Window expired: start a new one.
                *window_start = now;
                *count = 1;
                None
            } else if *count >= self.max_requests {
                let elapsed = now.duration_since(*window_start);
                let remaining = self.window.saturating_sub(elapsed);
                Some(remaining.as_secs() + 1)
            } else {
                *count += 1;
                None
            }
        } else {
            state.insert(client.to_owned(), (now, 1));
            None
        }
    }
}

/// Extract the best available client identifier for rate limiting.
///
/// Priority order:
/// 1. Peer TCP address from `ConnectInfo<SocketAddr>` (most trustworthy)
/// 2. `X-Forwarded-For` / `X-Real-IP` headers: only when `trust_proxy` is
///    true, because clients control these headers and can spoof them
/// 3. Fallback literal `"peer"` (`ConnectInfo` not injected by the server)
///
/// WHY: Using `X-Forwarded-For` by default allows any client to set an
/// arbitrary IP and bypass per-IP rate limits. Only trusted reverse proxies
/// should supply these headers, controlled by the `trust_proxy` flag.
fn extract_client_key(request: &Request, trust_proxy: bool) -> String {
    use axum::extract::ConnectInfo;
    use std::net::SocketAddr;

    // Prefer the actual peer address: not spoofable.
    if let Some(info) = request.extensions().get::<ConnectInfo<SocketAddr>>() {
        return info.0.ip().to_string();
    }

    // Only consult proxy headers when explicitly trusted.
    if trust_proxy {
        if let Some(xff) = request.headers().get("x-forwarded-for")
            && let Ok(s) = xff.to_str()
        {
            let ip = s.split(',').next().unwrap_or("").trim();
            if !ip.is_empty() {
                return ip.to_owned();
            }
        }
        if let Some(xri) = request.headers().get("x-real-ip")
            && let Ok(s) = xri.to_str()
        {
            let ip = s.trim();
            if !ip.is_empty() {
                return ip.to_owned();
            }
        }
    }

    // ConnectInfo was not injected (e.g. testing without real TCP).
    "peer".to_owned()
}

/// Middleware that enforces per-IP rate limiting.
///
/// Reads the `Arc<RateLimiter>` from request extensions (installed by
/// `build_router`). Returns 429 Too Many Requests with a `Retry-After` header
/// when the client has exceeded the configured limit.
pub async fn rate_limit(request: Request, next: Next) -> Response {
    let limiter = request.extensions().get::<Arc<RateLimiter>>().cloned();
    let Some(limiter) = limiter else {
        return next.run(request).await;
    };

    let client = extract_client_key(&request, limiter.trust_proxy);
    if let Some(retry_after_secs) = limiter.check(&client) {
        let mut response = (
            StatusCode::TOO_MANY_REQUESTS,
            axum::Json(ErrorResponse {
                error: ErrorBody {
                    code: "rate_limited".to_owned(),
                    message: format!("rate limited, retry after {retry_after_secs}s"),
                    details: Some(serde_json::json!({ "retry_after_secs": retry_after_secs })),
                },
            }),
        )
            .into_response();
        if let Ok(value) = axum::http::HeaderValue::from_str(&retry_after_secs.to_string()) {
            response
                .headers_mut()
                .insert(axum::http::header::RETRY_AFTER, value);
        }
        return response;
    }

    next.run(request).await
}

// ── Per-user rate limiting ──────────────────────────────────────────────────

/// Endpoint category for applying differentiated rate limits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum EndpointCategory {
    /// LLM/chat endpoints (most expensive).
    Llm,
    /// Tool execution endpoints.
    Tool,
    /// All other API endpoints.
    General,
}

impl EndpointCategory {
    /// Classify a request path into an endpoint category.
    pub(crate) fn from_path(path: &str) -> Self {
        // WHY: These paths trigger LLM inference, which is the most expensive
        // operation. Streaming and message-send both invoke the full pipeline.
        if path.contains("/sessions/stream") || path.contains("/messages") {
            return Self::Llm;
        }
        if path.contains("/tools") || path.contains("/config/reload") {
            return Self::Tool;
        }
        Self::General
    }
}

/// Token bucket state for a single user and endpoint category.
struct TokenBucket {
    tokens: f64,
    last_fill: Instant,
    capacity: f64,
    fill_rate: f64,
}

impl TokenBucket {
    fn new(rpm: u32, burst: u32) -> Self {
        let capacity = f64::from(burst);
        Self {
            tokens: capacity,
            last_fill: Instant::now(),
            // WHY: Convert requests-per-minute to tokens-per-second for
            // continuous refill calculation.
            fill_rate: f64::from(rpm) / 60.0,
            capacity,
        }
    }

    #[cfg(test)]
    fn new_at(rpm: u32, burst: u32, now: Instant) -> Self {
        let capacity = f64::from(burst);
        Self {
            tokens: capacity,
            last_fill: now,
            fill_rate: f64::from(rpm) / 60.0,
            capacity,
        }
    }

    /// Try to consume one token. Returns `Ok(())` if allowed, or
    /// `Err(retry_after_secs)` if the bucket is empty.
    fn try_acquire(&mut self, now: Instant) -> Result<(), u64> {
        let elapsed = now.duration_since(self.last_fill).as_secs_f64();
        self.tokens = (self.tokens + elapsed * self.fill_rate).min(self.capacity);
        self.last_fill = now;

        if self.tokens >= 1.0 {
            self.tokens -= 1.0;
            Ok(())
        } else {
            let deficit = 1.0 - self.tokens;
            let wait_secs = deficit / self.fill_rate;
            // SAFETY: ceil() of a positive f64 ratio is always non-negative.
            #[expect(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                reason = "ceil of positive f64 ratio fits in u64; minimum is 1"
            )]
            let retry_after = (wait_secs.ceil() as u64).max(1);
            Err(retry_after)
        }
    }
}

/// Per-user rate limit state across endpoint categories.
struct UserBuckets {
    general: TokenBucket,
    llm: TokenBucket,
    tool: TokenBucket,
    last_access: Instant,
}

/// Per-user token-bucket rate limiter with endpoint-category differentiation.
///
/// Each authenticated user gets separate token buckets for general, LLM, and
/// tool endpoints. Uses `std::sync::Mutex` (not tokio): the critical section
/// is short and contains no `.await` points.
pub struct UserRateLimiter {
    config: PerUserRateLimitConfig,
    state: Mutex<HashMap<String, UserBuckets>>,
}

impl UserRateLimiter {
    pub(crate) fn new(config: PerUserRateLimitConfig) -> Self {
        Self {
            config,
            state: Mutex::new(HashMap::new()),
        }
    }

    /// Check whether a request from `user` to the given `category` is allowed.
    ///
    /// Returns `None` if allowed, or `Some(retry_after_secs)` if rate limited.
    pub(crate) fn check(&self, user: &str, category: EndpointCategory) -> Option<u64> {
        let now = Instant::now();
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);

        let buckets = state.entry(user.to_owned()).or_insert_with(|| UserBuckets {
            general: TokenBucket::new(self.config.default_rpm, self.config.default_burst),
            llm: TokenBucket::new(self.config.llm_rpm, self.config.llm_burst),
            tool: TokenBucket::new(self.config.tool_rpm, self.config.tool_burst),
            last_access: now,
        });

        buckets.last_access = now;

        let bucket = match category {
            EndpointCategory::General => &mut buckets.general,
            EndpointCategory::Llm => &mut buckets.llm,
            EndpointCategory::Tool => &mut buckets.tool,
        };

        bucket.try_acquire(now).err()
    }

    /// Remove entries for users who haven't made requests within the stale
    /// threshold. Returns the number of entries evicted.
    pub(crate) fn cleanup_stale(&self) -> usize {
        let now = Instant::now();
        let stale_threshold = Duration::from_secs(self.config.stale_after_secs);
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);

        let before = state.len();
        state.retain(|_, buckets| now.duration_since(buckets.last_access) < stale_threshold);
        before - state.len()
    }

    /// Number of tracked users (for diagnostics).
    #[cfg(test)]
    pub(crate) fn tracked_users(&self) -> usize {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .len()
    }
}

/// Extract the user identity for per-user rate limiting.
///
/// Reads the `sub` claim from the JWT bearer token without full validation.
/// Falls back to the client IP key used by the per-IP limiter.
fn extract_user_key(request: &Request, trust_proxy: bool) -> String {
    // WHY: We decode the JWT payload without signature verification here
    // because full auth validation happens later in the handler extractor.
    // This is only for rate-limit keying, not for authorization.
    if let Some(auth) = request
        .headers()
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        && let Some(token) = auth.strip_prefix("Bearer ")
        && let Some(sub) = extract_jwt_sub(token)
    {
        return sub;
    }
    extract_client_key(request, trust_proxy)
}

/// Decode the `sub` claim from a JWT token without signature verification.
///
/// Used only for rate-limit keying. Authorization is validated separately.
fn extract_jwt_sub(token: &str) -> Option<String> {
    let payload_part = token.split('.').nth(1)?;
    let decoded = base64_decode_url_safe(payload_part);
    let payload: serde_json::Value = serde_json::from_slice(&decoded).ok()?;
    payload
        .get("sub")
        .and_then(|v| v.as_str())
        .map(String::from)
}

/// Decode base64url without padding (JWT standard encoding).
fn base64_decode_url_safe(input: &str) -> Vec<u8> {
    // WHY: JWT uses base64url encoding without padding. We convert to standard
    // base64 alphabet and add padding before decoding.
    let mut s = input.replace('-', "+").replace('_', "/");
    let padding = (4 - s.len() % 4) % 4;
    for _ in 0..padding {
        s.push('=');
    }

    let bytes: Vec<u8> = s.bytes().collect();
    let mut output = Vec::with_capacity(bytes.len() * 3 / 4);
    let decode_char = |c: u8| -> Option<u8> {
        match c {
            b'A'..=b'Z' => Some(c - b'A'),
            b'a'..=b'z' => Some(c - b'a' + 26),
            b'0'..=b'9' => Some(c - b'0' + 52),
            b'+' => Some(62),
            b'/' => Some(63),
            _ => None,
        }
    };

    for chunk in bytes.chunks(4) {
        if chunk.len() < 4 {
            break;
        }
        let vals: Vec<Option<u8>> = chunk.iter().map(|&b| decode_char(b)).collect();
        if let (Some(a), Some(b)) = (vals[0], vals[1]) {
            output.push((a << 2) | (b >> 4));
            if let Some(c) = vals[2] {
                output.push((b << 4) | (c >> 2));
                if let Some(d) = vals[3] {
                    output.push((c << 6) | d);
                }
            }
        }
    }

    output
}

/// Middleware that enforces per-user rate limiting with endpoint categories.
///
/// Reads the `Arc<UserRateLimiter>` from request extensions. Extracts user
/// identity from the JWT bearer token. Returns 429 with `Retry-After` header
/// when the user has exceeded the configured limit for the endpoint category.
pub async fn per_user_rate_limit(request: Request, next: Next) -> Response {
    let limiter = request.extensions().get::<Arc<UserRateLimiter>>().cloned();
    let trust_proxy = request
        .extensions()
        .get::<Arc<RateLimiter>>()
        .is_some_and(|l| l.trust_proxy);

    let Some(limiter) = limiter else {
        return next.run(request).await;
    };

    let user = extract_user_key(&request, trust_proxy);
    let category = EndpointCategory::from_path(request.uri().path());

    if let Some(retry_after_secs) = limiter.check(&user, category) {
        debug!(
            user = %user,
            category = ?category,
            retry_after_secs,
            "per-user rate limit exceeded"
        );
        let mut response = (
            StatusCode::TOO_MANY_REQUESTS,
            axum::Json(ErrorResponse {
                error: ErrorBody {
                    code: "rate_limited".to_owned(),
                    message: format!(
                        "per-user rate limit exceeded, retry after {retry_after_secs}s"
                    ),
                    details: Some(serde_json::json!({
                        "retry_after_secs": retry_after_secs,
                        "category": format!("{category:?}").to_lowercase(),
                    })),
                },
            }),
        )
            .into_response();
        if let Ok(value) = axum::http::HeaderValue::from_str(&retry_after_secs.to_string()) {
            response
                .headers_mut()
                .insert(axum::http::header::RETRY_AFTER, value);
        }
        return response;
    }

    next.run(request).await
}

/// Spawn a background task that periodically cleans up stale user rate limit
/// entries to prevent unbounded memory growth.
pub(crate) fn spawn_stale_cleanup(
    limiter: Arc<UserRateLimiter>,
    shutdown: tokio_util::sync::CancellationToken,
) {
    let stale_secs = limiter.config.stale_after_secs;
    let interval = Duration::from_secs(stale_secs / 2).max(Duration::from_secs(60));
    let span = tracing::info_span!("rate_limit_cleanup");

    tokio::spawn(
        async move {
            loop {
                tokio::select! {
                    biased;
                    () = shutdown.cancelled() => break,
                    () = tokio::time::sleep(interval) => {
                        let evicted = limiter.cleanup_stale();
                        if evicted > 0 {
                            debug!(evicted, "cleaned up stale rate limit entries");
                        }
                    }
                }
            }
        }
        .instrument(span),
    );
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use super::*;

    // ── EndpointCategory ────────────────────────────────────────────────

    #[test]
    fn classifies_llm_endpoints() {
        assert_eq!(
            EndpointCategory::from_path("/api/v1/sessions/stream"),
            EndpointCategory::Llm,
            "streaming endpoint must be classified as LLM"
        );
        assert_eq!(
            EndpointCategory::from_path("/api/v1/sessions/abc123/messages"),
            EndpointCategory::Llm,
            "message send endpoint must be classified as LLM"
        );
    }

    #[test]
    fn classifies_tool_endpoints() {
        assert_eq!(
            EndpointCategory::from_path("/api/v1/nous/syn/tools"),
            EndpointCategory::Tool,
            "tools endpoint must be classified as Tool"
        );
        assert_eq!(
            EndpointCategory::from_path("/api/v1/config/reload"),
            EndpointCategory::Tool,
            "config reload must be classified as Tool"
        );
    }

    #[test]
    fn classifies_general_endpoints() {
        assert_eq!(
            EndpointCategory::from_path("/api/v1/sessions"),
            EndpointCategory::General,
            "sessions list must be classified as General"
        );
        assert_eq!(
            EndpointCategory::from_path("/api/v1/nous"),
            EndpointCategory::General,
            "nous list must be classified as General"
        );
        assert_eq!(
            EndpointCategory::from_path("/api/health"),
            EndpointCategory::General,
            "health check must be classified as General"
        );
    }

    // ── TokenBucket ─────────────────────────────────────────────────────

    #[test]
    fn token_bucket_allows_burst_requests() {
        let now = Instant::now();
        let mut bucket = TokenBucket::new_at(60, 5, now);

        for i in 0..5 {
            assert!(
                bucket.try_acquire(now).is_ok(),
                "request {i} within burst should be allowed"
            );
        }
        assert!(
            bucket.try_acquire(now).is_err(),
            "request beyond burst should be rejected"
        );
    }

    #[test]
    fn token_bucket_refills_over_time() {
        let now = Instant::now();
        let mut bucket = TokenBucket::new_at(60, 2, now);

        assert!(bucket.try_acquire(now).is_ok(), "first request allowed");
        assert!(
            bucket.try_acquire(now).is_ok(),
            "second request (burst) allowed"
        );
        assert!(
            bucket.try_acquire(now).is_err(),
            "third request rejected (burst exhausted)"
        );

        // Advance 2 seconds: at 60 rpm = 1/sec, should refill 2 tokens
        let later = now + Duration::from_secs(2);
        assert!(
            bucket.try_acquire(later).is_ok(),
            "request after refill should be allowed"
        );
    }

    #[test]
    fn token_bucket_returns_retry_after() {
        let now = Instant::now();
        let mut bucket = TokenBucket::new_at(60, 1, now);

        assert!(bucket.try_acquire(now).is_ok(), "first request allowed");
        let result = bucket.try_acquire(now);
        assert!(result.is_err(), "second request should be rejected");
        let retry_after = result.expect_err("already checked is_err");
        assert!(retry_after >= 1, "retry_after should be at least 1 second");
    }

    // ── UserRateLimiter ─────────────────────────────────────────────────

    #[test]
    fn per_user_isolation() {
        let config = PerUserRateLimitConfig {
            default_rpm: 60,
            default_burst: 2,
            ..PerUserRateLimitConfig::default()
        };
        let limiter = UserRateLimiter::new(config);

        // Exhaust alice's budget
        assert!(limiter.check("alice", EndpointCategory::General).is_none());
        assert!(limiter.check("alice", EndpointCategory::General).is_none());
        assert!(limiter.check("alice", EndpointCategory::General).is_some());

        // Bob should be unaffected
        assert!(
            limiter.check("bob", EndpointCategory::General).is_none(),
            "bob's requests must not be affected by alice's usage"
        );
    }

    #[test]
    fn different_categories_have_independent_buckets() {
        let config = PerUserRateLimitConfig {
            default_rpm: 60,
            default_burst: 1,
            llm_rpm: 20,
            llm_burst: 1,
            tool_rpm: 30,
            tool_burst: 1,
            ..PerUserRateLimitConfig::default()
        };
        let limiter = UserRateLimiter::new(config);

        // Exhaust general
        assert!(limiter.check("alice", EndpointCategory::General).is_none());
        assert!(limiter.check("alice", EndpointCategory::General).is_some());

        // LLM should still work
        assert!(
            limiter.check("alice", EndpointCategory::Llm).is_none(),
            "LLM bucket must be independent from general"
        );

        // Tool should still work
        assert!(
            limiter.check("alice", EndpointCategory::Tool).is_none(),
            "Tool bucket must be independent from general"
        );
    }

    #[test]
    fn cleanup_stale_evicts_old_entries() {
        let config = PerUserRateLimitConfig {
            stale_after_secs: 0,
            ..PerUserRateLimitConfig::default()
        };
        let limiter = UserRateLimiter::new(config);

        limiter.check("alice", EndpointCategory::General);
        limiter.check("bob", EndpointCategory::General);
        assert_eq!(
            limiter.tracked_users(),
            2,
            "should track 2 users before cleanup"
        );

        std::thread::sleep(Duration::from_millis(10));
        let evicted = limiter.cleanup_stale();
        assert_eq!(evicted, 2, "both stale entries should be evicted");
        assert_eq!(
            limiter.tracked_users(),
            0,
            "no users should remain after cleanup"
        );
    }

    #[test]
    fn cleanup_preserves_active_entries() {
        let config = PerUserRateLimitConfig {
            stale_after_secs: 600,
            ..PerUserRateLimitConfig::default()
        };
        let limiter = UserRateLimiter::new(config);

        limiter.check("alice", EndpointCategory::General);
        let evicted = limiter.cleanup_stale();
        assert_eq!(evicted, 0, "recent entries should not be evicted");
        assert_eq!(limiter.tracked_users(), 1, "active user should remain");
    }

    // ── JWT sub extraction ──────────────────────────────────────────────

    #[test]
    fn extracts_sub_from_valid_jwt() {
        let header = base64_encode_url_safe(br#"{"alg":"HS256","typ":"JWT"}"#);
        let payload = base64_encode_url_safe(br#"{"sub":"alice","iat":1000000}"#);
        let token = format!("{header}.{payload}.fakesignature");

        assert_eq!(
            extract_jwt_sub(&token),
            Some("alice".to_owned()),
            "should extract sub claim from JWT payload"
        );
    }

    #[test]
    fn returns_none_for_malformed_jwt() {
        assert_eq!(extract_jwt_sub("not-a-jwt"), None);
        assert_eq!(extract_jwt_sub("a.b"), None);
        assert_eq!(extract_jwt_sub(""), None);
    }

    /// Base64url encode without padding (for test JWT construction).
    fn base64_encode_url_safe(input: &[u8]) -> String {
        const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut result = String::new();
        for chunk in input.chunks(3) {
            let b0 = chunk[0];
            let b1 = chunk.get(1).copied().unwrap_or(0);
            let b2 = chunk.get(2).copied().unwrap_or(0);
            let combined = u32::from(b0) << 16 | u32::from(b1) << 8 | u32::from(b2);
            result.push(CHARS[((combined >> 18) & 0x3F) as usize] as char);
            result.push(CHARS[((combined >> 12) & 0x3F) as usize] as char);
            if chunk.len() > 1 {
                result.push(CHARS[((combined >> 6) & 0x3F) as usize] as char);
            }
            if chunk.len() > 2 {
                result.push(CHARS[(combined & 0x3F) as usize] as char);
            }
        }
        result.replace('+', "-").replace('/', "_")
    }
}
