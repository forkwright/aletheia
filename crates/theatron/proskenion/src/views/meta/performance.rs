//! Agent performance patterns: scorecards, radar chart, anomaly detection.

use dioxus::prelude::*;

use crate::state::meta::{AgentPerformanceStore, AgentScorecard, Anomaly};
use crate::views::meta::{GRID_STYLE, META_SERIES_COLORS, MUTED_TEXT};

const RADAR_SIZE: f64 = 300.0;
const RADAR_CENTER: f64 = RADAR_SIZE / 2.0;
const RADAR_RADIUS: f64 = 110.0;

const RADAR_AXIS_LABELS: [&str; 5] = [
    "Quality",
    "Tool Eff.",
    "Context",
    "Productivity",
    "Reliability",
];

const SCORECARD_STYLE: &str = "\
    background: var(--bg-surface); \
    border: 1px solid var(--border); \
    border-radius: var(--radius-md); \
    padding: var(--space-4); \
    min-width: 220px; \
    flex: 1;\
";

const ALERT_STYLE: &str = "\
    background: var(--status-error-bg); \
    border: 1px solid var(--status-error); \
    border-radius: var(--radius-md); \
    padding: var(--space-3) var(--space-4); \
    font-size: var(--text-sm); \
    color: var(--status-error);\
";

#[component]
pub(super) fn AgentPerformanceSection(store: AgentPerformanceStore) -> Element {
    rsx! {
        if !store.endpoint_available {
            div {
                style: "{MUTED_TEXT} margin-bottom: var(--space-4);",
                "Agent performance metrics endpoint not available on this pylon instance."
            }

            if !store.scorecards.is_empty() {
                h3 { style: "font-size: var(--text-base); color: var(--text-secondary); margin: 0 0 var(--space-3) 0;", "Agent Scorecards" }
                div {
                    style: "{GRID_STYLE}",
                    for card in &store.scorecards {
                        SimplifiedScorecardCard { scorecard: card.clone() }
                    }
                }
            }
        } else {
            // NOTE: Anomaly alerts at the top.
            if !store.anomalies.is_empty() {
                div {
                    style: "display: flex; flex-direction: column; gap: var(--space-2); margin-bottom: var(--space-4);",
                    for anomaly in &store.anomalies {
                        AnomalyCard { anomaly: anomaly.clone() }
                    }
                }
            }

            if store.scorecards.is_empty() {
                div { style: "{MUTED_TEXT}", "No agent data available" }
            } else {
                // NOTE: Radar chart (comparative).
                div {
                    style: "margin-bottom: var(--space-4);",
                    h3 { style: "font-size: var(--text-base); color: var(--text-secondary); margin: 0 0 var(--space-3) 0;", "Comparative Radar" }
                    RadarChart { scorecards: store.scorecards.clone() }
                }

                // NOTE: Individual scorecards.
                h3 { style: "font-size: var(--text-base); color: var(--text-secondary); margin: 0 0 var(--space-3) 0;", "Agent Scorecards" }
                div {
                    style: "{GRID_STYLE}",
                    for card in &store.scorecards {
                        FullScorecardCard { scorecard: card.clone() }
                    }
                }
            }
        }
    }
}

#[component]
fn SimplifiedScorecardCard(scorecard: AgentScorecard) -> Element {
    rsx! {
        div {
            style: "{SCORECARD_STYLE}",
            div {
                style: "font-size: var(--text-md); font-weight: var(--weight-semibold); color: var(--text-primary); margin-bottom: var(--space-3);",
                "{scorecard.agent_name}"
            }
            MetricRow {
                label: "Msgs/session",
                value: format!("{:.1}", scorecard.messages_per_session),
            }
            MetricRow {
                label: "Sessions/day",
                value: format!("{:.1}", scorecard.sessions_per_day),
            }
        }
    }
}

#[component]
fn FullScorecardCard(scorecard: AgentScorecard) -> Element {
    rsx! {
        div {
            style: "{SCORECARD_STYLE}",
            div {
                style: "font-size: var(--text-md); font-weight: var(--weight-semibold); color: var(--text-primary); margin-bottom: var(--space-3);",
                "{scorecard.agent_name}"
            }
            MetricRow {
                label: "Msgs/session",
                value: format!("{:.1}", scorecard.messages_per_session),
            }
            MetricRow {
                label: "Tool success rate",
                value: format!("{:.0}%", scorecard.tool_success_rate * 100.0),
            }
            MetricRow {
                label: "Tokens/response",
                value: format!("{:.0}", scorecard.avg_tokens_per_response),
            }
            MetricRow {
                label: "Errors/session",
                value: format!("{:.2}", scorecard.errors_per_session),
            }
            MetricRow {
                label: "Sessions/day",
                value: format!("{:.1}", scorecard.sessions_per_day),
            }
        }
    }
}

#[component]
fn MetricRow(label: &'static str, value: String) -> Element {
    rsx! {
        div {
            style: "display: flex; justify-content: space-between; padding: var(--space-1) 0; \
                    border-bottom: 1px solid var(--border);",
            span { style: "font-size: var(--text-xs); color: var(--text-secondary);", "{label}" }
            span { style: "font-size: var(--text-xs); color: var(--text-primary); font-weight: var(--weight-medium);", "{value}" }
        }
    }
}

#[component]
fn AnomalyCard(anomaly: Anomaly) -> Element {
    let msg = anomaly.message();
    let arrow = anomaly.direction.arrow();
    let color = anomaly.direction.color();

    rsx! {
        div {
            style: "{ALERT_STYLE}",
            span { style: "color: {color}; margin-right: var(--space-2);", "{arrow}" }
            "{msg}"
        }
    }
}

// ── Radar/spider chart ──

#[component]
fn RadarChart(scorecards: Vec<AgentScorecard>) -> Element {
    let axis_count = 5;
    #[expect(clippy::as_conversions, reason = "axis count to f64 for angle step")]
    let angle_step = std::f64::consts::TAU / axis_count as f64;

    // WHY: Compute polygon vertices for each ring level.
    let ring_levels = [0.25, 0.5, 0.75, 1.0];

    let viewbox = format!("0 0 {RADAR_SIZE} {RADAR_SIZE}");

    rsx! {
        div {
            style: "display: flex; align-items: flex-start; gap: var(--space-4); flex-wrap: wrap;",
            svg {
                width: "{RADAR_SIZE}",
                height: "{RADAR_SIZE}",
                view_box: "{viewbox}",
                style: "display: block;",

                // NOTE: Grid rings.
                for level in ring_levels {
                    {
                        let ring_points = (0..axis_count)
                            .map(|i| {
                                #[expect(clippy::as_conversions, reason = "axis index to f64 for radar chart angle")]
                                let angle = (i as f64 * angle_step) - std::f64::consts::FRAC_PI_2;
                                let r = RADAR_RADIUS * level;
                                let x = RADAR_CENTER + r * angle.cos();
                                let y = RADAR_CENTER + r * angle.sin();
                                format!("{x:.1},{y:.1}")
                            })
                            .collect::<Vec<_>>()
                            .join(" ");
                        rsx! {
                            polygon {
                                points: "{ring_points}",
                                fill: "none",
                                stroke: "var(--border)",
                                stroke_width: "1",
                            }
                        }
                    }
                }

                // NOTE: Axis lines.
                for i in 0..axis_count {
                    {
                        #[expect(clippy::as_conversions, reason = "axis index to f64 for radar chart angle")]
                                let angle = (i as f64 * angle_step) - std::f64::consts::FRAC_PI_2;
                        let x = RADAR_CENTER + RADAR_RADIUS * angle.cos();
                        let y = RADAR_CENTER + RADAR_RADIUS * angle.sin();
                        rsx! {
                            line {
                                x1: "{RADAR_CENTER:.1}",
                                y1: "{RADAR_CENTER:.1}",
                                x2: "{x:.1}",
                                y2: "{y:.1}",
                                stroke: "var(--border)",
                                stroke_width: "1",
                            }
                        }
                    }
                }

                // NOTE: Axis labels.
                for (i , label) in RADAR_AXIS_LABELS.iter().enumerate() {
                    {
                        #[expect(clippy::as_conversions, reason = "axis index to f64 for radar chart angle")]
                                let angle = (i as f64 * angle_step) - std::f64::consts::FRAC_PI_2;
                        let label_r = RADAR_RADIUS + 18.0;
                        let x = RADAR_CENTER + label_r * angle.cos();
                        let y = RADAR_CENTER + label_r * angle.sin();
                        rsx! {
                            text {
                                x: "{x:.1}",
                                y: "{y:.1}",
                                fill: "var(--text-secondary)",
                                font_size: "11",
                                text_anchor: "middle",
                                dominant_baseline: "middle",
                                "{label}"
                            }
                        }
                    }
                }

                // NOTE: Agent polygons.
                for (idx , card) in scorecards.iter().enumerate() {
                    {
                        let axes = card.radar_axes();
                        let points = (0..axis_count)
                            .map(|i| {
                                #[expect(clippy::as_conversions, reason = "axis index to f64 for radar chart angle")]
                                let angle = (i as f64 * angle_step) - std::f64::consts::FRAC_PI_2;
                                let r = RADAR_RADIUS * axes.get(i).copied().unwrap_or(0.0);
                                let x = RADAR_CENTER + r * angle.cos();
                                let y = RADAR_CENTER + r * angle.sin();
                                format!("{x:.1},{y:.1}")
                            })
                            .collect::<Vec<_>>()
                            .join(" ");
                        let color = META_SERIES_COLORS[idx % META_SERIES_COLORS.len()];
                        rsx! {
                            polygon {
                                points: "{points}",
                                fill: "{color}",
                                fill_opacity: "0.15",
                                stroke: "{color}",
                                stroke_width: "2",
                            }
                        }
                    }
                }
            }

            // NOTE: Legend.
            div {
                style: "display: flex; flex-direction: column; gap: var(--space-2);",
                for (idx , card) in scorecards.iter().enumerate() {
                    {
                        let color = META_SERIES_COLORS[idx % META_SERIES_COLORS.len()];
                        rsx! {
                            div {
                                style: "display: flex; align-items: center; gap: var(--space-2);",
                                div {
                                    style: "width: 12px; height: 12px; border-radius: var(--radius-sm); background: {color};",
                                }
                                span { style: "font-size: var(--text-xs); color: var(--text-secondary);", "{card.agent_name}" }
                            }
                        }
                    }
                }
            }
        }
    }
}
