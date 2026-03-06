use snafu::Snafu;

#[derive(Debug, Snafu)]
#[snafu(visibility(pub(crate)))]
pub(crate) enum QueryError {
    #[snafu(display("compilation failed: {message}"))]
    CompilationFailed {
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    #[snafu(display("assertion failure: {message}"))]
    AssertionFailure {
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    #[snafu(display("stratification failed: {message}"))]
    StratificationFailed {
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    #[snafu(display("{message}"))]
    Internal {
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },
}
