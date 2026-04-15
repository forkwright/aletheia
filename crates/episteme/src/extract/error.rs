use snafu::Snafu;

/// Errors from the knowledge extraction pipeline.
#[derive(Debug, Snafu)]
#[snafu(visibility(pub))]
#[non_exhaustive]
#[expect(
    missing_docs,
    reason = "snafu error variant fields (source, message, location) are self-documenting via display format"
)]
pub enum ExtractionError {
    /// The LLM response could not be parsed as valid extraction JSON.
    ///
    /// Includes a truncated snippet of the raw response for debugging.
    #[snafu(display("failed to parse extraction response: {response_snippet}"))]
    ParseResponse {
        source: serde_json::Error,
        /// First 500 characters of the raw LLM response that failed to parse.
        response_snippet: String,
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
