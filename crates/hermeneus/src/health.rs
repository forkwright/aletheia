//! Provider health state machine.
//!
//! Tracks availability per provider through Up / Degraded / Down states.
//! Transitions happen on success, transient errors (server 5xx, rate limits),
//! and fatal errors (auth failure). Cooldown timers allow automatic recovery
//! from transient failures; auth failures require manual intervention.

use std::sync::Mutex;

use jiff::Timestamp;

use crate::error::Error;

/// Provider availability states.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ProviderHealth {
    /// Provider is responding normally.
    Up,
    /// Provider has had recent errors but is still accepting requests.
    Degraded {
        /// Number of errors since the last successful request.
        consecutive_errors: u32,
        /// When the most recent error occurred.
        last_error_at: Timestamp,
    },
    /// Provider is unavailable.
    Down {
        /// When the provider entered the Down state.
        since: Timestamp,
        /// What caused the transition to Down.
        reason: DownReason,
    },
}

/// Why a provider transitioned to `Down`.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum DownReason {
    /// Too many consecutive failures.
    ConsecutiveFailures,
    /// Provider returned 429 with retry-after.
    RateLimited {
        /// Milliseconds to wait before retrying, from the `retry-after` header.
        retry_after_ms: u64,
    },
    /// Authentication failed: no auto-recovery.
    AuthFailure,
    /// Request timed out repeatedly.
    Timeout,
}

/// Thresholds for health state transitions.
#[derive(Debug, Clone)]
pub struct HealthConfig {
    /// Consecutive errors before Degraded → Down. Default: 5.
    pub consecutive_failure_threshold: u32,
    /// Cooldown before retrying a Down provider (ms). Default: `60_000`.
    pub down_cooldown_ms: u64,
}

impl Default for HealthConfig {
    fn default() -> Self {
        Self {
            consecutive_failure_threshold: 5,
            down_cooldown_ms: 60_000,
        }
    }
}

struct TrackerInner {
    health: ProviderHealth,
    total_requests: u64,
    total_errors: u64,
}

/// Tracks health for a single LLM provider.
///
/// Thread-safe via `std::sync::Mutex`: all operations are short
/// (no `.await` while holding the lock).
pub struct ProviderHealthTracker {
    inner: Mutex<TrackerInner>,
    config: HealthConfig,
}

impl ProviderHealthTracker {
    /// Create a new tracker starting in `Up` state.
    #[must_use]
    pub fn new(config: HealthConfig) -> Self {
        Self {
            inner: Mutex::new(TrackerInner {
                health: ProviderHealth::Up,
                total_requests: 0,
                total_errors: 0,
            }),
            config,
        }
    }

    /// Current health state.
    #[must_use]
    pub fn health(&self) -> ProviderHealth {
        #[expect(
            clippy::expect_used,
            reason = "Mutex poisoning means a thread panicked; no Result return to propagate through"
        )]
        self.inner
            .lock()
            .expect("health lock poisoned")
            .health
            .clone()
    }

    /// Check if the provider can accept a request.
    ///
    /// Returns `Ok(())` if Up or Degraded. Returns `Err(health)` if Down,
    /// unless the cooldown has elapsed (auto-transitions to Degraded).
    /// `Down(AuthFailure)` never auto-recovers.
    pub fn check_available(&self) -> Result<(), ProviderHealth> {
        #[expect(
            clippy::expect_used,
            reason = "Mutex poisoning means a thread panicked; error type is ProviderHealth, not suitable for lock errors"
        )]
        let mut inner = self.inner.lock().expect("health lock poisoned");
        match &inner.health {
            ProviderHealth::Up | ProviderHealth::Degraded { .. } => Ok(()),
            ProviderHealth::Down { since, reason } => {
                if matches!(reason, DownReason::AuthFailure) {
                    return Err(inner.health.clone());
                }

                let cooldown_ms = match reason {
                    DownReason::RateLimited { retry_after_ms } => *retry_after_ms,
                    _ => self.config.down_cooldown_ms,
                };

                let cooldown = jiff::SignedDuration::from_millis(
                    i64::try_from(cooldown_ms).unwrap_or(i64::MAX),
                );
                let elapsed = Timestamp::now().duration_since(*since);

                if elapsed >= cooldown {
                    inner.health = ProviderHealth::Degraded {
                        consecutive_errors: 0,
                        last_error_at: *since,
                    };
                    Ok(())
                } else {
                    Err(inner.health.clone())
                }
            }
        }
    }

    /// Record a successful request.
    pub fn record_success(&self) {
        #[expect(
            clippy::expect_used,
            reason = "Mutex poisoning means a thread panicked; no Result return to propagate through"
        )]
        let mut inner = self.inner.lock().expect("health lock poisoned");
        inner.total_requests += 1;
        match inner.health {
            ProviderHealth::Degraded { .. } => {
                inner.health = ProviderHealth::Up;
            }
            // NOTE: already healthy, no state transition needed
            ProviderHealth::Up => {}
            ProviderHealth::Down { .. } => {
                // NOTE: Success while Down means a probe succeeded: transition to Up.
                inner.health = ProviderHealth::Up;
            }
        }
    }

    /// Record a failed request and update health state.
    pub fn record_error(&self, error: &Error) {
        #[expect(
            clippy::expect_used,
            reason = "Mutex poisoning means a thread panicked; no Result return to propagate through"
        )]
        let mut inner = self.inner.lock().expect("health lock poisoned");
        inner.total_requests += 1;
        inner.total_errors += 1;

        match error {
            Error::AuthFailed { .. } => {
                inner.health = ProviderHealth::Down {
                    since: Timestamp::now(),
                    reason: DownReason::AuthFailure,
                };
            }
            Error::RateLimited { retry_after_ms, .. } => {
                inner.health = ProviderHealth::Down {
                    since: Timestamp::now(),
                    reason: DownReason::RateLimited {
                        retry_after_ms: *retry_after_ms,
                    },
                };
            }
            Error::ApiRequest { .. }
            | Error::ApiError {
                status: 500..=599, ..
            } => {
                let now = Timestamp::now();
                match &inner.health {
                    ProviderHealth::Up => {
                        inner.health = ProviderHealth::Degraded {
                            consecutive_errors: 1,
                            last_error_at: now,
                        };
                    }
                    ProviderHealth::Degraded {
                        consecutive_errors, ..
                    } => {
                        let next = consecutive_errors + 1;
                        if next >= self.config.consecutive_failure_threshold {
                            inner.health = ProviderHealth::Down {
                                since: now,
                                reason: DownReason::ConsecutiveFailures,
                            };
                        } else {
                            inner.health = ProviderHealth::Degraded {
                                consecutive_errors: next,
                                last_error_at: now,
                            };
                        }
                    }
                    ProviderHealth::Down { .. } => {
                        // NOTE: Already down, no further transition.
                    }
                }
            }
            // NOTE: non-availability errors (parse, unsupported model) do not affect health
            _ => {}
        }
    }
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
mod tests {
    use super::*;

    fn tracker(threshold: u32, cooldown_ms: u64) -> ProviderHealthTracker {
        ProviderHealthTracker::new(HealthConfig {
            consecutive_failure_threshold: threshold,
            down_cooldown_ms: cooldown_ms,
        })
    }

    use std::time::Duration;

    use snafu::IntoError;

    fn api_request_error() -> Error {
        crate::error::ApiRequestSnafu { message: "timeout" }.build()
    }

    fn server_error() -> Error {
        crate::error::ApiSnafu {
            status: 500_u16,
            message: "internal",
        }
        .build()
    }

    fn auth_error() -> Error {
        crate::error::AuthFailedSnafu {
            message: "invalid key",
        }
        .build()
    }

    fn rate_limit_error(ms: u64) -> Error {
        crate::error::RateLimitedSnafu { retry_after_ms: ms }.build()
    }

    fn parse_error() -> Error {
        let json_err = serde_json::from_str::<String>("invalid").unwrap_err();
        crate::error::ParseResponseSnafu.into_error(json_err)
    }

    // --- State transitions ---

    #[test]
    fn starts_up() {
        let t = tracker(5, 60_000);
        assert_eq!(t.health(), ProviderHealth::Up);
    }

    #[test]
    fn up_to_degraded_on_first_error() {
        let t = tracker(5, 60_000);
        t.record_error(&api_request_error());
        match t.health() {
            ProviderHealth::Degraded {
                consecutive_errors, ..
            } => assert_eq!(consecutive_errors, 1),
            other => panic!("expected Degraded, got {other:?}"),
        }
    }

    #[test]
    fn degraded_to_up_on_success() {
        let t = tracker(5, 60_000);
        t.record_error(&api_request_error());
        t.record_success();
        assert_eq!(t.health(), ProviderHealth::Up);
    }

    #[test]
    fn degraded_to_down_after_threshold() {
        let t = tracker(3, 60_000);
        for _ in 0..3 {
            t.record_error(&server_error());
        }
        match t.health() {
            ProviderHealth::Down { reason, .. } => {
                assert_eq!(reason, DownReason::ConsecutiveFailures);
            }
            other => panic!("expected Down, got {other:?}"),
        }
    }

    #[test]
    fn down_to_degraded_after_cooldown() {
        let t = tracker(2, 1); // 1ms cooldown
        t.record_error(&api_request_error());
        t.record_error(&api_request_error());
        assert!(matches!(t.health(), ProviderHealth::Down { .. }));

        std::thread::sleep(Duration::from_millis(5));
        assert!(t.check_available().is_ok());
        assert!(matches!(t.health(), ProviderHealth::Degraded { .. }));
    }

    #[test]
    fn success_while_up_stays_up() {
        let t = tracker(5, 60_000);
        t.record_success();
        assert_eq!(t.health(), ProviderHealth::Up);
    }

    // --- Auth failure never auto-recovers ---

    #[test]
    fn auth_failure_immediate_down() {
        let t = tracker(5, 60_000);
        t.record_error(&auth_error());
        match t.health() {
            ProviderHealth::Down { reason, .. } => {
                assert_eq!(reason, DownReason::AuthFailure);
            }
            other => panic!("expected Down(AuthFailure), got {other:?}"),
        }
    }

    #[test]
    fn auth_failure_no_auto_recovery() {
        let t = tracker(5, 1); // 1ms cooldown
        t.record_error(&auth_error());
        std::thread::sleep(Duration::from_millis(5));
        assert!(t.check_available().is_err());
    }

    // --- Rate limiting ---

    #[test]
    fn rate_limit_immediate_down() {
        let t = tracker(5, 60_000);
        t.record_error(&rate_limit_error(5000));
        match t.health() {
            ProviderHealth::Down { reason, .. } => {
                assert_eq!(
                    reason,
                    DownReason::RateLimited {
                        retry_after_ms: 5000
                    }
                );
            }
            other => panic!("expected Down(RateLimited), got {other:?}"),
        }
    }

    #[test]
    fn rate_limit_recovers_after_retry_after() {
        let t = tracker(5, 60_000);
        t.record_error(&rate_limit_error(1)); // 1ms retry_after
        std::thread::sleep(Duration::from_millis(5));
        assert!(t.check_available().is_ok());
    }

    // --- Error classification ---

    #[test]
    fn parse_error_no_state_change() {
        let t = tracker(5, 60_000);
        t.record_error(&parse_error());
        assert_eq!(t.health(), ProviderHealth::Up);
    }

    #[test]
    fn server_500_increments_errors() {
        let t = tracker(5, 60_000);
        t.record_error(&server_error());
        match t.health() {
            ProviderHealth::Degraded {
                consecutive_errors, ..
            } => assert_eq!(consecutive_errors, 1),
            other => panic!("expected Degraded, got {other:?}"),
        }
    }

    // --- check_available ---

    #[test]
    fn check_available_up() {
        let t = tracker(5, 60_000);
        assert!(t.check_available().is_ok());
    }

    #[test]
    fn check_available_degraded() {
        let t = tracker(5, 60_000);
        t.record_error(&api_request_error());
        assert!(t.check_available().is_ok());
    }

    #[test]
    fn check_available_down_before_cooldown() {
        let t = tracker(2, 60_000);
        t.record_error(&api_request_error());
        t.record_error(&api_request_error());
        assert!(t.check_available().is_err());
    }

    // --- Send + Sync ---

    #[test]
    fn tracker_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<ProviderHealthTracker>();
    }
}
