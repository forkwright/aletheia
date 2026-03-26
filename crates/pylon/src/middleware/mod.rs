//! Custom middleware layers for pylon.

use axum::body::Body;
use axum::extract::Request;
use axum::http::{Method, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use tracing::warn;

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
                        request_id: None,
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
mod rate_limiter;
mod user_rate_limiter;

pub use rate_limiter::{RateLimiter, rate_limit};
pub use user_rate_limiter::{
    EndpointCategory, UserRateLimiter, per_user_rate_limit, spawn_stale_cleanup,
};

#[cfg(test)]
use user_rate_limiter::TokenBucket;

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use std::time::{Duration, Instant};

    use aletheia_taxis::config::PerUserRateLimitConfig;

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
}
