//! Shared retry backoff helper for Hermeneus providers.

use std::time::Duration;

use koina::retry::BackoffStrategy;

use crate::error::Error;
use crate::models::{BACKOFF_BASE_MS, BACKOFF_MAX_MS};

/// Compute the sleep duration before the next provider retry attempt.
///
/// Rate-limit errors are honored exactly when the provider supplies a
/// `retry-after` value. All other retryable errors use an exponential-jitter
/// strategy capped at [`BACKOFF_MAX_MS`].
pub(crate) fn backoff_delay(attempt: u32, last_error: Option<&Error>) -> Duration {
    if let Some(Error::RateLimited { retry_after_ms, .. }) = last_error {
        return Duration::from_millis(*retry_after_ms);
    }

    let strategy = BackoffStrategy::ExponentialJitter {
        base: Duration::from_millis(BACKOFF_BASE_MS),
        factor: 2,
        max_delay: Duration::from_millis(BACKOFF_MAX_MS),
    };
    // WHY: call site passes 1-indexed attempt; delay_for_attempt is 0-indexed.
    let delay = strategy.delay_for_attempt(attempt.saturating_sub(1));
    delay.max(Duration::from_millis(100))
}
