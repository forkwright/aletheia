//! Conversation quality trends: turn length, ratios, depth distribution, topics.

use dioxus::prelude::*;

use crate::state::meta::QualityStore;
use crate::views::meta::{LineChart, CARD_LABEL, CARD_STYLE, GRID_STYLE, MUTED_TEXT};

const HISTOGRAM_BAR_STYLE: &str = "\
    display: flex; \
    align-items: center; \
    gap: var(--space-2); \
    margin-bottom: var(--space-2);\
";

const BAR_LABEL_STYLE: &str = "font-size: var(--text-xs); color: var(--text-secondary); width: 80px; text-align: right;";

const BAR_BG_STYLE: &str = "\
    flex: 1; \
    height: 20px; \
    background: var(--border); \
    border-radius: var(--radius-sm); \
    overflow: hidden;\
";

const BAR_COUNT_STYLE: &str = "font-size: var(--text-xs); color: var(--text-secondary); width: 40px;";

const TOPIC_ROW_STYLE: &str = "\
    display: flex; \
    justify-content: space-between; \
    padding: var(--space-2) 0; \
    border-bottom: 1px solid var(--border);\
";

#[component]
pub(super) fn ConversationQualitySection(store: QualityStore) -> Element {
    rsx! {
        // NOTE: Quality indicator charts.
        h3 { style: "font-size: var(--text-base); color: var(--text-secondary); margin: 0 0 var(--space-3) 0;", "Quality Indicators" }
        div {
            style: "{GRID_STYLE}",
            div {
                style: "{CARD_STYLE} flex: 1; min-width: 280px;",
                div { style: "{CARD_LABEL} margin-bottom: var(--space-2);", "Avg Turn Length" }
                LineChart {
                    data: store.avg_turn_length.clone(),
                    width: 300.0,
                    height: 150.0,
                    color: "#4a9aff",
                    show_labels: true,
                }
            }
            div {
                style: "{CARD_STYLE} flex: 1; min-width: 280px;",
                div { style: "{CARD_LABEL} margin-bottom: var(--space-2);", "Response-to-Question Ratio" }
                LineChart {
                    data: store.response_to_question_ratio.clone(),
                    width: 300.0,
                    height: 150.0,
                    color: "var(--status-success)",
                    show_labels: true,
                }
            }
        }
        div {
            style: "{GRID_STYLE} margin-top: var(--space-3);",
            div {
                style: "{CARD_STYLE} flex: 1; min-width: 280px;",
                div { style: "{CARD_LABEL} margin-bottom: var(--space-2);", "Tool Call Density" }
                LineChart {
                    data: store.tool_call_density.clone(),
                    width: 300.0,
                    height: 150.0,
                    color: "var(--status-warning)",
                    show_labels: true,
                }
            }
            div {
                style: "{CARD_STYLE} flex: 1; min-width: 280px;",
                div { style: "{CARD_LABEL} margin-bottom: var(--space-2);", "Thinking Time Ratio" }
                LineChart {
                    data: store.thinking_time_ratio.clone(),
                    width: 300.0,
                    height: 150.0,
                    color: "#8b5cf6",
                    show_labels: true,
                }
            }
        }

        // NOTE: Session depth distribution.
        h3 {
            style: "font-size: var(--text-base); color: var(--text-secondary); margin: var(--space-4) 0 var(--space-3) 0;",
            "Session Depth Distribution"
        }
        DepthHistogram {
            short: store.depth_distribution.short,
            medium: store.depth_distribution.medium,
            long: store.depth_distribution.long,
        }

        // NOTE: Topics.
        if !store.top_topics.is_empty() {
            h3 {
                style: "font-size: var(--text-base); color: var(--text-secondary); margin: var(--space-4) 0 var(--space-3) 0;",
                "Top Topics"
            }
            div {
                style: "{CARD_STYLE}",
                for topic in &store.top_topics {
                    div {
                        style: "{TOPIC_ROW_STYLE}",
                        span { style: "font-size: var(--text-sm); color: var(--text-primary);", "{topic.name}" }
                        span { style: "font-size: var(--text-xs); color: var(--text-secondary);", "{topic.count} sessions" }
                    }
                }
            }
        }
    }
}

#[component]
fn DepthHistogram(short: u32, medium: u32, long: u32) -> Element {
    let total = short + medium + long;
    let max_val = short.max(medium).max(long).max(1);

    let buckets: Vec<(&str, u32, &str)> = vec![
        ("Short (<10)", short, "#4a9aff"),
        ("Medium (10-50)", medium, "var(--status-success)"),
        ("Long (50+)", long, "var(--status-warning)"),
    ];

    rsx! {
        div {
            style: "{CARD_STYLE}",
            if total == 0 {
                div { style: "{MUTED_TEXT}", "No session data available" }
            } else {
                for (label , count , color) in &buckets {
                    {
                        let pct = if max_val > 0 {
                            (f64::from(*count) / f64::from(max_val)) * 100.0
                        } else {
                            0.0
                        };
                        let session_pct = if total > 0 {
                            (f64::from(*count) / f64::from(total)) * 100.0
                        } else {
                            0.0
                        };
                        rsx! {
                            div {
                                style: "{HISTOGRAM_BAR_STYLE}",
                                span { style: "{BAR_LABEL_STYLE}", "{label}" }
                                div {
                                    style: "{BAR_BG_STYLE}",
                                    div {
                                        style: "height: 100%; width: {pct:.0}%; background: {color}; \
                                                border-radius: var(--radius-sm); transition: width var(--transition-measured);",
                                    }
                                }
                                span { style: "{BAR_COUNT_STYLE}", "{count} ({session_pct:.0}%)" }
                            }
                        }
                    }
                }
                div {
                    style: "margin-top: var(--space-2); font-size: var(--text-xs); color: var(--text-muted);",
                    "Total: {total} sessions"
                }
            }
        }
    }
}
