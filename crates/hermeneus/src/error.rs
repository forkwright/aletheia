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
pub type Result<T> = std::result::Result<T, Error>;
