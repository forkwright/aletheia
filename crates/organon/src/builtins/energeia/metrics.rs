//! Metrics tool (metron — μέτρον, measure).
//!
//! Produces health, cost, and velocity reports for the dispatch pipeline.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use indexmap::IndexMap;

use aletheia_energeia::metrics::MetricsService;
use aletheia_energeia::store::EnergeiaStore;
use aletheia_koina::id::ToolName;

use crate::error::Result;
use crate::registry::ToolExecutor;
use crate::types::{
    InputSchema, PropertyDef, PropertyType, Reversibility, ToolCategory, ToolContext, ToolDef,
    ToolInput, ToolResult,
};

use super::shared::{opt_u64, require_str, to_json_text};

// ── metron (μέτρον — measure) ──────────────────────────────────────────────

pub(super) fn metron_def() -> ToolDef {
    ToolDef {
        name: ToolName::from_static("metron"),
        description: "Produce health and performance metrics for the dispatch pipeline: \
            dispatch counts, success rates, one-shot rates, and cost summaries."
            .to_owned(),
        extended_description: None,
        input_schema: InputSchema {
            properties: IndexMap::from([
                (
                    "report_type".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "Report to generate: `health`, `cost`, or `velocity`"
                            .to_owned(),
                        enum_values: Some(vec![
                            "health".to_owned(),
                            "cost".to_owned(),
                            "velocity".to_owned(),
                        ]),
                        default: None,
                    },
                ),
                (
                    "days".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::Integer,
                        description: "Number of days to include in the report window (default: 30)"
                            .to_owned(),
                        enum_values: None,
                        default: Some(serde_json::json!(30)),
                    },
                ),
                (
                    "project".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "Scope the report to a specific project (owner/repo); \
                            omit for aggregate across all projects"
                            .to_owned(),
                        enum_values: None,
                        default: None,
                    },
                ),
            ]),
            required: vec!["report_type".to_owned()],
        },
        category: ToolCategory::System,
        reversibility: Reversibility::FullyReversible,
        auto_activate: false,
    }
}

pub(super) struct MetronExecutor {
    pub(super) store: Option<Arc<EnergeiaStore>>,
}

impl ToolExecutor for MetronExecutor {
    fn execute<'a>(
        &'a self,
        input: &'a ToolInput,
        _ctx: &'a ToolContext,
    ) -> Pin<Box<dyn Future<Output = Result<ToolResult>> + Send + 'a>> {
        Box::pin(async move {
            let Some(ref store) = self.store else {
                return Ok(ToolResult::error(
                    "metron: store not configured (missing EnergeiaServices)",
                ));
            };

            let args = &input.arguments;
            let report_type = match require_str(args, "report_type") {
                Ok(s) => s,
                Err(e) => return Ok(e),
            };
            let days = opt_u64(args, "days")
                .and_then(|d| u32::try_from(d).ok())
                .unwrap_or(30);

            let service = MetricsService::new(Arc::clone(store));

            match report_type {
                "health" => match service.health_report(days) {
                    Ok(report) => {
                        let metrics: Vec<serde_json::Value> = report
                            .metrics
                            .iter()
                            .map(|m| {
                                serde_json::json!({
                                    "name": m.name,
                                    "description": m.description,
                                    "value": m.value,
                                    "status": m.status.to_string(),
                                    "sample_size": m.sample_size,
                                    "ok_threshold": m.ok_threshold,
                                    "warn_threshold": m.warn_threshold,
                                    "higher_is_better": m.higher_is_better,
                                })
                            })
                            .collect();
                        let output = serde_json::json!({
                            "report_type": "health",
                            "window_days": report.window_days,
                            "computed_at": report.computed_at.to_string(),
                            "metrics": metrics,
                        });
                        Ok(to_json_text(&output))
                    }
                    Err(e) => Ok(ToolResult::error(format!(
                        "metron: health report failed: {e}"
                    ))),
                },
                "cost" | "velocity" => match service.cost_report(days) {
                    Ok(report) => {
                        let daily: Vec<serde_json::Value> = report
                            .daily_velocity
                            .iter()
                            .map(|d| {
                                serde_json::json!({
                                    "date": d.date.to_string(),
                                    "dispatches": d.dispatches,
                                    "sessions": d.sessions,
                                    "cost_usd": d.cost_usd,
                                })
                            })
                            .collect();
                        let by_project: Vec<serde_json::Value> = report
                            .by_project
                            .iter()
                            .map(|p| {
                                serde_json::json!({
                                    "project": p.project,
                                    "cost_usd": p.cost_usd,
                                    "dispatches": p.dispatches,
                                    "sessions": p.sessions,
                                    "success_rate": p.success_rate,
                                })
                            })
                            .collect();
                        let output = serde_json::json!({
                            "report_type": report_type,
                            "window_days": days,
                            "period_start": report.period_start.to_string(),
                            "period_end": report.period_end.to_string(),
                            "total_cost_usd": report.total_cost_usd,
                            "total_dispatches": report.total_dispatches,
                            "total_sessions": report.total_sessions,
                            "avg_cost_per_dispatch": report.avg_cost_per_dispatch,
                            "avg_cost_per_session": report.avg_cost_per_session,
                            "by_project": by_project,
                            "daily_velocity": daily,
                        });
                        Ok(to_json_text(&output))
                    }
                    Err(e) => Ok(ToolResult::error(format!(
                        "metron: cost report failed: {e}"
                    ))),
                },
                other => Ok(ToolResult::error(format!(
                    "metron: unknown report_type '{other}' (use 'health', 'cost', or 'velocity')"
                ))),
            }
        })
    }
}
