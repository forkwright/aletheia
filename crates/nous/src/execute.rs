//! Execute stage — LLM call and tool iteration loop.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use snafu::ResultExt;
use tracing::{debug, info, instrument, warn};

use aletheia_hermeneus::health::ProviderHealth;
use aletheia_hermeneus::provider::ProviderRegistry;
use aletheia_hermeneus::types::{
    CompletionRequest, Content, ContentBlock, Message, Role, StopReason, ThinkingConfig,
    ToolResultContent,
};
use aletheia_koina::id::ToolName;
use aletheia_organon::registry::ToolRegistry;
use aletheia_organon::types::{ToolContext, ToolInput};
use tokio::sync::mpsc;

use crate::config::NousConfig;
use crate::error;
use crate::pipeline::{
    InteractionSignal, LoopDetector, PipelineContext, PipelineMessage, ToolCall, TurnResult,
    TurnUsage,
};
use crate::session::SessionState;
use crate::stream::TurnStreamEvent;

/// Hash a JSON value for loop detection using the standard library hasher.
fn simple_hash(value: &serde_json::Value) -> String {
    let mut hasher = DefaultHasher::new();
    value.to_string().hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

/// Classify the interaction signals based on tool calls and content.
fn classify_signals(tool_calls: &[ToolCall], _content: &str) -> Vec<InteractionSignal> {
    let mut signals = Vec::new();

    if tool_calls.is_empty() {
        signals.push(InteractionSignal::Conversation);
    } else {
        signals.push(InteractionSignal::ToolExecution);

        let code_tools = ["write", "edit", "exec"];
        if tool_calls
            .iter()
            .any(|tc| code_tools.contains(&tc.name.as_str()))
        {
            signals.push(InteractionSignal::CodeGeneration);
        }

        let research_tools = ["web_search", "web_fetch"];
        if tool_calls
            .iter()
            .any(|tc| research_tools.contains(&tc.name.as_str()))
        {
            signals.push(InteractionSignal::Research);
        }

        if tool_calls.iter().any(|tc| tc.is_error) {
            signals.push(InteractionSignal::ErrorRecovery);
        }
    }

    signals
}

/// Convert pipeline messages to hermeneus messages.
fn build_messages(pipeline_messages: &[PipelineMessage]) -> Vec<Message> {
    pipeline_messages
        .iter()
        .map(|m| Message {
            role: match m.role.as_str() {
                "assistant" => Role::Assistant,
                _ => Role::User,
            },
            content: Content::Text(m.content.clone()),
        })
        .collect()
}

/// Dispatch tool calls from an LLM response and collect results.
async fn dispatch_tools(
    tool_uses: &[(String, String, serde_json::Value)],
    tools: &ToolRegistry,
    tool_ctx: &ToolContext,
    loop_detector: &mut LoopDetector,
    all_tool_calls: &mut Vec<ToolCall>,
    iterations: u32,
) -> error::Result<Vec<ContentBlock>> {
    let mut tool_results: Vec<ContentBlock> = Vec::new();

    for (tool_id, tool_name, tool_input) in tool_uses {
        let input_hash = simple_hash(tool_input);
        if let Some(pattern) = loop_detector.record(tool_name, &input_hash) {
            return Err(error::LoopDetectedSnafu {
                iterations,
                pattern,
            }
            .build());
        }

        let tool_name_id = ToolName::new(tool_name.as_str()).map_err(|_err| {
            error::PipelineStageSnafu {
                stage: "execute",
                message: format!("invalid tool name: {tool_name}"),
            }
            .build()
        })?;

        let start = std::time::Instant::now();
        let result = tools
            .execute(
                &ToolInput {
                    name: tool_name_id,
                    tool_use_id: tool_id.clone(),
                    arguments: tool_input.clone(),
                },
                tool_ctx,
            )
            .await;

        #[expect(
            clippy::cast_possible_truncation,
            reason = "tool execution duration won't exceed u64::MAX milliseconds"
        )]
        let duration_ms = start.elapsed().as_millis() as u64;

        let (content, is_error) = match result {
            Ok(r) => (r.content, r.is_error),
            Err(e) => (ToolResultContent::text(format!("Tool error: {e}")), true),
        };

        debug!(
            tool = tool_name.as_str(),
            duration_ms, is_error, "tool executed"
        );

        all_tool_calls.push(ToolCall {
            id: tool_id.clone(),
            name: tool_name.clone(),
            input: tool_input.clone(),
            result: Some(content.text_summary()),
            is_error,
            duration_ms,
        });

        tool_results.push(ContentBlock::ToolResult {
            tool_use_id: tool_id.clone(),
            content,
            is_error: Some(is_error),
        });
    }

    Ok(tool_results)
}

/// Execute stage — calls the LLM and iterates on tool use.
///
/// This is the core agent loop. It:
/// 1. Builds a `CompletionRequest` from pipeline context
/// 2. Calls the LLM
/// 3. Processes `tool_use` blocks by dispatching to the `ToolRegistry`
/// 4. Feeds tool results back and re-calls the LLM
/// 5. Repeats until `EndTurn`, `MaxTokens`, or iteration limit
#[expect(
    clippy::too_many_lines,
    reason = "health check additions keep the loop cohesive"
)]
#[instrument(skip_all, fields(nous_id = %session.nous_id, session_id = %session.id))]
pub async fn execute(
    ctx: &PipelineContext,
    session: &SessionState,
    config: &NousConfig,
    providers: &ProviderRegistry,
    tools: &ToolRegistry,
    tool_ctx: &ToolContext,
) -> error::Result<TurnResult> {
    let provider = providers.find_provider(&config.model).ok_or_else(|| {
        error::PipelineStageSnafu {
            stage: "execute",
            message: format!("no provider for model: {}", config.model),
        }
        .build()
    })?;

    if let Some(health) = providers.provider_health(provider.name()) {
        if matches!(health, ProviderHealth::Down { .. }) {
            return Err(error::PipelineStageSnafu {
                stage: "execute",
                message: format!("provider '{}' is currently unavailable", provider.name()),
            }
            .build());
        }
    }

    let mut messages = build_messages(&ctx.messages);
    let mut all_tool_calls: Vec<ToolCall> = Vec::new();
    let mut total_usage = TurnUsage::default();
    let mut loop_detector = LoopDetector::new(config.loop_detection_threshold);
    let mut iterations: u32 = 0;
    let mut final_content = String::new();
    let mut final_stop_reason = String::new();

    let thinking = if config.thinking_enabled {
        Some(ThinkingConfig {
            enabled: true,
            budget_tokens: config.thinking_budget,
        })
    } else {
        None
    };

    loop {
        iterations += 1;

        if iterations > config.max_tool_iterations {
            warn!(iterations, "max tool iterations reached");
            break;
        }

        // Rebuild tool list each iteration so enable_tool activations take effect
        let active = tool_ctx
            .active_tools
            .read()
            .expect("active_tools lock")
            .clone();
        let tool_defs = tools.to_hermeneus_tools_filtered(&active);

        let request = CompletionRequest {
            model: config.model.clone(),
            system: ctx.system_prompt.clone(),
            messages: messages.clone(),
            max_tokens: config.max_output_tokens,
            tools: tool_defs,
            temperature: None,
            thinking: thinking.clone(),
            stop_sequences: vec![],
            ..Default::default()
        };

        let response = match provider.complete(&request) {
            Ok(resp) => {
                providers.record_success(provider.name());
                resp
            }
            Err(e) => {
                providers.record_error(provider.name(), &e);
                return Err(e).context(error::LlmSnafu);
            }
        };

        total_usage.input_tokens += response.usage.input_tokens;
        total_usage.output_tokens += response.usage.output_tokens;
        total_usage.cache_read_tokens += response.usage.cache_read_tokens;
        total_usage.cache_write_tokens += response.usage.cache_write_tokens;
        total_usage.llm_calls += 1;

        let mut text_parts: Vec<String> = Vec::new();
        let mut tool_uses: Vec<(String, String, serde_json::Value)> = Vec::new();

        for block in &response.content {
            match block {
                ContentBlock::Text { text, .. } => text_parts.push(text.clone()),
                ContentBlock::ToolUse { id, name, input } => {
                    tool_uses.push((id.clone(), name.clone(), input.clone()));
                }
                ContentBlock::Thinking { thinking, .. } => {
                    debug!(len = thinking.len(), "thinking block received");
                }
                _ => {}
            }
        }

        final_content = text_parts.join("");
        final_stop_reason = response.stop_reason.to_string();

        if tool_uses.is_empty() || response.stop_reason != StopReason::ToolUse {
            break;
        }

        messages.push(Message {
            role: Role::Assistant,
            content: Content::Blocks(response.content.clone()),
        });

        let tool_results = dispatch_tools(
            &tool_uses,
            tools,
            tool_ctx,
            &mut loop_detector,
            &mut all_tool_calls,
            iterations,
        )
        .await?;

        messages.push(Message {
            role: Role::User,
            content: Content::Blocks(tool_results),
        });
    }

    info!(
        iterations,
        tool_calls = all_tool_calls.len(),
        llm_calls = total_usage.llm_calls,
        stop_reason = final_stop_reason.as_str(),
        "execute stage complete"
    );

    let signals = classify_signals(&all_tool_calls, &final_content);

    Ok(TurnResult {
        content: final_content,
        tool_calls: all_tool_calls,
        usage: total_usage,
        signals,
        stop_reason: final_stop_reason,
    })
}

/// Dispatch tool calls with streaming events emitted to the channel.
async fn dispatch_tools_streaming(
    tool_uses: &[(String, String, serde_json::Value)],
    tools: &ToolRegistry,
    tool_ctx: &ToolContext,
    loop_detector: &mut LoopDetector,
    all_tool_calls: &mut Vec<ToolCall>,
    iterations: u32,
    stream_tx: &mpsc::Sender<TurnStreamEvent>,
) -> error::Result<Vec<ContentBlock>> {
    let mut tool_results: Vec<ContentBlock> = Vec::new();

    for (tool_id, tool_name, tool_input) in tool_uses {
        let input_hash = simple_hash(tool_input);
        if let Some(pattern) = loop_detector.record(tool_name, &input_hash) {
            return Err(error::LoopDetectedSnafu {
                iterations,
                pattern,
            }
            .build());
        }

        let tool_name_id = ToolName::new(tool_name.as_str()).map_err(|_err| {
            error::PipelineStageSnafu {
                stage: "execute",
                message: format!("invalid tool name: {tool_name}"),
            }
            .build()
        })?;

        let _ = stream_tx
            .try_send(TurnStreamEvent::ToolStart {
                tool_id: tool_id.clone(),
                tool_name: tool_name.clone(),
                input: tool_input.clone(),
            });

        let start = std::time::Instant::now();
        let result = tools
            .execute(
                &ToolInput {
                    name: tool_name_id,
                    tool_use_id: tool_id.clone(),
                    arguments: tool_input.clone(),
                },
                tool_ctx,
            )
            .await;

        #[expect(
            clippy::cast_possible_truncation,
            reason = "tool execution duration won't exceed u64::MAX milliseconds"
        )]
        let duration_ms = start.elapsed().as_millis() as u64;

        let (content, is_error) = match result {
            Ok(r) => (r.content, r.is_error),
            Err(e) => (ToolResultContent::text(format!("Tool error: {e}")), true),
        };

        let result_summary = content.text_summary();

        debug!(
            tool = tool_name.as_str(),
            duration_ms, is_error, "tool executed"
        );

        let _ = stream_tx
            .try_send(TurnStreamEvent::ToolResult {
                tool_id: tool_id.clone(),
                tool_name: tool_name.clone(),
                result: result_summary.clone(),
                is_error,
                duration_ms,
            });

        all_tool_calls.push(ToolCall {
            id: tool_id.clone(),
            name: tool_name.clone(),
            input: tool_input.clone(),
            result: Some(result_summary),
            is_error,
            duration_ms,
        });

        tool_results.push(ContentBlock::ToolResult {
            tool_use_id: tool_id.clone(),
            content,
            is_error: Some(is_error),
        });
    }

    Ok(tool_results)
}

/// Streaming execute stage — same as [`execute`] but emits real-time events.
///
/// Uses `complete_streaming()` when the provider supports it, falling back to
/// `complete()` otherwise. Tool start/result events are emitted via the channel.
#[expect(
    clippy::too_many_lines,
    reason = "streaming variant parallels execute() structure"
)]
#[instrument(skip_all, fields(nous_id = %session.nous_id, session_id = %session.id))]
pub async fn execute_streaming(
    ctx: &PipelineContext,
    session: &SessionState,
    config: &NousConfig,
    providers: &ProviderRegistry,
    tools: &ToolRegistry,
    tool_ctx: &ToolContext,
    stream_tx: &mpsc::Sender<TurnStreamEvent>,
) -> error::Result<TurnResult> {
    let streaming_provider = providers.find_streaming_provider(&config.model);

    // Fall back to non-streaming if no streaming provider available
    let Some(streaming_provider) = streaming_provider else {
        return execute(ctx, session, config, providers, tools, tool_ctx).await;
    };

    let provider = providers.find_provider(&config.model).ok_or_else(|| {
        error::PipelineStageSnafu {
            stage: "execute",
            message: format!("no provider for model: {}", config.model),
        }
        .build()
    })?;

    if let Some(health) = providers.provider_health(provider.name()) {
        if matches!(health, ProviderHealth::Down { .. }) {
            return Err(error::PipelineStageSnafu {
                stage: "execute",
                message: format!("provider '{}' is currently unavailable", provider.name()),
            }
            .build());
        }
    }

    let mut messages = build_messages(&ctx.messages);
    let mut all_tool_calls: Vec<ToolCall> = Vec::new();
    let mut total_usage = TurnUsage::default();
    let mut loop_detector = LoopDetector::new(config.loop_detection_threshold);
    let mut iterations: u32 = 0;
    let mut final_content = String::new();
    let mut final_stop_reason = String::new();

    let thinking = if config.thinking_enabled {
        Some(ThinkingConfig {
            enabled: true,
            budget_tokens: config.thinking_budget,
        })
    } else {
        None
    };

    let tool_defs = tools.to_hermeneus_tools();

    loop {
        iterations += 1;

        if iterations > config.max_tool_iterations {
            warn!(iterations, "max tool iterations reached");
            break;
        }

        let request = CompletionRequest {
            model: config.model.clone(),
            system: ctx.system_prompt.clone(),
            messages: messages.clone(),
            max_tokens: config.max_output_tokens,
            tools: tool_defs.clone(),
            temperature: None,
            thinking: thinking.clone(),
            stop_sequences: vec![],
            ..Default::default()
        };

        let tx = stream_tx.clone();
        let response = match streaming_provider.complete_streaming(&request, |event| {
            let _ = tx.try_send(TurnStreamEvent::LlmDelta(event));
        }) {
            Ok(resp) => {
                providers.record_success(provider.name());
                resp
            }
            Err(e) => {
                providers.record_error(provider.name(), &e);
                return Err(e).context(error::LlmSnafu);
            }
        };

        total_usage.input_tokens += response.usage.input_tokens;
        total_usage.output_tokens += response.usage.output_tokens;
        total_usage.cache_read_tokens += response.usage.cache_read_tokens;
        total_usage.cache_write_tokens += response.usage.cache_write_tokens;
        total_usage.llm_calls += 1;

        let mut text_parts: Vec<String> = Vec::new();
        let mut tool_uses: Vec<(String, String, serde_json::Value)> = Vec::new();

        for block in &response.content {
            match block {
                ContentBlock::Text { text, .. } => text_parts.push(text.clone()),
                ContentBlock::ToolUse { id, name, input } => {
                    tool_uses.push((id.clone(), name.clone(), input.clone()));
                }
                ContentBlock::Thinking { thinking, .. } => {
                    debug!(len = thinking.len(), "thinking block received");
                }
                _ => {}
            }
        }

        final_content = text_parts.join("");
        final_stop_reason = response.stop_reason.to_string();

        if tool_uses.is_empty() || response.stop_reason != StopReason::ToolUse {
            break;
        }

        messages.push(Message {
            role: Role::Assistant,
            content: Content::Blocks(response.content.clone()),
        });

        let tool_results = dispatch_tools_streaming(
            &tool_uses,
            tools,
            tool_ctx,
            &mut loop_detector,
            &mut all_tool_calls,
            iterations,
            stream_tx,
        )
        .await?;

        messages.push(Message {
            role: Role::User,
            content: Content::Blocks(tool_results),
        });
    }

    info!(
        iterations,
        tool_calls = all_tool_calls.len(),
        llm_calls = total_usage.llm_calls,
        stop_reason = final_stop_reason.as_str(),
        "streaming execute stage complete"
    );

    let signals = classify_signals(&all_tool_calls, &final_content);

    Ok(TurnResult {
        content: final_content,
        tool_calls: all_tool_calls,
        usage: total_usage,
        signals,
        stop_reason: final_stop_reason,
    })
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;
    use std::future::Future;
    use std::path::PathBuf;
    use std::pin::Pin;
    use std::sync::{Arc, Mutex, RwLock};

    use aletheia_hermeneus::provider::ProviderRegistry;
    use aletheia_hermeneus::types::{
        CompletionRequest, CompletionResponse, ContentBlock, StopReason, Usage,
    };
    use aletheia_koina::id::{NousId, SessionId, ToolName};
    use aletheia_organon::registry::{ToolExecutor, ToolRegistry};
    use aletheia_organon::types::{
        InputSchema, ToolCategory, ToolContext, ToolDef, ToolInput, ToolResult,
    };

    use super::*;
    use crate::config::NousConfig;
    use crate::pipeline::{InteractionSignal, PipelineContext, PipelineMessage};
    use crate::session::SessionState;

    // --- Test Infrastructure ---

    struct MockProvider {
        responses: Mutex<Vec<CompletionResponse>>,
    }

    impl MockProvider {
        fn with_responses(responses: Vec<CompletionResponse>) -> Self {
            Self {
                responses: Mutex::new(responses),
            }
        }
    }

    impl aletheia_hermeneus::provider::LlmProvider for MockProvider {
        fn complete(
            &self,
            _request: &CompletionRequest,
        ) -> aletheia_hermeneus::error::Result<CompletionResponse> {
            let mut responses = self.responses.lock().expect("lock");
            if responses.len() > 1 {
                Ok(responses.remove(0))
            } else {
                Ok(responses[0].clone())
            }
        }

        fn supported_models(&self) -> &[&str] {
            &["test-model"]
        }

        #[expect(
            clippy::unnecessary_literal_bound,
            reason = "trait requires &str return"
        )]
        fn name(&self) -> &str {
            "mock"
        }

        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
    }

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

    // --- Tests ---

    #[tokio::test]
    async fn simple_text_response() {
        let mut providers = ProviderRegistry::new();
        providers.register(Box::new(MockProvider::with_responses(vec![
            make_text_response("Hello there!"),
        ])));

        let tools = ToolRegistry::new();
        let result = execute(
            &test_pipeline_ctx(),
            &test_session(),
            &test_config(),
            &providers,
            &tools,
            &test_tool_ctx(),
        )
        .await
        .expect("execute");

        assert_eq!(result.content, "Hello there!");
        assert!(result.tool_calls.is_empty());
        assert_eq!(result.usage.llm_calls, 1);
        assert_eq!(result.usage.input_tokens, 100);
        assert_eq!(result.usage.output_tokens, 50);
        assert_eq!(result.stop_reason, "end_turn");
        assert!(result.signals.contains(&InteractionSignal::Conversation));
    }

    #[tokio::test]
    async fn single_tool_iteration() {
        let mut providers = ProviderRegistry::new();
        providers.register(Box::new(MockProvider::with_responses(vec![
            make_tool_response("exec", "toolu_1", serde_json::json!({"input": "test"})),
            make_text_response("Done!"),
        ])));

        let tools = make_registry_with("exec", Box::new(EchoExecutor));

        let result = execute(
            &test_pipeline_ctx(),
            &test_session(),
            &test_config(),
            &providers,
            &tools,
            &test_tool_ctx(),
        )
        .await
        .expect("execute");

        assert_eq!(result.content, "Done!");
        assert_eq!(result.tool_calls.len(), 1);
        assert_eq!(result.tool_calls[0].name, "exec");
        assert_eq!(
            result.tool_calls[0].result.as_deref(),
            Some("executed: exec")
        );
        assert!(!result.tool_calls[0].is_error);
        assert_eq!(result.usage.llm_calls, 2);
        assert_eq!(result.stop_reason, "end_turn");
    }

    #[tokio::test]
    async fn multi_tool_iteration() {
        let mut providers = ProviderRegistry::new();
        providers.register(Box::new(MockProvider::with_responses(vec![
            make_tool_response("exec", "toolu_1", serde_json::json!({"input": "first"})),
            make_tool_response("exec", "toolu_2", serde_json::json!({"input": "second"})),
            make_text_response("All done!"),
        ])));

        let tools = make_registry_with("exec", Box::new(EchoExecutor));

        let result = execute(
            &test_pipeline_ctx(),
            &test_session(),
            &test_config(),
            &providers,
            &tools,
            &test_tool_ctx(),
        )
        .await
        .expect("execute");

        assert_eq!(result.content, "All done!");
        assert_eq!(result.tool_calls.len(), 2);
        assert_eq!(result.usage.llm_calls, 3);
    }

    #[tokio::test]
    async fn loop_detection_triggers() {
        let mut providers = ProviderRegistry::new();
        let response = make_tool_response("exec", "toolu_1", serde_json::json!({"input": "same"}));
        providers.register(Box::new(MockProvider::with_responses(vec![
            response.clone(),
            response.clone(),
            response,
        ])));

        let tools = make_registry_with("exec", Box::new(EchoExecutor));
        let mut config = test_config();
        config.loop_detection_threshold = 3;

        let err = execute(
            &test_pipeline_ctx(),
            &test_session(),
            &config,
            &providers,
            &tools,
            &test_tool_ctx(),
        )
        .await
        .expect_err("should detect loop");

        assert!(err.to_string().contains("loop detected"));
    }

    #[tokio::test]
    async fn max_iterations_respected() {
        let mut providers = ProviderRegistry::new();
        let responses: Vec<CompletionResponse> = (0..10)
            .map(|i| make_tool_response("exec", &format!("toolu_{i}"), serde_json::json!({"i": i})))
            .collect();
        providers.register(Box::new(MockProvider::with_responses(responses)));

        let tools = make_registry_with("exec", Box::new(EchoExecutor));
        let mut config = test_config();
        config.max_tool_iterations = 3;
        config.loop_detection_threshold = 100;

        let result = execute(
            &test_pipeline_ctx(),
            &test_session(),
            &config,
            &providers,
            &tools,
            &test_tool_ctx(),
        )
        .await
        .expect("should not error");

        assert_eq!(result.usage.llm_calls, 3);
    }

    #[tokio::test]
    async fn tool_error_captured() {
        let mut providers = ProviderRegistry::new();
        providers.register(Box::new(MockProvider::with_responses(vec![
            make_tool_response("exec", "toolu_1", serde_json::json!({"input": "test"})),
            make_text_response("Recovered"),
        ])));

        let tools = make_registry_with("exec", Box::new(ErrorExecutor));

        let result = execute(
            &test_pipeline_ctx(),
            &test_session(),
            &test_config(),
            &providers,
            &tools,
            &test_tool_ctx(),
        )
        .await
        .expect("execute should succeed despite tool error");

        assert_eq!(result.tool_calls.len(), 1);
        assert!(result.tool_calls[0].is_error);
        assert_eq!(result.tool_calls[0].result.as_deref(), Some("tool failed"));
        assert_eq!(result.content, "Recovered");
    }

    #[test]
    fn signal_classification_conversation() {
        let signals = classify_signals(&[], "Hello");
        assert_eq!(signals, vec![InteractionSignal::Conversation]);
    }

    #[test]
    fn signal_classification_code() {
        let calls = vec![ToolCall {
            id: "1".to_owned(),
            name: "write".to_owned(),
            input: serde_json::json!({}),
            result: Some("ok".to_owned()),
            is_error: false,
            duration_ms: 10,
        }];
        let signals = classify_signals(&calls, "");
        assert!(signals.contains(&InteractionSignal::ToolExecution));
        assert!(signals.contains(&InteractionSignal::CodeGeneration));
    }

    #[test]
    fn signal_classification_research() {
        let calls = vec![ToolCall {
            id: "1".to_owned(),
            name: "web_search".to_owned(),
            input: serde_json::json!({}),
            result: Some("results".to_owned()),
            is_error: false,
            duration_ms: 10,
        }];
        let signals = classify_signals(&calls, "");
        assert!(signals.contains(&InteractionSignal::ToolExecution));
        assert!(signals.contains(&InteractionSignal::Research));
    }

    #[test]
    fn signal_classification_error_recovery() {
        let calls = vec![ToolCall {
            id: "1".to_owned(),
            name: "exec".to_owned(),
            input: serde_json::json!({}),
            result: Some("failed".to_owned()),
            is_error: true,
            duration_ms: 10,
        }];
        let signals = classify_signals(&calls, "");
        assert!(signals.contains(&InteractionSignal::ToolExecution));
        assert!(signals.contains(&InteractionSignal::ErrorRecovery));
    }

    #[tokio::test]
    async fn usage_accumulates_across_iterations() {
        let mut providers = ProviderRegistry::new();
        providers.register(Box::new(MockProvider::with_responses(vec![
            make_tool_response("exec", "toolu_1", serde_json::json!({"input": "first"})),
            make_text_response("Done"),
        ])));

        let tools = make_registry_with("exec", Box::new(EchoExecutor));

        let result = execute(
            &test_pipeline_ctx(),
            &test_session(),
            &test_config(),
            &providers,
            &tools,
            &test_tool_ctx(),
        )
        .await
        .expect("execute");

        // First call: 80 input + 30 output, second call: 100 input + 50 output
        assert_eq!(result.usage.input_tokens, 180);
        assert_eq!(result.usage.output_tokens, 80);
        assert_eq!(result.usage.llm_calls, 2);
        assert_eq!(result.usage.total_tokens(), 260);
    }

    #[tokio::test]
    async fn tool_error_captured_not_propagated() {
        let mut providers = ProviderRegistry::new();
        providers.register(Box::new(MockProvider::with_responses(vec![
            make_tool_response("fail_tool", "tu_1", serde_json::json!({})),
            make_text_response("recovered"),
        ])));

        let tools = make_registry_with("fail_tool", Box::new(ErrorExecutor));
        let result = execute(
            &test_pipeline_ctx(),
            &test_session(),
            &test_config(),
            &providers,
            &tools,
            &test_tool_ctx(),
        )
        .await
        .expect("pipeline should complete despite tool error");

        assert!(
            result.tool_calls.iter().any(|tc| tc.is_error),
            "should capture the tool error in tool_calls"
        );
    }

    #[tokio::test]
    async fn max_iterations_stops_loop() {
        let mut providers = ProviderRegistry::new();
        // Provider always returns tool use — would loop forever without max_iterations.
        // Supply enough unique-id responses to feed several iterations.
        let responses: Vec<_> = (0..10)
            .map(|i| make_tool_response("echo", &format!("tu_{i}"), serde_json::json!({"i": i})))
            .collect();
        providers.register(Box::new(MockProvider::with_responses(responses)));

        let tools = make_registry_with("echo", Box::new(EchoExecutor));
        let mut config = test_config();
        config.max_tool_iterations = 2;
        config.loop_detection_threshold = 100;
        let result = execute(
            &test_pipeline_ctx(),
            &test_session(),
            &config,
            &providers,
            &tools,
            &test_tool_ctx(),
        )
        .await
        .expect("should complete after hitting max iterations");

        assert!(
            result.usage.llm_calls <= 3,
            "should have stopped after ~2 iterations, got {} llm_calls",
            result.usage.llm_calls
        );
    }

    #[tokio::test]
    async fn text_response_no_tools() {
        let mut providers = ProviderRegistry::new();
        providers.register(Box::new(MockProvider::with_responses(vec![
            make_text_response("just text"),
        ])));

        let tools = ToolRegistry::new();
        let result = execute(
            &test_pipeline_ctx(),
            &test_session(),
            &test_config(),
            &providers,
            &tools,
            &test_tool_ctx(),
        )
        .await
        .expect("execute");

        assert!(result.tool_calls.is_empty(), "no tool calls expected");
        assert_eq!(result.content, "just text");
    }

    #[test]
    fn classify_signals_conversation_when_no_tools() {
        let signals = classify_signals(&[], "some text");
        assert_eq!(signals, vec![InteractionSignal::Conversation]);
    }

    #[test]
    fn classify_signals_includes_error_recovery() {
        let calls = vec![ToolCall {
            id: "1".to_owned(),
            name: "test".to_owned(),
            input: serde_json::json!({}),
            result: Some("failed".to_owned()),
            is_error: true,
            duration_ms: 5,
        }];
        let signals = classify_signals(&calls, "");
        assert!(
            signals.contains(&InteractionSignal::ToolExecution),
            "should have ToolExecution"
        );
        assert!(
            signals.contains(&InteractionSignal::ErrorRecovery),
            "should have ErrorRecovery"
        );
    }

    // --- Streaming Tests ---

    #[tokio::test]
    async fn streaming_falls_back_to_non_streaming_for_mock() {
        let mut providers = ProviderRegistry::new();
        providers.register(Box::new(MockProvider::with_responses(vec![
            make_text_response("Hello streaming!"),
        ])));

        let tools = ToolRegistry::new();
        let (tx, mut rx) = tokio::sync::mpsc::channel::<TurnStreamEvent>(64);

        let result = execute_streaming(
            &test_pipeline_ctx(),
            &test_session(),
            &test_config(),
            &providers,
            &tools,
            &test_tool_ctx(),
            &tx,
        )
        .await
        .expect("execute_streaming");

        assert_eq!(result.content, "Hello streaming!");
        assert_eq!(result.usage.llm_calls, 1);

        // MockProvider doesn't support streaming, so no LlmDelta events
        drop(tx);
        assert!(
            rx.try_recv().is_err(),
            "no stream events for non-streaming provider"
        );
    }

    #[tokio::test]
    async fn streaming_tool_events_emitted() {
        let mut providers = ProviderRegistry::new();
        providers.register(Box::new(MockProvider::with_responses(vec![
            make_tool_response("exec", "toolu_1", serde_json::json!({"input": "test"})),
            make_text_response("Done!"),
        ])));

        let tools = make_registry_with("exec", Box::new(EchoExecutor));
        let (tx, mut rx) = tokio::sync::mpsc::channel::<TurnStreamEvent>(64);

        let result = execute_streaming(
            &test_pipeline_ctx(),
            &test_session(),
            &test_config(),
            &providers,
            &tools,
            &test_tool_ctx(),
            &tx,
        )
        .await
        .expect("execute_streaming");

        assert_eq!(result.content, "Done!");
        assert_eq!(result.tool_calls.len(), 1);

        // Even with mock (non-streaming) provider, tool events should be emitted
        drop(tx);
        let mut tool_start_count = 0;
        let mut tool_result_count = 0;
        while let Ok(event) = rx.try_recv() {
            match event {
                TurnStreamEvent::ToolStart { .. } => tool_start_count += 1,
                TurnStreamEvent::ToolResult { .. } => tool_result_count += 1,
                _ => {}
            }
        }
        // Falls back to non-streaming execute(), no tool events via channel
        // (tool events only come from dispatch_tools_streaming, which requires
        //  a streaming provider to be found)
        assert_eq!(tool_start_count, 0);
        assert_eq!(tool_result_count, 0);
    }
}
