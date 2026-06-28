//! Provider health state machine.
//!
//! Tracks availability per provider through Up / Degraded / Down / Probing
//! states. Transitions happen on success, transient errors (server 5xx, rate
//! limits), and fatal errors (auth failure). Cooldown timers allow automatic
//! recovery from transient failures; auth failures require manual intervention.
//!
//! The `Probing` state is the single-flight recovery gate: after a `Down`
//! provider's cooldown elapses, exactly one caller is allowed through to test
//! the provider. Concurrent callers fail fast until the probe succeeds or the
//! provider goes `Down` again.

// WHY: std::sync::Mutex is correct here -- lock is held only during brief state reads/writes, never across .await
use std::sync::Mutex; // kanon:ignore RUST/std-mutex-in-async

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
    /// Single probe in flight to test whether a `Down` provider has recovered.
    ///
    /// Only the caller that transitioned `Down -> Probing` may send a request;
    /// all other callers fail fast until the probe resolves to `Up` or `Down`.
    Probing {
        /// When the probe started.
        since: Timestamp,
        /// Original reason the provider went `Down`.
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
    // kanon:ignore RUST/pub-visibility
    inner: Mutex<TrackerInner>,
    config: HealthConfig,
}

impl ProviderHealthTracker {
    /// Create a new tracker starting in `Up` state.
    #[must_use]
    pub fn new(config: HealthConfig) -> Self {
        // kanon:ignore RUST/pub-visibility
        Self {
            inner: Mutex::new(TrackerInner {
                health: ProviderHealth::Up,
                total_requests: 0,
                total_errors: 0,
            }),
            config,
        }
    }

    fn lock_inner(&self) -> std::sync::MutexGuard<'_, TrackerInner> {
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Current health state.
    #[must_use]
    pub fn health(&self) -> ProviderHealth {
        // kanon:ignore RUST/pub-visibility
        self.lock_inner().health.clone()
    }

    /// Check if the provider can accept a request.
    ///
    /// Returns `Ok(())` if `Up` or `Degraded`. Returns `Err(health)` if `Down`,
    /// unless the cooldown has elapsed — in which case the caller is elected as
    /// the single probe (`Down -> Probing`). While a probe is in flight, all
    /// other callers fail fast. `Down(AuthFailure)` never auto-recovers.
    #[must_use = "caller must handle provider unavailability"]
    pub fn check_available(&self) -> Result<(), ProviderHealth> {
        // kanon:ignore RUST/pub-visibility
        let mut inner = self.lock_inner();
        match &inner.health {
            ProviderHealth::Up | ProviderHealth::Degraded { .. } => Ok(()),
            ProviderHealth::Probing { .. } => Err(inner.health.clone()),
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
                    // WHY: Elect exactly one caller as the recovery probe.
                    // Subsequent callers see `Probing` and fail fast until the
                    // probe records success or error.
                    inner.health = ProviderHealth::Probing {
                        since: Timestamp::now(),
                        reason: reason.clone(),
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
        // kanon:ignore RUST/pub-visibility
        let mut inner = self.lock_inner();
        inner.total_requests += 1;
        match inner.health {
            ProviderHealth::Degraded { .. } | ProviderHealth::Probing { .. } => {
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
        // kanon:ignore RUST/pub-visibility
        let mut inner = self.lock_inner();
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
            // WHY: ProviderInit/SubprocessFailure errors (e.g. CLI binary
            // disappeared, spawn failed, or timed out)
            // indicate the provider process is unavailable. They must follow the
            // same degradation path as ApiRequest errors so the health tracker
            // transitions to Degraded/Down and subsequent requests get a clear
            // 503 instead of repeated spawn failures.
            Error::ProviderInit { .. }
            | Error::SubprocessFailure { .. }
            | Error::ApiRequest { .. }
            | Error::ApiError {
                status: 500..=599, ..
            } => {
                let now = Timestamp::now();
                match &inner.health {
                    ProviderHealth::Up => {
                        if 1 >= self.config.consecutive_failure_threshold {
                            inner.health = ProviderHealth::Down {
                                since: now,
                                reason: DownReason::ConsecutiveFailures,
                            };
                        } else {
                            inner.health = ProviderHealth::Degraded {
                                consecutive_errors: 1,
                                last_error_at: now,
                            };
                        }
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
                    ProviderHealth::Probing { .. } => {
                        // WHY: Probe failed with an availability error. Reopen
                        // the circuit with a fresh timestamp so the next probe
                        // waits the full cooldown.
                        inner.health = ProviderHealth::Down {
                            since: Timestamp::now(),
                            reason: DownReason::ConsecutiveFailures,
                        };
                    }
                }
            }
            _ => {
                // NOTE: non-availability errors (parse, unsupported model) do not affect health
                // unless a probe is in flight: an unresolved probe must not stay
                // stuck in `Probing`.
                if matches!(inner.health, ProviderHealth::Probing { .. }) {
                    inner.health = ProviderHealth::Down {
                        since: Timestamp::now(),
                        reason: DownReason::ConsecutiveFailures,
                    };
                }
            }
        }
    }
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
// WHY: std::thread::sleep required because ProviderHealthTracker uses jiff::Timestamp::now() (real wall clock), // kanon:ignore TESTING/sleep-in-test
// not tokio's controllable clock. tokio::time::pause/advance would not affect jiff timestamps.
mod tests {
    use super::*;
    use crate::error::ApiErrorContext;

    fn tracker(threshold: u32, cooldown_ms: u64) -> ProviderHealthTracker {
        ProviderHealthTracker::new(HealthConfig {
            consecutive_failure_threshold: threshold,
            down_cooldown_ms: cooldown_ms,
        })
    }

    use std::sync::Arc;
    use std::time::Duration;

    use snafu::IntoError;

    fn api_request_error() -> Error {
        crate::error::ApiRequestSnafu { message: "timeout" }.build()
    }

    fn server_error() -> Error {
        crate::error::ApiSnafu {
            status: 500_u16,
            message: "internal",
            context: ApiErrorContext::empty(),
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
    fn down_to_probing_after_cooldown() {
        let t = tracker(2, 1); // 1ms cooldown
        t.record_error(&api_request_error());
        t.record_error(&api_request_error());
        assert!(matches!(t.health(), ProviderHealth::Down { .. }));

        std::thread::sleep(Duration::from_millis(5)); // kanon:ignore TESTING/sleep-in-test
        assert!(t.check_available().is_ok());
        assert!(matches!(t.health(), ProviderHealth::Probing { .. }));
    }

    #[test]
    fn probing_blocks_concurrent_requests() {
        let t = tracker(1, 1); // 1ms cooldown, threshold 1
        t.record_error(&api_request_error());
        t.record_error(&api_request_error());
        assert!(matches!(t.health(), ProviderHealth::Down { .. }));

        std::thread::sleep(Duration::from_millis(5)); // kanon:ignore TESTING/sleep-in-test
        let tracker = Arc::new(t);
        let mut handles = Vec::new();
        for _ in 0..10 {
            let tracker = Arc::clone(&tracker);
            handles.push(std::thread::spawn(move || tracker.check_available()));
        }

        let results: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
        let ok_count = results.iter().filter(|r| r.is_ok()).count();
        let err_count = results.iter().filter(|r| r.is_err()).count();

        assert_eq!(ok_count, 1, "exactly one concurrent caller may probe");
        assert_eq!(err_count, 9, "remaining concurrent callers must fail fast");
        assert!(
            matches!(tracker.health(), ProviderHealth::Probing { .. }),
            "tracker must remain in Probing after electing the probe"
        );
    }

    #[test]
    fn probing_to_up_on_success() {
        let t = tracker(1, 1); // 1ms cooldown
        t.record_error(&api_request_error());
        t.record_error(&api_request_error());
        std::thread::sleep(Duration::from_millis(5)); // kanon:ignore TESTING/sleep-in-test
        assert!(t.check_available().is_ok());
        t.record_success();
        assert_eq!(t.health(), ProviderHealth::Up);
    }

    #[test]
    fn probing_to_down_on_availability_error() {
        let t = tracker(1, 1); // 1ms cooldown
        t.record_error(&api_request_error());
        t.record_error(&api_request_error());
        std::thread::sleep(Duration::from_millis(5)); // kanon:ignore TESTING/sleep-in-test
        assert!(t.check_available().is_ok());
        t.record_error(&server_error());
        assert!(
            matches!(t.health(), ProviderHealth::Down { .. }),
            "failed probe must reopen the circuit"
        );
    }

    #[test]
    fn probing_to_down_on_non_availability_error() {
        // WHY: A probe must resolve. Non-availability errors (e.g. parse) must
        // not leave the tracker stuck in `Probing`.
        let t = tracker(1, 1); // 1ms cooldown
        t.record_error(&api_request_error());
        t.record_error(&api_request_error());
        std::thread::sleep(Duration::from_millis(5)); // kanon:ignore TESTING/sleep-in-test
        assert!(t.check_available().is_ok());
        t.record_error(&parse_error());
        assert!(
            matches!(t.health(), ProviderHealth::Down { .. }),
            "unresolved probe must reopen the circuit even for non-availability errors"
        );
    }

    #[test]
    fn success_while_up_stays_up() {
        let t = tracker(5, 60_000);
        t.record_success();
        assert_eq!(t.health(), ProviderHealth::Up);
    }

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
        std::thread::sleep(Duration::from_millis(5)); // kanon:ignore TESTING/sleep-in-test
        assert!(t.check_available().is_err());
    }

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
        std::thread::sleep(Duration::from_millis(5)); // kanon:ignore TESTING/sleep-in-test
        assert!(t.check_available().is_ok());
    }

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

    #[test]
    fn provider_init_error_transitions_to_degraded() {
        // WHY: ProviderInit errors (CC binary crashed, disappeared) must
        // update health state so the circuit breaker activates and
        // subsequent requests get a clear 503 instead of repeated spawn failures.
        let t = tracker(5, 60_000);
        let err = crate::error::ProviderInitSnafu {
            message: "failed to spawn claude CLI",
        }
        .build();
        t.record_error(&err);
        match t.health() {
            ProviderHealth::Degraded {
                consecutive_errors, ..
            } => assert_eq!(consecutive_errors, 1),
            other => panic!("expected Degraded after ProviderInit error, got {other:?}"),
        }
    }

    #[test]
    fn provider_init_errors_transition_to_down_at_threshold() {
        // WHY: Repeated ProviderInit errors (CC binary remains unavailable)
        // must eventually transition to Down so resolve_provider_checked
        // rejects requests early with a clear error.
        let t = tracker(3, 60_000);
        let err = crate::error::ProviderInitSnafu {
            message: "failed to spawn claude CLI",
        }
        .build();
        for _ in 0..3 {
            t.record_error(&err);
        }
        match t.health() {
            ProviderHealth::Down { reason, .. } => {
                assert_eq!(reason, DownReason::ConsecutiveFailures);
            }
            other => panic!("expected Down after threshold ProviderInit errors, got {other:?}"),
        }
    }

    #[test] // kanon:ignore TESTING/tautological-test — compile-time Send+Sync bound check; compilation itself is the assertion
    fn tracker_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<ProviderHealthTracker>();
    }
}
