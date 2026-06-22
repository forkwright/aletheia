use std::path::{Path, PathBuf};
use std::process::Command;

use tracing::warn;

use mneme::workspace::ProjectId;
use nous::config::{NousConfig, PipelineConfig};
use organon::types::{ToolGroupId, ToolGroupPolicy};
use taxis::config::{AgentToolGroupPolicy, AletheiaConfig, resolve_nous};
use taxis::oikos::Oikos;

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
        AgentToolGroupPolicy::DenyAll => ToolGroupPolicy::DenyAll,
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

pub(super) fn build_nous_runtime_config(
    config: &AletheiaConfig,
    oikos: &Oikos,
    packs: &[thesauros::loader::LoadedPack],
    agent_id: &str,
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
    nous_config.recall.surprise_weight = config.knowledge.recall_surprise_weight;
    nous_config.recall.evidence_coverage_weight = config.knowledge.recall_evidence_coverage_weight;
    nous_config.recall.surprise_threshold = config.knowledge.surprise_threshold;
    nous_config.recall.surprise_ema_alpha = config.knowledge.surprise_ema_alpha;
    nous_config.recall.convergence_weight = config.knowledge.recall_convergence_weight;
    nous_config.recall.serendipity_weight = config.knowledge.recall_serendipity_weight;

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
