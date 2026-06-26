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
#[non_exhaustive]
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
                        ..Default::default(),
                    },
                ),
                (
                    "scope".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "Scope of the checkpoint. 'session' only today.".to_owned(),
                        enum_values: Some(vec!["session".to_owned()]),
                        default: Some(serde_json::Value::String("session".to_owned())),
                        ..Default::default(),
                    },
                ),
            ]),
            required: vec!["content".to_owned()],
        },
        category: ToolCategory::Memory,
        reversibility: Reversibility::PartiallyReversible,
        auto_activate: true,
        groups: vec![ToolGroupId::Edit],
        tags: vec![ToolTag::Edit],
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

#[cfg(test)]
mod tests {
    use std::collections::HashSet;
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex, RwLock};

    use hermeneus::secret::SecretVault;
    use koina::id::{NousId, SessionId};

    use super::*;
    use crate::types::{
        ApprovalRequirement, ServerToolConfig, ToolGroupPolicy, ToolHttpClients, ToolServices,
        WorkingCheckpoint, WorkingCheckpointStore,
    };

    type RecordedWrite = (String, u64, String);

    #[derive(Default)]
    struct RecordingCheckpointStore {
        writes: Mutex<Vec<RecordedWrite>>,
    }

    impl RecordingCheckpointStore {
        fn writes(&self) -> Vec<RecordedWrite> {
            self.writes
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .clone()
        }
    }

    impl WorkingCheckpointStore for RecordingCheckpointStore {
        fn write_checkpoint(
            &self,
            session_id: &str,
            turn_number: u64,
            content: &str,
        ) -> std::result::Result<(), crate::error::StoreError> {
            self.writes
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push((session_id.to_owned(), turn_number, content.to_owned()));
            Ok(())
        }

        fn read_latest(
            &self,
            _session_id: &str,
        ) -> std::result::Result<Option<WorkingCheckpoint>, crate::error::StoreError> {
            Ok(None)
        }

        fn read_recent(
            &self,
            _session_id: &str,
            _limit: usize,
        ) -> std::result::Result<Vec<WorkingCheckpoint>, crate::error::StoreError> {
            Ok(Vec::new())
        }
    }

    fn register_checkpoint_tool() -> ToolRegistry {
        let mut registry = ToolRegistry::new();
        if let Err(err) = register(&mut registry) {
            panic!("register update_working_checkpoint failed: {err}");
        }
        registry
    }

    fn checkpoint_input(content: &str) -> ToolInput {
        ToolInput {
            name: ToolName::from_static("update_working_checkpoint"),
            tool_use_id: "toolu_checkpoint".to_owned(),
            arguments: serde_json::json!({ "content": content }),
        }
    }

    fn test_context(
        store: Arc<RecordingCheckpointStore>,
        session_id: SessionId,
        turn_number: u64,
    ) -> ToolContext {
        crate::testing::install_crypto_provider();
        let nous_id = match NousId::new("test-agent") {
            Ok(id) => id,
            Err(err) => panic!("static test nous id invalid: {err}"),
        };
        let working_checkpoint_store: Arc<dyn WorkingCheckpointStore> = store;
        ToolContext {
            nous_id,
            session_id,
            turn_number,
            workspace: PathBuf::from("/tmp/test"),
            allowed_roots: vec![PathBuf::from("/tmp")],
            services: Some(Arc::new(ToolServices {
                cross_nous: None,
                messenger: None,
                note_store: None,
                blackboard_store: None,
                spawn: None,
                planning: None,
                knowledge: None,
                working_checkpoint_store: Some(working_checkpoint_store),
                http_clients: ToolHttpClients::for_tests(),
                secret_vault: SecretVault::new(),
                lazy_tool_catalog: Vec::new(),
                server_tool_config: ServerToolConfig::default(),
            })),
            active_tools: Arc::new(RwLock::new(HashSet::default())),
            tool_config: Arc::new(taxis::config::ToolLimitsConfig::default()),
        }
    }

    #[test]
    fn checkpoint_tool_is_edit_memory_not_read_recon() {
        let registry = register_checkpoint_tool();
        let name = ToolName::from_static("update_working_checkpoint");
        let Some(def) = registry.get_def(&name) else {
            panic!("update_working_checkpoint should be registered");
        };

        assert_eq!(def.category, ToolCategory::Memory);
        assert_eq!(def.reversibility, Reversibility::PartiallyReversible);
        assert_eq!(def.groups, vec![ToolGroupId::Edit]);
        assert_eq!(def.tags, vec![ToolTag::Edit]);

        let read_policy = ToolGroupPolicy::groups(vec![ToolGroupId::Read]);
        let edit_policy = ToolGroupPolicy::groups(vec![ToolGroupId::Edit]);

        assert!(
            !registry
                .definitions_for_policy(&read_policy)
                .iter()
                .any(|tool| tool.name == name),
            "checkpoint writes must not appear in a read-only tool surface"
        );
        assert!(
            registry
                .definitions_for_policy(&edit_policy)
                .iter()
                .any(|tool| tool.name == name),
            "checkpoint writes should appear for edit-capable roles"
        );
        assert!(
            !registry
                .definitions_for_tags(&[ToolTag::Recon])
                .iter()
                .any(|tool| tool.name == name),
            "checkpoint writes must not be discoverable as recon"
        );

        let input = checkpoint_input("keep this");
        let approval = match registry.approval_requirement_for_input(&input) {
            Ok(approval) => approval,
            Err(err) => panic!("checkpoint call should classify: {err}"),
        };
        assert_eq!(approval, ApprovalRequirement::Required);

        let metadata = match registry.call_metadata_for_input(&input, false) {
            Ok(metadata) => metadata,
            Err(err) => panic!("checkpoint call metadata should classify: {err}"),
        };
        assert_eq!(metadata.reversibility, Reversibility::PartiallyReversible);
        assert_eq!(metadata.approval, ApprovalRequirement::Required);
    }

    #[tokio::test]
    async fn read_policy_denies_checkpoint_write_and_edit_policy_persists() {
        let registry = register_checkpoint_tool();
        let store = Arc::new(RecordingCheckpointStore::default());
        let session_id = SessionId::new();
        let ctx = test_context(Arc::clone(&store), session_id.clone(), 42);
        let input = checkpoint_input("decision: persist this");

        let read_policy = ToolGroupPolicy::groups(vec![ToolGroupId::Read]);
        let read_result = registry
            .execute_checked(&input, &ctx, "reader", &read_policy)
            .await;
        assert!(
            read_result.is_err(),
            "read-only policy should reject checkpoint writes"
        );
        assert!(
            store.writes().is_empty(),
            "read-only denial should happen before executor persistence"
        );

        let edit_policy = ToolGroupPolicy::groups(vec![ToolGroupId::Edit]);
        let write_result = match registry
            .execute_checked(&input, &ctx, "editor", &edit_policy)
            .await
        {
            Ok(result) => result,
            Err(err) => panic!("edit policy should permit checkpoint writes: {err}"),
        };
        assert!(!write_result.is_error);
        assert_eq!(
            store.writes(),
            vec![(
                session_id.to_string(),
                42,
                "decision: persist this".to_owned()
            )]
        );
    }
}
