//! Custom middleware layers for pylon.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axum::body::Body;
use axum::extract::Request;
use axum::http::{Method, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use tracing::{Instrument, warn};

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
        .is_some_and(|ct| ct.contains("application/json"));

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
        return build_rate_limit_response(retry_after_secs);
    }

    next.run(request).await
}

// ---------------------------------------------------------------------------
// Per-user token bucket rate limiter
// ---------------------------------------------------------------------------

/// Endpoint category for per-user rate limiting.
///
/// Different endpoint categories have different rate limits to reflect their
/// cost: LLM calls are expensive, tool execution is moderate, and general
/// API calls are cheapest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum EndpointCategory {
    General,
    Llm,
    Tool,
}

/// Token bucket state for a single user+category combination.
struct TokenBucket {
    tokens: f64,
    last_refill: Instant,
}

/// Per-user rate limiter using the token bucket algorithm.
///
/// Each (user, endpoint category) combination gets an independent token
/// bucket. Tokens refill at a steady rate (rpm / 60 tokens per second) up to
/// a burst cap. Uses `std::sync::Mutex`: the critical section is a `HashMap`
/// lookup with arithmetic, no `.await` points.
pub struct UserRateLimiter {
    /// WHY: lock held only during `HashMap` lookup + arithmetic, no await
    state: Mutex<HashMap<(String, EndpointCategory), TokenBucket>>,
    default_rpm: u32,
    default_burst: u32,
    llm_rpm: u32,
    tool_rpm: u32,
}

impl UserRateLimiter {
    pub(crate) fn new(default_rpm: u32, default_burst: u32, llm_rpm: u32, tool_rpm: u32) -> Self {
        Self {
            state: Mutex::new(HashMap::new()),
            default_rpm,
            default_burst,
            llm_rpm,
            tool_rpm,
        }
    }

    /// Return (`refill_rate_per_sec`, `max_tokens`) for the given category.
    fn limits(&self, category: EndpointCategory) -> (f64, f64) {
        let rpm = match category {
            EndpointCategory::General => self.default_rpm,
            EndpointCategory::Llm => self.llm_rpm,
            EndpointCategory::Tool => self.tool_rpm,
        };
        let refill_rate = f64::from(rpm) / 60.0;
        // WHY: burst for LLM/tool is proportional to their rpm, scaled by
        // the same ratio as default_burst / default_rpm, floored at 1.
        let max_tokens = if self.default_rpm == 0 {
            f64::from(self.default_burst).max(1.0)
        } else {
            (f64::from(rpm) * f64::from(self.default_burst) / f64::from(self.default_rpm)).max(1.0)
        };
        (refill_rate, max_tokens)
    }

    /// Check whether a request from `user` to an endpoint of `category` is allowed.
    ///
    /// Returns `None` if allowed, `Some(retry_after_secs)` if rate limited.
    pub(crate) fn check(&self, user: &str, category: EndpointCategory) -> Option<u64> {
        let now = Instant::now();
        let (refill_rate, max_tokens) = self.limits(category);

        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);

        let key = (user.to_owned(), category);
        let bucket = state.entry(key).or_insert_with(|| TokenBucket {
            tokens: max_tokens,
            last_refill: now,
        });

        // Refill tokens based on elapsed time.
        let elapsed = now.duration_since(bucket.last_refill).as_secs_f64();
        bucket.tokens = (bucket.tokens + elapsed * refill_rate).min(max_tokens);
        bucket.last_refill = now;

        if bucket.tokens >= 1.0 {
            bucket.tokens -= 1.0;
            None
        } else {
            // Calculate how long until one token is available.
            let deficit = 1.0 - bucket.tokens;
            let wait_secs = if refill_rate > 0.0 {
                deficit / refill_rate
            } else {
                60.0
            };
            #[expect(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                reason = "wait_secs is a small positive f64 from ceil(), safe to convert"
            )]
            Some(wait_secs.ceil() as u64)
        }
    }

    /// Remove entries for users who haven't made requests in the given duration.
    ///
    /// Call periodically to prevent unbounded memory growth from departed users.
    pub(crate) fn cleanup_stale(&self, max_idle: Duration) {
        let now = Instant::now();
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.retain(|_key, bucket| now.duration_since(bucket.last_refill) < max_idle);
    }

    /// Number of tracked user+category entries (for testing and metrics).
    #[cfg(test)]
    pub(crate) fn entry_count(&self) -> usize {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .len()
    }
}

/// Classify a request path into an endpoint category for per-user rate limiting.
pub(crate) fn classify_endpoint(path: &str, method: &Method) -> EndpointCategory {
    // LLM endpoints: streaming chat turns (the expensive operation).
    if path.starts_with("/api/v1/sessions") && path.ends_with("/stream") && *method == Method::POST
    {
        return EndpointCategory::Llm;
    }

    // Tool endpoints: sending messages (triggers tool execution pipeline).
    if path.starts_with("/api/v1/sessions")
        && path.ends_with("/messages")
        && *method == Method::POST
    {
        return EndpointCategory::Tool;
    }

    EndpointCategory::General
}

/// Extract user identity from the JWT Bearer token for rate limiting.
///
/// Decodes the JWT payload to read the `sub` claim without full cryptographic
/// validation. Full auth validation happens in the handler-level `Claims`
/// extractor; this is only for rate limiting keying. Falls back to
/// `extract_client_key` when no valid Bearer token is present.
fn extract_user_key(request: &Request) -> Option<String> {
    use base64::Engine;

    let header = request.headers().get("authorization")?.to_str().ok()?;
    let token = header.strip_prefix("Bearer ")?;

    // JWT format: base64url(header).base64url(payload).signature
    let payload = token.split('.').nth(1)?;

    let decoded = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(payload)
        .ok()?;
    let json: serde_json::Value = serde_json::from_slice(&decoded).ok()?;
    json.get("sub")?.as_str().map(str::to_owned)
}

/// Middleware that enforces per-user rate limiting with endpoint-specific limits.
///
/// Reads `Arc<UserRateLimiter>` from request extensions. Identifies the user
/// from the JWT Bearer token and classifies the endpoint category from the
/// request path. Returns 429 with `Retry-After` when the user's token bucket
/// is exhausted.
pub async fn user_rate_limit(request: Request, next: Next) -> Response {
    let limiter = request.extensions().get::<Arc<UserRateLimiter>>().cloned();
    let Some(limiter) = limiter else {
        return next.run(request).await;
    };

    let user = extract_user_key(&request).unwrap_or_else(|| extract_client_key(&request, false));
    let category = classify_endpoint(request.uri().path(), request.method());

    if let Some(retry_after_secs) = limiter.check(&user, category) {
        tracing::info!(
            user = %user,
            category = ?category,
            retry_after_secs,
            "per-user rate limit exceeded"
        );
        return build_rate_limit_response(retry_after_secs);
    }

    next.run(request).await
}

/// Build a 429 Too Many Requests response with `Retry-After` header.
fn build_rate_limit_response(retry_after_secs: u64) -> Response {
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
    response
}

/// Spawn a background task that periodically removes stale rate limit entries.
///
/// Runs every `interval` and removes entries idle for longer than `max_idle`.
/// Cancelled via the provided `CancellationToken`.
pub fn spawn_rate_limit_cleanup(
    limiter: Arc<UserRateLimiter>,
    interval: Duration,
    max_idle: Duration,
    cancel: tokio_util::sync::CancellationToken,
) {
    let span = tracing::info_span!("rate_limit_cleanup");
    tokio::spawn(
        async move {
            loop {
                tokio::select! {
                    biased;
                    () = cancel.cancelled() => break,
                    () = tokio::time::sleep(interval) => {
                        limiter.cleanup_stale(max_idle);
                    }
                }
            }
            tracing::debug!("rate limit cleanup task stopped");
        }
        .instrument(span),
    );
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use super::*;

    #[test]
    fn token_bucket_allows_requests_under_limit() {
        let limiter = UserRateLimiter::new(60, 10, 20, 30);
        // First request should always succeed (bucket starts full).
        assert!(
            limiter.check("alice", EndpointCategory::General).is_none(),
            "first request must be allowed"
        );
    }

    #[test]
    fn token_bucket_allows_burst() {
        let limiter = UserRateLimiter::new(60, 10, 20, 30);
        // Burst of 10 requests should all succeed.
        for i in 0..10 {
            assert!(
                limiter.check("alice", EndpointCategory::General).is_none(),
                "burst request {i} must be allowed"
            );
        }
        // 11th request should be rate limited.
        assert!(
            limiter.check("alice", EndpointCategory::General).is_some(),
            "request after burst exhaustion must be limited"
        );
    }

    #[test]
    fn token_bucket_returns_retry_after_on_limit() {
        let limiter = UserRateLimiter::new(60, 1, 20, 30);
        // Exhaust the single-token bucket.
        assert!(limiter.check("bob", EndpointCategory::General).is_none());
        let retry = limiter.check("bob", EndpointCategory::General);
        assert!(retry.is_some(), "must return retry_after when limited");
        assert!(
            retry.expect("checked above") > 0,
            "retry_after must be positive"
        );
    }

    #[test]
    fn per_user_isolation() {
        let limiter = UserRateLimiter::new(60, 1, 20, 30);
        // Exhaust alice's bucket.
        assert!(limiter.check("alice", EndpointCategory::General).is_none());
        assert!(limiter.check("alice", EndpointCategory::General).is_some());
        // bob's bucket is independent.
        assert!(
            limiter.check("bob", EndpointCategory::General).is_none(),
            "bob must not be affected by alice's usage"
        );
    }

    #[test]
    fn different_categories_have_different_limits() {
        // LLM has lower limit (20 rpm) so burst = 10 * 20/60 ≈ 3.
        let limiter = UserRateLimiter::new(60, 10, 20, 30);
        let mut llm_allowed = 0;
        for _ in 0..20 {
            if limiter.check("alice", EndpointCategory::Llm).is_none() {
                llm_allowed += 1;
            }
        }
        let mut general_allowed = 0;
        for _ in 0..20 {
            if limiter.check("bob", EndpointCategory::General).is_none() {
                general_allowed += 1;
            }
        }
        assert!(
            llm_allowed < general_allowed,
            "LLM endpoints must have a stricter limit than general: llm={llm_allowed}, general={general_allowed}"
        );
    }

    #[test]
    fn cleanup_stale_removes_old_entries() {
        let limiter = UserRateLimiter::new(60, 10, 20, 30);
        let _ = limiter.check("alice", EndpointCategory::General);
        let _ = limiter.check("bob", EndpointCategory::General);
        assert_eq!(limiter.entry_count(), 2, "should have 2 entries");
        // Cleanup with zero max_idle removes everything.
        limiter.cleanup_stale(Duration::ZERO);
        assert_eq!(
            limiter.entry_count(),
            0,
            "stale entries must be removed after cleanup"
        );
    }

    #[test]
    fn cleanup_stale_preserves_recent_entries() {
        let limiter = UserRateLimiter::new(60, 10, 20, 30);
        let _ = limiter.check("alice", EndpointCategory::General);
        // 10 minute max_idle should keep recent entries.
        limiter.cleanup_stale(Duration::from_secs(600));
        assert_eq!(limiter.entry_count(), 1, "recent entries must be preserved");
    }

    #[test]
    fn classify_endpoint_llm() {
        assert_eq!(
            classify_endpoint("/api/v1/sessions/stream", &Method::POST),
            EndpointCategory::Llm,
        );
    }

    #[test]
    fn classify_endpoint_tool() {
        assert_eq!(
            classify_endpoint("/api/v1/sessions/abc123/messages", &Method::POST),
            EndpointCategory::Tool,
        );
    }

    #[test]
    fn classify_endpoint_general_get() {
        assert_eq!(
            classify_endpoint("/api/v1/sessions", &Method::GET),
            EndpointCategory::General,
        );
    }

    #[test]
    fn classify_endpoint_general_other() {
        assert_eq!(
            classify_endpoint("/api/health", &Method::GET),
            EndpointCategory::General,
        );
    }

    #[test]
    fn extract_user_key_from_valid_jwt() {
        // Build a minimal JWT with sub claim.
        use base64::Engine;
        let header = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(br#"{"alg":"HS256"}"#);
        let payload = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .encode(br#"{"sub":"alice@example.com","role":"operator"}"#);
        let token = format!("{header}.{payload}.fakesig");

        let request = Request::builder()
            .header("authorization", format!("Bearer {token}"))
            .body(Body::empty())
            .expect("test request");

        let user = extract_user_key(&request);
        assert_eq!(user.as_deref(), Some("alice@example.com"));
    }

    #[test]
    fn extract_user_key_returns_none_without_auth() {
        let request = Request::builder()
            .body(Body::empty())
            .expect("test request");
        assert!(extract_user_key(&request).is_none());
    }

    #[test]
    fn extract_user_key_returns_none_for_invalid_jwt() {
        let request = Request::builder()
            .header("authorization", "Bearer not-a-jwt")
            .body(Body::empty())
            .expect("test request");
        assert!(extract_user_key(&request).is_none());
    }
}
