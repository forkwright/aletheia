use snafu::Snafu;

#[derive(Debug, Snafu)]
#[snafu(visibility(pub(crate)))]
pub(crate) enum FtsError {
    #[snafu(display("tokenizer configuration error: {message}"))]
    TokenizerConfig {
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    #[snafu(display("indexing failed: {message}"))]
    IndexingFailed {
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
