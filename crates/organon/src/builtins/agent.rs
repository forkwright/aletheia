//! Agent coordination tool executors: sessions_spawn, sessions_dispatch.
#![expect(
    clippy::expect_used,
    reason = "ToolName::new() with static string literals is infallible — name validation would only fail on invalid chars which these names don't contain"
)]

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use indexmap::IndexMap;

use aletheia_koina::id::ToolName;

use super::workspace::{extract_opt_u64, extract_str};
use crate::error::Result;
use crate::registry::{ToolExecutor, ToolRegistry};
use crate::types::{
    InputSchema, PropertyDef, PropertyType, Reversibility, SpawnRequest, ToolCategory, ToolContext,
    ToolDef, ToolInput, ToolResult,
};

const DEFAULT_TIMEOUT_SECS: u64 = 300;
const MAX_DISPATCH_TASKS: usize = 10;

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
            let timeout =
                extract_opt_u64(&input.arguments, "timeoutSeconds").unwrap_or(DEFAULT_TIMEOUT_SECS);

            let request = SpawnRequest {
                role: role.to_owned(),
                task: task.to_owned(),
                model,
                allowed_tools: None,
                timeout_secs: timeout,
            };

            match spawn_svc.spawn_and_run(request, ctx.nous_id.as_str()).await {
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

            if tasks.len() > MAX_DISPATCH_TASKS {
                return Ok(ToolResult::error(format!(
                    "Too many tasks: {} (max {MAX_DISPATCH_TASKS})",
                    tasks.len()
                )));
            }

            let default_timeout =
                extract_opt_u64(&input.arguments, "timeoutSeconds").unwrap_or(DEFAULT_TIMEOUT_SECS);
            let nous_id = ctx.nous_id.as_str().to_owned();

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
                join_set.spawn(async move { svc.spawn_and_run(request, &parent).await });
            }

            let mut results = Vec::with_capacity(join_set.len());
            while let Some(join_result) = join_set.join_next().await {
                match join_result {
                    Ok(Ok(spawn_result)) => results.push(serde_json::json!({
                        "content": spawn_result.content,
                        "is_error": spawn_result.is_error,
                        "input_tokens": spawn_result.input_tokens,
                        "output_tokens": spawn_result.output_tokens,
                    })),
                    Ok(Err(e)) => results.push(serde_json::json!({
                        "content": format!("Spawn error: {e}"),
                        "is_error": true,
                    })),
                    Err(e) => results.push(serde_json::json!({
                        "content": format!("Task panicked: {e}"),
                        "is_error": true,
                    })),
                }
            }

            Ok(ToolResult::text(
                serde_json::to_string(&results).unwrap_or_else(|_| "[]".to_owned()),
            ))
        })
    }
}

/// Register agent coordination tools.
pub(crate) fn register(registry: &mut ToolRegistry) -> Result<()> {
    registry.register(sessions_spawn_def(), Box::new(SessionsSpawnExecutor))?;
    registry.register(sessions_dispatch_def(), Box::new(SessionsDispatchExecutor))?;
    Ok(())
}

fn sessions_spawn_def() -> ToolDef {
    ToolDef {
        name: ToolName::new("sessions_spawn").expect("valid tool name"), // kanon:ignore RUST/expect
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
    }
}

fn sessions_dispatch_def() -> ToolDef {
    ToolDef {
        name: ToolName::new("sessions_dispatch").expect("valid tool name"), // kanon:ignore RUST/expect
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

    use aletheia_koina::id::{NousId, SessionId, ToolName};

    use crate::registry::ToolRegistry;
    use crate::types::{
        ServerToolConfig, SpawnRequest, SpawnResult, SpawnService, ToolContext, ToolInput,
        ToolServices,
    };

    fn install_crypto_provider() {
        let _ = rustls::crypto::ring::default_provider().install_default();
    }

    fn mock_ctx() -> ToolContext {
        ToolContext {
            nous_id: NousId::new("test-agent").expect("valid"),
            session_id: SessionId::new(),
            workspace: PathBuf::from("/tmp/test"),
            allowed_roots: vec![PathBuf::from("/tmp")],
            services: None,
            active_tools: Arc::new(RwLock::new(HashSet::new())),
        }
    }

    fn mock_ctx_with_spawn(spawn: Arc<dyn SpawnService>) -> ToolContext {
        install_crypto_provider();
        ToolContext {
            nous_id: NousId::new("test-agent").expect("valid"),
            session_id: SessionId::new(),
            workspace: PathBuf::from("/tmp/test"),
            allowed_roots: vec![PathBuf::from("/tmp")],
            services: Some(Arc::new(ToolServices {
                cross_nous: None,
                messenger: None,
                note_store: None,
                blackboard_store: None,
                spawn: Some(spawn),
                planning: None,
                knowledge: None,
                lazy_tool_catalog: vec![],
                server_tool_config: ServerToolConfig::default(),
                http_client: reqwest::Client::new(),
            })),
            active_tools: Arc::new(RwLock::new(HashSet::new())),
        }
    }

    #[derive(Default)]
    struct MockSpawnService;

    impl SpawnService for MockSpawnService {
        fn spawn_and_run(
            &self,
            _request: SpawnRequest,
            _parent_nous_id: &str,
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
        let name = ToolName::new("sessions_spawn").expect("valid");
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
        let name = ToolName::new("sessions_dispatch").expect("valid");
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
            name: ToolName::new("sessions_spawn").expect("valid"),
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
            name: ToolName::new("sessions_spawn").expect("valid"),
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
            name: ToolName::new("sessions_dispatch").expect("valid"),
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
            name: ToolName::new("sessions_dispatch").expect("valid"),
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
            name: ToolName::new("sessions_dispatch").expect("valid"),
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
