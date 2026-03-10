//! Error types for fixed rules (graph algorithms, utilities).
use snafu::Snafu;

use crate::engine::error::BoxErr;

#[derive(Debug, Snafu)]
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

pub(crate) type FixedRuleResult<T> = std::result::Result<T, FixedRuleError>;

// `From<FixedRuleError> for BoxErr` is satisfied automatically by the stdlib
// blanket `impl<E: Error + Send + Sync + 'static> From<E> for Box<dyn Error + Send + Sync>`.
// Verify the bound holds at compile time:
const _: fn() = || {
    fn _assert_into_box_err<E: Into<BoxErr>>() {}
    _assert_into_box_err::<FixedRuleError>();
};
