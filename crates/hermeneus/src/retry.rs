//! Shared retry backoff helpers for provider implementations.

use std::time::Duration;

use koina::retry::{BackoffStrategy, retry_after_or_strategy_delay};

use crate::error;
use crate::models::{BACKOFF_BASE_MS, BACKOFF_MAX_MS};

const MIN_BACKOFF_MS: u64 = 100;

/// Compute the sleep duration before the next provider retry attempt.
///
/// Rate-limit errors are honored exactly when the provider supplies a
/// `retry-after` value. All other retryable errors use an exponential-jitter
/// strategy capped at [`BACKOFF_MAX_MS`].
pub(crate) fn backoff_delay(attempt: u32, last_error: Option<&error::Error>) -> Duration {
    let retry_after = last_error.and_then(|err| match err {
        error::Error::RateLimited { retry_after_ms, .. } => {
            Some(Duration::from_millis(*retry_after_ms))
        }
        _ => None,
    });
    let strategy = BackoffStrategy::ExponentialJitter {
        base: Duration::from_millis(BACKOFF_BASE_MS),
        factor: 2,
        max_delay: Duration::from_millis(BACKOFF_MAX_MS),
    };
    // WHY: provider retry loops pass 1-indexed retry attempts; koina strategies are 0-indexed.
    retry_after_or_strategy_delay(
        &strategy,
        attempt.saturating_sub(1),
        retry_after,
        Some(Duration::from_millis(MIN_BACKOFF_MS)),
    )
}
