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
use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};
use std::task::{Context, Poll};
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use tokio::sync::Notify;
use tower::{Layer, Service};

use crate::types::CompletionResponse;

// Encoded request outcomes stored in `ConcurrencyPermit::outcome` as `u8`.
const OUTCOME_NEUTRAL: u8 = 0;
const OUTCOME_SUCCESS: u8 = 1;
const OUTCOME_OVERLOAD: u8 = 2;

/// Default EWMA smoothing factor (higher = more weight on history).
pub const DEFAULT_EWMA_ALPHA: f64 = 0.8;

/// Default latency threshold in seconds.
pub const DEFAULT_LATENCY_THRESHOLD_SECS: f64 = 30.0;

/// Fallback saturation retry hint in milliseconds, used before the limiter
/// has observed any latency sample.
pub const DEFAULT_SATURATION_RETRY_HINT_MS: u64 = 1_000;

/// How the limiter admits new requests (#7152).
///
/// `Adaptive` is the historical behavior: an AIMD-adjusted limit with
/// unbounded parking for callers above it. `Fixed` is the hard admission
/// bound required for localhosted model servers: at most `max_running`
/// requests run, at most `max_waiting` park, and any excess acquisition is
/// refused immediately with a typed
/// [`Error::ProviderSaturated`](crate::error::Error::ProviderSaturated)
/// carrying retry guidance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, Default)]
#[serde(tag = "mode", rename_all = "lowercase")]
#[non_exhaustive]
pub enum AdmissionPolicy {
    /// AIMD adaptive limit; waiters park until a slot frees up.
    #[default]
    Adaptive,
    /// Hard cap: the limit is pinned to `max_running` (AIMD feedback does
    /// not move it) and at most `max_waiting` callers may wait for a slot.
    Fixed {
        /// Maximum concurrently running requests.
        max_running: u32,
        /// Maximum parked waiters before excess work is refused.
        max_waiting: u32,
    },
}

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

/// Classify a provider call result into a [`RequestOutcome`] for concurrency feedback.
///
/// Success increases the limit. A retryable error (per
/// [`Error::is_retryable`](crate::error::Error::is_retryable)) signals overload
/// and decreases it. Any other error is neutral and leaves the limit unchanged,
/// since it is not evidence the endpoint itself is overloaded.
#[must_use]
pub fn concurrency_outcome(result: &crate::error::Result<CompletionResponse>) -> RequestOutcome {
    // kanon:ignore RUST/pub-visibility
    match result {
        Ok(_) => RequestOutcome::Success,
        Err(err) if err.is_retryable() => RequestOutcome::Overload,
        Err(_) => RequestOutcome::Neutral,
    }
}

/// Configuration for the [`AdaptiveConcurrencyLimiter`].
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
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
    /// Admission policy (#7152). Default: [`AdmissionPolicy::Adaptive`],
    /// preserving the historical AIMD behavior. Localhosted model servers
    /// use [`AdmissionPolicy::Fixed`] so excess work is refused with typed
    /// retry guidance instead of parking unboundedly.
    #[serde(default)]
    pub admission: AdmissionPolicy,
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
            admission: AdmissionPolicy::Adaptive,
        }
    }
}

struct LimiterInner {
    limit: u32,
    in_flight: u32,
    /// Parked callers counted by the fixed admission policy. Always 0 under
    /// the adaptive policy, whose waiters are unbounded and untracked.
    waiting: u32,
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
/// # use hermeneus::concurrency::{AdaptiveConcurrencyLimiter, ConcurrencyConfig, RequestOutcome};
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
    // kanon:ignore RUST/pub-visibility
    inner: Mutex<LimiterInner>,
    notify: Notify,
    config: ConcurrencyConfig,
    provider_name: String,
}

impl AdaptiveConcurrencyLimiter {
    /// Create a new limiter starting at `config.initial_limit`.
    #[must_use]
    pub fn new(provider_name: impl Into<String>, config: ConcurrencyConfig) -> Self {
        // kanon:ignore RUST/pub-visibility
        // WHY(#7152): a fixed admission bound pins the limit to max_running;
        // initial_limit only seeds the adaptive policy.
        let initial = match config.admission {
            AdmissionPolicy::Fixed { max_running, .. } => max_running.max(1),
            _ => config.initial_limit,
        };
        let name = provider_name.into();
        let limiter = Self {
            inner: Mutex::new(LimiterInner {
                limit: initial,
                in_flight: 0,
                waiting: 0,
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
        // kanon:ignore RUST/pub-visibility
        Self::new(provider_name, ConcurrencyConfig::default())
    }

    /// Current concurrency limit (snapshot).
    #[must_use]
    pub fn limit(&self) -> u32 {
        // kanon:ignore RUST/pub-visibility
        self.inner.lock().limit
    }

    /// Current number of in-flight requests (snapshot).
    #[must_use]
    pub fn in_flight(&self) -> u32 {
        // kanon:ignore RUST/pub-visibility
        self.inner.lock().in_flight
    }

    /// Current number of parked waiters under the fixed admission policy
    /// (snapshot). Always 0 under the adaptive policy.
    #[must_use]
    pub fn waiting(&self) -> u32 {
        // kanon:ignore RUST/pub-visibility
        self.inner.lock().waiting
    }

    /// The configured admission policy.
    #[must_use]
    pub fn admission(&self) -> AdmissionPolicy {
        // kanon:ignore RUST/pub-visibility
        self.config.admission
    }

    /// Current EWMA latency estimate in seconds, or `None` if no samples yet.
    #[must_use]
    pub fn latency_ewma(&self) -> Option<f64> {
        // kanon:ignore RUST/pub-visibility
        self.inner.lock().latency_ewma
    }

    /// Acquire a permit, waiting asynchronously when at capacity.
    ///
    /// The permit must be consumed via [`ConcurrencyPermit::finish`] or
    /// [`ConcurrencyPermit::finish_with_latency`] to record the outcome.
    /// Dropping without calling either applies a `Neutral` outcome.
    ///
    /// # Cancel safety
    ///
    /// Not cancel-safe. If cancelled after incrementing `in_flight` but
    /// before returning the permit, the counter is never decremented,
    /// effectively leaking a concurrency slot until the limiter is dropped.
    /// However, the permit's `Drop` implementation will eventually release
    /// the slot if the permit itself is dropped.
    #[tracing::instrument(skip_all)]
    pub async fn acquire(self: &Arc<Self>) -> ConcurrencyPermit {
        loop {
            // WHY: create the notified future before the limit check so a wakeup
            // cannot land between the check and the await.
            let notified = self.notify.notified();

            {
                let mut inner = self.inner.lock();
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

    /// Acquire a permit under the configured [`AdmissionPolicy`].
    ///
    /// Under [`AdmissionPolicy::Adaptive`] this behaves exactly like
    /// [`Self::acquire`] and never fails. Under [`AdmissionPolicy::Fixed`],
    /// at most `max_running` permits are outstanding and at most
    /// `max_waiting` callers may park; any further caller is refused
    /// immediately with [`Error::ProviderSaturated`](crate::error::Error::ProviderSaturated)
    /// carrying the configured bound and a retry hint derived from the
    /// latency EWMA (#7152).
    ///
    /// # Errors
    ///
    /// Returns [`Error::ProviderSaturated`](crate::error::Error::ProviderSaturated)
    /// when the fixed admission queue is full.
    ///
    /// # Cancel safety
    ///
    /// A parked caller that is dropped releases its bounded waiter slot via
    /// an internal guard; the slot accounting stays correct across
    /// cancellation. The permit-slot caveat documented on [`Self::acquire`]
    /// applies here identically.
    #[tracing::instrument(skip_all)]
    pub async fn acquire_admitted(self: &Arc<Self>) -> crate::error::Result<ConcurrencyPermit> {
        // kanon:ignore RUST/pub-visibility
        let AdmissionPolicy::Fixed {
            max_running,
            max_waiting,
        } = self.config.admission
        else {
            return Ok(self.acquire().await);
        };

        let mut waiter: Option<WaiterSlot<'_>> = None;
        loop {
            // WHY: create the notified future before the limit check so a
            // wakeup cannot land between the check and the await.
            let notified = self.notify.notified();

            {
                let mut inner = self.inner.lock();
                if inner.in_flight < inner.limit {
                    if let Some(slot) = waiter.take() {
                        inner.waiting = inner.waiting.saturating_sub(1);
                        // WHY: the decrement above already released the
                        // waiter slot inside this lock; the guard must not
                        // release it a second time (its Drop also locks).
                        std::mem::forget(slot);
                    }
                    inner.in_flight += 1;
                    crate::metrics::set_concurrency_in_flight(&self.provider_name, inner.in_flight);
                    return Ok(ConcurrencyPermit {
                        limiter: Arc::clone(self),
                        outcome: AtomicU8::new(OUTCOME_NEUTRAL),
                        released: AtomicU8::new(0),
                        start: Instant::now(),
                    });
                }
                if waiter.is_none() {
                    if inner.waiting >= max_waiting {
                        let retry_after_ms = saturation_retry_hint_ms(inner.latency_ewma);
                        drop(inner);
                        tracing::warn!(
                            provider = %self.provider_name,
                            max_running,
                            max_waiting,
                            retry_after_ms,
                            "admission queue full; refusing request with typed retry guidance"
                        );
                        return Err(crate::error::ProviderSaturatedSnafu {
                            provider: self.provider_name.clone(),
                            max_running,
                            max_waiting,
                            retry_after_ms,
                        }
                        .build());
                    }
                    inner.waiting += 1;
                    waiter = Some(WaiterSlot {
                        limiter: self.as_ref(),
                    });
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
            let mut inner = self.inner.lock();

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

            // WHY: treat high EWMA latency as overload so successes back off.
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

            // WHY(#7152): a fixed admission bound is a hard cap — AIMD
            // feedback must not grow it past max_running on success or
            // shrink it below on overload.
            let adjust_limit = matches!(self.config.admission, AdmissionPolicy::Adaptive);
            match effective_outcome {
                RequestOutcome::Success if adjust_limit => {
                    inner.limit =
                        (inner.limit + self.config.increase_step).min(self.config.max_limit);
                }
                RequestOutcome::Overload if adjust_limit => {
                    // INVARIANT: f64::from(u32) is lossless; all u32 values fit in
                    // the f64 mantissa.
                    let limit_f64 = f64::from(inner.limit);
                    let decreased_f64 = (limit_f64 * self.config.decrease_factor).floor();
                    #[expect(
                        clippy::cast_possible_truncation,
                        clippy::cast_sign_loss,
                        clippy::as_conversions,
                        reason = "decreased_f64 is non-negative and bounded by inner.limit (a u32)"
                    )]
                    let decreased = decreased_f64 as u32; // kanon:ignore RUST/as-cast
                    inner.limit = decreased.max(self.config.min_limit);
                }
                RequestOutcome::Success | RequestOutcome::Overload | RequestOutcome::Neutral => {}
            }

            (inner.limit, inner.in_flight, inner.latency_ewma)
        };

        crate::metrics::set_concurrency_limit(&self.provider_name, new_limit);
        crate::metrics::set_concurrency_in_flight(&self.provider_name, new_in_flight);
        if let Some(ewma) = ewma {
            crate::metrics::set_concurrency_latency_ewma(&self.provider_name, ewma);
        }

        // Wake one waiter per newly available slot. A single release usually
        // frees exactly one slot; an additive limit increase frees additional
        // slots. The acquire loop re-checks the limit and re-parks if needed.
        let available = new_limit.saturating_sub(new_in_flight);
        for _ in 0..available {
            self.notify.notify_one();
        }
    }
}

/// Retry hint for a saturation refusal, derived from the latency EWMA.
///
/// One in-flight request is expected to take roughly the EWMA latency, so
/// that is the earliest a freed slot is plausible. Falls back to
/// [`DEFAULT_SATURATION_RETRY_HINT_MS`] before the first sample, floors at
/// 100ms, and caps at 10 minutes.
fn saturation_retry_hint_ms(latency_ewma: Option<f64>) -> u64 {
    match latency_ewma {
        Some(secs) if secs.is_finite() && secs > 0.0 => {
            let ms_f64 = (secs * 1000.0).ceil().min(600_000.0);
            #[expect(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                clippy::as_conversions,
                reason = "ms_f64 is non-negative and capped at 600_000, well within u64"
            )]
            let ms = ms_f64 as u64; // kanon:ignore RUST/as-cast
            ms.max(100)
        }
        _ => DEFAULT_SATURATION_RETRY_HINT_MS,
    }
}

/// Guard releasing one bounded waiter slot if a parked
/// [`AdaptiveConcurrencyLimiter::acquire_admitted`] caller is dropped
/// before it obtains a permit.
struct WaiterSlot<'a> {
    limiter: &'a AdaptiveConcurrencyLimiter,
}

impl Drop for WaiterSlot<'_> {
    fn drop(&mut self) {
        // WHY: without this, a cancelled waiter would permanently consume
        // one of the max_waiting slots and eventually wedge admission shut.
        let mut inner = self.limiter.inner.lock();
        inner.waiting = inner.waiting.saturating_sub(1);
    }
}

/// RAII permit that holds a concurrency slot.
///
/// Call [`finish`](ConcurrencyPermit::finish) or
/// [`finish_with_latency`](ConcurrencyPermit::finish_with_latency) to record
/// the outcome explicitly. If dropped without calling either, a `Neutral`
/// outcome is applied with the elapsed time as latency.
pub struct ConcurrencyPermit {
    // kanon:ignore RUST/pub-visibility
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
        // kanon:ignore RUST/pub-visibility
        let latency = self.start.elapsed();
        self.finish_inner(outcome, Some(latency));
    }

    /// Record the request outcome with an explicit latency and release the slot.
    ///
    /// Use this when the caller measures latency separately (e.g., excluding
    /// queue wait time).
    pub fn finish_with_latency(self, outcome: RequestOutcome, latency: Duration) {
        // kanon:ignore RUST/pub-visibility
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
        // kanon:ignore RUST/pub-visibility
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
/// Constructed via [`ConcurrencyLayer::layer`]. Acquires a permit before each
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
        // kanon:ignore RUST/pub-visibility
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
    use crate::error;
    use crate::types::{ContentBlock, StopReason, Usage};

    fn ok_response() -> CompletionResponse {
        CompletionResponse {
            id: "resp-1".to_owned(),
            model: "test-model".to_owned(),
            stop_reason: StopReason::EndTurn,
            content: vec![ContentBlock::Text {
                text: "response".to_owned(),
                citations: None,
            }],
            usage: Usage::default(),
            cost_usd: None,
            duration_ms: None,
        }
    }

    fn retryable_error() -> error::Result<CompletionResponse> {
        Err(error::RateLimitedSnafu {
            retry_after_ms: 100_u64,
        }
        .build())
    }

    fn non_retryable_error() -> error::Result<CompletionResponse> {
        Err(error::AuthFailedSnafu {
            message: "invalid key".to_owned(),
        }
        .build())
    }

    #[test]
    fn concurrency_outcome_table() {
        assert_eq!(
            concurrency_outcome(&Ok(ok_response())),
            RequestOutcome::Success
        );
        assert_eq!(
            concurrency_outcome(&retryable_error()),
            RequestOutcome::Overload
        );
        assert_eq!(
            concurrency_outcome(&non_retryable_error()),
            RequestOutcome::Neutral
        );
    }

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
                ..ConcurrencyConfig::default()
            },
        ))
    }

    /// Unwrap a rejection without requiring `Debug` on the permit.
    fn expect_saturated(result: crate::error::Result<ConcurrencyPermit>) -> crate::error::Error {
        match result {
            Ok(_) => panic!("expected a ProviderSaturated rejection"),
            Err(e) => e,
        }
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

        // WHY: tokio::time::sleep used because tokio test-util feature is not enabled for this crate. // kanon:ignore TESTING/sleep-in-test
        tokio::time::sleep(Duration::from_millis(10)).await; // kanon:ignore TESTING/sleep-in-test
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

    #[test] // kanon:ignore TESTING/tautological-test — compile-time trait bound check; compilation fails if bounds are not satisfied
    fn limiter_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<AdaptiveConcurrencyLimiter>();
        assert_send_sync::<ConcurrencyPermit>();
    }

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

    #[test] // kanon:ignore TESTING/tautological-test — compile-time trait bound check; compilation fails if bounds are not satisfied
    fn layer_is_clone_send() {
        fn assert_clone_send<T: Clone + Send>() {}
        assert_clone_send::<ConcurrencyLayer>();
    }

    fn fixed_limiter(max_running: u32, max_waiting: u32) -> Arc<AdaptiveConcurrencyLimiter> {
        Arc::new(AdaptiveConcurrencyLimiter::new(
            "fixed-test",
            ConcurrencyConfig {
                admission: AdmissionPolicy::Fixed {
                    max_running,
                    max_waiting,
                },
                ..ConcurrencyConfig::default()
            },
        ))
    }

    #[tokio::test]
    async fn fixed_admission_caps_running_and_rejects_excess_waiters() {
        // WHY(#7152): the localhosted launch contract is one running request
        // and at most two bounded waiters; the third waiter must be rejected
        // with typed guidance instead of parking unboundedly.
        let l = fixed_limiter(1, 2);
        let running = l.acquire_admitted().await.unwrap();
        assert_eq!(l.in_flight(), 1);

        let w1 = tokio::spawn({
            let l = Arc::clone(&l);
            async move { l.acquire_admitted().await }
        });
        let w2 = tokio::spawn({
            let l = Arc::clone(&l);
            async move { l.acquire_admitted().await }
        });
        // WHY: tokio::time::sleep because test-util is not enabled for this crate. // kanon:ignore TESTING/sleep-in-test
        tokio::time::sleep(Duration::from_millis(20)).await; // kanon:ignore TESTING/sleep-in-test
        assert!(!w1.is_finished());
        assert!(!w2.is_finished());
        assert_eq!(l.waiting(), 2);

        let err = expect_saturated(l.acquire_admitted().await);
        match err {
            crate::error::Error::ProviderSaturated {
                max_running,
                max_waiting,
                ..
            } => {
                assert_eq!(max_running, 1);
                assert_eq!(max_waiting, 2);
            }
            other => panic!("expected ProviderSaturated, got: {other}"),
        }

        // Releasing the running permit admits one waiter; the other stays parked.
        running.finish(RequestOutcome::Success);
        let p = tokio::select! {
            r = w1 => r.unwrap().unwrap(),
            r = w2 => r.unwrap().unwrap(),
        };
        assert_eq!(l.in_flight(), 1);
        assert_eq!(l.waiting(), 1);
        p.finish(RequestOutcome::Success);
    }

    #[tokio::test]
    async fn fixed_admission_limit_never_adapts() {
        // WHY(#7152): the hard cap must not inherit AIMD growth — success
        // must not raise the limit above max_running, and overload must not
        // shrink it below.
        let l = fixed_limiter(1, 2);
        let p = l.acquire_admitted().await.unwrap();
        p.finish(RequestOutcome::Success);
        assert_eq!(l.limit(), 1, "success must not grow a fixed limit");

        let p = l.acquire_admitted().await.unwrap();
        p.finish(RequestOutcome::Overload);
        assert_eq!(l.limit(), 1, "overload must not shrink a fixed limit");
    }

    #[tokio::test]
    async fn fixed_admission_zero_waiters_rejects_immediately() {
        let l = fixed_limiter(1, 0);
        let running = l.acquire_admitted().await.unwrap();
        let err = expect_saturated(l.acquire_admitted().await);
        assert!(
            matches!(err, crate::error::Error::ProviderSaturated { .. }),
            "with max_waiting = 0 any concurrent acquire must be rejected"
        );
        running.finish(RequestOutcome::Neutral);
    }

    #[tokio::test]
    async fn fixed_admission_rejection_carries_retry_hint_from_ewma() {
        let l = Arc::new(AdaptiveConcurrencyLimiter::new(
            "fixed-hint",
            ConcurrencyConfig {
                admission: AdmissionPolicy::Fixed {
                    max_running: 1,
                    max_waiting: 0,
                },
                ewma_alpha: 0.0,
                ..ConcurrencyConfig::default()
            },
        ));
        // Seed the EWMA with a 2s sample.
        let p = l.acquire_admitted().await.unwrap();
        p.finish_with_latency(RequestOutcome::Success, Duration::from_secs(2));

        let running = l.acquire_admitted().await.unwrap();
        let err = expect_saturated(l.acquire_admitted().await);
        match err {
            crate::error::Error::ProviderSaturated { retry_after_ms, .. } => {
                assert_eq!(
                    retry_after_ms, 2000,
                    "retry hint should reflect the observed latency EWMA"
                );
            }
            other => panic!("expected ProviderSaturated, got: {other}"),
        }
        running.finish(RequestOutcome::Neutral);
    }

    #[tokio::test]
    async fn fixed_admission_rejection_hint_defaults_without_samples() {
        let l = fixed_limiter(1, 0);
        let running = l.acquire_admitted().await.unwrap();
        let err = expect_saturated(l.acquire_admitted().await);
        match err {
            crate::error::Error::ProviderSaturated { retry_after_ms, .. } => {
                assert_eq!(
                    retry_after_ms, DEFAULT_SATURATION_RETRY_HINT_MS,
                    "without latency samples the hint falls back to the default"
                );
            }
            other => panic!("expected ProviderSaturated, got: {other}"),
        }
        running.finish(RequestOutcome::Neutral);
    }

    #[tokio::test]
    async fn cancelled_waiter_releases_queue_slot() {
        // WHY: a caller that gives up while parked must not permanently
        // consume one of the bounded waiter slots.
        let l = fixed_limiter(1, 1);
        let running = l.acquire_admitted().await.unwrap();

        let waiter = tokio::spawn({
            let l = Arc::clone(&l);
            async move { l.acquire_admitted().await }
        });
        // WHY: tokio::time::sleep because test-util is not enabled for this crate. // kanon:ignore TESTING/sleep-in-test
        tokio::time::sleep(Duration::from_millis(20)).await; // kanon:ignore TESTING/sleep-in-test
        assert_eq!(l.waiting(), 1);
        waiter.abort();
        let _ = waiter.await;
        // WHY: abort drops the parked future; the waiter guard must restore the slot. // kanon:ignore TESTING/sleep-in-test
        tokio::time::sleep(Duration::from_millis(20)).await; // kanon:ignore TESTING/sleep-in-test
        assert_eq!(l.waiting(), 0, "aborted waiter must release its queue slot");

        // A new waiter can now park instead of being rejected.
        let w2 = tokio::spawn({
            let l = Arc::clone(&l);
            async move { l.acquire_admitted().await }
        });
        // WHY: tokio::time::sleep because test-util is not enabled for this crate. // kanon:ignore TESTING/sleep-in-test
        tokio::time::sleep(Duration::from_millis(20)).await; // kanon:ignore TESTING/sleep-in-test
        assert_eq!(l.waiting(), 1, "freed slot must be reusable");
        running.finish(RequestOutcome::Neutral);
        w2.await.unwrap().unwrap().finish(RequestOutcome::Neutral);
    }

    #[tokio::test]
    async fn adaptive_admission_never_rejects() {
        // The default policy keeps the historical parking behavior.
        let l = limiter(1, 1, 10);
        let p1 = l.acquire_admitted().await.unwrap();
        let waiter = tokio::spawn({
            let l = Arc::clone(&l);
            async move { l.acquire_admitted().await }
        });
        // WHY: tokio::time::sleep because test-util is not enabled for this crate. // kanon:ignore TESTING/sleep-in-test
        tokio::time::sleep(Duration::from_millis(20)).await; // kanon:ignore TESTING/sleep-in-test
        assert!(
            !waiter.is_finished(),
            "adaptive mode parks instead of rejecting"
        );
        p1.finish(RequestOutcome::Neutral);
        waiter
            .await
            .unwrap()
            .unwrap()
            .finish(RequestOutcome::Neutral);
    }

    #[test]
    fn limiter_survives_panic_during_unwind() {
        // With std::sync::Mutex, a panic that triggers Drop → release() during
        // unwinding would poison the mutex, killing all future LLM traffic.
        // parking_lot::Mutex has no poisoning, so the limiter keeps working.
        let limiter = Arc::new(AdaptiveConcurrencyLimiter::new(
            "panic-test",
            ConcurrencyConfig::default(),
        ));

        let limiter2 = Arc::clone(&limiter);
        let handle = std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .build()
                .unwrap();
            rt.block_on(async {
                let _permit = limiter2.acquire().await;
                panic!("intentional panic while permit is held");
            });
        });

        // The spawned thread must have panicked.
        assert!(handle.join().is_err());

        // The limiter should still be fully usable.
        let rt = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        rt.block_on(async {
            let permit = limiter.acquire().await;
            permit.finish(RequestOutcome::Success);
        });
        assert_eq!(limiter.in_flight(), 0);
        assert_eq!(limiter.limit(), 11); // default initial 10 + 1 for success
    }
}
