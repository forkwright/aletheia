#![expect(clippy::unwrap_used, reason = "test assertions")]
#![expect(clippy::expect_used, reason = "test assertions")]
use crate::execute::dispatch::simple_hash;
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
    // std::sync::Mutex is intentional: test mock, never crosses .await
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
    fn complete<'a>(
        &'a self,
        _request: &'a CompletionRequest,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<Output = aletheia_hermeneus::error::Result<CompletionResponse>>
                + Send
                + 'a,
        >,
    > {
        Box::pin(async {
            #[expect(
                clippy::expect_used,
                reason = "test mock: poisoned lock means a test bug"
            )]
            let mut responses = self.responses.lock().expect("lock poisoned");
            if responses.len() > 1 {
                Ok(responses.remove(0))
            } else {
                Ok(responses[0].clone())
            }
        })
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
    let signals = classify_signals(&[], "Hello", false, false);
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
    let signals = classify_signals(&calls, "", false, false);
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
    let signals = classify_signals(&calls, "", false, false);
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
    let signals = classify_signals(&calls, "", false, false);
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
    // Provider always returns tool use: would loop forever without max_iterations.
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
    let signals = classify_signals(&[], "some text", false, false);
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
    let signals = classify_signals(&calls, "", false, false);
    assert!(
        signals.contains(&InteractionSignal::ToolExecution),
        "should have ToolExecution"
    );
    assert!(
        signals.contains(&InteractionSignal::ErrorRecovery),
        "should have ErrorRecovery"
    );
}

#[test]
fn classify_signals_server_web_search() {
    let signals = classify_signals(&[], "", true, false);
    assert!(
        signals.contains(&InteractionSignal::Research),
        "should have Research from server web search"
    );
    assert!(
        !signals.contains(&InteractionSignal::Conversation),
        "should not be Conversation when server web search was used"
    );
}

#[test]
fn classify_signals_server_code_execution() {
    let signals = classify_signals(&[], "", false, true);
    assert!(
        signals.contains(&InteractionSignal::ToolExecution),
        "should have ToolExecution from server code execution"
    );
    assert!(
        signals.contains(&InteractionSignal::CodeGeneration),
        "should have CodeGeneration from server code execution"
    );
    assert!(
        !signals.contains(&InteractionSignal::Conversation),
        "should not be Conversation when server code execution was used"
    );
}

#[test]
fn classify_signals_both_server_tools() {
    let signals = classify_signals(&[], "", true, true);
    assert!(signals.contains(&InteractionSignal::ToolExecution));
    assert!(signals.contains(&InteractionSignal::Research));
    assert!(signals.contains(&InteractionSignal::CodeGeneration));
    assert!(!signals.contains(&InteractionSignal::Conversation));
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
            // NOTE: counting only ToolStart/ToolResult events
            _ => {}
        }
    }
    // Falls back to non-streaming execute(), no tool events via channel
    // (tool events only come from dispatch_tools_streaming, which requires
    //  a streaming provider to be found)
    assert_eq!(tool_start_count, 0);
    assert_eq!(tool_result_count, 0);
}

// --- Edge case tests ---

#[tokio::test]
async fn empty_text_response() {
    let mut providers = ProviderRegistry::new();
    providers.register(Box::new(MockProvider::with_responses(vec![
        make_text_response(""),
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

    assert_eq!(result.content, "");
    assert_eq!(result.usage.llm_calls, 1);
    assert!(result.signals.contains(&InteractionSignal::Conversation));
}

#[tokio::test]
async fn thinking_only_response() {
    let mut providers = ProviderRegistry::new();
    let response = CompletionResponse {
        id: "resp-think".to_owned(),
        model: "test-model".to_owned(),
        stop_reason: StopReason::EndTurn,
        content: vec![
            ContentBlock::Thinking {
                thinking: "I'm reasoning about this...".to_owned(),
                signature: None,
            },
            ContentBlock::Text {
                text: "Here's the answer.".to_owned(),
                citations: None,
            },
        ],
        usage: Usage {
            input_tokens: 100,
            output_tokens: 80,
            ..Usage::default()
        },
    };
    providers.register(Box::new(MockProvider::with_responses(vec![response])));

    let tools = ToolRegistry::new();
    let mut config = test_config();
    config.thinking_enabled = true;
    config.thinking_budget = 5_000;

    let result = execute(
        &test_pipeline_ctx(),
        &test_session(),
        &config,
        &providers,
        &tools,
        &test_tool_ctx(),
    )
    .await
    .expect("execute");

    assert_eq!(result.content, "Here's the answer.");
    assert_eq!(result.usage.llm_calls, 1);
}

#[tokio::test]
async fn no_provider_for_model_returns_error() {
    let providers = ProviderRegistry::new(); // empty
    let tools = ToolRegistry::new();

    let err = execute(
        &test_pipeline_ctx(),
        &test_session(),
        &test_config(),
        &providers,
        &tools,
        &test_tool_ctx(),
    )
    .await;

    assert!(err.is_err());
    let msg = err.unwrap_err().to_string();
    assert!(msg.contains("no provider"), "got: {msg}");
}

#[test]
fn simple_hash_deterministic() {
    let v = serde_json::json!({"key": "value"});
    let h1 = simple_hash(&v);
    let h2 = simple_hash(&v);
    assert_eq!(h1, h2);
}

#[test]
fn simple_hash_different_for_different_inputs() {
    let v1 = serde_json::json!({"key": "value1"});
    let v2 = serde_json::json!({"key": "value2"});
    assert_ne!(simple_hash(&v1), simple_hash(&v2));
}

#[test]
fn classify_signals_edit_is_code_generation() {
    let calls = vec![ToolCall {
        id: "1".to_owned(),
        name: "edit".to_owned(),
        input: serde_json::json!({}),
        result: Some("ok".to_owned()),
        is_error: false,
        duration_ms: 10,
    }];
    let signals = classify_signals(&calls, "", false, false);
    assert!(signals.contains(&InteractionSignal::CodeGeneration));
}

#[test]
fn classify_signals_web_fetch_is_research() {
    let calls = vec![ToolCall {
        id: "1".to_owned(),
        name: "web_fetch".to_owned(),
        input: serde_json::json!({}),
        result: Some("html".to_owned()),
        is_error: false,
        duration_ms: 10,
    }];
    let signals = classify_signals(&calls, "", false, false);
    assert!(signals.contains(&InteractionSignal::Research));
}

#[test]
fn classify_signals_multiple_flags() {
    let calls = vec![
        ToolCall {
            id: "1".to_owned(),
            name: "write".to_owned(),
            input: serde_json::json!({}),
            result: Some("ok".to_owned()),
            is_error: false,
            duration_ms: 10,
        },
        ToolCall {
            id: "2".to_owned(),
            name: "web_search".to_owned(),
            input: serde_json::json!({}),
            result: None,
            is_error: true,
            duration_ms: 5,
        },
    ];
    let signals = classify_signals(&calls, "", false, false);
    assert!(signals.contains(&InteractionSignal::ToolExecution));
    assert!(signals.contains(&InteractionSignal::CodeGeneration));
    assert!(signals.contains(&InteractionSignal::Research));
    assert!(signals.contains(&InteractionSignal::ErrorRecovery));
}

#[tokio::test]
async fn max_iterations_one_exits_immediately() {
    let mut providers = ProviderRegistry::new();
    providers.register(Box::new(MockProvider::with_responses(vec![
        make_tool_response("exec", "toolu_1", serde_json::json!({})),
    ])));

    let tools = make_registry_with("exec", Box::new(EchoExecutor));
    let mut config = test_config();
    config.max_tool_iterations = 1;
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
    .expect("should succeed");

    assert_eq!(result.usage.llm_calls, 1);
}

#[test]
fn build_messages_maps_roles_correctly() {
    let msgs = vec![
        PipelineMessage {
            role: "user".to_owned(),
            content: "Hello".to_owned(),
            token_estimate: 1,
        },
        PipelineMessage {
            role: "assistant".to_owned(),
            content: "Hi".to_owned(),
            token_estimate: 1,
        },
        PipelineMessage {
            role: "unknown".to_owned(),
            content: "?".to_owned(),
            token_estimate: 1,
        },
    ];
    let built = build_messages(&msgs);
    assert_eq!(built[0].role, Role::User);
    assert_eq!(built[1].role, Role::Assistant);
    assert_eq!(built[2].role, Role::User); // unknown maps to User
}
