//! Per-session token bucket rate limiter for MCP requests.

use std::sync::Mutex;
use std::time::Instant;

use aletheia_taxis::config::McpRateLimitConfig;

/// Operation cost tier for rate limiting.
pub(crate) enum Tier {
    /// Expensive operations: `session_message`, `session_create`, `knowledge_search`.
    Expensive,
    /// Cheap operations: list, status, health, config reads.
    Cheap,
}

/// Per-session rate limiter with separate buckets for expensive and cheap operations.
pub(crate) struct RateLimiter {
    expensive: TokenBucket,
    cheap: TokenBucket,
    enabled: bool,
}

impl RateLimiter {
    pub(crate) fn from_config(config: &McpRateLimitConfig) -> Self {
        Self {
            expensive: TokenBucket::new(config.message_requests_per_minute),
            cheap: TokenBucket::new(config.read_requests_per_minute),
            enabled: config.enabled,
        }
    }

    /// Check whether a request at the given tier is allowed.
    ///
    /// Returns `Ok(())` when permitted, or an MCP error when the bucket is
    /// exhausted.
    pub(crate) fn check(&self, tier: Tier) -> Result<(), rmcp::ErrorData> {
        if !self.enabled {
            return Ok(());
        }
        let bucket = match tier {
            Tier::Expensive => &self.expensive,
            Tier::Cheap => &self.cheap,
        };
        if bucket.try_acquire() {
            Ok(())
        } else {
            Err(rmcp::ErrorData::new(
                rmcp::model::ErrorCode(-32000),
                "rate limit exceeded: too many requests, retry after a brief delay",
                None,
            ))
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
        let mut state = self.inner.lock().expect("rate limiter lock poisoned");
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

    fn test_config(enabled: bool, message_rpm: u32, read_rpm: u32) -> McpRateLimitConfig {
        McpRateLimitConfig {
            enabled,
            message_requests_per_minute: message_rpm,
            read_requests_per_minute: read_rpm,
        }
    }

    #[test]
    fn disabled_limiter_always_allows() {
        let limiter = RateLimiter::from_config(&test_config(false, 1, 1));
        for _ in 0..100 {
            assert!(limiter.check(Tier::Expensive).is_ok());
            assert!(limiter.check(Tier::Cheap).is_ok());
        }
    }

    #[test]
    fn expensive_bucket_exhausts_before_cheap() {
        let limiter = RateLimiter::from_config(&test_config(true, 2, 100));

        assert!(limiter.check(Tier::Expensive).is_ok());
        assert!(limiter.check(Tier::Expensive).is_ok());
        assert!(limiter.check(Tier::Expensive).is_err());

        // Cheap bucket should still have capacity.
        assert!(limiter.check(Tier::Cheap).is_ok());
    }

    #[test]
    fn rate_limit_error_has_correct_code() {
        let limiter = RateLimiter::from_config(&test_config(true, 0, 0));
        let err = limiter.check(Tier::Expensive).unwrap_err();
        assert_eq!(err.code, rmcp::model::ErrorCode(-32000));
        assert!(err.message.contains("rate limit exceeded"));
    }

    #[test]
    fn cheap_bucket_exhausts_independently() {
        let limiter = RateLimiter::from_config(&test_config(true, 100, 3));

        assert!(limiter.check(Tier::Cheap).is_ok());
        assert!(limiter.check(Tier::Cheap).is_ok());
        assert!(limiter.check(Tier::Cheap).is_ok());
        assert!(limiter.check(Tier::Cheap).is_err());

        // Expensive bucket should still have capacity.
        assert!(limiter.check(Tier::Expensive).is_ok());
    }

    #[test]
    fn bucket_refills_over_time() {
        let bucket = TokenBucket::new(60);
        // Drain all tokens.
        for _ in 0..60 {
            assert!(bucket.try_acquire());
        }
        assert!(!bucket.try_acquire());

        // Manually advance time by adjusting last_refill.
        {
            let mut state = bucket.inner.lock().expect("lock");
            state.last_refill -= std::time::Duration::from_secs(2);
        }

        // 2 seconds at 1 token/sec should yield at least 1 token.
        assert!(bucket.try_acquire());
    }
}
