use snafu::Snafu;

/// Errors from the knowledge extraction pipeline.
#[derive(Debug, Snafu)]
#[snafu(visibility(pub))]
#[non_exhaustive]
pub enum ExtractionError {
    /// The LLM response could not be parsed as valid extraction JSON.
    #[snafu(display("failed to parse extraction response"))]
    ParseResponse {
        source: serde_json::Error,
        #[snafu(implicit)]
        location: snafu::Location,
    },
    /// The LLM provider returned an error during extraction.
    #[snafu(display("LLM extraction failed: {message}"))]
    LlmCall {
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },
    /// Persisting extracted knowledge to the store failed.
    #[snafu(display("failed to persist extraction: {message}"))]
    Persist {
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },
}
