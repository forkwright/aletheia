//! Rate limiting for MCP requests, shared across sessions and keyed by principal.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Instant;

use taxis::config::McpRateLimitConfig;

/// Bucket key used for requests that never resolved an authenticated principal.
///
/// WHY(#5182, #4843): invalid-credential probing must still be throttled, not
/// exempted for lacking a subject.
const UNAUTHENTICATED_PRINCIPAL: &str = "unauthenticated";

/// Multiplier applied to the configured per-tier limit to derive the
/// GLOBAL bucket's capacity.
///
/// WHY(#4843): the global bucket exists to cap aggregate MCP load
/// independent of session lifecycle — it must not be the everyday
/// bottleneck for every principal. If it shared the identical per-principal
/// capacity, a second distinct principal's very first request would already
/// find the global bucket drained by the first principal's own traffic,
/// defeating "one caller's exhausted quota does not throttle another" the
/// moment more than one caller is active. A generous multiplier keeps the
/// global bucket a true many-identities backstop.
const GLOBAL_CAPACITY_MULTIPLIER: u32 = 20;

/// Operation cost tier for rate limiting.
#[derive(Clone, Copy)]
pub(crate) enum Tier {
    /// Expensive operations: `session_message`, `session_create`, `knowledge_search`.
    Expensive,
    /// Cheap operations: list, status, health, config reads.
    Cheap,
}

/// Rate limiter for one MCP transport bind, shared across every session.
///
/// WHY(#5182, #4843): the streamable HTTP transport creates a fresh
/// `DiaporeiaServer` per session (`LocalSessionManager`). A limiter owned by
/// the server instance therefore reset every time a client opened a new
/// session — a caller could reset an exhausted budget by reconnecting. This
/// type is built ONCE per transport bind (see `transport.rs`) and shared via
/// `Arc` into every session's `DiaporeiaServer::with_state`, so quota state
/// survives session churn for the life of the bind.
///
/// Two layers apply on every check: a `global` bucket (covers pre-auth and
/// invalid-credential traffic, independent of any resolved identity) and a
/// per-`principal` bucket (keyed by the authenticated subject, or the shared
/// [`UNAUTHENTICATED_PRINCIPAL`] bucket when no principal resolves), so one
/// caller's exhausted quota does not throttle another.
pub struct RateLimiter {
    config: McpRateLimitConfig,
    global: TieredBuckets,
    principals: Mutex<HashMap<String, TieredBuckets>>,
}

impl RateLimiter {
    /// Build a rate limiter from a config snapshot.
    ///
    /// WHY: takes a snapshot rather than the live config so construction
    /// never blocks on the config `RwLock` (see `with_state`'s doc comment on
    /// why that matters inside a tokio runtime).
    #[must_use]
    pub fn from_config(config: &McpRateLimitConfig) -> Self {
        Self {
            global: TieredBuckets::new_global(config),
            principals: Mutex::new(HashMap::new()),
            config: config.clone(),
        }
    }

    /// Check whether a request at the given tier is allowed for `principal`.
    ///
    /// `principal` should be the authenticated caller's subject, or `None`
    /// when no principal resolved (bucketed under a shared unauthenticated
    /// key so unauthenticated traffic is still throttled). Checks the global
    /// bucket first, then the principal's own bucket (created on first use);
    /// both must have capacity.
    pub(crate) fn check(&self, tier: Tier, principal: Option<&str>) -> Result<(), rmcp::ErrorData> {
        if !self.config.enabled {
            return Ok(());
        }
        if !self.global.try_acquire(tier) {
            return Err(rate_limit_error());
        }

        let key = principal.unwrap_or(UNAUTHENTICATED_PRINCIPAL);
        let mut principals = self.principals.lock().unwrap_or_else(|e| {
            tracing::warn!("rate limiter principal map lock poisoned, recovering");
            e.into_inner()
        });
        let bucket = principals
            .entry(key.to_owned())
            .or_insert_with(|| TieredBuckets::new(&self.config));
        if bucket.try_acquire(tier) {
            Ok(())
        } else {
            Err(rate_limit_error())
        }
    }
}

fn rate_limit_error() -> rmcp::ErrorData {
    rmcp::ErrorData::new(
        rmcp::model::ErrorCode(-32000),
        "rate limit exceeded: too many requests, retry after a brief delay",
        None,
    )
}

/// Paired expensive/cheap token buckets for one scope (global or a principal).
struct TieredBuckets {
    expensive: TokenBucket,
    cheap: TokenBucket,
}

impl TieredBuckets {
    fn new(config: &McpRateLimitConfig) -> Self {
        Self {
            expensive: TokenBucket::new(config.message_requests_per_minute),
            cheap: TokenBucket::new(config.read_requests_per_minute),
        }
    }

    /// Build the global (aggregate) buckets: the per-tier limit scaled by
    /// [`GLOBAL_CAPACITY_MULTIPLIER`]. See that constant's WHY for why the
    /// global ceiling must be looser than any single principal's.
    fn new_global(config: &McpRateLimitConfig) -> Self {
        Self {
            expensive: TokenBucket::new(
                config
                    .message_requests_per_minute
                    .saturating_mul(GLOBAL_CAPACITY_MULTIPLIER),
            ),
            cheap: TokenBucket::new(
                config
                    .read_requests_per_minute
                    .saturating_mul(GLOBAL_CAPACITY_MULTIPLIER),
            ),
        }
    }

    fn try_acquire(&self, tier: Tier) -> bool {
        match tier {
            Tier::Expensive => self.expensive.try_acquire(),
            Tier::Cheap => self.cheap.try_acquire(),
        }
    }
}

/// Simple token bucket: tokens refill at a constant rate up to capacity.
struct TokenBucket {
    inner: Mutex<BucketState>,
}

struct BucketState {
    tokens: f64,
    capacity: f64,
    refill_rate: f64,
    last_refill: Instant,
}

impl TokenBucket {
    fn new(per_minute: u32) -> Self {
        let capacity = f64::from(per_minute);
        Self {
            inner: Mutex::new(BucketState {
                tokens: capacity,
                capacity,
                refill_rate: capacity / 60.0,
                last_refill: Instant::now(),
            }),
        }
    }

    fn try_acquire(&self) -> bool {
        let mut state = self.inner.lock().unwrap_or_else(|e| {
            tracing::warn!("rate limiter lock poisoned, recovering");
            e.into_inner()
        });
        let now = Instant::now();
        let elapsed = now.duration_since(state.last_refill).as_secs_f64();
        state.tokens = (state.tokens + elapsed * state.refill_rate).min(state.capacity);
        state.last_refill = now;
        if state.tokens >= 1.0 {
            state.tokens -= 1.0;
            true
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_config(enabled: bool, message_rpm: u32, read_rpm: u32) -> McpRateLimitConfig {
        McpRateLimitConfig {
            enabled,
            message_requests_per_minute: message_rpm,
            read_requests_per_minute: read_rpm,
        }
    }

    #[test]
    fn disabled_limiter_always_allows() {
        let limiter = RateLimiter::from_config(&make_config(false, 1, 1));
        for _ in 0..100 {
            assert!(limiter.check(Tier::Expensive, Some("agent-a")).is_ok());
            assert!(limiter.check(Tier::Cheap, Some("agent-a")).is_ok());
        }
    }

    #[test]
    fn expensive_bucket_exhausts_before_cheap() {
        let limiter = RateLimiter::from_config(&make_config(true, 2, 100));

        assert!(limiter.check(Tier::Expensive, Some("agent-a")).is_ok());
        assert!(limiter.check(Tier::Expensive, Some("agent-a")).is_ok());
        assert!(limiter.check(Tier::Expensive, Some("agent-a")).is_err());

        // Cheap bucket should still have capacity.
        assert!(limiter.check(Tier::Cheap, Some("agent-a")).is_ok());
    }

    #[test]
    fn rate_limit_error_has_correct_code() {
        let limiter = RateLimiter::from_config(&make_config(true, 0, 0));
        let Err(err) = limiter.check(Tier::Expensive, Some("agent-a")) else {
            panic!("expected rate limit error")
        };
        assert_eq!(err.code, rmcp::model::ErrorCode(-32000));
        assert!(err.message.contains("rate limit exceeded"));
    }

    #[test]
    fn cheap_bucket_exhausts_independently() {
        let limiter = RateLimiter::from_config(&make_config(true, 100, 3));

        assert!(limiter.check(Tier::Cheap, Some("agent-a")).is_ok());
        assert!(limiter.check(Tier::Cheap, Some("agent-a")).is_ok());
        assert!(limiter.check(Tier::Cheap, Some("agent-a")).is_ok());
        assert!(limiter.check(Tier::Cheap, Some("agent-a")).is_err());

        // Expensive bucket should still have capacity.
        assert!(limiter.check(Tier::Expensive, Some("agent-a")).is_ok());
    }

    #[test]
    fn bucket_refills_over_time() {
        let bucket = TokenBucket::new(60);
        for _ in 0..60 {
            assert!(bucket.try_acquire());
        }
        assert!(!bucket.try_acquire());

        {
            let mut state = bucket.inner.lock().unwrap_or_else(|p| panic!("{p}"));
            state.last_refill -= std::time::Duration::from_secs(2);
        }

        // 2 seconds at 1 token/sec should yield at least 1 token.
        assert!(bucket.try_acquire());
    }

    #[test]
    fn distinct_principals_get_independent_budgets() {
        // WHY(#5182, #4843): a shared bucket keyed only by tier would let one
        // principal's traffic exhaust another's quota. Each principal must
        // have its own bucket inside the same `RateLimiter`.
        let limiter = RateLimiter::from_config(&make_config(true, 1, 1));

        assert!(limiter.check(Tier::Expensive, Some("agent-a")).is_ok());
        assert!(
            limiter.check(Tier::Expensive, Some("agent-a")).is_err(),
            "agent-a's own budget must now be exhausted"
        );
        assert!(
            limiter.check(Tier::Expensive, Some("agent-b")).is_ok(),
            "agent-b must have its own, independent budget"
        );
    }

    #[test]
    fn same_principal_shares_one_budget_across_repeated_checks() {
        // WHY(#5182, #4843): this is the core regression this type exists to
        // fix — a `RateLimiter` instance built once and shared (e.g. across
        // what would previously have been per-session `DiaporeiaServer`
        // instances) must not let the same principal's quota reset just
        // because a caller checks it "again" (simulating a new session
        // reusing the same shared limiter rather than constructing a fresh
        // one — see `server.rs` integration coverage for the full
        // cross-instance proof).
        let limiter = RateLimiter::from_config(&make_config(true, 1, 1));

        assert!(limiter.check(Tier::Cheap, Some("agent-a")).is_ok());
        assert!(
            limiter.check(Tier::Cheap, Some("agent-a")).is_err(),
            "the same limiter instance must remember agent-a's exhausted budget"
        );
    }

    #[test]
    fn unauthenticated_requests_share_one_throttled_bucket() {
        let limiter = RateLimiter::from_config(&make_config(true, 1, 100));

        assert!(limiter.check(Tier::Expensive, None).is_ok());
        assert!(
            limiter.check(Tier::Expensive, None).is_err(),
            "repeated unauthenticated probing must still be throttled"
        );
    }

    #[test]
    fn global_bucket_caps_traffic_even_across_many_distinct_principals() {
        // WHY(#4843): "add global ... quotas independent of session
        // lifecycle" — many distinct (legitimately independent) principals,
        // each individually within their own per-principal budget, must
        // still be capped in aggregate by the global bucket. One call per
        // principal keeps each principal's own bucket irrelevant here —
        // this isolates the global bucket as the only possible constraint.
        let limiter = RateLimiter::from_config(&make_config(true, 1, 100));

        for i in 0..GLOBAL_CAPACITY_MULTIPLIER {
            let principal = format!("agent-{i}");
            assert!(
                limiter.check(Tier::Expensive, Some(&principal)).is_ok(),
                "call {i} is within the global aggregate ceiling ({GLOBAL_CAPACITY_MULTIPLIER})"
            );
        }
        assert!(
            limiter.check(Tier::Expensive, Some("agent-overflow")).is_err(),
            "the (GLOBAL_CAPACITY_MULTIPLIER + 1)-th distinct principal must be denied by \
             the global aggregate ceiling even though it has never made a request before"
        );
    }

    #[test]
    fn distinct_principals_do_not_prematurely_exhaust_the_global_bucket() {
        // WHY(#4843): this is the defect the multiplier fixes — without it,
        // the global bucket's capacity equals a single principal's, so a
        // second distinct principal's very first request would already find
        // the global bucket drained by the first principal's traffic.
        let limiter = RateLimiter::from_config(&make_config(true, 1, 1));

        assert!(limiter.check(Tier::Cheap, Some("agent-a")).is_ok());
        assert!(
            limiter.check(Tier::Cheap, Some("agent-b")).is_ok(),
            "a second distinct principal's first request must not be denied by a global \
             bucket sized only for the first principal's traffic"
        );
    }
}
