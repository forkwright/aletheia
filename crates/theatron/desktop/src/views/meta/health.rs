//! Memory health dashboard: composite score, confidence distribution, staleness.

use dioxus::prelude::*;

use crate::state::meta::{health_score_color, MemoryHealthStore};
use crate::views::meta::{
    LineChart, CARD_LABEL, CARD_STYLE, CARD_SUB, CARD_VALUE, GRID_STYLE, MUTED_TEXT,
};

const TABLE_HEADER_STYLE: &str = "\
    display: flex; \
    padding: 8px 12px; \
    background: #1a1a2e; \
    border-bottom: 1px solid #333; \
    font-size: 11px; \
    color: #888; \
    text-transform: uppercase; \
    letter-spacing: 0.5px;\
";

const TABLE_ROW_STYLE: &str = "\
    display: flex; \
    padding: 8px 12px; \
    border-bottom: 1px solid #2a2a3a; \
    font-size: 13px; \
    color: #e0e0e0;\
";

const REC_STYLE: &str = "\
    background: #1a1a2e; \
    border-left: 3px solid #f59e0b; \
    padding: 10px 14px; \
    font-size: 13px; \
    color: #e0e0e0; \
    margin-bottom: 6px; \
    border-radius: 0 6px 6px 0;\
";

const HISTOGRAM_BAR_STYLE: &str = "\
    display: flex; \
    align-items: flex-end; \
    gap: 2px;\
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
            style: "font-size: 14px; color: #aaa; margin: 16px 0 12px 0;",
            "Confidence Distribution"
        }
        ConfidenceHistogram { buckets: store.confidence_distribution.clone() }

        // NOTE: Health trend over time.
        if !store.health_over_time.is_empty() {
            h3 {
                style: "font-size: 14px; color: #aaa; margin: 16px 0 12px 0;",
                "Health Trend"
            }
            div {
                style: "{CARD_STYLE}",
                LineChart {
                    data: store.health_over_time.clone(),
                    width: 600.0,
                    height: 150.0,
                    color: "#22c55e",
                    show_labels: true,
                }
            }
        }

        // NOTE: Staleness table.
        if !store.stale_entities.is_empty() {
            h3 {
                style: "font-size: 14px; color: #aaa; margin: 16px 0 12px 0;",
                "Stale Entities"
            }
            div {
                style: "border: 1px solid #333; border-radius: 8px; overflow: hidden;",
                div {
                    style: "{TABLE_HEADER_STYLE}",
                    span { style: "flex: 2;", "Entity" }
                    span { style: "flex: 1;", "Last Updated" }
                    span { style: "flex: 1; text-align: right;", "Days Stale" }
                }
                for entity in &store.stale_entities {
                    {
                        let staleness_color = if entity.days_stale > 90 {
                            "#ef4444"
                        } else if entity.days_stale > 60 {
                            "#f59e0b"
                        } else {
                            "#888"
                        };
                        rsx! {
                            div {
                                style: "{TABLE_ROW_STYLE}",
                                span { style: "flex: 2;", "{entity.name}" }
                                span { style: "flex: 1; color: #888;", "{entity.last_updated}" }
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
                style: "font-size: 14px; color: #aaa; margin: 16px 0 12px 0;",
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
                                        style: "font-size: 10px; color: #888; margin-bottom: 2px;",
                                        "{bucket.count}"
                                    }
                                }
                                div {
                                    style: "width: 80%; height: {h:.0}px; background: {color}; \
                                            border-radius: 2px 2px 0 0; min-height: 1px;",
                                }
                            }
                        }
                    }
                }
            }
            div {
                style: "display: flex; margin-top: 4px;",
                for bucket in &buckets {
                    span {
                        style: "flex: 1; text-align: center; font-size: 9px; color: #666;",
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
        s if s.starts_with("0.8") || s.starts_with("0.9") => "#22c55e",
        s if s.starts_with("0.6") || s.starts_with("0.7") => "#4a9aff",
        s if s.starts_with("0.4") || s.starts_with("0.5") => "#f59e0b",
        _ => "#ef4444",
    }
}
