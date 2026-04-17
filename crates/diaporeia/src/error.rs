//! Diaporeia error types.

use snafu::Snafu;

/// Errors from diaporeia MCP operations.
#[derive(Debug, Snafu)]
#[snafu(visibility(pub(crate)))]
#[non_exhaustive]
#[expect(
    missing_docs,
    reason = "snafu error variant fields (id, message, source, location) are self-documenting via display format"
)]
pub enum Error {
    /// Nous agent not found.
    #[snafu(display("nous agent not found: {id}"))]
    NousNotFound {
        id: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Session not found.
    #[snafu(display("session not found: {id}"))]
    SessionNotFound {
        id: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Nous pipeline error.
    #[snafu(display("nous pipeline error: {message}"))]
    Pipeline {
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Session store error.
    #[snafu(display("session store error: {source}"))]
    SessionStore {
        source: mneme::error::Error,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Serialization error.
    #[snafu(display("serialization error: {source}"))]
    Serialization {
        source: serde_json::Error,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Transport error.
    #[snafu(display("transport error: {message}"))]
    Transport {
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// I/O error reading a workspace file.
    #[snafu(display("workspace file error: {source}"))]
    WorkspaceFile {
        source: std::io::Error,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Caller lacks the required role for this operation.
    #[snafu(display("unauthorized: {message}"))]
    Unauthorized {
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// The knowledge graph MCP surface is disabled or not configured.
    #[snafu(display(
        "knowledge graph MCP surface is disabled; \
         set mcp.knowledge_graph.enabled = true in aletheia.toml"
    ))]
    KnowledgeStoreUnavailable {
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

    /// The requested fact was not found.
    #[snafu(display("fact not found: {id}"))]
    FactNotFound {
        id: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// The caller supplied an invalid input value.
    #[snafu(display("invalid input: {message}"))]
    InvalidInput {
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },
}

/// Result alias using diaporeia's [`Error`] type.
pub type Result<T> = std::result::Result<T, Error>;

/// Convert a diaporeia `Error` into an rmcp `ErrorData` for tool return types.
///
/// Maps each variant to the appropriate MCP error code and strips server-side
/// file paths from the message before it reaches the client.
impl From<Error> for rmcp::ErrorData {
    fn from(err: Error) -> Self {
        let message = crate::sanitize::strip_paths(&err.to_string());
        match &err {
            // NOTE: client provided an invalid agent or session ID: include what wasn't found
            Error::NousNotFound { .. }
            | Error::SessionNotFound { .. }
            | Error::FactNotFound { .. }
            | Error::InvalidInput { .. } => rmcp::ErrorData::invalid_params(message, None),
            // WHY: authorization failures return a clear message so clients can
            // distinguish access-denied from invalid-params.
            Error::Unauthorized { .. } => {
                rmcp::ErrorData::new(rmcp::model::ErrorCode(-32001), message, None)
            }
            // WHY: feature-disabled surfaces a distinct code so clients can
            // detect the configuration issue rather than treating it as a
            // transient server error.
            Error::KnowledgeStoreUnavailable { .. } => {
                rmcp::ErrorData::new(rmcp::model::ErrorCode(-32002), message, None)
            }
            // WHY: server-side failures expose only a sanitized message, never internal details
            Error::Pipeline { .. }
            | Error::SessionStore { .. }
            | Error::KnowledgeStore { .. }
            | Error::Serialization { .. }
            | Error::Transport { .. }
            | Error::WorkspaceFile { .. } => rmcp::ErrorData::internal_error(message, None),
        }
    }
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions may panic on failure")]
mod tests {
    use snafu::IntoError as _;

    use super::*;

    #[test]
    fn nous_not_found_maps_to_invalid_params() {
        let err = NousNotFoundSnafu {
            id: "missing-agent".to_string(),
        }
        .build();
        let mcp: rmcp::ErrorData = err.into();
        assert_eq!(mcp.code, rmcp::model::ErrorCode::INVALID_PARAMS);
        assert!(mcp.message.contains("missing-agent"));
    }

    #[test]
    fn session_not_found_maps_to_invalid_params() {
        let err = SessionNotFoundSnafu {
            id: "no-such-session".to_string(),
        }
        .build();
        let mcp: rmcp::ErrorData = err.into();
        assert_eq!(mcp.code, rmcp::model::ErrorCode::INVALID_PARAMS);
    }

    #[test]
    fn pipeline_error_maps_to_internal_error() {
        let err = PipelineSnafu {
            message: "actor channel closed".to_string(),
        }
        .build();
        let mcp: rmcp::ErrorData = err.into();
        assert_eq!(mcp.code, rmcp::model::ErrorCode::INTERNAL_ERROR);
    }

    #[test]
    fn pipeline_error_strips_server_path() {
        let err = PipelineSnafu {
            message: "error reading /home/alice/project/nous.rs: permission denied".to_string(),
        }
        .build();
        let mcp: rmcp::ErrorData = err.into();
        assert!(
            !mcp.message.contains("/home/alice"),
            "server path must not reach the client"
        );
        assert!(mcp.message.contains("[server path]"));
    }

    #[test]
    fn serialization_error_maps_to_internal_error() {
        let raw_err = serde_json::from_str::<serde_json::Value>("{invalid").unwrap_err();
        let err = SerializationSnafu {}.into_error(raw_err);
        let mcp: rmcp::ErrorData = err.into();
        assert_eq!(mcp.code, rmcp::model::ErrorCode::INTERNAL_ERROR);
    }

    #[test]
    fn transport_error_maps_to_internal_error() {
        let err = TransportSnafu {
            message: "connection reset by peer".to_string(),
        }
        .build();
        let mcp: rmcp::ErrorData = err.into();
        assert_eq!(mcp.code, rmcp::model::ErrorCode::INTERNAL_ERROR);
        assert!(mcp.message.contains("connection reset"));
    }

    #[test]
    fn workspace_file_error_maps_to_internal_error() {
        let io_err = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "permission denied");
        let err = WorkspaceFileSnafu {}.into_error(io_err);
        let mcp: rmcp::ErrorData = err.into();
        assert_eq!(mcp.code, rmcp::model::ErrorCode::INTERNAL_ERROR);
    }

    #[test]
    fn workspace_file_error_strips_server_path_from_message() {
        let io_err = std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "No such file: /srv/aletheia/nous/syn/SOUL.md",
        );
        let err = WorkspaceFileSnafu {}.into_error(io_err);
        let mcp: rmcp::ErrorData = err.into();
        assert!(
            !mcp.message.contains("/srv/aletheia"),
            "internal path must not reach client: {}",
            mcp.message
        );
    }

    #[test]
    fn nous_not_found_message_contains_agent_id() {
        let err = NousNotFoundSnafu {
            id: "syn".to_string(),
        }
        .build();
        let mcp: rmcp::ErrorData = err.into();
        assert!(
            mcp.message.contains("syn"),
            "message must identify the missing agent"
        );
    }

    #[test]
    fn unauthorized_maps_to_custom_error_code() {
        let err = UnauthorizedSnafu {
            message: "session_create requires operator role or above".to_string(),
        }
        .build();
        let mcp: rmcp::ErrorData = err.into();
        assert_eq!(
            mcp.code,
            rmcp::model::ErrorCode(-32001),
            "unauthorized must use custom error code -32001"
        );
        assert!(mcp.message.contains("operator"));
    }
}
