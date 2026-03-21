//! Per-IP rate limiting for anonymous requests.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axum::extract::Request;
use axum::http::StatusCode;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

use crate::error::{ErrorBody, ErrorResponse};

/// Per-IP sliding-window rate limiter for anonymous HTTP requests.
pub struct RateLimiter {
    // kanon:ignore RUST/pub-visibility
    max_requests: u32,
    window: Duration,
    state: Mutex<HashMap<String, (Instant, u32)>>,
    /// When true, `X-Forwarded-For` / `X-Real-IP` headers are trusted for
    /// client IP resolution. Enable only when pylon sits behind a trusted
    /// reverse proxy that strips these headers from untrusted clients.
    pub(super) trust_proxy: bool,
}

impl RateLimiter {
    /// Create a rate limiter that allows `requests_per_minute` per client IP.
    #[must_use]
    pub(crate) fn new(requests_per_minute: u32) -> Self {
        Self {
            max_requests: requests_per_minute,
            window: Duration::from_secs(60),
            state: Mutex::new(HashMap::new()),
            trust_proxy: false,
        }
    }

    /// Set whether to trust `X-Forwarded-For` / `X-Real-IP` headers for IP resolution.
    #[must_use]
    pub(crate) fn with_trust_proxy(mut self, trust_proxy: bool) -> Self {
        self.trust_proxy = trust_proxy;
        self
    }

    /// Check whether the given client key is within the rate limit.
    ///
    /// Returns `None` if the request is allowed, or `Some(retry_after_secs)`
    /// if the limit has been exceeded.
    pub(crate) fn check(&self, client: &str) -> Option<u64> {
        let now = Instant::now();
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);

        if let Some((window_start, count)) = state.get_mut(client) {
            if now.duration_since(*window_start) >= self.window {
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
pub(super) fn extract_client_key(request: &Request, trust_proxy: bool) -> String {
    use std::net::SocketAddr;

    use axum::extract::ConnectInfo;

    // NOTE: Prefer the actual peer address: not spoofable.
    if let Some(info) = request.extensions().get::<ConnectInfo<SocketAddr>>() {
        return info.0.ip().to_string();
    }

    // NOTE: Only consult proxy headers when explicitly trusted.
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

    // NOTE: ConnectInfo was not injected (e.g. testing without real TCP).
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
                    request_id: None,
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
