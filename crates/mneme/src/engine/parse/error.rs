use snafu::Snafu;

#[derive(Debug, Snafu)]
#[snafu(visibility(pub(crate)))]
pub(crate) enum CozoParseError {
    #[snafu(display("syntax error: {message}"))]
    Syntax {
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    #[snafu(display("invalid expression: {message}"))]
    InvalidExpression {
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
