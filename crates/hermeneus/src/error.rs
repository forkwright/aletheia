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
    pub credential_source: String,
}

impl ApiErrorContext {
    /// Empty context for error sites without model/credential information.
    #[must_use]
    pub fn empty() -> Box<Self> {
        // kanon:ignore RUST/pub-visibility
        Box::new(Self {
            model: String::new(),
            credential_source: String::new(),
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
    /// with a different model (429, 503, 529, timeout).
    #[must_use]
    pub fn is_retryable(&self) -> bool {
        // kanon:ignore RUST/pub-visibility
        matches!(
            self,
            Error::RateLimited { .. }
                | Error::ApiRequest { .. }
                | Error::ApiError {
                    status: 500..=599,
                    ..
                }
        )
    }
}

/// Convenience alias for `Result<T, Error>`.
pub type Result<T> = std::result::Result<T, Error>; // kanon:ignore RUST/pub-visibility

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
}
