use snafu::prelude::*;

/// Unified error type for the TUI crate.
#[derive(Debug, Snafu)]
#[snafu(visibility(pub(crate)))]
pub enum Error {
    /// HTTP transport or status error from a REST API call.
    #[snafu(display("{operation}: {source}"))]
    Http {
        operation: &'static str,
        source: reqwest::Error,
    },

    /// Credentials rejected by the gateway.
    #[snafu(display("authentication failed: token expired or invalid"))]
    Auth,

    /// Token is required but was not supplied.
    #[snafu(display("{message}"))]
    TokenRequired { message: String },

    /// Gateway is unreachable (health check returned false or connection refused).
    #[snafu(display("cannot reach gateway at {url}. Is it running?"))]
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
