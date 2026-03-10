use snafu::prelude::*;

/// API-layer error type for the TUI HTTP client.
///
/// Covers the two failure modes the client can produce:
/// - `Http`: a transport or status error from `reqwest`
/// - `Auth`: a 401/403 from the gateway
#[derive(Debug, Snafu)]
#[snafu(visibility(pub(crate)))]
pub enum ApiError {
    /// HTTP transport or status error from a REST API call.
    #[snafu(display("{operation}: {source}"))]
    Http {
        operation: &'static str,
        source: reqwest::Error,
    },

    /// Credentials rejected by the gateway.
    #[snafu(display("authentication failed: token expired or invalid"))]
    Auth,
}

pub(crate) type Result<T> = std::result::Result<T, ApiError>;
