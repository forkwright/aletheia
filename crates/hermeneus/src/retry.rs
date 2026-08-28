//! Shared retry backoff helpers for provider implementations.

use std::time::Duration;

use koina::retry::{BackoffStrategy, retry_after_or_strategy_delay};
use reqwest::Response;

use crate::error;
use crate::models::{BACKOFF_BASE_MS, BACKOFF_FACTOR, BACKOFF_MAX_MS, DEFAULT_MAX_RETRIES};

const MIN_BACKOFF_MS: u64 = 100;

/// Parse a response's `retry-after` header as whole seconds and convert to
/// milliseconds.
///
/// Accepted syntax is a bare non-negative integer (RFC 9110's delay-seconds
/// form); the HTTP-date form is not accepted. Overflow policy: `u64::parse`
/// rejects any value that would not fit, so an absurd header is treated the
/// same as a missing one rather than wrapping or saturating.
pub(crate) fn extract_retry_after(response: &Response) -> Option<u64> {
    response
        .headers()
        .get("retry-after")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u64>().ok())
        .map(|secs| secs * 1000)
}

/// Format an error and its full source chain into a single message string.
///
/// WHY(#4885/#4887/#5875): reqwest's `Display` can hide the underlying
/// transport cause ("connection reset by peer"). Retry classification scans
/// for those cause words in the full chain when deciding whether a
/// pre-content streaming failure can be retried.
pub(crate) fn error_chain_message(prefix: &str, err: &dyn std::error::Error) -> String {
    let mut parts = vec![format!("{prefix}: {err}")];
    let mut source = err.source();
    while let Some(s) = source {
        parts.push(s.to_string());
        source = s.source();
    }
    parts.join(": ")
}

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
            factor: BACKOFF_FACTOR,
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

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use wiremock::matchers::method;
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use super::*;

    async fn response_with_retry_after(value: Option<&str>) -> Response {
        // WHY: this helper builds a bare reqwest client rather than going through
        // `AnthropicClient`/`OpenAiProvider`, which is where the crate installs the
        // rustls provider. Without this, `reqwest::get` panics with "No rustls
        // crypto provider is configured".
        let _ = rustls::crypto::ring::default_provider().install_default(); // kanon:ignore RUST/no-silent-result-swallow WHY: install_default is idempotent; Err on a second call is expected and safe to discard
        let server = MockServer::start().await;
        let mut template = ResponseTemplate::new(429);
        if let Some(value) = value {
            template = template.insert_header("retry-after", value);
        }
        Mock::given(method("GET"))
            .respond_with(template)
            .mount(&server)
            .await;
        reqwest::get(server.uri()).await.expect("mock request")
    }

    #[tokio::test]
    async fn extract_retry_after_table() {
        let cases: &[(Option<&str>, Option<u64>)] = &[
            (Some("30"), Some(30_000)),
            (Some("0"), Some(0)),
            (None, None),
            (Some("not-a-number"), None),
            (Some("Wed, 21 Oct 2026 07:28:00 GMT"), None),
            (Some("-5"), None),
        ];
        for (header, expected) in cases {
            let response = response_with_retry_after(*header).await;
            assert_eq!(
                extract_retry_after(&response),
                *expected,
                "header {header:?}"
            );
        }
    }

    #[test]
    fn error_chain_message_flattens_full_source_chain() {
        #[derive(Debug)]
        struct Root;
        impl std::fmt::Display for Root {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "connection reset by peer")
            }
        }
        impl std::error::Error for Root {}

        #[derive(Debug)]
        struct Wrapper(Root);
        impl std::fmt::Display for Wrapper {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "stream read failed")
            }
        }
        impl std::error::Error for Wrapper {
            fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
                Some(&self.0)
            }
        }

        let message = error_chain_message("prefix", &Wrapper(Root));
        assert_eq!(
            message,
            "prefix: stream read failed: connection reset by peer"
        );
    }

    #[test]
    fn error_chain_message_without_source_is_just_the_prefix_and_message() {
        #[derive(Debug)]
        struct Leaf;
        impl std::fmt::Display for Leaf {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "leaf error")
            }
        }
        impl std::error::Error for Leaf {}

        let message = error_chain_message("prefix", &Leaf);
        assert_eq!(message, "prefix: leaf error");
    }
}
