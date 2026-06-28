#![expect(clippy::expect_used, reason = "test assertions")]

use tokio_util::sync::CancellationToken;

use hermeneus::provider::LlmProvider;
use hermeneus::test_utils::MockProvider;
use hermeneus::types::{CompletionRequest, CompletionResponse, ContentBlock, StopReason, Usage};

use super::*;

use crate::cross::{CrossNousEnvelope, CrossNousMessage};
use crate::handle::NousHandle;

mod cross_nous;
mod internal_state;
mod lifecycle_and_panic;
mod sessions_and_io;

fn test_config() -> NousConfig {
    NousConfig {
        id: Arc::from("test-agent"),
        generation: crate::config::NousGenerationConfig {
            model: "test-model".to_owned(),
            ..crate::config::NousGenerationConfig::default()
        },
        limits: crate::config::NousLimits {
            consecutive_mistake_limit: 100,
            ..crate::config::NousLimits::default()
        },
        ..NousConfig::default()
    }
}

fn test_config_with_tools(workspace: std::path::PathBuf) -> NousConfig {
    NousConfig {
        workspace: workspace.clone(),
        allowed_roots: vec![workspace],
        tool_groups: organon::types::ToolGroupPolicy::AllowAll {
            reason: "tool limit regression test".to_owned(),
        },
        hooks: crate::config::HookConfig {
            cost_control_enabled: false,
            scope_enforcement_enabled: false,
            correction_hooks_enabled: false,
            audit_logging_enabled: false,
            self_audit_enabled: false,
            working_checkpoint_enabled: false,
            ..crate::config::HookConfig::default()
        },
        ..test_config()
    }
}

fn test_oikos() -> (tempfile::TempDir, Arc<Oikos>) {
    let dir = tempfile::TempDir::new().expect("tmpdir");
    let root = dir.path();
    std::fs::create_dir_all(root.join("nous/test-agent")).expect("mkdir");
    std::fs::create_dir_all(root.join("shared")).expect("mkdir");
    std::fs::create_dir_all(root.join("theke")).expect("mkdir");
    #[expect(
        clippy::disallowed_methods,
        reason = "nous bootstrap and test setup writes configuration files to temp directories; synchronous I/O is required in test contexts"
    )]
    std::fs::write(root.join("nous/test-agent/SOUL.md"), "Test agent.").expect("write");
    let oikos = Arc::new(Oikos::from_root(root));
    (dir, oikos)
}

fn test_providers() -> Arc<ProviderRegistry> {
    let mut providers = ProviderRegistry::new();
    providers.register(Box::new(
        MockProvider::new("Hello from actor!").models(&["test-model"]),
    ));
    Arc::new(providers)
}

/// Mock provider that never completes, so cancellation can race without
/// the pipeline finishing first.
struct PendingProvider;

impl LlmProvider for PendingProvider {
    fn complete<'a>(
        &'a self,
        _request: &'a CompletionRequest,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<Output = hermeneus::error::Result<CompletionResponse>>
                + Send
                + 'a,
        >,
    > {
        Box::pin(std::future::pending())
    }

    fn supported_models(&self) -> &[&str] {
        &["test-model"]
    }

    #[expect(clippy::unnecessary_literal_bound, reason = "trait requires &str")]
    fn name(&self) -> &str {
        "pending-mock"
    }
}

fn pending_providers() -> Arc<ProviderRegistry> {
    let mut providers = ProviderRegistry::new();
    providers.register(Box::new(PendingProvider));
    Arc::new(providers)
}

fn mock_tool_response(
    tool_name: &str,
    tool_id: &str,
    input: serde_json::Value,
) -> CompletionResponse {
    CompletionResponse {
        id: format!("resp-{tool_id}"),
        model: "test-model".to_owned(),
        stop_reason: StopReason::ToolUse,
        content: vec![ContentBlock::ToolUse {
            id: tool_id.to_owned(),
            name: tool_name.to_owned(),
            input,
        }],
        usage: Usage {
            input_tokens: 10,
            output_tokens: 5,
            ..Usage::default()
        },
        cost_usd: None,
        duration_ms: None,
    }
}

fn mock_text_response(text: &str) -> CompletionResponse {
    CompletionResponse {
        id: "resp-final".to_owned(),
        model: "test-model".to_owned(),
        stop_reason: StopReason::EndTurn,
        content: vec![ContentBlock::Text {
            text: text.to_owned(),
            citations: None,
        }],
        usage: Usage {
            input_tokens: 10,
            output_tokens: 5,
            ..Usage::default()
        },
        cost_usd: None,
        duration_ms: None,
    }
}

struct RecordingSequenceProvider {
    responses: std::sync::Mutex<Vec<CompletionResponse>>,
    request_tool_names: std::sync::Mutex<Vec<Vec<String>>>,
}

struct ArcRecordingSequenceProvider(Arc<RecordingSequenceProvider>);

impl RecordingSequenceProvider {
    fn new(responses: Vec<CompletionResponse>) -> Self {
        Self {
            responses: std::sync::Mutex::new(responses),
            request_tool_names: std::sync::Mutex::new(Vec::new()),
        }
    }

    fn request_tool_names(&self) -> Vec<Vec<String>> {
        self.request_tool_names
            .lock()
            .expect("request tool names lock")
            .clone()
    }
}

impl LlmProvider for ArcRecordingSequenceProvider {
    fn complete<'a>(
        &'a self,
        request: &'a CompletionRequest,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<Output = hermeneus::error::Result<CompletionResponse>>
                + Send
                + 'a,
        >,
    > {
        let tool_names = request.tools.iter().map(|tool| tool.name.clone()).collect();
        self.0
            .request_tool_names
            .lock()
            .expect("request tool names lock")
            .push(tool_names);
        let response = self.0.responses.lock().expect("responses lock").remove(0);
        Box::pin(async move { Ok(response) })
    }

    fn supported_models(&self) -> &[&str] {
        &["test-model"]
    }

    #[expect(
        clippy::unnecessary_literal_bound,
        reason = "WHY(#4715): LlmProvider trait requires returning &str"
    )]
    fn name(&self) -> &str {
        "recording-sequence"
    }
}

fn tool_response_provider(tool_name: &str, input: serde_json::Value) -> Arc<ProviderRegistry> {
    let mut providers = ProviderRegistry::new();
    providers.register(Box::new(
        MockProvider::with_responses(vec![
            mock_tool_response(tool_name, "tool-1", input),
            mock_text_response("done"),
        ])
        .models(&["test-model"]),
    ));
    Arc::new(providers)
}

fn builtins_registry() -> Arc<ToolRegistry> {
    let mut tools = ToolRegistry::new();
    organon::builtins::register_all(&mut tools).expect("register builtins");
    Arc::new(tools)
}

struct RecordingMessenger;

impl organon::types::MessageService for RecordingMessenger {
    fn send_message(
        &self,
        _to: &str,
        _text: &str,
        _from_nous: &str,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = std::result::Result<(), String>> + Send + '_>,
    > {
        Box::pin(async { Ok(()) })
    }
}

fn tool_services_with_messenger() -> Arc<organon::types::ToolServices> {
    organon::testing::install_crypto_provider();
    Arc::new(organon::types::ToolServices {
        cross_nous: None,
        messenger: Some(Arc::new(RecordingMessenger)),
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
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .unwrap_or_else(|_| reqwest::Client::new()),
        },
        secret_vault: hermeneus::secret::SecretVault::new(),
        lazy_tool_catalog: Vec::new(),
        server_tool_config: organon::types::ServerToolConfig::default(),
    })
}

async fn run_tool_with_limits(
    tool_name: &str,
    input: serde_json::Value,
    tool_limits: taxis::config::ToolLimitsConfig,
    tool_services: Option<Arc<organon::types::ToolServices>>,
) -> crate::pipeline::TurnResult {
    let (dir, oikos) = test_oikos();
    let config = test_config_with_tools(oikos.nous_dir("test-agent"));
    let (handle, join, _active_turn, _turn_started_at_ms) = spawn(
        config,
        PipelineConfig::default(),
        tool_response_provider(tool_name, input),
        builtins_registry(),
        oikos,
        None,
        None,
        None,
        #[cfg(feature = "knowledge-store")]
        None,
        tool_services,
        Vec::new(),
        None,
        None,
        CancellationToken::new(),
        taxis::config::NousBehaviorConfig::default(),
        Arc::new(tool_limits),
        None,
        None,
        None,
    );

    // WHY: the unified dispatcher fail-closes mandatory-approval tools on
    // gateless turns, so limit tests stream with an auto-approving gate —
    // the limits themselves are what's under test.
    let (event_tx, mut event_rx) = tokio::sync::mpsc::channel(64);
    let (decision_tx, decision_rx) = tokio::sync::mpsc::channel(8);
    let gate = crate::approval::ApprovalGate::new(decision_rx, std::time::Duration::from_secs(10));
    let approver = tokio::spawn(async move {
        while let Some(event) = event_rx.recv().await {
            if let crate::stream::TurnStreamEvent::ToolApprovalRequired { tool_id, .. } = event {
                let _ = decision_tx
                    .send(crate::approval::ApprovalDecision {
                        tool_id,
                        choice: crate::approval::ApprovalChoice::Approved,
                    })
                    .await;
            }
        }
    });
    let result = handle
        .send_turn_streaming_with_approval(
            "main",
            None,
            "run tool",
            event_tx,
            Some(gate),
            std::time::Duration::from_secs(30),
            CancellationToken::new(),
        )
        .await
        .expect("turn");
    handle.shutdown().await.expect("shutdown");
    approver.abort();
    join.await.expect("actor join");
    drop(dir);
    result
}

#[tokio::test]
async fn enable_tool_activation_persists_to_next_turn() {
    let provider = Arc::new(RecordingSequenceProvider::new(vec![
        mock_tool_response(
            "enable_tool",
            "tool-enable",
            serde_json::json!({ "name": "web_fetch" }),
        ),
        mock_text_response("enabled"),
        mock_text_response("still enabled"),
    ]));
    let mut providers = ProviderRegistry::new();
    providers.register(Box::new(ArcRecordingSequenceProvider(Arc::clone(
        &provider,
    ))));

    let (dir, oikos) = test_oikos();
    let config = test_config_with_tools(oikos.nous_dir("test-agent"));
    let (handle, join, _active_turn, _turn_started_at_ms) = spawn(
        config,
        PipelineConfig::default(),
        Arc::new(providers),
        builtins_registry(),
        oikos,
        None,
        None,
        None,
        #[cfg(feature = "knowledge-store")]
        None,
        None,
        Vec::new(),
        None,
        None,
        CancellationToken::new(),
        taxis::config::NousBehaviorConfig::default(),
        Arc::new(taxis::config::ToolLimitsConfig::default()),
        None,
        None,
        None,
    );

    // WHY(#4828): enable_tool is approval-required. This test verifies that an
    // operator-approved activation persists, not that no-gate turns may mutate
    // the active tool surface.
    let (event_tx, mut event_rx) = tokio::sync::mpsc::channel(64);
    let (decision_tx, decision_rx) = tokio::sync::mpsc::channel(8);
    let gate = crate::approval::ApprovalGate::new(decision_rx, std::time::Duration::from_secs(10));
    let approver = tokio::spawn(async move {
        while let Some(event) = event_rx.recv().await {
            if let crate::stream::TurnStreamEvent::ToolApprovalRequired { tool_id, .. } = event {
                let _ = decision_tx
                    .send(crate::approval::ApprovalDecision {
                        tool_id,
                        choice: crate::approval::ApprovalChoice::Approved,
                    })
                    .await;
            }
        }
    });
    let first = handle
        .send_turn_streaming_with_approval(
            "main",
            None,
            "enable web fetch",
            event_tx,
            Some(gate),
            std::time::Duration::from_secs(30),
            CancellationToken::new(),
        )
        .await
        .expect("first turn");
    approver.abort();
    let activation = first.tool_calls.first().expect("enable_tool call");
    assert_eq!(activation.name, "enable_tool");
    assert!(
        !activation.is_error,
        "enable_tool should activate web_fetch: {activation:?}"
    );

    let second = handle
        .send_turn("main", "continue with the same session")
        .await
        .expect("second turn");
    assert_eq!(second.content, "still enabled");

    handle.shutdown().await.expect("shutdown");
    join.await.expect("actor join");
    drop(dir);

    let requests = provider.request_tool_names();
    let [first_request, refreshed_first_turn, second_turn, ..] = requests.as_slice() else {
        panic!("expected at least three LLM requests, got {requests:?}");
    };
    assert!(
        !has_tool(first_request, "web_fetch"),
        "web_fetch should start inactive: {first_request:?}"
    );
    assert!(
        has_tool(refreshed_first_turn, "web_fetch"),
        "web_fetch should be active after enable_tool in the first turn: {refreshed_first_turn:?}"
    );
    assert!(
        has_tool(second_turn, "web_fetch"),
        "web_fetch activation must persist into the next turn: {second_turn:?}"
    );
}

fn has_tool(names: &[String], expected: &str) -> bool {
    names.iter().any(|name| name == expected)
}

#[tokio::test]
async fn configured_max_write_bytes_limits_workspace_write_tool() {
    let tool_limits = taxis::config::ToolLimitsConfig {
        max_write_bytes: 5,
        ..taxis::config::ToolLimitsConfig::default()
    };
    let result = run_tool_with_limits(
        "write",
        serde_json::json!({
            "path": "limited.txt",
            "content": "abcdef"
        }),
        tool_limits,
        None,
    )
    .await;

    let call = result.tool_calls.first().expect("tool call");
    let output = call.result.as_deref().expect("tool output");
    assert!(
        call.is_error,
        "write should be rejected by configured limit"
    );
    assert!(
        output.contains("max 5 bytes"),
        "write output should mention configured limit: {output}"
    );
}

#[tokio::test]
async fn configured_subprocess_timeout_limits_workspace_exec_tool() {
    let tool_limits = taxis::config::ToolLimitsConfig {
        subprocess_timeout_secs: 1,
        ..taxis::config::ToolLimitsConfig::default()
    };
    let result = run_tool_with_limits(
        "exec",
        serde_json::json!({
            "command": "sleep 2",
            "timeout": 60_000
        }),
        tool_limits,
        None,
    )
    .await;

    let call = result.tool_calls.first().expect("tool call");
    let output = call.result.as_deref().expect("tool output");
    assert!(call.is_error, "exec should time out at configured cap");
    assert!(
        output.contains("timed out after 1000ms"),
        "exec output should mention configured timeout: {output}"
    );
}

#[tokio::test]
async fn configured_message_max_len_limits_message_tool() {
    let tool_limits = taxis::config::ToolLimitsConfig {
        message_max_len: 5,
        ..taxis::config::ToolLimitsConfig::default()
    };
    let result = run_tool_with_limits(
        "message",
        serde_json::json!({
            "to": "u:alice",
            "text": "abcdef"
        }),
        tool_limits,
        Some(tool_services_with_messenger()),
    )
    .await;

    let call = result.tool_calls.first().expect("tool call");
    let output = call.result.as_deref().expect("tool output");
    assert!(
        call.is_error,
        "message should be rejected by configured limit"
    );
    assert!(
        output.contains("5 character limit"),
        "message output should mention configured limit: {output}"
    );
}

#[tokio::test]
async fn reload_config_updates_tool_limits_for_existing_actor() {
    let (dir, oikos) = test_oikos();
    let config = test_config_with_tools(oikos.nous_dir("test-agent"));
    let (handle, join, _active_turn, _turn_started_at_ms) = spawn(
        config.clone(),
        PipelineConfig::default(),
        tool_response_provider(
            "write",
            serde_json::json!({
                "path": "reloaded.txt",
                "content": "abcdef"
            }),
        ),
        builtins_registry(),
        oikos,
        None,
        None,
        None,
        #[cfg(feature = "knowledge-store")]
        None,
        None,
        Vec::new(),
        None,
        None,
        CancellationToken::new(),
        taxis::config::NousBehaviorConfig::default(),
        Arc::new(taxis::config::ToolLimitsConfig::default()),
        None,
        None,
        None,
    );
    let tool_limits = taxis::config::ToolLimitsConfig {
        max_write_bytes: 5,
        ..taxis::config::ToolLimitsConfig::default()
    };
    handle
        .reload_config(
            config,
            PipelineConfig::default(),
            tool_limits,
            crate::handle::DEFAULT_SEND_TIMEOUT,
        )
        .await
        .expect("reload");

    // WHY: the unified dispatcher fail-closes mandatory-approval tools on
    // gateless turns, so limit tests stream with an auto-approving gate —
    // the limits themselves are what's under test.
    let (event_tx, mut event_rx) = tokio::sync::mpsc::channel(64);
    let (decision_tx, decision_rx) = tokio::sync::mpsc::channel(8);
    let gate = crate::approval::ApprovalGate::new(decision_rx, std::time::Duration::from_secs(10));
    let approver = tokio::spawn(async move {
        while let Some(event) = event_rx.recv().await {
            if let crate::stream::TurnStreamEvent::ToolApprovalRequired { tool_id, .. } = event {
                let _ = decision_tx
                    .send(crate::approval::ApprovalDecision {
                        tool_id,
                        choice: crate::approval::ApprovalChoice::Approved,
                    })
                    .await;
            }
        }
    });
    let result = handle
        .send_turn_streaming_with_approval(
            "main",
            None,
            "run tool",
            event_tx,
            Some(gate),
            std::time::Duration::from_secs(30),
            CancellationToken::new(),
        )
        .await
        .expect("turn");
    handle.shutdown().await.expect("shutdown");
    approver.abort();
    join.await.expect("actor join");
    drop(dir);

    let call = result.tool_calls.first().expect("tool call");
    let output = call.result.as_deref().expect("tool output");
    assert!(
        output.contains("max 5 bytes"),
        "reloaded write output should mention configured limit: {output}"
    );
}

fn spawn_test_actor() -> (NousHandle, tokio::task::JoinHandle<()>, tempfile::TempDir) {
    let (dir, oikos) = test_oikos();
    let providers = test_providers();
    let tools = Arc::new(ToolRegistry::new());
    let config = test_config();
    let pipeline_config = PipelineConfig::default();

    let (handle, join, _active_turn, _turn_started_at_ms) = spawn(
        config,
        pipeline_config,
        providers,
        tools,
        oikos,
        None,
        None,
        None,
        #[cfg(feature = "knowledge-store")]
        None,
        None,
        Vec::new(),
        None,
        None,
        CancellationToken::new(),
        taxis::config::NousBehaviorConfig::default(),
        Arc::new(taxis::config::ToolLimitsConfig::default()),
        None, // audit_log
        None, // empirical router
        None, // cross_router
    );
    (handle, join, dir)
}

/// Mock provider that panics on every call.
struct PanickingProvider;

impl LlmProvider for PanickingProvider {
    fn complete<'a>(
        &'a self,
        _request: &'a CompletionRequest,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<Output = hermeneus::error::Result<CompletionResponse>>
                + Send
                + 'a,
        >,
    > {
        Box::pin(async { panic!("deliberate test panic in pipeline") })
    }

    fn supported_models(&self) -> &[&str] {
        &["test-model"]
    }

    #[expect(clippy::unnecessary_literal_bound, reason = "trait requires &str")]
    fn name(&self) -> &str {
        "panicking-mock"
    }
}

fn panicking_providers() -> Arc<ProviderRegistry> {
    let mut providers = ProviderRegistry::new();
    providers.register(Box::new(PanickingProvider));
    Arc::new(providers)
}

fn spawn_panicking_actor() -> (NousHandle, tokio::task::JoinHandle<()>, tempfile::TempDir) {
    let (dir, oikos) = test_oikos();
    let providers = panicking_providers();
    let tools = Arc::new(ToolRegistry::new());
    let config = test_config();
    let pipeline_config = PipelineConfig::default();

    let (handle, join, _active_turn, _turn_started_at_ms) = spawn(
        config,
        pipeline_config,
        providers,
        tools,
        oikos,
        None,
        None,
        None,
        #[cfg(feature = "knowledge-store")]
        None,
        None,
        Vec::new(),
        None,
        None,
        CancellationToken::new(),
        taxis::config::NousBehaviorConfig::default(),
        Arc::new(taxis::config::ToolLimitsConfig::default()),
        None, // audit_log
        None, // empirical router
        None, // cross_router
    );
    (handle, join, dir)
}

fn spawn_test_actor_with_store(
    store: Arc<tokio::sync::Mutex<mneme::store::SessionStore>>,
) -> (NousHandle, tokio::task::JoinHandle<()>, tempfile::TempDir) {
    let (dir, oikos) = test_oikos();
    let providers = test_providers();
    let tools = Arc::new(ToolRegistry::new());
    let config = test_config();
    let pipeline_config = PipelineConfig::default();

    let (handle, join, _active_turn, _turn_started_at_ms) = spawn(
        config,
        pipeline_config,
        providers,
        tools,
        oikos,
        None,
        None,
        Some(store),
        #[cfg(feature = "knowledge-store")]
        None,
        None,
        Vec::new(),
        None,
        None,
        CancellationToken::new(),
        taxis::config::NousBehaviorConfig::default(),
        Arc::new(taxis::config::ToolLimitsConfig::default()),
        None, // audit_log
        None, // empirical router
        None, // cross_router
    );
    (handle, join, dir)
}

// ── Direct actor construction helper ─────────────────────────────────────────

/// Build a bare `NousActor` for unit tests that exercise internal state
/// without running the actor loop. Returns the actor and the inbox sender
/// (kept alive so the receiver does not close).
fn make_test_actor(
    pipeline_config: PipelineConfig,
) -> (
    NousActor,
    mpsc::Sender<NousMessage>,
    tempfile::TempDir, // kept alive: drops would delete tempdir
) {
    make_test_actor_with_providers(test_providers(), pipeline_config)
}

fn make_test_actor_with_providers(
    providers: Arc<ProviderRegistry>,
    pipeline_config: PipelineConfig,
) -> (
    NousActor,
    mpsc::Sender<NousMessage>,
    tempfile::TempDir, // kept alive: drops would delete tempdir
) {
    let (dir, oikos) = test_oikos();
    let tools = Arc::new(ToolRegistry::new());
    let config = test_config();
    let (tx, rx) = mpsc::channel(DEFAULT_INBOX_CAPACITY);
    let active_turn = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let turn_started_at_ms = Arc::new(std::sync::atomic::AtomicU64::new(0));
    let actor = NousActor::new(
        "test-agent".to_owned(),
        config,
        pipeline_config,
        rx,
        None,
        None,
        CancellationToken::new(),
        providers,
        tools,
        oikos,
        None,
        None,
        None,
        #[cfg(feature = "knowledge-store")]
        None,
        None,
        Vec::new(),
        active_turn,
        turn_started_at_ms,
        taxis::config::NousBehaviorConfig::default(),
        Arc::new(taxis::config::ToolLimitsConfig::default()),
        None, // audit_log
        None, // empirical router
        None, // cross_router
    );
    (actor, tx, dir)
}

// ── turn.rs: record_drift_metrics ────────────────────────────────────────────

fn make_turn_result(
    output_tokens: u64,
    tool_calls: Vec<crate::pipeline::ToolCall>,
) -> crate::pipeline::TurnResult {
    crate::pipeline::TurnResult {
        content: "ok".to_owned(),
        tool_calls,
        usage: crate::pipeline::TurnUsage {
            input_tokens: 10,
            output_tokens,
            cache_read_tokens: 0,
            cache_write_tokens: 0,
            llm_calls: 1,
        },
        signals: vec![],
        stop_reason: "end_turn".to_owned(),
        degraded: None,
        reasoning: String::new(),
        model_used: "test-model".to_owned(),
        tool_surface_hashes: Vec::new(),
    }
}

fn make_tool_call(name: &str, is_error: bool) -> crate::pipeline::ToolCall {
    make_tool_call_with_result(name, is_error, None)
}

fn make_tool_call_with_result(
    name: &str,
    is_error: bool,
    result: Option<&str>,
) -> crate::pipeline::ToolCall {
    crate::pipeline::ToolCall {
        id: "tc-1".to_owned(),
        name: name.to_owned(),
        input: serde_json::Value::Null,
        result: result.map(str::to_owned),
        is_error,
        duration_ms: 10,
        approval: None,
        receipt: None,
    }
}

fn spawn_test_actor_with_cross() -> (
    NousHandle,
    tokio::task::JoinHandle<()>,
    tokio::sync::mpsc::Sender<CrossNousEnvelope>,
    tempfile::TempDir,
) {
    let (dir, oikos) = test_oikos();
    let providers = test_providers();
    let tools = Arc::new(ToolRegistry::new());
    let config = test_config();
    let pipeline_config = PipelineConfig::default();

    let (cross_tx, cross_rx) = tokio::sync::mpsc::channel(32);

    let (handle, join, _active_turn, _turn_started_at_ms) = spawn(
        config,
        pipeline_config,
        providers,
        tools,
        oikos,
        None,
        None,
        None,
        #[cfg(feature = "knowledge-store")]
        None,
        None,
        Vec::new(),
        Some(cross_rx),
        Some(cross_tx.clone()),
        CancellationToken::new(),
        taxis::config::NousBehaviorConfig::default(),
        Arc::new(taxis::config::ToolLimitsConfig::default()),
        None, // audit_log
        None, // empirical router
        None, // cross_router
    );
    (handle, join, cross_tx, dir)
}

async fn spawn_test_actor_in_router(
    router: Arc<crate::cross::CrossNousRouter>,
) -> (NousHandle, tokio::task::JoinHandle<()>, tempfile::TempDir) {
    let (dir, oikos) = test_oikos();
    let providers = test_providers();
    let tools = Arc::new(ToolRegistry::new());
    let config = test_config();
    let pipeline_config = PipelineConfig::default();

    let (cross_tx, cross_rx) = tokio::sync::mpsc::channel(32);
    router
        .register(config.id.to_string(), cross_tx.clone())
        .await;

    let (handle, join, _active_turn, _turn_started_at_ms) = spawn(
        config,
        pipeline_config,
        providers,
        tools,
        oikos,
        None,
        None,
        None,
        #[cfg(feature = "knowledge-store")]
        None,
        None,
        Vec::new(),
        Some(cross_rx),
        Some(cross_tx),
        CancellationToken::new(),
        taxis::config::NousBehaviorConfig::default(),
        Arc::new(taxis::config::ToolLimitsConfig::default()),
        None, // audit_log
        None, // empirical router
        Some(router),
    );
    (handle, join, dir)
}

fn spawn_test_actor_with_router(
    router: Option<Arc<dyn aletheia_routing::Router>>,
) -> (NousHandle, tokio::task::JoinHandle<()>, tempfile::TempDir) {
    let (dir, oikos) = test_oikos();
    let providers = test_providers();
    let tools = Arc::new(ToolRegistry::new());
    let config = test_config();
    let pipeline_config = PipelineConfig::default();

    let (handle, join, _active_turn, _turn_started_at_ms) = spawn(
        config,
        pipeline_config,
        providers,
        tools,
        oikos,
        None,
        None,
        None,
        #[cfg(feature = "knowledge-store")]
        None,
        None,
        Vec::new(),
        None,
        None,
        CancellationToken::new(),
        taxis::config::NousBehaviorConfig::default(),
        Arc::new(taxis::config::ToolLimitsConfig::default()),
        None, // audit_log
        router,
        None, // cross_router
    );
    (handle, join, dir)
}

fn spawn_panicking_actor_with_cross() -> (
    NousHandle,
    tokio::task::JoinHandle<()>,
    tokio::sync::mpsc::Sender<CrossNousEnvelope>,
    tempfile::TempDir,
) {
    let (dir, oikos) = test_oikos();
    let providers = panicking_providers();
    let tools = Arc::new(ToolRegistry::new());
    let config = test_config();
    let pipeline_config = PipelineConfig::default();

    let (cross_tx, cross_rx) = tokio::sync::mpsc::channel(32);

    let (handle, join, _active_turn, _turn_started_at_ms) = spawn(
        config,
        pipeline_config,
        providers,
        tools,
        oikos,
        None,
        None,
        None,
        #[cfg(feature = "knowledge-store")]
        None,
        None,
        Vec::new(),
        Some(cross_rx),
        Some(cross_tx.clone()),
        CancellationToken::new(),
        taxis::config::NousBehaviorConfig::default(),
        Arc::new(taxis::config::ToolLimitsConfig::default()),
        None, // audit_log
        None, // empirical router
        None, // cross_router
    );
    (handle, join, cross_tx, dir)
}
