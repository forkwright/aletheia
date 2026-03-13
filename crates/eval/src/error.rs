//! Error types for the eval framework.

use snafu::Snafu;

#[derive(Debug, Snafu)]
#[snafu(visibility(pub))]
#[non_exhaustive]
pub enum Error {
    #[snafu(display("HTTP request failed: {source}"))]
    Http {
        source: reqwest::Error,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    #[snafu(display("unexpected status {status} from {endpoint}: {body}"))]
    UnexpectedStatus {
        endpoint: String,
        status: u16,
        body: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    #[snafu(display("SSE parse error: {message}"))]
    SseParse {
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    #[snafu(display("assertion failed: {message}"))]
    Assertion {
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    #[snafu(display("JSON error: {source}"))]
    Json {
        source: serde_json::Error,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    #[snafu(display("timeout after {elapsed_ms}ms"))]
    Timeout {
        elapsed_ms: u64,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    #[snafu(display("no agents available: agent list is empty"))]
    NoAgentsAvailable {
        #[snafu(implicit)]
        location: snafu::Location,
    },
}

pub type Result<T> = std::result::Result<T, Error>;
