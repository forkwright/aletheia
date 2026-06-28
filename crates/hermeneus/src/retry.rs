//! Shared retry backoff helpers for provider implementations.

use std::time::Duration;

use koina::retry::{BackoffStrategy, retry_after_or_strategy_delay};

use crate::error;
use crate::models::{BACKOFF_BASE_MS, BACKOFF_MAX_MS, DEFAULT_MAX_RETRIES};

const MIN_BACKOFF_MS: u64 = 100;
#[cfg(any(
    feature = "cc-provider",
    feature = "codex-provider",
    feature = "kimi-provider"
))]
const SUBPROCESS_MAX_RETRIES: u32 = 2;
#[cfg(any(
    feature = "cc-provider",
    feature = "codex-provider",
    feature = "kimi-provider"
))]
const SUBPROCESS_BACKOFF_BASE_MS: u64 = 500;
#[cfg(any(
    feature = "cc-provider",
    feature = "codex-provider",
    feature = "kimi-provider"
))]
const SUBPROCESS_BACKOFF_MAX_MS: u64 = 1_000;

/// Runtime retry attempts and exponential backoff policy for LLM providers.
///
/// `max_retries` counts retries after the initial request. A value of `0`
/// disables retries. The backoff fields are milliseconds because the operator
/// config surface exposes retry timing at millisecond precision.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RetryPolicy {
    /// Maximum retry attempts after the initial request.
    pub max_retries: u32,
    /// Initial exponential backoff delay in milliseconds.
    pub backoff_base_ms: u64,
    /// Maximum exponential backoff delay in milliseconds.
    pub backoff_max_ms: u64,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_retries: DEFAULT_MAX_RETRIES,
            backoff_base_ms: BACKOFF_BASE_MS,
            backoff_max_ms: BACKOFF_MAX_MS,
        }
    }
}

impl RetryPolicy {
    /// Compute the delay before the next retry attempt.
    ///
    /// Provider loops pass 1-indexed retry attempts; this method converts them
    /// to the 0-indexed convention used by [`BackoffStrategy`]. Rate-limit
    /// `retry-after` values take precedence over configured exponential backoff.
    #[must_use]
    pub fn delay(self, attempt: u32, last_error: Option<&error::Error>) -> Duration {
        let retry_after = last_error.and_then(|err| match err {
            error::Error::RateLimited { retry_after_ms, .. } => {
                Some(Duration::from_millis(*retry_after_ms))
            }
            _ => None,
        });
        let strategy = BackoffStrategy::ExponentialJitter {
            base: Duration::from_millis(self.backoff_base_ms),
            factor: 2,
            max_delay: Duration::from_millis(self.backoff_max_ms.max(self.backoff_base_ms)),
        };
        retry_after_or_strategy_delay(
            &strategy,
            attempt.saturating_sub(1),
            retry_after,
            Some(Duration::from_millis(MIN_BACKOFF_MS)),
        )
    }
}

/// Retry policy for local subprocess providers.
///
/// WHY(#5763): local CLI spawn failures and one-off timeouts are transient but
/// should recover quickly; use a short fixed policy instead of the longer
/// network-provider defaults.
#[must_use]
#[cfg(any(
    feature = "cc-provider",
    feature = "codex-provider",
    feature = "kimi-provider"
))]
pub(crate) const fn subprocess_retry_policy() -> RetryPolicy {
    RetryPolicy {
        max_retries: SUBPROCESS_MAX_RETRIES,
        backoff_base_ms: SUBPROCESS_BACKOFF_BASE_MS,
        backoff_max_ms: SUBPROCESS_BACKOFF_MAX_MS,
    }
}
