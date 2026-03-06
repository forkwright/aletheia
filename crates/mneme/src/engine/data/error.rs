use snafu::Snafu;

#[derive(Debug, Snafu)]
#[snafu(visibility(pub(crate)))]
pub(crate) enum DataError {
    #[snafu(display("type coercion failed: {message}"))]
    TypeCoercion {
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    #[snafu(display("invalid value: {message}"))]
    InvalidValue {
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    #[snafu(display("unbound variable: {name}"))]
    UnboundVariable {
        name: String,
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
