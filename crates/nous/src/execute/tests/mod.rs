#![expect(clippy::expect_used, reason = "test assertions")]
use std::collections::HashSet;
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, RwLock};

use hermeneus::health::{HealthConfig, ProviderHealth};
use hermeneus::provider::ProviderRegistry;
use hermeneus::test_utils::MockProvider;
use hermeneus::types::{CompletionResponse, ContentBlock, StopReason, Usage};
use koina::id::{NousId, SessionId, ToolName};
use organon::registry::{ToolExecutor, ToolRegistry};
use organon::types::{InputSchema, ToolCategory, ToolContext, ToolDef, ToolInput, ToolResult};

use super::*;
use crate::config::NousConfig;
use crate::execute::dispatch::simple_hash;
use crate::pipeline::{InteractionSignal, PipelineContext, PipelineMessage};
use crate::session::SessionState;

struct EchoExecutor;

impl ToolExecutor for EchoExecutor {
    fn execute<'a>(
        &'a self,
        input: &'a ToolInput,
        _ctx: &'a ToolContext,
    ) -> Pin<Box<dyn Future<Output = organon::error::Result<ToolResult>> + Send + 'a>> {
        Box::pin(async {
            Ok(ToolResult::text(format!(
                "executed: {}",
                input.name.as_str()
            )))
        })
    }
}

struct ErrorExecutor;

impl ToolExecutor for ErrorExecutor {
    fn execute<'a>(
        &'a self,
        _input: &'a ToolInput,
        _ctx: &'a ToolContext,
    ) -> Pin<Box<dyn Future<Output = organon::error::Result<ToolResult>> + Send + 'a>> {
        Box::pin(async { Ok(ToolResult::error("tool failed")) })
    }
}

struct CountingExecutor {
    executions: Arc<AtomicUsize>,
}

impl CountingExecutor {
    fn new(executions: Arc<AtomicUsize>) -> Self {
        Self { executions }
    }
}

impl ToolExecutor for CountingExecutor {
    fn execute<'a>(
        &'a self,
        input: &'a ToolInput,
        _ctx: &'a ToolContext,
    ) -> Pin<Box<dyn Future<Output = organon::error::Result<ToolResult>> + Send + 'a>> {
        Box::pin(async {
            self.executions.fetch_add(1, Ordering::SeqCst);
            Ok(ToolResult::text(format!(
                "executed: {}",
                input.name.as_str()
            )))
        })
    }
}

fn test_config() -> NousConfig {
    NousConfig {
        id: Arc::from("test-agent"),
        generation: crate::config::NousGenerationConfig {
            model: "test-model".to_owned(),
            ..crate::config::NousGenerationConfig::default()
        },
        tool_groups: organon::types::ToolGroupPolicy::AllowAll {
            reason: "execute test helper".to_owned(),
        },
        ..NousConfig::default()
    }
}

fn test_tool_ctx() -> ToolContext {
    ToolContext {
        nous_id: NousId::new("test-agent").expect("valid"),
        session_id: SessionId::new(),
        turn_number: 0,
        workspace: PathBuf::from("/tmp/test"),
        allowed_roots: vec![PathBuf::from("/tmp")],
        services: None,
        active_tools: Arc::new(RwLock::new(HashSet::new())),
        tool_config: Arc::new(taxis::config::ToolLimitsConfig::default()),
    }
}

fn test_pipeline_ctx() -> PipelineContext {
    PipelineContext {
        system_prompt: Some("You are a test agent.".to_owned()),
        messages: vec![PipelineMessage::text("user", "Hello", 1)],
        ..PipelineContext::default()
    }
}

fn test_session() -> SessionState {
    let config = test_config();
    SessionState::new("test-session".to_owned(), "main".to_owned(), &config)
}

fn make_text_response(text: &str) -> CompletionResponse {
    CompletionResponse {
        id: "resp-1".to_owned(),
        model: "test-model".to_owned(),
        stop_reason: StopReason::EndTurn,
        content: vec![ContentBlock::Text {
            text: text.to_owned(),
            citations: None,
        }],
        usage: Usage {
            input_tokens: 100,
            output_tokens: 50,
            ..Usage::default()
        },
        cost_usd: None,
        duration_ms: None,
    }
}

fn make_text_response_for_model(text: &str, model: &str) -> CompletionResponse {
    let mut response = make_text_response(text);
    response.model = model.to_owned();
    response
}

fn make_tool_response(
    tool_name: &str,
    tool_id: &str,
    input: serde_json::Value,
) -> CompletionResponse {
    CompletionResponse {
        id: "resp-tool".to_owned(),
        model: "test-model".to_owned(),
        stop_reason: StopReason::ToolUse,
        content: vec![ContentBlock::ToolUse {
            id: tool_id.to_owned(),
            name: tool_name.to_owned(),
            input,
        }],
        usage: Usage {
            input_tokens: 80,
            output_tokens: 30,
            ..Usage::default()
        },
        cost_usd: None,
        duration_ms: None,
    }
}

fn make_tool_def(name: &str) -> ToolDef {
    ToolDef {
        name: ToolName::new(name).expect("valid"),
        description: format!("Test tool: {name}"),
        extended_description: None,
        input_schema: InputSchema {
            properties: indexmap::IndexMap::default(),
            required: vec![],
        },
        category: ToolCategory::Workspace,
        reversibility: organon::types::Reversibility::FullyReversible,
        auto_activate: true,
        groups: vec![organon::types::ToolGroupId::Read],
        tags: vec![],
    }
}

fn make_registry_with(name: &str, executor: Box<dyn ToolExecutor>) -> ToolRegistry {
    let mut registry = ToolRegistry::new();
    registry
        .register(make_tool_def(name), executor)
        .expect("register");
    registry
}

mod approval;
mod core;
mod deferred_schemas;
mod edge_cases;
mod streaming;
