//! Hermeneus-specific errors.
//!
//! Each variant maps to a distinct failure mode in the LLM call path:
//! initialization, network transport, HTTP status, rate limiting, response
//! parsing, model support, and authentication.

use snafu::Snafu;

/// Diagnostic context carried by [`Error::ApiError`].
///
/// Grouped into a separate struct so it can be boxed in the enum variant,
/// keeping the variant size below clippy's `result_large_err` threshold.
#[derive(Debug)]
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
#[non_exhaustive]
#[expect(
    missing_docs,
    reason = "snafu error variant fields (source, location, message) are self-documenting via display format"
)]
pub enum Error {
    // kanon:ignore RUST/pub-visibility
    /// Provider initialization failed.
    #[snafu(display("provider init failed: {message}"))]
    ProviderInit {
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
}

impl Error {
    /// Whether this error indicates a transient failure worth retrying
    /// with a different model (429, 503, 529, timeout, connection reset,
    /// provider process unavailable).
    #[must_use]
    pub fn is_retryable(&self) -> bool {
        // kanon:ignore RUST/pub-visibility
        match self {
            // WHY: ProviderInit errors from subprocess providers (CC) indicate the
            // provider binary is temporarily unavailable (crashed, auth expired,
            // deleted). This is a transient condition — the binary may come back —
            // so the execute stage should activate degraded-mode fallback instead
            // of surfacing a hard error.
            Error::ProviderInit { .. }
            | Error::RateLimited { .. }
            | Error::ApiError {
                status: 500..=599, ..
            } => true,
            Error::ApiRequest { message, .. } => {
                let msg = message.to_lowercase();
                msg.contains("timeout")
                    || msg.contains("connection")
                    || msg.contains("reset")
                    || msg.contains("broken pipe")
                    || msg.contains("cc process exited")
                    || msg.contains("cc subprocess")
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
            // and provider init failures (CC binary crashed/disappeared).
            Error::RateLimited { .. }
            | Error::ProviderInit { .. }
            | Error::ApiError {
                status: 500..=599, ..
            } => ErrorClass::Transient,

            // Mixed: classify by message content (timeout/connection/reset/pipe → transient)
            Error::ApiRequest { message, .. } => {
                let msg = message.to_lowercase();
                if msg.contains("timeout")
                    || msg.contains("connection")
                    || msg.contains("reset")
                    || msg.contains("broken pipe")
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
            | Error::ParseResponse { .. } => ErrorClass::Permanent,
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
            Error::ApiRequest { message, .. } => {
                let msg = message.to_lowercase();
                if msg.contains("timeout")
                    || msg.contains("connection")
                    || msg.contains("reset")
                    || msg.contains("broken pipe")
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
            Error::ParseResponse { .. } => ErrorAction::Escalate,
        }
    }
}

#[cfg(test)]
mod tests {
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

    #[test]
    fn api_request_cc_process_exit_is_retryable() {
        // WHY: CC subprocess crash mid-turn produces an ApiRequest error with
        // "CC process exited" in the message. Must be retryable so degraded
        // mode activates instead of surfacing a hard 500.
        let err = ApiRequestSnafu {
            message: "CC process exited with exit status: 1: OAuth token rejected",
        }
        .build();
        assert!(err.is_retryable(), "CC process exit should be retryable");
    }

    #[test]
    fn api_request_cc_subprocess_timeout_is_retryable() {
        let err = ApiRequestSnafu {
            message: "CC subprocess timed out after 300s",
        }
        .build();
        assert!(
            err.is_retryable(),
            "CC subprocess timeout should be retryable"
        );
    }
}
