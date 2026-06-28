//! Custom middleware layers for pylon.

use std::sync::Arc;

use axum::body::Body;

use axum::extract::{FromRequestParts, Request, State};
use axum::http::{HeaderValue, Method, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use tracing::warn;

use koina::http::CONTENT_TYPE_JSON;
use koina::secret::SecretString;

use crate::error::{ApiError, ErrorBody, ErrorResponse};
use crate::extract::Claims;
use crate::state::AppState;

/// CSRF protection state stored as a router extension.
#[derive(Debug, Clone)]
pub struct CsrfState {
    /// Whether the CSRF header check is active.
    ///
    /// When disabled, mutating requests still receive same-origin enforcement
    /// so routes are not left fully unprotected in browser-facing deployments.
    pub enabled: bool,
    /// HTTP header name to check (e.g. `"x-requested-with"`).
    pub header_name: String,
    /// Expected header value (e.g. `"aletheia"`).
    pub header_value: SecretString,
}

/// Middleware that validates bearer auth for an entire router subtree.
///
/// The validated claims are cached in request extensions so handlers that also
/// extract [`Claims`] do not re-validate the same token.
pub async fn require_bearer_auth(
    State(state): State<Arc<AppState>>,
    request: Request,
    next: Next,
) -> Result<Response, ApiError> {
    let (mut parts, body) = request.into_parts();
    let claims = Claims::from_request_parts(&mut parts, &state).await?;
    parts.extensions.insert(claims);
    Ok(next.run(Request::from_parts(parts, body)).await)
}

/// Middleware that protects state-changing requests against CSRF and
/// cross-origin drive-by attacks.
///
/// Safe methods (GET, HEAD, OPTIONS) always pass. When CSRF is enabled,
/// mutating methods must carry the configured header and value. When CSRF is
/// disabled, mutating methods are still rejected if the browser-sent
/// `Origin` or `Referer` does not match the request host, while native or
/// loopback clients that send no origin indicator continue to pass.
///
/// # Cancel safety
///
/// Cancel-safe. Axum middleware; cancellation drops the future with no
/// side effects beyond not returning a response.
pub async fn require_csrf_header(request: Request, next: Next) -> Response {
    if is_safe_method(request.method()) {
        return next.run(request).await;
    }

    let Some(csrf) = request.extensions().get::<CsrfState>().cloned() else {
        return csrf_rejected_response();
    };

    if csrf.enabled {
        let header_value = request
            .headers()
            .get(&csrf.header_name)
            .and_then(|v| v.to_str().ok());

        match header_value {
            Some(v) if v == csrf.header_value.expose_secret() => next.run(request).await,
            _ => csrf_rejected_response(),
        }
    } else {
        // WHY(#5558): With CSRF disabled, mutating routes must still reject
        // browser-style cross-origin requests. Same-origin and non-browser
        // clients (no Origin/Referer) continue to pass for operator/loopback use.
        if is_same_origin(&request) {
            next.run(request).await
        } else {
            cross_origin_rejected_response()
        }
    }
}

/// Return whether the method is exempt from mutation guards.
fn is_safe_method(method: &Method) -> bool {
    matches!(*method, Method::GET | Method::HEAD | Method::OPTIONS)
}

/// Build the standard 403 response for a failed CSRF header check.
fn csrf_rejected_response() -> Response {
    (
        StatusCode::FORBIDDEN,
        axum::Json(ErrorResponse {
            error: ErrorBody {
                code: "csrf_rejected".to_owned(),
                message: "missing or invalid CSRF header".to_owned(),
                request_id: None,
                details: None,
            },
        }),
    )
        .into_response()
}

/// Build the standard 403 response for a cross-origin mutating request.
fn cross_origin_rejected_response() -> Response {
    (
        StatusCode::FORBIDDEN,
        axum::Json(ErrorResponse {
            error: ErrorBody {
                code: "cross_origin_rejected".to_owned(),
                message: "cross-origin mutating request rejected".to_owned(),
                request_id: None,
                details: None,
            },
        }),
    )
        .into_response()
}

/// Extract the host the request was sent to, including any non-default port.
///
/// WHY: Browsers send the target host in the `Host` header. Fallback to the
/// request URI authority supports direct tower tests that omit `Host`.
fn request_host(req: &Request) -> Option<String> {
    req.headers()
        .get(axum::http::header::HOST)
        .and_then(|v| v.to_str().ok())
        .map(str::to_lowercase)
        .or_else(|| req.uri().authority().map(|a| a.to_string().to_lowercase()))
}

/// Extract the authority (host[:port]) from an `Origin` or `Referer` header.
///
/// Returns `None` for opaque origins (`Origin: null`) or malformed values.
fn header_host(value: &HeaderValue) -> Option<String> {
    let s = value.to_str().ok()?;
    if s.eq_ignore_ascii_case("null") {
        return None;
    }
    let uri: axum::http::Uri = s.parse().ok()?;
    uri.authority().map(|a| a.to_string().to_lowercase())
}

/// Determine whether a mutating request is same-origin or origin-less.
///
/// Requests without `Origin` or `Referer` are treated as native/non-browser
/// and allowed. Otherwise the indicator must match the request host.
fn is_same_origin(req: &Request) -> bool {
    let indicator = req
        .headers()
        .get(axum::http::header::ORIGIN)
        .or_else(|| req.headers().get(axum::http::header::REFERER));

    let Some(indicator) = indicator else {
        return true;
    };

    let Some(host) = request_host(req) else {
        // WHY: A browser-style request with Origin/Referer but no discernible
        // host cannot be verified; reject rather than risk a cross-origin bypass.
        return false;
    };

    header_host(indicator).is_some_and(|h| h == host)
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

/// Header name for request ID correlation across systems.
const X_REQUEST_ID: &str = "x-request-id";

/// Middleware that generates a ULID request ID and stores it in request extensions.
///
/// If the client sends an `X-Request-ID` header, the server echoes it for
/// client-initiated correlation. Otherwise a new ULID is generated.
///
/// # Cancel safety
///
/// Cancel-safe. Axum middleware; cancellation drops the future with no
/// side effects beyond not returning a response.
pub async fn inject_request_id(mut request: Request, next: Next) -> Response {
    let id = request
        .headers()
        .get(X_REQUEST_ID)
        .and_then(|v| v.to_str().ok())
        .map_or_else(|| koina::ulid::Ulid::new().to_string(), String::from);
    request.extensions_mut().insert(RequestId(id.clone()));

    let mut response = next.run(request).await;
    if let Ok(header_value) = axum::http::HeaderValue::from_str(&id) {
        response.headers_mut().insert(X_REQUEST_ID, header_value);
    }
    response
}

/// Middleware that normalizes error responses into the `ErrorResponse` JSON
/// envelope and injects `request_id`.
///
/// WHY: Some error paths (e.g. Axum's built-in `Json` extractor rejection)
/// produce plain-text error bodies instead of the `ErrorResponse` envelope.
/// This middleware catches those responses and wraps them so all API errors
/// have a consistent `{error: {code, message}}` shape (#3160).
///
/// Must be placed inside the compression layer so the body is uncompressed.
///
/// # Cancel safety
///
/// Cancel-safe. Axum middleware; cancellation drops the future with no
/// side effects beyond not returning a response.
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

    let (parts, body) = response.into_parts();
    let Ok(bytes) = axum::body::to_bytes(body, 64 * 1024).await else {
        return Response::from_parts(parts, Body::empty());
    };

    // WHY: Non-JSON error responses (e.g. Axum extractor rejections) are wrapped
    // in the ErrorResponse envelope so clients see a uniform JSON error shape.
    // The original plain-text message is preserved as the error message (#3160).
    if !is_json {
        let text_body = String::from_utf8_lossy(&bytes);
        let code = error_code_from_status(parts.status);
        let envelope = ErrorResponse {
            error: ErrorBody {
                code: code.to_owned(),
                message: text_body.into_owned(),
                request_id: Some(rid),
                details: None,
            },
        };
        let new_bytes = serde_json::to_vec(&envelope).unwrap_or_else(|e| {
            warn!(error = %e, "failed to serialize error envelope for plain-text response");
            bytes.to_vec()
        });
        let mut response = Response::from_parts(parts, Body::from(new_bytes));
        response.headers_mut().insert(
            axum::http::header::CONTENT_TYPE,
            axum::http::HeaderValue::from_static(CONTENT_TYPE_JSON),
        );
        return response;
    }

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

/// Map an HTTP status code to a machine-readable error code string.
///
/// Used by the error-wrapping middleware to generate an error code for
/// plain-text error responses that lack a structured code field.
fn error_code_from_status(status: StatusCode) -> &'static str {
    match status {
        StatusCode::BAD_REQUEST => "bad_request",
        StatusCode::UNAUTHORIZED => "unauthorized",
        StatusCode::FORBIDDEN => "forbidden",
        StatusCode::NOT_FOUND => "not_found",
        StatusCode::METHOD_NOT_ALLOWED => "method_not_allowed",
        StatusCode::CONFLICT => "conflict",
        StatusCode::GONE => "gone",
        StatusCode::UNPROCESSABLE_ENTITY => "validation_failed",
        StatusCode::TOO_MANY_REQUESTS => "rate_limited",
        StatusCode::INTERNAL_SERVER_ERROR => "internal_error",
        StatusCode::NOT_IMPLEMENTED => "not_implemented",
        StatusCode::SERVICE_UNAVAILABLE => "service_unavailable",
        _ if status.is_client_error() => "client_error",
        _ => "internal_error",
    }
}

/// Middleware that records HTTP request metrics (count + duration).
///
/// # Cancel safety
///
/// Cancel-safe. Axum middleware; cancellation drops the future with no
/// side effects beyond not returning a response.
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
mod deprecation;
mod etag;
mod rate_limiter;
mod user_rate_limiter;

pub use deprecation::{DeprecationInfo, DeprecationLayer, DeprecationMap, deprecate};
pub use etag::{ETagLayer, ETagService};
pub use rate_limiter::{RateLimiter, rate_limit, spawn_anon_cleanup};
pub use user_rate_limiter::{
    EndpointCategory, UserRateLimiter, per_user_rate_limit, spawn_stale_cleanup,
};

#[cfg(test)]
use user_rate_limiter::TokenBucket;

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use std::time::{Duration, Instant};

    use taxis::config::PerUserRateLimitConfig;

    use super::*;

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
    fn classifies_sensitive_credential_endpoints() {
        assert_eq!(
            EndpointCategory::from_path("/api/v1/system/credentials"),
            EndpointCategory::Sensitive,
            "credential list/mutation endpoint must be classified as Sensitive"
        );
        assert_eq!(
            EndpointCategory::from_path("/api/v1/system/credentials/anthropic:primary/validate"),
            EndpointCategory::Sensitive,
            "credential validation endpoint must be classified as Sensitive"
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

    #[test]
    fn per_user_isolation() {
        let config = PerUserRateLimitConfig {
            default_rpm: 60,
            default_burst: 2,
            ..PerUserRateLimitConfig::default()
        };
        let limiter = UserRateLimiter::new(config);

        assert!(limiter.check("alice", EndpointCategory::General).is_none());
        assert!(limiter.check("alice", EndpointCategory::General).is_none());
        assert!(limiter.check("alice", EndpointCategory::General).is_some());

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

        assert!(limiter.check("alice", EndpointCategory::General).is_none());
        assert!(limiter.check("alice", EndpointCategory::General).is_some());

        assert!(
            limiter.check("alice", EndpointCategory::Llm).is_none(),
            "LLM bucket must be independent from general"
        );

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

        // WHY: real sleep required -- sync test with std::time::Instant; tokio time
        // control is unavailable. Ensures entries are strictly in the past for stale_after_secs=0.
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

    #[test]
    fn ip_ceiling_limits_aggregate_traffic_from_one_ip() {
        // WHY: Verifies that multiple tokens from the same IP cannot bypass
        // the per-user rate limit. The IP ceiling is 5x the per-user burst
        // (IP_CEILING_BURST_MULTIPLIER = 5), so 5 different users can each
        // make 1 request, but 6 total from the same IP hits the ceiling (#3228).
        let config = PerUserRateLimitConfig {
            default_rpm: 60,
            default_burst: 1,
            ..PerUserRateLimitConfig::default()
        };
        let limiter = UserRateLimiter::new(config);

        // 5 distinct users, 1 request each = 5 total from same IP (at ceiling).
        for i in 0..5 {
            let user = format!("user-{i}");
            assert!(
                limiter.check(&user, EndpointCategory::General).is_none(),
                "user {i} should be allowed (within per-user burst)"
            );
            assert!(
                limiter
                    .check_ip("192.168.1.100", EndpointCategory::General)
                    .is_none(),
                "IP should be allowed for user {i} (within IP ceiling burst)"
            );
        }

        // 6th user from same IP: per-user bucket allows it, but IP ceiling rejects.
        assert!(
            limiter.check("user-5", EndpointCategory::General).is_none(),
            "user-5 per-user bucket should allow (first request)"
        );
        assert!(
            limiter
                .check_ip("192.168.1.100", EndpointCategory::General)
                .is_some(),
            "IP ceiling should reject the 6th request from the same IP"
        );
    }

    #[test]
    fn ip_ceiling_tracks_separately_from_user_state() {
        let config = PerUserRateLimitConfig {
            stale_after_secs: 600,
            ..PerUserRateLimitConfig::default()
        };
        let limiter = UserRateLimiter::new(config);

        limiter.check("alice", EndpointCategory::General);
        limiter.check_ip("192.168.1.100", EndpointCategory::General);

        assert_eq!(limiter.tracked_users(), 1, "should track 1 user");
        assert_eq!(limiter.tracked_ips(), 1, "should track 1 IP");
    }

    #[test]
    fn cleanup_stale_evicts_ip_entries() {
        let config = PerUserRateLimitConfig {
            stale_after_secs: 0,
            ..PerUserRateLimitConfig::default()
        };
        let limiter = UserRateLimiter::new(config);

        limiter.check("alice", EndpointCategory::General);
        limiter.check_ip("192.168.1.100", EndpointCategory::General);
        assert_eq!(limiter.tracked_users(), 1);
        assert_eq!(limiter.tracked_ips(), 1);

        std::thread::sleep(Duration::from_millis(10));
        let evicted = limiter.cleanup_stale();
        assert_eq!(evicted, 2, "both user and IP entries should be evicted");
        assert_eq!(limiter.tracked_users(), 0);
        assert_eq!(limiter.tracked_ips(), 0);
    }
}
