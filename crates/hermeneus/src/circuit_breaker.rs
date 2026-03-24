//! Circuit breaker for LLM provider calls.
//!
//! Implements the Closed → Open → HalfOpen state machine with configurable
//! failure threshold, open duration, and exponential backoff between probes.
//! State-change events are emitted to the metrics surface.

// WHY: std::sync::Mutex is correct here -- lock is held only during brief state reads/writes, never across .await
use std::sync::Mutex; // kanon:ignore RUST/std-mutex-in-async

use jiff::Timestamp;

/// Circuit breaker states.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum CircuitState {
    /// Accepting requests normally; counting consecutive failures.
    Closed,
    /// Rejecting all requests; waiting for cooldown before allowing a probe.
    Open {
        /// When the circuit opened.
        since: Timestamp,
    },
    /// Allowing a single probe request to test provider recovery.
    HalfOpen,
}

/// Configuration for a [`CircuitBreaker`].
#[derive(Debug, Clone)]
pub struct CircuitBreakerConfig {
    /// Consecutive failures required to open the circuit. Default: 5.
    pub failure_threshold: u32,
    /// Base cooldown before transitioning `Open` → `HalfOpen` (ms). Default: `30_000`.
    pub open_duration_ms: u64,
    /// Multiplier applied to `open_duration_ms` after each failed probe. Default: 2.0.
    pub backoff_multiplier: f64,
    /// Maximum backoff duration (ms). Default: `300_000` (5 minutes).
    pub backoff_max_ms: u64,
}

impl Default for CircuitBreakerConfig {
    fn default() -> Self {
        Self {
            failure_threshold: 5,
            open_duration_ms: 30_000,
            backoff_multiplier: 2.0,
            backoff_max_ms: 300_000,
        }
    }
}

struct CircuitBreakerInner {
    state: CircuitState,
    consecutive_failures: u32,
    /// How many `HalfOpen` probes have failed; drives exponential backoff.
    probe_attempts: u32,
}

/// Circuit breaker for a single LLM provider.
///
/// Thread-safe via `std::sync::Mutex`: no lock is held across `.await`.
///
/// # State machine
///
/// ```text
/// Closed ──[threshold failures]──▶ Open
///   ▲                               │
///   │         [cooldown elapsed]    │
///   │    ┌──── HalfOpen ◀───────────┘
///   └────┤
///        │    [probe fails]
///        └──▶ Open (backoff *= multiplier)
/// ```
pub struct CircuitBreaker {
    // kanon:ignore RUST/pub-visibility
    inner: Mutex<CircuitBreakerInner>,
    config: CircuitBreakerConfig,
    provider_name: String,
}

impl CircuitBreaker {
    /// Create a new circuit breaker starting in the `Closed` state.
    #[must_use]
    pub fn new(provider_name: impl Into<String>, config: CircuitBreakerConfig) -> Self {
        // kanon:ignore RUST/pub-visibility
        Self {
            inner: Mutex::new(CircuitBreakerInner {
                state: CircuitState::Closed,
                consecutive_failures: 0,
                probe_attempts: 0,
            }),
            config,
            provider_name: provider_name.into(),
        }
    }

    /// Create a new circuit breaker with default configuration.
    #[must_use]
    pub fn with_defaults(provider_name: impl Into<String>) -> Self {
        // kanon:ignore RUST/pub-visibility
        Self::new(provider_name, CircuitBreakerConfig::default())
    }

    /// Current circuit state (snapshot).
    #[must_use]
    pub fn state(&self) -> CircuitState {
        // kanon:ignore RUST/pub-visibility
        #[expect(
            clippy::expect_used,
            reason = "Mutex poisoning means a thread panicked; no Result return to propagate through"
        )]
        self.inner
            .lock()
            .expect("circuit breaker lock poisoned") // kanon:ignore RUST/expect
            .state
            .clone()
    }

    /// Check whether a new request is allowed to proceed.
    ///
    /// - `Closed`: always returns `true`.
    /// - `Open`: returns `false` until the probe cooldown elapses, then
    ///   transitions to `HalfOpen` and returns `true` for that one probe.
    /// - `HalfOpen`: returns `false` (probe already in flight).
    #[must_use]
    pub fn is_allowed(&self) -> bool {
        // kanon:ignore RUST/pub-visibility
        #[expect(
            clippy::expect_used,
            reason = "Mutex poisoning means a thread panicked; no Result return to propagate through"
        )]
        let mut inner = self.inner.lock().expect("circuit breaker lock poisoned"); // kanon:ignore RUST/expect
        match &inner.state {
            CircuitState::Closed => true,
            CircuitState::HalfOpen => false,
            CircuitState::Open { since } => {
                let cooldown_ms = self.probe_cooldown_ms(inner.probe_attempts);
                let cooldown = jiff::SignedDuration::from_millis(
                    i64::try_from(cooldown_ms).unwrap_or(i64::MAX),
                );
                let elapsed = Timestamp::now().duration_since(*since);
                if elapsed >= cooldown {
                    crate::metrics::record_circuit_transition(
                        &self.provider_name,
                        "open",
                        "half_open",
                    );
                    inner.state = CircuitState::HalfOpen;
                    true // this caller is the probe
                } else {
                    false
                }
            }
        }
    }

    /// Record a successful request outcome.
    ///
    /// - `Closed`: resets consecutive failure count.
    /// - `HalfOpen`: transitions to `Closed` and resets probe attempt count.
    pub fn on_success(&self) {
        // kanon:ignore RUST/pub-visibility
        #[expect(
            clippy::expect_used,
            reason = "Mutex poisoning means a thread panicked; no Result return to propagate through"
        )]
        let mut inner = self.inner.lock().expect("circuit breaker lock poisoned"); // kanon:ignore RUST/expect
        match inner.state {
            CircuitState::Closed => {
                inner.consecutive_failures = 0;
            }
            CircuitState::HalfOpen => {
                crate::metrics::record_circuit_transition(
                    &self.provider_name,
                    "half_open",
                    "closed",
                );
                inner.state = CircuitState::Closed;
                inner.consecutive_failures = 0;
                inner.probe_attempts = 0;
            }
            CircuitState::Open { .. } => {
                // NOTE: Success while Open is unexpected; probe cannot run while Open.
            }
        }
    }

    /// Record a failed request outcome.
    ///
    /// - `Closed`: increments consecutive failure count; opens circuit at threshold.
    /// - `HalfOpen`: increments probe attempt counter (extends backoff) and re-opens.
    /// - `Open`: no-op (already open).
    pub fn on_failure(&self) {
        // kanon:ignore RUST/pub-visibility
        #[expect(
            clippy::expect_used,
            reason = "Mutex poisoning means a thread panicked; no Result return to propagate through"
        )]
        let mut inner = self.inner.lock().expect("circuit breaker lock poisoned"); // kanon:ignore RUST/expect
        match inner.state {
            CircuitState::Closed => {
                inner.consecutive_failures += 1;
                if inner.consecutive_failures >= self.config.failure_threshold {
                    crate::metrics::record_circuit_transition(
                        &self.provider_name,
                        "closed",
                        "open",
                    );
                    inner.state = CircuitState::Open {
                        since: Timestamp::now(),
                    };
                }
            }
            CircuitState::HalfOpen => {
                inner.probe_attempts += 1;
                crate::metrics::record_circuit_transition(&self.provider_name, "half_open", "open");
                inner.state = CircuitState::Open {
                    since: Timestamp::now(),
                };
            }
            CircuitState::Open { .. } => {
                // NOTE: Already open; no further state change.
            }
        }
    }

    /// Compute the cooldown before the next probe attempt, applying backoff.
    ///
    /// `cooldown = min(open_duration_ms * backoff_multiplier^probe_attempts, backoff_max_ms)`
    fn probe_cooldown_ms(&self, probe_attempts: u32) -> u64 {
        #[expect(
            clippy::cast_precision_loss,
            clippy::as_conversions,
            reason = "open_duration_ms and backoff_max_ms are ms durations (≤300_000) well within f64 mantissa precision"
        )]
        let base = self.config.open_duration_ms as f64; // kanon:ignore RUST/as-cast
        let factor = self
            .config
            .backoff_multiplier
            .powi(i32::try_from(probe_attempts).unwrap_or(i32::MAX));
        let computed = base * factor;
        #[expect(
            clippy::cast_precision_loss,
            clippy::as_conversions,
            reason = "backoff_max_ms is a practical ms duration well within f64 mantissa precision"
        )]
        let max = self.config.backoff_max_ms as f64; // kanon:ignore RUST/as-cast
        let capped = computed.min(max);
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            clippy::as_conversions,
            reason = "capped is always non-negative and bounded by backoff_max_ms which fits in u64"
        )]
        {
            capped as u64 // kanon:ignore RUST/as-cast
        }
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test: accessing internal state via lock"
)]
// WHY: std::thread::sleep required because CircuitBreaker uses jiff::Timestamp::now() (real wall clock), // kanon:ignore TESTING/sleep-in-test
// not tokio's controllable clock. tokio::time::pause/advance would not affect jiff timestamps.
mod tests {
    use std::time::Duration;

    use super::*;

    fn breaker(threshold: u32, open_ms: u64) -> CircuitBreaker {
        CircuitBreaker::new(
            "test-provider",
            CircuitBreakerConfig {
                failure_threshold: threshold,
                open_duration_ms: open_ms,
                backoff_multiplier: 2.0,
                backoff_max_ms: 300_000,
            },
        )
    }

    #[test]
    fn starts_closed() {
        let cb = breaker(3, 30_000);
        assert_eq!(cb.state(), CircuitState::Closed);
        assert!(cb.is_allowed());
    }

    #[test]
    fn closed_to_open_at_threshold() {
        let cb = breaker(3, 30_000);
        cb.on_failure();
        cb.on_failure();
        assert_eq!(cb.state(), CircuitState::Closed);
        cb.on_failure();
        assert!(matches!(cb.state(), CircuitState::Open { .. }));
    }

    #[test]
    fn open_blocks_requests() {
        let cb = breaker(1, 30_000);
        cb.on_failure();
        assert!(!cb.is_allowed());
    }

    #[test]
    fn open_to_half_open_after_cooldown() {
        let cb = breaker(1, 1); // 1ms cooldown
        cb.on_failure();
        assert!(!cb.is_allowed());

        std::thread::sleep(Duration::from_millis(5)); // kanon:ignore TESTING/sleep-in-test
        // First call transitions Open → HalfOpen and returns true (probe).
        assert!(cb.is_allowed());
        assert_eq!(cb.state(), CircuitState::HalfOpen);
    }

    #[test]
    fn half_open_blocks_concurrent_requests() {
        let cb = breaker(1, 1); // 1ms cooldown
        cb.on_failure();
        std::thread::sleep(Duration::from_millis(5)); // kanon:ignore TESTING/sleep-in-test
        let _ = cb.is_allowed(); // transitions to HalfOpen
        // Subsequent callers must wait.
        assert!(!cb.is_allowed());
    }

    #[test]
    fn half_open_to_closed_on_success() {
        let cb = breaker(1, 1);
        cb.on_failure();
        std::thread::sleep(Duration::from_millis(5)); // kanon:ignore TESTING/sleep-in-test
        let _ = cb.is_allowed(); // → HalfOpen
        cb.on_success();
        assert_eq!(cb.state(), CircuitState::Closed);
        assert!(cb.is_allowed());
    }

    #[test]
    fn half_open_to_open_on_failure() {
        let cb = breaker(1, 1);
        cb.on_failure();
        std::thread::sleep(Duration::from_millis(5)); // kanon:ignore TESTING/sleep-in-test
        let _ = cb.is_allowed(); // → HalfOpen
        cb.on_failure(); // probe failed → Open again
        assert!(matches!(cb.state(), CircuitState::Open { .. }));
    }

    #[test]
    fn backoff_increases_after_failed_probe() {
        let cb = breaker(1, 1_000);
        // probe_attempts=0: cooldown = 1_000ms
        assert_eq!(cb.probe_cooldown_ms(0), 1_000);
        // probe_attempts=1: cooldown = 1_000 * 2 = 2_000ms
        assert_eq!(cb.probe_cooldown_ms(1), 2_000);
        // probe_attempts=2: cooldown = 1_000 * 4 = 4_000ms
        assert_eq!(cb.probe_cooldown_ms(2), 4_000);
    }

    #[test]
    fn backoff_capped_at_max() {
        let cb = CircuitBreaker::new(
            "test",
            CircuitBreakerConfig {
                failure_threshold: 1,
                open_duration_ms: 60_000,
                backoff_multiplier: 2.0,
                backoff_max_ms: 100_000,
            },
        );
        // 60_000 * 2^2 = 240_000 > 100_000 → capped
        assert_eq!(cb.probe_cooldown_ms(2), 100_000);
    }

    #[test]
    fn success_resets_probe_attempts() {
        let cb = breaker(1, 1);
        cb.on_failure(); // Open, probe_attempts=0
        std::thread::sleep(Duration::from_millis(5)); // kanon:ignore TESTING/sleep-in-test
        let _ = cb.is_allowed(); // → HalfOpen
        cb.on_failure(); // probe_attempts=1, → Open
        // Wait long enough for the backed-off cooldown (2ms with backoff).
        std::thread::sleep(Duration::from_millis(10)); // kanon:ignore TESTING/sleep-in-test
        let _ = cb.is_allowed(); // → HalfOpen again
        cb.on_success(); // → Closed, probe_attempts reset to 0
        assert_eq!(cb.state(), CircuitState::Closed);
        let inner = cb.inner.lock().expect("test: lock");
        assert_eq!(inner.probe_attempts, 0);
    }

    #[test]
    fn success_while_closed_resets_failures() {
        let cb = breaker(3, 30_000);
        cb.on_failure();
        cb.on_failure();
        cb.on_success();
        // Should still be Closed after success; failures reset.
        assert_eq!(cb.state(), CircuitState::Closed);
        // Now two more failures should NOT open (count was reset).
        cb.on_failure();
        cb.on_failure();
        assert_eq!(cb.state(), CircuitState::Closed);
    }

    #[test]
    fn circuit_breaker_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<CircuitBreaker>();
    }
}
