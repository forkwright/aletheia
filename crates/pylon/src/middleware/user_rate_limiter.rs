//! Per-user rate limiting with endpoint-differentiated token buckets.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axum::extract::Request;
use axum::http::StatusCode;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use tracing::{Instrument, debug};

use aletheia_taxis::config::PerUserRateLimitConfig;

use crate::error::{ErrorBody, ErrorResponse};

use super::rate_limiter::{RateLimiter, extract_client_key};

/// Endpoint category for applying differentiated rate limits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EndpointCategory {
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
pub(super) struct TokenBucket {
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
    pub(super) fn new_at(rpm: u32, burst: u32, now: Instant) -> Self {
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
    pub(super) fn try_acquire(&mut self, now: Instant) -> Result<(), u64> {
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
                clippy::as_conversions,
                reason = "f64→u64: ceil of positive f64 ratio fits in u64; minimum is 1"
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
pub(super) fn extract_jwt_sub(token: &str) -> Option<String> {
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
        // vals has exactly 4 elements because chunk.len() >= 4 is checked above
        #[expect(
            clippy::indexing_slicing,
            reason = "vals has exactly 4 elements: chunk.len() >= 4 is checked"
        )]
        if let (Some(a), Some(b)) = (vals[0], vals[1]) {
            output.push((a << 2) | (b >> 4));
            #[expect(clippy::indexing_slicing, reason = "vals has exactly 4 elements")]
            if let Some(c) = vals[2] {
                output.push((b << 4) | (c >> 2));
                #[expect(clippy::indexing_slicing, reason = "vals has exactly 4 elements")]
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
pub fn spawn_stale_cleanup(
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
