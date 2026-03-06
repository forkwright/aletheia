use snafu::Snafu;

#[derive(Debug, Snafu)]
#[snafu(visibility(pub(crate)))]
pub(crate) enum RuntimeError {
    #[snafu(display("relation access error: {message}"))]
    RelationAccess {
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    #[snafu(display("arity mismatch: {message}"))]
    ArityMismatch {
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    #[snafu(display("transaction failed: {message}"))]
    TransactionFailed {
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
