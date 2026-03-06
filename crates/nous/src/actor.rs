//! Tokio actor for a single nous agent instance.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

#[cfg(feature = "knowledge-store")]
use aletheia_mneme::knowledge_store::KnowledgeStore;
use aletheia_mneme::store::SessionStore;

use tokio::sync::mpsc;
use tracing::{Instrument, debug, error, info, instrument, warn};

use crate::cross::CrossNousEnvelope;
use crate::stream::TurnStreamEvent;

use aletheia_hermeneus::provider::ProviderRegistry;
use aletheia_koina::id::{NousId, SessionId};
use aletheia_mneme::embedding::EmbeddingProvider;
use aletheia_organon::registry::ToolRegistry;
use aletheia_organon::types::{ToolContext, ToolServices};
use aletheia_taxis::cascade;
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
    #[cfg(feature = "knowledge-store")]
    knowledge_store: Option<Arc<KnowledgeStore>>,
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
        #[cfg(feature = "knowledge-store")] knowledge_store: Option<Arc<KnowledgeStore>>,
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
            #[cfg(feature = "knowledge-store")]
            knowledge_store,
            tool_services,
            extra_bootstrap,
        }
    }

    /// Run the actor loop until shutdown or all handles are dropped.
    ///
    /// # Cancel safety
    ///
    /// Cancel-safe. The `select!` branches use `inbox.recv()` and
    /// `cross_rx.recv()`, both of which are cancel-safe. Dropping the
    /// future exits the loop without leaving inconsistent state.
    #[instrument(skip(self), fields(nous.id = %self.id))]
    pub async fn run(mut self) {
        if let Err(e) = validate_workspace(&self.oikos, &self.id) {
            error!(error = %e, "workspace validation failed, shutting down");
            return;
        }

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
                        NousMessage::StreamingTurn {
                            session_key,
                            content,
                            stream_tx,
                            reply,
                        } => {
                            self.handle_streaming_turn(session_key, content, stream_tx, reply).await;
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

    /// # Cancel safety
    ///
    /// Cancel-safe. No state is modified before the `.await` point — only
    /// local variables are prepared from the envelope. If cancelled, the
    /// actor remains in a consistent state.
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

    /// # Cancel safety
    ///
    /// Not cancel-safe in isolation. Sets `lifecycle = Active` and
    /// `active_session` before awaiting `execute_turn`. If the future
    /// were dropped mid-await, those fields would not be reset. In
    /// practice this is only called from the sequential actor loop, so
    /// cancellation only occurs at shutdown when the actor is consumed.
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
            self.maybe_spawn_distillation(&session_key);
        }

        self.active_session = None;
        self.lifecycle = NousLifecycle::Idle;

        // Ignore send error — caller may have dropped the receiver
        let _ = reply.send(result);
    }

    /// # Cancel safety
    ///
    /// Not cancel-safe in isolation — same profile as `handle_turn`.
    /// Sets `lifecycle` and `active_session` before the `.await` point.
    /// Only called from the sequential actor loop.
    async fn handle_streaming_turn(
        &mut self,
        session_key: String,
        content: String,
        stream_tx: mpsc::Sender<TurnStreamEvent>,
        reply: tokio::sync::oneshot::Sender<crate::error::Result<TurnResult>>,
    ) {
        if self.lifecycle == NousLifecycle::Dormant {
            debug!("auto-waking from dormant for streaming turn");
            self.lifecycle = NousLifecycle::Idle;
        }

        self.lifecycle = NousLifecycle::Active;
        self.active_session = Some(session_key.clone());

        let result = self
            .execute_streaming_turn(&session_key, &content, &stream_tx)
            .await;

        if let Ok(ref turn_result) = result {
            self.maybe_spawn_extraction(&content, &turn_result.content);
            self.maybe_spawn_distillation(&session_key);
        }

        self.active_session = None;
        self.lifecycle = NousLifecycle::Idle;

        let _ = reply.send(result);
    }

    /// # Cancel safety
    ///
    /// Cancel-safe. Session creation via `entry().or_insert_with()` is
    /// idempotent. If cancelled during `run_pipeline`, the session exists
    /// but the turn is incomplete — no persistent state is corrupted.
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
            active_tools: std::sync::Arc::new(std::sync::RwLock::new(
                std::collections::HashSet::new(),
            )),
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
            None,
        )
        .await
    }

    /// # Cancel safety
    ///
    /// Cancel-safe — same profile as `execute_turn`.
    async fn execute_streaming_turn(
        &mut self,
        session_key: &str,
        content: &str,
        stream_tx: &mpsc::Sender<TurnStreamEvent>,
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
            active_tools: std::sync::Arc::new(std::sync::RwLock::new(
                std::collections::HashSet::new(),
            )),
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
            Some(stream_tx),
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
        #[cfg(feature = "knowledge-store")]
        let knowledge_store = self.knowledge_store.clone();

        tokio::spawn(
            async move {
                run_extraction(
                    &config,
                    providers,
                    &nous_id,
                    &user,
                    &assistant,
                    #[cfg(feature = "knowledge-store")]
                    knowledge_store.as_ref(),
                );
            }
            .instrument(span),
        );
    }

    fn maybe_spawn_distillation(&self, session_key: &str) {
        let Some(ref store_arc) = self.session_store else {
            return;
        };
        let Some(session_state) = self.sessions.get(session_key) else {
            return;
        };
        let session_id = session_state.id.clone();

        // Quick trigger check under the lock
        let should_distill = {
            let Ok(store) = store_arc.lock() else {
                warn!("session store lock poisoned, skipping distillation check");
                return;
            };
            let Ok(Some(session)) = store.find_session_by_id(&session_id) else {
                return;
            };
            let config = crate::distillation::DistillTriggerConfig::default();
            crate::distillation::should_trigger_distillation(
                &session,
                u64::from(self.config.context_window),
                &config,
            )
            .is_some()
        };

        if !should_distill {
            return;
        }

        let config = crate::distillation::DistillTriggerConfig::default();
        if self.providers.find_provider(&config.model).is_none() {
            warn!(model = %config.model, "no provider for distillation model");
            return;
        }

        let store = Arc::clone(store_arc);
        let providers = Arc::clone(&self.providers);
        let nous_id = self.id.clone();
        let span =
            tracing::info_span!("distillation", nous.id = %nous_id, session.id = %session_id);

        tokio::spawn(
            run_background_distillation(store, providers, session_id, nous_id, config)
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
    #[cfg(feature = "knowledge-store")] knowledge_store: Option<&Arc<KnowledgeStore>>,
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
            let entities = extraction.entities.len();
            let relationships = extraction.relationships.len();
            let facts = extraction.facts.len();

            #[cfg(feature = "knowledge-store")]
            if let Some(store) = knowledge_store {
                match engine.persist(&extraction, store, "background", nous_id) {
                    Ok(result) => {
                        info!(
                            nous_id = %nous_id,
                            entities_persisted = result.entities_inserted,
                            relationships_persisted = result.relationships_inserted,
                            facts_persisted = result.facts_inserted,
                            "extraction persisted to knowledge store"
                        );
                    }
                    Err(e) => {
                        warn!(nous_id = %nous_id, error = %e, "extraction persist failed");
                    }
                }
            }

            info!(
                nous_id = %nous_id,
                entities,
                relationships,
                facts,
                "extraction completed"
            );
        }
        Err(e) => {
            warn!(nous_id = %nous_id, error = %e, "extraction failed");
        }
    }
}

/// Run distillation as a background task. Loads history, calls LLM, applies results.
async fn run_background_distillation(
    store: Arc<Mutex<SessionStore>>,
    providers: Arc<ProviderRegistry>,
    session_id: String,
    nous_id: String,
    config: crate::distillation::DistillTriggerConfig,
) {
    let Some(provider) = providers.find_provider(&config.model) else {
        return;
    };

    // Load history under the lock, then release before async work
    let (history, session) = {
        let Ok(s) = store.lock() else {
            warn!("session store lock poisoned, skipping distillation");
            return;
        };
        let Ok(Some(session)) = s.find_session_by_id(&session_id) else {
            return;
        };
        match s.get_history(&session_id, None) {
            Ok(h) if !h.is_empty() => (h, session),
            Ok(_) => return,
            Err(e) => {
                warn!(error = %e, "failed to load history for distillation");
                return;
            }
        }
    };

    let messages = crate::distillation::convert_to_hermeneus_messages(&history);
    let engine =
        aletheia_melete::distill::DistillEngine::new(aletheia_melete::distill::DistillConfig {
            model: config.model.clone(),
            verbatim_tail: config.verbatim_tail,
            ..Default::default()
        });

    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "distillation count is small non-negative"
    )]
    let distill_count = session.distillation_count as u32;
    let result = match engine
        .distill(&messages, &nous_id, provider, distill_count + 1)
        .await
    {
        Ok(r) => r,
        Err(e) => {
            warn!(error = %e, "distillation LLM call failed");
            return;
        }
    };

    // Apply results under the lock
    let Ok(s) = store.lock() else {
        warn!("session store lock poisoned, skipping distillation apply");
        return;
    };
    if let Err(e) = crate::distillation::apply_distillation(&s, &session_id, &result, &history) {
        warn!(error = %e, "failed to apply distillation");
        return;
    }

    info!(
        session_id = %session_id,
        messages_distilled = result.messages_distilled,
        "background distillation complete"
    );
}

/// Validate the workspace directory exists and required files are resolvable.
///
/// Called at actor startup before entering the message loop. Creates the
/// workspace directory if missing and fails fast if SOUL.md cannot be found
/// through the cascade.
fn validate_workspace(oikos: &Oikos, nous_id: &str) -> crate::error::Result<()> {
    let workspace = oikos.nous_dir(nous_id);
    if !workspace.exists() {
        warn!(
            agent = nous_id,
            path = %workspace.display(),
            "workspace directory missing, creating"
        );
        std::fs::create_dir_all(&workspace).map_err(|e| {
            crate::error::WorkspaceValidationSnafu {
                nous_id: nous_id.to_owned(),
                message: format!("failed to create workspace directory: {e}"),
            }
            .build()
        })?;
    }

    if cascade::resolve(oikos, nous_id, "SOUL.md", None).is_none() {
        return Err(crate::error::WorkspaceValidationSnafu {
            nous_id: nous_id.to_owned(),
            message: "SOUL.md not found in cascade (nous/, shared/, theke/)".to_owned(),
        }
        .build());
    }

    // Log warnings for missing optional workspace files
    for filename in &[
        "USER.md",
        "AGENTS.md",
        "GOALS.md",
        "TOOLS.md",
        "MEMORY.md",
        "IDENTITY.md",
        "PROSOCHE.md",
        "CONTEXT.md",
    ] {
        if cascade::resolve(oikos, nous_id, filename, None).is_none() {
            debug!(
                agent = nous_id,
                file = *filename,
                "optional workspace file not found"
            );
        }
    }

    info!(agent = nous_id, "workspace validated");
    Ok(())
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
    #[cfg(feature = "knowledge-store")] knowledge_store: Option<Arc<KnowledgeStore>>,
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
        #[cfg(feature = "knowledge-store")]
        knowledge_store,
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

        fn as_any(&self) -> &dyn std::any::Any {
            self
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
            #[cfg(feature = "knowledge-store")]
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

    #[test]
    fn validate_workspace_creates_missing_dir() {
        let dir = tempfile::TempDir::new().expect("tmpdir");
        let root = dir.path();
        // Only create shared/ with SOUL.md for cascade fallback
        std::fs::create_dir_all(root.join("shared")).expect("mkdir shared");
        std::fs::create_dir_all(root.join("theke")).expect("mkdir theke");
        std::fs::write(root.join("shared/SOUL.md"), "# Test Soul").expect("write");

        let oikos = Oikos::from_root(root);
        super::validate_workspace(&oikos, "test-agent").unwrap();
        assert!(root.join("nous/test-agent").exists());
    }

    #[test]
    fn validate_workspace_fails_without_soul() {
        let dir = tempfile::TempDir::new().expect("tmpdir");
        let root = dir.path();
        std::fs::create_dir_all(root.join("nous/test-agent")).expect("mkdir");
        std::fs::create_dir_all(root.join("shared")).expect("mkdir shared");
        std::fs::create_dir_all(root.join("theke")).expect("mkdir theke");

        let oikos = Oikos::from_root(root);
        let result = super::validate_workspace(&oikos, "test-agent");
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("SOUL.md"),
            "error should mention SOUL.md: {msg}"
        );
    }
}
