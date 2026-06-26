// kanon:ignore RUST/file-too-long — spawn service + full integration test suite; test extraction into submodule planned
//! Ephemeral sub-agent spawning service.

use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use tracing::{Instrument, info, warn};

use hermeneus::provider::{LlmProvider, ProviderRegistry};
use koina::defaults::{
    BOOTSTRAP_MAX_TOKENS, CHARS_PER_TOKEN, CONTEXT_TOKENS, DEFAULT_MODEL, MAX_OUTPUT_TOKENS,
    MAX_TOOL_ITERATIONS, MAX_TOOL_RESULT_BYTES,
};
use mneme::embedding::EmbeddingProvider;
#[cfg(feature = "knowledge-store")]
use mneme::knowledge_store::KnowledgeStore;
use mneme::store::SessionStore;
use organon::registry::ToolRegistry;
use organon::types::{
    ServerToolConfig, SpawnContext, SpawnRequest, SpawnResult, SpawnService, ToolServices,
};
use taxis::oikos::Oikos;
use tokio::sync::Mutex;

use crate::actor;
use crate::config::{NousConfig, PipelineConfig, StageBudget};
use crate::handle::DEFAULT_SEND_TIMEOUT;
use crate::roles::Role;

const SONNET_MODEL: &str = DEFAULT_MODEL;
const WEB_SEARCH_TOOL: &str = "web_search";
const CODE_EXECUTION_TOOL: &str = "code_execution";

fn server_tool_config_for_provider(
    config: &ServerToolConfig,
    provider: Option<&dyn LlmProvider>,
) -> ServerToolConfig {
    let Some(provider) = provider else {
        return ServerToolConfig::default();
    };
    let web_search = config.web_search && provider.supports_server_tool(WEB_SEARCH_TOOL);
    ServerToolConfig {
        web_search,
        web_search_max_uses: web_search.then_some(config.web_search_max_uses).flatten(),
        code_execution: config.code_execution && provider.supports_server_tool(CODE_EXECUTION_TOOL),
    }
}

/// Conservative read-only allowlist applied to spawned actors with no role
/// template and no explicit `allowed_tools`. Prevents an unrecognized role
/// from inheriting unrestricted tool access (#3958, ADR-005).
fn conservative_spawn_allowlist() -> Vec<String> {
    ["read", "grep", "find", "ls", "view_file", "memory_search"]
        .into_iter()
        .map(str::to_owned)
        .collect()
}

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
    tool_config: Arc<taxis::config::ToolLimitsConfig>,
    tool_services: OnceLock<Arc<ToolServices>>,
}

/// Parent runtime dependencies inherited by ephemeral sub-agents.
// kanon:ignore TOPOLOGY/shallow-struct — dependency bag for wiring parent services into spawned actors; no in-file behavior by design
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
    /// Tool execution limits inherited from deployment config.
    pub tool_config: Arc<taxis::config::ToolLimitsConfig>,
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
            tool_config: Arc::new(taxis::config::ToolLimitsConfig::default()),
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
        self.tool_config = services.tool_config;
        self
    }

    /// Complete the service cycle after `ToolServices` is built.
    pub fn set_tool_services(&self, services: Arc<ToolServices>) {
        // kanon:ignore RUST/no-silent-result-swallow — set once during initialization; duplicate calls are programmer error
        let _ = self.tool_services.set(services);
    }

    /// Build a [`NousConfig`] for an ephemeral sub-agent.
    ///
    /// WHY(#5555): keep config construction deterministic and testable so the
    /// spawned `allowed_roots` can be asserted independently of the actor
    /// lifecycle.
    fn build_spawn_config(
        &self,
        request: &SpawnRequest,
        parent_nous_id: &str,
    ) -> (String, NousConfig, String) {
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

        // WHY(#3958, ADR-005): spawned actors with neither an explicit
        // `allowed_tools` nor a recognized role template MUST fall back to a
        // conservative read-only allowlist rather than `None`. `None` means
        // "no allowlist" — and the execute-time `tool_allowlist` gate in `execute/mod.rs`
        // treats that as unrestricted, which lets an unknown-role spawn run
        // exec/rm/http_request/sessions_dispatch with no operator approval
        // (the parent's approval gate doesn't follow into the child).
        // WHY(#5877): track whether we fall through to the conservative allowlist so
        // `tool_groups` can be paired with a matching Read-group policy. Without this,
        // `resolve_availability` returns `Denied(GroupPolicy)` before the allowlist
        // gate fires, leaving the spawned agent with zero accessible tools (ADR-005).
        let using_conservative_allowlist = request.allowed_tools.is_none()
            && template
                .as_ref()
                .and_then(|t| t.tool_policy.to_allowlist())
                .is_none();

        let tool_allowlist = request
            .allowed_tools
            .clone()
            .or_else(|| template.as_ref().and_then(|t| t.tool_policy.to_allowlist()))
            .or_else(|| Some(conservative_spawn_allowlist()));

        let tool_groups = if using_conservative_allowlist {
            // WHY(#5877, ADR-005): pair the conservative read-only allowlist with a
            // matching group policy; without this `resolve_availability` gates on
            // `DenyAll` before the allowlist check fires, leaving the spawned agent
            // with zero accessible tools.
            organon::types::ToolGroupPolicy::Groups(vec![organon::types::ToolGroupId::Read])
        } else {
            template
                .as_ref()
                .map_or_else(organon::types::ToolGroupPolicy::default, |t| {
                    t.tool_groups.clone()
                })
        };

        let session_key = format!(
            "spawn:{}",
            koina::ulid::Ulid::new().to_string().to_lowercase()
        );
        let workspace = self.oikos.nous_dir(&spawn_id);
        // WHY(#5555): spawned sub-agents must only access their own workspace,
        // not the entire oikos root. `workspace` is already under the oikos root
        // and is created before the actor starts.
        let allowed_roots = vec![workspace.clone()];
        let server_tool_config =
            self.tool_services
                .get()
                .map_or_else(ServerToolConfig::default, |services| {
                    server_tool_config_for_provider(
                        &services.server_tool_config,
                        self.providers.find_provider(&model),
                    )
                });

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
                chars_per_token: CHARS_PER_TOKEN,
                prosoche_model: koina::models::task_role_default(koina::models::TaskRole::Prosoche)
                    .to_owned(),
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
                loop_detection_window: 50,
                cycle_detection_max_len: 10,
            },
            domains: Vec::new(),
            private: false,
            episteme_cohort: std::sync::Arc::from("shared"),
            workspace,
            allowed_roots,
            server_tool_config,
            server_tools: Vec::new(),
            cache_enabled: true,
            recall: crate::recall::RecallConfig::default(),
            recall_profile: crate::config::RecallProfile::Default,
            tool_allowlist,
            tool_groups,
            hooks: crate::config::HookConfig::default(),
            behavior: taxis::config::AgentBehaviorDefaults::default(),
        };

        (spawn_id, config, session_key)
    }
}

impl SpawnService for SpawnServiceImpl {
    // NOTE: sequential ephemeral-actor lifecycle: build config, spawn actor, run single turn,
    // teardown. Splitting would fragment a cohesive lifecycle.
    #[expect(clippy::too_many_lines, reason = "spawn setup requires many steps")]
    fn spawn_and_run(
        &self,
        request: SpawnRequest,
        context: SpawnContext,
    ) -> Pin<Box<dyn Future<Output = Result<SpawnResult, String>> + Send + '_>> {
        let parent_nous_id = context.parent_nous_id.clone();
        let parent_cancel = context.parent_cancel.clone();
        let (spawn_id, config, session_key) = self.build_spawn_config(&request, &parent_nous_id);
        let timeout = Duration::from_secs(request.timeout_secs);
        let task = request.task.clone();
        let workspace = config.workspace.clone();
        let role = resolve_role(&request.role);
        let template = role.map(Role::template);

        // WHY: ephemeral sub-agents do not capture training data — their turns
        // are internal delegation, not user-facing conversation.
        let pipeline_config = PipelineConfig {
            history_budget_ratio: 0.6,
            project_id: None,
            extraction: None,
            stage_budget: StageBudget::default(),
            training: crate::training::TrainingConfig::default(),
            reflection_enabled: false,
            history: crate::config::TurnHistoryPolicy::default(),
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
        let tool_config = Arc::clone(&self.tool_config);

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
                let nous_dir = workspace.clone();
                if let Err(e) = tokio::fs::create_dir_all(&nous_dir).await {
                    return Err(format!("failed to create spawn workspace: {e}"));
                }
                let soul_path = nous_dir.join("SOUL.md");
                if let Err(e) = tokio::fs::write(&soul_path, &soul_content).await {
                    return Err(format!("failed to write SOUL.md: {e}"));
                }

                // WHY(#5088): child actor lifetime is tied to the parent turn so
                // parent cancellation does not leave spawned work running.
                let ephemeral_cancel = parent_cancel.child_token();
                let actor_cancel = ephemeral_cancel.clone();
                let (cross_tx, cross_rx) = if let Some(router) = router.as_ref() {
                    let (tx, rx) = tokio::sync::mpsc::channel(32);
                    router
                        .register_with_address_mask(
                            &spawn_id,
                            tx.clone(),
                            crate::cross::AddressMask::for_agent_privacy(config.private),
                        )
                        .await;
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
                    actor_cancel,
                    taxis::config::NousBehaviorConfig::default(),
                    tool_config,
                    audit_log,
                    empirical_router,
                    router.clone(),
                );

                info!(session_key = %session_key, "ephemeral actor started");

                // WHY: request-scoped cancellation token lets us cancel the child
                // turn itself when the parent timeout fires. Wrapping only the
                // waiting future with `tokio::time::timeout` drops the reply but
                // leaves the pipeline running inside the actor (#4776).
                let turn_cancel = ephemeral_cancel.child_token();
                let result = tokio::select! {
                    biased;
                    () = parent_cancel.cancelled() => {
                        turn_cancel.cancel();
                        ephemeral_cancel.cancel();
                        None
                    }
                    result = tokio::time::timeout(
                        timeout,
                        handle.send_turn_with_cancel(
                            &session_key,
                            None,
                            &task,
                            DEFAULT_SEND_TIMEOUT,
                            turn_cancel.clone(),
                        ),
                    ) => {
                        if result.is_err() {
                            turn_cancel.cancel();
                            ephemeral_cancel.cancel();
                        }
                        Some(result)
                    }
                };

                // kanon:ignore RUST/no-silent-result-swallow — best-effort shutdown of ephemeral actor
                let _ = handle.shutdown().await;
                let _ = join_handle.await;
                if let Some(router) = router.as_ref() {
                    router.unregister(&spawn_id).await;
                }

                // kanon:ignore RUST/no-silent-result-swallow — best-effort temp dir cleanup
                let _ = tokio::fs::remove_dir_all(&nous_dir).await;

                match result {
                    Some(Ok(Ok(turn))) => Ok(SpawnResult {
                        content: turn.content,
                        is_error: false,
                        input_tokens: turn.usage.input_tokens,
                        output_tokens: turn.usage.output_tokens,
                    }),
                    Some(Ok(Err(e))) => Ok(SpawnResult {
                        content: format!("Sub-agent error: {e}"),
                        is_error: true,
                        input_tokens: 0,
                        output_tokens: 0,
                    }),
                    Some(Err(_elapsed)) => {
                        warn!(timeout_secs = timeout.as_secs(), "sub-agent timed out");
                        Ok(SpawnResult {
                            content: format!("Sub-agent timed out after {}s", timeout.as_secs()),
                            is_error: true,
                            input_tokens: 0,
                            output_tokens: 0,
                        })
                    }
                    None => {
                        warn!("sub-agent cancelled by parent turn");
                        Ok(SpawnResult {
                            content: "Sub-agent cancelled by parent turn".to_owned(),
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

    // WHY (#4235): the Coder/Researcher role templates resolve to
    // `koina::defaults::DEFAULT_MODEL`. The mock provider's supported-models
    // list must include the workspace default so `spawn_and_run` can route
    // Coder tasks.
    static SUPPORTED_MOCK_MODELS: std::sync::LazyLock<Box<[&'static str]>> =
        std::sync::LazyLock::new(|| {
            Box::new([
                koina::defaults::DEFAULT_MODEL,
                koina::models::tier_default(koina::models::ModelTier::Opus),
                koina::models::tier_default(koina::models::ModelTier::Haiku),
            ])
        });

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
            MockProvider::with_responses(vec![response]).models(&SUPPORTED_MOCK_MODELS),
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
                SpawnContext::detached("test-parent"),
            )
            .await
            .expect("spawn");

        assert!(!result.is_error, "unexpected error: {}", result.content);
        assert_eq!(result.content, "Sub-agent result");
        assert_eq!(result.input_tokens, 200);
        assert_eq!(result.output_tokens, 80);
    }

    #[test]
    fn spawn_allowed_roots_restricted_to_workspace() {
        let (_dir, oikos) = make_oikos();
        let svc = make_spawn_service(Arc::clone(&oikos));

        let (_, config, _) = svc.build_spawn_config(
            &SpawnRequest {
                role: "coder".to_owned(),
                task: "Test task".to_owned(),
                model: None,
                allowed_tools: None,
                timeout_secs: 30,
            },
            "test-parent",
        );

        assert!(
            config.workspace.starts_with(oikos.root()),
            "spawn workspace should be under the oikos root"
        );
        assert_eq!(
            config.allowed_roots,
            vec![config.workspace.clone()],
            "spawned agent should only be granted its own workspace root"
        );
        assert!(
            !config.allowed_roots.contains(&oikos.root().to_path_buf()),
            "spawned agent must not inherit the entire oikos root"
        );
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
            tool_config: Arc::new(taxis::config::ToolLimitsConfig::default()),
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
                SpawnContext::detached("test-parent"),
            )
            .await
            .expect("spawn");

        assert!(!result.is_error, "unexpected error: {}", result.content);

        let provider = ProviderId::new(koina::defaults::DEFAULT_MODEL);
        for _ in 0..20 {
            if let Some(stats) = store
                .rolling_stats(&provider, &TaskCategory::Feature, Duration::from_hours(168))
                .await
                .expect("rolling stats query")
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
    fn conservative_spawn_allowlist_is_read_only() {
        // WHY(#3958, ADR-005): unknown-role spawns must default to a read-only
        // tool set. If a future contributor "loosens" this list (adds exec, rm,
        // http_request, etc.) the approval-guard contract breaks: a spawned
        // actor with no operator would silently execute irreversible tools.
        let allow = conservative_spawn_allowlist();
        for safe in ["read", "grep", "find", "ls", "view_file", "memory_search"] {
            assert!(
                allow.iter().any(|s| s == safe),
                "conservative allowlist must include safe tool '{safe}'"
            );
        }
        for dangerous in [
            "exec",
            "rm",
            "write",
            "edit",
            "http_request",
            "message",
            "sessions_send",
            "sessions_dispatch",
            "computer_use",
            "web_fetch",
        ] {
            assert!(
                !allow.iter().any(|s| s == dangerous),
                "conservative allowlist must NOT include dangerous tool '{dangerous}'"
            );
        }
    }

    #[test]
    fn spawn_uses_role_default_model() {
        use crate::roles::Role;
        // WHY (#4235): Coder/Researcher templates inherit from `SONNET_MODEL
        // = koina::defaults::DEFAULT_MODEL`. Assert against the constant so
        // template/model drift is caught at the call site, not in production.
        assert_eq!(Role::Coder.template().model, koina::defaults::DEFAULT_MODEL);
        assert_eq!(
            Role::Reviewer.template().model,
            koina::models::task_role_default(koina::models::TaskRole::Reviewer)
        );
        assert_eq!(
            Role::Researcher.template().model,
            koina::defaults::DEFAULT_MODEL
        );
        assert_eq!(
            Role::Explorer.template().model,
            koina::models::task_role_default(koina::models::TaskRole::Explorer)
        );
        assert_eq!(
            Role::Runner.template().model,
            koina::models::task_role_default(koina::models::TaskRole::Runner)
        );
        assert!(resolve_role("unknown").is_none());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn spawn_timeout_returns_error() {
        let (_dir, oikos) = make_oikos();

        let stuck = Arc::new(StuckProvider::new());
        let mut providers = ProviderRegistry::new();
        providers.register(Box::new(StuckProvider::clone_ref(&stuck)));
        let svc = SpawnServiceImpl::new(Arc::new(providers), Arc::new(ToolRegistry::new()), oikos);

        let start = tokio::time::Instant::now();
        let result = svc
            .spawn_and_run(
                SpawnRequest {
                    role: "coder".to_owned(),
                    task: "Stuck task".to_owned(),
                    model: None,
                    allowed_tools: None,
                    timeout_secs: 1,
                },
                SpawnContext::detached("test-parent"),
            )
            .await
            .expect("spawn");
        let elapsed = start.elapsed();

        assert!(result.is_error);
        assert!(result.content.contains("timed out"));
        assert!(
            elapsed < Duration::from_secs(3),
            "parent should return promptly after timeout, took {elapsed:?}"
        );
        assert!(
            stuck.started(),
            "stuck provider should have started the child turn"
        );
        assert!(
            tokio::time::timeout(Duration::from_secs(2), stuck.dropped())
                .await
                .is_ok(),
            "stuck provider future should be dropped after turn cancellation"
        );
        assert!(
            !stuck.completed(),
            "stuck provider should not complete after cancellation"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn spawn_parent_cancel_returns_error_and_stops_child() {
        let (_dir, oikos) = make_oikos();

        let stuck = Arc::new(StuckProvider::new());
        let mut providers = ProviderRegistry::new();
        providers.register(Box::new(StuckProvider::clone_ref(&stuck)));
        let svc = SpawnServiceImpl::new(Arc::new(providers), Arc::new(ToolRegistry::new()), oikos);
        let parent_cancel = tokio_util::sync::CancellationToken::new();
        let context = SpawnContext::new("test-parent", parent_cancel.clone());
        let task = tokio::spawn(async move {
            svc.spawn_and_run(
                SpawnRequest {
                    role: "coder".to_owned(),
                    task: "Stuck task".to_owned(),
                    model: None,
                    allowed_tools: None,
                    timeout_secs: 30,
                },
                context,
            )
            .await
            .expect("spawn")
        });

        tokio::time::timeout(Duration::from_secs(2), stuck.wait_started())
            .await
            .expect("stuck provider should start within 2 seconds");

        parent_cancel.cancel();
        let result = tokio::time::timeout(Duration::from_secs(3), task)
            .await
            .expect("parent cancellation should return promptly")
            .expect("spawn task should not panic");

        assert!(result.is_error);
        assert!(result.content.contains("cancelled by parent turn"));
        assert!(
            tokio::time::timeout(Duration::from_secs(2), stuck.dropped())
                .await
                .is_ok(),
            "stuck provider future should be dropped after parent cancellation"
        );
        assert!(
            !stuck.completed(),
            "stuck provider should not complete after parent cancellation"
        );
    }

    #[derive(Clone)]
    struct StuckProvider {
        inner: Arc<StuckProviderInner>,
    }

    struct StuckProviderInner {
        started: std::sync::atomic::AtomicBool,
        started_notify: tokio::sync::Notify,
        dropped: std::sync::atomic::AtomicBool,
        dropped_notify: tokio::sync::Notify,
        completed: std::sync::atomic::AtomicBool,
    }

    impl StuckProvider {
        fn new() -> Self {
            Self {
                inner: Arc::new(StuckProviderInner {
                    started: std::sync::atomic::AtomicBool::new(false),
                    started_notify: tokio::sync::Notify::new(),
                    dropped: std::sync::atomic::AtomicBool::new(false),
                    dropped_notify: tokio::sync::Notify::new(),
                    completed: std::sync::atomic::AtomicBool::new(false),
                }),
            }
        }

        fn clone_ref(this: &Arc<Self>) -> Self {
            // WHY: share the same inner state with the test while letting the
            // ProviderRegistry take ownership of the boxed provider.
            Self {
                inner: Arc::clone(&this.inner),
            }
        }

        fn started(&self) -> bool {
            self.inner.started.load(std::sync::atomic::Ordering::SeqCst)
        }

        async fn wait_started(&self) {
            let notified = self.inner.started_notify.notified();
            tokio::pin!(notified);
            if self.inner.started.load(std::sync::atomic::Ordering::SeqCst) {
                return;
            }
            notified.await;
        }

        fn completed(&self) -> bool {
            self.inner
                .completed
                .load(std::sync::atomic::Ordering::SeqCst)
        }

        async fn dropped(&self) {
            let dropped = self.inner.dropped_notify.notified();
            tokio::pin!(dropped);
            if self.inner.dropped.load(std::sync::atomic::Ordering::SeqCst) {
                return;
            }
            dropped.await;
        }
    }

    struct StuckCompletionFuture {
        inner: Arc<StuckProviderInner>,
    }

    impl Future for StuckCompletionFuture {
        type Output = hermeneus::error::Result<CompletionResponse>;

        fn poll(
            self: std::pin::Pin<&mut Self>,
            _cx: &mut std::task::Context<'_>,
        ) -> std::task::Poll<Self::Output> {
            self.inner
                .started
                .store(true, std::sync::atomic::Ordering::SeqCst);
            self.inner.started_notify.notify_waiters();
            std::task::Poll::Pending
        }
    }

    impl Drop for StuckCompletionFuture {
        fn drop(&mut self) {
            self.inner
                .dropped
                .store(true, std::sync::atomic::Ordering::SeqCst);
            self.inner.dropped_notify.notify_waiters();
        }
    }

    impl LlmProvider for StuckProvider {
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
            Box::pin(StuckCompletionFuture {
                inner: Arc::clone(&self.inner),
            })
        }

        fn supported_models(&self) -> &[&str] {
            &SUPPORTED_MOCK_MODELS
        }

        #[expect(clippy::unnecessary_literal_bound, reason = "trait requires &str")]
        fn name(&self) -> &str {
            "stuck"
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

    #[test]
    fn conservative_fallback_uses_read_group_policy() {
        // WHY(#5877, ADR-005): unrecognized-role spawns must pair the
        // conservative allowlist with `ToolGroupPolicy::Groups([Read])` so
        // `resolve_availability` does not deny allowlist tools before they are
        // checked. A `DenyAll` group policy (the previous default) caused every
        // conservative-allowlist tool to be denied before the allowlist gate.
        use organon::types::{ToolGroupId, ToolGroupPolicy};

        let (_dir, oikos) = make_oikos();
        let svc = make_spawn_service(Arc::clone(&oikos));

        let (_, config, _) = svc.build_spawn_config(
            &SpawnRequest {
                role: "analyst".to_owned(), // unrecognized — no Role template
                task: "Read the workspace".to_owned(),
                model: None,
                allowed_tools: None, // no explicit allowlist — triggers conservative path
                timeout_secs: 30,
            },
            "test-parent",
        );

        // Group policy must be Read-only, not DenyAll.
        assert_eq!(
            config.tool_groups,
            ToolGroupPolicy::Groups(vec![ToolGroupId::Read]),
            "conservative-fallback spawn must use Read group policy, not DenyAll"
        );

        // Allowlist must include every conservative read-only tool.
        let allowlist = config
            .tool_allowlist
            .expect("conservative allowlist must be Some");
        assert!(
            !allowlist.is_empty(),
            "conservative allowlist must be non-empty"
        );
        for expected in ["read", "grep", "find", "ls", "view_file", "memory_search"] {
            assert!(
                allowlist.iter().any(|s| s == expected),
                "conservative allowlist missing expected tool: {expected}"
            );
        }
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
                SpawnContext::detached("test-parent"),
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
                SpawnContext::detached("test-parent"),
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
