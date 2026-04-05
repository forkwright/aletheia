//! Bookkeeper tools for prompt archival and worktree cleanup.
//!
//! - `tamias` (ταμίας — steward/treasurer): archive completed prompts
//! - `katharos` (καθαρός — clean): remove stale worktrees and artifacts

use std::future::Future;
use std::pin::Pin;

use indexmap::IndexMap;

use aletheia_koina::id::ToolName;

use crate::error::Result;
use crate::registry::{ToolExecutor, ToolRegistry};
use crate::types::{
    InputSchema, PropertyDef, PropertyType, Reversibility, ToolCategory, ToolContext, ToolDef,
    ToolInput, ToolResult,
};

/// Stub executor for bookkeeper tools.
///
/// WHY: Real implementations require integration with the dispatch store
/// and git subprocess calls. Stubs are registered now so that tool schemas
/// are available to agents; implementations land in a follow-up.
struct BookkeeperStub {
    tool_name: &'static str,
}

impl ToolExecutor for BookkeeperStub {
    fn execute<'a>(
        &'a self,
        _input: &'a ToolInput,
        _ctx: &'a ToolContext,
    ) -> Pin<Box<dyn Future<Output = Result<ToolResult>> + Send + 'a>> {
        let name = self.tool_name;
        Box::pin(async move {
            tracing::warn!(tool = name, "bookkeeper tool invoked before implementation");
            Ok(ToolResult::error(format!(
                "bookkeeper: {name} is not yet implemented"
            )))
        })
    }
}

// -- tamias (ταμίας -- steward/treasurer) ----------------------------------

fn tamias_def() -> ToolDef {
    ToolDef {
        name: ToolName::from_static("tamias"),
        description: "Archive completed prompts: move prompt files from queue/ to done/ \
            after successful merge. Optionally archives by prompt number or batch."
            .to_owned(),
        extended_description: None,
        input_schema: InputSchema {
            properties: IndexMap::from([
                (
                    "prompt_number".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::Integer,
                        description: "Specific prompt number to archive".to_owned(),
                        enum_values: None,
                        default: None,
                    },
                ),
                (
                    "project".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "Project slug to scope the archive operation".to_owned(),
                        enum_values: None,
                        default: None,
                    },
                ),
                (
                    "batch".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::Boolean,
                        description: "Archive all merged prompts in the queue (default: false)"
                            .to_owned(),
                        enum_values: None,
                        default: Some(serde_json::json!(false)),
                    },
                ),
                (
                    "dry_run".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::Boolean,
                        description:
                            "List what would be archived without moving files (default: false)"
                                .to_owned(),
                        enum_values: None,
                        default: Some(serde_json::json!(false)),
                    },
                ),
            ]),
            required: vec!["project".to_owned()],
        },
        category: ToolCategory::System,
        reversibility: Reversibility::PartiallyReversible,
        auto_activate: false,
    }
}

// -- katharos (καθαρός -- clean) -------------------------------------------

fn katharos_def() -> ToolDef {
    ToolDef {
        name: ToolName::from_static("katharos"),
        description: "Clean up stale dispatch artifacts: remove orphaned worktrees, \
            close merged PR branches, and delete expired lock files."
            .to_owned(),
        extended_description: None,
        input_schema: InputSchema {
            properties: IndexMap::from([
                (
                    "project".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "Project slug to scope cleanup".to_owned(),
                        enum_values: None,
                        default: None,
                    },
                ),
                (
                    "max_age_hours".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::Integer,
                        description: "Remove worktrees older than this many hours (default: 48)"
                            .to_owned(),
                        enum_values: None,
                        default: Some(serde_json::json!(48)),
                    },
                ),
                (
                    "dry_run".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::Boolean,
                        description: "List what would be cleaned without removing (default: false)"
                            .to_owned(),
                        enum_values: None,
                        default: Some(serde_json::json!(false)),
                    },
                ),
            ]),
            required: vec!["project".to_owned()],
        },
        category: ToolCategory::System,
        reversibility: Reversibility::Irreversible,
        auto_activate: false,
    }
}

// -- registration ----------------------------------------------------------

/// Register bookkeeper tools with the given registry.
///
/// # Errors
///
/// Returns an error if any tool name collides with an already-registered tool.
pub(crate) fn register(registry: &mut ToolRegistry) -> Result<()> {
    registry.register(
        tamias_def(),
        Box::new(BookkeeperStub {
            tool_name: "tamias",
        }),
    )?;
    registry.register(
        katharos_def(),
        Box::new(BookkeeperStub {
            tool_name: "katharos",
        }),
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::ToolRegistry;

    #[test]
    fn bookkeeper_tools_register_without_collision() {
        let mut registry = ToolRegistry::new();
        register(&mut registry).expect("bookkeeper tools registered without collision");
        let defs = registry.definitions();
        assert_eq!(defs.len(), 2, "expected 2 bookkeeper tools registered");
    }

    #[test]
    fn tamias_is_system_category() {
        assert_eq!(tamias_def().category, ToolCategory::System);
    }

    #[test]
    fn katharos_is_system_category() {
        assert_eq!(katharos_def().category, ToolCategory::System);
    }

    #[test]
    fn katharos_is_irreversible() {
        assert_eq!(katharos_def().reversibility, Reversibility::Irreversible);
    }

    #[test]
    fn tamias_is_partially_reversible() {
        assert_eq!(
            tamias_def().reversibility,
            Reversibility::PartiallyReversible
        );
    }

    #[test]
    fn no_tools_auto_activate() {
        assert!(!tamias_def().auto_activate);
        assert!(!katharos_def().auto_activate);
    }

    #[tokio::test]
    async fn stubs_return_not_implemented() {
        use std::collections::HashSet;
        use std::sync::{Arc, RwLock};

        use aletheia_koina::id::{NousId, SessionId};

        let ctx = ToolContext {
            nous_id: NousId::new("test").expect("valid nous id"),
            session_id: SessionId::new(),
            workspace: std::path::PathBuf::from("/tmp"),
            allowed_roots: vec![std::path::PathBuf::from("/tmp")],
            services: None,
            active_tools: Arc::new(RwLock::new(HashSet::new())),
        };

        let stub = BookkeeperStub {
            tool_name: "tamias",
        };
        let input = ToolInput {
            name: ToolName::from_static("tamias"),
            tool_use_id: "toolu_test".to_owned(),
            arguments: serde_json::json!({}),
        };

        let result = stub
            .execute(&input, &ctx)
            .await
            .expect("stub execute returns Ok");
        assert!(result.is_error, "stub must return an error result");
        let text = match &result.content {
            crate::types::ToolResultContent::Text(t) => t.clone(),
            _ => panic!("expected text content"),
        };
        assert!(
            text.contains("not yet implemented"),
            "error message must mention 'not yet implemented', got: {text}"
        );
    }
}
