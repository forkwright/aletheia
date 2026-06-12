//! Streaming execute tests.
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use hermeneus::provider::LlmProvider;

use super::*;
use crate::approval::{ApprovalChoice, ApprovalDecision, ApprovalGate};

struct StreamingMockProvider {
    inner: MockProvider,
}

impl StreamingMockProvider {
    fn with_responses(responses: Vec<CompletionResponse>) -> Self {
        Self {
            inner: MockProvider::with_responses(responses).models(&["test-model"]),
        }
    }
}

impl LlmProvider for StreamingMockProvider {
    fn complete<'a>(
        &'a self,
        request: &'a hermeneus::types::CompletionRequest,
    ) -> Pin<Box<dyn Future<Output = hermeneus::error::Result<CompletionResponse>> + Send + 'a>>
    {
        self.inner.complete(request)
    }

    fn supported_models(&self) -> &[&str] {
        self.inner.supported_models()
    }

    fn name(&self) -> &'static str {
        "streaming-test"
    }

    fn supports_streaming(&self) -> bool {
        true
    }
}

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
        None,
        None,
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
        None,
        None,
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
    assert_eq!(
        tool_start_count, 1,
        "fallback dispatch should emit the same ToolStart event as streaming dispatch"
    );
    assert_eq!(
        tool_result_count, 1,
        "fallback dispatch should emit the same ToolResult event as streaming dispatch"
    );
}

#[tokio::test]
async fn streaming_denies_unadvertised_lazy_tool_before_execution() {
    let mut providers = ProviderRegistry::new();
    providers.register(Box::new(StreamingMockProvider::with_responses(vec![
        make_tool_response("lazy_exec", "toolu_1", serde_json::json!({"input": "test"})),
        make_text_response("Done!"),
    ])));

    let executions = Arc::new(AtomicUsize::new(0));
    let mut def = make_tool_def("lazy_exec");
    def.auto_activate = false;
    let mut tools = ToolRegistry::new();
    tools
        .register(
            def,
            Box::new(CountingExecutor::new(Arc::clone(&executions))),
        )
        .expect("register");
    let (tx, mut rx) = tokio::sync::mpsc::channel::<TurnStreamEvent>(64);

    let result = execute_streaming(
        &test_pipeline_ctx(),
        &test_session(),
        &test_config(),
        &providers,
        &tools,
        &test_tool_ctx(),
        &tx,
        None,
        None,
    )
    .await
    .expect("execute_streaming");

    assert_eq!(result.content, "Done!");
    assert_eq!(
        executions.load(Ordering::SeqCst),
        0,
        "streaming lazy denial must not run executor"
    );
    assert_eq!(result.tool_calls.len(), 1);
    let call = result
        .tool_calls
        .first()
        .expect("tool call should be recorded");
    assert!(call.is_error);
    assert!(
        call.result
            .as_deref()
            .unwrap_or_default()
            .contains("not active")
    );

    drop(tx);
    let mut tool_start = 0;
    let mut tool_result = 0;
    while let Ok(event) = rx.try_recv() {
        match event {
            TurnStreamEvent::ToolStart { .. } => tool_start += 1,
            TurnStreamEvent::ToolResult {
                result, is_error, ..
            } => {
                tool_result += 1;
                assert!(is_error);
                assert!(result.contains("not active"));
            }
            _ => {}
        }
    }
    assert_eq!(tool_start, 0, "inactive lazy tool must not start");
    assert_eq!(tool_result, 1, "inactive lazy denial must emit one result");
}

#[tokio::test]
async fn streaming_fallback_uses_approval_gate_for_mandatory_tool() {
    let mut providers = ProviderRegistry::new();
    providers.register(Box::new(
        MockProvider::with_responses(vec![
            make_tool_response("exec", "toolu_1", serde_json::json!({"input": "test"})),
            make_text_response("Denied handled"),
        ])
        .models(&["test-model"]),
    ));

    let mut tools = ToolRegistry::new();
    let mut def = make_tool_def("exec");
    def.reversibility = organon::types::Reversibility::Irreversible;
    tools
        .register(def, Box::new(EchoExecutor))
        .expect("register");
    let (tx, mut rx) = tokio::sync::mpsc::channel::<TurnStreamEvent>(64);
    let (decision_tx, decision_rx) = tokio::sync::mpsc::channel::<ApprovalDecision>(4);
    let gate = ApprovalGate::new(decision_rx, Duration::from_secs(5));
    decision_tx
        .send(ApprovalDecision {
            tool_id: "toolu_1".to_owned(),
            choice: ApprovalChoice::Denied,
        })
        .await
        .expect("send denial");

    let result = execute_streaming(
        &test_pipeline_ctx(),
        &test_session(),
        &test_config(),
        &providers,
        &tools,
        &test_tool_ctx(),
        &tx,
        Some(&gate),
        None,
    )
    .await
    .expect("execute_streaming fallback");

    assert_eq!(result.content, "Denied handled");
    assert_eq!(result.tool_calls.len(), 1);
    let tool_call = result.tool_calls.first().expect("one denied tool call");
    assert!(tool_call.is_error);
    assert!(
        tool_call
            .result
            .as_deref()
            .unwrap_or_default()
            .contains("denied by user")
    );

    drop(tx);
    let mut approval_required = 0;
    let mut approval_resolved = 0;
    let mut tool_start = 0;
    let mut tool_result = 0;
    while let Ok(event) = rx.try_recv() {
        match event {
            TurnStreamEvent::ToolApprovalRequired { .. } => approval_required += 1,
            TurnStreamEvent::ToolApprovalResolved { decision, .. } => {
                approval_resolved += 1;
                assert_eq!(decision, "denied");
            }
            TurnStreamEvent::ToolStart { .. } => tool_start += 1,
            TurnStreamEvent::ToolResult { .. } => tool_result += 1,
            TurnStreamEvent::LlmDelta(_) => {}
        }
    }
    assert_eq!(approval_required, 1);
    assert_eq!(approval_resolved, 1);
    assert_eq!(tool_start, 0, "denied fallback call must not execute");
    assert_eq!(tool_result, 1);
}
