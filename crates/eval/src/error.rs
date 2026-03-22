//! Error types for the eval framework.

use snafu::Snafu;

/// Errors from eval framework operations.
#[derive(Debug, Snafu)]
#[snafu(visibility(pub))]
#[non_exhaustive]
pub enum Error {
    /// HTTP request failed.
    #[snafu(display("HTTP request failed: {source}"))]
    Http {
        source: reqwest::Error,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Unexpected HTTP status from the server.
    #[snafu(display("unexpected status {status} from {endpoint}: {body}"))]
    UnexpectedStatus {
        endpoint: String,
        status: u16,
        body: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// SSE stream parse error.
    #[snafu(display("SSE parse error: {message}"))]
    SseParse {
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Scenario assertion failed.
    #[snafu(display("assertion failed: {message}"))]
    Assertion {
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// JSON serialization or deserialization failed.
    #[snafu(display("JSON error: {source}"))]
    Json {
        source: serde_json::Error,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Scenario exceeded the configured timeout.
    #[snafu(display("timeout after {elapsed_ms}ms"))]
    Timeout {
        elapsed_ms: u64,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// No agents are registered on the target instance.
    #[snafu(display("no agents available: agent list is empty"))]
    NoAgentsAvailable {
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// File I/O error during result persistence.
    #[snafu(display("I/O error: {source}"))]
    Io {
        source: std::io::Error,
        #[snafu(implicit)]
        location: snafu::Location,
    },
}

/// Convenience alias for `Result` with eval's [`Error`] type.
pub type Result<T> = std::result::Result<T, Error>;
