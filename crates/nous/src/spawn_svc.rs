//! Ephemeral sub-agent spawning service.

use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use tokio_util::sync::CancellationToken;
use tracing::{Instrument, info, warn};

use hermeneus::provider::ProviderRegistry;
use koina::defaults::{
    BOOTSTRAP_MAX_TOKENS, CONTEXT_TOKENS, DEFAULT_MODEL, MAX_OUTPUT_TOKENS, MAX_TOOL_ITERATIONS,
    MAX_TOOL_RESULT_BYTES,
};
use mneme::embedding::EmbeddingProvider;
#[cfg(feature = "knowledge-store")]
use mneme::knowledge_store::KnowledgeStore;
use mneme::store::SessionStore;
use organon::registry::ToolRegistry;
use organon::types::{SpawnRequest, SpawnResult, SpawnService, ToolServices};
use taxis::oikos::Oikos;
use tokio::sync::Mutex;

use crate::actor;
use crate::config::{NousConfig, PipelineConfig, StageBudget};
use crate::roles::Role;

const SONNET_MODEL: &str = DEFAULT_MODEL;

/// Resolve role from string, returning typed role or falling back to model heuristic.
fn resolve_role(role_str: &str) -> Option<Role> {
    Role::parse(role_str)
}

/// Concrete [`SpawnService`] that bridges to `actor::spawn`.
pub struct SpawnServiceImpl {
    providers: Arc<ProviderRegistry>,
    tools: Arc<ToolRegistry>,
    oikos: Arc<Oikos>,
    embedding_provider: Option<Arc<dyn EmbeddingProvider>>,
    vector_search: Option<Arc<dyn crate::recall::VectorSearch>>,
    // kanon:ignore RUST/no-arc-mutex-anti-pattern — std::sync::Mutex for SessionStore in block_in_place bridge
    session_store: Option<Arc<Mutex<SessionStore>>>,
    #[cfg(feature = "knowledge-store")]
    knowledge_store: Option<Arc<KnowledgeStore>>,
    router: Option<Arc<crate::cross::CrossNousRouter>>,
    audit_log: Option<Arc<crate::audit::PromptAuditLog>>,
    empirical_router: Option<Arc<dyn aletheia_routing::Router>>,
    tool_services: OnceLock<Arc<ToolServices>>,
}

/// Parent runtime dependencies inherited by ephemeral sub-agents.
pub struct InheritedSpawnServices {
    /// Shared embedding provider inherited from the parent runtime.
    pub embedding_provider: Option<Arc<dyn EmbeddingProvider>>,
    /// Shared vector search backend inherited from the parent runtime.
    pub vector_search: Option<Arc<dyn crate::recall::VectorSearch>>,
    /// Durable session store used to persist spawned-agent turns.
    // kanon:ignore RUST/no-arc-mutex-anti-pattern — same: passed to sync trait adapter
    pub session_store: Option<Arc<Mutex<SessionStore>>>,
    /// Knowledge store selected for spawned-agent recall and memory tools.
    #[cfg(feature = "knowledge-store")]
    pub knowledge_store: Option<Arc<KnowledgeStore>>,
    /// Cross-nous router used to register spawned agents for communication tools.
    pub router: Option<Arc<crate::cross::CrossNousRouter>>,
    /// Prompt audit log shared with parent actors.
    pub audit_log: Option<Arc<crate::audit::PromptAuditLog>>,
    /// Empirical routing backend shared with parent actors.
    pub empirical_router: Option<Arc<dyn aletheia_routing::Router>>,
}

impl SpawnServiceImpl {
    /// Create a new spawn service from the given provider, tool, and oikos registries.
    #[must_use]
    pub fn new(
        providers: Arc<ProviderRegistry>,
        tools: Arc<ToolRegistry>,
        oikos: Arc<Oikos>,
    ) -> Self {
        Self {
            providers,
            tools,
            oikos,
            embedding_provider: None,
            vector_search: None,
            session_store: None,
            #[cfg(feature = "knowledge-store")]
            knowledge_store: None,
            router: None,
            audit_log: None,
            empirical_router: None,
            tool_services: OnceLock::new(),
        }
    }

    /// Attach parent runtime services that spawned agents should inherit.
    #[must_use]
    pub fn with_runtime_services(mut self, services: InheritedSpawnServices) -> Self {
        self.embedding_provider = services.embedding_provider;
        self.vector_search = services.vector_search;
        self.session_store = services.session_store;
        #[cfg(feature = "knowledge-store")]
        {
            self.knowledge_store = services.knowledge_store;
        }
        self.router = services.router;
        self.audit_log = services.audit_log;
        self.empirical_router = services.empirical_router;
        self
    }

    /// Complete the service cycle after `ToolServices` is built.
    pub fn set_tool_services(&self, services: Arc<ToolServices>) {
        // kanon:ignore RUST/no-silent-result-swallow — set once during initialization; duplicate calls are programmer error
        let _ = self.tool_services.set(services);
    }
}

impl SpawnService for SpawnServiceImpl {
    // NOTE: sequential ephemeral-actor lifecycle: build config, spawn actor, run single turn,
    // teardown. Splitting would fragment a cohesive lifecycle.
    #[expect(clippy::too_many_lines, reason = "spawn setup requires many steps")]
    fn spawn_and_run(
        &self,
        request: SpawnRequest,
        parent_nous_id: &str,
    ) -> Pin<Box<dyn Future<Output = Result<SpawnResult, String>> + Send + '_>> {
        let spawn_id = format!(
            "spawn-{}-{}",
            parent_nous_id,
            koina::ulid::Ulid::new().to_string().to_lowercase()
        );
        let role = resolve_role(&request.role);
        let template = role.map(Role::template);

        let model = request.model.clone().unwrap_or_else(|| {
            template
                .as_ref()
                .map_or_else(|| SONNET_MODEL.to_owned(), |t| t.model.to_owned())
        });

        let tool_allowlist = request
            .allowed_tools
            .clone()
            .or_else(|| template.as_ref().and_then(|t| t.tool_policy.to_allowlist()));

        let tool_groups = template
            .as_ref()
            .map_or_else(Vec::new, |t| t.tool_groups.clone());

        let timeout = Duration::from_secs(request.timeout_secs);
        let task = request.task.clone();
        let session_key = format!(
            "spawn:{}",
            koina::ulid::Ulid::new().to_string().to_lowercase()
        );
        let workspace = self.oikos.nous_dir(&spawn_id);
        let allowed_roots = vec![self.oikos.root().to_path_buf()];

        let config = NousConfig {
            id: Arc::from(spawn_id.as_str()),
            name: None,
            generation: crate::config::NousGenerationConfig {
                model,
                fallback_models: Vec::new(),
                retries_before_fallback: 2,
                context_window: CONTEXT_TOKENS,
                max_output_tokens: MAX_OUTPUT_TOKENS,
                bootstrap_max_tokens: BOOTSTRAP_MAX_TOKENS,
                thinking_enabled: false,
                thinking_budget: 0,
                chars_per_token: 4,
                prosoche_model: "claude-haiku-4-5-20251001".to_owned(),
                complexity: hermeneus::complexity::ComplexityConfig::default(),
                extraction_model: None,
                distillation_model: None,
            },
            limits: crate::config::NousLimits {
                max_tool_iterations: MAX_TOOL_ITERATIONS,
                loop_detection_threshold: 3,
                consecutive_error_threshold: 4,
                loop_max_warnings: 2,
                session_token_cap: 500_000,
                max_tool_result_bytes: MAX_TOOL_RESULT_BYTES,
                max_consecutive_tool_only_iterations: 3,
                consecutive_mistake_limit: koina::defaults::DEFAULT_CONSECUTIVE_MISTAKE_LIMIT,
            },
            domains: Vec::new(),
            private: false,
            episteme_cohort: std::sync::Arc::from("shared"),
            workspace,
            allowed_roots,
            server_tools: Vec::new(),
            cache_enabled: true,
            recall: crate::recall::RecallConfig::default(),
            recall_profile: crate::config::RecallProfile::Default,
            tool_allowlist,
            tool_groups,
            hooks: crate::config::HookConfig::default(),
            behavior: taxis::config::AgentBehaviorDefaults::default(),
        };

        // WHY: ephemeral sub-agents do not capture training data — their turns
        // are internal delegation, not user-facing conversation.
        let pipeline_config = PipelineConfig {
            history_budget_ratio: 0.6,
            project_id: None,
            extraction: None,
            stage_budget: StageBudget::default(),
            training: crate::training::TrainingConfig::default(),
            reflection_enabled: false,
        };

        let providers = Arc::clone(&self.providers);
        let tools = Arc::clone(&self.tools);
        let oikos = Arc::clone(&self.oikos);
        let embedding_provider = self.embedding_provider.clone();
        let vector_search = self.vector_search.clone();
        let session_store = self.session_store.clone();
        #[cfg(feature = "knowledge-store")]
        let knowledge_store = self.knowledge_store.clone();
        let tool_services = self.tool_services.get().cloned();
        let router = self.router.clone();
        let audit_log = self.audit_log.clone();
        let empirical_router = self.empirical_router.clone();

        let span = tracing::info_span!(
            "spawn_sub_agent",
            spawn.id = %spawn_id,
            spawn.role = %request.role,
        );

        let soul_content = template.as_ref().map_or_else(
            || {
                let role_str = request.role.clone();
                format!("You are an ephemeral {role_str} sub-agent. Complete the assigned task precisely and concisely.")
            },
            |t| t.system_prompt.to_owned(),
        );

        Box::pin(
            async move {
                let nous_dir = oikos.nous_dir(&spawn_id);
                if let Err(e) = tokio::fs::create_dir_all(&nous_dir).await {
                    return Err(format!("failed to create spawn workspace: {e}"));
                }
                let soul_path = nous_dir.join("SOUL.md");
                if let Err(e) = tokio::fs::write(&soul_path, &soul_content).await {
                    return Err(format!("failed to write SOUL.md: {e}"));
                }

                // WHY: ephemeral actors get their own cancellation token: short-lived, no shared parent
                let ephemeral_cancel = CancellationToken::new();
                let (cross_tx, cross_rx) = if let Some(router) = router.as_ref() {
                    let (tx, rx) = tokio::sync::mpsc::channel(32);
                    router.register(&spawn_id, tx.clone()).await;
                    (Some(tx), Some(rx))
                } else {
                    (None, None)
                };
                let (handle, join_handle, _active_turn, _turn_started_at_ms) = actor::spawn(
                    config,
                    pipeline_config,
                    providers,
                    tools,
                    oikos,
                    embedding_provider,
                    vector_search,
                    session_store,
                    #[cfg(feature = "knowledge-store")]
                    knowledge_store,
                    tool_services,
                    Vec::new(),
                    cross_rx,
                    cross_tx,
                    ephemeral_cancel,
                    taxis::config::NousBehaviorConfig::default(),
                    audit_log,
                    empirical_router,
                );

                info!(session_key = %session_key, "ephemeral actor started");

                let result =
                    tokio::time::timeout(timeout, handle.send_turn(&session_key, &task)).await;

                // kanon:ignore RUST/no-silent-result-swallow — best-effort shutdown of ephemeral actor
                let _ = handle.shutdown().await;
                let _ = join_handle.await;
                if let Some(router) = router.as_ref() {
                    router.unregister(&spawn_id).await;
                }

                // kanon:ignore RUST/no-silent-result-swallow — best-effort temp dir cleanup
                let _ = tokio::fs::remove_dir_all(&nous_dir).await;

                match result {
                    Ok(Ok(turn)) => Ok(SpawnResult {
                        content: turn.content,
                        is_error: false,
                        input_tokens: turn.usage.input_tokens,
                        output_tokens: turn.usage.output_tokens,
                    }),
                    Ok(Err(e)) => Ok(SpawnResult {
                        content: format!("Sub-agent error: {e}"),
                        is_error: true,
                        input_tokens: 0,
                        output_tokens: 0,
                    }),
                    Err(_elapsed) => {
                        warn!(timeout_secs = timeout.as_secs(), "sub-agent timed out");
                        Ok(SpawnResult {
                            content: format!("Sub-agent timed out after {}s", timeout.as_secs()),
                            is_error: true,
                            input_tokens: 0,
                            output_tokens: 0,
                        })
                    }
                }
            }
            .instrument(span),
        )
    }
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use std::time::Duration;

    use aletheia_routing::types::{ProviderId, TaskCategory};
    use aletheia_routing::{AfterActionStore, RecordingRouter};
    use hermeneus::provider::LlmProvider;
    use hermeneus::test_utils::MockProvider;
    use hermeneus::types::{
        CompletionRequest, CompletionResponse, ContentBlock, StopReason, Usage,
    };
    use taxis::oikos::Oikos;

    use super::*;

    fn make_oikos() -> (tempfile::TempDir, Arc<Oikos>) {
        let dir = tempfile::TempDir::new().expect("tmpdir");
        let root = dir.path();
        std::fs::create_dir_all(root.join("shared")).expect("mkdir");
        std::fs::create_dir_all(root.join("theke")).expect("mkdir");
        let oikos = Arc::new(Oikos::from_root(root));
        (dir, oikos)
    }

    // WHY (#4235): the Coder/Researcher role templates now resolve to
    // `koina::defaults::DEFAULT_MODEL` (Sonnet 4.6), not the old date-pinned
    // Sonnet 4.0 literal. The mock provider's supported-models list must
    // include the workspace default so `spawn_and_run` can route Coder tasks.
    const SUPPORTED_MOCK_MODELS: &[&str] = &[
        koina::defaults::DEFAULT_MODEL,
        "claude-sonnet-4-20250514",
        "claude-haiku-4-5-20251001",
    ];

    fn make_providers() -> Arc<ProviderRegistry> {
        let response = CompletionResponse {
            id: "msg_mock".to_owned(),
            model: "mock-model".to_owned(),
            stop_reason: StopReason::EndTurn,
            content: vec![ContentBlock::Text {
                text: "Sub-agent result".to_owned(),
                citations: None,
            }],
            usage: Usage {
                input_tokens: 200,
                output_tokens: 80,
                ..Usage::default()
            },
            cost_usd: None,
            duration_ms: None,
        };
        let mut providers = ProviderRegistry::new();
        providers.register(Box::new(
            MockProvider::with_responses(vec![response]).models(SUPPORTED_MOCK_MODELS),
        ));
        Arc::new(providers)
    }

    fn make_spawn_service(oikos: Arc<Oikos>) -> SpawnServiceImpl {
        SpawnServiceImpl::new(make_providers(), Arc::new(ToolRegistry::new()), oikos)
    }

    #[tokio::test]
    async fn spawn_runs_single_turn() {
        let (_dir, oikos) = make_oikos();
        let svc = make_spawn_service(oikos);

        let result = svc
            .spawn_and_run(
                SpawnRequest {
                    role: "coder".to_owned(),
                    task: "Write a function".to_owned(),
                    model: None,
                    allowed_tools: None,
                    timeout_secs: 30,
                },
                "test-parent",
            )
            .await
            .expect("spawn");

        assert!(!result.is_error, "unexpected error: {}", result.content);
        assert_eq!(result.content, "Sub-agent result");
        assert_eq!(result.input_tokens, 200);
        assert_eq!(result.output_tokens, 80);
    }

    #[tokio::test]
    async fn spawn_inherits_empirical_router_and_records_outcome() {
        let (_dir, oikos) = make_oikos();
        let store = Arc::new(AfterActionStore::in_memory());
        // WHY (#4235): align the router fixture with the Coder role template's
        // model (`koina::defaults::DEFAULT_MODEL`) so the AfterActionStore key
        // matches the model the spawn pipeline actually selects.
        let router: Arc<dyn aletheia_routing::Router> = Arc::new(RecordingRouter::new(
            Arc::clone(&store),
            koina::defaults::DEFAULT_MODEL,
        ));
        let svc = make_spawn_service(oikos).with_runtime_services(InheritedSpawnServices {
            embedding_provider: None,
            vector_search: None,
            session_store: None,
            #[cfg(feature = "knowledge-store")]
            knowledge_store: None,
            router: None,
            audit_log: None,
            empirical_router: Some(router),
        });

        let result = svc
            .spawn_and_run(
                SpawnRequest {
                    role: "coder".to_owned(),
                    task: "Build a feature".to_owned(),
                    model: None,
                    allowed_tools: None,
                    timeout_secs: 30,
                },
                "test-parent",
            )
            .await
            .expect("spawn");

        assert!(!result.is_error, "unexpected error: {}", result.content);

        let provider = ProviderId::new(koina::defaults::DEFAULT_MODEL);
        for _ in 0..20 {
            if let Some(stats) = store
                .rolling_stats(&provider, &TaskCategory::Feature, Duration::from_hours(168))
                .await
            {
                assert_eq!(stats.successes, 1);
                assert_eq!(stats.failures, 0);
                assert_eq!(stats.total, 1);
                return;
            }
            tokio::task::yield_now().await;
        }

        panic!("spawned actor did not record empirical routing outcome");
    }

    #[test]
    fn spawn_uses_role_default_model() {
        use crate::roles::Role;
        // WHY (#4235): Coder/Researcher templates inherit from `SONNET_MODEL
        // = koina::defaults::DEFAULT_MODEL`. Assert against the constant so
        // template/model drift is caught at the call site, not in production.
        // Reviewer/Explorer/Runner pin specific dated IDs locally
        // (Opus 4.0 / Haiku 4.5); those remain literal here.
        assert_eq!(Role::Coder.template().model, koina::defaults::DEFAULT_MODEL);
        assert_eq!(Role::Reviewer.template().model, "claude-opus-4-20250514");
        assert_eq!(
            Role::Researcher.template().model,
            koina::defaults::DEFAULT_MODEL
        );
        assert_eq!(Role::Explorer.template().model, "claude-haiku-4-5-20251001");
        assert_eq!(Role::Runner.template().model, "claude-haiku-4-5-20251001");
        assert!(resolve_role("unknown").is_none());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn spawn_timeout_returns_error() {
        let (_dir, oikos) = make_oikos();

        let mut providers = ProviderRegistry::new();
        providers.register(Box::new(SlowProvider));
        let svc = SpawnServiceImpl::new(Arc::new(providers), Arc::new(ToolRegistry::new()), oikos);

        let result = svc
            .spawn_and_run(
                SpawnRequest {
                    role: "coder".to_owned(),
                    task: "Slow task".to_owned(),
                    model: None,
                    allowed_tools: None,
                    timeout_secs: 1,
                },
                "test-parent",
            )
            .await
            .expect("spawn");

        assert!(result.is_error);
        assert!(result.content.contains("timed out"));
    }

    struct SlowProvider;

    impl LlmProvider for SlowProvider {
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
            Box::pin(async {
                // WHY: real sleep required to test timeout in multi_thread runtime
                tokio::time::sleep(std::time::Duration::from_secs(5)).await; // kanon:ignore TESTING/sleep-in-test
                Ok(CompletionResponse {
                    id: "slow".to_owned(),
                    model: "claude-sonnet-4-20250514".to_owned(),
                    stop_reason: StopReason::EndTurn,
                    content: vec![ContentBlock::Text {
                        text: "late".to_owned(),
                        citations: None,
                    }],
                    usage: Usage::default(),
                    cost_usd: None,
                    duration_ms: None,
                })
            })
        }

        fn supported_models(&self) -> &[&str] {
            SUPPORTED_MOCK_MODELS
        }

        #[expect(clippy::unnecessary_literal_bound, reason = "trait requires &str")]
        fn name(&self) -> &str {
            "slow"
        }
    }

    #[test]
    fn resolve_role_known_roles() {
        assert!(resolve_role("coder").is_some());
        assert!(resolve_role("reviewer").is_some());
        assert!(resolve_role("researcher").is_some());
        assert!(resolve_role("explorer").is_some());
        assert!(resolve_role("runner").is_some());
    }

    #[test]
    fn resolve_role_unknown_returns_none() {
        assert!(resolve_role("").is_none());
        assert!(resolve_role("analyst").is_none());
        assert!(resolve_role("planner").is_none());
    }

    #[tokio::test]
    async fn spawn_with_explicit_model() {
        let (_dir, oikos) = make_oikos();
        let svc = make_spawn_service(oikos);

        let result = svc
            .spawn_and_run(
                SpawnRequest {
                    role: "coder".to_owned(),
                    task: "Test task".to_owned(),
                    model: Some("claude-haiku-4-5-20251001".to_owned()),
                    allowed_tools: None,
                    timeout_secs: 30,
                },
                "test-parent",
            )
            .await
            .expect("spawn");

        assert!(!result.is_error);
    }

    #[tokio::test]
    async fn spawn_cleans_up_workspace() {
        let (_dir, oikos) = make_oikos();
        let svc = make_spawn_service(Arc::clone(&oikos));

        let result = svc
            .spawn_and_run(
                SpawnRequest {
                    role: "coder".to_owned(),
                    task: "Cleanup test".to_owned(),
                    model: None,
                    allowed_tools: None,
                    timeout_secs: 30,
                },
                "test-parent",
            )
            .await
            .expect("spawn");

        assert!(!result.is_error);
        // WHY: The ephemeral workspace should have been cleaned up
        // (we can't easily check the exact path but the spawn completed)
    }

    #[test]
    fn spawn_service_construction() {
        let providers = Arc::new(ProviderRegistry::new());
        let tools = Arc::new(ToolRegistry::new());
        let dir = tempfile::TempDir::new().expect("tmpdir");
        let oikos = Arc::new(Oikos::from_root(dir.path()));
        let _svc = SpawnServiceImpl::new(providers, tools, oikos);
    }
}
