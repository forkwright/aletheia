//! Agent-curated working-memory checkpoint tool.
//!
//! Agents call `update_working_checkpoint` to persist structured key-info
//! that the turn-start hook reinjects into the next user message as a
//! `<key_info>` block.

use std::future::Future;
use std::pin::Pin;

use indexmap::IndexMap;

use koina::id::ToolName;

use crate::error::Result;
use crate::registry::{ToolExecutor, ToolRegistry};
use crate::types::{
    InputSchema, PropertyDef, PropertyType, Reversibility, ToolCategory, ToolContext, ToolDef,
    ToolGroupId, ToolInput, ToolResult, ToolTag,
};

/// Scope of a working checkpoint.
#[derive(Debug, Clone, Default, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkingCheckpointScope {
    /// Session-scoped checkpoint (default).
    #[default]
    Session,
}

/// Input schema for `update_working_checkpoint`.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct UpdateWorkingCheckpointInput {
    /// Structured `key_info` content the agent has decided is worth retaining.
    pub content: String,
    /// Scope of the checkpoint. Currently "session" only; "project" follow-up.
    #[serde(default)]
    pub scope: WorkingCheckpointScope,
}

// ── Executor ─────────────────────────────────────────────────────────────────

struct UpdateWorkingCheckpointExecutor;

impl ToolExecutor for UpdateWorkingCheckpointExecutor {
    #[tracing::instrument(skip(self, input, ctx))]
    fn execute<'a>(
        &'a self,
        input: &'a ToolInput,
        ctx: &'a ToolContext,
    ) -> Pin<Box<dyn Future<Output = Result<ToolResult>> + Send + 'a>> {
        Box::pin(async move {
            let args: UpdateWorkingCheckpointInput =
                match serde_json::from_value(input.arguments.clone()) {
                    Ok(a) => a,
                    Err(e) => {
                        return Ok(ToolResult::error(format!(
                            "invalid arguments for update_working_checkpoint: {e}"
                        )));
                    }
                };

            let Some(ref services) = ctx.services else {
                return Ok(ToolResult::error("tool services unavailable"));
            };
            let Some(ref store) = services.working_checkpoint_store else {
                return Ok(ToolResult::error("working checkpoint store unavailable"));
            };

            let _ = args.scope; // acknowledged; only Session is supported today

            let session_id = ctx.session_id.to_string();
            match store.write_checkpoint(&session_id, ctx.turn_number, &args.content) {
                Ok(()) => Ok(ToolResult::text("working checkpoint updated")),
                Err(e) => Ok(ToolResult::error(format!(
                    "failed to persist working checkpoint: {e}"
                ))),
            }
        })
    }
}

// ── ToolDef ──────────────────────────────────────────────────────────────────

fn working_checkpoint_def() -> ToolDef {
    ToolDef {
        name: ToolName::from_static("update_working_checkpoint"),
        description: "Persist structured key-info that the agent wants to retain \
             across turns. This content is reinjected into the next user message \
             as a <key_info> block, surviving context compaction."
            .to_owned(),
        extended_description: Some(
            "Use this when you have distilled important facts, decisions, or context \
             that should not be lost when the conversation is compacted. \
             Keep content concise and structured."
                .to_owned(),
        ),
        input_schema: InputSchema {
            properties: IndexMap::from([
                (
                    "content".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description:
                            "Structured key_info content the agent has decided is worth retaining."
                                .to_owned(),
                        enum_values: None,
                        default: None,
                    },
                ),
                (
                    "scope".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "Scope of the checkpoint. 'session' only today.".to_owned(),
                        enum_values: Some(vec!["session".to_owned()]),
                        default: Some(serde_json::Value::String("session".to_owned())),
                    },
                ),
            ]),
            required: vec!["content".to_owned()],
        },
        category: ToolCategory::Memory,
        reversibility: Reversibility::FullyReversible,
        auto_activate: true,
        groups: vec![ToolGroupId::Read],
        tags: vec![ToolTag::Recon],
    }
}

// ── Registration ─────────────────────────────────────────────────────────────

/// Register the `update_working_checkpoint` tool into `registry`.
///
/// # Errors
///
/// Returns an error if the tool name collides with an already-registered tool.
pub fn register(registry: &mut ToolRegistry) -> Result<()> {
    registry.register(
        working_checkpoint_def(),
        Box::new(UpdateWorkingCheckpointExecutor),
    )
}
