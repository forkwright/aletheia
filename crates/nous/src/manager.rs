//! Manages all nous actor instances.

use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::Mutex;

#[cfg(feature = "knowledge-store")]
use aletheia_mneme::knowledge_store::KnowledgeStore;
use aletheia_mneme::store::SessionStore;
use aletheia_thesauros::loader::LoadedPack;

use std::time::Duration;

use tokio::sync::watch;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use tracing::{info, warn};

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
use crate::message::NousStatus;

struct ActorEntry {
    handle: NousHandle,
    /// Wrapped in `Mutex<Option<_>>` so `shutdown_readonly` can take the handle
    /// from a shared reference, await it, and confirm the actor has stopped.
    join: Mutex<Option<JoinHandle<()>>>,
    config: NousConfig,
}

/// Manages the lifecycle of all nous actors.
pub struct NousManager {
    actors: HashMap<String, ActorEntry>,
    providers: Arc<ProviderRegistry>,
    tools: Arc<ToolRegistry>,
    oikos: Arc<Oikos>,
    embedding_provider: Option<Arc<dyn EmbeddingProvider>>,
    vector_search: Option<Arc<dyn crate::recall::VectorSearch>>,
    session_store: Option<Arc<Mutex<SessionStore>>>,
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
        session_store: Option<Arc<Mutex<SessionStore>>>,
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
            let join_opt = old.join.lock().ok().and_then(|mut g| g.take());
            if let Some(join) = join_opt {
                let _ = join.await;
            }
        }

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
            pipeline_config,
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
                let join = e.join.lock().ok().and_then(|mut g| g.take());
                (id, join)
            })
            .collect();

        for (id, handle) in &handles {
            if let Err(e) = handle.shutdown().await {
                warn!(nous_id = %id, error = %e, "failed to send shutdown");
            }
        }

        for (id, join_opt) in joins {
            if let Some(join) = join_opt {
                if let Err(e) = join.await {
                    warn!(nous_id = %id, error = %e, "actor task panicked");
                }
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
    /// redb WAL checkpoints and other finalisation complete before exit.
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
        // all Arc<KnowledgeStore> / redb Database references are dropped.
        // Take handles first to avoid holding MutexGuard across .await points.
        let mut joins: Vec<(String, JoinHandle<()>)> = self
            .actors
            .iter()
            .filter_map(|(id, entry)| {
                let join = entry.join.lock().ok()?.take()?;
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
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use aletheia_hermeneus::provider::LlmProvider;
    use aletheia_hermeneus::types::{
        CompletionRequest, CompletionResponse, ContentBlock, StopReason, Usage,
    };

    use super::*;
    use crate::message::NousLifecycle;

    struct MockProvider {
        // std::sync::Mutex is intentional — test mock, never crosses .await
        response: Mutex<CompletionResponse>,
    }

    impl LlmProvider for MockProvider {
        fn complete(
            &self,
            _request: &CompletionRequest,
        ) -> aletheia_hermeneus::error::Result<CompletionResponse> {
            #[expect(
                clippy::expect_used,
                reason = "test mock: poisoned lock means a test bug"
            )]
            Ok(self.response.lock().expect("lock poisoned").clone())
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

    fn test_oikos() -> (tempfile::TempDir, Arc<Oikos>) {
        let dir = tempfile::TempDir::new().expect("tmpdir");
        let root = dir.path();
        std::fs::create_dir_all(root.join("nous/syn")).expect("mkdir");
        std::fs::create_dir_all(root.join("nous/demiurge")).expect("mkdir");
        std::fs::create_dir_all(root.join("shared")).expect("mkdir");
        std::fs::create_dir_all(root.join("theke")).expect("mkdir");
        std::fs::write(root.join("nous/syn/SOUL.md"), "I am Syn.").expect("write");
        std::fs::write(root.join("nous/demiurge/SOUL.md"), "I am Demiurge.").expect("write");
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
                    text: "Hello!".to_owned(),
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

    fn test_manager(oikos: Arc<Oikos>) -> NousManager {
        NousManager::new(
            test_providers(),
            Arc::new(ToolRegistry::new()),
            oikos,
            None,
            None,
            None,
            #[cfg(feature = "knowledge-store")]
            None,
            Arc::new(Vec::new()),
            None,
            None,
        )
    }

    fn syn_config() -> NousConfig {
        NousConfig {
            id: "syn".to_owned(),
            model: "test-model".to_owned(),
            ..NousConfig::default()
        }
    }

    fn demiurge_config() -> NousConfig {
        NousConfig {
            id: "demiurge".to_owned(),
            model: "test-model".to_owned(),
            ..NousConfig::default()
        }
    }

    #[tokio::test]
    async fn spawn_returns_handle() {
        let (_dir, oikos) = test_oikos();
        let mut mgr = test_manager(oikos);

        let handle = mgr.spawn(syn_config(), PipelineConfig::default()).await;
        assert_eq!(handle.id(), "syn");
        assert_eq!(mgr.count(), 1);

        mgr.shutdown_all().await;
    }

    #[tokio::test]
    async fn get_finds_spawned_actor() {
        let (_dir, oikos) = test_oikos();
        let mut mgr = test_manager(oikos);

        mgr.spawn(syn_config(), PipelineConfig::default()).await;

        let handle = mgr.get("syn").expect("found");
        assert_eq!(handle.id(), "syn");

        mgr.shutdown_all().await;
    }

    #[tokio::test]
    async fn get_returns_none_for_unknown() {
        let (_dir, oikos) = test_oikos();
        let mgr = test_manager(oikos);
        assert!(mgr.get("unknown").is_none());
    }

    #[tokio::test]
    async fn get_config_returns_stored_config() {
        let (_dir, oikos) = test_oikos();
        let mut mgr = test_manager(oikos);

        mgr.spawn(syn_config(), PipelineConfig::default()).await;

        let config = mgr.get_config("syn").expect("config");
        assert_eq!(config.id, "syn");
        assert_eq!(config.model, "test-model");

        mgr.shutdown_all().await;
    }

    #[tokio::test]
    async fn get_config_returns_none_for_unknown() {
        let (_dir, oikos) = test_oikos();
        let mgr = test_manager(oikos);
        assert!(mgr.get_config("unknown").is_none());
    }

    #[tokio::test]
    async fn configs_returns_all() {
        let (_dir, oikos) = test_oikos();
        let mut mgr = test_manager(oikos);

        mgr.spawn(syn_config(), PipelineConfig::default()).await;
        mgr.spawn(demiurge_config(), PipelineConfig::default())
            .await;

        let configs = mgr.configs();
        assert_eq!(configs.len(), 2);

        let ids: Vec<&str> = configs.iter().map(|c| c.id.as_str()).collect();
        assert!(ids.contains(&"syn"));
        assert!(ids.contains(&"demiurge"));

        mgr.shutdown_all().await;
    }

    #[tokio::test]
    async fn list_returns_all_statuses() {
        let (_dir, oikos) = test_oikos();
        let mut mgr = test_manager(oikos);

        mgr.spawn(syn_config(), PipelineConfig::default()).await;
        mgr.spawn(demiurge_config(), PipelineConfig::default())
            .await;

        let statuses = mgr.list().await;
        assert_eq!(statuses.len(), 2);

        let ids: Vec<&str> = statuses.iter().map(|s| s.id.as_str()).collect();
        assert!(ids.contains(&"syn"));
        assert!(ids.contains(&"demiurge"));

        mgr.shutdown_all().await;
    }

    #[tokio::test]
    async fn shutdown_all_stops_all_actors() {
        let (_dir, oikos) = test_oikos();
        let mut mgr = test_manager(oikos);

        let handle1 = mgr.spawn(syn_config(), PipelineConfig::default()).await;
        let handle2 = mgr
            .spawn(demiurge_config(), PipelineConfig::default())
            .await;

        mgr.shutdown_all().await;

        assert_eq!(mgr.count(), 0);
        assert!(handle1.status().await.is_err());
        assert!(handle2.status().await.is_err());
    }

    #[tokio::test]
    async fn spawn_multiple_actors() {
        let (_dir, oikos) = test_oikos();
        let mut mgr = test_manager(oikos);

        mgr.spawn(syn_config(), PipelineConfig::default()).await;
        mgr.spawn(demiurge_config(), PipelineConfig::default())
            .await;

        assert_eq!(mgr.count(), 2);

        let syn = mgr.get("syn").expect("syn");
        let dem = mgr.get("demiurge").expect("demiurge");

        let s1 = syn.status().await.expect("status");
        let s2 = dem.status().await.expect("status");
        assert_eq!(s1.lifecycle, NousLifecycle::Idle);
        assert_eq!(s2.lifecycle, NousLifecycle::Idle);

        mgr.shutdown_all().await;
    }

    #[tokio::test]
    async fn spawn_replaces_existing_actor() {
        let (_dir, oikos) = test_oikos();
        let mut mgr = test_manager(oikos);

        let old_handle = mgr.spawn(syn_config(), PipelineConfig::default()).await;
        let new_handle = mgr.spawn(syn_config(), PipelineConfig::default()).await;

        assert_eq!(mgr.count(), 1);

        // Old handle should be disconnected
        assert!(old_handle.status().await.is_err());

        // New handle should work
        let status = new_handle.status().await.expect("status");
        assert_eq!(status.id, "syn");

        mgr.shutdown_all().await;
    }

    #[tokio::test]
    async fn manager_turn_through_handle() {
        let (_dir, oikos) = test_oikos();
        let mut mgr = test_manager(oikos);

        let handle = mgr.spawn(syn_config(), PipelineConfig::default()).await;
        let result = handle.send_turn("main", "Hello").await.expect("turn");
        assert_eq!(result.content, "Hello!");

        mgr.shutdown_all().await;
    }

    /// `drain()` cancels all actors via the root token and awaits their exit.
    #[tokio::test]
    async fn drain_stops_all_actors() {
        let (_dir, oikos) = test_oikos();
        let mut mgr = test_manager(oikos);

        let handle1 = mgr.spawn(syn_config(), PipelineConfig::default()).await;
        let handle2 = mgr
            .spawn(demiurge_config(), PipelineConfig::default())
            .await;

        // drain() takes &self — no mutable access needed.
        mgr.drain(Duration::from_secs(5)).await;

        // After drain join handles are taken and tasks have stopped.
        assert!(handle1.status().await.is_err(), "syn actor should have exited");
        assert!(
            handle2.status().await.is_err(),
            "demiurge actor should have exited"
        );
    }

    /// Cancelling the manager's root token reaches all actor child tokens.
    #[tokio::test]
    async fn cancel_token_propagates_to_actors() {
        let (_dir, oikos) = test_oikos();
        let mut mgr = test_manager(oikos);

        let handle = mgr.spawn(syn_config(), PipelineConfig::default()).await;

        // Cancel via manager's root token directly (as drain() would do internally).
        mgr.cancel.cancel();

        // Wait for actor to observe cancellation and exit.
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if handle.status().await.is_err() {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("actor should stop when cancel token fires");
    }

    /// `drain()` with a very short timeout should warn and return, not panic.
    #[tokio::test]
    async fn drain_timeout_does_not_panic() {
        let (_dir, oikos) = test_oikos();
        let mut mgr = test_manager(oikos);

        mgr.spawn(syn_config(), PipelineConfig::default()).await;
        mgr.spawn(demiurge_config(), PipelineConfig::default())
            .await;

        // 1-nanosecond timeout: drain will warn but must not panic.
        mgr.drain(Duration::from_nanos(1)).await;
    }
}
