use snafu::prelude::*;

/// API-layer error type for the TUI HTTP client.
///
/// Covers three failure modes:
/// - `Http`: a transport or connection error from `reqwest`
/// - `Server`: a non-2xx HTTP response with a human-readable message from the server body
/// - `Auth`: a 401/403 from the gateway
#[derive(Debug, Snafu)]
#[snafu(visibility(pub(crate)))]
pub enum ApiError {
    /// HTTP transport or connection error (no response received).
    #[snafu(display("{operation}: {source}"))]
    Http {
        operation: &'static str,
        source: reqwest::Error,
    },

    /// Non-2xx HTTP response. Message is extracted from the server body when possible.
    #[snafu(display("{operation}: {message}"))]
    Server {
        operation: &'static str,
        message: String,
    },

    /// Credentials rejected by the gateway.
    #[snafu(display("authentication failed: token expired or invalid"))]
    Auth,

    /// Token contains characters that are not valid in an HTTP header value.
    #[snafu(display("invalid token: contains characters not valid in HTTP headers"))]
    InvalidToken,
}

pub(crate) type Result<T> = std::result::Result<T, ApiError>;
