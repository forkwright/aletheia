use snafu::prelude::*;

/// Unified error type for the TUI crate.
#[derive(Debug, Snafu)]
#[snafu(visibility(pub(crate)))]
pub enum Error {
    /// API transport or authentication error from the HTTP client.
    #[snafu(context(false))]
    Api { source: crate::api::error::ApiError },

    /// Token is required but was not supplied.
    #[snafu(display("{message}"))]
    TokenRequired { message: String },

    /// Gateway is unreachable (health check returned false or connection refused).
    #[snafu(display(
        "cannot reach gateway at {url}\n  Server not running. Start it with: aletheia"
    ))]
    GatewayUnreachable { url: String },

    /// Could not determine the OS config directory (e.g. $HOME unset).
    #[snafu(display("could not determine config directory"))]
    ConfigDir,

    /// File-system I/O error.
    #[snafu(display("{context}: {source}"))]
    Io {
        context: &'static str,
        source: std::io::Error,
    },

    /// YAML serialization / deserialization error.
    #[snafu(display("YAML error: {source}"))]
    Yaml { source: serde_yaml::Error },

    /// Invalid `tracing` filter directive.
    #[snafu(display("invalid log directive: {source}"))]
    LogDirective {
        source: tracing_subscriber::filter::ParseError,
    },
}

pub(crate) type Result<T, E = Error> = std::result::Result<T, E>;
