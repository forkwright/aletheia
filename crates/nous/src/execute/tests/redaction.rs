//! Declared-redaction enforcement tests (#6808): the registry's declared
//! `RedactionPolicy` is applied at the dispatch boundary to durable/replay
//! surfaces and independently to the prepared live-approval evidence. The
//! executor still sees the exact prepared arguments, while persisted
//! `ToolCall`, tool lifecycle events, and receipt-ledger display copies remain
//! replay-safe.

use std::time::Duration;

use koina::id::ToolName;
use organon::registry::ToolRegistry;
use organon::types::{RedactionPolicy, Reversibility, ToolCapabilityMetadata};
use tokio::sync::mpsc;

use super::*;
use crate::approval::{ApprovalChoice, ApprovalDecision, ApprovalGate};
use crate::execute::dispatch::{ToolDispatchPolicy, dispatch_tools};
use crate::pipeline::LoopDetector;
use crate::stream::TurnStreamEvent;

fn registry_with_redaction(
    name: &str,
    reversibility: Reversibility,
    redaction: RedactionPolicy,
) -> ToolRegistry {
    let mut registry = ToolRegistry::new();
    let mut def = make_tool_def_rev(name, reversibility);
    if let RedactionPolicy::Fields(fields) = &redaction {
        for field in fields {
            def.input_schema
                .properties
                .insert(field.clone(), organon::types::PropertyDef::default());
        }
    }
    registry
        .register(def, Box::new(EchoExecutor))
        .expect("register test tool");
    registry
        .declare_capability(
            ToolName::new(name).expect("valid test tool name"),
            ToolCapabilityMetadata {
                owner: "nous::execute::tests".to_owned(),
                redaction,
                ..ToolCapabilityMetadata::default()
            },
        )
        .expect("valid test capability declaration");
    registry
}

struct DispatchOutcome {
    calls: Vec<crate::pipeline::ToolCall>,
    events: Vec<TurnStreamEvent>,
}

struct SensitiveFailureExecutor;

struct CanonicalPathEchoExecutor;

impl organon::registry::ToolExecutor for CanonicalPathEchoExecutor {
    fn path_arguments(&self) -> &'static [&'static str] {
        &["path"]
    }

    fn execute<'a>(
        &'a self,
        input: &'a organon::types::ToolInput,
        ctx: &'a organon::types::ToolContext,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<Output = organon::error::Result<organon::types::ToolResult>>
                + Send
                + 'a,
        >,
    > {
        organon::registry::ToolExecutor::execute(&EchoExecutor, input, ctx)
    }
}

impl organon::registry::ToolExecutor for SensitiveFailureExecutor {
    fn execute<'a>(
        &'a self,
        _input: &'a organon::types::ToolInput,
        _ctx: &'a organon::types::ToolContext,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<Output = organon::error::Result<organon::types::ToolResult>>
                + Send
                + 'a,
        >,
    > {
        Box::pin(async { Ok(organon::types::ToolResult::error("password=outcome-secret")) })
    }
}

fn tool_services_with_secret(
    name: &str,
    value: &str,
) -> std::sync::Arc<organon::types::ToolServices> {
    organon::testing::install_crypto_provider();
    let secret_vault = hermeneus::secret::SecretVault::new();
    secret_vault.store(name, value);
    std::sync::Arc::new(organon::types::ToolServices {
        cross_nous: None,
        messenger: None,
        note_store: None,
        blackboard_store: None,
        spawn: None,
        planning: None,
        knowledge: None,
        working_checkpoint_store: None,
        http_clients: organon::types::ToolHttpClients {
            general: reqwest::Client::new(),
            ssrf_safe: reqwest::Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .unwrap_or_else(|_| reqwest::Client::new()),
        },
        secret_vault,
        lazy_tool_catalog: Vec::new(),
        server_tool_config: organon::types::ServerToolConfig::default(),
    })
}

async fn dispatch_one(
    tools: &ToolRegistry,
    tool_name: &str,
    input: serde_json::Value,
    approval_gate: Option<&ApprovalGate>,
    receipt_ledger: Option<&std::sync::Mutex<organon::receipts::ReceiptLedger>>,
) -> DispatchOutcome {
    dispatch_one_with_ctx(
        tools,
        tool_name,
        input,
        &test_tool_ctx(),
        approval_gate,
        receipt_ledger,
    )
    .await
}

async fn dispatch_one_with_ctx(
    tools: &ToolRegistry,
    tool_name: &str,
    input: serde_json::Value,
    tool_ctx: &organon::types::ToolContext,
    approval_gate: Option<&ApprovalGate>,
    receipt_ledger: Option<&std::sync::Mutex<organon::receipts::ReceiptLedger>>,
) -> DispatchOutcome {
    dispatch_one_with_ctx_and_signer(
        tools,
        tool_name,
        input,
        tool_ctx,
        approval_gate,
        receipt_ledger,
        None,
    )
    .await
}

async fn dispatch_one_with_ctx_and_signer(
    tools: &ToolRegistry,
    tool_name: &str,
    input: serde_json::Value,
    tool_ctx: &organon::types::ToolContext,
    approval_gate: Option<&ApprovalGate>,
    receipt_ledger: Option<&std::sync::Mutex<organon::receipts::ReceiptLedger>>,
    receipt_signer: Option<&organon::receipts::ReceiptSigner>,
) -> DispatchOutcome {
    let (event_tx, mut event_rx) = mpsc::channel::<TurnStreamEvent>(64);
    let tool_uses = vec![("tool-1".to_owned(), tool_name.to_owned(), input)];
    let mut loop_detector = LoopDetector::new(3);
    let mut all_calls = Vec::new();
    let policy = ToolDispatchPolicy::allow_all_for_tests(tools);

    dispatch_tools(
        &tool_uses,
        tools,
        tool_ctx,
        &mut loop_detector,
        &mut all_calls,
        1,
        Some(&event_tx),
        approval_gate,
        &policy,
        0,
        receipt_signer,
        receipt_ledger,
    )
    .await
    .expect("dispatch ok");

    drop(event_tx);
    let mut events = Vec::new();
    while let Ok(event) = event_rx.try_recv() {
        events.push(event);
    }
    DispatchOutcome {
        calls: all_calls,
        events,
    }
}

#[tokio::test]
async fn full_policy_redacts_recorded_input_and_result() {
    let tool = "_test_redaction_full";
    // WHY these values: short, whitespace-bearing, prose-like — the
    // hermeneus content heuristic (long, whitespace-free strings) would NOT
    // catch them, so any redaction observed is the declared policy's alone.
    let input = serde_json::json!({"note": "standup moved to nine", "count": 2});
    let tools =
        registry_with_redaction(tool, Reversibility::FullyReversible, RedactionPolicy::Full);

    let outcome = dispatch_one(&tools, tool, input, None, None).await;

    assert_eq!(outcome.calls.len(), 1);
    let call = &outcome.calls[0];
    assert_eq!(
        call.input,
        serde_json::json!({"__redaction__": "[REDACTED]"}),
        "Full uses one fixed payload with no input-derived shape"
    );
    assert_eq!(
        call.result.as_deref(),
        Some("[REDACTED]"),
        "Full redacts the recorded result text"
    );
    // The LLM-facing result block is deliberately NOT redacted: the model
    // mid-turn needs the real output; the policy governs the trace.
    let tool_result = outcome
        .events
        .iter()
        .find_map(|e| match e {
            TurnStreamEvent::ToolResult { result, .. } => Some(result.clone()),
            _ => None,
        })
        .expect("tool result event emitted");
    assert_eq!(
        tool_result, "[REDACTED]",
        "the stream-side result is a trace surface and follows the policy"
    );
}

#[tokio::test]
async fn fields_policy_redacts_named_field_and_passes_others() {
    let tool = "_test_redaction_fields";
    let input = serde_json::json!({
        "url": "https://acme.corp/api",
        "headers": {"PRIVATE-TOKEN": "tok-value"},
    });
    let tools = registry_with_redaction(
        tool,
        Reversibility::FullyReversible,
        RedactionPolicy::Fields(vec!["headers".to_owned()]),
    );

    let outcome = dispatch_one(&tools, tool, input, None, None).await;

    let call = &outcome.calls[0];
    assert_eq!(
        call.input["headers"],
        serde_json::json!("[REDACTED]"),
        "declared field redacted in the persisted record"
    );
    assert_eq!(
        call.input["url"],
        serde_json::json!("https://acme.corp/api"),
        "undeclared field passes through"
    );
    assert!(
        call.result
            .as_deref()
            .unwrap_or_default()
            .contains("executed:"),
        "Fields is argument-scoped: the recorded result passes through, got {:?}",
        call.result
    );
}

#[tokio::test]
async fn none_policy_passes_input_and_result_through() {
    let tool = "_test_redaction_none";
    let input = serde_json::json!({"note": "plain value", "count": 2});
    let tools = make_registry_rev(tool, Reversibility::FullyReversible);

    let outcome = dispatch_one(&tools, tool, input.clone(), None, None).await;

    let call = &outcome.calls[0];
    assert_eq!(
        call.input, input,
        "no declaration means no per-tool redaction"
    );
    assert!(
        call.result
            .as_deref()
            .unwrap_or_default()
            .contains("executed:"),
        "result passes through, got {:?}",
        call.result
    );
}

#[tokio::test]
async fn generic_redaction_still_applies_under_none_policy() {
    let tool = "_test_redaction_generic";
    let secret = "abcdefghijklmnopqrstuvwxyz0123456789-token";
    let input = serde_json::json!({"token": secret, "note": "ordinary prose"});
    let tools = make_registry_rev(tool, Reversibility::FullyReversible);

    let outcome = dispatch_one(&tools, tool, input, None, None).await;

    assert_eq!(outcome.calls[0].input["token"], "[REDACTED]");
    assert_eq!(outcome.calls[0].input["note"], "ordinary prose");
}

#[tokio::test]
async fn generic_redaction_does_not_emit_secret_shaped_dynamic_keys() {
    let tool = "_test_redaction_dynamic_key";
    let dynamic_key = "dynamic-token-abcdefghijklmnopqrstuvwxyz0123456789";
    let input = serde_json::json!({(dynamic_key): "ordinary value"});
    let tools = make_registry_rev(tool, Reversibility::FullyReversible);

    let outcome = dispatch_one(&tools, tool, input, None, None).await;

    assert_eq!(
        outcome.calls[0].input,
        serde_json::json!({"__redaction__": "[REDACTED]"})
    );
    assert!(!outcome.calls[0].input.to_string().contains(dynamic_key));
    assert!(
        !format!("{:?}", outcome.events).contains(dynamic_key),
        "runtime data keys must not reappear through event Debug"
    );
}

#[tokio::test]
async fn absent_optional_declared_field_redacts_nothing_else() {
    // A schema-valid optional field may be absent from one concrete payload;
    // that does not widen redaction to unrelated fields.
    let tool = "_test_redaction_absent";
    let input = serde_json::json!({"url": "https://acme.corp", "method": "GET"});
    let tools = registry_with_redaction(
        tool,
        Reversibility::FullyReversible,
        RedactionPolicy::Fields(vec!["heders".to_owned()]),
    );

    let outcome = dispatch_one(&tools, tool, input.clone(), None, None).await;

    assert_eq!(
        outcome.calls[0].input, input,
        "a declared field that matched nothing leaves the payload otherwise untouched"
    );
}

#[tokio::test]
async fn approval_prompt_and_tool_start_carry_redacted_input() {
    let tool = "_test_redaction_approval";
    let input = serde_json::json!({
        "text": "type-this-password",
        "action": "type_text",
    });
    let tools = registry_with_redaction(
        tool,
        Reversibility::Irreversible,
        RedactionPolicy::Fields(vec!["text".to_owned()]),
    );
    let (decision_tx, decision_rx) = mpsc::channel::<ApprovalDecision>(4);
    let gate = ApprovalGate::new(decision_rx, Duration::from_secs(5));
    decision_tx
        .send(ApprovalDecision {
            tool_id: "tool-1".to_owned(),
            choice: ApprovalChoice::Approved,
        })
        .await
        .expect("send approval");

    let outcome = dispatch_one(&tools, tool, input, Some(&gate), None).await;

    let (approval_input, replay_input) = outcome
        .events
        .iter()
        .find_map(|e| match e {
            TurnStreamEvent::ToolApprovalRequired {
                input,
                replay_input,
                ..
            } => Some((input.as_value().clone(), replay_input.clone())),
            _ => None,
        })
        .expect("approval-required event emitted");
    assert_eq!(
        approval_input["text"],
        serde_json::json!("[REDACTED]"),
        "the approval prompt redacts the declared field"
    );
    assert_eq!(
        approval_input["action"],
        serde_json::json!("type_text"),
        "the approval prompt keeps undeclared fields legible"
    );
    assert_eq!(
        replay_input, approval_input,
        "live and replay representations are independently carried even when policy makes them equal"
    );

    let start_input = outcome
        .events
        .iter()
        .find_map(|e| match e {
            TurnStreamEvent::ToolStart { input, .. } => Some(input.clone()),
            _ => None,
        })
        .expect("tool-start event emitted");
    assert_eq!(
        start_input["text"],
        serde_json::json!("[REDACTED]"),
        "the tool-start event carries the same redacted copy"
    );
}

#[tokio::test]
async fn live_approval_uses_prepared_input_while_replay_and_history_keep_placeholder() {
    let tool = "_test_redaction_live_prepared";
    let tools = registry_with_redaction(tool, Reversibility::Irreversible, RedactionPolicy::None);
    let dir = tempfile::TempDir::new().expect("temp workspace");
    std::fs::write(dir.path().join("payload.txt"), "expanded approval prose")
        .expect("write file-ref fixture");
    let mut ctx = test_tool_ctx();
    ctx.workspace = dir.path().to_path_buf();
    ctx.services = Some(tool_services_with_secret("pin", "1234"));
    let placeholder = serde_json::json!({
        "note": "{{file:payload.txt}}",
        "pin": "{{secret:pin}}",
    });
    let (decision_tx, decision_rx) = mpsc::channel::<ApprovalDecision>(4);
    let gate = ApprovalGate::new(decision_rx, Duration::from_secs(5));
    decision_tx
        .send(ApprovalDecision {
            tool_id: "tool-1".to_owned(),
            choice: ApprovalChoice::Approved,
        })
        .await
        .expect("send approval");

    let outcome =
        dispatch_one_with_ctx(&tools, tool, placeholder.clone(), &ctx, Some(&gate), None).await;

    let approval_event = outcome
        .events
        .iter()
        .find(|event| matches!(event, TurnStreamEvent::ToolApprovalRequired { .. }))
        .expect("approval-required event");
    let (live, replay) = match approval_event {
        TurnStreamEvent::ToolApprovalRequired {
            input,
            replay_input,
            ..
        } => (input.as_value(), replay_input),
        _ => unreachable!("matched approval event"),
    };
    assert_eq!(
        live,
        &serde_json::json!({
            "note": "expanded approval prose",
            "pin": "[REDACTED]",
        })
    );
    assert_eq!(replay, &placeholder);
    let debug = format!("{approval_event:?}");
    assert!(debug.contains("LiveApprovalEvidence([REDACTED])"));
    assert!(
        !debug.contains("expanded approval prose"),
        "live-only evidence leaked through Debug: {debug}"
    );

    let start_input = outcome
        .events
        .iter()
        .find_map(|event| match event {
            TurnStreamEvent::ToolStart { input, .. } => Some(input),
            _ => None,
        })
        .expect("tool-start event");
    assert_eq!(start_input, &placeholder);
    assert_eq!(outcome.calls[0].input, placeholder);
}

#[tokio::test]
async fn live_approval_uses_canonical_path_while_replay_keeps_model_path() {
    let tool = "_test_redaction_live_path";
    let mut tools = ToolRegistry::new();
    let mut def = make_tool_def_rev(tool, Reversibility::Irreversible);
    def.input_schema
        .properties
        .insert("path".to_owned(), organon::types::PropertyDef::default());
    tools
        .register(def, Box::new(CanonicalPathEchoExecutor))
        .expect("register path-aware test tool");
    tools
        .declare_capability(
            ToolName::new(tool).expect("valid test tool name"),
            ToolCapabilityMetadata {
                owner: "nous::execute::tests".to_owned(),
                ..ToolCapabilityMetadata::default()
            },
        )
        .expect("declare path-aware test tool");
    let dir = tempfile::TempDir::new().expect("temp workspace");
    let long_segment = "canonical-path-segment-that-exceeds-thirty-two-bytes";
    std::fs::create_dir_all(dir.path().join(long_segment)).expect("create long path fixture");
    std::fs::write(dir.path().join(long_segment).join("target.txt"), "fixture")
        .expect("write path fixture");
    let workspace = dir.path().canonicalize().expect("canonical workspace");
    let canonical = workspace
        .join(long_segment)
        .join("target.txt")
        .to_string_lossy()
        .into_owned();
    assert!(canonical.len() > 32, "exercise the generic token heuristic");
    let mut ctx = test_tool_ctx();
    ctx.workspace = workspace.clone();
    ctx.allowed_roots = vec![workspace];
    let model_input = serde_json::json!({"path": format!("./{long_segment}/target.txt")});
    let (decision_tx, decision_rx) = mpsc::channel::<ApprovalDecision>(4);
    let gate = ApprovalGate::new(decision_rx, Duration::from_secs(5));
    decision_tx
        .send(ApprovalDecision {
            tool_id: "tool-1".to_owned(),
            choice: ApprovalChoice::Approved,
        })
        .await
        .expect("send approval");

    let outcome =
        dispatch_one_with_ctx(&tools, tool, model_input.clone(), &ctx, Some(&gate), None).await;
    let (live, replay) = outcome
        .events
        .iter()
        .find_map(|event| match event {
            TurnStreamEvent::ToolApprovalRequired {
                input,
                replay_input,
                ..
            } => Some((input.as_value(), replay_input)),
            _ => None,
        })
        .expect("approval event");

    assert_eq!(live["path"], canonical);
    let durable = serde_json::json!({"path": "[REDACTED]"});
    assert_eq!(replay, &durable);
    assert_eq!(outcome.calls[0].input, durable);
}

#[tokio::test]
async fn live_approval_preserves_long_declared_url_while_replay_is_generic_redacted() {
    let tool = "_test_redaction_live_long_url";
    let mut tools = ToolRegistry::new();
    let mut def = make_tool_def_rev(tool, Reversibility::Irreversible);
    for field in ["url", "headers", "note"] {
        def.input_schema
            .properties
            .insert(field.to_owned(), organon::types::PropertyDef::default());
    }
    tools
        .register(def, Box::new(EchoExecutor))
        .expect("register URL test tool");
    tools
        .declare_capability(
            ToolName::new(tool).expect("valid test tool name"),
            ToolCapabilityMetadata {
                owner: "nous::execute::tests".to_owned(),
                redaction: RedactionPolicy::Fields(vec!["headers".to_owned()]),
                ..ToolCapabilityMetadata::default()
            },
        )
        .expect("declare URL test capability");
    let url = "https://api.example.test/v1/resources/approval-target-12345";
    assert!(url.len() > 32, "exercise the generic token heuristic");
    let api_key = format!("{}{}", "sk-ant-api03-", "synthetic-approval-key");
    let bearer = "Bearer synthetic.approval.token";
    let jwt = "eyJhbGciOiJIUzI1NiJ9.c3ludGhldGlj.c2lnbmF0dXJl";
    let note = format!("key={api_key}; auth={bearer}; jwt={jwt}");
    let model_input = serde_json::json!({
        "url": url,
        "headers": "private-header",
        "note": note,
    });
    let (decision_tx, decision_rx) = mpsc::channel::<ApprovalDecision>(4);
    let gate = ApprovalGate::new(decision_rx, Duration::from_secs(5));
    decision_tx
        .send(ApprovalDecision {
            tool_id: "tool-1".to_owned(),
            choice: ApprovalChoice::Approved,
        })
        .await
        .expect("send approval");

    let outcome = dispatch_one(&tools, tool, model_input, Some(&gate), None).await;
    let (live, replay) = outcome
        .events
        .iter()
        .find_map(|event| match event {
            TurnStreamEvent::ToolApprovalRequired {
                input,
                replay_input,
                ..
            } => Some((input.as_value(), replay_input)),
            _ => None,
        })
        .expect("approval event");

    assert_eq!(live["url"], url);
    assert_eq!(live["headers"], "[REDACTED]");
    let live_note = live["note"].as_str().expect("live note is text");
    assert!(!live_note.contains(&api_key));
    assert!(!live_note.contains(bearer));
    assert!(!live_note.contains(jwt));
    assert!(live_note.contains("sk-ant-***"));
    assert!(live_note.contains("Bearer ***"));
    assert!(live_note.contains("[JWT REDACTED]"));
    assert_eq!(replay["url"], "[REDACTED]");
    assert_eq!(replay["headers"], "[REDACTED]");
    let replay_note = replay["note"].as_str().expect("replay note is text");
    assert!(!replay_note.contains(&api_key));
    assert!(!replay_note.contains(bearer));
    assert!(!replay_note.contains(jwt));
    assert!(replay_note.contains("sk-ant-***"));
    assert!(replay_note.contains("Bearer ***"));
    assert!(replay_note.contains("[JWT REDACTED]"));
}

#[tokio::test]
async fn full_policy_keeps_computer_use_payload_free_even_for_live_approval() {
    let tool = "computer_use";
    let tools = registry_with_redaction(tool, Reversibility::Irreversible, RedactionPolicy::Full);
    let input = serde_json::json!({
        "action": "type_text",
        "text": "operator-private text",
        "x": 412,
        "y": 97,
    });
    let (decision_tx, decision_rx) = mpsc::channel::<ApprovalDecision>(4);
    let gate = ApprovalGate::new(decision_rx, Duration::from_secs(5));
    decision_tx
        .send(ApprovalDecision {
            tool_id: "tool-1".to_owned(),
            choice: ApprovalChoice::Approved,
        })
        .await
        .expect("send approval");

    let outcome = dispatch_one(&tools, tool, input, Some(&gate), None).await;
    let (live, replay) = outcome
        .events
        .iter()
        .find_map(|event| match event {
            TurnStreamEvent::ToolApprovalRequired {
                input,
                replay_input,
                ..
            } => Some((input.as_value(), replay_input)),
            _ => None,
        })
        .expect("approval-required event");
    let fixed = serde_json::json!({"__redaction__": "[REDACTED]"});
    assert_eq!(live, &fixed);
    assert_eq!(replay, &fixed);
    assert_eq!(outcome.calls[0].input, fixed);
}

#[tokio::test]
async fn saturated_live_stream_defaults_to_deny_without_pre_timeout_blocking() {
    let tool = "_test_redaction_saturated_approval";
    let tools = registry_with_redaction(tool, Reversibility::Irreversible, RedactionPolicy::None);
    let (event_tx, _event_rx) = mpsc::channel::<TurnStreamEvent>(1);
    event_tx
        .try_send(TurnStreamEvent::ToolApprovalResolved {
            tool_id: "filler".to_owned(),
            decision: "filler".to_owned(),
        })
        .expect("fill stream channel");

    let tool_uses = vec![(
        "tool-1".to_owned(),
        tool.to_owned(),
        serde_json::json!({"note": "approval evidence"}),
    )];
    let mut loop_detector = LoopDetector::new(3);
    let mut all_calls = Vec::new();
    let policy = ToolDispatchPolicy::allow_all_for_tests(&tools);
    let tool_ctx = test_tool_ctx();
    let dispatch = dispatch_tools(
        &tool_uses,
        &tools,
        &tool_ctx,
        &mut loop_detector,
        &mut all_calls,
        1,
        Some(&event_tx),
        None,
        &policy,
        0,
        None,
        None,
    );

    tokio::time::timeout(Duration::from_secs(1), dispatch)
        .await
        .expect("saturated approval transport must not block before gate timeout")
        .expect("dispatch records a fail-closed denial");

    assert_eq!(all_calls.len(), 1);
    assert!(all_calls[0].is_error);
    assert_eq!(
        all_calls[0].approval.as_deref(),
        Some("approval_event_unavailable_denied")
    );
    assert_eq!(all_calls[0].duration_ms, 0, "the executor never ran");
}

#[tokio::test]
async fn receipt_ledger_holds_only_the_redacted_copy() {
    let tool = "_test_redaction_receipt";
    let secret = "value the heuristic would keep: has spaces";
    let input = serde_json::json!({"note": secret});
    let tools =
        registry_with_redaction(tool, Reversibility::FullyReversible, RedactionPolicy::Full);
    let ledger = std::sync::Mutex::new(organon::receipts::ReceiptLedger::new());

    let outcome = dispatch_one(&tools, tool, input, None, Some(&ledger)).await;

    let receipt = outcome.calls[0]
        .receipt
        .as_deref()
        .expect("receipt emitted")
        .to_owned();
    let guard = ledger.lock().expect("ledger lock");
    let entry = guard.lookup(&receipt).expect("receipt recorded");
    assert_eq!(
        entry.version,
        organon::receipts::ReceiptVersion::V2,
        "Nous dispatch must emit the prepared-input receipt contract"
    );
    assert!(
        !entry.args_json.contains(secret),
        "ledger args must not hold the sensitive value: {}",
        entry.args_json
    );
    assert!(
        entry.args_json.contains("[REDACTED]"),
        "ledger args carry the redaction marker: {}",
        entry.args_json
    );
    assert_eq!(
        entry.result, "[REDACTED]",
        "ledger result follows the Full policy"
    );
    assert!(
        entry.attestation_v2.is_some(),
        "V2 ledger entry carries session-keyed attestation metadata"
    );
}

#[tokio::test]
async fn receipt_v2_binds_vault_and_file_expanded_input_without_storing_it() {
    let tool = "_test_redaction_expanded_receipt";
    let redaction = RedactionPolicy::Fields(vec![
        "note".to_owned(),
        "auth".to_owned(),
        "path".to_owned(),
    ]);
    let mut tools = ToolRegistry::new();
    let mut def = make_tool_def_rev(tool, Reversibility::FullyReversible);
    for field in ["note", "auth", "path"] {
        def.input_schema
            .properties
            .insert(field.to_owned(), organon::types::PropertyDef::default());
    }
    tools
        .register(def, Box::new(CanonicalPathEchoExecutor))
        .expect("register path-aware test tool");
    tools
        .declare_capability(
            ToolName::new(tool).expect("valid test tool name"),
            ToolCapabilityMetadata {
                owner: "nous::execute::tests".to_owned(),
                redaction: redaction.clone(),
                ..ToolCapabilityMetadata::default()
            },
        )
        .expect("valid path-aware capability declaration");
    let dir = tempfile::TempDir::new().expect("temp workspace");
    std::fs::write(dir.path().join("payload.txt"), "expanded private prose")
        .expect("write file-ref fixture");
    let mut ctx = test_tool_ctx();
    ctx.workspace = dir.path().to_path_buf();
    ctx.services = Some(tool_services_with_secret("token", "short-secret"));
    let placeholder = serde_json::json!({
        "note": "{{file:payload.txt}}",
        "auth": "{{secret:token}}",
        "path": "./payload.txt",
    });
    let canonical_path = dir
        .path()
        .join("payload.txt")
        .canonicalize()
        .expect("canonical fixture path")
        .to_string_lossy()
        .into_owned();
    let expanded = serde_json::json!({
        "note": "expanded private prose",
        "auth": "short-secret",
        "path": canonical_path,
    });
    let ledger = std::sync::Mutex::new(organon::receipts::ReceiptLedger::new());
    let signer = organon::receipts::ReceiptSigner::new_session();

    let outcome = dispatch_one_with_ctx_and_signer(
        &tools,
        tool,
        placeholder.clone(),
        &ctx,
        None,
        Some(&ledger),
        Some(&signer),
    )
    .await;

    let receipt = outcome.calls[0]
        .receipt
        .as_deref()
        .expect("receipt emitted");
    let guard = ledger.lock().expect("ledger lock");
    let entry = guard.lookup(receipt).expect("receipt recorded");
    let attestation = entry.attestation_v2.as_ref().expect("V2 attestation");
    let expected = signer.attest_v2(
        "tool-1",
        tool,
        &expanded,
        &serde_json::json!("executed: _test_redaction_expanded_receipt"),
        "none",
        "auto_approved",
        redaction.clone(),
        entry.ts,
    );
    let placeholder_attestation = signer.attest_v2(
        "tool-1",
        tool,
        &placeholder,
        &serde_json::json!("executed: _test_redaction_expanded_receipt"),
        "none",
        "auto_approved",
        redaction,
        entry.ts,
    );
    assert_eq!(attestation.tool_use_id, "tool-1");
    assert_eq!(attestation.input_commitment, expected.input_commitment);
    assert_ne!(
        attestation.input_commitment, placeholder_attestation.input_commitment,
        "the receipt must not attest the pre-expansion placeholder"
    );
    assert!(!entry.args_json.contains("expanded private prose"));
    assert!(!entry.args_json.contains("short-secret"));
    assert!(!entry.args_json.contains("payload.txt"));
}

#[tokio::test]
async fn denied_call_record_is_redacted_too() {
    let tool = "_test_redaction_denied";
    let input = serde_json::json!({"text": "password-to-type", "action": "type_text"});
    let tools = registry_with_redaction(
        tool,
        Reversibility::Irreversible,
        RedactionPolicy::Fields(vec!["text".to_owned()]),
    );
    let (decision_tx, decision_rx) = mpsc::channel::<ApprovalDecision>(4);
    let gate = ApprovalGate::new(decision_rx, Duration::from_secs(5));
    decision_tx
        .send(ApprovalDecision {
            tool_id: "tool-1".to_owned(),
            choice: ApprovalChoice::Denied,
        })
        .await
        .expect("send denial");

    let outcome = dispatch_one(&tools, tool, input, Some(&gate), None).await;

    assert_eq!(outcome.calls.len(), 1);
    assert!(outcome.calls[0].is_error, "denied call recorded as error");
    assert_eq!(
        outcome.calls[0].input["text"],
        serde_json::json!("[REDACTED]"),
        "a denied call's recorded input follows the same policy"
    );
    assert_eq!(
        outcome.calls[0].input["action"],
        serde_json::json!("type_text"),
        "undeclared fields stay legible on the denial record"
    );
}

#[tokio::test]
async fn durable_result_and_outcome_detail_receive_generic_redaction() {
    let tool = "_test_redaction_outcome";
    let tools = make_registry_with(tool, Box::new(SensitiveFailureExecutor));

    let outcome = dispatch_one(&tools, tool, serde_json::json!({}), None, None).await;

    let call = &outcome.calls[0];
    assert!(call.is_error);
    let recorded_result = call.result.as_deref().expect("recorded result");
    let recorded_detail = call.outcome_detail.as_deref().expect("outcome detail");
    assert!(!recorded_result.contains("outcome-secret"));
    assert!(!recorded_detail.contains("outcome-secret"));
    assert!(recorded_result.contains("password=***"));
    assert_eq!(recorded_detail, "password=***");
    let streamed = outcome
        .events
        .iter()
        .find_map(|event| match event {
            TurnStreamEvent::ToolResult { result, .. } => Some(result.as_str()),
            _ => None,
        })
        .expect("tool result event");
    assert!(!streamed.contains("outcome-secret"));
}
