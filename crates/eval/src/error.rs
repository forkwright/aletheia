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
        /// Underlying reqwest error.
        source: reqwest::Error,
        /// Source location where the error was created.
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Unexpected HTTP status from the server.
    #[snafu(display("unexpected status {status} from {endpoint}: {body}"))]
    UnexpectedStatus {
        /// The endpoint URL that returned the unexpected status.
        endpoint: String,
        /// HTTP status code that was returned.
        status: u16,
        /// Response body (for diagnostic context).
        body: String,
        /// Source location where the error was created.
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// SSE stream parse error.
    #[snafu(display("SSE parse error: {message}"))]
    SseParse {
        /// Parser failure detail.
        message: String,
        /// Source location where the error was created.
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Scenario assertion failed.
    #[snafu(display("assertion failed: {message}"))]
    Assertion {
        /// Assertion failure detail.
        message: String,
        /// Source location where the error was created.
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// JSON serialization or deserialization failed.
    #[snafu(display("JSON error: {source}"))]
    Json {
        /// Underlying `serde_json` error.
        source: serde_json::Error,
        /// Source location where the error was created.
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Scenario exceeded the configured timeout.
    #[snafu(display("timeout after {elapsed_ms}ms"))]
    Timeout {
        /// Elapsed time in milliseconds when the timeout fired.
        elapsed_ms: u64,
        /// Source location where the error was created.
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// No agents are registered on the target instance.
    #[snafu(display("no agents available: agent list is empty"))]
    NoAgentsAvailable {
        /// Source location where the error was created.
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// File I/O error during result persistence.
    #[snafu(display("I/O error: {source}"))]
    Io {
        /// Underlying `std::io` error.
        source: std::io::Error,
        /// Source location where the error was created.
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Benchmark question failed to produce a scorable answer.
    #[snafu(display("benchmark error: {message}"))]
    Benchmark {
        /// Human-readable failure detail.
        message: String,
        /// Source location where the error was created.
        #[snafu(implicit)]
        location: snafu::Location,
    },
}

/// Convenience alias for `Result` with eval's [`Error`] type.
pub type Result<T> = std::result::Result<T, Error>;
