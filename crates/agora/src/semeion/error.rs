//! Signal-specific error types.

use snafu::Snafu;

/// Signal-specific error variants.
#[derive(Debug, Snafu)]
#[snafu(visibility(pub(crate)))]
#[non_exhaustive]
pub enum Error {
    /// JSON-RPC returned an error response.
    #[snafu(display("signal RPC error {code}: {message}"))]
    Rpc {
        code: i64,
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// HTTP transport error communicating with signal-cli daemon.
    #[snafu(display("signal HTTP error: {source}"))]
    Http {
        source: reqwest::Error,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// No Signal account configured for the requested operation.
    #[snafu(display("no signal account: {account_id}"))]
    NoAccount {
        account_id: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// JSON serialization/deserialization failure.
    #[snafu(display("signal JSON error: {source}"))]
    Json {
        source: serde_json::Error,
        #[snafu(implicit)]
        location: snafu::Location,
    },
}

/// Convenience alias for signal client results.
pub type Result<T> = std::result::Result<T, Error>;
