//! Per-user rate limiting with endpoint-differentiated token buckets.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axum::extract::Request;
use axum::http::StatusCode;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use tracing::{Instrument, debug};

use koina::http::BEARER_PREFIX;
use taxis::config::PerUserRateLimitConfig;

use crate::error::{ErrorBody, ErrorResponse};
use crate::extract::Claims;
use crate::state::AppState;

use super::rate_limiter::{RateLimiter, extract_client_key};

/// Rate limit quota snapshot for a single bucket.
#[derive(Debug, Clone, Copy)]
pub(crate) struct RateLimitQuota {
    /// Total requests allowed per window (bucket capacity).
    pub(crate) limit: u64,
    /// Requests remaining in the current window.
    pub(crate) remaining: u64,
    /// Seconds until the bucket fully refills.
    pub(crate) reset_secs: u64,
}

/// Endpoint category for applying differentiated rate limits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum EndpointCategory {
    /// LLM/chat endpoints (most expensive).
    Llm,
    /// Tool execution endpoints.
    Tool,
    /// Control-plane endpoints that affect sensitive backend state (#4773):
    /// tool approvals, event subscriptions, and knowledge mutations.
    ControlPlane,
    /// Credential control-plane endpoints. They share the stricter tool bucket
    /// until the config grows a dedicated sensitive quota (#4878).
    Sensitive,
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
        // WHY(#4878): Credential management changes provider authentication
        // state. It must not share a bucket with low-risk general traffic.
        if path.contains("/system/credentials") {
            return Self::Sensitive;
        }
        // WHY(#4773): Approval decisions, event subscriptions, and knowledge
        // mutations are control-plane operations with outsized backend impact;
        // they should not share a bucket with low-risk general traffic.
        if path.contains("/approvals")
            || path.contains("/events/subscribe")
            || path.contains("/knowledge/ingest")
            || path.contains("/knowledge/import")
            || path.contains("/knowledge/bulk")
            || path.contains("/knowledge/entities/merge")
        {
            return Self::ControlPlane;
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

    /// Return quota information without consuming a token.
    ///
    /// Returns `(limit, remaining, reset_secs)` where:
    /// - `limit` is the bucket capacity (burst)
    /// - `remaining` is the number of tokens available (floor)
    /// - `reset_secs` is seconds until the bucket fully refills
    pub(super) fn quota(&self, now: Instant) -> RateLimitQuota {
        let elapsed = now.duration_since(self.last_fill).as_secs_f64();
        let current = (self.tokens + elapsed * self.fill_rate).min(self.capacity);

        // SAFETY: capacity and current are non-negative f64; casting to u64 is safe.
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            clippy::as_conversions,
            reason = "f64→u64: floor of non-negative f64 fits in u64"
        )]
        let remaining = current.floor() as u64;

        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            clippy::as_conversions,
            reason = "f64→u64: capacity is non-negative and small"
        )]
        let limit = self.capacity as u64;

        // WHY: seconds until the bucket refills from current level to capacity.
        let deficit = self.capacity - current;
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            clippy::as_conversions,
            reason = "f64→u64: ceil of positive f64 ratio fits in u64"
        )]
        let reset_secs = if deficit > 0.0 && self.fill_rate > 0.0 {
            (deficit / self.fill_rate).ceil() as u64
        } else {
            0
        };

        RateLimitQuota {
            limit,
            remaining,
            reset_secs,
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

/// Burst multiplier for the per-IP rate limit ceiling.
///
/// WHY: The per-IP ceiling must be more generous than the per-user limit to
/// accommodate multiple legitimate users behind a shared IP (NAT, corporate
/// proxy). A 5x multiplier means up to 5 distinct users behind one IP can
/// each burst at their per-user rate before the IP ceiling kicks in. Beyond
/// that, token-multiplied abuse is throttled (#3228).
const IP_CEILING_BURST_MULTIPLIER: u32 = 5;

/// Per-user token-bucket rate limiter with endpoint-category differentiation.
///
/// Each authenticated user gets separate token buckets for general, LLM, and
/// tool endpoints. Additionally enforces a per-IP ceiling so that a single
/// IP address cannot bypass limits by creating multiple bearer tokens (#3228).
/// The per-IP ceiling uses the same RPM but a higher burst allowance
/// ([`IP_CEILING_BURST_MULTIPLIER`] x per-user burst) to accommodate
/// multiple legitimate users behind a shared IP.
///
/// Uses `std::sync::Mutex` (not tokio): the critical section
/// is short and contains no `.await` points.
pub struct UserRateLimiter {
    config: PerUserRateLimitConfig,
    /// Per-user rate limit state keyed by verified subject.
    state: Mutex<HashMap<String, UserBuckets>>,
    /// Per-IP rate limit ceiling. Checked alongside the per-user bucket so
    /// that a single IP cannot exceed the configured limit regardless of how
    /// many bearer tokens it presents (#3228).
    ip_state: Mutex<HashMap<String, UserBuckets>>,
}

impl UserRateLimiter {
    pub(crate) fn new(config: PerUserRateLimitConfig) -> Self {
        Self {
            config,
            state: Mutex::new(HashMap::new()),
            ip_state: Mutex::new(HashMap::new()),
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
            EndpointCategory::Tool
            | EndpointCategory::ControlPlane
            | EndpointCategory::Sensitive => &mut buckets.tool,
        };

        bucket.try_acquire(now).err()
    }

    /// Return the rate limit quota for a user in the given category.
    ///
    /// This reads the current bucket state without consuming a token.
    /// Returns `None` if the user has no existing bucket (first request).
    pub(crate) fn quota(&self, user: &str, category: EndpointCategory) -> Option<RateLimitQuota> {
        let now = Instant::now();
        let state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);

        let buckets = state.get(user)?;
        let bucket = match category {
            EndpointCategory::General => &buckets.general,
            EndpointCategory::Llm => &buckets.llm,
            EndpointCategory::Tool
            | EndpointCategory::ControlPlane
            | EndpointCategory::Sensitive => &buckets.tool,
        };

        Some(bucket.quota(now))
    }

    /// Check the per-IP rate limit ceiling.
    ///
    /// WHY: A client can obtain N bearer tokens and get N x the per-user rate
    /// limit from a single IP. This per-IP ceiling ensures one IP address
    /// cannot exceed the configured rate regardless of token count (#3228).
    /// The ceiling uses `IP_CEILING_BURST_MULTIPLIER` x the per-user burst
    /// so multiple legitimate users behind a shared IP are not penalized.
    ///
    /// Returns `None` if allowed, or `Some(retry_after_secs)` if rate limited.
    pub(crate) fn check_ip(&self, ip: &str, category: EndpointCategory) -> Option<u64> {
        let m = IP_CEILING_BURST_MULTIPLIER;
        let now = Instant::now();
        let mut state = self
            .ip_state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);

        let buckets = state.entry(ip.to_owned()).or_insert_with(|| UserBuckets {
            general: TokenBucket::new(
                self.config.default_rpm.saturating_mul(m),
                self.config.default_burst.saturating_mul(m),
            ),
            llm: TokenBucket::new(
                self.config.llm_rpm.saturating_mul(m),
                self.config.llm_burst.saturating_mul(m),
            ),
            tool: TokenBucket::new(
                self.config.tool_rpm.saturating_mul(m),
                self.config.tool_burst.saturating_mul(m),
            ),
            last_access: now,
        });

        buckets.last_access = now;

        let bucket = match category {
            EndpointCategory::General => &mut buckets.general,
            EndpointCategory::Llm => &mut buckets.llm,
            EndpointCategory::Tool
            | EndpointCategory::ControlPlane
            | EndpointCategory::Sensitive => &mut buckets.tool,
        };

        bucket.try_acquire(now).err()
    }

    /// Remove entries for users who haven't made requests within the stale
    /// threshold. Returns the number of entries evicted.
    pub(crate) fn cleanup_stale(&self) -> usize {
        let now = Instant::now();
        let stale_threshold = Duration::from_secs(self.config.stale_after_secs);

        let evicted_users = {
            let mut state = self
                .state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let before = state.len();
            state.retain(|_, buckets| now.duration_since(buckets.last_access) < stale_threshold);
            before - state.len()
        };

        let evicted_ips = {
            let mut ip_state = self
                .ip_state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let before = ip_state.len();
            ip_state.retain(|_, buckets| now.duration_since(buckets.last_access) < stale_threshold);
            before - ip_state.len()
        };

        evicted_users + evicted_ips
    }

    /// Number of tracked users (for diagnostics).
    #[cfg(test)]
    pub(crate) fn tracked_users(&self) -> usize {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .len()
    }

    /// Number of tracked IPs (for diagnostics).
    #[cfg(test)]
    pub(crate) fn tracked_ips(&self) -> usize {
        self.ip_state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .len()
    }
}

/// Extract the user identity for per-user rate limiting.
///
/// Uses verified JWT claims when a bearer token is present. Falls back to the
/// client IP only when no verified subject is available, such as public routes
/// without authentication.
fn extract_user_key(
    request: &Request,
    app_state: Option<&Arc<AppState>>,
    trust_proxy: bool,
) -> String {
    if let Some(claims) = request.extensions().get::<Claims>() {
        return subject_key(&claims.sub);
    }

    if let Some(state) = app_state {
        if state.auth_mode == "none" {
            return subject_key("anonymous");
        }

        if let Some(sub) = verified_bearer_subject(request, state) {
            return subject_key(&sub);
        }
    }

    extract_client_key(request, trust_proxy)
}

fn verified_bearer_subject(request: &Request, state: &AppState) -> Option<String> {
    if let Some(auth) = request.headers().get("authorization")
        && let Ok(val) = auth.to_str()
        && let Some(token) = val.strip_prefix(BEARER_PREFIX)
        && let Ok(claims) = state.auth_facade.validate_token(token)
    {
        return Some(claims.sub);
    }
    None
}

fn subject_key(sub: &str) -> String {
    format!("sub:{sub}")
}

/// Middleware that enforces per-user rate limiting with endpoint categories.
///
/// Reads the `Arc<UserRateLimiter>` from request extensions. Keys on both the
/// verified JWT subject (per-user) and client IP (per-IP ceiling). The per-IP
/// check prevents a single IP from bypassing limits by creating multiple
/// identities (#3228). Returns 429 with `Retry-After` header when the client
/// has exceeded the configured limit for the endpoint category.
///
/// On successful responses, injects standard rate limit headers
/// (`RateLimit-Limit`, `RateLimit-Remaining`, `RateLimit-Reset`) so
/// consumers can self-throttle (#3268).
///
/// # Cancel safety
///
/// Cancel-safe. Axum middleware; cancellation drops the future with no
/// side effects beyond not returning a response.
pub async fn per_user_rate_limit(request: Request, next: Next) -> Response {
    let limiter = request.extensions().get::<Arc<UserRateLimiter>>().cloned();
    let trust_proxy = request
        .extensions()
        .get::<Arc<RateLimiter>>()
        .is_some_and(|l| l.trust_proxy);
    let app_state = request.extensions().get::<Arc<AppState>>().cloned();

    let Some(limiter) = limiter else {
        return next.run(request).await;
    };

    let user = extract_user_key(&request, app_state.as_ref(), trust_proxy);
    let ip = extract_client_key(&request, trust_proxy);
    let category = EndpointCategory::from_path(request.uri().path());

    // WHY: Check per-IP ceiling first. A client opening N sessions (N bearer
    // tokens) from one IP would get N x the per-user limit without this
    // check. The IP ceiling ensures aggregate traffic from one address stays
    // within bounds regardless of token count (#3228).
    if let Some(retry_after_secs) = limiter.check_ip(&ip, category) {
        debug!(
            ip = %ip,
            category = ?category,
            retry_after_secs,
            "per-IP rate limit ceiling exceeded"
        );
        return rate_limit_response(retry_after_secs, category);
    }

    if let Some(retry_after_secs) = limiter.check(&user, category) {
        debug!(
            user = %user,
            category = ?category,
            retry_after_secs,
            "per-user rate limit exceeded"
        );
        return rate_limit_response(retry_after_secs, category);
    }

    let mut response = next.run(request).await;

    // WHY(#3268): Inject IETF rate limit headers on all responses so
    // consumers can monitor quota and self-throttle before hitting 429.
    if let Some(quota) = limiter.quota(&user, category) {
        inject_rate_limit_headers(response.headers_mut(), &quota);
    }

    response
}

/// Inject standard rate limit headers into a response.
///
/// IETF draft-ietf-httpapi-ratelimit-headers:
/// - `RateLimit-Limit`: total requests allowed per window
/// - `RateLimit-Remaining`: requests remaining in current window
/// - `RateLimit-Reset`: seconds until window resets
pub(crate) fn inject_rate_limit_headers(
    headers: &mut axum::http::HeaderMap,
    quota: &RateLimitQuota,
) {
    // WHY: HeaderName::from_static requires lowercase; these are standard draft headers.
    if let Ok(v) = axum::http::HeaderValue::from_str(&quota.limit.to_string()) {
        headers.insert(axum::http::HeaderName::from_static("ratelimit-limit"), v);
    }
    if let Ok(v) = axum::http::HeaderValue::from_str(&quota.remaining.to_string()) {
        headers.insert(
            axum::http::HeaderName::from_static("ratelimit-remaining"),
            v,
        );
    }
    if let Ok(v) = axum::http::HeaderValue::from_str(&quota.reset_secs.to_string()) {
        headers.insert(axum::http::HeaderName::from_static("ratelimit-reset"), v);
    }
}

/// Build a 429 Too Many Requests response with `Retry-After` header.
fn rate_limit_response(retry_after_secs: u64, category: EndpointCategory) -> Response {
    let mut response = (
        StatusCode::TOO_MANY_REQUESTS,
        axum::Json(ErrorResponse {
            error: ErrorBody {
                code: "rate_limited".to_owned(),
                message: format!("per-user rate limit exceeded, retry after {retry_after_secs}s"),
                request_id: None,
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
    response
}

/// Spawn a background task that periodically cleans up stale user rate limit
/// entries to prevent unbounded memory growth.
pub fn spawn_stale_cleanup(
    limiter: Arc<UserRateLimiter>,
    shutdown: tokio_util::sync::CancellationToken,
) {
    let stale_secs = limiter.config.stale_after_secs;
    let interval = Duration::from_secs(stale_secs / 2).max(Duration::from_mins(1));
    let span = tracing::info_span!("rate_limit_cleanup");

    tokio::spawn(
        async move {
            loop {
                tokio::select! {
                    biased;
                    // SAFETY: cancel-safe. `CancellationToken::cancelled()` is cancel-safe;
                    // dropping it before it fires has no side effects. Polled first (biased)
                    // so shutdown is never starved by a busy cleanup interval.
                    () = shutdown.cancelled() => break,
                    // SAFETY: cancel-safe. `tokio::time::sleep` is cancel-safe: if dropped
                    // before it fires, the sleep is abandoned and a new one starts on the
                    // next iteration. `cleanup_stale` is synchronous and idempotent.
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
