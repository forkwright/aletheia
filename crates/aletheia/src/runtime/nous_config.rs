use std::path::{Path, PathBuf};
use std::process::Command;

use hermeneus::provider::{LlmProvider, ProviderRegistry};
use tracing::warn;

use mneme::workspace::ProjectId;
use nous::config::{NousConfig, PipelineConfig};
use organon::types::{ServerToolConfig, ToolGroupId, ToolGroupPolicy};
use taxis::config::{AgentToolGroupPolicy, AletheiaConfig, ServerToolsConfig, resolve_nous};
use taxis::oikos::Oikos;

const WEB_SEARCH_TOOL: &str = "web_search";
const CODE_EXECUTION_TOOL: &str = "code_execution";

fn resolve_config_path(oikos: &Oikos, configured: &str) -> PathBuf {
    let path = Path::new(configured);
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        oikos.root().join(path)
    };
    absolute.canonicalize().unwrap_or(absolute)
}

fn resolve_allowed_roots(
    oikos: &Oikos,
    workspace: &str,
    configured_roots: &[String],
) -> Vec<PathBuf> {
    let mut roots = Vec::with_capacity(configured_roots.len() + 1);
    roots.push(resolve_config_path(oikos, workspace));
    for root in configured_roots {
        let resolved = resolve_config_path(oikos, root);
        if !roots.iter().any(|existing| existing == &resolved) {
            roots.push(resolved);
        }
    }
    roots
}

fn detect_project_id(workspace: &Path) -> Option<ProjectId> {
    let output = Command::new("git")
        .arg("-C")
        .arg(workspace)
        .args(["remote", "get-url", "origin"])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let remote = String::from_utf8(output.stdout).ok()?;
    ProjectId::from_git_remote(remote).ok()
}

fn resolve_tool_group_policy(agent_id: &str, policy: &AgentToolGroupPolicy) -> ToolGroupPolicy {
    match policy {
        AgentToolGroupPolicy::AllowAll => ToolGroupPolicy::AllowAll {
            reason: "explicit agents toolGroups = \"all\"".to_owned(),
        },
        AgentToolGroupPolicy::Groups(names) => {
            let mut groups = Vec::with_capacity(names.len());
            for name in names {
                match name.parse::<ToolGroupId>() {
                    Ok(group) => groups.push(group),
                    Err(error) => {
                        warn!(
                            agent = %agent_id,
                            group = %name,
                            error = %error,
                            "invalid tool group in agent config; denying all tool groups"
                        );
                        return ToolGroupPolicy::DenyAll;
                    }
                }
            }
            ToolGroupPolicy::groups(groups)
        }
        _ => ToolGroupPolicy::DenyAll,
    }
}

pub(super) fn configured_server_tool_config(config: &ServerToolsConfig) -> ServerToolConfig {
    ServerToolConfig {
        web_search: config.web_search,
        web_search_max_uses: config.web_search_max_uses,
        code_execution: config.code_execution,
    }
}

fn provider_gated_server_tool_config(
    config: &ServerToolsConfig,
    provider: Option<&dyn LlmProvider>,
) -> ServerToolConfig {
    let requested = configured_server_tool_config(config);
    let Some(provider) = provider else {
        if requested.web_search || requested.code_execution {
            warn!("server tools configured but no provider resolved for agent model; disabling");
        }
        return ServerToolConfig::default();
    };

    let web_search = requested.web_search && provider.supports_server_tool(WEB_SEARCH_TOOL);
    let code_execution =
        requested.code_execution && provider.supports_server_tool(CODE_EXECUTION_TOOL);

    if requested.web_search && !web_search {
        warn!(
            provider = provider.name(),
            tool = WEB_SEARCH_TOOL,
            "configured server tool is unsupported by resolved provider; disabling for agent"
        );
    }
    if requested.code_execution && !code_execution {
        warn!(
            provider = provider.name(),
            tool = CODE_EXECUTION_TOOL,
            "configured server tool is unsupported by resolved provider; disabling for agent"
        );
    }

    ServerToolConfig {
        web_search,
        web_search_max_uses: web_search
            .then_some(requested.web_search_max_uses)
            .flatten(),
        code_execution,
    }
}

fn apply_knowledge_recall_settings(nous_config: &mut NousConfig, config: &AletheiaConfig) {
    nous_config.recall.surprise_weight = config.knowledge.recall_surprise_weight;
    nous_config.recall.evidence_coverage_weight = config.knowledge.recall_evidence_coverage_weight;
    nous_config.recall.surprise_threshold = config.knowledge.surprise_threshold;
    nous_config.recall.surprise_ema_alpha = config.knowledge.surprise_ema_alpha;
    nous_config.recall.convergence_weight = config.knowledge.recall_convergence_weight;
    nous_config.recall.serendipity_weight = config.knowledge.recall_serendipity_weight;
}

pub(super) fn build_nous_runtime_config(
    config: &AletheiaConfig,
    oikos: &Oikos,
    packs: &[thesauros::loader::LoadedPack],
    agent_id: &str,
    providers: &ProviderRegistry,
) -> (NousConfig, PipelineConfig) {
    let resolved = resolve_nous(config, agent_id);
    let mut domains = resolved.domains.clone();
    let mut model = resolved.model.primary.to_string();
    let mut max_tool_iterations = resolved.capabilities.max_tool_iterations;
    for pack in packs {
        for domain in pack.domains_for_agent(agent_id) {
            if !domains.contains(&domain) {
                domains.push(domain);
            }
        }
        if let Some(pack_model) = pack.model_for_agent(agent_id) {
            model = pack_model;
        }
        if let Some(agency) = pack.agency_for_agent(agent_id) {
            max_tool_iterations = match agency.as_str() {
                "unrestricted" => 10_000,
                "standard" => koina::defaults::MAX_TOOL_ITERATIONS,
                "restricted" => 50,
                other => {
                    warn!(
                        agent = %agent_id,
                        agency = %other,
                        pack = %pack.manifest.name,
                        "unknown agency level in pack overlay, skipping"
                    );
                    continue;
                }
            };
        }
    }

    let workspace = resolve_config_path(oikos, &resolved.workspace);
    let project_id = detect_project_id(&workspace);
    let server_tool_config =
        provider_gated_server_tool_config(&config.server_tools, providers.find_provider(&model));
    let mut nous_config = NousConfig {
        id: resolved.id,
        name: resolved.name,
        generation: nous::config::NousGenerationConfig {
            model,
            fallback_models: resolved
                .model
                .fallbacks
                .iter()
                .map(ToString::to_string)
                .collect(),
            retries_before_fallback: resolved.model.retries_before_fallback,
            context_window: resolved.limits.context_tokens,
            max_output_tokens: resolved.limits.max_output_tokens,
            bootstrap_max_tokens: resolved.limits.bootstrap_max_tokens,
            thinking_enabled: resolved.capabilities.thinking_enabled,
            thinking_budget: resolved.limits.thinking_budget,
            chars_per_token: resolved.limits.chars_per_token,
            prosoche_model: resolved.prosoche_model.to_string(),
            complexity: hermeneus::complexity::ComplexityConfig::default(),
            extraction_model: None,
            distillation_model: None,
        },
        limits: nous::config::NousLimits {
            max_tool_iterations,
            loop_detection_threshold: 3,
            consecutive_error_threshold: 4,
            loop_max_warnings: 2,
            session_token_cap: 500_000,
            max_tool_result_bytes: resolved.limits.max_tool_result_bytes,
            max_consecutive_tool_only_iterations: 3,
            consecutive_mistake_limit: koina::defaults::DEFAULT_CONSECUTIVE_MISTAKE_LIMIT,
            loop_detection_window: config.nous_behavior.loop_detection_window,
            cycle_detection_max_len: config.nous_behavior.cycle_detection_max_len,
        },
        domains,
        private: resolved.private,
        episteme_cohort: resolved.episteme_cohort,
        workspace,
        allowed_roots: resolve_allowed_roots(oikos, &resolved.workspace, &resolved.allowed_roots),
        server_tool_config,
        server_tools: Vec::new(),
        cache_enabled: resolved.capabilities.cache_enabled,
        recall: resolved.recall.into(),
        recall_profile: resolved.recall_profile.into(),
        tool_allowlist: resolved.tool_allowlist,
        tool_groups: resolve_tool_group_policy(agent_id, &resolved.tool_groups),
        hooks: nous::config::HookConfig::default(),
        behavior: resolved.behavior,
    };
    // WHY: thread the knowledge-config surprise/evidence recall knobs into the
    // recall engine. The From<RecallSettings> conversion cannot carry these —
    // they live on KnowledgeConfig, not RecallSettings — so they are applied
    // here where the global config is in scope. Defaults keep recall inert.
    apply_knowledge_recall_settings(&mut nous_config, config);

    let mut extraction_cfg = mneme::extract::ExtractionConfig::default();
    if let Some(model) = nous_config.generation.extraction_model.as_deref() {
        model.clone_into(&mut extraction_cfg.model);
    }
    (
        nous_config,
        PipelineConfig {
            project_id,
            extraction: Some(extraction_cfg),
            training: config.training.clone(),
            ..PipelineConfig::default()
        },
    )
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test setup and assertions")]
mod tests {
    use std::collections::HashSet;

    use hermeneus::provider::ProviderRegistry;
    use hermeneus::test_utils::MockProvider;
    use koina::id::ToolName;
    use organon::registry::ToolRegistry;
    use organon::surface::SurfaceInputs;
    use taxis::config::{AgentToolGroupPolicy, ModelSpec, NousDefinition};

    use super::*;

    const ANTHROPIC_MODEL: &str = "claude-test-server-tools";
    const OPENAI_MODEL: &str = "gpt-test-server-tools";

    fn test_oikos() -> (tempfile::TempDir, Oikos) {
        let dir = tempfile::tempdir().expect("tempdir");
        let oikos = Oikos::from_root(dir.path());
        (dir, oikos)
    }

    fn config_for(model: &str) -> AletheiaConfig {
        let mut config = AletheiaConfig::default();
        config.server_tools.web_search = true;
        config.server_tools.web_search_max_uses = Some(7);
        config.server_tools.code_execution = true;
        config.agents.list.push(NousDefinition {
            id: "alice".to_owned(),
            name: None,
            enabled: true,
            model: Some(ModelSpec {
                primary: model.to_owned(),
                fallbacks: Vec::new(),
                retries_before_fallback: 0,
            }),
            workspace: "nous/alice".to_owned(),
            thinking_enabled: None,
            agency: None,
            allowed_roots: Vec::new(),
            domains: Vec::new(),
            default: false,
            private: false,
            episteme_cohort: None,
            recall: None,
            tool_allowlist: None,
            tool_groups: Some(AgentToolGroupPolicy::AllowAll),
            recall_profile: None,
            behavior: None,
        });
        config
    }

    fn provider_registry_with_server_tools() -> ProviderRegistry {
        let mut providers = ProviderRegistry::new();
        providers.register(Box::new(
            MockProvider::new("ok")
                .models(&[ANTHROPIC_MODEL])
                .server_tools(&["web_search", "code_execution"]),
        ));
        providers
    }

    fn provider_registry_without_server_tools() -> ProviderRegistry {
        let mut providers = ProviderRegistry::new();
        providers.register(Box::new(MockProvider::new("ok").models(&[OPENAI_MODEL])));
        providers
    }

    #[test]
    fn configured_server_tools_are_lazy_advertised_and_activate_for_supporting_provider() {
        let (_dir, oikos) = test_oikos();
        let config = config_for(ANTHROPIC_MODEL);
        let providers = provider_registry_with_server_tools();

        let (nous_config, _pipeline_config) =
            build_nous_runtime_config(&config, &oikos, &[], "alice", &providers);

        let active = HashSet::new();
        let surface = ToolRegistry::new().effective_surface(SurfaceInputs {
            policy: &nous_config.tool_groups,
            allowlist: nous_config.tool_allowlist.as_deref(),
            active: &active,
            server_tools: &nous_config.server_tools,
            server_tool_config: Some(&nous_config.server_tool_config),
        });
        let lazy_names: HashSet<String> = surface
            .lazy_catalog()
            .into_iter()
            .map(|(name, _description)| name.as_str().to_owned())
            .collect();
        assert!(lazy_names.contains("web_search"));
        assert!(lazy_names.contains("code_execution"));

        let active = HashSet::from([
            ToolName::new("web_search").expect("valid tool name"),
            ToolName::new("code_execution").expect("valid tool name"),
        ]);
        let definitions = nous_config.server_tool_config.active_definitions(&active);
        assert_eq!(definitions.len(), 2);
        assert!(
            definitions.iter().any(|tool| {
                tool.name == "web_search"
                    && tool.tool_type == "web_search_20250305"
                    && tool.max_uses == Some(7)
            }),
            "web_search must resolve to the configured provider server-tool definition"
        );
        assert!(
            definitions
                .iter()
                .any(|tool| tool.name == "code_execution"
                    && tool.tool_type == "code_execution_20250522"),
            "code_execution must resolve to the configured provider server-tool definition"
        );
    }

    #[test]
    fn configured_server_tools_are_disabled_for_provider_without_capability() {
        let (_dir, oikos) = test_oikos();
        let config = config_for(OPENAI_MODEL);
        let providers = provider_registry_without_server_tools();

        let (nous_config, _pipeline_config) =
            build_nous_runtime_config(&config, &oikos, &[], "alice", &providers);

        let active = HashSet::from([
            ToolName::new("web_search").expect("valid tool name"),
            ToolName::new("code_execution").expect("valid tool name"),
        ]);
        assert!(
            nous_config
                .server_tool_config
                .active_definitions(&active)
                .is_empty(),
            "unsupported providers must not receive configured server-tool definitions"
        );

        let surface = ToolRegistry::new().effective_surface(SurfaceInputs {
            policy: &nous_config.tool_groups,
            allowlist: nous_config.tool_allowlist.as_deref(),
            active: &HashSet::new(),
            server_tools: &nous_config.server_tools,
            server_tool_config: Some(&nous_config.server_tool_config),
        });
        assert!(
            surface.lazy_catalog().is_empty(),
            "enable_tool must not advertise provider server tools that cannot be activated"
        );
    }
}
