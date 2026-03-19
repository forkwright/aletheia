#![expect(
    clippy::indexing_slicing,
    reason = "test: vec indices are valid after asserting len"
)]
//! Edge case and utility tests.
use super::*;

#[tokio::test]
async fn empty_text_response() {
    let mut providers = ProviderRegistry::new();
    providers.register(Box::new(
        MockProvider::with_responses(vec![make_text_response("")]).models(&["test-model"]),
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
        result.content, "",
        "empty text response should produce empty content string"
    );
    assert_eq!(
        result.usage.llm_calls, 1,
        "empty text response should still use exactly one LLM call"
    );
    assert!(
        result.signals.contains(&InteractionSignal::Conversation),
        "empty text response should still produce Conversation signal"
    );
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
    providers.register(Box::new(
        MockProvider::with_responses(vec![response]).models(&["test-model"]),
    ));

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

    assert_eq!(
        result.content, "Here's the answer.",
        "response should contain only the text block, not the thinking block"
    );
    assert_eq!(
        result.usage.llm_calls, 1,
        "thinking response should use exactly one LLM call"
    );
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

    assert!(
        err.is_err(),
        "execute with no matching provider should return an error"
    );
    let msg = err.unwrap_err().to_string();
    assert!(msg.contains("no provider"), "got: {msg}");
}

#[test]
fn simple_hash_deterministic() {
    let v = serde_json::json!({"key": "value"});
    let h1 = simple_hash(&v);
    let h2 = simple_hash(&v);
    assert_eq!(h1, h2, "same input should always produce the same hash");
}

#[test]
fn simple_hash_different_for_different_inputs() {
    let v1 = serde_json::json!({"key": "value1"});
    let v2 = serde_json::json!({"key": "value2"});
    assert_ne!(
        simple_hash(&v1),
        simple_hash(&v2),
        "different inputs should produce different hashes"
    );
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
    assert!(
        signals.contains(&InteractionSignal::CodeGeneration),
        "edit tool call should produce CodeGeneration signal"
    );
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
    assert!(
        signals.contains(&InteractionSignal::Research),
        "web_fetch tool call should produce Research signal"
    );
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
    assert!(
        signals.contains(&InteractionSignal::ToolExecution),
        "mixed tool calls should produce ToolExecution signal"
    );
    assert!(
        signals.contains(&InteractionSignal::CodeGeneration),
        "write tool call should produce CodeGeneration signal"
    );
    assert!(
        signals.contains(&InteractionSignal::Research),
        "web_search tool call should produce Research signal"
    );
    assert!(
        signals.contains(&InteractionSignal::ErrorRecovery),
        "failed tool call should produce ErrorRecovery signal"
    );
}

#[tokio::test]
async fn max_iterations_one_exits_immediately() {
    let mut providers = ProviderRegistry::new();
    providers.register(Box::new(
        MockProvider::with_responses(vec![make_tool_response(
            "exec",
            "toolu_1",
            serde_json::json!({}),
        )])
        .models(&["test-model"]),
    ));

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

    assert_eq!(
        result.usage.llm_calls, 1,
        "with max_tool_iterations=1, should exit after the first LLM call"
    );
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
    assert_eq!(
        built[0].role,
        Role::User,
        "user role should map to Role::User"
    );
    assert_eq!(
        built[1].role,
        Role::Assistant,
        "assistant role should map to Role::Assistant"
    );
    assert_eq!(
        built[2].role,
        Role::User,
        "unknown role should fall back to Role::User"
    ); // unknown maps to User
}
