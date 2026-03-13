//! Manages all nous actor instances.

use std::collections::HashMap;
use std::sync::Arc;

use std::sync::Mutex;
use tokio::sync::Mutex as TokioMutex;

#[cfg(feature = "knowledge-store")]
use aletheia_mneme::knowledge_store::KnowledgeStore;
use aletheia_mneme::store::SessionStore;
use aletheia_thesauros::loader::LoadedPack;

use std::collections::BTreeMap;
use std::time::Duration;

use tokio::sync::watch;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use tracing::{Instrument, debug, error, info, warn};

use aletheia_hermeneus::provider::ProviderRegistry;
use aletheia_mneme::embedding::EmbeddingProvider;
use aletheia_organon::registry::ToolRegistry;
use aletheia_organon::types::ToolServices;
use aletheia_taxis::oikos::Oikos;

use crate::actor;
use crate::bootstrap::pack_sections_to_bootstrap;
use crate::budget::CharEstimator;
use crate::config::{NousConfig, PipelineConfig};
use crate::handle::NousHandle;
use crate::message::{ActorHealth, NousStatus};

struct ActorEntry {
    handle: NousHandle,
    /// Wrapped in `Mutex<Option<_>>` so `shutdown_readonly` can take the handle
    /// from a shared reference, await it, and confirm the actor has stopped.
    join: Mutex<Option<JoinHandle<()>>>,
    config: NousConfig,
    pipeline_config: PipelineConfig,
    /// Number of consecutive health-check misses.
    consecutive_misses: u32,
    /// Number of times this actor has been restarted.
    restart_count: u32,
    /// When the actor was last (re)started.
    last_start: std::time::Instant,
}

/// Default interval between health polls (30 seconds).
pub const DEFAULT_HEALTH_INTERVAL: Duration = Duration::from_secs(30);

/// Default ping timeout for liveness checks (5 seconds).
pub const DEFAULT_PING_TIMEOUT: Duration = Duration::from_secs(5);

/// Consecutive misses before declaring an actor dead.
const DEAD_THRESHOLD: u32 = 3;

/// Maximum backoff between restart attempts (5 minutes).
const MAX_RESTART_BACKOFF: Duration = Duration::from_secs(300);

/// Manages the lifecycle of all nous actors.
pub struct NousManager {
    actors: HashMap<String, ActorEntry>,
    providers: Arc<ProviderRegistry>,
    tools: Arc<ToolRegistry>,
    oikos: Arc<Oikos>,
    embedding_provider: Option<Arc<dyn EmbeddingProvider>>,
    vector_search: Option<Arc<dyn crate::recall::VectorSearch>>,
    session_store: Option<Arc<TokioMutex<SessionStore>>>,
    #[cfg(feature = "knowledge-store")]
    knowledge_store: Option<Arc<KnowledgeStore>>,
    packs: Arc<Vec<LoadedPack>>,
    router: Option<Arc<crate::cross::CrossNousRouter>>,
    tool_services: Option<Arc<ToolServices>>,
    ready_tx: watch::Sender<bool>,
    ready_rx: watch::Receiver<bool>,
    /// Root cancellation token. Child tokens are given to each actor.
    /// Cancelling this stops all actors without needing `&mut self`.
    cancel: CancellationToken,
}

impl NousManager {
    /// Create a new manager with shared dependencies.
    #[must_use]
    #[expect(
        clippy::too_many_arguments,
        reason = "runtime init requires all shared dependencies"
    )]
    pub fn new(
        providers: Arc<ProviderRegistry>,
        tools: Arc<ToolRegistry>,
        oikos: Arc<Oikos>,
        embedding_provider: Option<Arc<dyn EmbeddingProvider>>,
        vector_search: Option<Arc<dyn crate::recall::VectorSearch>>,
        session_store: Option<Arc<TokioMutex<SessionStore>>>,
        #[cfg(feature = "knowledge-store")] knowledge_store: Option<Arc<KnowledgeStore>>,
        packs: Arc<Vec<LoadedPack>>,
        router: Option<Arc<crate::cross::CrossNousRouter>>,
        tool_services: Option<Arc<ToolServices>>,
    ) -> Self {
        let (ready_tx, ready_rx) = watch::channel(false);
        Self {
            actors: HashMap::new(),
            providers,
            tools,
            oikos,
            embedding_provider,
            vector_search,
            session_store,
            #[cfg(feature = "knowledge-store")]
            knowledge_store,
            packs,
            router,
            tool_services,
            ready_tx,
            ready_rx,
            cancel: CancellationToken::new(),
        }
    }

    /// Signal that all actors are spawned and the system is ready for inbound messages.
    pub fn ready(&self) {
        let _ = self.ready_tx.send(true);
        info!(count = self.actors.len(), "system ready");
    }

    /// Subscribe to the ready signal.
    #[must_use]
    pub fn ready_rx(&self) -> watch::Receiver<bool> {
        self.ready_rx.clone()
    }

    /// Access the cross-nous router, if configured.
    #[must_use]
    pub fn router(&self) -> Option<&Arc<crate::cross::CrossNousRouter>> {
        self.router.as_ref()
    }

    /// Spawn a new nous actor and return its handle.
    ///
    /// If an actor with the same id already exists, the old actor is shut down first.
    ///
    /// # Cancel safety
    ///
    /// Not cancel-safe. If cancelled after removing an old actor but before
    /// inserting the new entry, the old actor is lost. Only call during
    /// sequential startup, never in a `select!`.
    #[expect(
        clippy::expect_used,
        reason = "Mutex::lock is infallible under normal operation"
    )]
    pub async fn spawn(
        &mut self,
        config: NousConfig,
        pipeline_config: PipelineConfig,
    ) -> NousHandle {
        let id = config.id.clone();

        if let Some(old) = self.actors.remove(&id) {
            warn!(nous_id = %id, "replacing existing actor");
            let _ = old.handle.shutdown().await;
            // Take join handle before awaiting — must not hold MutexGuard across .await.
            let join_opt = old.join.lock().expect("join mutex not poisoned").take();
            if let Some(join) = join_opt {
                let _ = join.await;
            }
        }

        self.spawn_inner(config, pipeline_config).await
    }

    /// Internal spawn that does not check for existing actors.
    async fn spawn_inner(
        &mut self,
        config: NousConfig,
        pipeline_config: PipelineConfig,
    ) -> NousHandle {
        let id = config.id.clone();

        // Filter and convert domain pack sections for this agent (by ID or domain tags)
        let extra_bootstrap = {
            let estimator = CharEstimator;
            let mut sections = Vec::new();
            for pack in self.packs.iter() {
                let agent_sections = pack.sections_for_agent_or_domains(&id, &config.domains);
                sections.extend(pack_sections_to_bootstrap(&agent_sections, &estimator));
            }
            if !sections.is_empty() {
                info!(nous_id = %id, sections = sections.len(), "domain pack sections resolved");
            }
            sections
        };

        let cross_rx = if let Some(ref router) = self.router {
            let (cross_tx, cross_rx) = tokio::sync::mpsc::channel(32);
            router.register(&id, cross_tx).await;
            Some(cross_rx)
        } else {
            None
        };

        let child_cancel = self.cancel.child_token();
        let (handle, join_handle) = actor::spawn(
            config.clone(),
            pipeline_config.clone(),
            Arc::clone(&self.providers),
            Arc::clone(&self.tools),
            Arc::clone(&self.oikos),
            self.embedding_provider.clone(),
            self.vector_search.clone(),
            self.session_store.clone(),
            #[cfg(feature = "knowledge-store")]
            self.knowledge_store.clone(),
            self.tool_services.clone(),
            extra_bootstrap,
            cross_rx,
            child_cancel,
        );

        info!(nous_id = %id, "actor spawned");
        self.actors.insert(
            id,
            ActorEntry {
                handle: handle.clone(),
                join: Mutex::new(Some(join_handle)),
                config,
                pipeline_config,
                consecutive_misses: 0,
                restart_count: 0,
                last_start: std::time::Instant::now(),
            },
        );
        handle
    }

    /// Look up a handle by nous id.
    #[must_use]
    pub fn get(&self, nous_id: &str) -> Option<&NousHandle> {
        self.actors.get(nous_id).map(|e| &e.handle)
    }

    /// Look up a config by nous id.
    #[must_use]
    pub fn get_config(&self, nous_id: &str) -> Option<&NousConfig> {
        self.actors.get(nous_id).map(|e| &e.config)
    }

    /// All stored configs.
    #[must_use]
    pub fn configs(&self) -> Vec<&NousConfig> {
        self.actors.values().map(|e| &e.config).collect()
    }

    /// Check liveness of all actors by sending a ping with a timeout.
    ///
    /// Returns a map of `nous_id → ActorHealth`.
    pub async fn check_health(&self) -> BTreeMap<String, ActorHealth> {
        let mut results = BTreeMap::new();
        for (id, entry) in &self.actors {
            let ping_result = entry.handle.ping(DEFAULT_PING_TIMEOUT).await;
            let alive = ping_result.is_ok();

            // Try to get status for panic_count and uptime
            let (panic_count, uptime) = if let Ok(status) = entry.handle.status().await {
                (status.panic_count, status.uptime)
            } else {
                (0, entry.last_start.elapsed())
            };

            results.insert(
                id.clone(),
                ActorHealth {
                    alive,
                    panic_count,
                    uptime,
                },
            );
        }
        results
    }

    /// Run one health-check cycle: ping all actors, track misses, restart dead ones.
    ///
    /// Call this periodically from a background task.
    pub async fn health_cycle(&mut self) {
        let health = self.check_health().await;

        // Collect IDs that need restart to avoid borrow issues
        let mut to_restart: Vec<String> = Vec::new();

        for (id, actor_health) in &health {
            if let Some(entry) = self.actors.get_mut(id) {
                if actor_health.alive {
                    entry.consecutive_misses = 0;
                } else {
                    entry.consecutive_misses += 1;
                    if entry.consecutive_misses == 1 {
                        warn!(nous_id = %id, "actor missed health check");
                    }
                    if entry.consecutive_misses >= DEAD_THRESHOLD {
                        error!(
                            nous_id = %id,
                            misses = entry.consecutive_misses,
                            "actor declared dead — scheduling restart"
                        );
                        to_restart.push(id.clone());
                    }
                }
            }
        }

        for id in to_restart {
            self.restart_actor(&id).await;
        }
    }

    /// Restart a dead actor with exponential backoff.
    #[expect(
        clippy::expect_used,
        reason = "Mutex::lock is infallible under normal operation"
    )]
    async fn restart_actor(&mut self, id: &str) {
        let Some(entry) = self.actors.get(id) else {
            return;
        };

        let restart_count = entry.restart_count;
        let backoff = calculate_backoff(restart_count);

        info!(
            nous_id = %id,
            restart_count = restart_count + 1,
            backoff_secs = backoff.as_secs(),
            "restarting dead actor after backoff"
        );

        tokio::time::sleep(backoff).await;

        let config = entry.config.clone();
        let pipeline_config = entry.pipeline_config.clone();
        let prev_restart_count = entry.restart_count;

        // Remove old entry
        if let Some(old) = self.actors.remove(id) {
            // Take join handle before any await
            let join_opt = old.join.lock().expect("join mutex not poisoned").take();
            if let Some(join) = join_opt {
                // Don't wait too long for a dead actor
                let _ = tokio::time::timeout(Duration::from_secs(2), join).await;
            }
        }

        // Respawn
        let handle = self
            .spawn_inner(config.clone(), pipeline_config.clone())
            .await;

        // Update restart tracking
        if let Some(entry) = self.actors.get_mut(id) {
            entry.restart_count = prev_restart_count + 1;
            entry.consecutive_misses = 0;
        }

        info!(
            nous_id = %id,
            restart_count = prev_restart_count + 1,
            "actor restarted successfully"
        );

        drop(handle);
    }

    /// Spawn a background task that runs `health_cycle` on an interval.
    ///
    /// Returns a `JoinHandle` that can be used to track or cancel the poller.
    /// The poller stops when the cancellation token fires.
    pub fn start_health_poller(
        manager: Arc<TokioMutex<Self>>,
        interval: Duration,
        cancel: CancellationToken,
    ) -> JoinHandle<()> {
        tokio::spawn(
            async move {
                debug!(interval_secs = interval.as_secs(), "health poller started");
                loop {
                    tokio::select! {
                        () = tokio::time::sleep(interval) => {
                            let mut mgr = manager.lock().await;
                            mgr.health_cycle().await;
                        }
                        () = cancel.cancelled() => {
                            debug!("health poller cancelled");
                            break;
                        }
                    }
                }
            }
            .instrument(tracing::info_span!("health_poller")),
        )
    }

    /// Query status from all actors.
    ///
    /// # Cancel safety
    ///
    /// Cancel-safe. Partial results are discarded if cancelled; no state is
    /// mutated.
    pub async fn list(&self) -> Vec<NousStatus> {
        let mut statuses = Vec::with_capacity(self.actors.len());
        for entry in self.actors.values() {
            match entry.handle.status().await {
                Ok(status) => statuses.push(status),
                Err(_) => {
                    warn!(nous_id = entry.handle.id(), "failed to query actor status");
                }
            }
        }
        statuses
    }

    /// Gracefully shut down all actors.
    ///
    /// # Cancel safety
    ///
    /// Not cancel-safe. If cancelled after draining actors but before joining
    /// them, actor tasks may be leaked. Only call from shutdown paths.
    pub async fn shutdown_all(&mut self) {
        info!(count = self.actors.len(), "shutting down all actors");

        if let Some(ref router) = self.router {
            for id in self.actors.keys() {
                router.unregister(id).await;
            }
        }

        // Drain actors; collect handles and join handles for two-phase shutdown.
        let handles: Vec<(String, NousHandle)> = self
            .actors
            .iter()
            .map(|(id, e)| (id.clone(), e.handle.clone()))
            .collect();

        // Take all join handles before any await — must not hold MutexGuard across .await.
        let joins: Vec<(String, Option<JoinHandle<()>>)> = self
            .actors
            .drain()
            .map(|(id, e)| {
                let join = e.join.try_lock().ok().and_then(|mut g| g.take());
                (id, join)
            })
            .collect();

        for (id, handle) in &handles {
            if let Err(e) = handle.shutdown().await {
                warn!(nous_id = %id, error = %e, "failed to send shutdown");
            }
        }

        for (id, join_opt) in joins {
            if let Some(join) = join_opt
                && let Err(e) = join.await
            {
                warn!(nous_id = %id, error = %e, "actor task panicked");
            }
        }

        info!("all actors stopped");
    }

    /// Cancel all actors and wait for them to drain, bounded by a timeout.
    ///
    /// 1. Cancels the root token — all child actor tokens fire simultaneously.
    /// 2. Unregisters from the cross-nous router so no new messages arrive.
    /// 3. Awaits each actor join handle within the provided `timeout`.
    /// 4. On timeout, logs a warning and returns; Tokio will abort the tasks
    ///    when the runtime shuts down.
    ///
    /// This is the primary shutdown path when the manager is behind `Arc`
    /// and mutable access is unavailable. Awaiting join handles ensures that
    /// fjall WAL checkpoints and other finalisation complete before exit.
    ///
    /// # Cancel safety
    ///
    /// Cancel-safe. Token cancellation is idempotent; partial completion
    /// leaves remaining actors running until the runtime exits.
    pub async fn drain(&self, timeout: Duration) {
        info!(count = self.actors.len(), "draining all actors");

        // Notify all actors simultaneously via the shared root token.
        self.cancel.cancel();

        if let Some(ref router) = self.router {
            for id in self.actors.keys() {
                router.unregister(id).await;
            }
        }

        // Also send explicit Shutdown messages so actors waiting on inbox wake up.
        for entry in self.actors.values() {
            if let Err(e) = entry.handle.shutdown().await {
                warn!(nous_id = entry.handle.id(), error = %e, "failed to send shutdown");
            }
        }

        // Await all join handles within the timeout, so callers can guarantee
        // all Arc<KnowledgeStore> / fjall Database references are dropped.
        // Take handles first to avoid holding MutexGuard across .await points.
        let mut joins: Vec<(String, JoinHandle<()>)> = self
            .actors
            .iter()
            .filter_map(|(id, entry)| {
                let join = entry.join.try_lock().ok()?.take()?;
                Some((id.clone(), join))
            })
            .collect();

        let drain_fut = async move {
            for (id, join) in joins.drain(..) {
                if let Err(e) = join.await {
                    warn!(nous_id = %id, error = %e, "actor task panicked during drain");
                }
            }
        };

        if tokio::time::timeout(timeout, drain_fut).await.is_ok() {
            info!("all actors drained cleanly");
        } else {
            warn!(
                timeout_secs = timeout.as_secs(),
                "actor drain timed out — some actors may not have flushed state"
            );
        }
    }

    /// Send shutdown to all actors without requiring `&mut self`.
    ///
    /// Used when the manager is behind `Arc` and mutable access is unavailable.
    /// Does not drain the entries — cleanup happens when the `Arc` drops.
    /// Prefer [`drain`](Self::drain) when clean resource release is needed.
    ///
    /// # Cancel safety
    ///
    /// Cancel-safe. Each `shutdown` send is independent; partial completion
    /// leaves remaining actors running (they stop when the `Arc` drops).
    pub async fn shutdown_readonly(&self) {
        info!(count = self.actors.len(), "shutting down all actors");

        if let Some(ref router) = self.router {
            for id in self.actors.keys() {
                router.unregister(id).await;
            }
        }

        self.cancel.cancel();

        for entry in self.actors.values() {
            if let Err(e) = entry.handle.shutdown().await {
                warn!(nous_id = entry.handle.id(), error = %e, "failed to send shutdown");
            }
        }
    }

    /// Number of managed actors.
    #[must_use]
    pub fn count(&self) -> usize {
        self.actors.len()
    }

    /// Access the shared knowledge store, if configured.
    #[cfg(feature = "knowledge-store")]
    #[must_use]
    pub fn knowledge_store(&self) -> Option<&Arc<KnowledgeStore>> {
        self.knowledge_store.as_ref()
    }
}

/// Calculate exponential backoff: 5s, 15s, 45s, 2min, 5min cap.
fn calculate_backoff(restart_count: u32) -> Duration {
    let base_secs: u64 = 5;
    let multiplier = 3u64.saturating_pow(restart_count);
    let secs = base_secs.saturating_mul(multiplier);
    Duration::from_secs(secs).min(MAX_RESTART_BACKOFF)
}

#[cfg(test)]
#[path = "manager_tests.rs"]
mod tests;
