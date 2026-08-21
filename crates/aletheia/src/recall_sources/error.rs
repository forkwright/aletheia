//! Recall source error types.

use snafu::prelude::*;

#[derive(Debug, Snafu)]
#[snafu(visibility(pub(crate)))]
#[non_exhaustive]
pub(crate) enum RecallSourceError {
    #[snafu(display("HTTP request to {endpoint} failed"))]
    HttpRequest {
        endpoint: String,
        source: reqwest::Error,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    #[snafu(display("failed to parse response from {endpoint}"))]
    ParseResponse {
        endpoint: String,
        source: serde_json::Error,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// The egress checkpoint refused the request before or during the exchange.
    ///
    /// WHY a variant of its own rather than `SourceUnavailable`: a refusal here is a
    /// policy decision about where this process may connect, not a statement that the
    /// source is down. Collapsing them would make an operator reading the log think
    /// Semantic Scholar was unreachable.
    #[snafu(display("egress checkpoint refused the request to {endpoint}: {message}"))]
    EgressRefused {
        endpoint: String,
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    #[snafu(display("{message}"))]
    SourceUnavailable {
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },
}
