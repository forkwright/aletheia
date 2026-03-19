#![expect(clippy::unwrap_used, reason = "test assertions")]
#![expect(clippy::expect_used, reason = "test assertions")]
use std::collections::HashSet;
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::{Arc, RwLock};

use aletheia_hermeneus::provider::ProviderRegistry;
use aletheia_hermeneus::test_utils::MockProvider;
use aletheia_hermeneus::types::{CompletionResponse, ContentBlock, StopReason, Usage};
use aletheia_koina::id::{NousId, SessionId, ToolName};
use aletheia_organon::registry::{ToolExecutor, ToolRegistry};
use aletheia_organon::types::{
    InputSchema, ToolCategory, ToolContext, ToolDef, ToolInput, ToolResult,
};

use super::*;
use crate::config::NousConfig;
use crate::execute::dispatch::simple_hash;
use crate::pipeline::{InteractionSignal, PipelineContext, PipelineMessage};
use crate::session::SessionState;

// --- Test Infrastructure ---

struct EchoExecutor;

impl ToolExecutor for EchoExecutor {
    fn execute<'a>(
        &'a self,
        input: &'a ToolInput,
        _ctx: &'a ToolContext,
    ) -> Pin<Box<dyn Future<Output = aletheia_organon::error::Result<ToolResult>> + Send + 'a>>
    {
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
    ) -> Pin<Box<dyn Future<Output = aletheia_organon::error::Result<ToolResult>> + Send + 'a>>
    {
        Box::pin(async { Ok(ToolResult::error("tool failed")) })
    }
}

fn test_config() -> NousConfig {
    NousConfig {
        id: "test-agent".to_owned(),
        model: "test-model".to_owned(),
        ..NousConfig::default()
    }
}

fn test_tool_ctx() -> ToolContext {
    ToolContext {
        nous_id: NousId::new("test-agent").expect("valid"),
        session_id: SessionId::new(),
        workspace: PathBuf::from("/tmp/test"),
        allowed_roots: vec![PathBuf::from("/tmp")],
        services: None,
        active_tools: Arc::new(RwLock::new(HashSet::new())),
    }
}

fn test_pipeline_ctx() -> PipelineContext {
    PipelineContext {
        system_prompt: Some("You are a test agent.".to_owned()),
        messages: vec![PipelineMessage {
            role: "user".to_owned(),
            content: "Hello".to_owned(),
            token_estimate: 1,
        }],
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
    }
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
        auto_activate: false,
    }
}

fn make_registry_with(name: &str, executor: Box<dyn ToolExecutor>) -> ToolRegistry {
    let mut registry = ToolRegistry::new();
    registry
        .register(make_tool_def(name), executor)
        .expect("register");
    registry
}

mod core;
mod edge_cases;
mod streaming;
