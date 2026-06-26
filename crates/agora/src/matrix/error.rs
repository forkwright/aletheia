//! Matrix-specific error types.

use snafu::Snafu;

/// Result alias for Matrix client operations.
pub(crate) type Result<T> = std::result::Result<T, Error>;

/// Matrix client and wire errors.
#[derive(Debug, Snafu)]
#[snafu(visibility(pub(crate)))]
#[non_exhaustive]
pub enum Error {
    /// HTTP transport or response decoding failed.
    #[snafu(display("Matrix HTTP error: {source}"))]
    Http {
        /// Underlying reqwest error.
        source: reqwest::Error,
        #[snafu(implicit)]
        /// Source location captured by snafu.
        location: snafu::Location,
    },

    /// JSON encoding or decoding failed.
    #[snafu(display("Matrix JSON error: {source}"))]
    Json {
        /// Underlying serde JSON error.
        source: serde_json::Error,
        #[snafu(implicit)]
        /// Source location captured by snafu.
        location: snafu::Location,
    },

    /// Matrix API returned an unsuccessful status.
    #[snafu(display("Matrix API error {status}: {message}"))]
    Api {
        /// HTTP status code returned by the homeserver.
        status: u16,
        /// Matrix API error message.
        message: String,
        #[snafu(implicit)]
        /// Source location captured by snafu.
        location: snafu::Location,
    },

    /// Downstream receiver has been dropped; sync should stop immediately.
    #[snafu(display("Matrix sync receiver dropped"))]
    ReceiverDropped {
        #[snafu(implicit)]
        /// Source location captured by snafu.
        location: snafu::Location,
    },
}
