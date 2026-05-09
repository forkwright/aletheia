//! Signal-specific error types.

use snafu::Snafu;

/// Errors from Signal JSON-RPC communication and envelope processing.
#[derive(Debug, Snafu)]
#[snafu(visibility(pub(crate)))]
#[non_exhaustive]
#[expect(
    missing_docs,
    reason = "snafu error variant fields (source, location, code, message) are self-documenting via display format"
)]
pub enum Error {
    /// JSON-RPC returned an error response.
    #[snafu(display("signal RPC error {code}: {message}"))]
    Rpc {
        code: i64,
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// HTTP transport error communicating with signal-cli daemon.
    #[snafu(display("signal HTTP error: {source}"))]
    Http {
        source: reqwest::Error,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// No Signal account configured for the requested operation.
    #[snafu(display("no signal account: {account_id}"))]
    NoAccount {
        account_id: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// JSON serialization/deserialization failure.
    #[snafu(display("signal JSON error: {source}"))]
    Json {
        source: serde_json::Error,
        #[snafu(implicit)]
        location: snafu::Location,
    },
}

/// Convenience alias for `Result` with Signal's [`Error`] type.
pub(crate) type Result<T> = std::result::Result<T, Error>;

impl koina::error_class::Classifiable for Error {
    fn class(&self) -> koina::error_class::ErrorClass {
        use koina::error_class::ErrorClass;
        match self {
            Self::Http { source, .. } if source.is_timeout() || source.is_connect() => {
                ErrorClass::Transient
            }
            Self::Http { .. } => ErrorClass::Unknown,
            Self::Rpc { .. } | Self::NoAccount { .. } | Self::Json { .. } => ErrorClass::Permanent,
        }
    }

    fn action(&self) -> koina::error_class::ErrorAction {
        use koina::error_class::{ErrorAction, ErrorClass};
        match self.class() {
            ErrorClass::Transient => ErrorAction::Retry {
                max_attempts: 3,
                backoff_base_ms: 500,
            },
            ErrorClass::Permanent => ErrorAction::Surface {
                user_message: self.to_string(),
            },
            _ => ErrorAction::Escalate,
        }
    }
}
