//! Error types for the Datalog query parser.
//!
//! [`ParseError`] is the structured error enum for all parse-time failures.
//! It implements [`std::error::Error`] + [`Send`] + [`Sync`] + `'static`, so
//! the standard-library blanket impl provides
//! `From<ParseError> for InternalError` (via `#[snafu(context(false))]`),
//! allowing `?`-based propagation into engine-internal `InternalResult<T>` contexts.
use snafu::Snafu;

/// Structured error for all Datalog parse failures.
#[derive(Debug, Snafu)]
#[snafu(visibility(pub(crate)))]
#[non_exhaustive]
pub enum ParseError {
    /// Pest-level syntax error: unexpected token or end-of-input.
    #[snafu(display("syntax error at {span}: {message}"))]
    Syntax {
        span: String,
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Semantically invalid query structure caught after parsing.
    #[snafu(display("invalid query: {message}"))]
    InvalidQuery {
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Reference to a variable that is not bound in any rule head.
    #[snafu(display("unbound variable '{name}'"))]
    UnboundVariable {
        name: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Raw pest parser failure: wraps the pest error so span details are preserved.
    ///
    /// Constructed by callers that have a pest error in hand; `parse_script` uses
    /// [`Syntax`] instead so it can attach span information inline.
    #[snafu(display("parse failed: {source}"))]
    PestError {
        source: pest::error::Error<super::Rule>,
        #[snafu(implicit)]
        location: snafu::Location,
    },
}
