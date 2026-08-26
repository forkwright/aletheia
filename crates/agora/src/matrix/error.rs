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
        /// Privacy-safe Matrix error code or fixed rejection description.
        message: String,
        #[snafu(implicit)]
        /// Source location captured by snafu.
        location: snafu::Location,
    },

    /// Matrix wire response violated a required protocol invariant.
    #[snafu(display("Matrix protocol error: {reason}"))]
    Protocol {
        /// Stable, privacy-safe description of the violated invariant.
        reason: &'static str,
        #[snafu(implicit)]
        /// Source location captured by snafu.
        location: snafu::Location,
    },

    /// Durable cursor storage was unavailable or invalid.
    #[snafu(display("Matrix cursor {operation} failed: {source}"))]
    Cursor {
        /// Cursor operation that failed.
        operation: &'static str,
        /// Underlying durable-store failure.
        source: std::io::Error,
        #[snafu(implicit)]
        /// Source location captured by snafu.
        location: snafu::Location,
    },

    /// A limited room timeline would skip history if checkpointed.
    #[snafu(display("Matrix sync contained {limited_rooms} limited room timelines"))]
    TimelineGap {
        /// Number of joined rooms whose timeline declared a gap.
        limited_rooms: usize,
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
