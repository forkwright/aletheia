//! Signal-specific error types.

use snafu::Snafu;

/// Privacy-safe classification of an HTTP transport failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum TransportFailure {
    /// The daemon connection could not be established, so it could not accept the request.
    Connect,
    /// The request timed out after connection establishment may have begun.
    Timeout,
    /// Another transport failure whose delivery outcome is unknown.
    Other,
}

/// Why a destructive receive may have consumed an unknown number of messages.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ReceiveLossReason {
    /// The HTTP request failed after connection establishment may have begun.
    Transport,
    /// The daemon returned an HTTP status that cannot carry a receive result.
    HttpStatus,
    /// The response was not a valid correlated JSON-RPC response.
    Protocol,
    /// The daemon returned an RPC error instead of a receive result.
    Rpc,
    /// The response's `result` member was not an envelope array.
    ResultShape,
    /// A response wrapper named a different or missing account.
    AccountMismatch,
}

/// Errors from Signal JSON-RPC communication and envelope processing.
#[derive(Snafu)]
#[snafu(visibility(pub(crate)))]
#[expect(
    missing_docs,
    reason = "snafu error variant fields are documented by their public field types"
)]
#[non_exhaustive]
pub enum Error {
    /// JSON-RPC returned an error response.
    #[snafu(display("signal RPC error {code}"))]
    Rpc {
        code: i64,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// HTTP transport error communicating with signal-cli daemon.
    #[snafu(display("signal HTTP transport failure ({kind:?})"))]
    Http {
        kind: TransportFailure,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Signal daemon returned an unexpected HTTP status.
    #[snafu(display("signal HTTP status {status}"))]
    HttpStatus {
        status: u16,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Signal daemon base URL could not be parsed after normalization.
    #[snafu(display("invalid signal daemon URL"))]
    InvalidUrl {
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Signal daemon base URL is plaintext to a host that is not loopback.
    #[snafu(display("refusing insecure signal daemon transport"))]
    InsecureTransport {
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// No Signal account configured for the requested operation.
    #[snafu(display("no signal account configured for the requested operation"))]
    NoAccount {
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Local JSON request serialization failure.
    #[snafu(display("signal request serialization failure"))]
    Json {
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Daemon response could not be decoded or did not match this request.
    #[snafu(display("signal response protocol failure"))]
    Protocol {
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// A destructive receive produced no usable, correlated result.
    #[snafu(display("signal receive outcome unknown ({reason:?})"))]
    ReceiveOutcomeUnknown {
        reason: ReceiveLossReason,
        #[snafu(implicit)]
        location: snafu::Location,
    },
}

// SECURITY: derived Debug would expose reqwest URLs, remote error strings,
// JSON fragments, and source locations. Keep diagnostics on the same closed
// vocabulary as Display.
impl std::fmt::Debug for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "SignalError({self})")
    }
}

impl Error {
    pub(crate) fn from_http(error: &reqwest::Error) -> Self {
        let kind = if error.is_connect() {
            TransportFailure::Connect
        } else if error.is_timeout() {
            TransportFailure::Timeout
        } else {
            TransportFailure::Other
        };
        Self::Http {
            kind,
            location: snafu::location!(),
        }
    }

    pub(crate) fn json() -> Self {
        Self::Json {
            location: snafu::location!(),
        }
    }

    pub(crate) fn receive_unknown(reason: ReceiveLossReason) -> Self {
        Self::ReceiveOutcomeUnknown {
            reason,
            location: snafu::location!(),
        }
    }

    /// True only when the daemon could not accept the request. Timeouts,
    /// response decoding, and status failures must not be replayed because
    /// Signal exposes no idempotency key.
    pub(crate) fn safe_to_retry_delivery(&self) -> bool {
        matches!(
            self,
            Self::Http {
                kind: TransportFailure::Connect,
                ..
            }
        )
    }

    /// Whether a failed send may nevertheless have become visible in Signal.
    pub(crate) fn delivery_outcome_ambiguous(&self) -> bool {
        matches!(
            self,
            Self::Http {
                kind: TransportFailure::Timeout | TransportFailure::Other,
                ..
            } | Self::HttpStatus { .. }
                | Self::Rpc { .. }
                | Self::Protocol { .. }
        )
    }

    /// Whether a failed destructive receive may have consumed messages.
    pub(crate) fn receive_outcome_ambiguous(&self) -> bool {
        matches!(self, Self::ReceiveOutcomeUnknown { .. })
    }
}

/// Convenience alias for `Result` with Signal's [`Error`] type.
pub(crate) type Result<T> = std::result::Result<T, Error>;

impl koina::error_class::Classifiable for Error {
    fn class(&self) -> koina::error_class::ErrorClass {
        use koina::error_class::ErrorClass;
        match self {
            Self::Http {
                kind: TransportFailure::Connect,
                ..
            } => ErrorClass::Transient,
            Self::Http { .. } | Self::ReceiveOutcomeUnknown { .. } => ErrorClass::Unknown,
            Self::Rpc { .. }
            | Self::HttpStatus { .. }
            | Self::InvalidUrl { .. }
            | Self::InsecureTransport { .. }
            | Self::NoAccount { .. }
            | Self::Json { .. }
            | Self::Protocol { .. } => ErrorClass::Permanent,
        }
    }

    fn action(&self) -> koina::error_class::ErrorAction {
        use koina::error_class::ErrorAction;
        match self {
            Self::Http {
                kind: TransportFailure::Connect,
                ..
            } => ErrorAction::Retry {
                max_attempts: 3,
                backoff_base_ms: 500,
            },
            Self::Http { .. } | Self::ReceiveOutcomeUnknown { .. } => ErrorAction::Escalate,
            Self::Rpc { .. }
            | Self::HttpStatus { .. }
            | Self::InvalidUrl { .. }
            | Self::InsecureTransport { .. }
            | Self::NoAccount { .. }
            | Self::Json { .. }
            | Self::Protocol { .. } => ErrorAction::Surface {
                user_message: self.to_string(),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_debug_never_carries_remote_detail() {
        let error = Error::Rpc {
            code: -32_603,
            location: snafu::location!(),
        };
        assert_eq!(format!("{error:?}"), "SignalError(signal RPC error -32603)");
    }

    #[test]
    fn retry_and_ambiguity_are_disjoint() {
        let connect = Error::Http {
            kind: TransportFailure::Connect,
            location: snafu::location!(),
        };
        let timeout = Error::Http {
            kind: TransportFailure::Timeout,
            location: snafu::location!(),
        };

        assert!(connect.safe_to_retry_delivery());
        assert!(!connect.delivery_outcome_ambiguous());
        assert!(!timeout.safe_to_retry_delivery());
        assert!(timeout.delivery_outcome_ambiguous());
    }
}
