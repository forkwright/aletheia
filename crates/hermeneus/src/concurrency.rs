//! Adaptive concurrency limiter for LLM provider calls.
//!
//! Combines AIMD (Additive Increase, Multiplicative Decrease) with latency-based
//! back-pressure. The limiter tracks response latency using an EWMA (Exponentially
//! Weighted Moving Average) and reduces the concurrency limit when the estimated
//! latency exceeds a configurable threshold.
//!
//! - **Increase**: on success below latency threshold, `limit += increase_step` (additive).
//! - **Decrease**: on timeout, 429, or latency above threshold, `limit = max(limit * decrease_factor, min_limit)` (multiplicative).
//! - **Recovery**: when latency drops below threshold, additive increase resumes.
//!
//! The current limit, in-flight count, and latency EWMA are exposed as Prometheus metrics.
//!
//! A tower `Layer`/`Service` wrapper (`ConcurrencyLayer`/`ConcurrencyService`)
//! is provided for middleware-style integration.

use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};
use std::time::{Duration, Instant};

use tokio::sync::Notify;
use tower::{Layer, Service};

/// Stored in `ConcurrencyPermit.outcome` as a `u8` without any `as` cast.
const OUTCOME_NEUTRAL: u8 = 0;
/// Stored in `ConcurrencyPermit.outcome` as a `u8` without any `as` cast.
const OUTCOME_SUCCESS: u8 = 1;
/// Stored in `ConcurrencyPermit.outcome` as a `u8` without any `as` cast.
const OUTCOME_OVERLOAD: u8 = 2;

/// Default EWMA smoothing factor (higher = more weight on history).
const DEFAULT_EWMA_ALPHA: f64 = 0.8;

/// Default latency threshold in seconds.
const DEFAULT_LATENCY_THRESHOLD_SECS: f64 = 30.0;

/// Outcome of a request that held a [`ConcurrencyPermit`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
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
    /// EWMA smoothing factor for latency estimation (`0.0..1.0`).
    /// Higher values weight history more heavily. Default: 0.8.
    pub ewma_alpha: f64,
    /// Latency threshold in seconds. When the EWMA latency exceeds this value,
    /// new successes are treated as overload (triggering multiplicative decrease).
    /// Default: 30.0.
    pub latency_threshold_secs: f64,
}

impl Default for ConcurrencyConfig {
    fn default() -> Self {
        Self {
            initial_limit: 10,
            min_limit: 1,
            max_limit: 200,
            increase_step: 1,
            decrease_factor: 0.9,
            ewma_alpha: DEFAULT_EWMA_ALPHA,
            latency_threshold_secs: DEFAULT_LATENCY_THRESHOLD_SECS,
        }
    }
}

struct LimiterInner {
    limit: u32,
    in_flight: u32,
    /// EWMA of response latency in seconds. `None` until the first sample.
    latency_ewma: Option<f64>,
}

/// AIMD adaptive concurrency limiter for LLM calls with latency-based back-pressure.
///
/// Callers acquire a [`ConcurrencyPermit`] before sending a request.
/// On permit release the outcome and latency are applied, adjusting the limit.
///
/// When the EWMA latency exceeds [`ConcurrencyConfig::latency_threshold_secs`],
/// successes are treated as overload and the limit decreases multiplicatively.
/// When latency drops below the threshold, additive increase resumes.
///
/// Thread-safe; `acquire` is async and parks the caller when at capacity.
///
/// # Example
///
/// ```rust,no_run
/// # use aletheia_hermeneus::concurrency::{AdaptiveConcurrencyLimiter, ConcurrencyConfig, RequestOutcome};
/// # use std::sync::Arc;
/// # use std::time::Duration;
/// # async fn example() {
/// let limiter = Arc::new(AdaptiveConcurrencyLimiter::new("anthropic", ConcurrencyConfig::default()));
/// let permit = limiter.acquire().await;
/// // … call the provider …
/// permit.finish_with_latency(RequestOutcome::Success, Duration::from_secs(2));
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
                latency_ewma: None,
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

    /// Current EWMA latency estimate in seconds, or `None` if no samples yet.
    #[must_use]
    pub fn latency_ewma(&self) -> Option<f64> {
        #[expect(
            clippy::expect_used,
            reason = "Mutex poisoning means a thread panicked; no Result to propagate"
        )]
        self.inner
            .lock()
            .expect("concurrency limiter lock poisoned")
            .latency_ewma
    }

    /// Acquire a permit, waiting asynchronously when at capacity.
    ///
    /// The permit must be consumed via [`ConcurrencyPermit::finish`] or
    /// [`ConcurrencyPermit::finish_with_latency`] to record the outcome.
    /// Dropping without calling either applies a `Neutral` outcome.
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
                        start: Instant::now(),
                    };
                }
            }

            notified.await;
        }
    }

    /// Release a permit slot and adjust the limit based on `outcome` and optional latency.
    ///
    /// Called by [`ConcurrencyPermit`] on `finish`/`finish_with_latency` or drop.
    fn release(&self, outcome: RequestOutcome, latency: Option<Duration>) {
        let (new_limit, new_in_flight, ewma) = {
            #[expect(
                clippy::expect_used,
                reason = "Mutex poisoning means a thread panicked; no Result to propagate"
            )]
            let mut inner = self
                .inner
                .lock()
                .expect("concurrency limiter lock poisoned");

            inner.in_flight = inner.in_flight.saturating_sub(1);

            // Update EWMA with the latency sample if provided.
            if let Some(dur) = latency {
                let sample = dur.as_secs_f64();
                let alpha = self.config.ewma_alpha;
                inner.latency_ewma = Some(match inner.latency_ewma {
                    Some(prev) => prev * alpha + sample * (1.0 - alpha),
                    None => sample,
                });
            }

            // Determine effective outcome: if EWMA latency exceeds threshold,
            // treat success as overload to trigger back-off.
            let effective_outcome = match outcome {
                RequestOutcome::Success => {
                    if let Some(ewma) = inner.latency_ewma {
                        if ewma > self.config.latency_threshold_secs {
                            RequestOutcome::Overload
                        } else {
                            RequestOutcome::Success
                        }
                    } else {
                        RequestOutcome::Success
                    }
                }
                other => other,
            };

            match effective_outcome {
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

            (inner.limit, inner.in_flight, inner.latency_ewma)
        };

        crate::metrics::set_concurrency_limit(&self.provider_name, new_limit);
        crate::metrics::set_concurrency_in_flight(&self.provider_name, new_in_flight);
        if let Some(ewma) = ewma {
            crate::metrics::set_concurrency_latency_ewma(&self.provider_name, ewma);
        }

        // Wake any parked callers; they will re-check the limit.
        self.notify.notify_waiters();
    }
}

/// RAII permit that holds a concurrency slot.
///
/// Call [`finish`](ConcurrencyPermit::finish) or
/// [`finish_with_latency`](ConcurrencyPermit::finish_with_latency) to record
/// the outcome explicitly. If dropped without calling either, a `Neutral`
/// outcome is applied with the elapsed time as latency.
pub struct ConcurrencyPermit {
    limiter: Arc<AdaptiveConcurrencyLimiter>,
    /// Encoded outcome; written by `finish`, read by `Drop`.
    outcome: AtomicU8,
    /// Set to 1 once released so `Drop` does not double-release.
    released: AtomicU8,
    /// When the permit was acquired, used for automatic latency measurement.
    start: Instant,
}

impl ConcurrencyPermit {
    /// Record the request outcome and release the slot.
    ///
    /// Uses the elapsed time since permit acquisition as the latency sample.
    /// Consumes the permit so `Drop` will not release a second time.
    pub fn finish(self, outcome: RequestOutcome) {
        let latency = self.start.elapsed();
        self.finish_inner(outcome, Some(latency));
    }

    /// Record the request outcome with an explicit latency and release the slot.
    ///
    /// Use this when the caller measures latency separately (e.g., excluding
    /// queue wait time).
    pub fn finish_with_latency(self, outcome: RequestOutcome, latency: Duration) {
        self.finish_inner(outcome, Some(latency));
    }

    fn finish_inner(self, outcome: RequestOutcome, latency: Option<Duration>) {
        let code = match outcome {
            RequestOutcome::Success => OUTCOME_SUCCESS,
            RequestOutcome::Overload => OUTCOME_OVERLOAD,
            RequestOutcome::Neutral => OUTCOME_NEUTRAL,
        };
        self.outcome.store(code, Ordering::Relaxed);
        self.released.store(1, Ordering::Relaxed);
        self.limiter.release(outcome, latency);
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
            let latency = self.start.elapsed();
            self.limiter.release(outcome, Some(latency));
        }
    }
}

// ---------------------------------------------------------------------------
// Tower Layer / Service
// ---------------------------------------------------------------------------

/// Tower `Layer` that wraps an inner service with adaptive concurrency limiting.
///
/// Acquires a [`ConcurrencyPermit`] before forwarding each request, measures
/// response latency, and feeds it back to the limiter. The inner service's
/// error type determines the outcome: errors classified as retryable map to
/// [`RequestOutcome::Overload`], others to [`Neutral`](RequestOutcome::Neutral).
///
/// # Type parameters
///
/// The layer itself is generic over the inner service type; the service is
/// determined when [`layer`](Layer::layer) is called.
#[derive(Clone)]
pub struct ConcurrencyLayer {
    limiter: Arc<AdaptiveConcurrencyLimiter>,
}

impl ConcurrencyLayer {
    /// Create a layer backed by the given limiter.
    #[must_use]
    pub fn new(limiter: Arc<AdaptiveConcurrencyLimiter>) -> Self {
        Self { limiter }
    }
}

impl<S> Layer<S> for ConcurrencyLayer {
    type Service = ConcurrencyService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        ConcurrencyService {
            inner,
            limiter: Arc::clone(&self.limiter),
        }
    }
}

/// Tower `Service` that enforces adaptive concurrency limits.
///
/// Created by [`ConcurrencyLayer::layer`]. Acquires a permit before each
/// request, measures latency, and classifies the result to update the limiter.
#[derive(Clone)]
pub struct ConcurrencyService<S> {
    inner: S,
    limiter: Arc<AdaptiveConcurrencyLimiter>,
}

impl<S> ConcurrencyService<S> {
    /// Access the underlying limiter for metrics inspection.
    #[must_use]
    pub fn limiter(&self) -> &Arc<AdaptiveConcurrencyLimiter> {
        &self.limiter
    }
}

impl<S, Req> Service<Req> for ConcurrencyService<S>
where
    S: Service<Req> + Clone + Send + 'static,
    S::Future: Send,
    S::Response: Send + 'static,
    S::Error: Send + 'static,
    Req: Send + 'static,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = Pin<Box<dyn Future<Output = Result<S::Response, S::Error>> + Send>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Req) -> Self::Future {
        let limiter = Arc::clone(&self.limiter);
        let mut inner = self.inner.clone();

        Box::pin(async move {
            let permit = limiter.acquire().await;

            match inner.call(req).await {
                Ok(resp) => {
                    permit.finish(RequestOutcome::Success);
                    Ok(resp)
                }
                Err(err) => {
                    // Default to Overload for all errors; callers needing finer
                    // classification can use the permit API directly instead of
                    // the tower layer.
                    permit.finish(RequestOutcome::Overload);
                    Err(err)
                }
            }
        })
    }
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
mod tests {
    use std::sync::Arc;
    use std::time::Duration;

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
                ..ConcurrencyConfig::default()
            },
        ))
    }

    fn limiter_with_threshold(
        initial: u32,
        threshold_secs: f64,
    ) -> Arc<AdaptiveConcurrencyLimiter> {
        Arc::new(AdaptiveConcurrencyLimiter::new(
            "test",
            ConcurrencyConfig {
                initial_limit: initial,
                min_limit: 1,
                max_limit: 200,
                increase_step: 1,
                decrease_factor: 0.5,
                ewma_alpha: 0.0, // alpha=0: EWMA = latest sample only (for test determinism)
                latency_threshold_secs: threshold_secs,
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

    // -----------------------------------------------------------------------
    // Latency EWMA tests
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn latency_ewma_initialized_on_first_sample() {
        let l = limiter_with_threshold(10, 5.0);
        assert!(l.latency_ewma().is_none(), "no EWMA before first sample");

        let permit = l.acquire().await;
        permit.finish_with_latency(RequestOutcome::Success, Duration::from_secs(2));

        let ewma = l.latency_ewma().unwrap();
        assert!(
            (ewma - 2.0).abs() < 0.01,
            "first sample should seed the EWMA: got {ewma}"
        );
    }

    #[tokio::test]
    async fn latency_ewma_updates_with_alpha() {
        // alpha = 0 means EWMA = latest sample only
        let l = limiter_with_threshold(10, 100.0);
        let permit = l.acquire().await;
        permit.finish_with_latency(RequestOutcome::Success, Duration::from_secs(10));
        assert!((l.latency_ewma().unwrap() - 10.0).abs() < 0.01);

        let permit = l.acquire().await;
        permit.finish_with_latency(RequestOutcome::Success, Duration::from_secs(20));
        // alpha=0 → EWMA = prev*0 + 20*(1-0) = 20
        assert!((l.latency_ewma().unwrap() - 20.0).abs() < 0.01);
    }

    #[tokio::test]
    async fn high_latency_triggers_backoff() {
        // Threshold = 5s, initial limit = 10.
        let l = limiter_with_threshold(10, 5.0);

        // First request: latency 2s (below threshold) → success → limit increases.
        let permit = l.acquire().await;
        permit.finish_with_latency(RequestOutcome::Success, Duration::from_secs(2));
        assert_eq!(l.limit(), 11, "below threshold: limit should increase");

        // Second request: latency 10s (above threshold) → treated as overload.
        let permit = l.acquire().await;
        permit.finish_with_latency(RequestOutcome::Success, Duration::from_secs(10));
        // 11 * 0.5 = 5 (floor)
        assert_eq!(l.limit(), 5, "above threshold: limit should decrease");
    }

    #[tokio::test]
    async fn latency_recovery_resumes_increase() {
        // Threshold = 5s, initial limit = 10.
        let l = limiter_with_threshold(10, 5.0);

        // Push latency above threshold → back off.
        let permit = l.acquire().await;
        permit.finish_with_latency(RequestOutcome::Success, Duration::from_secs(10));
        let after_backoff = l.limit();
        assert!(
            after_backoff < 10,
            "limit should have decreased: got {after_backoff}"
        );

        // Latency drops below threshold → additive increase resumes.
        let permit = l.acquire().await;
        permit.finish_with_latency(RequestOutcome::Success, Duration::from_secs(1));
        assert_eq!(
            l.limit(),
            after_backoff + 1,
            "below threshold: limit should increase from {after_backoff}"
        );
    }

    #[tokio::test]
    async fn explicit_overload_still_decreases_regardless_of_latency() {
        // Even with low latency, explicit Overload should decrease the limit.
        let l = limiter_with_threshold(10, 100.0);
        let permit = l.acquire().await;
        permit.finish_with_latency(RequestOutcome::Overload, Duration::from_secs(1));
        assert_eq!(l.limit(), 5, "explicit overload must decrease limit");
    }

    // -----------------------------------------------------------------------
    // Tower Layer / Service tests
    // -----------------------------------------------------------------------

    /// Minimal tower service for testing.
    #[derive(Clone)]
    struct EchoService;

    impl Service<String> for EchoService {
        type Response = String;
        type Error = std::convert::Infallible;
        type Future = Pin<Box<dyn Future<Output = Result<String, Self::Error>> + Send>>;

        fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
            Poll::Ready(Ok(()))
        }

        fn call(&mut self, req: String) -> Self::Future {
            Box::pin(async move { Ok(req) })
        }
    }

    #[derive(Clone)]
    struct FailService;

    impl Service<String> for FailService {
        type Response = String;
        type Error = String;
        type Future = Pin<Box<dyn Future<Output = Result<String, String>> + Send>>;

        fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
            Poll::Ready(Ok(()))
        }

        fn call(&mut self, _req: String) -> Self::Future {
            Box::pin(async { Err("test error".to_owned()) })
        }
    }

    #[tokio::test]
    async fn layer_wraps_service_and_tracks_limit() {
        let lim = Arc::new(AdaptiveConcurrencyLimiter::new(
            "test",
            ConcurrencyConfig {
                initial_limit: 5,
                ..ConcurrencyConfig::default()
            },
        ));
        let layer = ConcurrencyLayer::new(Arc::clone(&lim));
        let mut svc = layer.layer(EchoService);

        let resp = svc.call("hello".to_owned()).await.unwrap();
        assert_eq!(resp, "hello");
        // Success should have increased the limit.
        assert!(lim.limit() > 5, "limit should increase after success");
    }

    #[tokio::test]
    async fn layer_decreases_limit_on_error() {
        let lim = Arc::new(AdaptiveConcurrencyLimiter::new(
            "test",
            ConcurrencyConfig {
                initial_limit: 10,
                decrease_factor: 0.5,
                ..ConcurrencyConfig::default()
            },
        ));
        let layer = ConcurrencyLayer::new(Arc::clone(&lim));
        let mut svc = layer.layer(FailService);

        let result: Result<String, String> = svc.call("hello".to_owned()).await;
        assert!(result.is_err());
        assert_eq!(lim.limit(), 5, "error should decrease limit");
    }

    #[test]
    fn layer_is_clone_send() {
        fn assert_clone_send<T: Clone + Send>() {}
        assert_clone_send::<ConcurrencyLayer>();
    }
}
