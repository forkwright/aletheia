//! Conversation quality trends: turn length, ratios, depth distribution, topics.

use dioxus::prelude::*;

use crate::state::meta::QualityStore;
use crate::views::meta::{CARD_STYLE, ChartCard, ChartKind, GRID_STYLE, MUTED_TEXT};

const GRID_CARD_STYLE: &str = "flex: 1; min-width: 280px;";

const HISTOGRAM_BAR_STYLE: &str = "\
    display: flex; \
    align-items: center; \
    gap: var(--space-2); \
    margin-bottom: var(--space-2);\
";

const BAR_LABEL_STYLE: &str =
    "font-size: var(--text-xs); color: var(--text-secondary); width: 80px; text-align: right;";

const BAR_BG_STYLE: &str = "\
    flex: 1; \
    height: 20px; \
    background: var(--border); \
    border-radius: var(--radius-sm); \
    overflow: hidden;\
";

const BAR_COUNT_STYLE: &str =
    "font-size: var(--text-xs); color: var(--text-secondary); width: 40px;";

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
        if store.charts_endpoint_available {
            div {
                style: "{GRID_STYLE}",
                ChartCard {
                    title: "Avg Turn Length",
                    kind: ChartKind::Line,
                    data: store.avg_turn_length.clone(),
                    color: "var(--status-info)",
                    width: 460.0,
                    height: 220.0,
                    card_style: GRID_CARD_STYLE,
                }
                ChartCard {
                    title: "Response-to-Question Ratio",
                    kind: ChartKind::Line,
                    data: store.response_to_question_ratio.clone(),
                    color: "var(--status-success)",
                    width: 460.0,
                    height: 220.0,
                    card_style: GRID_CARD_STYLE,
                }
            }
            div {
                style: "{GRID_STYLE} margin-top: var(--space-3);",
                ChartCard {
                    title: "Tool Call Density",
                    kind: ChartKind::Line,
                    data: store.tool_call_density.clone(),
                    color: "var(--status-warning)",
                    width: 460.0,
                    height: 220.0,
                    card_style: GRID_CARD_STYLE,
                }
                ChartCard {
                    title: "Thinking Time Ratio",
                    kind: ChartKind::Line,
                    data: store.thinking_time_ratio.clone(),
                    color: "var(--thanatochromia)",
                    width: 460.0,
                    height: 220.0,
                    card_style: GRID_CARD_STYLE,
                }
            }
        } else {
            div {
                style: "{MUTED_TEXT} margin-bottom: var(--space-4);",
                "Conversation quality time-series endpoint not available on this pylon instance."
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
        ("Short (<10)", short, "var(--status-info)"),
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
