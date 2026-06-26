use std::path::{Path, PathBuf};
use std::process::Command;

use tracing::warn;

use mneme::workspace::ProjectId;
use nous::config::{HookConfig, NousConfig, NousLimits, PipelineConfig};
use organon::types::{ToolGroupId, ToolGroupPolicy};
use taxis::config::{AgentBehaviorDefaults, AgentToolGroupPolicy, AletheiaConfig, resolve_nous};
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

fn build_nous_limits(
    max_tool_iterations: u32,
    max_tool_result_bytes: u32,
    behavior: &AgentBehaviorDefaults,
    config: &AletheiaConfig,
) -> NousLimits {
    NousLimits {
        max_tool_iterations,
        loop_detection_threshold: behavior.safety_loop_detection_threshold,
        consecutive_error_threshold: behavior.safety_consecutive_error_threshold,
        loop_max_warnings: behavior.safety_loop_max_warnings,
        session_token_cap: behavior.safety_session_token_cap,
        max_tool_result_bytes,
        max_consecutive_tool_only_iterations: behavior.safety_max_consecutive_tool_only_iterations,
        consecutive_mistake_limit: koina::defaults::DEFAULT_CONSECUTIVE_MISTAKE_LIMIT,
        loop_detection_window: config.nous_behavior.loop_detection_window,
        cycle_detection_max_len: config.nous_behavior.cycle_detection_max_len,
    }
}

fn build_hook_config(behavior: &AgentBehaviorDefaults) -> HookConfig {
    HookConfig {
        cost_control_enabled: behavior.hooks_cost_control_enabled,
        turn_token_budget: behavior.hooks_turn_token_budget,
        scope_enforcement_enabled: behavior.hooks_scope_enforcement_enabled,
        correction_hooks_enabled: behavior.hooks_correction_hooks_enabled,
        audit_logging_enabled: behavior.hooks_audit_logging_enabled,
        ..HookConfig::default()
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
    let behavior = resolved.behavior.clone();
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
        limits: build_nous_limits(
            max_tool_iterations,
            resolved.limits.max_tool_result_bytes,
            &behavior,
            config,
        ),
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
        hooks: build_hook_config(&behavior),
        behavior,
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

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test setup assertions")]
mod tests {
    use taxis::config::{AgentBehaviorDefaults, AletheiaConfig, NousDefinition};
    use taxis::oikos::Oikos;
    use tempfile::TempDir;

    use super::build_nous_runtime_config;

    #[test]
    fn resolved_behavior_drives_runtime_limits_and_hooks() {
        let mut config = AletheiaConfig::default();
        let behavior = AgentBehaviorDefaults {
            safety_loop_detection_threshold: 7,
            safety_consecutive_error_threshold: 8,
            safety_loop_max_warnings: 9,
            safety_session_token_cap: 123_456,
            safety_max_consecutive_tool_only_iterations: 6,
            hooks_cost_control_enabled: false,
            hooks_turn_token_budget: 42,
            hooks_scope_enforcement_enabled: false,
            hooks_correction_hooks_enabled: false,
            hooks_audit_logging_enabled: false,
            ..AgentBehaviorDefaults::default()
        };
        config.agents.list.push(NousDefinition {
            id: "custom".to_owned(),
            workspace: "nous/custom".to_owned(),
            behavior: Some(behavior.clone()),
            ..NousDefinition::default()
        });
        let instance = TempDir::new().expect("create instance temp directory");
        let oikos = Oikos::from_root(instance.path());

        let (nous_config, _pipeline_config) =
            build_nous_runtime_config(&config, &oikos, &[], "custom");

        assert_eq!(nous_config.limits.loop_detection_threshold, 7);
        assert_eq!(nous_config.limits.consecutive_error_threshold, 8);
        assert_eq!(nous_config.limits.loop_max_warnings, 9);
        assert_eq!(nous_config.limits.session_token_cap, 123_456);
        assert_eq!(nous_config.limits.max_consecutive_tool_only_iterations, 6);
        assert!(!nous_config.hooks.cost_control_enabled);
        assert_eq!(nous_config.hooks.turn_token_budget, 42);
        assert!(!nous_config.hooks.scope_enforcement_enabled);
        assert!(!nous_config.hooks.correction_hooks_enabled);
        assert!(!nous_config.hooks.audit_logging_enabled);
        assert!(
            nous_config.hooks.self_audit_enabled,
            "schema-unbacked hook fields should keep their runtime defaults"
        );
        assert!(
            nous_config.hooks.working_checkpoint_enabled,
            "schema-unbacked hook fields should keep their runtime defaults"
        );
        assert_eq!(
            nous_config.behavior.safety_loop_detection_threshold,
            behavior.safety_loop_detection_threshold
        );
    }
}
