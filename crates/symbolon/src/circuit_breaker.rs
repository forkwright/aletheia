//! Three-state circuit breaker for OAuth token refresh.
//!
//! Protects the OAuth refresh endpoint from repeated failed requests by
//! tracking failures within a sliding window and temporarily blocking
//! requests when the failure threshold is exceeded.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

/// Circuit breaker state.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum CircuitState {
    /// Normal operation: requests flow through, failures counted.
    Closed,
    /// Tripped: requests fail immediately, cooldown timer running.
    Open,
    /// Probing: one request allowed through to test recovery.
    HalfOpen,
}

impl std::fmt::Display for CircuitState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Closed => f.write_str("closed"),
            Self::Open => f.write_str("open"),
            Self::HalfOpen => f.write_str("half-open"),
        }
    }
}

/// Configuration for the circuit breaker.
#[derive(Debug, Clone)]
pub struct CircuitBreakerConfig {
    /// Number of failures within the window to trip the circuit.
    pub failure_threshold: u32,
    /// Sliding window for failure counting.
    pub failure_window: Duration,
    /// Base cooldown before transitioning from Open to `HalfOpen`.
    pub cooldown: Duration,
    /// Maximum cooldown after exponential backoff.
    pub max_cooldown: Duration,
}

impl Default for CircuitBreakerConfig {
    fn default() -> Self {
        Self {
            failure_threshold: 5,
            failure_window: Duration::from_secs(60),
            cooldown: Duration::from_secs(30),
            max_cooldown: Duration::from_secs(300),
        }
    }
}

struct Inner {
    state: CircuitState,
    /// Failure timestamps within the sliding window (Closed state).
    failures: VecDeque<Instant>,
    /// When the circuit entered the Open state.
    opened_at: Option<Instant>,
    /// Current cooldown duration (increases with exponential backoff).
    current_cooldown: Duration,
    /// Number of consecutive Open→HalfOpen→Open cycles (for backoff).
    consecutive_trips: u32,
}

/// Three-state circuit breaker with exponential backoff.
///
/// Thread-safe via `std::sync::Mutex`: all operations are short
/// (no `.await` while holding the lock).
pub(crate) struct CircuitBreaker {
    // WHY: std::sync::Mutex is correct here because the lock is never held across .await
    inner: std::sync::Mutex<Inner>,
    config: CircuitBreakerConfig,
}

impl CircuitBreaker {
    /// Create a new circuit breaker starting in Closed state.
    #[must_use]
    pub(crate) fn new(config: CircuitBreakerConfig) -> Self {
        let initial_cooldown = config.cooldown;
        Self {
            inner: std::sync::Mutex::new(Inner {
                state: CircuitState::Closed,
                failures: VecDeque::new(),
                opened_at: None,
                current_cooldown: initial_cooldown,
                consecutive_trips: 0,
            }),
            config,
        }
    }

    /// Current circuit state.
    #[must_use]
    pub(crate) fn state(&self) -> CircuitState {
        // WHY: Mutex poisoning means a thread panicked; no recovery path exists
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .state
            .clone()
    }

    /// Check if a request is allowed through the circuit breaker.
    ///
    /// - Closed: always allowed
    /// - Open: allowed only if cooldown has elapsed (transitions to `HalfOpen`)
    /// - `HalfOpen`: blocked (one probe is already in flight)
    #[must_use]
    pub(crate) fn check_allowed(&self) -> bool {
        // WHY: Mutex poisoning means a thread panicked; recover with stale state
        let mut inner = self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        match inner.state {
            CircuitState::Closed => true,
            CircuitState::HalfOpen => false,
            CircuitState::Open => {
                let Some(opened_at) = inner.opened_at else {
                    return false;
                };
                if opened_at.elapsed() >= inner.current_cooldown {
                    let prev = inner.state.clone();
                    inner.state = CircuitState::HalfOpen;
                    tracing::info!(
                        from = %prev,
                        to = %inner.state,
                        cooldown_secs = inner.current_cooldown.as_secs(),
                        "circuit breaker state transition"
                    );
                    true
                } else {
                    false
                }
            }
        }
    }

    /// Record a successful request.
    ///
    /// - Closed: clears failure history
    /// - `HalfOpen`: closes the circuit (probe succeeded)
    /// - Open: no-op (requests should not reach here)
    pub(crate) fn record_success(&self) {
        // WHY: Mutex poisoning means a thread panicked; recover with stale state
        let mut inner = self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        match inner.state {
            CircuitState::Closed => {
                inner.failures.clear();
                inner.consecutive_trips = 0;
            }
            CircuitState::HalfOpen => {
                let prev = inner.state.clone();
                inner.state = CircuitState::Closed;
                inner.failures.clear();
                inner.consecutive_trips = 0;
                inner.current_cooldown = self.config.cooldown;
                inner.opened_at = None;
                tracing::info!(
                    from = %prev,
                    to = %inner.state,
                    "circuit breaker state transition"
                );
            }
            CircuitState::Open => {}
        }
    }

    /// Record a failed request.
    ///
    /// - Closed: adds failure to sliding window; trips to Open if threshold exceeded
    /// - `HalfOpen`: reopens circuit with exponentially increased cooldown
    /// - Open: no-op (requests should not reach here)
    pub(crate) fn record_failure(&self) {
        // WHY: Mutex poisoning means a thread panicked; recover with stale state
        let mut inner = self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let now = Instant::now();

        match inner.state {
            CircuitState::Closed => {
                inner.failures.push_back(now);
                // SAFETY: failure_window is always less than elapsed time since
                // process start, so underflow is not possible in practice.
                let window_start = now.checked_sub(self.config.failure_window).unwrap_or(now);
                while inner.failures.front().is_some_and(|&t| t < window_start) {
                    inner.failures.pop_front();
                }

                // WHY: failure count is bounded by failure_threshold (u32), so truncation is impossible
                let count = u32::try_from(inner.failures.len()).unwrap_or(u32::MAX);
                if count >= self.config.failure_threshold {
                    let prev = inner.state.clone();
                    inner.state = CircuitState::Open;
                    inner.opened_at = Some(now);
                    inner.current_cooldown =
                        Self::compute_cooldown(&self.config, inner.consecutive_trips);
                    inner.consecutive_trips = inner.consecutive_trips.saturating_add(1);
                    inner.failures.clear();
                    tracing::info!(
                        from = %prev,
                        to = %inner.state,
                        failure_count = count,
                        threshold = self.config.failure_threshold,
                        cooldown_secs = inner.current_cooldown.as_secs(),
                        "circuit breaker state transition"
                    );
                }
            }
            CircuitState::HalfOpen => {
                let prev = inner.state.clone();
                inner.current_cooldown =
                    Self::compute_cooldown(&self.config, inner.consecutive_trips);
                inner.consecutive_trips = inner.consecutive_trips.saturating_add(1);
                inner.state = CircuitState::Open;
                inner.opened_at = Some(now);
                tracing::info!(
                    from = %prev,
                    to = %inner.state,
                    consecutive_trips = inner.consecutive_trips,
                    cooldown_secs = inner.current_cooldown.as_secs(),
                    "circuit breaker state transition"
                );
            }
            CircuitState::Open => {}
        }
    }

    fn compute_cooldown(config: &CircuitBreakerConfig, consecutive_trips: u32) -> Duration {
        let multiplier = 2u64.saturating_pow(consecutive_trips);
        let cooldown_secs = config.cooldown.as_secs().saturating_mul(multiplier);
        let capped = cooldown_secs.min(config.max_cooldown.as_secs());
        Duration::from_secs(capped)
    }
}

#[cfg(test)]
mod tests {
    use std::thread;

    use super::*;

    fn breaker(
        threshold: u32,
        window_ms: u64,
        cooldown_ms: u64,
        max_cooldown_ms: u64,
    ) -> CircuitBreaker {
        CircuitBreaker::new(CircuitBreakerConfig {
            failure_threshold: threshold,
            failure_window: Duration::from_millis(window_ms),
            cooldown: Duration::from_millis(cooldown_ms),
            max_cooldown: Duration::from_millis(max_cooldown_ms),
        })
    }

    #[test]
    fn starts_closed() {
        let cb = breaker(5, 60_000, 30_000, 300_000);
        assert_eq!(cb.state(), CircuitState::Closed);
        assert!(cb.check_allowed());
    }

    #[test]
    fn trips_after_threshold_failures() {
        let cb = breaker(3, 60_000, 30_000, 300_000);
        for _ in 0..3 {
            cb.record_failure();
        }
        assert_eq!(cb.state(), CircuitState::Open);
        assert!(!cb.check_allowed());
    }

    #[test]
    fn failures_below_threshold_stay_closed() {
        let cb = breaker(3, 60_000, 30_000, 300_000);
        cb.record_failure();
        cb.record_failure();
        assert_eq!(cb.state(), CircuitState::Closed);
        assert!(cb.check_allowed());
    }

    #[test]
    fn enters_half_open_after_cooldown() {
        let cb = breaker(2, 60_000, 1, 300_000);
        cb.record_failure();
        cb.record_failure();
        assert_eq!(cb.state(), CircuitState::Open);

        thread::sleep(Duration::from_millis(5));
        assert!(cb.check_allowed(), "should allow probe after cooldown");
        assert_eq!(cb.state(), CircuitState::HalfOpen);
    }

    #[test]
    fn half_open_blocks_additional_requests() {
        let cb = breaker(2, 60_000, 1, 300_000);
        cb.record_failure();
        cb.record_failure();

        thread::sleep(Duration::from_millis(5));
        assert!(cb.check_allowed(), "first check transitions to half-open");
        assert!(!cb.check_allowed(), "second check blocked in half-open");
    }

    #[test]
    fn successful_probe_closes_circuit() {
        let cb = breaker(2, 60_000, 1, 300_000);
        cb.record_failure();
        cb.record_failure();

        thread::sleep(Duration::from_millis(5));
        assert!(cb.check_allowed());
        assert_eq!(cb.state(), CircuitState::HalfOpen);

        cb.record_success();
        assert_eq!(cb.state(), CircuitState::Closed);
        assert!(cb.check_allowed());
    }

    #[test]
    fn failed_probe_reopens_circuit() {
        let cb = breaker(2, 60_000, 1, 300_000);
        cb.record_failure();
        cb.record_failure();
        assert_eq!(cb.state(), CircuitState::Open);

        thread::sleep(Duration::from_millis(5));
        assert!(cb.check_allowed());
        assert_eq!(cb.state(), CircuitState::HalfOpen);

        cb.record_failure();
        assert_eq!(
            cb.state(),
            CircuitState::Open,
            "failed probe should reopen circuit"
        );

        thread::sleep(Duration::from_millis(10));
        assert!(cb.check_allowed(), "should allow after increased cooldown");
        assert_eq!(cb.state(), CircuitState::HalfOpen);
    }

    #[test]
    fn exponential_backoff_computes_correct_cooldowns() {
        let config = CircuitBreakerConfig {
            failure_threshold: 5,
            failure_window: Duration::from_secs(60),
            cooldown: Duration::from_secs(30),
            max_cooldown: Duration::from_secs(300),
        };
        assert_eq!(
            CircuitBreaker::compute_cooldown(&config, 0),
            Duration::from_secs(30),
            "trip 0: base cooldown"
        );
        assert_eq!(
            CircuitBreaker::compute_cooldown(&config, 1),
            Duration::from_secs(60),
            "trip 1: 2x base"
        );
        assert_eq!(
            CircuitBreaker::compute_cooldown(&config, 2),
            Duration::from_secs(120),
            "trip 2: 4x base"
        );
        assert_eq!(
            CircuitBreaker::compute_cooldown(&config, 3),
            Duration::from_secs(240),
            "trip 3: 8x base"
        );
        assert_eq!(
            CircuitBreaker::compute_cooldown(&config, 4),
            Duration::from_secs(300),
            "trip 4: capped at max"
        );
        assert_eq!(
            CircuitBreaker::compute_cooldown(&config, 10),
            Duration::from_secs(300),
            "trip 10: still capped"
        );
    }

    #[test]
    fn cooldown_caps_at_max() {
        let config = CircuitBreakerConfig {
            failure_threshold: 1,
            failure_window: Duration::from_secs(60),
            cooldown: Duration::from_secs(100),
            max_cooldown: Duration::from_secs(300),
        };
        assert_eq!(
            CircuitBreaker::compute_cooldown(&config, 0),
            Duration::from_secs(100),
            "trip 0"
        );
        assert_eq!(
            CircuitBreaker::compute_cooldown(&config, 1),
            Duration::from_secs(200),
            "trip 1"
        );
        assert_eq!(
            CircuitBreaker::compute_cooldown(&config, 2),
            Duration::from_secs(300),
            "trip 2: capped"
        );
        assert_eq!(
            CircuitBreaker::compute_cooldown(&config, 3),
            Duration::from_secs(300),
            "trip 3: still capped"
        );
    }

    #[test]
    fn success_in_closed_resets_failure_count() {
        let cb = breaker(3, 60_000, 30_000, 300_000);
        cb.record_failure();
        cb.record_failure();
        assert_eq!(cb.state(), CircuitState::Closed);

        cb.record_success();

        cb.record_failure();
        cb.record_failure();
        assert_eq!(cb.state(), CircuitState::Closed);
    }

    #[test]
    fn failures_outside_window_are_pruned() {
        let cb = breaker(3, 10, 30_000, 300_000);
        cb.record_failure();
        cb.record_failure();

        thread::sleep(Duration::from_millis(15));

        cb.record_failure();
        assert_eq!(
            cb.state(),
            CircuitState::Closed,
            "old failures should be pruned from window"
        );
    }

    #[test]
    fn successful_close_resets_backoff() {
        let cb = breaker(1, 60_000, 10, 300_000);

        cb.record_failure();
        thread::sleep(Duration::from_millis(15));
        assert!(cb.check_allowed());
        cb.record_success();
        assert_eq!(cb.state(), CircuitState::Closed);

        cb.record_failure();
        assert_eq!(cb.state(), CircuitState::Open);
        thread::sleep(Duration::from_millis(15));
        assert!(
            cb.check_allowed(),
            "cooldown should reset to base after successful close"
        );
    }

    #[test]
    fn open_before_cooldown_stays_blocked() {
        let cb = breaker(2, 60_000, 60_000, 300_000);
        cb.record_failure();
        cb.record_failure();
        assert_eq!(cb.state(), CircuitState::Open);
        assert!(!cb.check_allowed(), "should block before cooldown elapses");
    }

    #[test]
    fn breaker_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<CircuitBreaker>();
    }
}
