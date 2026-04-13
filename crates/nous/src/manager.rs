//! Manages all nous actor instances.

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
// WHY: lock held only during synchronous .take() on Option<JoinHandle>: no await while locked
use std::sync::Mutex; // kanon:ignore RUST/std-mutex-in-async
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use tokio::sync::Mutex as TokioMutex;
use tokio::sync::watch;
use tokio::task::{JoinHandle, JoinSet};
use tokio_util::sync::CancellationToken;
use tracing::{Instrument, debug, error, info, warn};

use hermeneus::provider::ProviderRegistry;
use mneme::embedding::EmbeddingProvider;
#[cfg(feature = "knowledge-store")]
use mneme::knowledge_store::KnowledgeStore;
use mneme::store::SessionStore;
use organon::registry::ToolRegistry;
use organon::types::ToolServices;
use taxis::oikos::Oikos;
use thesauros::loader::LoadedPack;

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
    /// When the actor was last restarted. `None` until first restart.
    /// Used to determine if `restart_count` should decay after stable operation.
    last_restart: Option<std::time::Instant>,
    /// Shared with the actor task. `true` while the actor is processing a turn.
    /// Readable without queuing through the inbox: used by `check_health` to
    /// distinguish a busy (healthy) actor from an unresponsive (dead) one.
    active_turn: Arc<AtomicBool>,
}


/// Manages the lifecycle of all nous actors.
// NOTE: 14 fields: runtime dependency injection (providers, tools, stores) plus
// actor state. Kept flat because splitting would scatter logically-paired fields.
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
    /// Deployment-level behavioral configuration (health intervals, restart limits).
    nous_behavior: taxis::config::NousBehaviorConfig,
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
        nous_behavior: taxis::config::NousBehaviorConfig,
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
            nous_behavior,
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
        let id = config.id.to_string();

        if let Some(old) = self.actors.remove(&id) {
            warn!(nous_id = %id, "replacing existing actor");
            let _ = old.handle.shutdown().await;
            // WHY: take join handle before awaiting: must not hold MutexGuard across .await
            let join_opt = old.join.lock().expect("join mutex not poisoned").take(); // kanon:ignore RUST/expect
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
        let id = config.id.to_string();

        let extra_bootstrap = {
            let estimator = CharEstimator::new(u64::from(config.generation.chars_per_token));
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
        let (handle, join_handle, active_turn) = actor::spawn(
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
            self.nous_behavior.clone(),
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
                last_restart: None,
                active_turn,
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
    /// An actor that does not respond to a ping but has its `active_turn` flag
    /// set is considered healthy-busy: it is processing a long turn (e.g. an
    /// LLM call) and cannot dequeue inbox messages until the turn completes.
    /// Only an actor that fails the ping **and** is not processing a turn is
    /// reported as dead.
    ///
    /// Returns a map of `nous_id → ActorHealth`.
    ///
    /// # Cancel safety
    ///
    /// Cancel-safe. Each ping and status query is independent. If cancelled
    /// mid-loop, partial results are discarded; no manager state is mutated.
    pub async fn check_health(&self) -> BTreeMap<String, ActorHealth> {
        let mut results = BTreeMap::new();
        let ping_timeout = Duration::from_secs(self.nous_behavior.manager_ping_timeout_secs);
        for (id, entry) in &self.actors {
            let ping_result = entry.handle.ping(ping_timeout).await;
            // WHY: An actor processing a long turn cannot dequeue Ping messages
            // until the turn completes. Treat active_turn=true as a liveness
            // signal so busy actors are not incorrectly declared dead.
            let alive = ping_result.is_ok() || entry.active_turn.load(Ordering::Acquire);

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
    ///
    /// # Cancel safety
    ///
    /// Not cancel-safe. If cancelled between `check_health` and `restart_actor`,
    /// miss counters may have been incremented without the corresponding restart
    /// being attempted. Only call from non-cancellable contexts (e.g. a dedicated
    /// poller task that never races with a cancellation signal).
    pub async fn health_cycle(&mut self) {
        let health = self.check_health().await;

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
                    if entry.consecutive_misses >= self.nous_behavior.manager_dead_threshold {
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

        let restart_decay_window =
            Duration::from_secs(self.nous_behavior.manager_restart_decay_window_secs);
        // Decay restart_count if actor has been stable since last restart
        let restart_count = if let Some(last_restart) = entry.last_restart {
            if last_restart.elapsed() >= restart_decay_window {
                0
            } else {
                entry.restart_count
            }
        } else {
            entry.restart_count
        };

        let backoff = calculate_backoff(
            restart_count,
            self.nous_behavior.manager_max_restart_backoff_secs,
        );
        tracing::debug!(
            nous_id = %id,
            restart_count,
            backoff_secs = backoff.as_secs(),
            manager_max_restart_backoff_secs = self.nous_behavior.manager_max_restart_backoff_secs,
            "restart_actor: calculated backoff"
        );

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

        if let Some(old) = self.actors.remove(id) {
            // WHY: take join handle before awaiting: must not hold MutexGuard across .await
            let join_opt = old.join.lock().expect("join mutex not poisoned").take(); // kanon:ignore RUST/expect
            if let Some(join) = join_opt {
                let restart_drain_timeout =
                    Duration::from_secs(self.nous_behavior.manager_restart_drain_timeout_secs);
                match tokio::time::timeout(restart_drain_timeout, join).await {
                    Ok(_) => {
                        tracing::debug!(nous_id = %id, "old actor drained cleanly before restart");
                    }
                    Err(_) => {
                        tracing::warn!(
                            nous_id = %id,
                            drain_timeout_secs = self.nous_behavior.manager_restart_drain_timeout_secs,
                            "actor did not drain within timeout, spawning replacement — concurrent store access possible"
                        );
                    }
                }
            }
        }

        let handle = self
            .spawn_inner(config.clone(), pipeline_config.clone())
            .await;

        if let Some(entry) = self.actors.get_mut(id) {
            entry.restart_count = restart_count + 1;
            entry.last_restart = Some(std::time::Instant::now());
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
                        // SAFETY: cancel-safe. `tokio::time::sleep` is cancel-safe:
                        // if dropped before it fires, the sleep is simply abandoned
                        // and a new one starts next iteration. The mutex lock and
                        // `health_cycle` call only run once the sleep completes.
                        () = tokio::time::sleep(interval) => {
                            let mut mgr = manager.lock().await;
                            mgr.health_cycle().await;
                        }
                        // SAFETY: cancel-safe. `CancellationToken::cancelled()` is
                        // cancel-safe; dropping it before it fires has no side effects.
                        () = cancel.cancelled() => {
                            debug!("health poller cancelled");
                            break;
                        }
                    }
                }
            }
            .instrument(tracing::info_span!(
                "health_poller",
                interval_secs = interval.as_secs()
            )),
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

        let handles: Vec<(String, NousHandle)> = self
            .actors
            .iter()
            .map(|(id, e)| (id.clone(), e.handle.clone()))
            .collect();

        // WHY: take all join handles before any await: must not hold MutexGuard across .await
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

        let mut join_set: JoinSet<(String, Result<(), tokio::task::JoinError>)> = JoinSet::new();
        for (id, join_opt) in joins {
            if let Some(join) = join_opt {
                join_set.spawn(async move { (id, join.await) });
            }
        }
        while let Some(result) = join_set.join_next().await {
            if let Ok((id, Err(e))) = result {
                warn!(nous_id = %id, error = %e, "actor task panicked");
            }
        }

        info!("all actors stopped");
    }

    /// Cancel all actors and wait for them to drain, bounded by a timeout.
    ///
    /// 1. Cancels the root token: all child actor tokens fire simultaneously.
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

        self.cancel.cancel();

        if let Some(ref router) = self.router {
            for id in self.actors.keys() {
                router.unregister(id).await;
            }
        }

        // WHY: explicit Shutdown messages wake actors blocking on inbox recv
        for entry in self.actors.values() {
            if let Err(e) = entry.handle.shutdown().await {
                warn!(nous_id = entry.handle.id(), error = %e, "failed to send shutdown");
            }
        }

        // WHY: take handles before await: must not hold MutexGuard across .await
        let mut join_set: JoinSet<(String, Result<(), tokio::task::JoinError>)> = JoinSet::new();
        for (id, entry) in &self.actors {
            if let Some(join) = entry.join.try_lock().ok().and_then(|mut g| g.take()) {
                let id = id.clone();
                join_set.spawn(async move { (id, join.await) });
            }
        }

        let drain_fut = async move {
            while let Some(result) = join_set.join_next().await {
                if let Ok((id, Err(e))) = result {
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
    /// Does not drain the entries: cleanup happens when the `Arc` drops.
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

/// Calculate exponential backoff: 5s, 15s, 45s, 2min, up to `max_secs` cap.
fn calculate_backoff(restart_count: u32, max_secs: u64) -> Duration {
    let base_secs: u64 = 5;
    let multiplier = 3u64.saturating_pow(restart_count);
    let secs = base_secs.saturating_mul(multiplier);
    Duration::from_secs(secs).min(Duration::from_secs(max_secs))
}

#[cfg(test)]
#[path = "manager_tests.rs"]
mod tests;
