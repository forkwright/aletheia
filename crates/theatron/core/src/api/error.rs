//! API-layer error type for the HTTP client.
//!
//! Covers three failure modes:
//! - `Http`: a transport or connection error from `reqwest`
//! - `Server`: a non-2xx HTTP response with a human-readable message from the server body
//! - `Auth`: a 401/403 from the gateway

use snafu::prelude::*;

/// Errors returned by [`super::ApiClient`] methods.
#[derive(Debug, Snafu)]
#[snafu(visibility(pub(crate)))]
#[non_exhaustive]
pub enum ApiError {
    /// HTTP transport or connection error (no response received).
    #[snafu(display("{operation}: {source}"))]
    Http {
        /// Which API call failed.
        operation: &'static str,
        /// Underlying reqwest error.
        source: reqwest::Error,
    },

    /// Non-2xx HTTP response. Message is extracted from the server body when possible.
    #[snafu(display("{operation}: {message}"))]
    Server {
        /// Which API call failed.
        operation: &'static str,
        /// Human-readable error from the server.
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
