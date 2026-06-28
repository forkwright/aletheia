//! Hermeneus-specific errors.
//!
//! Each variant maps to a distinct failure mode in the LLM call path:
//! initialization, network transport, HTTP status, rate limiting, response
//! parsing, model support, and authentication.

use snafu::Snafu;

/// Subprocess failure class for seat-bridged providers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum SubprocessFailureKind {
    /// Provider subprocess could not be spawned.
    Spawn,
    /// Provider subprocess exited unsuccessfully.
    Exit,
    /// Provider subprocess exceeded its wall-clock timeout.
    Timeout,
    /// Provider subprocess completed without producing required output.
    NoOutput,
}

impl std::fmt::Display for SubprocessFailureKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let label = match self {
            Self::Spawn => "spawn failed",
            Self::Exit => "exited unsuccessfully",
            Self::Timeout => "timed out",
            Self::NoOutput => "produced no output",
        };
        f.write_str(label)
    }
}

/// Diagnostic context carried by [`Error::ApiError`].
///
/// Grouped into a separate struct so it can be boxed in the enum variant,
/// keeping the variant size below clippy's `result_large_err` threshold.
#[derive(Debug)] // kanon:ignore RUST/no-debug-derive-on-public-types
pub struct ApiErrorContext {
    /// Model requested when the error occurred.
    pub model: String,
    /// Credential source used (e.g. `"oauth"`, `"environment"`, `"file"`).
    pub credential_source: String, // kanon:ignore RUST/plain-string-secret
}

impl ApiErrorContext {
    /// Empty context for error sites without model/credential information.
    #[must_use]
    pub fn empty() -> Box<Self> {
        // kanon:ignore RUST/pub-visibility
        Box::new(Self {
            model: String::new(),
            credential_source: String::new(), // kanon:ignore RUST/plain-string-secret
        })
    }
}

/// Errors from LLM provider operations.
#[derive(Debug, Snafu)]
#[snafu(visibility(pub))]
#[expect(
    missing_docs,
    reason = "snafu error variant fields (source, location, message) are self-documenting via display format"
)]
#[non_exhaustive]
pub enum Error {
    // kanon:ignore RUST/pub-visibility
    /// Provider initialization failed.
    #[snafu(display("provider init failed: {message}"))]
    ProviderInit {
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Seat-bridged provider subprocess failed in a retryable way.
    #[snafu(display("{provider} subprocess {kind}: {message}"))]
    SubprocessFailure {
        provider: String,
        kind: SubprocessFailureKind,
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// API request failed.
    #[snafu(display("API request failed: {message}"))]
    ApiRequest {
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// API error response body could not be read.
    #[snafu(display("failed to read API error response body for status {status}: {source}"))]
    ApiErrorBodyRead { status: u16, source: reqwest::Error },

    /// API returned an error response.
    #[snafu(display("API error {status}: {message}"))]
    ApiError {
        status: u16,
        message: String,
        /// Diagnostic context (model + credential source).
        ///
        /// Boxed so that the variant stays within clippy's `result_large_err`
        /// limit. `hermeneus::Error` is embedded as a `source` field inside
        /// `nous::Error`, and two unboxed `String` fields would push the
        /// `nous::Error` variant size over 128 bytes.
        context: Box<ApiErrorContext>,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Rate limited (429).
    #[snafu(display("rate limited, retry after {retry_after_ms}ms"))]
    RateLimited {
        retry_after_ms: u64,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Response parsing failed.
    #[snafu(display("failed to parse response: {source}"))]
    ParseResponse {
        source: serde_json::Error,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Provider response violated the expected adapter contract.
    #[snafu(display("provider contract violation: {message}"))]
    ProviderContract {
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Model not supported by this provider.
    #[snafu(display("model not supported: {model}"))]
    UnsupportedModel {
        model: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Authentication failed.
    #[snafu(display("authentication failed: {message}"))]
    AuthFailed {
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// SSE stream ended before the provider sent a terminal completion marker.
    #[snafu(display("stream incomplete: {message} (partial content: {partial_content})"))]
    StreamIncomplete {
        message: String,
        /// Buffered partial content preserved for diagnostics.
        partial_content: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },
}

impl Error {
    /// Whether this error indicates a transient failure worth retrying
    /// with a different model (429, 503, 529, timeout, connection reset,
    /// provider process unavailable).
    #[must_use]
    pub fn is_retryable(&self) -> bool {
        // kanon:ignore RUST/pub-visibility
        match self {
            // WHY: ProviderInit errors from subprocess providers indicate the
            // provider binary is temporarily unavailable (crashed, auth expired,
            // deleted). This is a transient condition — the binary may come back —
            // so the execute stage should activate degraded-mode fallback instead
            // of surfacing a hard error.
            Error::ProviderInit { .. }
            | Error::SubprocessFailure { .. }
            | Error::RateLimited { .. }
            | Error::ApiErrorBodyRead { .. }
            | Error::StreamIncomplete { .. }
            | Error::ApiError {
                status: 500..=599, ..
            } => true,
            Error::ApiRequest { message, .. } => {
                let msg = message.to_lowercase();
                msg.contains("timeout")
                    || msg.contains("timed out")
                    || msg.contains("connection")
                    || msg.contains("reset")
                    || msg.contains("broken pipe")
                    || msg.contains("is currently unavailable")
                    || msg.contains("circuit-breaker open")
            }
            _ => false,
        }
    }
}

/// Convenience alias for `Result<T, Error>`.
pub type Result<T> = std::result::Result<T, Error>; // kanon:ignore RUST/pub-visibility

impl koina::error_class::Classifiable for Error {
    fn class(&self) -> koina::error_class::ErrorClass {
        use koina::error_class::ErrorClass;
        match self {
            // Transient: safe to retry — rate limits, server errors (5xx),
            // subprocess failures, and provider init failures.
            Error::RateLimited { .. }
            | Error::ProviderInit { .. }
            | Error::SubprocessFailure { .. }
            | Error::ApiErrorBodyRead { .. }
            | Error::StreamIncomplete { .. }
            | Error::ApiError {
                status: 500..=599, ..
            } => ErrorClass::Transient,

            // Mixed: classify by message content (timeout/connection/reset/pipe → transient)
            Error::ApiRequest { message, .. } => {
                let msg = message.to_lowercase();
                if msg.contains("timeout")
                    || msg.contains("timed out")
                    || msg.contains("connection")
                    || msg.contains("reset")
                    || msg.contains("broken pipe")
                    || msg.contains("is currently unavailable")
                    || msg.contains("circuit-breaker open")
                {
                    ErrorClass::Transient
                } else {
                    ErrorClass::Permanent
                }
            }

            // Permanent: retrying will not help — auth, unsupported model,
            // non-5xx API errors, parse failures
            Error::AuthFailed { .. }
            | Error::UnsupportedModel { .. }
            | Error::ApiError { .. }
            | Error::ParseResponse { .. }
            | Error::ProviderContract { .. } => ErrorClass::Permanent,
        }
    }

    fn action(&self) -> koina::error_class::ErrorAction {
        use koina::error_class::ErrorAction;
        match self {
            Error::RateLimited { retry_after_ms, .. } => ErrorAction::Retry {
                max_attempts: 4,
                // WHY: respect provider's hint when available; fall back to 2 s
                backoff_base_ms: (*retry_after_ms).max(2_000),
            },
            Error::ApiError {
                status: 500..=599, ..
            } => ErrorAction::Retry {
                max_attempts: 3,
                backoff_base_ms: 1_000,
            },
            Error::ApiErrorBodyRead { .. }
            | Error::StreamIncomplete { .. }
            | Error::SubprocessFailure { .. } => ErrorAction::Retry {
                max_attempts: 3,
                backoff_base_ms: 500,
            },
            Error::ApiRequest { message, .. } => {
                let msg = message.to_lowercase();
                if msg.contains("timeout")
                    || msg.contains("timed out")
                    || msg.contains("connection")
                    || msg.contains("reset")
                    || msg.contains("broken pipe")
                    || msg.contains("is currently unavailable")
                    || msg.contains("circuit-breaker open")
                {
                    ErrorAction::Retry {
                        max_attempts: 3,
                        backoff_base_ms: 500,
                    }
                } else {
                    ErrorAction::Escalate
                }
            }

            Error::AuthFailed { .. } => ErrorAction::Surface {
                user_message: "Authentication failed — check your API credentials.".to_owned(),
            },
            Error::UnsupportedModel { model, .. } => ErrorAction::Surface {
                user_message: format!("Model '{model}' is not supported by this provider."),
            },
            Error::ApiError {
                status, message, ..
            } => ErrorAction::Surface {
                user_message: format!("API error {status}: {message}"),
            },
            Error::ProviderInit { .. } => ErrorAction::Retry {
                max_attempts: 3,
                backoff_base_ms: 2_000,
            },
            Error::ParseResponse { .. } | Error::ProviderContract { .. } => ErrorAction::Escalate,
        }
    }
}

#[cfg(test)]
mod tests {
    use koina::error_class::Classifiable;

    use super::*;

    #[test]
    fn api_error_context_empty_has_empty_fields() {
        let ctx = ApiErrorContext::empty();
        assert!(ctx.model.is_empty());
        assert!(ctx.credential_source.is_empty());
    }

    #[test]
    fn rate_limited_is_retryable() {
        let err = RateLimitedSnafu {
            retry_after_ms: 1000u64,
        }
        .build();
        assert!(err.is_retryable());
    }

    #[test]
    fn api_request_error_is_retryable() {
        let err = ApiRequestSnafu {
            message: "connection timeout".to_owned(),
        }
        .build();
        assert!(err.is_retryable());
    }

    #[test]
    fn api_request_timed_out_only_is_retryable() {
        // WHY(#5455): "timed out" must be retryable on its own, without relying
        // on "timeout", "connection", or provider-specific substrings.
        let err = ApiRequestSnafu {
            message: "timed out".to_owned(),
        }
        .build();
        assert!(
            err.is_retryable(),
            "bare 'timed out' ApiRequest should be retryable"
        );
    }

    #[test]
    fn api_error_5xx_is_retryable() {
        let err = Error::ApiError {
            status: 503u16,
            message: "service unavailable".to_owned(),
            context: ApiErrorContext::empty(),
            location: snafu::location!(),
        };
        assert!(err.is_retryable());
    }

    #[test]
    fn api_error_4xx_is_not_retryable() {
        let err = Error::ApiError {
            status: 401u16,
            message: "unauthorized".to_owned(),
            context: ApiErrorContext::empty(),
            location: snafu::location!(),
        };
        assert!(!err.is_retryable());
    }

    #[test]
    fn unsupported_model_is_not_retryable() {
        let err = UnsupportedModelSnafu {
            model: "gpt-99".to_owned(),
        }
        .build();
        assert!(!err.is_retryable());
    }

    #[test]
    fn auth_failed_is_not_retryable() {
        let err = AuthFailedSnafu {
            message: "invalid key".to_owned(),
        }
        .build();
        assert!(!err.is_retryable());
    }

    #[test]
    fn api_request_non_transient_is_not_retryable() {
        let err = ApiRequestSnafu {
            message: "invalid request body".to_owned(),
        }
        .build();
        assert!(
            !err.is_retryable(),
            "non-transient ApiRequest should not be retryable"
        );
    }

    #[test]
    fn provider_init_is_retryable() {
        // WHY: ProviderInit errors from CC subprocess (binary crashed, disappeared)
        // are transient — the binary may come back. Degraded mode must activate.
        let err = ProviderInitSnafu {
            message: "failed to spawn claude CLI at /usr/bin/claude: No such file or directory",
        }
        .build();
        assert!(
            err.is_retryable(),
            "ProviderInit should be retryable (transient provider unavailability)"
        );
    }

    fn assert_subprocess_failure_retryable(provider: &str, kind: SubprocessFailureKind) {
        let err = SubprocessFailureSnafu {
            provider: provider.to_owned(),
            kind,
            message: "synthetic subprocess failure".to_owned(),
        }
        .build();
        assert!(err.is_retryable(), "{provider} {kind} should be retryable");
        assert_eq!(
            err.class(),
            koina::error_class::ErrorClass::Transient,
            "{provider} {kind} should classify as transient"
        );
        assert!(
            matches!(err.action(), koina::error_class::ErrorAction::Retry { .. }),
            "{provider} {kind} should action as retry"
        );
    }

    macro_rules! subprocess_retryable_test {
        ($name:ident, $provider:literal, $kind:expr) => {
            #[test]
            fn $name() {
                assert_subprocess_failure_retryable($provider, $kind);
            }
        };
    }

    subprocess_retryable_test!(
        cc_spawn_failure_is_retryable,
        "cc",
        SubprocessFailureKind::Spawn
    );
    subprocess_retryable_test!(
        cc_exit_failure_is_retryable,
        "cc",
        SubprocessFailureKind::Exit
    );
    subprocess_retryable_test!(
        cc_timeout_failure_is_retryable,
        "cc",
        SubprocessFailureKind::Timeout
    );
    subprocess_retryable_test!(
        kimi_spawn_failure_is_retryable,
        "kimi",
        SubprocessFailureKind::Spawn
    );
    subprocess_retryable_test!(
        kimi_exit_failure_is_retryable,
        "kimi",
        SubprocessFailureKind::Exit
    );
    subprocess_retryable_test!(
        kimi_timeout_failure_is_retryable,
        "kimi",
        SubprocessFailureKind::Timeout
    );
    subprocess_retryable_test!(
        codex_spawn_failure_is_retryable,
        "codex",
        SubprocessFailureKind::Spawn
    );
    subprocess_retryable_test!(
        codex_exit_failure_is_retryable,
        "codex",
        SubprocessFailureKind::Exit
    );
    subprocess_retryable_test!(
        codex_timeout_failure_is_retryable,
        "codex",
        SubprocessFailureKind::Timeout
    );
    subprocess_retryable_test!(
        codex_no_output_failure_is_retryable,
        "codex",
        SubprocessFailureKind::NoOutput
    );

    #[test]
    fn api_request_provider_unavailable_is_transient_retry() {
        // WHY(#5260): provider registry marks a provider Down before the turn;
        // the resulting ApiRequest must be retryable so fallback activates.
        let err = ApiRequestSnafu {
            message: "provider 'primary' is currently unavailable".to_owned(),
        }
        .build();
        assert!(
            err.is_retryable(),
            "provider unavailable should be retryable"
        );
        assert_eq!(
            err.class(),
            koina::error_class::ErrorClass::Transient,
            "provider unavailable should classify as transient"
        );
        assert!(
            matches!(err.action(), koina::error_class::ErrorAction::Retry { .. }),
            "provider unavailable should action as retry"
        );
    }

    #[test]
    fn api_request_circuit_breaker_open_is_transient_retry() {
        // WHY(#5260): Anthropic/OpenAI clients emit circuit-breaker messages
        // as generic ApiRequest errors; they must be fallback-eligible.
        let err = ApiRequestSnafu {
            message: "provider circuit-breaker open: too many failures".to_owned(),
        }
        .build();
        assert!(
            err.is_retryable(),
            "circuit-breaker open should be retryable"
        );
        assert_eq!(
            err.class(),
            koina::error_class::ErrorClass::Transient,
            "circuit-breaker open should classify as transient"
        );
        assert!(
            matches!(err.action(), koina::error_class::ErrorAction::Retry { .. }),
            "circuit-breaker open should action as retry"
        );
    }

    #[test]
    fn stream_incomplete_is_retryable_and_transient() {
        // WHY(#5050): A truncated SSE stream is a provider-transport failure;
        // the caller should fall back or retry rather than surface a partial
        // completion as a successful answer.
        let err = StreamIncompleteSnafu {
            message: "SSE stream ended without [DONE]".to_owned(),
            partial_content: "text_len=12, tool_calls=0".to_owned(),
        }
        .build();
        assert!(err.is_retryable(), "truncated stream should be retryable");
        assert_eq!(
            err.class(),
            koina::error_class::ErrorClass::Transient,
            "truncated stream should classify as transient"
        );
        assert!(
            matches!(err.action(), koina::error_class::ErrorAction::Retry { .. }),
            "truncated stream should action as retry"
        );
    }
}
