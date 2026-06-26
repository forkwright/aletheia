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

/// Provider that emits a configurable number of text deltas in `complete_streaming`.
struct DeltaEmitter {
    deltas: usize,
    response: CompletionResponse,
}

impl DeltaEmitter {
    fn new(deltas: usize, response: CompletionResponse) -> Self {
        Self { deltas, response }
    }
}

impl LlmProvider for DeltaEmitter {
    fn complete<'a>(
        &'a self,
        _request: &'a hermeneus::types::CompletionRequest,
    ) -> Pin<Box<dyn Future<Output = hermeneus::error::Result<CompletionResponse>> + Send + 'a>>
    {
        let response = self.response.clone();
        Box::pin(async move { Ok(response) })
    }

    fn complete_streaming<'a>(
        &'a self,
        _request: &'a hermeneus::types::CompletionRequest,
        on_event: &'a mut (dyn FnMut(hermeneus::anthropic::StreamEvent) + Send),
    ) -> Pin<Box<dyn Future<Output = hermeneus::error::Result<CompletionResponse>> + Send + 'a>>
    {
        for i in 0..self.deltas {
            on_event(hermeneus::anthropic::StreamEvent::TextDelta {
                text: format!("delta-{i}"),
            });
        }
        let response = self.response.clone();
        Box::pin(async move { Ok(response) })
    }

    fn supported_models(&self) -> &[&str] {
        &["test-model"]
    }

    fn name(&self) -> &'static str {
        "delta-emitter"
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

#[tokio::test]
async fn streaming_max_iterations_reports_stop_reason() {
    let mut providers = ProviderRegistry::new();
    let responses: Vec<CompletionResponse> = (0..10)
        .map(|i| make_tool_response("exec", &format!("toolu_{i}"), serde_json::json!({"i": i})))
        .collect();
    providers.register(Box::new(StreamingMockProvider::with_responses(responses)));

    let tools = make_registry_with("exec", Box::new(EchoExecutor));
    let mut config = test_config();
    config.limits.max_tool_iterations = 3;
    config.limits.loop_detection_threshold = 100;

    let (tx, _rx) = tokio::sync::mpsc::channel::<TurnStreamEvent>(64);

    let result = execute_streaming(
        &test_pipeline_ctx(),
        &test_session(),
        &config,
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
        result.stop_reason, "max_tool_iterations",
        "streaming stop reason should report max tool iterations cutoff"
    );
    assert_eq!(
        result.usage.llm_calls, 3,
        "streaming should stop after max_tool_iterations=3 LLM calls"
    );
}

#[tokio::test]
async fn streaming_client_disconnect_reports_stop_reason() {
    let mut providers = ProviderRegistry::new();
    // WHY: the provider would loop forever on tool_use if disconnect were ignored.
    let responses: Vec<CompletionResponse> = (0..10)
        .map(|i| make_tool_response("exec", &format!("toolu_{i}"), serde_json::json!({"i": i})))
        .collect();
    providers.register(Box::new(StreamingMockProvider::with_responses(responses)));

    let tools = make_registry_with("exec", Box::new(EchoExecutor));
    let mut config = test_config();
    config.limits.max_tool_iterations = 100;
    config.limits.loop_detection_threshold = 1000;

    let (tx, rx) = tokio::sync::mpsc::channel::<TurnStreamEvent>(64);
    drop(rx);

    let result = execute_streaming(
        &test_pipeline_ctx(),
        &test_session(),
        &config,
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
        result.stop_reason, "client_disconnect",
        "streaming stop reason should report client disconnect"
    );
    assert_eq!(
        result.usage.llm_calls, 0,
        "disconnected stream should not call the LLM"
    );
    assert!(
        result.tool_calls.is_empty(),
        "disconnected stream should not dispatch tools"
    );
}

#[tokio::test]
async fn streaming_dropped_llm_deltas_record_metric() {
    // WHY(#4893): A saturated stream channel must not silently lose user-visible
    // LLM deltas. This test forces the channel to capacity 1 and emits multiple
    // text deltas, asserting that the drop counter increments with the
    // `text_delta` event type label.
    let mut providers = ProviderRegistry::new();
    providers.register(Box::new(DeltaEmitter::new(5, make_text_response("done"))));

    let tools = ToolRegistry::new();
    let (tx, _rx) = tokio::sync::mpsc::channel::<TurnStreamEvent>(1);

    let registry = koina::metrics::MetricsRegistry::new();
    registry.with_registry(crate::metrics::register);

    execute_streaming(
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

    let mut buf = String::new();
    registry
        .encode(&mut buf)
        .expect("encoding into String is infallible");

    assert!(
        buf.contains("aletheia_stream_events_dropped_total"),
        "expected stream drop metric, got: {buf}"
    );
    assert!(
        buf.contains("event_type=\"text_delta\""),
        "expected text_delta event type label, got: {buf}"
    );
    assert!(
        buf.contains("reason=\"buffer_full\""),
        "expected buffer_full reason label, got: {buf}"
    );
}

/// Provider that emits text deltas with yields between them, allowing a
/// concurrent task to drop the receiver mid-stream.
struct SlowDeltaEmitter {
    deltas: usize,
    response: CompletionResponse,
}

impl SlowDeltaEmitter {
    fn new(deltas: usize, response: CompletionResponse) -> Self {
        Self { deltas, response }
    }
}

impl LlmProvider for SlowDeltaEmitter {
    fn complete<'a>(
        &'a self,
        _request: &'a hermeneus::types::CompletionRequest,
    ) -> Pin<Box<dyn Future<Output = hermeneus::error::Result<CompletionResponse>> + Send + 'a>>
    {
        let response = self.response.clone();
        Box::pin(async move { Ok(response) })
    }

    fn complete_streaming<'a>(
        &'a self,
        _request: &'a hermeneus::types::CompletionRequest,
        on_event: &'a mut (dyn FnMut(hermeneus::anthropic::StreamEvent) + Send),
    ) -> Pin<Box<dyn Future<Output = hermeneus::error::Result<CompletionResponse>> + Send + 'a>>
    {
        let response = self.response.clone();
        let deltas = self.deltas;
        Box::pin(async move {
            for i in 0..deltas {
                on_event(hermeneus::anthropic::StreamEvent::TextDelta {
                    text: format!("delta-{i}"),
                });
                tokio::task::yield_now().await;
            }
            Ok(response)
        })
    }

    fn supported_models(&self) -> &[&str] {
        &["test-model"]
    }

    fn name(&self) -> &'static str {
        "slow-delta-emitter"
    }

    fn supports_streaming(&self) -> bool {
        true
    }
}

/// Tool executor that drops the stream receiver when it runs, simulating a
/// client disconnect after a `tool_use` response has been dispatched.
struct DisconnectOnExecute {
    rx: tokio::sync::Mutex<Option<tokio::sync::mpsc::Receiver<TurnStreamEvent>>>,
}

impl DisconnectOnExecute {
    fn new(rx: tokio::sync::mpsc::Receiver<TurnStreamEvent>) -> Self {
        Self {
            rx: tokio::sync::Mutex::new(Some(rx)),
        }
    }
}

impl ToolExecutor for DisconnectOnExecute {
    fn execute<'a>(
        &'a self,
        input: &'a ToolInput,
        _ctx: &'a ToolContext,
    ) -> Pin<Box<dyn Future<Output = organon::error::Result<ToolResult>> + Send + 'a>> {
        Box::pin(async move {
            let mut guard = self.rx.lock().await;
            drop(guard.take());
            Ok(ToolResult::text(format!(
                "executed: {}",
                input.name.as_str()
            )))
        })
    }
}

#[tokio::test]
async fn streaming_client_disconnect_mid_delta_reports_stop_reason() {
    let mut providers = ProviderRegistry::new();
    providers.register(Box::new(SlowDeltaEmitter::new(
        10,
        make_text_response("partial answer"),
    )));

    let tools = ToolRegistry::new();
    let (tx, mut rx) = tokio::sync::mpsc::channel::<TurnStreamEvent>(1);

    // WHY: read a couple of deltas so the disconnect happens mid-stream, then
    // drop the receiver so the remaining deltas and final response are dropped.
    let reader = tokio::spawn(async move {
        let mut seen = 0;
        while let Some(event) = rx.recv().await {
            if matches!(event, TurnStreamEvent::LlmDelta(_)) {
                seen += 1;
                if seen >= 2 {
                    break;
                }
            }
        }
    });

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

    let _ = reader.await;

    assert_eq!(
        result.stop_reason, "client_disconnect",
        "streaming stop reason should report client disconnect when receiver drops mid-delta"
    );
    assert_eq!(
        result.usage.llm_calls, 1,
        "one LLM call should have completed before detecting disconnect"
    );
    assert_eq!(
        result.content, "partial answer",
        "partial response content should still be captured"
    );
}

#[tokio::test]
async fn streaming_client_disconnect_after_tool_use_reports_stop_reason() {
    let mut providers = ProviderRegistry::new();
    // WHY: first response requests a tool; the executor will drop the receiver.
    // The second response should never be reached because the closed check
    // breaks the loop on the next iteration.
    providers.register(Box::new(StreamingMockProvider::with_responses(vec![
        make_tool_response("exec", "toolu_1", serde_json::json!({"input": "test"})),
        make_text_response("should not be reached"),
    ])));

    let (tx, rx) = tokio::sync::mpsc::channel::<TurnStreamEvent>(64);
    let tools = make_registry_with("exec", Box::new(DisconnectOnExecute::new(rx)));

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
        result.stop_reason, "client_disconnect",
        "streaming stop reason should report client disconnect after tool-use disconnect"
    );
    assert_eq!(
        result.usage.llm_calls, 1,
        "only one LLM call should run before disconnect"
    );
    assert_eq!(
        result.tool_calls.len(),
        1,
        "the tool call dispatched before disconnect should be recorded"
    );
}
