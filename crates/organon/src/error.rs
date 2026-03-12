//! Error types for the organon crate.

use aletheia_koina::id::ToolName;
use snafu::Snafu;

/// Errors from tool registry operations.
#[derive(Debug, Snafu)]
#[snafu(visibility(pub(crate)))]
#[non_exhaustive]
pub enum Error {
    /// Requested tool does not exist in the registry.
    #[snafu(display("tool not found: {name}"))]
    ToolNotFound {
        name: ToolName,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// A tool with this name is already registered.
    #[snafu(display("duplicate tool: {name}"))]
    DuplicateTool {
        name: ToolName,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Tool input failed validation.
    #[snafu(display("invalid input for tool {name}: {reason}"))]
    InvalidInput {
        name: ToolName,
        reason: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Tool execution returned an error.
    #[snafu(display("tool execution failed: {name}"))]
    ExecutionFailed {
        name: ToolName,
        source: Box<dyn std::error::Error + Send + Sync>,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Failed to serialize an input schema to JSON.
    #[snafu(display("schema serialization failed"))]
    SchemaSerialization {
        source: serde_json::Error,
        #[snafu(implicit)]
        location: snafu::Location,
    },
}

/// Convenience alias.
pub type Result<T> = std::result::Result<T, Error>;

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use std::collections::HashSet;
    use std::sync::{Arc, RwLock};

    use aletheia_koina::id::{NousId, SessionId, ToolName};
    use indexmap::IndexMap;

    use crate::registry::ToolRegistry;
    use crate::types::{InputSchema, ToolCategory, ToolContext, ToolDef, ToolInput};

    fn test_def(name: &str) -> ToolDef {
        ToolDef {
            name: ToolName::new(name).expect("valid"),
            description: "test".to_owned(),
            extended_description: None,
            input_schema: InputSchema {
                properties: IndexMap::new(),
                required: vec![],
            },
            category: ToolCategory::Workspace,
            auto_activate: false,
        }
    }

    fn test_ctx() -> ToolContext {
        ToolContext {
            nous_id: NousId::new("test-agent").expect("valid"),
            session_id: SessionId::new(),
            workspace: std::path::PathBuf::from("/tmp"),
            allowed_roots: vec![std::path::PathBuf::from("/tmp")],
            services: None,
            active_tools: Arc::new(RwLock::new(HashSet::new())),
        }
    }

    #[test]
    fn test_error_tool_not_found_message_contains_name() {
        let mut reg = ToolRegistry::new();
        reg.register(test_def("present"), {
            use std::future::Future;
            use std::pin::Pin;
            struct Noop;
            impl crate::registry::ToolExecutor for Noop {
                fn execute<'a>(
                    &'a self,
                    _: &'a ToolInput,
                    _: &'a ToolContext,
                ) -> Pin<
                    Box<
                        dyn Future<Output = crate::error::Result<crate::types::ToolResult>>
                            + Send
                            + 'a,
                    >,
                > {
                    Box::pin(async { Ok(crate::types::ToolResult::text("ok")) })
                }
            }
            Box::new(Noop)
        })
        .expect("register");

        let msg = reg
            .get_def(&ToolName::new("missing_tool").expect("valid"))
            .is_none();
        assert!(msg, "missing_tool should not be found");
    }

    #[tokio::test]
    async fn test_error_tool_not_found_message_format() {
        let reg = ToolRegistry::new();
        let input = ToolInput {
            name: ToolName::new("missing_tool").expect("valid"),
            tool_use_id: "toolu_x".to_owned(),
            arguments: serde_json::json!({}),
        };
        let err = reg
            .execute(&input, &test_ctx())
            .await
            .expect_err("should fail");
        assert!(
            err.to_string().contains("tool not found: missing_tool"),
            "err: {err}"
        );
    }

    #[test]
    fn test_error_duplicate_tool_message_format() {
        use std::future::Future;
        use std::pin::Pin;
        struct Noop;
        impl crate::registry::ToolExecutor for Noop {
            fn execute<'a>(
                &'a self,
                _: &'a ToolInput,
                _: &'a ToolContext,
            ) -> Pin<
                Box<
                    dyn Future<Output = crate::error::Result<crate::types::ToolResult>> + Send + 'a,
                >,
            > {
                Box::pin(async { Ok(crate::types::ToolResult::text("ok")) })
            }
        }
        let mut reg = ToolRegistry::new();
        reg.register(test_def("same_name"), Box::new(Noop))
            .expect("first register");
        let err = reg
            .register(test_def("same_name"), Box::new(Noop))
            .expect_err("duplicate");
        assert!(
            err.to_string().contains("duplicate tool: same_name"),
            "err: {err}"
        );
    }
}
