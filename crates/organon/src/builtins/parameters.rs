//! Agent tool for discovering tunable parameters and their metadata.
//!
//! Lets agents (and operators through agent conversations) query what
//! configuration knobs exist, what they control, and how they should be tuned.

use std::fmt::Write;
use std::future::Future;
use std::pin::Pin;

use indexmap::IndexMap;

use koina::id::ToolName;
use taxis::registry::{self, ParameterSpec, ParameterTier};

use crate::error::Result;
use crate::registry::{ToolExecutor, ToolRegistry};
use crate::types::{
    InputSchema, PropertyDef, PropertyType, Reversibility, ToolCategory, ToolContext, ToolDef,
    ToolInput, ToolResult,
};

/// Filter specs against the optional section/affects/tier inputs.
fn matches_filters(
    spec: &ParameterSpec,
    section: Option<&str>,
    affects: Option<&str>,
    tier: Option<&str>,
) -> bool {
    if let Some(sec) = section
        && !spec.section.contains(sec)
    {
        return false;
    }
    if let Some(aff) = affects
        && !spec.affects.contains(aff)
    {
        return false;
    }
    if let Some(t) = tier {
        let tier_match = match t {
            "deployment" => spec.tier == ParameterTier::Deployment,
            "per-agent" | "peragent" | "per_agent" => spec.tier == ParameterTier::PerAgent,
            "self-tuning" | "selftuning" | "self_tuning" => spec.tier == ParameterTier::SelfTuning,
            _ => true,
        };
        if !tier_match {
            return false;
        }
    }
    true
}

/// Format a single spec into the output buffer.
fn format_spec(out: &mut String, spec: &ParameterSpec) {
    let _ = writeln!(out, "## {}", spec.key);
    let _ = writeln!(out, "  Section: {}", spec.section);
    let _ = writeln!(out, "  Tier: {}", spec.tier);
    let _ = writeln!(out, "  Default: {}", spec.default);
    if let Some((min, max)) = spec.bounds {
        let _ = writeln!(out, "  Bounds: [{min}, {max}]");
    }
    let _ = writeln!(
        out,
        "  Hot-reloadable: {}",
        if spec.hot_reloadable { "yes" } else { "no" }
    );
    let _ = writeln!(out, "  Description: {}", spec.description);
    let _ = writeln!(out, "  Affects: {}", spec.affects);
    let _ = writeln!(out, "  Outcome signal: {}", spec.outcome_signal);
    let _ = writeln!(out, "  Evidence required: {}", spec.evidence_required);
    let _ = writeln!(out, "  Direction hint: {}\n", spec.direction_hint);
}

struct QueryParametersExecutor;

impl ToolExecutor for QueryParametersExecutor {
    fn execute<'a>(
        &'a self,
        input: &'a ToolInput,
        _ctx: &'a ToolContext,
    ) -> Pin<Box<dyn Future<Output = Result<ToolResult>> + Send + 'a>> {
        Box::pin(async {
            let section = input
                .arguments
                .get("section")
                .and_then(serde_json::Value::as_str);
            let affects = input
                .arguments
                .get("affects")
                .and_then(serde_json::Value::as_str);
            let tier = input
                .arguments
                .get("tier")
                .and_then(serde_json::Value::as_str);

            let all = registry::all_specs();

            let filtered: Vec<_> = all
                .iter()
                .filter(|s| matches_filters(s, section, affects, tier))
                .collect();

            if filtered.is_empty() {
                return Ok(ToolResult::text(
                    "No parameters match the given filters.".to_owned(),
                ));
            }

            let mut output = format!("Found {} parameter(s):\n\n", filtered.len());
            for spec in &filtered {
                format_spec(&mut output, spec);
            }

            Ok(ToolResult::text(output))
        })
    }
}

fn query_parameters_def() -> ToolDef {
    ToolDef {
        name: ToolName::from_static("query_parameters"), // kanon:ignore RUST/expect
        description: "Query tunable system parameters and their metadata. \
                      Returns parameter specs with defaults, bounds, tuning tier, \
                      and outcome signals for self-tuning."
            .to_owned(),
        extended_description: None,
        input_schema: InputSchema {
            properties: IndexMap::from([
                (
                    "section".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description:
                            "Filter by config section (e.g. 'knowledge', 'agents.defaults.behavior')"
                                .to_owned(),
                        enum_values: None,
                        default: None,
                    },
                ),
                (
                    "affects".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description:
                            "Filter by affected subsystem (e.g. 'distillation', 'competence_scoring')"
                                .to_owned(),
                        enum_values: None,
                        default: None,
                    },
                ),
                (
                    "tier".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description:
                            "Filter by tuning tier: 'deployment', 'per-agent', or 'self-tuning'"
                                .to_owned(),
                        enum_values: None,
                        default: None,
                    },
                ),
            ]),
            required: vec![],
        },
        category: ToolCategory::System,
        reversibility: Reversibility::FullyReversible,
        auto_activate: true,
    }
}

/// Register the `query_parameters` tool into the registry.
pub(crate) fn register(registry: &mut ToolRegistry) -> Result<()> {
    registry.register(query_parameters_def(), Box::new(QueryParametersExecutor))?;
    Ok(())
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use std::collections::HashSet;
    use std::sync::{Arc, RwLock};

    use koina::id::{NousId, SessionId, ToolName};

    use crate::testing::install_crypto_provider;
    use crate::types::{ServerToolConfig, ToolContext, ToolInput, ToolServices};

    use super::*;

    fn mock_ctx() -> ToolContext {
        install_crypto_provider();
        ToolContext {
            nous_id: NousId::new("test-agent").expect("valid"),
            session_id: SessionId::new(),
            workspace: std::path::PathBuf::from("/tmp/test"),
            allowed_roots: vec![std::path::PathBuf::from("/tmp")],
            services: Some(Arc::new(ToolServices {
                cross_nous: None,
                messenger: None,
                note_store: None,
                blackboard_store: None,
                spawn: None,
                planning: None,
                knowledge: None,
                http_client: reqwest::Client::new(),
                lazy_tool_catalog: vec![],
                server_tool_config: ServerToolConfig::default(),
            })),
            active_tools: Arc::new(RwLock::new(HashSet::new())),
        }
    }

    #[tokio::test]
    async fn query_all_returns_results() {
        let ctx = mock_ctx();
        let executor = QueryParametersExecutor;
        let input = ToolInput {
            name: ToolName::from_static("query_parameters"),
            tool_use_id: "toolu_1".to_owned(),
            arguments: serde_json::json!({}),
        };

        let result = executor.execute(&input, &ctx).await.expect("execute");
        assert!(!result.is_error, "expected successful result");
        assert!(
            result.content.text_summary().contains("parameter(s)"),
            "expected output to contain parameter count"
        );
    }

    #[tokio::test]
    async fn query_by_section_filters() {
        let ctx = mock_ctx();
        let executor = QueryParametersExecutor;
        let input = ToolInput {
            name: ToolName::from_static("query_parameters"),
            tool_use_id: "toolu_2".to_owned(),
            arguments: serde_json::json!({"section": "knowledge"}),
        };

        let result = executor.execute(&input, &ctx).await.expect("execute");
        assert!(!result.is_error, "expected successful result");
        let text = result.content.text_summary();
        assert!(
            text.contains("knowledge"),
            "expected output to contain knowledge section specs"
        );
    }

    #[tokio::test]
    async fn query_by_tier_filters() {
        let ctx = mock_ctx();
        let executor = QueryParametersExecutor;
        let input = ToolInput {
            name: ToolName::from_static("query_parameters"),
            tool_use_id: "toolu_3".to_owned(),
            arguments: serde_json::json!({"tier": "self-tuning"}),
        };

        let result = executor.execute(&input, &ctx).await.expect("execute");
        assert!(!result.is_error, "expected successful result");
        let text = result.content.text_summary();
        assert!(
            text.contains("self-tuning"),
            "expected output to reference self-tuning tier"
        );
    }

    #[tokio::test]
    async fn query_no_match_returns_message() {
        let ctx = mock_ctx();
        let executor = QueryParametersExecutor;
        let input = ToolInput {
            name: ToolName::from_static("query_parameters"),
            tool_use_id: "toolu_4".to_owned(),
            arguments: serde_json::json!({"section": "nonexistent_section_xyz"}),
        };

        let result = executor.execute(&input, &ctx).await.expect("execute");
        assert!(!result.is_error, "expected non-error result");
        assert!(
            result
                .content
                .text_summary()
                .contains("No parameters match"),
            "expected no-match message"
        );
    }

    #[tokio::test]
    async fn query_by_affects_filters() {
        let ctx = mock_ctx();
        let executor = QueryParametersExecutor;
        let input = ToolInput {
            name: ToolName::from_static("query_parameters"),
            tool_use_id: "toolu_5".to_owned(),
            arguments: serde_json::json!({"affects": "distillation"}),
        };

        let result = executor.execute(&input, &ctx).await.expect("execute");
        assert!(!result.is_error, "expected successful result");
        let text = result.content.text_summary();
        assert!(
            text.contains("distillation"),
            "expected output to reference distillation"
        );
    }
}
