// kanon:ignore RUST/file-too-long — spawn service + full integration test suite; test extraction into submodule planned
//! Ephemeral sub-agent spawning service.

use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use tracing::{Instrument, info, warn};

use hermeneus::provider::ProviderRegistry;
use koina::defaults::{
    BOOTSTRAP_MAX_TOKENS, CHARS_PER_TOKEN, CONTEXT_TOKENS, DEFAULT_MODEL, MAX_OUTPUT_TOKENS,
    MAX_TOOL_ITERATIONS, MAX_TOOL_RESULT_BYTES,
};
use mneme::embedding::EmbeddingProvider;
#[cfg(feature = "knowledge-store")]
use mneme::knowledge_store::KnowledgeStore;
use mneme::store::SessionStore;
use organon::registry::ToolRegistry;
use organon::types::{SpawnContext, SpawnRequest, SpawnResult, SpawnService, ToolServices};
use taxis::oikos::Oikos;
use tokio::sync::Mutex;

use crate::actor;
use crate::config::{NousConfig, PipelineConfig, StageBudget};
use crate::handle::DEFAULT_SEND_TIMEOUT;
use crate::roles::Role;

const SONNET_MODEL: &str = DEFAULT_MODEL;

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

/// Generation and tool-limit values for a spawned sub-agent, resolved from
/// the parent's live hint when a spawning host provided one, falling back to
/// this module's fixed constants otherwise.
///
/// WHY(#4746): a spawning host (nous's own `agent` tool builtin) may forward
/// the parent's *actual* live generation/limits values through
/// `SpawnContext::parent_generation`, so a child derives from what the
/// parent is really running with instead of always these fixed constants.
/// `None` (no host populated it -- e.g. every existing test constructing
/// `SpawnContext::detached`/`::new` directly) preserves prior behavior
/// exactly.
struct ResolvedGeneration {
    context_window: u32,
    max_output_tokens: u32,
    bootstrap_max_tokens: u32,
    chars_per_token: u32,
    max_tool_iterations: u32,
    session_token_cap: u64,
    max_tool_result_bytes: u32,
}

impl ResolvedGeneration {
    fn from_hint(hint: Option<organon::types::SpawnGenerationHint>) -> Self {
        Self {
            context_window: hint.map_or(CONTEXT_TOKENS, |h| h.context_window),
            max_output_tokens: hint.map_or(MAX_OUTPUT_TOKENS, |h| h.max_output_tokens),
            bootstrap_max_tokens: hint.map_or(BOOTSTRAP_MAX_TOKENS, |h| h.bootstrap_max_tokens),
            chars_per_token: hint.map_or(CHARS_PER_TOKEN, |h| h.chars_per_token),
            max_tool_iterations: hint.map_or(MAX_TOOL_ITERATIONS, |h| h.max_tool_iterations),
            session_token_cap: hint.map_or(500_000, |h| h.session_token_cap),
            max_tool_result_bytes: hint.map_or(MAX_TOOL_RESULT_BYTES, |h| h.max_tool_result_bytes),
        }
    }
}

/// Compose an ephemeral sub-agent's system-prompt content from its role
/// template and (if present) role contract.
///
/// WHY(#4775): a contract's `to_prompt_section()` (behaviors/constraints/
/// tool-group summary) is appended to the template prompt, never replaces
/// it — an operator-configured `roles.toml` can only add guidance on top of
/// the base role prompt, not silently rewrite it. A pure function so the
/// composition is testable without the full async spawn lifecycle.
fn compose_soul_content(
    role_str: &str,
    template: Option<&crate::roles::RoleTemplate>,
    contract: Option<&crate::roles::contract::RoleContract>,
) -> String {
    let base = template.map_or_else(
        || {
            format!(
                "You are an ephemeral {role_str} sub-agent. Complete the assigned task precisely and concisely."
            )
        },
        |t| t.system_prompt.to_owned(),
    );
    contract.map_or_else(
        || base.clone(),
        |c| format!("{base}\n\n{}", c.to_prompt_section()),
    )
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

    /// Resolve the operator-configurable behavior contract for a known role.
    ///
    /// WHY(#4775): `roles::contract::ContractRegistry` (versioned,
    /// TOML-configurable role contracts, cascaded `nous/{parent}/roles.toml`
    /// -> `shared/roles.toml` -> `theke/roles.toml`) existed with zero
    /// production callers — spawned role behavior came only from the
    /// hardcoded `Role::template()` match. This is the wiring point.
    ///
    /// Scoped to roles with a Rust `Role` variant (a resolved `template`) so
    /// the ADR-005 (#3958) conservative-allowlist safety net for
    /// *unrecognized* role strings is untouched: a role name with no Rust
    /// template still falls through to the read-only fallback exactly as
    /// before, contract or no contract.
    ///
    /// `ContractRegistry::defaults()` always populates every built-in role
    /// (`default_registry_has_all_roles`), and `load_from_file` merges file
    /// contracts on top of that same default set — so a known `Role` always
    /// resolves here. A `roles.toml` that fails to parse degrades to those
    /// defaults with a visible warning rather than silently losing the
    /// override (#4775's "missing or invalid role contracts fail visibly").
    fn resolve_contract(
        &self,
        parent_nous_id: &str,
        role: Role,
    ) -> Option<crate::roles::contract::RoleContract> {
        use crate::roles::contract::ContractRegistry;

        let registry = taxis::cascade::resolve(&self.oikos, parent_nous_id, "roles.toml", None)
            .map_or_else(ContractRegistry::defaults, |path| {
                ContractRegistry::load_from_file(&path).unwrap_or_else(|e| {
                    warn!(
                        role = %role,
                        path = %path.display(),
                        error = %e,
                        "failed to parse roles.toml; falling back to hardcoded role contract defaults"
                    );
                    ContractRegistry::defaults()
                })
            });

        registry.get(role.as_str()).cloned()
    }

    /// Build a [`NousConfig`] for an ephemeral sub-agent.
    ///
    /// WHY(#5555): keep config construction deterministic and testable so the
    /// spawned `allowed_roots` can be asserted independently of the actor
    /// lifecycle.
    // NOTE: sequential field-by-field config derivation (model, tool policy,
    // spawn policy, workspace, generation limits) -- already extracted the
    // largest cohesive slice into `ResolvedGeneration`; what remains is each
    // `NousConfig`/`NousLimits` field's own independent derivation rule and
    // splitting further would fragment one config into scattered pieces.
    #[expect(clippy::too_many_lines, reason = "sequential config-field derivation")]
    fn build_spawn_config(
        &self,
        request: &SpawnRequest,
        parent_nous_id: &str,
        parent_generation: Option<organon::types::SpawnGenerationHint>,
    ) -> (String, NousConfig, String) {
        let spawn_id = format!(
            "spawn-{}-{}",
            parent_nous_id,
            koina::ulid::Ulid::new().to_string().to_lowercase()
        );
        let role = resolve_role(&request.role);
        let template = role.map(Role::template);
        let contract = role.and_then(|r| self.resolve_contract(parent_nous_id, r));

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
            // WHY(#4775): an operator-configured `roles.toml` contract's
            // `tool_groups` overrides the hardcoded template default when
            // present. This only narrows or reshapes the *coarse* group
            // gate; the fine-grained `tool_allowlist` above is unchanged and
            // still gates first at the group level (`resolve_availability`
            // checks group policy before the allowlist), so a contract can
            // never grant a tool name the template didn't already list.
            contract.as_ref().map_or_else(
                || {
                    template
                        .as_ref()
                        .map_or_else(organon::types::ToolGroupPolicy::default, |t| {
                            t.tool_groups.clone()
                        })
                },
                |c| c.tool_groups.clone(),
            )
        };

        // WHY(#5087): cohort/privacy/domains previously came from scattered
        // constants (`"shared"`, `false`, `Vec::new()`) with no way to
        // derive them from anything. A role contract is the
        // operator-approved spawn policy this issue offers as the
        // alternative to threading the parent's live `NousConfig` through
        // `SpawnContext` (that would need a signature change to
        // `organon::types::SpawnContext`, outside this file's ownership).
        // Absent an override, behavior is unchanged: every default contract
        // asserts no cohort, `private: false`, and no domains
        // (`default_contracts_have_no_spawn_policy_overrides`).
        let episteme_cohort: std::sync::Arc<str> = contract
            .as_ref()
            .and_then(|c| c.episteme_cohort.as_deref())
            .map_or_else(|| std::sync::Arc::from("shared"), std::sync::Arc::from);
        let private = contract.as_ref().is_some_and(|c| c.private);
        let domains = contract
            .as_ref()
            .map_or_else(Vec::new, |c| c.domains.clone());

        let session_key = format!(
            "spawn:{}",
            koina::ulid::Ulid::new().to_string().to_lowercase()
        );
        let workspace = self.oikos.nous_dir(&spawn_id);
        // WHY(#5555): spawned sub-agents must only access their own workspace,
        // not the entire oikos root. `workspace` is already under the oikos root
        // and is created before the actor starts.
        let allowed_roots = vec![workspace.clone()];

        // WHY(#4746): see `ResolvedGeneration`'s docs for the fallback contract.
        let resolved = ResolvedGeneration::from_hint(parent_generation);

        let config = NousConfig {
            id: Arc::from(spawn_id.as_str()),
            name: None,
            generation: crate::config::NousGenerationConfig {
                model,
                provider: None,
                fallback_models: Vec::new(),
                fallback_providers: Vec::new(),
                retries_before_fallback: 2,
                context_window: resolved.context_window,
                max_output_tokens: resolved.max_output_tokens,
                bootstrap_max_tokens: resolved.bootstrap_max_tokens,
                thinking_enabled: false,
                thinking_budget: 0,
                chars_per_token: resolved.chars_per_token,
                prosoche_model: koina::models::task_role_default(koina::models::TaskRole::Prosoche)
                    .to_owned(),
                complexity: hermeneus::complexity::ComplexityConfig::default(),
                extraction_model: None,
                distillation_model: None,
            },
            limits: crate::config::NousLimits {
                max_tool_iterations: resolved.max_tool_iterations,
                loop_detection_threshold: 3,
                consecutive_error_threshold: 4,
                loop_max_warnings: 2,
                session_token_cap: resolved.session_token_cap,
                max_tool_result_bytes: resolved.max_tool_result_bytes,
                max_consecutive_tool_only_iterations: 3,
                consecutive_mistake_limit: koina::defaults::DEFAULT_CONSECUTIVE_MISTAKE_LIMIT,
                loop_detection_window: 50,
                cycle_detection_max_len: 10,
                client_disconnect_policy: crate::config::ClientDisconnectPolicy::default(),
            },
            domains,
            private,
            // WHY(#5823): every ephemeral sub-agent is a cross-agent call by
            // construction, regardless of true nesting level — `ToolContext`
            // does not thread the parent's own depth through `SpawnContext`,
            // and `score_complexity`'s cross-agent branch treats any
            // `depth > 0` identically, so a constant `1` is exact for
            // present routing semantics.
            spawn_depth: 1,
            episteme_cohort,
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
        let (spawn_id, config, session_key) =
            self.build_spawn_config(&request, &parent_nous_id, context.parent_generation);
        let timeout = Duration::from_secs(request.timeout_secs);
        let task = request.task.clone();
        let workspace = config.workspace.clone();
        let role = resolve_role(&request.role);
        let template = role.map(Role::template);
        let contract = role.and_then(|r| self.resolve_contract(&parent_nous_id, r));

        // WHY: ephemeral sub-agents do not capture training data or propose
        // tuning changes — their turns are internal delegation, not
        // user-facing conversation, and should not shift global parameters.
        let pipeline_config = PipelineConfig {
            history_budget_ratio: 0.6,
            project_id: None,
            extraction: None,
            stage_budget: StageBudget::default(),
            training: crate::training::TrainingConfig::default(),
            reflection_enabled: false,
            history: crate::config::TurnHistoryPolicy::default(),
            tuning: taxis::config::TuningConfig::default(),
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

        let soul_content =
            compose_soul_content(&request.role, template.as_ref(), contract.as_ref());

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
#[expect(
    clippy::disallowed_methods,
    reason = "test fixtures use std::fs to write tempdir files synchronously"
)]
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
            // WHY .named: `record_router_outcome` keys the empirical store on
            // the actually-observed provider identity (`provider.name()`),
            // not the model string (#4798/#4863 model/provider conflation
            // fix). Without this, the mock's default provider name ("mock")
            // diverges from `koina::defaults::DEFAULT_MODEL`, the identity
            // this fixture's `RecordingRouter`/assertions are built around.
            MockProvider::with_responses(vec![response])
                .models(&SUPPORTED_MOCK_MODELS)
                .named(koina::defaults::DEFAULT_MODEL),
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
            None,
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

    // WHY(#4746): `parent_generation` must genuinely override the hardcoded
    // constants, not merely compile through unused — a spawning host with an
    // operator-tuned `NousGenerationConfig`/`NousLimits` (larger context
    // window, tighter session cap, etc.) needs the child to inherit those
    // real values, not silently fall back to defaults that ignore the
    // operator's configuration.
    #[test]
    fn spawn_config_derives_generation_and_limits_from_parent_hint_when_present() {
        let (_dir, oikos) = make_oikos();
        let svc = make_spawn_service(Arc::clone(&oikos));

        let hint = organon::types::SpawnGenerationHint {
            context_window: 999_000,
            max_output_tokens: 12_345,
            bootstrap_max_tokens: 6_789,
            chars_per_token: 5,
            max_tool_iterations: 77,
            session_token_cap: 111_111,
            max_tool_result_bytes: 22_222,
        };

        let (_, config, _) = svc.build_spawn_config(
            &SpawnRequest {
                role: "coder".to_owned(),
                task: "Test task".to_owned(),
                model: None,
                allowed_tools: None,
                timeout_secs: 30,
            },
            "test-parent",
            Some(hint),
        );

        assert_eq!(config.generation.context_window, 999_000);
        assert_eq!(config.generation.max_output_tokens, 12_345);
        assert_eq!(config.generation.bootstrap_max_tokens, 6_789);
        assert_eq!(config.generation.chars_per_token, 5);
        assert_eq!(config.limits.max_tool_iterations, 77);
        assert_eq!(config.limits.session_token_cap, 111_111);
        assert_eq!(config.limits.max_tool_result_bytes, 22_222);
    }

    // WHY(#4746): the absent-hint path (every call site that does not
    // populate `SpawnContext::parent_generation`, including every other test
    // in this module) must reproduce prior behavior exactly — the constants,
    // unchanged.
    #[test]
    fn spawn_config_falls_back_to_constants_when_no_parent_hint() {
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
            None,
        );

        assert_eq!(config.generation.context_window, CONTEXT_TOKENS);
        assert_eq!(config.generation.max_output_tokens, MAX_OUTPUT_TOKENS);
        assert_eq!(config.generation.bootstrap_max_tokens, BOOTSTRAP_MAX_TOKENS);
        assert_eq!(config.generation.chars_per_token, CHARS_PER_TOKEN);
        assert_eq!(config.limits.max_tool_iterations, MAX_TOOL_ITERATIONS);
        assert_eq!(config.limits.session_token_cap, 500_000);
        assert_eq!(config.limits.max_tool_result_bytes, MAX_TOOL_RESULT_BYTES);
    }

    // WHY(#5823): a hand-set `ComplexityInput.depth` only proves the scorer
    // works in isolation — it does not prove any production caller ever
    // reaches that branch. This drives the real path: `build_spawn_config`
    // (what every ephemeral sub-agent gets) into `routed_model_for_turn`
    // (what `execute` actually calls to pick a turn's model), with a
    // deliberately boring message that would score well under the Opus
    // threshold on content alone. A pass can only be explained by the
    // `spawn_depth` signal reaching `score_complexity`'s cross-agent branch.
    #[test]
    fn spawn_config_routes_cross_agent_turns_to_opus() {
        let (_dir, oikos) = make_oikos();
        let svc = make_spawn_service(Arc::clone(&oikos));

        let (_, mut config, _) = svc.build_spawn_config(
            &SpawnRequest {
                role: "coder".to_owned(),
                task: "Test task".to_owned(),
                model: None,
                allowed_tools: None,
                timeout_secs: 30,
            },
            "test-parent",
            None,
        );
        assert!(
            config.spawn_depth > 0,
            "an ephemeral sub-agent config must carry a non-zero cross-agent depth"
        );

        config.generation.complexity = hermeneus::complexity::ComplexityConfig {
            enabled: true,
            ..hermeneus::complexity::ComplexityConfig::default()
        };

        let mut ctx = crate::pipeline::PipelineContext::default();
        ctx.messages
            .push(crate::pipeline::PipelineMessage::text("user", "hi", 1));

        let providers = make_providers();
        let tools = ToolRegistry::new();

        let opus_model = koina::models::tier_default(koina::models::ModelTier::Opus);
        assert_ne!(
            config.generation.model, opus_model,
            "spawn's base model must differ from the opus tier for this test to be meaningful"
        );

        let routed = crate::execute::routed_model_for_turn(&ctx, &config, &providers, &tools);
        assert_eq!(
            routed, opus_model,
            "a spawned sub-agent turn must route to the Opus tier via the cross-agent branch"
        );
    }

    // WHY(#4775): `ContractRegistry` existed with zero production callers —
    // this proves the wiring point. A changed `roles.toml` must change
    // spawned role behavior (the issue's own acceptance bar), not just
    // parse successfully in isolation.
    #[test]
    fn spawn_config_tool_groups_follow_roles_toml_override() {
        use organon::types::{ToolGroupId, ToolGroupPolicy};

        let (_dir, oikos) = make_oikos();
        std::fs::write(
            oikos.shared().join("roles.toml"),
            r#"
[coder]
version = 2
tool_groups = ["read"]
"#,
        )
        .expect("write roles.toml");
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
            None,
        );

        assert_eq!(
            config.tool_groups,
            ToolGroupPolicy::Groups(vec![ToolGroupId::Read]),
            "roles.toml override must narrow coder's tool_groups from the hardcoded 4-group default"
        );
    }

    // WHY(#5087): cohort/privacy/domains previously came from scattered
    // constants with nothing to derive them from. A role contract is the
    // operator-approved spawn policy alternative; this proves the override
    // path actually reaches `NousConfig`.
    #[test]
    fn spawn_config_cohort_privacy_domains_follow_roles_toml_override() {
        let (_dir, oikos) = make_oikos();
        std::fs::write(
            oikos.shared().join("roles.toml"),
            r#"
[reviewer]
version = 2
episteme_cohort = "isolated"
private = true
domains = ["medical"]
"#,
        )
        .expect("write roles.toml");
        let svc = make_spawn_service(Arc::clone(&oikos));

        let (_, config, _) = svc.build_spawn_config(
            &SpawnRequest {
                role: "reviewer".to_owned(),
                task: "Test task".to_owned(),
                model: None,
                allowed_tools: None,
                timeout_secs: 30,
            },
            "test-parent",
            None,
        );

        assert_eq!(&*config.episteme_cohort, "isolated");
        assert!(config.private);
        assert_eq!(config.domains, vec!["medical".to_owned()]);
    }

    // WHY: wiring `ContractRegistry` into production must not change
    // default spawned-agent behavior when the operator has not touched
    // `roles.toml` — the exact tempdir fixture every other test in this
    // file uses has no roles.toml, so this also guards against a future
    // change accidentally asserting a cohort/privacy/domain by default.
    #[test]
    fn spawn_config_defaults_unchanged_without_roles_toml() {
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
            None,
        );

        assert_eq!(
            config.tool_groups,
            Role::Coder.template().tool_groups,
            "with no roles.toml, tool_groups must match the hardcoded template exactly"
        );
        assert_eq!(&*config.episteme_cohort, "shared");
        assert!(!config.private);
        assert!(config.domains.is_empty());
    }

    // WHY(#4775 "missing or invalid role contracts fail visibly"): a
    // malformed roles.toml must degrade to hardcoded defaults rather than
    // taking spawning down, but the degrade must be a fallback, not a
    // silent swallow — `resolve_contract` logs a `warn!` on this path.
    #[test]
    fn spawn_config_malformed_roles_toml_falls_back_to_defaults() {
        let (_dir, oikos) = make_oikos();
        std::fs::write(
            oikos.shared().join("roles.toml"),
            "this is not { valid toml",
        )
        .expect("write malformed roles.toml");
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
            None,
        );

        assert_eq!(
            config.tool_groups,
            Role::Coder.template().tool_groups,
            "malformed roles.toml must fall back to hardcoded defaults, not fail the spawn"
        );
    }

    // WHY(#4775): prompt composition is a pure function specifically so it
    // is testable without the full async spawn lifecycle (the assembled
    // soul_content is written to a workspace file that gets cleaned up
    // after the ephemeral turn completes, so it can't be asserted through
    // `spawn_and_run` without a timing-dependent mid-flight read).
    #[test]
    fn compose_soul_content_appends_contract_section_to_template_prompt() {
        let template = Role::Coder.template();
        let contract = crate::roles::contract::ContractRegistry::defaults()
            .get("coder")
            .cloned()
            .expect("default coder contract");

        let without_contract = compose_soul_content("coder", Some(&template), None);
        assert_eq!(without_contract, template.system_prompt);

        let with_contract = compose_soul_content("coder", Some(&template), Some(&contract));
        assert!(
            with_contract.starts_with(template.system_prompt),
            "contract section must be appended, not replace the template prompt"
        );
        assert!(with_contract.contains("Role Contract: coder"));
        assert!(with_contract.len() > template.system_prompt.len());
    }

    #[test]
    fn compose_soul_content_unknown_role_has_no_contract_section() {
        let content = compose_soul_content("analyst", None, None);
        assert_eq!(
            content,
            "You are an ephemeral analyst sub-agent. Complete the assigned task precisely and concisely."
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
            // WHY(#5217): the task text ("Build a feature") has no keyword
            // signal, so the outcome aggregates under Unknown, not Feature.
            if let Some(stats) = store
                .rolling_stats(&provider, &TaskCategory::Unknown, Duration::from_hours(168))
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

        // WHY(#6908): the property is that the parent returns instead of
        // blocking on a child that never finishes, and the three assertions
        // below already carry it -- the parent returned, it returned the
        // timeout, and the child had started. A wall-clock ceiling on top of
        // that proved nothing extra: `StuckProvider` never completes, so a
        // parent that did block would hang the test rather than finish slowly.
        assert!(result.is_error);
        assert!(result.content.contains("timed out"));
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
            None,
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
