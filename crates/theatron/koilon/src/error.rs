use snafu::prelude::*;

/// Unified error type for the TUI crate.
#[derive(Debug, Snafu)]
#[snafu(visibility(pub(crate)))]
#[expect(
    missing_docs,
    reason = "snafu error variant fields (source, message, url, context, event_type, detail) are self-documenting via display format"
)]
#[non_exhaustive]
pub enum Error {
    /// API transport or authentication error from the HTTP client.
    #[snafu(context(false))]
    Api { source: skene::api::ApiError },

    /// Token is required but was not supplied.
    #[snafu(display("{message}"))]
    TokenRequired { message: String },

    /// Gateway is unreachable (health check returned false or connection refused).
    #[snafu(display(
        "cannot reach gateway at {url}\n  Server not running. Start it with: aletheia"
    ))]
    GatewayUnreachable { url: String },

    /// Gateway rejected the request's credentials (401/403) — split out of
    /// [`Error::GatewayUnreachable`] (#6818) because the server IS running
    /// here and the fix is re-authenticating, not checking whether it started.
    #[snafu(display(
        "gateway at {url} rejected the token\n  Token invalid or expired. Re-authenticate with :reauth <token>, or restart with --logout to clear it and sign in again."
    ))]
    AuthRejected { url: String },

    /// Could not determine the OS config directory (e.g. $HOME unset).
    #[snafu(display("could not determine config directory"))]
    ConfigDir,

    /// File-system I/O error.
    #[snafu(display("{context}: {source}"))]
    Io {
        context: &'static str,
        source: std::io::Error,
    },

    /// TOML serialization error.
    #[snafu(display("TOML error: {source}"))]
    Toml { source: toml::ser::Error },

    /// Tracing subscriber initialization failed.
    #[snafu(display("initialize logging: {source}"))]
    Logging {
        source: bathron::logging::LoggingError,
    },

    /// An unexpected event type was received during SSE parsing.
    #[snafu(display("unexpected event type: {event_type}"))]
    UnexpectedEventType { event_type: String },

    /// Malformed or missing data in an incoming SSE event.
    #[snafu(display("malformed event data: {detail}"))]
    MalformedEventData { detail: String },

    /// SSE protocol state machine received an event out of sequence.
    #[snafu(display("protocol mismatch: {detail}"))]
    ProtocolMismatch { detail: String },

    /// The user aborted the setup wizard (pressed Esc or Ctrl+C).
    #[snafu(display("setup wizard aborted"))]
    WizardAborted,
}

/// Convenience alias for `Result` with the TUI [`Error`] type.
pub type Result<T, E = Error> = std::result::Result<T, E>;
