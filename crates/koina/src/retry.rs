//! Configurable retry and backoff strategies.
//!
//! Provides [`BackoffStrategy`] for computing delay durations between retry
//! attempts and [`RetryConfig`] for executing async operations with automatic
//! retries. Replaces per-crate retry implementations with a shared vocabulary.

use std::fmt;
use std::future::Future;
use std::time::Duration;

use rand::Rng;

/// Backoff strategy for computing retry delays between attempts.
///
/// Attempts are 0-indexed: `delay_for_attempt(0)` returns the delay before
/// the first retry, `delay_for_attempt(1)` before the second, and so on.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum BackoffStrategy {
    /// Constant delay between every retry attempt.
    Constant {
        /// Delay applied before each retry.
        delay: Duration,
    },
    /// Fixed sequence of delays, used in order per attempt.
    ///
    /// If the attempt index exceeds the sequence length, the last delay is
    /// reused. An empty sequence yields zero delay.
    Fixed {
        /// Ordered delays for successive retry attempts.
        delays: Vec<Duration>,
    },
    /// Exponential backoff: `base * factor^attempt`, capped at `max_delay`.
    Exponential {
        /// Initial delay for the first retry (attempt 0).
        base: Duration,
        /// Multiplier applied per attempt.
        factor: u32,
        /// Upper bound on the computed delay.
        max_delay: Duration,
    },
    /// Exponential backoff with ±25% random jitter to prevent thundering herd.
    ExponentialJitter {
        /// Initial delay for the first retry (attempt 0).
        base: Duration,
        /// Multiplier applied per attempt.
        factor: u32,
        /// Upper bound on the computed delay (before jitter).
        max_delay: Duration,
    },
}

impl BackoffStrategy {
    /// Compute the delay for a given 0-indexed retry attempt.
    ///
    /// # Errors
    ///
    /// This method is infallible. Overflow is handled via saturation and
    /// capping against `max_delay`.
    #[must_use]
    pub fn delay_for_attempt(&self, attempt: u32) -> Duration {
        match self {
            Self::Constant { delay } => *delay,
            Self::Fixed { delays } => {
                let idx = usize::try_from(attempt).unwrap_or(usize::MAX);
                let clamped = idx.min(delays.len().saturating_sub(1));
                delays.get(clamped).copied().unwrap_or(Duration::ZERO)
            }
            Self::Exponential {
                base,
                factor,
                max_delay,
            } => compute_exponential(*base, *factor, *max_delay, attempt),
            Self::ExponentialJitter {
                base,
                factor,
                max_delay,
            } => {
                let base_delay = compute_exponential(*base, *factor, *max_delay, attempt);
                apply_jitter(base_delay)
            }
        }
    }
}

impl fmt::Display for BackoffStrategy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Constant { delay } => write!(f, "constant({delay:?})"),
            Self::Fixed { delays } => write!(f, "fixed({} steps)", delays.len()),
            Self::Exponential {
                base,
                factor,
                max_delay,
            } => {
                write!(f, "exponential({base:?}×{factor}, cap {max_delay:?})")
            }
            Self::ExponentialJitter {
                base,
                factor,
                max_delay,
            } => {
                write!(
                    f,
                    "exponential+jitter({base:?}×{factor}, cap {max_delay:?})"
                )
            }
        }
    }
}

/// Configuration for retry behavior.
///
/// Pairs a [`BackoffStrategy`] with a maximum retry count. Use
/// [`retry_async`][RetryConfig::retry_async] to execute an async operation
/// with automatic retries.
#[derive(Debug, Clone)]
pub struct RetryConfig {
    /// Maximum number of retry attempts (not counting the initial attempt).
    pub max_retries: u32,
    /// Strategy for computing delays between retries.
    pub strategy: BackoffStrategy,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_retries: 3,
            strategy: BackoffStrategy::Exponential {
                base: Duration::from_secs(1),
                factor: 2,
                max_delay: Duration::from_secs(30),
            },
        }
    }
}

impl fmt::Display for RetryConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "retry(max={}, {})", self.max_retries, self.strategy)
    }
}

impl RetryConfig {
    /// Run an async operation with retries according to this configuration.
    ///
    /// Calls `operation` up to `max_retries + 1` times (1 initial + retries).
    /// After each failure, `should_retry` decides whether to continue. When it
    /// returns `false` or all retries are exhausted, the last error is returned.
    ///
    /// Backoff delays are logged at `WARN` level with attempt metadata. Callers
    /// should wrap calls in a tracing span for operation-specific context.
    ///
    /// # Errors
    ///
    /// Returns the last error from `operation` when retries are exhausted
    /// or `should_retry` returns `false`.
    pub async fn retry_async<F, Fut, T, E>(
        &self,
        mut operation: F,
        should_retry: impl Fn(&E) -> bool,
    ) -> Result<T, E>
    where
        F: FnMut() -> Fut,
        Fut: Future<Output = Result<T, E>>,
    {
        let mut result = operation().await;

        for attempt in 0..self.max_retries {
            let err = match result {
                Ok(val) => return Ok(val),
                Err(e) => e,
            };

            if !should_retry(&err) {
                return Err(err);
            }

            let delay = self.strategy.delay_for_attempt(attempt);
            tracing::warn!(
                attempt = attempt + 1,
                max_retries = self.max_retries,
                delay_ms = u64::try_from(delay.as_millis()).unwrap_or(u64::MAX),
                "operation failed, retrying after backoff"
            );
            tokio::time::sleep(delay).await;

            result = operation().await;
        }

        result
    }
}

/// Compute exponential backoff steps for non-time-based backoff.
///
/// Returns `min(factor^attempt, cap)`. Useful when the backoff unit is not
/// time (e.g., conversation turns to skip between retry attempts).
///
/// # Examples
///
/// ```
/// use koina::retry::exponential_steps;
///
/// assert_eq!(exponential_steps(0, 2, 8), 1); // 2^0 = 1
/// assert_eq!(exponential_steps(1, 2, 8), 2); // 2^1 = 2
/// assert_eq!(exponential_steps(2, 2, 8), 4); // 2^2 = 4
/// assert_eq!(exponential_steps(3, 2, 8), 8); // 2^3 = 8
/// assert_eq!(exponential_steps(4, 2, 8), 8); // capped
/// ```
#[must_use]
pub fn exponential_steps(attempt: u32, factor: u32, cap: u32) -> u32 {
    // WHY: cap exponent at 30 to prevent overflow in checked_pow
    let exponent = attempt.min(30);
    factor.checked_pow(exponent).unwrap_or(u32::MAX).min(cap)
}

// --- Internal helpers ---

/// Compute exponential backoff delay: `base * factor^attempt`, capped at `max_delay`.
fn compute_exponential(base: Duration, factor: u32, max_delay: Duration, attempt: u32) -> Duration {
    // WHY: cap exponent at 30 to prevent u64 overflow (2^31 * base_ms > u64::MAX for large bases)
    let exponent = attempt.min(30);
    let multiplier = u64::from(factor).checked_pow(exponent).unwrap_or(u64::MAX);
    let base_ms = u64::try_from(base.as_millis()).unwrap_or(u64::MAX);
    let max_ms = u64::try_from(max_delay.as_millis()).unwrap_or(u64::MAX);
    let delay_ms = base_ms.saturating_mul(multiplier);
    Duration::from_millis(delay_ms.min(max_ms))
}

/// Apply ±25% random jitter to a delay duration.
fn apply_jitter(delay: Duration) -> Duration {
    let ms = u64::try_from(delay.as_millis()).unwrap_or(u64::MAX);
    let jitter_range = ms / 4;
    if jitter_range > 0 {
        // WHY: ±25% jitter prevents thundering herd under concurrent load
        let offset = rand::rng().random_range(0..jitter_range.saturating_mul(2));
        Duration::from_millis(ms.saturating_sub(jitter_range).saturating_add(offset))
    } else {
        delay
    }
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
mod tests {
    use super::*;

    #[test]
    fn constant_returns_same_delay_for_all_attempts() {
        let strategy = BackoffStrategy::Constant {
            delay: Duration::from_millis(100),
        };
        for attempt in 0..10 {
            assert_eq!(
                strategy.delay_for_attempt(attempt),
                Duration::from_millis(100),
                "constant strategy should return the same delay for attempt {attempt}"
            );
        }
    }

    #[test]
    fn fixed_returns_delays_in_order() {
        let strategy = BackoffStrategy::Fixed {
            delays: vec![
                Duration::from_millis(100),
                Duration::from_millis(200),
                Duration::from_millis(500),
            ],
        };
        assert_eq!(
            strategy.delay_for_attempt(0),
            Duration::from_millis(100),
            "attempt 0 should use first delay"
        );
        assert_eq!(
            strategy.delay_for_attempt(1),
            Duration::from_millis(200),
            "attempt 1 should use second delay"
        );
        assert_eq!(
            strategy.delay_for_attempt(2),
            Duration::from_millis(500),
            "attempt 2 should use third delay"
        );
    }

    #[test]
    fn fixed_repeats_last_delay_beyond_sequence() {
        let strategy = BackoffStrategy::Fixed {
            delays: vec![Duration::from_millis(100), Duration::from_millis(200)],
        };
        assert_eq!(
            strategy.delay_for_attempt(5),
            Duration::from_millis(200),
            "attempts beyond sequence length should reuse last delay"
        );
    }

    #[test]
    fn fixed_empty_returns_zero() {
        let strategy = BackoffStrategy::Fixed { delays: vec![] };
        assert_eq!(
            strategy.delay_for_attempt(0),
            Duration::ZERO,
            "empty Fixed sequence should return zero delay"
        );
    }

    #[test]
    fn exponential_computes_correct_delays() {
        let strategy = BackoffStrategy::Exponential {
            base: Duration::from_secs(2),
            factor: 2,
            max_delay: Duration::from_secs(300),
        };
        assert_eq!(
            strategy.delay_for_attempt(0),
            Duration::from_secs(2),
            "attempt 0: 2s * 2^0 = 2s"
        );
        assert_eq!(
            strategy.delay_for_attempt(1),
            Duration::from_secs(4),
            "attempt 1: 2s * 2^1 = 4s"
        );
        assert_eq!(
            strategy.delay_for_attempt(2),
            Duration::from_secs(8),
            "attempt 2: 2s * 2^2 = 8s"
        );
        assert_eq!(
            strategy.delay_for_attempt(3),
            Duration::from_secs(16),
            "attempt 3: 2s * 2^3 = 16s"
        );
    }

    #[test]
    fn exponential_caps_at_max_delay() {
        let strategy = BackoffStrategy::Exponential {
            base: Duration::from_secs(2),
            factor: 2,
            max_delay: Duration::from_secs(300),
        };
        assert_eq!(
            strategy.delay_for_attempt(20),
            Duration::from_secs(300),
            "delay should be capped at max_delay"
        );
    }

    #[test]
    fn exponential_jitter_within_bounds() {
        let strategy = BackoffStrategy::ExponentialJitter {
            base: Duration::from_secs(1),
            factor: 2,
            max_delay: Duration::from_secs(30),
        };
        // NOTE: run multiple times to exercise jitter randomness
        for _ in 0..20 {
            let delay = strategy.delay_for_attempt(0);
            // base=1s, ±25%: range is [750ms, 1250ms]
            assert!(
                delay >= Duration::from_millis(750) && delay <= Duration::from_millis(1250),
                "jitter delay {delay:?} should be within ±25% of 1s"
            );
        }
    }

    #[test]
    fn exponential_jitter_caps_before_jitter() {
        let strategy = BackoffStrategy::ExponentialJitter {
            base: Duration::from_secs(1),
            factor: 2,
            max_delay: Duration::from_secs(30),
        };
        for _ in 0..20 {
            let delay = strategy.delay_for_attempt(10);
            // base delay would be 1s * 2^10 = 1024s, capped at 30s
            // jitter: ±25% of 30s = [22.5s, 37.5s]
            assert!(
                delay >= Duration::from_millis(22_500) && delay <= Duration::from_millis(37_500),
                "jitter delay {delay:?} should be within ±25% of capped 30s"
            );
        }
    }

    #[test]
    fn exponential_steps_basic() {
        assert_eq!(exponential_steps(0, 2, 8), 1, "2^0 = 1");
        assert_eq!(exponential_steps(1, 2, 8), 2, "2^1 = 2");
        assert_eq!(exponential_steps(2, 2, 8), 4, "2^2 = 4");
        assert_eq!(exponential_steps(3, 2, 8), 8, "2^3 = 8");
    }

    #[test]
    fn exponential_steps_caps_at_max() {
        assert_eq!(exponential_steps(4, 2, 8), 8, "2^4 = 16, capped at 8");
        assert_eq!(exponential_steps(10, 2, 8), 8, "2^10 = 1024, capped at 8");
    }

    #[test]
    fn exponential_steps_handles_large_attempt() {
        assert_eq!(
            exponential_steps(31, 2, 1000),
            1000,
            "attempt beyond exponent cap should still produce capped value"
        );
    }

    #[test]
    fn display_impls() {
        let constant = BackoffStrategy::Constant {
            delay: Duration::from_millis(100),
        };
        assert!(
            constant.to_string().contains("constant"),
            "Constant display should contain 'constant'"
        );

        let config = RetryConfig::default();
        assert!(
            config.to_string().contains("retry"),
            "RetryConfig display should contain 'retry'"
        );
    }

    #[test]
    fn default_config_has_sensible_values() {
        let config = RetryConfig::default();
        assert_eq!(config.max_retries, 3, "default max_retries should be 3");
        // NOTE: verify it's an Exponential variant
        assert!(
            matches!(config.strategy, BackoffStrategy::Exponential { .. }),
            "default strategy should be Exponential"
        );
    }

    #[tokio::test]
    async fn retry_async_succeeds_on_first_attempt() {
        let config = RetryConfig {
            max_retries: 3,
            strategy: BackoffStrategy::Constant {
                delay: Duration::from_millis(1),
            },
        };
        let result: Result<i32, &str> = config.retry_async(|| async { Ok(42) }, |_| true).await;
        assert_eq!(result.unwrap(), 42, "should return success immediately");
    }

    #[tokio::test]
    async fn retry_async_retries_on_transient_failure() {
        tokio::time::pause();

        let config = RetryConfig {
            max_retries: 3,
            strategy: BackoffStrategy::Constant {
                delay: Duration::from_millis(10),
            },
        };
        let attempt = std::sync::atomic::AtomicU32::new(0);
        let result: Result<i32, &str> = config
            .retry_async(
                || {
                    let n = attempt.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    async move { if n < 2 { Err("transient") } else { Ok(42) } }
                },
                |_| true,
            )
            .await;
        assert_eq!(result.unwrap(), 42, "should succeed after retries");
        assert_eq!(
            attempt.load(std::sync::atomic::Ordering::Relaxed),
            3,
            "should have attempted 3 times (1 initial + 2 retries)"
        );
    }

    #[tokio::test]
    async fn retry_async_stops_on_non_retryable_error() {
        tokio::time::pause();

        let config = RetryConfig {
            max_retries: 5,
            strategy: BackoffStrategy::Constant {
                delay: Duration::from_millis(10),
            },
        };
        let attempt = std::sync::atomic::AtomicU32::new(0);
        let result: Result<i32, &str> = config
            .retry_async(
                || {
                    attempt.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    async { Err("fatal") }
                },
                |_e| false,
            )
            .await;
        assert!(result.is_err(), "should return error");
        assert_eq!(
            attempt.load(std::sync::atomic::Ordering::Relaxed),
            1,
            "should not retry non-retryable errors"
        );
    }

    #[tokio::test]
    async fn retry_async_exhausts_retries() {
        tokio::time::pause();

        let config = RetryConfig {
            max_retries: 2,
            strategy: BackoffStrategy::Constant {
                delay: Duration::from_millis(10),
            },
        };
        let attempt = std::sync::atomic::AtomicU32::new(0);
        let result: Result<i32, &str> = config
            .retry_async(
                || {
                    attempt.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    async { Err("always fails") }
                },
                |_| true,
            )
            .await;
        assert!(
            result.is_err(),
            "should return error after exhausting retries"
        );
        assert_eq!(
            attempt.load(std::sync::atomic::Ordering::Relaxed),
            3,
            "should attempt 1 + max_retries times"
        );
    }

    #[tokio::test]
    async fn retry_async_with_zero_retries() {
        let config = RetryConfig {
            max_retries: 0,
            strategy: BackoffStrategy::Constant {
                delay: Duration::from_millis(10),
            },
        };
        let attempt = std::sync::atomic::AtomicU32::new(0);
        let result: Result<i32, &str> = config
            .retry_async(
                || {
                    attempt.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    async { Err("fail") }
                },
                |_| true,
            )
            .await;
        assert!(result.is_err(), "should fail with zero retries");
        assert_eq!(
            attempt.load(std::sync::atomic::Ordering::Relaxed),
            1,
            "should attempt exactly once with zero retries"
        );
    }
}
