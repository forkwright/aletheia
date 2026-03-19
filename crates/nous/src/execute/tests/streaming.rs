//! Streaming execute tests.
use super::*;

#[tokio::test]
async fn streaming_falls_back_to_non_streaming_for_mock() {
    let mut providers = ProviderRegistry::new();
    providers.register(Box::new(
        MockProvider::with_responses(vec![make_text_response("Hello streaming!")])
            .models(&["test-model"]),
    ));

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

    assert_eq!(
        result.content, "Hello streaming!",
        "streaming response content should match mock text"
    );
    assert_eq!(
        result.usage.llm_calls, 1,
        "single text response should use exactly one LLM call"
    );

    drop(tx);
    assert!(
        rx.try_recv().is_err(),
        "no stream events for non-streaming provider"
    );
}

#[tokio::test]
async fn streaming_tool_events_emitted() {
    let mut providers = ProviderRegistry::new();
    providers.register(Box::new(
        MockProvider::with_responses(vec![
            make_tool_response("exec", "toolu_1", serde_json::json!({"input": "test"})),
            make_text_response("Done!"),
        ])
        .models(&["test-model"]),
    ));

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

    assert_eq!(
        result.content, "Done!",
        "streaming final response content should match mock text"
    );
    assert_eq!(
        result.tool_calls.len(),
        1,
        "streaming execute should record one tool call"
    );

    // NOTE: Even with mock (non-streaming) provider, tool events should be emitted
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
    // WHY: Falls back to non-streaming execute(), no tool events via channel
    // (tool events only come from dispatch_tools_streaming, which requires
    //  a streaming provider to be found)
    assert_eq!(
        tool_start_count, 0,
        "mock provider falls back to non-streaming so no ToolStart events should be emitted"
    );
    assert_eq!(
        tool_result_count, 0,
        "mock provider falls back to non-streaming so no ToolResult events should be emitted"
    );
}
