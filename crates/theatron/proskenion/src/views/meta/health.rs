//! Memory health dashboard: composite score, confidence distribution, staleness.

use dioxus::prelude::*;

use crate::state::meta::{health_score_color, MemoryHealthStore};
use crate::views::meta::{
    LineChart, CARD_LABEL, CARD_STYLE, CARD_SUB, CARD_VALUE, GRID_STYLE, MUTED_TEXT,
};

const TABLE_HEADER_STYLE: &str = "\
    display: flex; \
    padding: var(--space-2) var(--space-3); \
    background: var(--bg-surface); \
    border-bottom: 1px solid var(--border); \
    font-size: var(--text-xs); \
    color: var(--text-secondary); \
    text-transform: uppercase; \
    letter-spacing: 0.5px;\
";

const TABLE_ROW_STYLE: &str = "\
    display: flex; \
    padding: var(--space-2) var(--space-3); \
    border-bottom: 1px solid var(--border); \
    font-size: var(--text-sm); \
    color: var(--text-primary);\
";

const REC_STYLE: &str = "\
    background: var(--bg-surface); \
    border-left: 3px solid var(--status-warning); \
    padding: var(--space-3) var(--space-4); \
    font-size: var(--text-sm); \
    color: var(--text-primary); \
    margin-bottom: var(--space-2); \
    border-radius: 0 var(--radius-md) var(--radius-md) 0;\
";

const HISTOGRAM_BAR_STYLE: &str = "\
    display: flex; \
    align-items: flex-end; \
    gap: var(--space-1);\
";

#[component]
pub(super) fn MemoryHealthSection(store: MemoryHealthStore) -> Element {
    let score_color = health_score_color(store.health_score);
    let score_pct = store.health_score * 100.0;

    rsx! {
        // NOTE: Health overview cards.
        div {
            style: "{GRID_STYLE}",
            div {
                style: "{CARD_STYLE} flex: 1; min-width: 140px; text-align: center;",
                div {
                    style: "{CARD_VALUE} color: {score_color};",
                    "{score_pct:.0}%"
                }
                div { style: "{CARD_LABEL}", "Health Score" }
                div { style: "{CARD_SUB}",
                    "confidence 40% + connectivity 30% + freshness 30%"
                }
            }
            div {
                style: "{CARD_STYLE} flex: 1; min-width: 140px; text-align: center;",
                div { style: "{CARD_VALUE}", "{store.orphan_ratio * 100.0:.0}%" }
                div { style: "{CARD_LABEL}", "Orphan Ratio" }
                div { style: "{CARD_SUB}", "Entities with 0-1 relationships" }
            }
            div {
                style: "{CARD_STYLE} flex: 1; min-width: 140px; text-align: center;",
                div { style: "{CARD_VALUE}", "{store.decay_pressure_count}" }
                div { style: "{CARD_LABEL}", "Decay Pressure" }
                div { style: "{CARD_SUB}", "Near FSRS threshold" }
            }
        }

        // NOTE: Confidence distribution histogram.
        h3 {
            style: "font-size: var(--text-base); color: var(--text-secondary); margin: var(--space-4) 0 var(--space-3) 0;",
            "Confidence Distribution"
        }
        ConfidenceHistogram { buckets: store.confidence_distribution.clone() }

        // NOTE: Health trend over time.
        if !store.health_over_time.is_empty() {
            h3 {
                style: "font-size: var(--text-base); color: var(--text-secondary); margin: var(--space-4) 0 var(--space-3) 0;",
                "Health Trend"
            }
            div {
                style: "{CARD_STYLE}",
                LineChart {
                    data: store.health_over_time.clone(),
                    width: 600.0,
                    height: 150.0,
                    color: "var(--status-success)",
                    show_labels: true,
                }
            }
        }

        // NOTE: Staleness table.
        if !store.stale_entities.is_empty() {
            h3 {
                style: "font-size: var(--text-base); color: var(--text-secondary); margin: var(--space-4) 0 var(--space-3) 0;",
                "Stale Entities"
            }
            div {
                style: "border: 1px solid var(--border); border-radius: var(--radius-md); overflow: hidden;",
                div {
                    style: "{TABLE_HEADER_STYLE}",
                    span { style: "flex: 2;", "Entity" }
                    span { style: "flex: 1;", "Last Updated" }
                    span { style: "flex: 1; text-align: right;", "Days Stale" }
                }
                for entity in &store.stale_entities {
                    {
                        let staleness_color = if entity.days_stale > 90 {
                            "var(--status-error)"
                        } else if entity.days_stale > 60 {
                            "var(--status-warning)"
                        } else {
                            "var(--text-secondary)"
                        };
                        rsx! {
                            div {
                                style: "{TABLE_ROW_STYLE}",
                                span { style: "flex: 2;", "{entity.name}" }
                                span { style: "flex: 1; color: var(--text-secondary);", "{entity.last_updated}" }
                                span {
                                    style: "flex: 1; text-align: right; color: {staleness_color};",
                                    "{entity.days_stale}d"
                                }
                            }
                        }
                    }
                }
            }
        }

        // NOTE: Recommendations.
        if !store.recommendations.is_empty() {
            h3 {
                style: "font-size: var(--text-base); color: var(--text-secondary); margin: var(--space-4) 0 var(--space-3) 0;",
                "Recommendations"
            }
            for rec in &store.recommendations {
                div { style: "{REC_STYLE}", "{rec}" }
            }
        }
    }
}

#[component]
fn ConfidenceHistogram(
    buckets: Vec<crate::state::meta::ConfidenceBucket>,
) -> Element {
    if buckets.is_empty() {
        return rsx! {
            div { style: "{MUTED_TEXT}", "No confidence data" }
        };
    }

    let max_count = buckets.iter().map(|b| b.count).max().unwrap_or(1).max(1);
    let bar_height = 100.0;

    rsx! {
        div {
            style: "{CARD_STYLE}",
            div {
                style: "{HISTOGRAM_BAR_STYLE} height: {bar_height}px;",
                for bucket in &buckets {
                    {
                        let h = if max_count > 0 {
                            (f64::from(bucket.count) / f64::from(max_count)) * bar_height
                        } else {
                            0.0
                        };
                        let color = confidence_bucket_color(&bucket.range_label);
                        rsx! {
                            div {
                                style: "display: flex; flex-direction: column; align-items: center; \
                                        flex: 1; justify-content: flex-end;",
                                if bucket.count > 0 {
                                    span {
                                        style: "font-size: var(--text-xs); color: var(--text-secondary); margin-bottom: var(--space-1);",
                                        "{bucket.count}"
                                    }
                                }
                                div {
                                    style: "width: 80%; height: {h:.0}px; background: {color}; \
                                            border-radius: var(--radius-sm) var(--radius-sm) 0 0; min-height: 1px;",
                                }
                            }
                        }
                    }
                }
            }
            div {
                style: "display: flex; margin-top: var(--space-1);",
                for bucket in &buckets {
                    span {
                        style: "flex: 1; text-align: center; font-size: var(--text-xs); color: var(--text-muted);",
                        "{bucket.range_label}"
                    }
                }
            }
        }
    }
}

/// Color for a confidence bucket based on its range.
fn confidence_bucket_color(range_label: &str) -> &'static str {
    // WHY: Higher confidence = greener, lower = redder.
    match range_label {
        s if s.starts_with("0.8") || s.starts_with("0.9") => "var(--status-success)",
        s if s.starts_with("0.6") || s.starts_with("0.7") => "#4a9aff",
        s if s.starts_with("0.4") || s.starts_with("0.5") => "var(--status-warning)",
        _ => "var(--status-error)",
    }
}
