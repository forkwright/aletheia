//! Always-on memory-health strip above the fact list.

use dioxus::prelude::*;

use crate::state::memory::{FactHealth, confidence_color};

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

/// Slim health strip: total / stale / low-confidence / forgotten / avg-confidence.
#[component]
pub(crate) fn HealthStrip(health: FactHealth) -> Element {
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
        }
    }
}
