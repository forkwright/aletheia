//! Error types for fixed rules (graph algorithms, utilities).
use snafu::Snafu;

#[derive(Debug, Snafu)]
#[snafu(visibility(pub(crate)))]
#[non_exhaustive]
pub(crate) enum FixedRuleError {
    #[snafu(display("fixed rule '{rule}' received invalid input: {message}"))]
    InvalidInput {
        rule: String,
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    #[snafu(display("graph algorithm '{algorithm}' failed: {message}"))]
    GraphAlgorithm {
        algorithm: String,
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    #[snafu(display("configuration error in '{rule}': parameter '{param}' — {message}"))]
    Config {
        rule: String,
        param: String,
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },
}
