//! Conversation quality trends: turn length, ratios, depth distribution, topics.

use dioxus::prelude::*;

use crate::state::meta::QualityStore;
use crate::views::meta::{LineChart, CARD_LABEL, CARD_STYLE, GRID_STYLE, MUTED_TEXT};

const HISTOGRAM_BAR_STYLE: &str = "\
    display: flex; \
    align-items: center; \
    gap: 8px; \
    margin-bottom: 6px;\
";

const BAR_LABEL_STYLE: &str = "font-size: 12px; color: #aaa; width: 80px; text-align: right;";

const BAR_BG_STYLE: &str = "\
    flex: 1; \
    height: 20px; \
    background: #2a2a3a; \
    border-radius: 4px; \
    overflow: hidden;\
";

const BAR_COUNT_STYLE: &str = "font-size: 11px; color: #888; width: 40px;";

const TOPIC_ROW_STYLE: &str = "\
    display: flex; \
    justify-content: space-between; \
    padding: 6px 0; \
    border-bottom: 1px solid #2a2a3a;\
";

#[component]
pub(super) fn ConversationQualitySection(store: QualityStore) -> Element {
    rsx! {
        // NOTE: Quality indicator charts.
        h3 { style: "font-size: 14px; color: #aaa; margin: 0 0 12px 0;", "Quality Indicators" }
        div {
            style: "{GRID_STYLE}",
            div {
                style: "{CARD_STYLE} flex: 1; min-width: 280px;",
                div { style: "{CARD_LABEL} margin-bottom: 8px;", "Avg Turn Length" }
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
                div { style: "{CARD_LABEL} margin-bottom: 8px;", "Response-to-Question Ratio" }
                LineChart {
                    data: store.response_to_question_ratio.clone(),
                    width: 300.0,
                    height: 150.0,
                    color: "#22c55e",
                    show_labels: true,
                }
            }
        }
        div {
            style: "{GRID_STYLE} margin-top: 12px;",
            div {
                style: "{CARD_STYLE} flex: 1; min-width: 280px;",
                div { style: "{CARD_LABEL} margin-bottom: 8px;", "Tool Call Density" }
                LineChart {
                    data: store.tool_call_density.clone(),
                    width: 300.0,
                    height: 150.0,
                    color: "#f59e0b",
                    show_labels: true,
                }
            }
            div {
                style: "{CARD_STYLE} flex: 1; min-width: 280px;",
                div { style: "{CARD_LABEL} margin-bottom: 8px;", "Thinking Time Ratio" }
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
            style: "font-size: 14px; color: #aaa; margin: 16px 0 12px 0;",
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
                style: "font-size: 14px; color: #aaa; margin: 16px 0 12px 0;",
                "Top Topics"
            }
            div {
                style: "{CARD_STYLE}",
                for topic in &store.top_topics {
                    div {
                        style: "{TOPIC_ROW_STYLE}",
                        span { style: "font-size: 13px; color: #e0e0e0;", "{topic.name}" }
                        span { style: "font-size: 12px; color: #888;", "{topic.count} sessions" }
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
        ("Medium (10-50)", medium, "#22c55e"),
        ("Long (50+)", long, "#f59e0b"),
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
                                                border-radius: 4px; transition: width 0.3s ease;",
                                    }
                                }
                                span { style: "{BAR_COUNT_STYLE}", "{count} ({session_pct:.0}%)" }
                            }
                        }
                    }
                }
                div {
                    style: "margin-top: 8px; font-size: 11px; color: #555;",
                    "Total: {total} sessions"
                }
            }
        }
    }
}
