//! Custom middleware layers for pylon.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axum::body::Body;
use axum::extract::Request;
use axum::http::{Method, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use tracing::warn;

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
