//! Agent coordination tool executors: sessions_spawn, sessions_dispatch.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use indexmap::IndexMap;

use koina::id::ToolName;

use super::workspace::{extract_opt_u64, extract_str};
use crate::error::Result;
use crate::registry::{ToolExecutor, ToolRegistry};
use crate::types::{
    InputSchema, PropertyDef, PropertyType, Reversibility, SpawnContext, SpawnRequest, SpawnResult,
    ToolCategory, ToolContext, ToolDef, ToolGroupId, ToolInput, ToolResult, ToolTag,
};

/// Fallback default; runtime reads `ctx.tool_config.agent_dispatch_timeout_secs`.
pub const DEFAULT_TIMEOUT_SECS: u64 = 300;
/// Fallback default; runtime reads `ctx.tool_config.max_dispatch_tasks`.
pub const MAX_DISPATCH_TASKS: usize = 10;

struct SessionsSpawnExecutor;

impl ToolExecutor for SessionsSpawnExecutor {
    fn execute<'a>(
        &'a self,
        input: &'a ToolInput,
        ctx: &'a ToolContext,
    ) -> Pin<Box<dyn Future<Output = Result<ToolResult>> + Send + 'a>> {
        Box::pin(async {
            let Some(services) = ctx.services.as_ref() else {
                return Ok(ToolResult::error("spawn service not available"));
            };
            let Some(spawn_svc) = services.spawn.as_ref() else {
                return Ok(ToolResult::error("spawn service not configured"));
            };

            let role = extract_str(&input.arguments, "role", &input.name)?;
            let task = extract_str(&input.arguments, "task", &input.name)?;
            let model = input
                .arguments
                .get("model")
                .and_then(|v| v.as_str())
                .map(String::from);
            let timeout = extract_opt_u64(&input.arguments, "timeoutSeconds")
                .unwrap_or(ctx.tool_config.agent_dispatch_timeout_secs);

            let request = SpawnRequest {
                role: role.to_owned(),
                task: task.to_owned(),
                model,
                allowed_tools: None,
                timeout_secs: timeout,
            };

            let spawn_context = SpawnContext::new(ctx.nous_id.as_str(), ctx.turn_cancel());
            match spawn_svc.spawn_and_run(request, spawn_context).await {
                Ok(result) => {
                    let json = serde_json::json!({
                        "content": result.content,
                        "is_error": result.is_error,
                        "input_tokens": result.input_tokens,
                        "output_tokens": result.output_tokens,
                    });
                    Ok(ToolResult::text(json.to_string()))
                }
                Err(e) => Ok(ToolResult::error(format!("Spawn failed: {e}"))),
            }
        })
    }
}

struct SessionsDispatchExecutor;

impl ToolExecutor for SessionsDispatchExecutor {
    fn execute<'a>(
        &'a self,
        input: &'a ToolInput,
        ctx: &'a ToolContext,
    ) -> Pin<Box<dyn Future<Output = Result<ToolResult>> + Send + 'a>> {
        Box::pin(async {
            let Some(services) = ctx.services.as_ref() else {
                return Ok(ToolResult::error("spawn service not available"));
            };
            let Some(spawn_svc) = services.spawn.as_ref() else {
                return Ok(ToolResult::error("spawn service not configured"));
            };

            let tasks = input
                .arguments
                .get("tasks")
                .and_then(|v| v.as_array())
                .ok_or_else(|| {
                    crate::error::InvalidInputSnafu {
                        name: input.name.clone(),
                        reason: "missing or invalid 'tasks' array".to_owned(),
                    }
                    .build()
                })?;

            let max_tasks = ctx.tool_config.max_dispatch_tasks;
            if tasks.len() > max_tasks {
                return Ok(ToolResult::error(format!(
                    "Too many tasks: {} (max {max_tasks})",
                    tasks.len()
                )));
            }

            let default_timeout = extract_opt_u64(&input.arguments, "timeoutSeconds")
                .unwrap_or(ctx.tool_config.agent_dispatch_timeout_secs);
            let nous_id = ctx.nous_id.as_str().to_owned();
            let parent_cancel = ctx.turn_cancel();

            let mut join_set = tokio::task::JoinSet::new();

            for task_val in tasks {
                let role = task_val
                    .get("role")
                    .and_then(|v| v.as_str())
                    .unwrap_or("coder")
                    .to_owned();
                let task_text = task_val
                    .get("task")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_owned();
                let model = task_val
                    .get("model")
                    .and_then(|v| v.as_str())
                    .map(String::from);
                let timeout = task_val
                    .get("timeoutSeconds")
                    .and_then(serde_json::Value::as_u64)
                    .unwrap_or(default_timeout);

                let request = SpawnRequest {
                    role,
                    task: task_text,
                    model,
                    allowed_tools: None,
                    timeout_secs: timeout,
                };

                let svc = Arc::clone(spawn_svc);
                let parent = nous_id.clone();
                let cancel = parent_cancel.clone();
                join_set.spawn(async move {
                    svc.spawn_and_run(request, SpawnContext::new(parent, cancel))
                        .await
                });
            }

            Ok(aggregate_dispatch_results(join_set, parent_cancel).await)
        })
    }
}

/// Drain a `JoinSet` of sub-agent spawn futures into a single
/// [`ToolResult`] whose outcome reflects partial-success semantics.
///
/// # Outcome rules (#3633)
///
/// - All sub-agents succeeded -> [`ToolOutcome::Success`].
/// - All sub-agents failed or panicked -> [`ToolOutcome::Failure`].
/// - Mixed -> [`ToolOutcome::PartialSuccess`] with one reason per
///   degraded sub-operation.
///
/// Extracted from the dispatch executor to keep the executor body
/// inside the workspace's `too_many_lines` clippy threshold.
async fn aggregate_dispatch_results(
    mut join_set: tokio::task::JoinSet<std::result::Result<SpawnResult, String>>,
    parent_cancel: tokio_util::sync::CancellationToken,
) -> ToolResult {
    let mut results = Vec::with_capacity(join_set.len());
    let mut failure_reasons: Vec<String> = Vec::new();
    let mut success_count: u32 = 0;
    let mut parent_cancelled = false;
    while !join_set.is_empty() {
        let join_result = if parent_cancelled {
            join_set.join_next().await
        } else {
            tokio::select! {
                result = join_set.join_next() => result,
                () = parent_cancel.cancelled() => {
                    parent_cancelled = true;
                    join_set.abort_all();
                    continue;
                }
            }
        };

        let Some(join_result) = join_result else {
            break;
        };

        match join_result {
            Ok(Ok(spawn_result)) => {
                if spawn_result.is_error {
                    failure_reasons.push(format!(
                        "sub-agent reported error: {}",
                        truncate_reason(&spawn_result.content)
                    ));
                } else {
                    success_count += 1;
                }
                results.push(serde_json::json!({
                    "content": spawn_result.content,
                    "is_error": spawn_result.is_error,
                    "input_tokens": spawn_result.input_tokens,
                    "output_tokens": spawn_result.output_tokens,
                }));
            }
            Ok(Err(e)) => {
                failure_reasons.push(format!("spawn error: {e}"));
                results.push(serde_json::json!({
                    "content": format!("Spawn error: {e}"),
                    "is_error": true,
                }));
            }
            Err(e) if parent_cancelled && e.is_cancelled() => {
                failure_reasons.push("sub-agent cancelled by parent turn".to_owned());
                results.push(serde_json::json!({
                    "content": "Cancelled by parent turn",
                    "is_error": true,
                }));
            }
            Err(e) => {
                failure_reasons.push(format!("task panic: {e}"));
                results.push(serde_json::json!({
                    "content": format!("Task panicked: {e}"),
                    "is_error": true,
                }));
            }
        }
    }

    let payload = serde_json::to_string(&results).unwrap_or_else(|_| "[]".to_owned());

    // WHY (#3633): surface partial-success so downstream observers
    // (pipeline metrics, the LLM, audit trails) can tell "all N
    // sub-agents succeeded" apart from "K of N succeeded, rest
    // failed". Full failure still rides the `is_error` channel.
    if failure_reasons.is_empty() {
        ToolResult::text(payload)
    } else if success_count == 0 {
        ToolResult::error(payload)
    } else {
        ToolResult::partial_success(payload, failure_reasons)
    }
}

/// Truncate a sub-agent reason string to a token-friendly length for
/// inclusion in a `ToolOutcome::PartialSuccess` reason list.
fn truncate_reason(text: &str) -> String {
    const MAX: usize = 160;
    let trimmed = text.trim();
    if trimmed.len() <= MAX {
        return trimmed.to_owned();
    }
    let end = trimmed.floor_char_boundary(MAX);
    format!("{}…", trimmed.get(..end).unwrap_or(trimmed))
}

/// Register agent coordination tools.
pub(crate) fn register(registry: &mut ToolRegistry) -> Result<()> {
    registry.register(sessions_spawn_def(), Box::new(SessionsSpawnExecutor))?;
    registry.register(sessions_dispatch_def(), Box::new(SessionsDispatchExecutor))?;
    Ok(())
}

fn sessions_spawn_def() -> ToolDef {
    ToolDef {
        name: ToolName::from_static("sessions_spawn"), // kanon:ignore RUST/expect
        description: "Spawn an ephemeral sub-agent to execute a single task".to_owned(),
        extended_description: Some(
            "Creates a temporary agent with a role-appropriate model and tool set. \
             The sub-agent runs one turn against the task prompt and returns its response. \
             Use for delegating mechanical work: coding, reviewing, researching, exploring, \
             or running commands."
                .to_owned(),
        ),
        input_schema: InputSchema {
            properties: IndexMap::from([
                (
                    "role".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "Sub-agent role".to_owned(),
                        enum_values: Some(vec![
                            "coder".to_owned(),
                            "reviewer".to_owned(),
                            "researcher".to_owned(),
                            "explorer".to_owned(),
                            "runner".to_owned(),
                        ]),
                        default: None,
                    },
                ),
                (
                    "task".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "Task instruction for the sub-agent".to_owned(),
                        enum_values: None,
                        default: None,
                    },
                ),
                (
                    "model".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "Model override (default: role-based)".to_owned(),
                        enum_values: None,
                        default: None,
                    },
                ),
                (
                    "timeoutSeconds".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::Number,
                        description: "Max execution time in seconds (default: 300)".to_owned(),
                        enum_values: None,
                        default: Some(serde_json::json!(300)),
                    },
                ),
            ]),
            required: vec!["role".to_owned(), "task".to_owned()],
        },
        category: ToolCategory::Agent,
        reversibility: Reversibility::PartiallyReversible,
        auto_activate: false,
        groups: vec![ToolGroupId::SpawnSubtask],
        tags: vec![ToolTag::Spawn],
    }
}

fn sessions_dispatch_def() -> ToolDef {
    ToolDef {
        name: ToolName::from_static("sessions_dispatch"), // kanon:ignore RUST/expect
        description: "Spawn multiple sub-agents in parallel and collect their results".to_owned(),
        extended_description: Some(
            "Dispatches an array of tasks to ephemeral sub-agents running concurrently. \
             Each task specifies a role and instruction. Results are returned as an array \
             in completion order. Maximum 10 concurrent tasks."
                .to_owned(),
        ),
        input_schema: InputSchema {
            properties: IndexMap::from([
                (
                    "tasks".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::Array,
                        description: "Array of task objects: {role, task, model?, timeoutSeconds?}"
                            .to_owned(),
                        enum_values: None,
                        default: None,
                    },
                ),
                (
                    "timeoutSeconds".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::Number,
                        description: "Default timeout for all tasks (default: 300)".to_owned(),
                        enum_values: None,
                        default: Some(serde_json::json!(300)),
                    },
                ),
            ]),
            required: vec!["tasks".to_owned()],
        },
        category: ToolCategory::Agent,
        reversibility: Reversibility::Irreversible,
        auto_activate: false,
        groups: vec![ToolGroupId::SpawnSubtask],
        tags: vec![ToolTag::Spawn],
    }
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
#[expect(
    clippy::indexing_slicing,
    reason = "test: vec/JSON indices valid after asserting len"
)]
mod tests {
    use std::collections::HashSet;
    use std::future::Future;
    use std::path::PathBuf;
    use std::pin::Pin;
    use std::sync::{Arc, RwLock};

    use koina::id::{NousId, SessionId, ToolName};
    use taxis::config::ToolLimitsConfig;

    use crate::registry::ToolRegistry;
    use crate::testing::install_crypto_provider;
    use crate::types::{
        ServerToolConfig, SpawnContext, SpawnRequest, SpawnResult, SpawnService, ToolContext,
        ToolHttpClients, ToolInput, ToolServices,
    };

    fn mock_ctx() -> ToolContext {
        ToolContext {
            nous_id: NousId::new("test-agent").expect("valid"),
            session_id: SessionId::new(),
            turn_number: 0,
            workspace: PathBuf::from("/tmp/test"),
            allowed_roots: vec![PathBuf::from("/tmp")],
            services: None,
            active_tools: Arc::new(RwLock::new(HashSet::new())),
            tool_config: Arc::new(ToolLimitsConfig::default()),
        }
    }

    fn mock_ctx_with_spawn(spawn: Arc<dyn SpawnService>) -> ToolContext {
        install_crypto_provider();
        ToolContext {
            nous_id: NousId::new("test-agent").expect("valid"),
            session_id: SessionId::new(),
            turn_number: 0,
            workspace: PathBuf::from("/tmp/test"),
            allowed_roots: vec![PathBuf::from("/tmp")],
            services: Some(Arc::new(ToolServices {
                working_checkpoint_store: None,
                cross_nous: None,
                messenger: None,
                note_store: None,
                blackboard_store: None,
                spawn: Some(spawn),
                planning: None,
                knowledge: None,
                lazy_tool_catalog: vec![],
                server_tool_config: ServerToolConfig::default(),
                http_clients: ToolHttpClients::for_tests(),
                secret_vault: hermeneus::secret::SecretVault::new(),
            })),
            active_tools: Arc::new(RwLock::new(HashSet::new())),
            tool_config: Arc::new(ToolLimitsConfig::default()),
        }
    }

    #[derive(Default)]
    struct MockSpawnService;

    impl SpawnService for MockSpawnService {
        fn spawn_and_run(
            &self,
            _request: SpawnRequest,
            _context: SpawnContext,
        ) -> Pin<Box<dyn Future<Output = Result<SpawnResult, String>> + Send + '_>> {
            Box::pin(async {
                Ok(SpawnResult {
                    content: "mock result".to_owned(),
                    is_error: false,
                    input_tokens: 100,
                    output_tokens: 50,
                })
            })
        }
    }

    #[tokio::test]
    async fn dispatch_aggregation_aborts_pending_children_on_parent_cancel() {
        let parent_cancel = tokio_util::sync::CancellationToken::new();
        let mut join_set = tokio::task::JoinSet::new();
        join_set.spawn(async {
            std::future::pending::<std::result::Result<SpawnResult, String>>().await
        });

        parent_cancel.cancel();
        let result = super::aggregate_dispatch_results(join_set, parent_cancel).await;

        assert!(
            result.is_error,
            "expected parent cancellation to fail dispatch"
        );
        assert!(
            result
                .content
                .text_summary()
                .contains("Cancelled by parent turn"),
            "expected cancelled child outcome in payload"
        );
        assert!(
            result.outcome.failure_reason().contains("parent turn"),
            "expected parent cancellation reason"
        );
    }

    #[tokio::test]
    async fn register_agent_tools() {
        let mut reg = ToolRegistry::new();
        super::register(&mut reg).expect("register");
        assert_eq!(
            reg.definitions().len(),
            2,
            "expected reg.definitions().len() to equal 2"
        );
    }

    #[tokio::test]
    async fn spawn_def_requires_role_and_task() {
        let mut reg = ToolRegistry::new();
        super::register(&mut reg).expect("register");
        let name = ToolName::from_static("sessions_spawn");
        let def = reg.get_def(&name).expect("found");
        assert_eq!(
            def.input_schema.required,
            vec!["role", "task"],
            "expected def.input_schema.required to equal vec![\"role\", \"task\"]"
        );
        assert_eq!(
            def.category,
            crate::types::ToolCategory::Agent,
            "expected def.category to equal crate::types::ToolCategory::Agent"
        );
    }

    #[tokio::test]
    async fn dispatch_def_requires_tasks() {
        let mut reg = ToolRegistry::new();
        super::register(&mut reg).expect("register");
        let name = ToolName::from_static("sessions_dispatch");
        let def = reg.get_def(&name).expect("found");
        assert_eq!(
            def.input_schema.required,
            vec!["tasks"],
            "expected def.input_schema.required to equal vec![\"tasks\"]"
        );
    }

    #[tokio::test]
    async fn spawn_missing_service_returns_error() {
        let mut reg = ToolRegistry::new();
        super::register(&mut reg).expect("register");
        let input = ToolInput {
            name: ToolName::from_static("sessions_spawn"),
            tool_use_id: "tu_1".to_owned(),
            arguments: serde_json::json!({"role": "coder", "task": "write code"}),
        };
        let result = reg.execute(&input, &mock_ctx()).await.expect("execute");
        assert!(result.is_error, "expected result.is_error to be true");
        assert!(
            result.content.text_summary().contains("not available"),
            "expected result.content.text_summary().contains(\"not available\") to be true"
        );
    }

    #[tokio::test]
    async fn spawn_returns_json_result() {
        let spawn = Arc::new(MockSpawnService);
        let ctx = mock_ctx_with_spawn(spawn);
        let mut reg = ToolRegistry::new();
        super::register(&mut reg).expect("register");

        let input = ToolInput {
            name: ToolName::from_static("sessions_spawn"),
            tool_use_id: "tu_1".to_owned(),
            arguments: serde_json::json!({"role": "coder", "task": "write code"}),
        };
        let result = reg.execute(&input, &ctx).await.expect("execute");
        assert!(!result.is_error, "expected result.is_error to be false");

        let json: serde_json::Value =
            serde_json::from_str(&result.content.text_summary()).expect("json");
        assert_eq!(
            json["content"], "mock result",
            "expected json[\"content\"] to equal \"mock result\""
        );
        assert_eq!(
            json["input_tokens"], 100,
            "expected json[\"input_tokens\"] to equal 100"
        );
        assert_eq!(
            json["output_tokens"], 50,
            "expected json[\"output_tokens\"] to equal 50"
        );
    }

    #[tokio::test]
    async fn dispatch_missing_service_returns_error() {
        let mut reg = ToolRegistry::new();
        super::register(&mut reg).expect("register");
        let input = ToolInput {
            name: ToolName::from_static("sessions_dispatch"),
            tool_use_id: "tu_1".to_owned(),
            arguments: serde_json::json!({"tasks": [{"role": "coder", "task": "write code"}]}),
        };
        let result = reg.execute(&input, &mock_ctx()).await.expect("execute");
        assert!(result.is_error, "expected result.is_error to be true");
    }

    #[tokio::test]
    async fn dispatch_rejects_too_many_tasks() {
        let spawn = Arc::new(MockSpawnService);
        let ctx = mock_ctx_with_spawn(spawn);
        let mut reg = ToolRegistry::new();
        super::register(&mut reg).expect("register");

        let tasks: Vec<serde_json::Value> = (0..11)
            .map(|i| serde_json::json!({"role": "coder", "task": format!("task {i}")}))
            .collect();
        let input = ToolInput {
            name: ToolName::from_static("sessions_dispatch"),
            tool_use_id: "tu_1".to_owned(),
            arguments: serde_json::json!({"tasks": tasks}),
        };
        let result = reg.execute(&input, &ctx).await.expect("execute");
        assert!(result.is_error, "expected result.is_error to be true");
        assert!(
            result.content.text_summary().contains("Too many tasks"),
            "expected result.content.text_summary().contains(\"Too many tasks\") to be true"
        );
    }

    #[tokio::test]
    async fn dispatch_collects_results() {
        let spawn = Arc::new(MockSpawnService);
        let ctx = mock_ctx_with_spawn(spawn);
        let mut reg = ToolRegistry::new();
        super::register(&mut reg).expect("register");

        let input = ToolInput {
            name: ToolName::from_static("sessions_dispatch"),
            tool_use_id: "tu_1".to_owned(),
            arguments: serde_json::json!({
                "tasks": [
                    {"role": "coder", "task": "task 1"},
                    {"role": "reviewer", "task": "task 2"},
                ]
            }),
        };
        let result = reg.execute(&input, &ctx).await.expect("execute");
        assert!(!result.is_error, "expected result.is_error to be false");

        let json: Vec<serde_json::Value> =
            serde_json::from_str(&result.content.text_summary()).expect("json");
        assert_eq!(json.len(), 2, "expected json.len() to equal 2");
    }
}
