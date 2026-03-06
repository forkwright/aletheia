use snafu::Snafu;

#[derive(Debug, Snafu)]
#[snafu(visibility(pub(crate)))]
pub(crate) enum StorageError {
    #[snafu(display("write attempted in read transaction"))]
    WriteInReadTx {
        #[snafu(implicit)]
        location: snafu::Location,
    },

    #[snafu(display("transaction already committed"))]
    TransactionCommitted {
        #[snafu(implicit)]
        location: snafu::Location,
    },

    #[snafu(display("unsupported storage version: {version}"))]
    UnsupportedVersion {
        version: u64,
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
