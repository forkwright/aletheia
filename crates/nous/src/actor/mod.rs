//! Tokio actor for a single nous agent instance.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Instant;

use tokio::sync::Mutex;
use tokio::task::JoinSet;

#[cfg(feature = "knowledge-store")]
use aletheia_mneme::knowledge_store::KnowledgeStore;
use aletheia_mneme::store::SessionStore;

use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info, instrument, warn};

use crate::cross::CrossNousEnvelope;

use aletheia_hermeneus::provider::ProviderRegistry;
use aletheia_mneme::embedding::EmbeddingProvider;
use aletheia_organon::registry::ToolRegistry;
use aletheia_organon::types::ToolServices;
use aletheia_taxis::oikos::Oikos;

use crate::bootstrap::BootstrapSection;
use crate::config::{NousConfig, PipelineConfig};
use crate::message::{NousLifecycle, NousMessage, NousStatus};
use crate::session::SessionState;

mod background;
mod spawn;
mod turn;

pub use spawn::spawn;

/// Default bounded channel capacity for the actor inbox.
pub const DEFAULT_INBOX_CAPACITY: usize = 32;

/// Maximum number of concurrent background tasks (extraction, distillation, skills).
pub(crate) const MAX_SPAWNED_TASKS: usize = 8;

/// Number of panics within `DEGRADED_WINDOW` that triggers degraded mode.
const DEGRADED_PANIC_THRESHOLD: u32 = 5;

/// Time window for panic counting (10 minutes).
const DEGRADED_WINDOW: std::time::Duration = std::time::Duration::from_secs(600);

/// A single nous agent running as a Tokio actor.
///
/// Each actor owns its mutable state and processes messages sequentially
/// from a bounded inbox. External code interacts via [`NousHandle`](crate::handle::NousHandle).
pub struct NousActor {
    id: String,
    config: NousConfig,
    pipeline_config: PipelineConfig,
    inbox: mpsc::Receiver<NousMessage>,
    cross_rx: Option<mpsc::Receiver<CrossNousEnvelope>>,
    /// Token signalling a graceful shutdown request from the manager.
    cancel: CancellationToken,
    lifecycle: NousLifecycle,
    sessions: HashMap<String, SessionState>,
    active_session: Option<String>,
    providers: Arc<ProviderRegistry>,
    tools: Arc<ToolRegistry>,
    oikos: Arc<Oikos>,
    embedding_provider: Option<Arc<dyn EmbeddingProvider>>,
    vector_search: Option<Arc<dyn crate::recall::VectorSearch>>,
    /// BM25 text search — used as recall fallback when embedding provider is mock.
    #[cfg(feature = "knowledge-store")]
    text_search: Option<Arc<dyn crate::recall::TextSearch>>,
    session_store: Option<Arc<Mutex<SessionStore>>>,
    #[cfg(feature = "knowledge-store")]
    knowledge_store: Option<Arc<KnowledgeStore>>,
    tool_services: Option<Arc<ToolServices>>,
    extra_bootstrap: Vec<BootstrapSection>,
    /// Skill loader for per-turn skill injection. None when knowledge-store is disabled.
    #[cfg(feature = "knowledge-store")]
    skill_loader: Option<crate::skills::SkillLoader>,
    /// Candidate tracker for skill auto-capture pipeline.
    candidate_tracker: Arc<aletheia_mneme::skills::CandidateTracker>,
    /// Number of panics caught by the panic boundary since start.
    panic_count: u32,
    /// Timestamps of recent panics for degraded-mode window calculation.
    panic_timestamps: Vec<Instant>,
    /// When the actor started running.
    started_at: Instant,
    /// Background tasks (extraction, distillation, skill analysis).
    background_tasks: JoinSet<()>,
    /// Set to `true` while processing a turn; `false` when idle.
    /// Shared with the manager's `ActorEntry` so health checks can
    /// distinguish a busy actor from an unresponsive one without queuing
    /// through the inbox.
    active_turn: Arc<AtomicBool>,
    /// Guards against concurrent distillation when two turns finish close together.
    /// WHY: Background distillation is async; a second turn can finish while the
    /// first turn's distillation task is still running, triggering a duplicate.
    distillation_in_progress: Arc<AtomicBool>,
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
        cancel: CancellationToken,
        providers: Arc<ProviderRegistry>,
        tools: Arc<ToolRegistry>,
        oikos: Arc<Oikos>,
        embedding_provider: Option<Arc<dyn EmbeddingProvider>>,
        vector_search: Option<Arc<dyn crate::recall::VectorSearch>>,
        session_store: Option<Arc<Mutex<SessionStore>>>,
        #[cfg(feature = "knowledge-store")] knowledge_store: Option<Arc<KnowledgeStore>>,
        tool_services: Option<Arc<ToolServices>>,
        extra_bootstrap: Vec<BootstrapSection>,
        active_turn: Arc<AtomicBool>,
    ) -> Self {
        #[cfg(feature = "knowledge-store")]
        let skill_loader = knowledge_store
            .as_ref()
            .map(|ks| crate::skills::SkillLoader::new(Arc::clone(ks)));

        #[cfg(feature = "knowledge-store")]
        let text_search: Option<Arc<dyn crate::recall::TextSearch>> =
            knowledge_store
                .as_ref()
                .map(|ks| -> Arc<dyn crate::recall::TextSearch> {
                    Arc::new(crate::recall::KnowledgeTextSearch::new(Arc::clone(ks)))
                });

        Self {
            id,
            config,
            pipeline_config,
            inbox,
            cross_rx,
            cancel,
            lifecycle: NousLifecycle::Idle,
            sessions: HashMap::new(),
            active_session: None,
            providers,
            tools,
            oikos,
            embedding_provider,
            vector_search,
            #[cfg(feature = "knowledge-store")]
            text_search,
            session_store,
            #[cfg(feature = "knowledge-store")]
            knowledge_store,
            tool_services,
            extra_bootstrap,
            #[cfg(feature = "knowledge-store")]
            skill_loader,
            candidate_tracker: Arc::new(aletheia_mneme::skills::CandidateTracker::new()),
            panic_count: 0,
            panic_timestamps: Vec::new(),
            started_at: Instant::now(),
            background_tasks: JoinSet::new(),
            active_turn,
            distillation_in_progress: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Run the actor loop until shutdown or all handles are dropped.
    ///
    /// # Cancel safety
    ///
    /// Cancel-safe. The `select!` branches use `inbox.recv()`,
    /// `cross_rx.recv()`, and `cancel.cancelled()`, all of which are
    /// cancel-safe. Dropping the future exits the loop without leaving
    /// inconsistent state.
    #[instrument(skip(self), fields(nous.id = %self.id))]
    pub async fn run(mut self) {
        if let Err(e) = spawn::validate_workspace(&self.oikos, &self.id).await {
            error!(error = %e, "workspace validation failed, shutting down");
            return;
        }

        self.started_at = Instant::now();
        info!(lifecycle = %self.lifecycle, "actor started");

        loop {
            self.reap_background_tasks();

            tokio::select! {
                msg = self.inbox.recv() => {
                    let Some(msg) = msg else { break };
                    match msg {
                        NousMessage::Turn {
                            session_key,
                            session_id,
                            content,
                            span,
                            reply,
                        } => {
                            if self.lifecycle == NousLifecycle::Degraded {
                                let _ = reply.send(Err(crate::error::ServiceDegradedSnafu {
                                    nous_id: self.id.clone(),
                                    panic_count: self.panic_count,
                                }.build()));
                            } else {
                                self.handle_turn(session_key, session_id, content, span, reply).await;
                            }
                        }
                        NousMessage::StreamingTurn {
                            session_key,
                            session_id,
                            content,
                            stream_tx,
                            span,
                            reply,
                        } => {
                            if self.lifecycle == NousLifecycle::Degraded {
                                let _ = reply.send(Err(crate::error::ServiceDegradedSnafu {
                                    nous_id: self.id.clone(),
                                    panic_count: self.panic_count,
                                }.build()));
                                drop(stream_tx);
                            } else {
                                self.handle_streaming_turn(session_key, session_id, content, stream_tx, span, reply).await;
                            }
                        }
                        NousMessage::Status { reply } => {
                            self.handle_status(reply);
                        }
                        NousMessage::Ping { reply } => {
                            let _ = reply.send(());
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
                        let from = envelope.message.from.clone();
                        if let Err(e) = self.handle_cross_message(envelope).await {
                            warn!(nous_id = %self.id, from = %from, error = %e, "cross-nous message processing failed");
                        }
                    }
                }
                () = self.cancel.cancelled() => {
                    info!("cancellation token fired, draining and stopping");
                    break;
                }
            }
        }

        while self.background_tasks.join_next().await.is_some() {}

        info!(lifecycle = %self.lifecycle, panic_count = self.panic_count, "actor stopped");
    }

    /// # Cancel safety
    ///
    /// Cancel-safe. No state is modified before the `.await` point — only
    /// local variables are prepared from the envelope. If cancelled, the
    /// actor remains in a consistent state.
    async fn handle_cross_message(
        &mut self,
        envelope: CrossNousEnvelope,
    ) -> crate::error::Result<()> {
        let from = &envelope.message.from;
        let session_key = format!("cross:{from}");
        let content = envelope.message.content.clone();

        debug!(from = %from, session_key = %session_key, "processing cross-nous message");

        match self.execute_turn(&session_key, &content).await {
            Ok(result) => {
                debug!(from = %from, content_len = result.content.len(), "cross-nous turn completed");
                Ok(())
            }
            Err(e) => Err(e),
        }
    }

    fn handle_status(&self, reply: tokio::sync::oneshot::Sender<NousStatus>) {
        let status = NousStatus {
            id: self.id.clone(),
            lifecycle: self.lifecycle,
            session_count: self.sessions.len(),
            active_session: self.active_session.clone(),
            panic_count: self.panic_count,
            uptime: self.started_at.elapsed(),
        };
        let _ = reply.send(status);
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
            NousLifecycle::Degraded => {
                debug!("cannot sleep while degraded");
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
            NousLifecycle::Degraded => {
                debug!("cannot wake from degraded — requires restart");
            }
        }
    }

    /// Resolve skill sections for the current turn's task context.
    ///
    /// Returns bootstrap sections for skills relevant to `content`. Returns an
    /// empty vec when the knowledge-store feature is disabled, when no
    /// `KnowledgeStore` is configured, or when no skills match — preserving
    /// existing behaviour in all degraded cases.
    ///
    /// # Cancel safety
    ///
    /// Cancel-safe. The inner `resolve_skills` spawns a separate Tokio task for
    /// the blocking search; cancelling this future at the await point loses the
    /// skill result but leaves no inconsistent state.
    // WHY: body has no .await without knowledge-store feature — kept async so callers .await uniformly
    #[cfg_attr(
        not(feature = "knowledge-store"),
        expect(
            clippy::unused_async,
            reason = "await compiled away without knowledge-store feature"
        )
    )]
    async fn resolve_skill_sections(&self, content: &str) -> Vec<BootstrapSection> {
        #[cfg(feature = "knowledge-store")]
        {
            if let Some(ref loader) = self.skill_loader {
                let task_context = crate::skills::extract_task_context(content);
                return loader
                    .resolve_skills(&self.id, &task_context, crate::skills::DEFAULT_MAX_SKILLS)
                    .await;
            }
        }
        // WHY: suppress unused-variable warning when tui feature is off
        #[cfg(not(feature = "knowledge-store"))]
        let _ = content;

        vec![]
    }
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
