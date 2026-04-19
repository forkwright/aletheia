//! Error types for aletheia-memory-mcp.
//!
//! Each variant maps to an rmcp error code via `impl From<Error> for
//! rmcp::ErrorData`. Server-side paths are not included in MCP messages so
//! clients never see internal filesystem layout.

use snafu::Snafu;

/// Errors produced by the memory MCP server.
#[derive(Debug, Snafu)]
#[snafu(visibility(pub(crate)))]
#[non_exhaustive]
#[expect(
    missing_docs,
    reason = "snafu error variant fields are self-documenting via display format"
)]
pub enum Error {
    /// Failed to open the knowledge store.
    #[snafu(display("failed to open knowledge store: {message}"))]
    OpenStore {
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// A knowledge store operation failed.
    #[snafu(display("knowledge store error: {message}"))]
    KnowledgeStore {
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Serialization of a response payload failed.
    #[snafu(display("serialization error: {source}"))]
    Serialization {
        source: serde_json::Error,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Caller supplied an invalid input value.
    #[snafu(display("invalid input: {message}"))]
    InvalidInput {
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// MCP transport error (stdio read/write or shutdown).
    #[snafu(display("transport error: {message}"))]
    Transport {
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Background task join failure.
    #[snafu(display("task join error: {source}"))]
    Join {
        source: tokio::task::JoinError,
        #[snafu(implicit)]
        location: snafu::Location,
    },
}

/// Result alias using this crate's [`Error`].
pub type Result<T> = std::result::Result<T, Error>;

impl From<Error> for rmcp::ErrorData {
    fn from(err: Error) -> Self {
        let message = err.to_string();
        match &err {
            Error::InvalidInput { .. } => rmcp::ErrorData::invalid_params(message, None),
            Error::OpenStore { .. }
            | Error::KnowledgeStore { .. }
            | Error::Serialization { .. }
            | Error::Transport { .. }
            | Error::Join { .. } => rmcp::ErrorData::internal_error(message, None),
        }
    }
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions may panic on failure")]
mod tests {
    use super::*;

    #[test]
    fn invalid_input_maps_to_invalid_params() {
        let err = InvalidInputSnafu {
            message: "limit must be positive".to_owned(),
        }
        .build();
        let mcp: rmcp::ErrorData = err.into();
        assert_eq!(mcp.code, rmcp::model::ErrorCode::INVALID_PARAMS);
        assert!(mcp.message.contains("limit"));
    }

    #[test]
    fn knowledge_store_error_maps_to_internal_error() {
        let err = KnowledgeStoreSnafu {
            message: "datalog query failed".to_owned(),
        }
        .build();
        let mcp: rmcp::ErrorData = err.into();
        assert_eq!(mcp.code, rmcp::model::ErrorCode::INTERNAL_ERROR);
    }

    #[test]
    fn serialization_error_maps_to_internal_error() {
        let raw = serde_json::from_str::<serde_json::Value>("{invalid").unwrap_err();
        let err = Error::Serialization {
            source: raw,
            location: snafu::Location::new("test", 0, 0),
        };
        let mcp: rmcp::ErrorData = err.into();
        assert_eq!(mcp.code, rmcp::model::ErrorCode::INTERNAL_ERROR);
    }
}
