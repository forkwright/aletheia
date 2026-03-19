//! Adaptive concurrency limiter for LLM provider calls.
//!
//! Uses the AIMD (Additive Increase, Multiplicative Decrease) algorithm:
//! - **Increase**: on success, `limit += increase_step` (additive).
//! - **Decrease**: on timeout or 429, `limit = max(limit * decrease_factor, min_limit)` (multiplicative).
//!
//! The current limit and in-flight count are exposed as Prometheus metrics.

use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, Mutex};

use tokio::sync::Notify;

/// Stored in `ConcurrencyPermit.outcome` as a `u8` without any `as` cast.
const OUTCOME_NEUTRAL: u8 = 0;
/// Stored in `ConcurrencyPermit.outcome` as a `u8` without any `as` cast.
const OUTCOME_SUCCESS: u8 = 1;
/// Stored in `ConcurrencyPermit.outcome` as a `u8` without any `as` cast.
const OUTCOME_OVERLOAD: u8 = 2;

/// Outcome of a request that held a [`ConcurrencyPermit`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RequestOutcome {
    /// Request succeeded; increase the limit.
    Success,
    /// Request timed out or received 429; decrease the limit.
    Overload,
    /// Request was cancelled or outcome is unknown; no limit adjustment.
    Neutral,
}

/// Configuration for the [`AdaptiveConcurrencyLimiter`].
#[derive(Debug, Clone)]
pub struct ConcurrencyConfig {
    /// Starting concurrency limit. Default: 10.
    pub initial_limit: u32,
    /// Minimum concurrency limit (floor). Default: 1.
    pub min_limit: u32,
    /// Maximum concurrency limit (ceiling). Default: 200.
    pub max_limit: u32,
    /// Additive increase step on success. Default: 1.
    pub increase_step: u32,
    /// Multiplicative decrease factor on overload (must be in `(0.0, 1.0)`). Default: 0.9.
    pub decrease_factor: f64,
}

impl Default for ConcurrencyConfig {
    fn default() -> Self {
        Self {
            initial_limit: 10,
            min_limit: 1,
            max_limit: 200,
            increase_step: 1,
            decrease_factor: 0.9,
        }
    }
}

struct LimiterInner {
    limit: u32,
    in_flight: u32,
}

/// AIMD adaptive concurrency limiter for LLM calls.
///
/// Callers acquire a [`ConcurrencyPermit`] before sending a request.
/// On permit release the outcome is applied, adjusting the limit.
///
/// Thread-safe; `acquire` is async and parks the caller when at capacity.
///
/// # Example
///
/// ```rust,no_run
/// # use aletheia_hermeneus::concurrency::{AdaptiveConcurrencyLimiter, ConcurrencyConfig, RequestOutcome};
/// # use std::sync::Arc;
/// # async fn example() {
/// let limiter = Arc::new(AdaptiveConcurrencyLimiter::new("anthropic", ConcurrencyConfig::default()));
/// let permit = limiter.acquire().await;
/// // … call the provider …
/// permit.finish(RequestOutcome::Success);
/// # }
/// ```
pub struct AdaptiveConcurrencyLimiter {
    inner: Mutex<LimiterInner>,
    notify: Notify,
    config: ConcurrencyConfig,
    provider_name: String,
}

impl AdaptiveConcurrencyLimiter {
    /// Create a new limiter starting at `config.initial_limit`.
    #[must_use]
    pub fn new(provider_name: impl Into<String>, config: ConcurrencyConfig) -> Self {
        let initial = config.initial_limit;
        let name = provider_name.into();
        let limiter = Self {
            inner: Mutex::new(LimiterInner {
                limit: initial,
                in_flight: 0,
            }),
            notify: Notify::new(),
            config,
            provider_name: name,
        };
        crate::metrics::set_concurrency_limit(&limiter.provider_name, initial);
        limiter
    }

    /// Create a new limiter with default configuration.
    #[must_use]
    pub fn with_defaults(provider_name: impl Into<String>) -> Self {
        Self::new(provider_name, ConcurrencyConfig::default())
    }

    /// Current concurrency limit (snapshot).
    #[must_use]
    pub fn limit(&self) -> u32 {
        #[expect(
            clippy::expect_used,
            reason = "Mutex poisoning means a thread panicked; no Result to propagate"
        )]
        self.inner
            .lock()
            .expect("concurrency limiter lock poisoned")
            .limit
    }

    /// Current number of in-flight requests (snapshot).
    #[must_use]
    pub fn in_flight(&self) -> u32 {
        #[expect(
            clippy::expect_used,
            reason = "Mutex poisoning means a thread panicked; no Result to propagate"
        )]
        self.inner
            .lock()
            .expect("concurrency limiter lock poisoned")
            .in_flight
    }

    /// Acquire a permit, waiting asynchronously when at capacity.
    ///
    /// The permit must be consumed via [`ConcurrencyPermit::finish`] to record
    /// the outcome. Dropping without calling `finish` applies a `Neutral` outcome.
    pub async fn acquire(self: &Arc<Self>) -> ConcurrencyPermit {
        loop {
            // Create the notified future *before* inspecting state to avoid
            // missing a notification between the check and the await.
            let notified = self.notify.notified();

            {
                #[expect(
                    clippy::expect_used,
                    reason = "Mutex poisoning means a thread panicked; no Result to propagate"
                )]
                let mut inner = self
                    .inner
                    .lock()
                    .expect("concurrency limiter lock poisoned");
                if inner.in_flight < inner.limit {
                    inner.in_flight += 1;
                    crate::metrics::set_concurrency_in_flight(&self.provider_name, inner.in_flight);
                    return ConcurrencyPermit {
                        limiter: Arc::clone(self),
                        outcome: AtomicU8::new(OUTCOME_NEUTRAL),
                        released: AtomicU8::new(0),
                    };
                }
            }

            notified.await;
        }
    }

    /// Release a permit slot and adjust the limit based on `outcome`.
    ///
    /// Called by [`ConcurrencyPermit`] on `finish` or drop.
    fn release(&self, outcome: RequestOutcome) {
        let (new_limit, new_in_flight) = {
            #[expect(
                clippy::expect_used,
                reason = "Mutex poisoning means a thread panicked; no Result to propagate"
            )]
            let mut inner = self
                .inner
                .lock()
                .expect("concurrency limiter lock poisoned");

            inner.in_flight = inner.in_flight.saturating_sub(1);

            match outcome {
                RequestOutcome::Success => {
                    inner.limit =
                        (inner.limit + self.config.increase_step).min(self.config.max_limit);
                }
                RequestOutcome::Overload => {
                    // AIMD multiplicative decrease: floor(limit * decrease_factor).
                    // f64::from(u32) is lossless; all u32 values fit in f64 mantissa.
                    let limit_f64 = f64::from(inner.limit);
                    let decreased_f64 = (limit_f64 * self.config.decrease_factor).floor();
                    #[expect(
                        clippy::cast_possible_truncation,
                        clippy::cast_sign_loss,
                        clippy::as_conversions,
                        reason = "decreased_f64 is non-negative and bounded by inner.limit (a u32)"
                    )]
                    let decreased = decreased_f64 as u32;
                    inner.limit = decreased.max(self.config.min_limit);
                }
                RequestOutcome::Neutral => {}
            }

            (inner.limit, inner.in_flight)
        };

        crate::metrics::set_concurrency_limit(&self.provider_name, new_limit);
        crate::metrics::set_concurrency_in_flight(&self.provider_name, new_in_flight);

        // Wake any parked callers; they will re-check the limit.
        self.notify.notify_waiters();
    }
}

/// RAII permit that holds a concurrency slot.
///
/// Call [`finish`](ConcurrencyPermit::finish) to record the outcome explicitly.
/// If dropped without calling `finish`, a `Neutral` outcome is applied (no limit
/// change, slot is still released).
pub struct ConcurrencyPermit {
    limiter: Arc<AdaptiveConcurrencyLimiter>,
    /// Encoded outcome; written by `finish`, read by `Drop`.
    outcome: AtomicU8,
    /// Set to 1 once released so `Drop` does not double-release.
    released: AtomicU8,
}

impl ConcurrencyPermit {
    /// Record the request outcome and release the slot.
    ///
    /// Consumes the permit so `Drop` will not release a second time.
    pub fn finish(self, outcome: RequestOutcome) {
        let code = match outcome {
            RequestOutcome::Success => OUTCOME_SUCCESS,
            RequestOutcome::Overload => OUTCOME_OVERLOAD,
            RequestOutcome::Neutral => OUTCOME_NEUTRAL,
        };
        self.outcome.store(code, Ordering::Relaxed);
        self.released.store(1, Ordering::Relaxed);
        self.limiter.release(outcome);
        // Prevent Drop from releasing a second time.
        std::mem::forget(self);
    }
}

impl Drop for ConcurrencyPermit {
    fn drop(&mut self) {
        if self.released.load(Ordering::Relaxed) == 0 {
            let outcome = match self.outcome.load(Ordering::Relaxed) {
                OUTCOME_SUCCESS => RequestOutcome::Success,
                OUTCOME_OVERLOAD => RequestOutcome::Overload,
                _ => RequestOutcome::Neutral,
            };
            self.limiter.release(outcome);
        }
    }
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
mod tests {
    use std::sync::Arc;

    use super::*;

    fn limiter(initial: u32, min: u32, max: u32) -> Arc<AdaptiveConcurrencyLimiter> {
        Arc::new(AdaptiveConcurrencyLimiter::new(
            "test",
            ConcurrencyConfig {
                initial_limit: initial,
                min_limit: min,
                max_limit: max,
                increase_step: 1,
                decrease_factor: 0.5,
            },
        ))
    }

    #[tokio::test]
    async fn acquire_and_release_success() {
        let l = limiter(5, 1, 10);
        assert_eq!(l.in_flight(), 0);
        let permit = l.acquire().await;
        assert_eq!(l.in_flight(), 1);
        let limit_before = l.limit();
        permit.finish(RequestOutcome::Success);
        assert_eq!(l.in_flight(), 0);
        assert_eq!(l.limit(), limit_before + 1);
    }

    #[tokio::test]
    async fn overload_decreases_limit() {
        let l = limiter(10, 1, 20);
        let permit = l.acquire().await;
        permit.finish(RequestOutcome::Overload);
        // 10 * 0.5 = 5
        assert_eq!(l.limit(), 5);
    }

    #[tokio::test]
    async fn neutral_does_not_change_limit() {
        let l = limiter(10, 1, 20);
        let permit = l.acquire().await;
        let before = l.limit();
        permit.finish(RequestOutcome::Neutral);
        assert_eq!(l.limit(), before);
    }

    #[tokio::test]
    async fn drop_without_finish_releases_slot() {
        let l = limiter(5, 1, 10);
        {
            let _permit = l.acquire().await;
            assert_eq!(l.in_flight(), 1);
        } // drop applies Neutral
        assert_eq!(l.in_flight(), 0);
    }

    #[tokio::test]
    async fn limit_floors_at_min() {
        let l = limiter(1, 1, 10);
        let permit = l.acquire().await;
        permit.finish(RequestOutcome::Overload);
        // floor(1 * 0.5) = 0 → max(0, 1) = 1
        assert_eq!(l.limit(), 1);
    }

    #[tokio::test]
    async fn limit_caps_at_max() {
        let l = limiter(9, 1, 10);
        let permit = l.acquire().await;
        permit.finish(RequestOutcome::Success);
        assert_eq!(l.limit(), 10);
        // Another success should not exceed max.
        let permit = l.acquire().await;
        permit.finish(RequestOutcome::Success);
        assert_eq!(l.limit(), 10);
    }

    #[tokio::test]
    async fn blocks_when_at_capacity_then_unblocks() {
        let l = limiter(1, 1, 10);
        let permit = l.acquire().await;
        assert_eq!(l.in_flight(), 1);

        let l2 = Arc::clone(&l);
        let waiter = tokio::spawn(async move { l2.acquire().await });

        // Give the waiter a moment to park.
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
        assert!(!waiter.is_finished());

        // Release the first permit; waiter should unblock.
        permit.finish(RequestOutcome::Neutral);
        let second_permit = waiter.await.unwrap();
        assert_eq!(l.in_flight(), 1);
        second_permit.finish(RequestOutcome::Neutral);
        assert_eq!(l.in_flight(), 0);
    }

    #[tokio::test]
    async fn multiple_permits_up_to_limit() {
        let l = limiter(3, 1, 10);
        let p1 = l.acquire().await;
        let p2 = l.acquire().await;
        let p3 = l.acquire().await;
        assert_eq!(l.in_flight(), 3);
        p1.finish(RequestOutcome::Neutral);
        p2.finish(RequestOutcome::Neutral);
        p3.finish(RequestOutcome::Neutral);
        assert_eq!(l.in_flight(), 0);
    }

    #[test]
    fn limiter_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<AdaptiveConcurrencyLimiter>();
        assert_send_sync::<ConcurrencyPermit>();
    }
}
