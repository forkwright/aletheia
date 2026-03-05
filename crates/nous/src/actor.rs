//! Tokio actor for a single nous agent instance.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use aletheia_mneme::store::SessionStore;

use tokio::sync::mpsc;
use tracing::{Instrument, debug, info, instrument, warn};

use crate::cross::CrossNousEnvelope;

use aletheia_hermeneus::provider::ProviderRegistry;
use aletheia_koina::id::{NousId, SessionId};
use aletheia_mneme::embedding::EmbeddingProvider;
use aletheia_organon::registry::ToolRegistry;
use aletheia_organon::types::{ToolContext, ToolServices};
use aletheia_taxis::oikos::Oikos;

use crate::bootstrap::BootstrapSection;
use crate::config::{NousConfig, PipelineConfig};
use crate::handle::NousHandle;
use crate::message::{NousLifecycle, NousMessage, NousStatus};
use crate::pipeline::TurnResult;
use crate::session::SessionState;

/// Default bounded channel capacity for the actor inbox.
pub const DEFAULT_INBOX_CAPACITY: usize = 32;

/// A single nous agent running as a Tokio actor.
///
/// Each actor owns its mutable state and processes messages sequentially
/// from a bounded inbox. External code interacts via [`NousHandle`].
pub struct NousActor {
    id: String,
    config: NousConfig,
    pipeline_config: PipelineConfig,
    inbox: mpsc::Receiver<NousMessage>,
    cross_rx: Option<mpsc::Receiver<CrossNousEnvelope>>,
    lifecycle: NousLifecycle,
    sessions: HashMap<String, SessionState>,
    active_session: Option<String>,
    providers: Arc<ProviderRegistry>,
    tools: Arc<ToolRegistry>,
    oikos: Arc<Oikos>,
    embedding_provider: Option<Arc<dyn EmbeddingProvider>>,
    vector_search: Option<Arc<dyn crate::recall::VectorSearch>>,
    session_store: Option<Arc<Mutex<SessionStore>>>,
    tool_services: Option<Arc<ToolServices>>,
    extra_bootstrap: Vec<BootstrapSection>,
}

impl NousActor {
    /// Create a new actor. Use [`NousManager::spawn`](crate::manager::NousManager::spawn)
    /// or [`spawn`] to start it.
    #[expect(
        clippy::too_many_arguments,
        reason = "actor requires all runtime dependencies"
    )]
    pub(crate) fn new(
        id: String,
        config: NousConfig,
        pipeline_config: PipelineConfig,
        inbox: mpsc::Receiver<NousMessage>,
        cross_rx: Option<mpsc::Receiver<CrossNousEnvelope>>,
        providers: Arc<ProviderRegistry>,
        tools: Arc<ToolRegistry>,
        oikos: Arc<Oikos>,
        embedding_provider: Option<Arc<dyn EmbeddingProvider>>,
        vector_search: Option<Arc<dyn crate::recall::VectorSearch>>,
        session_store: Option<Arc<Mutex<SessionStore>>>,
        tool_services: Option<Arc<ToolServices>>,
        extra_bootstrap: Vec<BootstrapSection>,
    ) -> Self {
        Self {
            id,
            config,
            pipeline_config,
            inbox,
            cross_rx,
            lifecycle: NousLifecycle::Idle,
            sessions: HashMap::new(),
            active_session: None,
            providers,
            tools,
            oikos,
            embedding_provider,
            vector_search,
            session_store,
            tool_services,
            extra_bootstrap,
        }
    }

    /// Run the actor loop until shutdown or all handles are dropped.
    #[instrument(skip(self), fields(nous.id = %self.id))]
    pub async fn run(mut self) {
        info!(lifecycle = %self.lifecycle, "actor started");

        loop {
            tokio::select! {
                msg = self.inbox.recv() => {
                    let Some(msg) = msg else { break };
                    match msg {
                        NousMessage::Turn {
                            session_key,
                            content,
                            reply,
                        } => {
                            self.handle_turn(session_key, content, reply).await;
                        }
                        NousMessage::Status { reply } => {
                            self.handle_status(reply);
                        }
                        NousMessage::Sleep => {
                            self.handle_sleep();
                        }
                        NousMessage::Wake => {
                            self.handle_wake();
                        }
                        NousMessage::Shutdown => {
                            info!("shutdown requested");
                            break;
                        }
                    }
                }
                envelope = async {
                    match self.cross_rx.as_mut() {
                        Some(rx) => rx.recv().await,
                        None => std::future::pending().await,
                    }
                } => {
                    if let Some(envelope) = envelope {
                        self.handle_cross_message(envelope).await;
                    }
                }
            }
        }

        info!(lifecycle = %self.lifecycle, "actor stopped");
    }

    async fn handle_cross_message(&mut self, envelope: CrossNousEnvelope) {
        let from = &envelope.message.from;
        let session_key = format!("cross:{from}");
        let content = envelope.message.content.clone();

        debug!(from = %from, session_key = %session_key, "processing cross-nous message");

        match self.execute_turn(&session_key, &content).await {
            Ok(result) => {
                debug!(from = %from, content_len = result.content.len(), "cross-nous turn completed");
            }
            Err(e) => {
                warn!(from = %from, error = %e, "cross-nous turn failed");
            }
        }
    }

    async fn handle_turn(
        &mut self,
        session_key: String,
        content: String,
        reply: tokio::sync::oneshot::Sender<crate::error::Result<TurnResult>>,
    ) {
        if self.lifecycle == NousLifecycle::Dormant {
            debug!("auto-waking from dormant for turn");
            self.lifecycle = NousLifecycle::Idle;
        }

        self.lifecycle = NousLifecycle::Active;
        self.active_session = Some(session_key.clone());

        let result = self.execute_turn(&session_key, &content).await;

        if let Ok(ref turn_result) = result {
            self.maybe_spawn_extraction(&content, &turn_result.content);
        }

        self.active_session = None;
        self.lifecycle = NousLifecycle::Idle;

        // Ignore send error — caller may have dropped the receiver
        let _ = reply.send(result);
    }

    async fn execute_turn(
        &mut self,
        session_key: &str,
        content: &str,
    ) -> crate::error::Result<TurnResult> {
        let session = self
            .sessions
            .entry(session_key.to_owned())
            .or_insert_with(|| {
                let id = SessionId::new().to_string();
                debug!(session_key, session_id = %id, "creating new session");
                SessionState::new(id, session_key.to_owned(), &self.config)
            });

        session.next_turn();

        let input = crate::pipeline::PipelineInput {
            content: content.to_owned(),
            session: session.clone(),
            config: self.pipeline_config.clone(),
        };

        let nous_id = NousId::new(&self.id).map_err(|e| {
            crate::error::ConfigSnafu {
                message: format!("invalid nous id: {e}"),
            }
            .build()
        })?;

        let tool_ctx = ToolContext {
            nous_id,
            session_id: SessionId::new(),
            workspace: self.oikos.nous_dir(&self.id),
            allowed_roots: vec![self.oikos.root().to_path_buf()],
            services: self.tool_services.clone(),
        };

        crate::pipeline::run_pipeline(
            input,
            &self.oikos,
            &self.config,
            &self.pipeline_config,
            &self.providers,
            &self.tools,
            &tool_ctx,
            self.embedding_provider.as_deref(),
            self.vector_search.as_deref(),
            self.session_store.as_deref(),
            self.extra_bootstrap.clone(),
        )
        .await
    }

    fn handle_status(&self, reply: tokio::sync::oneshot::Sender<NousStatus>) {
        let status = NousStatus {
            id: self.id.clone(),
            lifecycle: self.lifecycle,
            session_count: self.sessions.len(),
            active_session: self.active_session.clone(),
        };
        let _ = reply.send(status);
    }

    fn maybe_spawn_extraction(&self, user_content: &str, assistant_content: &str) {
        let Some(ref extraction_config) = self.pipeline_config.extraction else {
            return;
        };
        if !extraction_config.enabled {
            return;
        }

        let content_len = user_content.len() + assistant_content.len();
        if content_len < extraction_config.min_message_length {
            return;
        }

        let config = extraction_config.clone();
        let providers = Arc::clone(&self.providers);
        let nous_id = self.id.clone();
        let user = user_content.to_owned();
        let assistant = assistant_content.to_owned();
        let span = tracing::info_span!("extraction", nous.id = %nous_id);

        tokio::spawn(
            async move {
                run_extraction(&config, providers, &nous_id, &user, &assistant);
            }
            .instrument(span),
        );
    }

    fn handle_sleep(&mut self) {
        match self.lifecycle {
            NousLifecycle::Idle => {
                debug!("transitioning to dormant");
                self.lifecycle = NousLifecycle::Dormant;
            }
            NousLifecycle::Active => {
                warn!("cannot sleep while active, ignoring");
            }
            NousLifecycle::Dormant => {
                debug!("already dormant");
            }
        }
    }

    fn handle_wake(&mut self) {
        match self.lifecycle {
            NousLifecycle::Dormant => {
                debug!("waking from dormant");
                self.lifecycle = NousLifecycle::Idle;
            }
            NousLifecycle::Idle | NousLifecycle::Active => {
                debug!(lifecycle = %self.lifecycle, "already awake");
            }
        }
    }
}

/// Run extraction as a background task. Logs results, never panics.
fn run_extraction(
    config: &aletheia_mneme::extract::ExtractionConfig,
    providers: Arc<ProviderRegistry>,
    nous_id: &str,
    user_content: &str,
    assistant_content: &str,
) {
    use aletheia_mneme::extract::{ConversationMessage, ExtractionEngine};

    let engine = ExtractionEngine::new(config.clone());
    let provider = crate::extraction::HermeneusExtractionProvider::new(providers, &config.model);

    let messages = vec![
        ConversationMessage {
            role: "user".to_owned(),
            content: user_content.to_owned(),
        },
        ConversationMessage {
            role: "assistant".to_owned(),
            content: assistant_content.to_owned(),
        },
    ];

    match engine.extract(&messages, &provider) {
        Ok(extraction) => {
            info!(
                nous_id = %nous_id,
                entities = extraction.entities.len(),
                relationships = extraction.relationships.len(),
                facts = extraction.facts.len(),
                "extraction completed"
            );
        }
        Err(e) => {
            warn!(nous_id = %nous_id, error = %e, "extraction failed");
        }
    }
}

/// Spawn a nous actor, returning its handle and join handle.
///
/// Creates a bounded channel with [`DEFAULT_INBOX_CAPACITY`], builds the actor,
/// and starts it on the Tokio runtime.
#[expect(
    clippy::too_many_arguments,
    reason = "actor spawn requires all runtime dependencies"
)]
pub fn spawn(
    config: NousConfig,
    pipeline_config: PipelineConfig,
    providers: Arc<ProviderRegistry>,
    tools: Arc<ToolRegistry>,
    oikos: Arc<Oikos>,
    embedding_provider: Option<Arc<dyn EmbeddingProvider>>,
    vector_search: Option<Arc<dyn crate::recall::VectorSearch>>,
    session_store: Option<Arc<Mutex<SessionStore>>>,
    tool_services: Option<Arc<aletheia_organon::types::ToolServices>>,
    extra_bootstrap: Vec<BootstrapSection>,
    cross_rx: Option<mpsc::Receiver<CrossNousEnvelope>>,
) -> (NousHandle, tokio::task::JoinHandle<()>) {
    let (tx, rx) = mpsc::channel(DEFAULT_INBOX_CAPACITY);
    let id = config.id.clone();
    let handle = NousHandle::new(id.clone(), tx);

    let actor = NousActor::new(
        id.clone(),
        config,
        pipeline_config,
        rx,
        cross_rx,
        providers,
        tools,
        oikos,
        embedding_provider,
        vector_search,
        session_store,
        tool_services,
        extra_bootstrap,
    );

    let span = tracing::info_span!("nous_actor", nous.id = %id);
    let join_handle = tokio::spawn(async move { actor.run().await }.instrument(span));

    (handle, join_handle)
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use aletheia_hermeneus::provider::LlmProvider;
    use aletheia_hermeneus::types::{
        CompletionRequest, CompletionResponse, ContentBlock, StopReason, Usage,
    };

    use super::*;

    // --- Test infrastructure ---

    struct MockProvider {
        response: Mutex<CompletionResponse>,
    }

    impl LlmProvider for MockProvider {
        fn complete(
            &self,
            _request: &CompletionRequest,
        ) -> aletheia_hermeneus::error::Result<CompletionResponse> {
            Ok(self.response.lock().expect("lock").clone())
        }

        fn supported_models(&self) -> &[&str] {
            &["test-model"]
        }

        #[expect(clippy::unnecessary_literal_bound, reason = "trait requires &str")]
        fn name(&self) -> &str {
            "mock"
        }
    }

    fn test_config() -> NousConfig {
        NousConfig {
            id: "test-agent".to_owned(),
            model: "test-model".to_owned(),
            ..NousConfig::default()
        }
    }

    fn test_oikos() -> (tempfile::TempDir, Arc<Oikos>) {
        let dir = tempfile::TempDir::new().expect("tmpdir");
        let root = dir.path();
        std::fs::create_dir_all(root.join("nous/test-agent")).expect("mkdir");
        std::fs::create_dir_all(root.join("shared")).expect("mkdir");
        std::fs::create_dir_all(root.join("theke")).expect("mkdir");
        std::fs::write(root.join("nous/test-agent/SOUL.md"), "Test agent.").expect("write");
        let oikos = Arc::new(Oikos::from_root(root));
        (dir, oikos)
    }

    fn test_providers() -> Arc<ProviderRegistry> {
        let mut providers = ProviderRegistry::new();
        providers.register(Box::new(MockProvider {
            response: Mutex::new(CompletionResponse {
                id: "resp-1".to_owned(),
                model: "test-model".to_owned(),
                stop_reason: StopReason::EndTurn,
                content: vec![ContentBlock::Text {
                    text: "Hello from actor!".to_owned(),
                    citations: None,
                }],
                usage: Usage {
                    input_tokens: 100,
                    output_tokens: 50,
                    ..Usage::default()
                },
            }),
        }));
        Arc::new(providers)
    }

    fn spawn_test_actor() -> (NousHandle, tokio::task::JoinHandle<()>, tempfile::TempDir) {
        let (dir, oikos) = test_oikos();
        let providers = test_providers();
        let tools = Arc::new(ToolRegistry::new());
        let config = test_config();
        let pipeline_config = PipelineConfig::default();

        let (handle, join) = spawn(
            config,
            pipeline_config,
            providers,
            tools,
            oikos,
            None,
            None,
            None,
            None,
            Vec::new(),
            None,
        );
        (handle, join, dir)
    }

    // --- Tests ---

    #[tokio::test]
    async fn turn_processes_and_returns_result() {
        let (handle, join, _dir) = spawn_test_actor();

        let result = handle.send_turn("main", "Hello").await.expect("turn");
        assert_eq!(result.content, "Hello from actor!");
        assert_eq!(result.usage.llm_calls, 1);

        handle.shutdown().await.expect("shutdown");
        join.await.expect("join");
    }

    #[tokio::test]
    async fn status_returns_snapshot() {
        let (handle, join, _dir) = spawn_test_actor();

        let status = handle.status().await.expect("status");
        assert_eq!(status.id, "test-agent");
        assert_eq!(status.lifecycle, NousLifecycle::Idle);
        assert_eq!(status.session_count, 0);
        assert!(status.active_session.is_none());

        handle.shutdown().await.expect("shutdown");
        join.await.expect("join");
    }

    #[tokio::test]
    async fn sleep_transitions_to_dormant() {
        let (handle, join, _dir) = spawn_test_actor();

        handle.sleep().await.expect("sleep");
        let status = handle.status().await.expect("status");
        assert_eq!(status.lifecycle, NousLifecycle::Dormant);

        handle.shutdown().await.expect("shutdown");
        join.await.expect("join");
    }

    #[tokio::test]
    async fn wake_transitions_to_idle() {
        let (handle, join, _dir) = spawn_test_actor();

        handle.sleep().await.expect("sleep");
        handle.wake().await.expect("wake");
        let status = handle.status().await.expect("status");
        assert_eq!(status.lifecycle, NousLifecycle::Idle);

        handle.shutdown().await.expect("shutdown");
        join.await.expect("join");
    }

    #[tokio::test]
    async fn dormant_auto_wakes_on_turn() {
        let (handle, join, _dir) = spawn_test_actor();

        handle.sleep().await.expect("sleep");
        let status = handle.status().await.expect("status");
        assert_eq!(status.lifecycle, NousLifecycle::Dormant);

        let result = handle.send_turn("main", "Wake up").await.expect("turn");
        assert_eq!(result.content, "Hello from actor!");

        let status = handle.status().await.expect("status");
        assert_eq!(status.lifecycle, NousLifecycle::Idle);

        handle.shutdown().await.expect("shutdown");
        join.await.expect("join");
    }

    #[tokio::test]
    async fn shutdown_exits_loop() {
        let (handle, join, _dir) = spawn_test_actor();

        handle.shutdown().await.expect("shutdown");
        join.await.expect("join");
    }

    #[tokio::test]
    async fn actor_exits_when_all_handles_dropped() {
        let (handle, join, _dir) = spawn_test_actor();
        drop(handle);
        join.await.expect("join");
    }

    #[tokio::test]
    async fn multiple_sequential_turns() {
        let (handle, join, _dir) = spawn_test_actor();

        for i in 0..5 {
            let result = handle
                .send_turn("main", format!("Turn {i}"))
                .await
                .expect("turn");
            assert_eq!(result.content, "Hello from actor!");
        }

        let status = handle.status().await.expect("status");
        assert_eq!(status.session_count, 1);

        handle.shutdown().await.expect("shutdown");
        join.await.expect("join");
    }

    #[tokio::test]
    async fn turn_creates_session_for_unknown_key() {
        let (handle, join, _dir) = spawn_test_actor();

        handle.send_turn("session-a", "Hello").await.expect("turn");
        handle.send_turn("session-b", "World").await.expect("turn");

        let status = handle.status().await.expect("status");
        assert_eq!(status.session_count, 2);

        handle.shutdown().await.expect("shutdown");
        join.await.expect("join");
    }

    #[tokio::test]
    async fn status_after_turn_shows_idle_and_session() {
        let (handle, join, _dir) = spawn_test_actor();

        handle.send_turn("main", "Hello").await.expect("turn");

        let status = handle.status().await.expect("status");
        assert_eq!(status.lifecycle, NousLifecycle::Idle);
        assert_eq!(status.session_count, 1);
        assert!(status.active_session.is_none());

        handle.shutdown().await.expect("shutdown");
        join.await.expect("join");
    }

    #[tokio::test]
    async fn sleep_then_sleep_is_idempotent() {
        let (handle, join, _dir) = spawn_test_actor();

        handle.sleep().await.expect("sleep");
        handle.sleep().await.expect("sleep again");
        let status = handle.status().await.expect("status");
        assert_eq!(status.lifecycle, NousLifecycle::Dormant);

        handle.shutdown().await.expect("shutdown");
        join.await.expect("join");
    }

    #[tokio::test]
    async fn wake_when_idle_is_noop() {
        let (handle, join, _dir) = spawn_test_actor();

        handle.wake().await.expect("wake");
        let status = handle.status().await.expect("status");
        assert_eq!(status.lifecycle, NousLifecycle::Idle);

        handle.shutdown().await.expect("shutdown");
        join.await.expect("join");
    }

    #[tokio::test]
    async fn send_after_shutdown_returns_error() {
        let (handle, join, _dir) = spawn_test_actor();

        handle.shutdown().await.expect("shutdown");
        join.await.expect("join");

        let err = handle.send_turn("main", "Hello").await;
        assert!(err.is_err());
        assert!(err.unwrap_err().to_string().contains("inbox closed"));
    }

    #[tokio::test]
    async fn handle_clone_works() {
        let (handle, join, _dir) = spawn_test_actor();

        let handle2 = handle.clone();
        let status = handle2.status().await.expect("status");
        assert_eq!(status.id, "test-agent");

        handle.shutdown().await.expect("shutdown");
        join.await.expect("join");
    }

    #[test]
    fn send_sync_assertions() {
        static_assertions::assert_impl_all!(NousHandle: Send, Sync, Clone);
        static_assertions::assert_impl_all!(NousMessage: Send);
        static_assertions::assert_impl_all!(NousStatus: Send, Sync);
        static_assertions::assert_impl_all!(NousLifecycle: Send, Sync, Copy);
    }

    #[test]
    fn default_inbox_capacity_is_32() {
        assert_eq!(DEFAULT_INBOX_CAPACITY, 32);
    }
}
