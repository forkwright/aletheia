use snafu::Snafu;

#[derive(Debug, Snafu)]
#[snafu(visibility(pub(crate)))]
pub(crate) enum FixedRuleError {
    #[snafu(display("bad edge weight: {message}"))]
    BadEdgeWeight {
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    #[snafu(display("node not found: {message}"))]
    NodeNotFound {
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    #[snafu(display("option not found: {message}"))]
    OptionNotFound {
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
