#![expect(
    clippy::indexing_slicing,
    reason = "test: vec indices are valid after asserting len"
)]
//! Edge case and utility tests.
use organon::types::ToolDiagnostics;

use super::*;

use crate::execute::spawn_guard::enforce_spawn_isolation;
use crate::history::{HistoryConfig, load_history};

// ── spawn-class isolation guard (#186) ───────────────────────────────────────

struct NoopExecutor;

impl ToolExecutor for NoopExecutor {
    fn execute<'a>(
        &'a self,
        _input: &'a ToolInput,
        _ctx: &'a ToolContext,
    ) -> Pin<Box<dyn Future<Output = organon::error::Result<ToolResult>> + Send + 'a>> {
        Box::pin(async { Ok(ToolResult::text("ok")) })
    }
}

fn make_spawn_def(name: &str) -> ToolDef {
    ToolDef {
        name: ToolName::new(name).expect("valid"),
        description: format!("Test tool: {name}"),
        extended_description: None,
        input_schema: InputSchema {
            properties: indexmap::IndexMap::default(),
            required: vec![],
        },
        category: ToolCategory::Workspace,
        reversibility: organon::types::Reversibility::Irreversible,
        auto_activate: false,
        groups: vec![organon::types::ToolGroupId::SpawnSubtask],
        tags: vec![],
    }
}

fn make_read_def(name: &str) -> ToolDef {
    ToolDef {
        name: ToolName::new(name).expect("valid"),
        description: format!("Test tool: {name}"),
        extended_description: None,
        input_schema: InputSchema {
            properties: indexmap::IndexMap::default(),
            required: vec![],
        },
        category: ToolCategory::Workspace,
        reversibility: organon::types::Reversibility::Irreversible,
        auto_activate: false,
        groups: vec![organon::types::ToolGroupId::Read],
        tags: vec![],
    }
}

fn make_spawn_registry() -> ToolRegistry {
    let mut registry = ToolRegistry::new();
    registry
        .register(make_spawn_def("spawn_subagent"), Box::new(NoopExecutor))
        .expect("register");
    registry
        .register(make_read_def("read_file"), Box::new(NoopExecutor))
        .expect("register");
    registry
        .register(make_read_def("write_file"), Box::new(NoopExecutor))
        .expect("register");
    registry
}

#[test]
fn spawn_isolation_truncates_after_spawn() {
    let mut tool_uses = vec![
        (
            "tu_1".to_owned(),
            "read_file".to_owned(),
            serde_json::json!({}),
        ),
        (
            "tu_2".to_owned(),
            "spawn_subagent".to_owned(),
            serde_json::json!({}),
        ),
        (
            "tu_3".to_owned(),
            "write_file".to_owned(),
            serde_json::json!({}),
        ),
    ];
    let mut denied = Vec::new();
    let tools = make_spawn_registry();

    enforce_spawn_isolation(&mut tool_uses, &mut denied, &tools);

    assert_eq!(tool_uses.len(), 2);
    assert_eq!(tool_uses[0].1, "read_file");
    assert_eq!(tool_uses[1].1, "spawn_subagent");
    assert_eq!(denied.len(), 1);
    match &denied[0] {
        hermeneus::types::ContentBlock::ToolResult {
            tool_use_id,
            content,
            is_error,
        } => {
            assert_eq!(tool_use_id, "tu_3");
            match content {
                hermeneus::types::ToolResultContent::Text(text) => {
                    assert_eq!(
                        text,
                        "Tool call dropped: spawn-class tool calls cannot co-occur with other tool calls in the same turn."
                    );
                }
                _ => panic!("expected Text content"),
            }
            assert_eq!(*is_error, Some(true));
        }
        _ => panic!("expected ToolResult block"),
    }
}

#[test]
fn spawn_isolation_passes_through_when_spawn_is_last() {
    let mut tool_uses = vec![
        (
            "tu_1".to_owned(),
            "read_file".to_owned(),
            serde_json::json!({}),
        ),
        (
            "tu_2".to_owned(),
            "spawn_subagent".to_owned(),
            serde_json::json!({}),
        ),
    ];
    let mut denied = Vec::new();
    let tools = make_spawn_registry();

    enforce_spawn_isolation(&mut tool_uses, &mut denied, &tools);

    assert_eq!(tool_uses.len(), 2);
    assert!(denied.is_empty());
}

#[test]
fn spawn_isolation_passes_through_when_no_spawn() {
    let mut tool_uses = vec![
        (
            "tu_1".to_owned(),
            "read_file".to_owned(),
            serde_json::json!({}),
        ),
        (
            "tu_2".to_owned(),
            "write_file".to_owned(),
            serde_json::json!({}),
        ),
    ];
    let mut denied = Vec::new();
    let tools = make_spawn_registry();

    enforce_spawn_isolation(&mut tool_uses, &mut denied, &tools);

    assert_eq!(tool_uses.len(), 2);
    assert!(denied.is_empty());
}

#[test]
fn spawn_isolation_truncates_multiple_after_spawn() {
    let mut tool_uses = vec![
        (
            "tu_1".to_owned(),
            "spawn_subagent".to_owned(),
            serde_json::json!({}),
        ),
        (
            "tu_2".to_owned(),
            "read_file".to_owned(),
            serde_json::json!({}),
        ),
        (
            "tu_3".to_owned(),
            "write_file".to_owned(),
            serde_json::json!({}),
        ),
    ];
    let mut denied = Vec::new();
    let tools = make_spawn_registry();

    enforce_spawn_isolation(&mut tool_uses, &mut denied, &tools);

    assert_eq!(tool_uses.len(), 1);
    assert_eq!(tool_uses[0].1, "spawn_subagent");
    assert_eq!(denied.len(), 2);
}

struct DiagnosticExecutor;

impl ToolExecutor for DiagnosticExecutor {
    fn execute<'a>(
        &'a self,
        _input: &'a ToolInput,
        _ctx: &'a ToolContext,
    ) -> Pin<Box<dyn Future<Output = organon::error::Result<ToolResult>> + Send + 'a>> {
        Box::pin(async {
            Ok(
                ToolResult::error("command failed").with_diagnostics(ToolDiagnostics {
                    exit_code: Some(127),
                    stderr: Some("command not found".to_owned()),
                    sandbox_violations: Vec::new(),
                    duration_ms: 0,
                }),
            )
        })
    }
}

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
        None,
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
        cost_usd: None,
        duration_ms: None,
    };
    providers.register(Box::new(
        MockProvider::with_responses(vec![response]).models(&["test-model"]),
    ));

    let tools = ToolRegistry::new();
    let mut config = test_config();
    config.generation.thinking_enabled = true;
    config.generation.thinking_budget = 5_000;

    let result = execute(
        &test_pipeline_ctx(),
        &test_session(),
        &config,
        &providers,
        &tools,
        &test_tool_ctx(),
        None,
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
        None,
    )
    .await;

    assert!(
        err.is_err(),
        "execute with no matching provider should return an error"
    );
    let msg = err
        .expect_err("execute with no matching provider should error")
        .to_string();
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
        approval: None,
        receipt: None,
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
        approval: None,
        receipt: None,
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
            approval: None,
            receipt: None,
        },
        ToolCall {
            id: "2".to_owned(),
            name: "web_search".to_owned(),
            input: serde_json::json!({}),
            result: None,
            is_error: true,
            duration_ms: 5,
            approval: None,
            receipt: None,
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
    config.limits.max_tool_iterations = 1;
    config.limits.loop_detection_threshold = 100;

    let result = execute(
        &test_pipeline_ctx(),
        &test_session(),
        &config,
        &providers,
        &tools,
        &test_tool_ctx(),
        None,
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
        PipelineMessage::text("user", "Hello", 1),
        PipelineMessage::text("assistant", "Hi", 1),
        PipelineMessage::text("unknown", "?", 1),
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

fn store_with_persisted_tool_history() -> mneme::store::SessionStore {
    use mneme::store::{
        FinalizeMessage, FinalizeToolAuditRecord, FinalizeTurnRequest, SessionStore,
    };
    use mneme::types::Role as StoreRole;

    let store = SessionStore::open_in_memory().expect("open store");
    let messages = [
        FinalizeMessage {
            role: StoreRole::User,
            content: "Read README.md",
            tool_call_id: None,
            tool_name: None,
            token_estimate: 4,
        },
        FinalizeMessage {
            role: StoreRole::Assistant,
            content: r#"{"path":"README.md"}"#,
            tool_call_id: Some("toolu_5012"),
            tool_name: Some("read_file"),
            token_estimate: 0,
        },
        FinalizeMessage {
            role: StoreRole::ToolResult,
            content: "[tool:read_file@1970-01-01T00:00:00Z] file contents",
            tool_call_id: Some("toolu_5012"),
            tool_name: Some("read_file"),
            token_estimate: 8,
        },
        FinalizeMessage {
            role: StoreRole::Assistant,
            content: "Done.",
            tool_call_id: None,
            tool_name: None,
            token_estimate: 2,
        },
    ];
    let audits = [FinalizeToolAuditRecord {
        turn_seq: 1,
        tool_call_id: "toolu_5012",
        tool_name: "read_file",
        duration_ms: 42,
        is_error: false,
        outcome: "success",
        result: Some("file contents"),
        approval: Some("approved"),
        receipt: Some("receipt-token"),
    }];
    store
        .finalize_turn(&FinalizeTurnRequest {
            session_id: "ses-typed-history",
            nous_id: "test-agent",
            session_key: "main",
            model: Some("test-model"),
            parent_session_id: None,
            messages: &messages,
            usage: None,
            tool_audit_records: &audits,
            completion_note: None,
        })
        .expect("finalize turn");
    store
}

fn assert_loaded_tool_history(pipeline_messages: &[PipelineMessage]) {
    assert_eq!(pipeline_messages[1].role, "assistant");
    assert_eq!(
        pipeline_messages[1].tool_call_id.as_deref(),
        Some("toolu_5012")
    );
    assert_eq!(pipeline_messages[2].role, "tool_result");
    assert_eq!(pipeline_messages[2].tool_name.as_deref(), Some("read_file"));
    assert_eq!(pipeline_messages[2].tool_is_error, Some(false));
    assert_eq!(pipeline_messages[2].tool_duration_ms, Some(42));
    assert_eq!(
        pipeline_messages[2].tool_approval.as_deref(),
        Some("approved")
    );
    assert_eq!(
        pipeline_messages[2].tool_receipt.as_deref(),
        Some("receipt-token")
    );
}

fn assert_typed_tool_use(message: &hermeneus::types::Message) {
    assert_eq!(message.role, Role::Assistant);
    let hermeneus::types::Content::Blocks(tool_use_blocks) = &message.content else {
        panic!("assistant tool call should be typed content");
    };
    match &tool_use_blocks[0] {
        ContentBlock::ToolUse { id, name, input } => {
            assert_eq!(id, "toolu_5012");
            assert_eq!(name, "read_file");
            assert_eq!(input["path"], "README.md");
        }
        other => panic!("expected tool_use block, got {other:?}"),
    }
}

fn assert_typed_tool_result(message: &hermeneus::types::Message) {
    assert_eq!(message.role, Role::User);
    let hermeneus::types::Content::Blocks(tool_result_blocks) = &message.content else {
        panic!("tool result should be typed content");
    };
    match &tool_result_blocks[0] {
        ContentBlock::ToolResult {
            tool_use_id,
            content,
            is_error,
        } => {
            assert_eq!(tool_use_id, "toolu_5012");
            assert_eq!(*is_error, Some(false));
            match content {
                hermeneus::types::ToolResultContent::Text(text) => {
                    assert!(text.contains("file contents"));
                }
                other => panic!("expected text tool result, got {other:?}"),
            }
        }
        other => panic!("expected tool_result block, got {other:?}"),
    }
}

#[test]
fn persisted_tool_history_builds_typed_context_blocks() {
    let store = store_with_persisted_tool_history();
    let config = HistoryConfig {
        max_messages: 10,
        reserve_for_current: 0,
        include_tool_messages: true,
    };
    let (pipeline_messages, result) =
        load_history(&store, "ses-typed-history", 10_000, &config, "next").expect("load history");

    assert_eq!(result.messages_loaded, 4);
    assert_loaded_tool_history(&pipeline_messages);

    let built = build_messages(&pipeline_messages);
    assert_typed_tool_use(&built[1]);
    assert_typed_tool_result(&built[2]);
    assert_eq!(built.last().expect("current message").role, Role::User);
}

#[tokio::test]
async fn tool_diagnostics_injected_into_llm_content() {
    let mock = MockProvider::with_responses(vec![
        make_tool_response("diag_tool", "tu_1", serde_json::json!({})),
        make_text_response("recovered"),
    ])
    .models(&["test-model"]);

    let mut providers = ProviderRegistry::new();
    providers.register(Box::new(mock));

    let tools = make_registry_with("diag_tool", Box::new(DiagnosticExecutor));
    let result = execute(
        &test_pipeline_ctx(),
        &test_session(),
        &test_config(),
        &providers,
        &tools,
        &test_tool_ctx(),
        None,
    )
    .await
    .expect("pipeline should complete");

    assert!(
        result.tool_calls.iter().any(|tc| tc.is_error),
        "tool call should be marked as error"
    );

    // Verify via tool_calls metadata: the result summary is computed from
    // the truncated content, which includes the injected diagnostics.
    let tc = result
        .tool_calls
        .iter()
        .find(|tc| tc.name == "diag_tool")
        .expect("should have diag_tool call");
    let result_text = tc.result.as_ref().expect("should have result");
    assert!(
        result_text.contains("exit_code=127"),
        "result summary should contain diagnostic exit_code: {result_text}"
    );
    assert!(
        result_text.contains("command not found"),
        "result summary should contain diagnostic stderr: {result_text}"
    );
}
