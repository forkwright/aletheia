//! Metrics tool (metron — μέτρον, measure).
//!
//! Produces health, cost, velocity, and status reports for the dispatch pipeline.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use indexmap::IndexMap;

use energeia::cron::{CronFireRecord, CronLockStore};
use energeia::metrics::MetricsService;
use energeia::store::EnergeiaStore;
use koina::id::ToolName;

use crate::error::Result;
use crate::registry::ToolExecutor;
use crate::types::{
    InputSchema, PropertyDef, PropertyType, Reversibility, ToolCategory, ToolContext, ToolDef,
    ToolGroupId, ToolInput, ToolResult, ToolTag,
};

use super::shared::{opt_u64, require_str, to_json_text};

// ── metron (μέτρον — measure) ──────────────────────────────────────────────

pub(super) fn metron_def() -> ToolDef {
    ToolDef {
        name: ToolName::from_static("metron"),
        description: "Produce health and performance metrics for the dispatch pipeline: \
            dispatch counts, success rates, one-shot rates, status, and cost summaries."
            .to_owned(),
        extended_description: None,
        input_schema: InputSchema {
            properties: IndexMap::from([
                (
                    "report_type".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description:
                            "Report to generate: `health`, `cost`, `velocity`, or `status`"
                                .to_owned(),
                        enum_values: Some(vec![
                            "health".to_owned(),
                            "cost".to_owned(),
                            "velocity".to_owned(),
                            "status".to_owned(),
                        ]),
                        default: None,
                        ..Default::default()
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
                        ..Default::default()
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
                        ..Default::default()
                    },
                ),
            ]),
            required: vec!["report_type".to_owned()],
        },
        category: ToolCategory::System,
        reversibility: Reversibility::FullyReversible,
        auto_activate: false,
        groups: vec![ToolGroupId::Verify],
        tags: vec![ToolTag::Recon, ToolTag::Verify],
    }
}

pub(super) struct MetronExecutor {
    pub(super) store: Option<Arc<EnergeiaStore>>,
    pub(super) cron_lock_store: Option<Arc<CronLockStore>>,
    pub(super) cron_task_names: Vec<String>,
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
                Err(e) => return Ok(ToolResult::error(e)),
            };
            let days = opt_u64(args, "days")
                .and_then(|d| u32::try_from(d).ok())
                .unwrap_or(30);

            let mut service = MetricsService::new(Arc::clone(store));
            if let Some(cron_lock_store) = self.cron_lock_store.as_ref() {
                service = service.with_cron_lock_store(
                    Arc::clone(cron_lock_store),
                    self.cron_task_names.clone(),
                );
            }

            match report_type {
                "status" => Ok(render_status_report(&service)),
                "health" => Ok(render_health_report(&service, days)),
                "cost" | "velocity" => Ok(render_cost_report(&service, days, report_type)),
                other => Ok(ToolResult::error(format!(
                    "metron: unknown report_type '{other}' (use 'health', 'cost', 'velocity', or 'status')"
                ))),
            }
        })
    }
}

fn render_status_report(service: &MetricsService) -> ToolResult {
    let report = match service.status_dashboard() {
        Ok(report) => report,
        Err(e) => {
            return ToolResult::error(format!("metron: status report failed: {e}"));
        }
    };
    let recent_outcomes: Vec<serde_json::Value> = report
        .recent_outcomes
        .iter()
        .map(|o| {
            serde_json::json!({
                "dispatch_id": o.dispatch_id,
                "project": o.project,
                "status": o.status,
                "started_at": o.started_at.to_string(),
                "finished_at": o.finished_at.map(|ts| ts.to_string()),
                "total_sessions": o.total_sessions,
                "total_cost_usd": o.total_cost_usd,
            })
        })
        .collect();
    let by_project: Vec<serde_json::Value> = report
        .by_project
        .iter()
        .map(|p| {
            serde_json::json!({
                "project": p.project,
                "active_dispatches": p.active_dispatches,
                "total_sessions": p.total_sessions,
                "total_cost_usd": p.total_cost_usd,
                "success_rate": p.success_rate,
            })
        })
        .collect();
    let cron = report.cron.as_ref().map(|cron| {
        let task_fires: Vec<serde_json::Value> = cron
            .task_fires
            .iter()
            .map(|task| {
                serde_json::json!({
                    "task_name": task.task_name,
                    "last_fire_record": task.last_fire_record.as_ref().map(cron_fire_record_json),
                })
            })
            .collect();
        serde_json::json!({
            "stale_fire_count": cron.stale_fire_count,
            "task_fires": task_fires,
        })
    });
    let output = serde_json::json!({
        "report_type": "status",
        "computed_at": report.computed_at.to_string(),
        "active_dispatches": report.active_dispatches,
        "queue_depth": report.queue_depth,
        "stale_running_dispatches": report.stale_running_dispatches,
        "recent_outcomes": recent_outcomes,
        "by_project": by_project,
        "cron": cron,
    });
    to_json_text(&output)
}

fn render_health_report(service: &MetricsService, days: u32) -> ToolResult {
    let report = match service.health_report(days) {
        Ok(report) => report,
        Err(e) => {
            return ToolResult::error(format!("metron: health report failed: {e}"));
        }
    };
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
    to_json_text(&output)
}

fn render_cost_report(service: &MetricsService, days: u32, report_type: &str) -> ToolResult {
    let report = match service.cost_report(days) {
        Ok(report) => report,
        Err(e) => {
            return ToolResult::error(format!("metron: cost report failed: {e}"));
        }
    };
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
    to_json_text(&output)
}

fn cron_fire_record_json(record: &CronFireRecord) -> serde_json::Value {
    serde_json::json!({
        "scheduled_at": record.scheduled_at.to_string(),
        "started_at": record.started_at.to_string(),
        "finished_at": record.finished_at.map(|ts| ts.to_string()),
        "succeeded": record.succeeded,
    })
}
