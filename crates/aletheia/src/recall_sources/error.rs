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

    #[snafu(display("failed to parse response FROM {endpoint}"))]
    ParseResponse {
        endpoint: String,
        source: serde_json::Error,
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
