#![expect(
    clippy::indexing_slicing,
    reason = "test: vec indices are valid after asserting len"
)]
//! Core execute loop tests.
use super::*;

#[tokio::test]
async fn simple_text_response() {
    let mut providers = ProviderRegistry::new();
    providers.register(Box::new(
        MockProvider::with_responses(vec![make_text_response("Hello there!")])
            .models(&["test-model"]),
    ));

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

    assert_eq!(
        result.content, "Hello there!",
        "response content should match mock text"
    );
    assert!(
        result.tool_calls.is_empty(),
        "text-only response should produce no tool calls"
    );
    assert_eq!(
        result.usage.llm_calls, 1,
        "single text response should use exactly one LLM call"
    );
    assert_eq!(
        result.usage.input_tokens, 100,
        "input token count should match mock response"
    );
    assert_eq!(
        result.usage.output_tokens, 50,
        "output token count should match mock response"
    );
    assert_eq!(
        result.stop_reason, "end_turn",
        "response should stop with end_turn reason"
    );
    assert!(
        result.signals.contains(&InteractionSignal::Conversation),
        "text-only response should produce Conversation signal"
    );
}

#[tokio::test]
async fn single_tool_iteration() {
    let mut providers = ProviderRegistry::new();
    providers.register(Box::new(
        MockProvider::with_responses(vec![
            make_tool_response("exec", "toolu_1", serde_json::json!({"input": "test"})),
            make_text_response("Done!"),
        ])
        .models(&["test-model"]),
    ));

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

    assert_eq!(
        result.content, "Done!",
        "final response content should match mock text"
    );
    assert_eq!(
        result.tool_calls.len(),
        1,
        "should have recorded exactly one tool call"
    );
    assert_eq!(
        result.tool_calls[0].name, "exec",
        "tool call name should match registered tool"
    );
    assert_eq!(
        result.tool_calls[0].result.as_deref(),
        Some("executed: exec"),
        "tool result should contain the echo executor output"
    );
    assert!(
        !result.tool_calls[0].is_error,
        "tool call should not be marked as an error"
    );
    assert_eq!(
        result.usage.llm_calls, 2,
        "one tool iteration requires two LLM calls"
    );
    assert_eq!(
        result.stop_reason, "end_turn",
        "final response should stop with end_turn reason"
    );
}

#[tokio::test]
async fn multi_tool_iteration() {
    let mut providers = ProviderRegistry::new();
    providers.register(Box::new(
        MockProvider::with_responses(vec![
            make_tool_response("exec", "toolu_1", serde_json::json!({"input": "first"})),
            make_tool_response("exec", "toolu_2", serde_json::json!({"input": "second"})),
            make_text_response("All done!"),
        ])
        .models(&["test-model"]),
    ));

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

    assert_eq!(
        result.content, "All done!",
        "final response content should match mock text"
    );
    assert_eq!(
        result.tool_calls.len(),
        2,
        "should have recorded two tool calls across iterations"
    );
    assert_eq!(
        result.usage.llm_calls, 3,
        "two tool iterations require three LLM calls"
    );
}

#[tokio::test]
async fn loop_detection_triggers() {
    let mut providers = ProviderRegistry::new();
    let response = make_tool_response("exec", "toolu_1", serde_json::json!({"input": "same"}));
    providers.register(Box::new(
        MockProvider::with_responses(vec![response.clone(), response.clone(), response])
            .models(&["test-model"]),
    ));

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

    assert!(
        err.to_string().contains("loop detected"),
        "error message should indicate loop was detected"
    );
}

#[tokio::test]
async fn max_iterations_respected() {
    let mut providers = ProviderRegistry::new();
    let responses: Vec<CompletionResponse> = (0..10)
        .map(|i| make_tool_response("exec", &format!("toolu_{i}"), serde_json::json!({"i": i})))
        .collect();
    providers.register(Box::new(
        MockProvider::with_responses(responses).models(&["test-model"]),
    ));

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

    assert_eq!(
        result.usage.llm_calls, 3,
        "should stop after max_tool_iterations=3 LLM calls"
    );
}

#[tokio::test]
async fn tool_error_captured() {
    let mut providers = ProviderRegistry::new();
    providers.register(Box::new(
        MockProvider::with_responses(vec![
            make_tool_response("exec", "toolu_1", serde_json::json!({"input": "test"})),
            make_text_response("Recovered"),
        ])
        .models(&["test-model"]),
    ));

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

    assert_eq!(
        result.tool_calls.len(),
        1,
        "should have recorded one tool call even when it errored"
    );
    assert!(
        result.tool_calls[0].is_error,
        "tool call should be marked as an error"
    );
    assert_eq!(
        result.tool_calls[0].result.as_deref(),
        Some("tool failed"),
        "tool error message should be captured in result"
    );
    assert_eq!(
        result.content, "Recovered",
        "final response content should reflect recovery after tool error"
    );
}

#[test]
fn signal_classification_conversation() {
    let signals = classify_signals(&[], "Hello", false, false);
    assert_eq!(
        signals,
        vec![InteractionSignal::Conversation],
        "no tool calls and plain text should produce only Conversation signal"
    );
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
    assert!(
        signals.contains(&InteractionSignal::ToolExecution),
        "write tool call should produce ToolExecution signal"
    );
    assert!(
        signals.contains(&InteractionSignal::CodeGeneration),
        "write tool call should produce CodeGeneration signal"
    );
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
    assert!(
        signals.contains(&InteractionSignal::ToolExecution),
        "web_search tool call should produce ToolExecution signal"
    );
    assert!(
        signals.contains(&InteractionSignal::Research),
        "web_search tool call should produce Research signal"
    );
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
    assert!(
        signals.contains(&InteractionSignal::ToolExecution),
        "error tool call should produce ToolExecution signal"
    );
    assert!(
        signals.contains(&InteractionSignal::ErrorRecovery),
        "failed tool call should produce ErrorRecovery signal"
    );
}

#[tokio::test]
async fn usage_accumulates_across_iterations() {
    let mut providers = ProviderRegistry::new();
    providers.register(Box::new(
        MockProvider::with_responses(vec![
            make_tool_response("exec", "toolu_1", serde_json::json!({"input": "first"})),
            make_text_response("Done"),
        ])
        .models(&["test-model"]),
    ));

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

    assert_eq!(
        result.usage.input_tokens, 180,
        "input tokens should be summed across both LLM calls (80 + 100)"
    );
    assert_eq!(
        result.usage.output_tokens, 80,
        "output tokens should be summed across both LLM calls (30 + 50)"
    );
    assert_eq!(
        result.usage.llm_calls, 2,
        "one tool iteration should produce exactly two LLM calls"
    );
    assert_eq!(
        result.usage.total_tokens(),
        260,
        "total tokens should equal sum of all input and output tokens (180 + 80)"
    );
}

#[tokio::test]
async fn tool_error_captured_not_propagated() {
    let mut providers = ProviderRegistry::new();
    providers.register(Box::new(
        MockProvider::with_responses(vec![
            make_tool_response("fail_tool", "tu_1", serde_json::json!({})),
            make_text_response("recovered"),
        ])
        .models(&["test-model"]),
    ));

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
    // WHY: Provider always returns tool use: would loop forever without max_iterations.
    // Supply enough unique-id responses to feed several iterations.
    let responses: Vec<_> = (0..10)
        .map(|i| make_tool_response("echo", &format!("tu_{i}"), serde_json::json!({"i": i})))
        .collect();
    providers.register(Box::new(
        MockProvider::with_responses(responses).models(&["test-model"]),
    ));

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
    providers.register(Box::new(
        MockProvider::with_responses(vec![make_text_response("just text")]).models(&["test-model"]),
    ));

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
    assert_eq!(
        result.content, "just text",
        "response content should match mock text"
    );
}

#[test]
fn classify_signals_conversation_when_no_tools() {
    let signals = classify_signals(&[], "some text", false, false);
    assert_eq!(
        signals,
        vec![InteractionSignal::Conversation],
        "no tool calls and plain text should produce only Conversation signal"
    );
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
    assert!(
        signals.contains(&InteractionSignal::ToolExecution),
        "both server tools should produce ToolExecution signal"
    );
    assert!(
        signals.contains(&InteractionSignal::Research),
        "server web search should produce Research signal"
    );
    assert!(
        signals.contains(&InteractionSignal::CodeGeneration),
        "server code execution should produce CodeGeneration signal"
    );
    assert!(
        !signals.contains(&InteractionSignal::Conversation),
        "should not produce Conversation signal when server tools were used"
    );
}
