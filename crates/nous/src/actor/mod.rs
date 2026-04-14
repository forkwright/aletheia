//! Tokio actor for a single nous agent instance.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

use tokio::sync::Mutex;
use tokio::sync::mpsc;
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info, instrument, warn};

use hermeneus::provider::ProviderRegistry;
use mneme::embedding::EmbeddingProvider;
#[cfg(feature = "knowledge-store")]
use mneme::knowledge_store::KnowledgeStore;
use mneme::store::SessionStore;
use organon::registry::ToolRegistry;
use organon::types::ToolServices;
use taxis::oikos::Oikos;

use crate::bootstrap::BootstrapSection;
use crate::config::{NousConfig, PipelineConfig};
use crate::cross::CrossNousEnvelope;
use crate::drift::DriftDetector;
use crate::message::{NousLifecycle, NousMessage, NousStatus};
use crate::session::SessionState;

mod background;
mod spawn;
mod turn;

pub(crate) use spawn::spawn;

/// Default bounded channel capacity for the actor inbox.
pub const DEFAULT_INBOX_CAPACITY: usize = 32;

/// Maximum number of concurrent background tasks (extraction, distillation, skills).
pub(crate) const MAX_SPAWNED_TASKS: usize = 8;

/// Maximum number of sessions tracked in the actor's in-memory `HashMap`.
/// When exceeded, the oldest session (by last activity) is evicted.
pub(crate) const MAX_SESSIONS: usize = 1000;

/// Messaging and lifecycle channels for the actor.
pub(crate) struct ActorChannel {
    inbox: mpsc::Receiver<NousMessage>,
    cross_rx: Option<mpsc::Receiver<CrossNousEnvelope>>,
    /// Token signalling a graceful shutdown request from the manager.
    cancel: CancellationToken,
    status: NousLifecycle,
}

/// External service dependencies injected at actor creation.
pub(crate) struct ActorServices {
    providers: Arc<ProviderRegistry>,
    tools: Arc<ToolRegistry>,
    oikos: Arc<Oikos>,
    tool_services: Option<Arc<ToolServices>>,
    embedding_provider: Option<Arc<dyn EmbeddingProvider>>,
    /// Candidate tracker for skill auto-capture pipeline.
    candidate_tracker: Arc<mneme::skills::CandidateTracker>,
    /// Deployment-tunable tool limits from taxis config.
    pub(crate) tool_config: Arc<taxis::config::ToolLimitsConfig>,
}

/// Data stores for sessions, knowledge, and search.
pub(crate) struct ActorStores {
    /// Shared session persistence. Lock guards `SQLite` write access; held
    /// briefly for session/message CRUD, never across `.await` points.
    session_store: Option<Arc<Mutex<SessionStore>>>,
    #[cfg(feature = "knowledge-store")]
    knowledge_store: Option<Arc<KnowledgeStore>>,
    vector_search: Option<Arc<dyn crate::recall::VectorSearch>>,
    /// BM25 text search: used as recall fallback when embedding provider is mock.
    #[cfg(feature = "knowledge-store")]
    text_search: Option<Arc<dyn crate::recall::TextSearch>>,
    /// Skill loader for per-turn skill injection. None when knowledge-store is disabled.
    #[cfg(feature = "knowledge-store")]
    skill_loader: Option<crate::skills::SkillLoader>,
}

/// Runtime state: background tasks, panic tracking, timing.
pub(crate) struct ActorRuntime {
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
    /// Number of pipeline panics caught by the panic boundary since start.
    pipeline_panic_count: u32,
    /// Timestamps of recent pipeline panics for degraded-mode window calculation.
    pipeline_panic_timestamps: Vec<Instant>,
    /// Number of background task panics since start.
    background_panic_count: u32,
    /// Timestamps of recent background task panics (for logging/monitoring only).
    background_panic_timestamps: Vec<Instant>,
    /// When the actor started running.
    started_at: Instant,
    /// Timestamp of the most recent pipeline panic, used for auto-recovery from degraded mode.
    last_panic_at: Option<Instant>,
    /// Consecutive inbox recv timeouts without a successful message. (#2159)
    consecutive_timeouts: u32,
}

/// A single nous agent running as a Tokio actor.
///
/// Each actor owns its mutable state and processes messages sequentially
/// from a bounded inbox. External code interacts via [`NousHandle`](crate::handle::NousHandle).
pub struct NousActor {
    id: String,
    config: NousConfig,
    pipeline_config: PipelineConfig,
    extra_bootstrap: Vec<BootstrapSection>,
    channel: ActorChannel,
    sessions: HashMap<String, SessionState>,
    active_session: Option<String>,
    services: ActorServices,
    stores: ActorStores,
    runtime: ActorRuntime,
    /// Per-session quality drift detectors keyed by session key.
    ///
    /// // WHY: Drift is tracked per-session, not globally, because different
    /// // sessions may have different quality baselines. A coding session
    /// // naturally has different tool-error patterns than a research session.
    drift_detectors: HashMap<String, DriftDetector>,
    /// Deployment-level behavioral configuration (panic thresholds, timeouts).
    pub(crate) nous_behavior: taxis::config::NousBehaviorConfig,
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
        nous_behavior: taxis::config::NousBehaviorConfig,
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
            extra_bootstrap,
            channel: ActorChannel {
                inbox,
                cross_rx,
                cancel,
                status: NousLifecycle::Idle,
            },
            sessions: HashMap::new(),
            active_session: None,
            services: ActorServices {
                providers,
                tools,
                oikos,
                tool_services,
                embedding_provider,
                candidate_tracker: Arc::new(mneme::skills::CandidateTracker::new()),
                tool_config: Arc::new(taxis::config::ToolLimitsConfig::default()),
            },
            stores: ActorStores {
                session_store,
                #[cfg(feature = "knowledge-store")]
                knowledge_store,
                vector_search,
                #[cfg(feature = "knowledge-store")]
                text_search,
                #[cfg(feature = "knowledge-store")]
                skill_loader,
            },
            runtime: ActorRuntime {
                background_tasks: JoinSet::new(),
                active_turn,
                distillation_in_progress: Arc::new(AtomicBool::new(false)),
                pipeline_panic_count: 0,
                pipeline_panic_timestamps: Vec::new(),
                background_panic_count: 0,
                background_panic_timestamps: Vec::new(),
                started_at: Instant::now(),
                last_panic_at: None,
                consecutive_timeouts: 0,
            },
            drift_detectors: HashMap::new(),
            nous_behavior,
        }
    }

    // NOTE(#940): 92 lines: actor message loop with tokio::select!. Under the 100-line
    // threshold but noted for completeness; the select loop is one cohesive operation.
    /// Run the actor loop until shutdown or all handles are dropped.
    ///
    /// # Cancel safety
    ///
    /// Cancel-safe. The `select!` branches use `inbox.recv()`,
    /// `cross_rx.recv()`, and `cancel.cancelled()`, all of which are
    /// cancel-safe. Dropping the future exits the loop without leaving
    /// inconsistent state.
    ///
    /// // SAFETY: The select! uses `biased;` ordering: cancellation and shutdown
    /// // branches are polled first. This ensures prompt shutdown even when
    /// // the inbox is flooded with messages.
    #[instrument(skip(self), fields(nous.id = %self.id))]
    #[expect(
        clippy::too_many_lines,
        reason = "actor run loop: sequential select! branches; splitting would obscure the event-driven structure"
    )]
    pub async fn run(mut self) {
        if let Err(e) = spawn::validate_workspace(&self.services.oikos, &self.id).await {
            error!(error = %e, "workspace validation failed, shutting down");
            return;
        }

        self.runtime.started_at = Instant::now();
        info!(lifecycle = %self.channel.status, "actor started");

        loop {
            self.reap_background_tasks();
            self.maybe_auto_recover();

            tokio::select! {
                // SAFETY: cancel-safe. `mpsc::Receiver::recv()` is cancel-safe:
                // if this branch is dropped before it fires, the message remains
                // in the inbox and will be delivered on the next poll.
                // WHY(#2159): wrapped in timeout to detect stuck actors during idle periods.
                recv_result = tokio::time::timeout(
                    Duration::from_secs(self.nous_behavior.inbox_recv_timeout_secs),
                    self.channel.inbox.recv()
                ) => {
                    let msg = match recv_result {
                        Ok(Some(msg)) => {
                            self.runtime.consecutive_timeouts = 0;
                            msg
                        }
                        Ok(None) => break,
                        Err(_elapsed) => {
                            self.runtime.consecutive_timeouts += 1;
                            debug!(
                                consecutive_timeouts = self.runtime.consecutive_timeouts,
                                inbox_recv_timeout_secs = self.nous_behavior.inbox_recv_timeout_secs,
                                "inbox recv timed out (idle)"
                            );
                            if self.runtime.consecutive_timeouts >= self.nous_behavior.consecutive_timeout_warn_threshold
                                && self.channel.status == NousLifecycle::Active
                            {
                                warn!(
                                    nous_id = %self.id,
                                    consecutive_timeouts = self.runtime.consecutive_timeouts,
                                    "inbox starved while Active — possible stuck state"
                                );
                            }
                            continue;
                        }
                    };
                    match msg {
                        NousMessage::Turn {
                            session_key,
                            session_id,
                            content,
                            span,
                            reply,
                        } => {
                            if self.channel.status == NousLifecycle::Degraded {
                                let _ = reply.send(Err(crate::error::ServiceDegradedSnafu {
                                    nous_id: self.id.clone(),
                                    panic_count: self.runtime.pipeline_panic_count,
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
                            if self.channel.status == NousLifecycle::Degraded {
                                let _ = reply.send(Err(crate::error::ServiceDegradedSnafu {
                                    nous_id: self.id.clone(),
                                    panic_count: self.runtime.pipeline_panic_count,
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
                        NousMessage::Recover { reply } => {
                            let recovered = self.recover();
                            let _ = reply.send(recovered);
                        }
                        NousMessage::Shutdown => {
                            info!("shutdown requested");
                            break;
                        }
                    }
                }
                // SAFETY: cancel-safe. `mpsc::Receiver::recv()` is cancel-safe;
                // `std::future::pending()` never resolves and is trivially cancel-safe.
                // Dropping this branch before it fires leaves the cross-nous message
                // in the channel for the next poll.
                envelope = async {
                    match self.channel.cross_rx.as_mut() {
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
                // SAFETY: cancel-safe. `CancellationToken::cancelled()` is cancel-safe;
                // dropping this branch before it resolves has no side effects.
                () = self.channel.cancel.cancelled() => {
                    // SAFETY: "token" here is a CancellationToken (shutdown coordination), not a credential
                    info!("cancellation token fired, draining and stopping");
                    break;
                }
            }
        }

        while self.runtime.background_tasks.join_next().await.is_some() {}

        info!(lifecycle = %self.channel.status, panic_count = self.runtime.pipeline_panic_count, "actor stopped");
    }

    /// # Cancel safety
    ///
    /// Cancel-safe. No state is modified before the `.await` point: only
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
            lifecycle: self.channel.status,
            session_count: self.sessions.len(),
            active_session: self.active_session.clone(),
            panic_count: self.runtime.pipeline_panic_count,
            uptime: self.runtime.started_at.elapsed(),
        };
        let _ = reply.send(status);
    }

    fn handle_sleep(&mut self) {
        match self.channel.status {
            NousLifecycle::Idle => {
                debug!("transitioning to dormant");
                self.channel.status = NousLifecycle::Dormant;
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
        match self.channel.status {
            NousLifecycle::Dormant => {
                debug!("waking from dormant");
                self.channel.status = NousLifecycle::Idle;
            }
            NousLifecycle::Idle | NousLifecycle::Active => {
                debug!(lifecycle = %self.channel.status, "already awake");
            }
            NousLifecycle::Degraded => {
                debug!("cannot wake from degraded — requires restart");
            }
        }
    }

    /// Attempt auto-recovery from degraded mode if the last panic was more than
    /// `degraded_window_secs` ago.
    fn maybe_auto_recover(&mut self) {
        if self.channel.status != NousLifecycle::Degraded {
            return;
        }
        let Some(last_panic) = self.runtime.last_panic_at else {
            return;
        };
        let degraded_window = Duration::from_secs(self.nous_behavior.degraded_window_secs);
        if last_panic.elapsed() >= degraded_window {
            info!(
                nous_id = %self.id,
                panic_count = self.runtime.pipeline_panic_count,
                elapsed_secs = last_panic.elapsed().as_secs(),
                degraded_window_secs = self.nous_behavior.degraded_window_secs,
                "auto-recovering from degraded mode: no panics in recovery window"
            );
            self.runtime.pipeline_panic_count = 0;
            self.runtime.pipeline_panic_timestamps.clear();
            self.runtime.last_panic_at = None;
            self.channel.status = NousLifecycle::Idle;
        }
    }

    /// Manually recover from degraded mode. Returns `true` if recovery occurred.
    fn recover(&mut self) -> bool {
        if self.channel.status != NousLifecycle::Degraded {
            debug!(nous_id = %self.id, lifecycle = %self.channel.status, "recover called but not degraded");
            return false;
        }
        info!(
            nous_id = %self.id,
            panic_count = self.runtime.pipeline_panic_count,
            "manual recovery from degraded mode"
        );
        self.runtime.pipeline_panic_count = 0;
        self.runtime.pipeline_panic_timestamps.clear();
        self.runtime.last_panic_at = None;
        self.channel.status = NousLifecycle::Idle;
        true
    }

    /// Evict the oldest session (by `last_accessed`) when the session count reaches
    /// `MAX_SESSIONS`. Prevents unbounded memory growth using LRU eviction.
    ///
    /// // WHY: LRU eviction prioritizes keeping active sessions over dormant ones.
    /// // This prevents memory exhaustion from abandoned sessions while preserving
    /// // context for ongoing conversations.
    fn evict_oldest_session_if_needed(&mut self) {
        if self.sessions.len() < MAX_SESSIONS {
            return;
        }
        // WHY: last_accessed tracks actual session activity; least recently used is evicted.
        let oldest_key = self
            .sessions
            .iter()
            .min_by_key(|(_, state)| state.last_accessed)
            .map(|(key, _)| key.clone());
        if let Some(key) = oldest_key {
            warn!(
                nous_id = %self.id,
                evicted_session = %key,
                session_count = self.sessions.len(),
                "session limit reached, evicting oldest session"
            );
            self.sessions.remove(&key);
        }
    }

    /// Resolve skill sections for the current turn's task context.
    ///
    /// Returns bootstrap sections for skills relevant to `content`. Returns an
    /// empty vec when the knowledge-store feature is disabled, when no
    /// `KnowledgeStore` is configured, or when no skills match: preserving
    /// existing behaviour in all degraded cases.
    ///
    /// // WHY: Skills are resolved per-turn because they depend on the specific
    /// // task context extracted from user input. A coding query needs different
    /// // skills than a research query, even for the same agent.
    ///
    /// # Cancel safety
    ///
    /// Cancel-safe. The inner `resolve_skills` spawns a separate Tokio task for
    /// the blocking search; cancelling this future at the await point loses the
    /// skill result but leaves no inconsistent state.
    // WHY: body has no .await without knowledge-store feature: kept async so callers .await uniformly
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
            if let Some(ref loader) = self.stores.skill_loader {
                let task_context = crate::skills::extract_task_context(content);
                let max_skills = self.config.behavior.skills_max_skills;
                tracing::debug!(max_skills, "resolve_skill_sections: max_skills from behavior");
                return loader
                    .resolve_skills(&self.id, &task_context, max_skills)
                    .await;
            }
        }
        // WHY: suppress unused-variable warning when tui feature is off
        #[cfg(not(feature = "knowledge-store"))]
        let _ = content;

        vec![]
    }

    /// Resolve intent sections for injection into the bootstrap system prompt.
    ///
    /// Reads active intents from `instance/nous/<id>/intents.json` and returns a
    /// single [`BootstrapSection`] when there are active intents. Returns an empty
    /// vec when no intents are active (file absent or all resolved/expired).
    ///
    /// // WHY: Intents are operator directives (e.g., "migrate all tests to insta")
    /// // that must be visible to the agent for planning. They live outside the
    /// // normal bootstrap files because they're dynamic and task-specific.
    ///
    /// Degrades gracefully: any I/O or deserialization error is logged and
    /// returns an empty vec so the pipeline is never blocked by intent load failures.
    pub(crate) fn resolve_intent_sections(&self) -> Vec<BootstrapSection> {
        use crate::bootstrap::{BootstrapSection, SectionPriority};
        use crate::budget::{CharEstimator, TokenEstimator as _};
        use dianoia::intent::IntentStore;
        use tracing::warn;

        let agent_dir = self.services.oikos.nous_dir(&self.id);
        let store = IntentStore::new(agent_dir);

        match store.render_for_bootstrap() {
            Ok(Some(content)) => {
                let tokens = CharEstimator::default().estimate(&content);
                vec![BootstrapSection {
                    name: "operator-intents".to_owned(),
                    // WHY: Important so intents survive token budget pressure — they must be
                    // consulted before every autonomous decision (per issue #2332).
                    priority: SectionPriority::Important,
                    content,
                    tokens,
                    truncatable: false,
                }]
            }
            Ok(None) => vec![],
            Err(e) => {
                warn!(
                    nous_id = %self.id,
                    error = %e,
                    "failed to load operator intents for bootstrap, continuing without"
                );
                vec![]
            }
        }
    }
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
