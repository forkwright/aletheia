//! Manages all nous actor instances.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use aletheia_mneme::store::SessionStore;
use aletheia_thesauros::loader::LoadedPack;

use tokio::task::JoinHandle;
use tracing::{info, warn};

use aletheia_hermeneus::provider::ProviderRegistry;
use aletheia_mneme::embedding::EmbeddingProvider;
use aletheia_organon::registry::ToolRegistry;
use aletheia_taxis::oikos::Oikos;

use crate::actor;
use crate::bootstrap::pack_sections_to_bootstrap;
use crate::budget::CharEstimator;
use crate::config::{NousConfig, PipelineConfig};
use crate::handle::NousHandle;
use crate::message::NousStatus;

struct ActorEntry {
    handle: NousHandle,
    join: JoinHandle<()>,
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
    packs: Arc<Vec<LoadedPack>>,
}

impl NousManager {
    /// Create a new manager with shared dependencies.
    #[must_use]
    pub fn new(
        providers: Arc<ProviderRegistry>,
        tools: Arc<ToolRegistry>,
        oikos: Arc<Oikos>,
        embedding_provider: Option<Arc<dyn EmbeddingProvider>>,
        vector_search: Option<Arc<dyn crate::recall::VectorSearch>>,
        session_store: Option<Arc<Mutex<SessionStore>>>,
        packs: Arc<Vec<LoadedPack>>,
    ) -> Self {
        Self {
            actors: HashMap::new(),
            providers,
            tools,
            oikos,
            embedding_provider,
            vector_search,
            session_store,
            packs,
        }
    }

    /// Spawn a new nous actor and return its handle.
    ///
    /// If an actor with the same id already exists, the old actor is shut down first.
    pub async fn spawn(
        &mut self,
        config: NousConfig,
        pipeline_config: PipelineConfig,
    ) -> NousHandle {
        let id = config.id.clone();

        if let Some(old) = self.actors.remove(&id) {
            warn!(nous_id = %id, "replacing existing actor");
            let _ = old.handle.shutdown().await;
            let _ = old.join.await;
        }

        // Filter and convert domain pack sections for this agent (by ID or domain tags)
        let extra_bootstrap = {
            let estimator = CharEstimator;
            let mut sections = Vec::new();
            for pack in self.packs.iter() {
                let agent_sections =
                    pack.sections_for_agent_or_domains(&id, &config.domains);
                sections.extend(pack_sections_to_bootstrap(&agent_sections, &estimator));
            }
            if !sections.is_empty() {
                info!(nous_id = %id, sections = sections.len(), "domain pack sections resolved");
            }
            sections
        };

        let (handle, join_handle) = actor::spawn(
            config.clone(),
            pipeline_config,
            Arc::clone(&self.providers),
            Arc::clone(&self.tools),
            Arc::clone(&self.oikos),
            self.embedding_provider.clone(),
            self.vector_search.clone(),
            self.session_store.clone(),
            extra_bootstrap,
        );

        info!(nous_id = %id, "actor spawned");
        self.actors.insert(
            id,
            ActorEntry {
                handle: handle.clone(),
                join: join_handle,
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
    pub async fn shutdown_all(&mut self) {
        info!(count = self.actors.len(), "shutting down all actors");

        let entries: Vec<(String, NousHandle, JoinHandle<()>)> = self
            .actors
            .drain()
            .map(|(id, e)| (id, e.handle, e.join))
            .collect();

        for (id, handle, _) in &entries {
            if let Err(e) = handle.shutdown().await {
                warn!(nous_id = %id, error = %e, "failed to send shutdown");
            }
        }

        for (id, _, join) in entries {
            if let Err(e) = join.await {
                warn!(nous_id = %id, error = %e, "actor task panicked");
            }
        }

        info!("all actors stopped");
    }

    /// Send shutdown to all actors without requiring `&mut self`.
    ///
    /// Used when the manager is behind `Arc` and mutable access is unavailable.
    /// Does not drain the entries — cleanup happens when the `Arc` drops.
    pub async fn shutdown_readonly(&self) {
        info!(count = self.actors.len(), "shutting down all actors");
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
            Arc::new(Vec::new()),
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
}
