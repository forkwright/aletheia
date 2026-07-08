//! Always-on memory-health strip above the fact list.

use dioxus::prelude::*;

use crate::state::{
    fetch::FetchState,
    memory::{FactHealth, GraphCheckReport, confidence_color},
};

const STRIP_STYLE: &str = "\
    display: flex; \
    align-items: stretch; \
    gap: var(--space-2); \
    padding: var(--space-2) 0 var(--space-3) 0; \
    flex-wrap: wrap;\
";

const STAT_STYLE: &str = "\
    display: flex; \
    flex-direction: column; \
    gap: 2px; \
    padding: var(--space-1) var(--space-3); \
    border: 1px solid var(--border); \
    border-radius: var(--radius-md); \
    background: var(--bg-surface); \
    min-width: 88px;\
";

const STAT_VALUE_STYLE: &str = "\
    font-size: var(--text-base); \
    font-weight: var(--weight-semibold); \
    line-height: 1.1;\
";

const STAT_LABEL_STYLE: &str = "\
    font-size: var(--text-xs); \
    color: var(--text-muted); \
    text-transform: uppercase; \
    letter-spacing: 0.4px;\
";

const GRAPH_BADGE_STYLE: &str = "\
    display: flex; \
    flex-direction: column; \
    gap: 2px; \
    padding: var(--space-1) var(--space-3); \
    border-radius: var(--radius-md); \
    min-width: 168px; \
    max-width: 280px;\
";

const GRAPH_DETAIL_STYLE: &str = "\
    font-size: var(--text-xs); \
    color: var(--text-muted); \
    line-height: 1.25;\
";

/// A single threshold-colored stat cell. A `count` of 0 reads neutral; any
/// positive count escalates to the supplied warning color.
#[component]
fn HealthStat(
    label: &'static str,
    value: String,
    /// Color token applied to the value text.
    color: &'static str,
) -> Element {
    rsx! {
        div {
            style: "{STAT_STYLE}",
            span { style: "{STAT_VALUE_STYLE} color: {color};", "{value}" }
            span { style: "{STAT_LABEL_STYLE}", "{label}" }
        }
    }
}

fn graph_badge(
    value: String,
    detail: String,
    color: &'static str,
    background: &'static str,
) -> Element {
    rsx! {
        div {
            style: "{GRAPH_BADGE_STYLE} border: 1px solid {color}; background: {background};",
            span { style: "{STAT_VALUE_STYLE} color: {color};", "{value}" }
            span { style: "{GRAPH_DETAIL_STYLE}", "{detail}" }
        }
    }
}

fn render_graph_integrity(graph_check: Signal<FetchState<GraphCheckReport>>) -> Element {
    match &*graph_check.read() {
        FetchState::Loading => graph_badge(
            "Checking graph...".to_string(),
            "Server integrity check pending".to_string(),
            "var(--text-muted)",
            "var(--bg-surface)",
        ),
        FetchState::Error(err) => graph_badge(
            "Graph check unavailable".to_string(),
            err.clone(),
            "var(--status-warning)",
            "var(--status-warning-bg)",
        ),
        FetchState::Loaded(report) => {
            if report.is_consistent() {
                graph_badge(
                    "Graph consistent".to_string(),
                    format!(
                        "{} facts / {} entities / {} edges",
                        report.fact_count(),
                        report.entity_count(),
                        report.relationship_count()
                    ),
                    "var(--status-success)",
                    "var(--status-success-bg)",
                )
            } else {
                graph_badge(
                    "Inconsistency detected".to_string(),
                    format!(
                        "{} orphaned / {} dangling / {} total issues",
                        report.orphaned_entity_count(),
                        report.dangling_edge_count(),
                        report.structural_issue_count()
                    ),
                    "var(--status-error)",
                    "var(--status-error-bg)",
                )
            }
        }
    }
}

/// Slim health strip: total / stale / low-confidence / forgotten / avg-confidence.
#[component]
pub(crate) fn HealthStrip(
    health: FactHealth,
    graph_check: Signal<FetchState<GraphCheckReport>>,
) -> Element {
    // WHY: counts read neutral at zero and escalate to a warning/error token
    // once any item needs attention — a glance answers "is my memory healthy?".
    let stale_color = if health.stale > 0 {
        "var(--status-warning)"
    } else {
        "var(--text-primary)"
    };
    let low_conf_color = if health.low_confidence > 0 {
        "var(--status-error)"
    } else {
        "var(--text-primary)"
    };
    let forgotten_color = if health.forgotten > 0 {
        "var(--text-muted)"
    } else {
        "var(--text-primary)"
    };
    let avg_color = confidence_color(health.avg_confidence);
    let avg_label = format!("{:.0}%", health.avg_confidence * 100.0);

    let total_label = format!("{} / {}", health.active, health.total);

    rsx! {
        div {
            style: "{STRIP_STYLE}",
            HealthStat { label: "Active / Total", value: total_label, color: "var(--text-primary)" }
            HealthStat { label: "Stale >30d", value: "{health.stale}", color: stale_color }
            HealthStat { label: "Low conf", value: "{health.low_confidence}", color: low_conf_color }
            HealthStat { label: "Forgotten", value: "{health.forgotten}", color: forgotten_color }
            HealthStat { label: "Avg conf", value: avg_label, color: avg_color }
            {render_graph_integrity(graph_check)}
        }
    }
}
