#![expect(clippy::expect_used, reason = "test assertions")]

use tokio_util::sync::CancellationToken;

use hermeneus::provider::LlmProvider;
use hermeneus::test_utils::MockProvider;
use hermeneus::types::{CompletionRequest, CompletionResponse};

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
        None, // audit_log
        None, // empirical router
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
        None, // audit_log
        None, // empirical router
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
        None, // audit_log
        None, // empirical router
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
    let (dir, oikos) = test_oikos();
    let providers = test_providers();
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
        None, // audit_log
        None, // empirical router
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
        None, // audit_log
        None, // empirical router
    );
    (handle, join, cross_tx, dir)
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
        None, // audit_log
        None, // empirical router
    );
    (handle, join, cross_tx, dir)
}
