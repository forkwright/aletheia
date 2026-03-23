//! Agent performance patterns: scorecards, radar chart, anomaly detection.

use dioxus::prelude::*;

use crate::state::meta::{AgentPerformanceStore, AgentScorecard, Anomaly};
use crate::views::meta::{GRID_STYLE, MUTED_TEXT};

const RADAR_SIZE: f64 = 250.0;
const RADAR_CENTER: f64 = RADAR_SIZE / 2.0;
const RADAR_RADIUS: f64 = 90.0;

const RADAR_AXIS_LABELS: [&str; 5] = [
    "Quality",
    "Tool Eff.",
    "Context",
    "Productivity",
    "Reliability",
];

const RADAR_COLORS: &[&str] = &["#4a9aff", "#22c55e", "#f59e0b", "#ef4444", "#8b5cf6"];

const SCORECARD_STYLE: &str = "\
    background: #1a1a2e; \
    border: 1px solid #333; \
    border-radius: 8px; \
    padding: 16px; \
    min-width: 220px; \
    flex: 1;\
";

const ALERT_STYLE: &str = "\
    background: #2a1a1a; \
    border: 1px solid #4a2a2a; \
    border-radius: 8px; \
    padding: 12px 16px; \
    font-size: 13px; \
    color: #f87171;\
";

#[component]
pub(super) fn AgentPerformanceSection(store: AgentPerformanceStore) -> Element {
    rsx! {
        // NOTE: Anomaly alerts at the top.
        if !store.anomalies.is_empty() {
            div {
                style: "display: flex; flex-direction: column; gap: 8px; margin-bottom: 16px;",
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
                style: "margin-bottom: 16px;",
                h3 { style: "font-size: 14px; color: #aaa; margin: 0 0 12px 0;", "Comparative Radar" }
                RadarChart { scorecards: store.scorecards.clone() }
            }

            // NOTE: Individual scorecards.
            h3 { style: "font-size: 14px; color: #aaa; margin: 0 0 12px 0;", "Agent Scorecards" }
            div {
                style: "{GRID_STYLE}",
                for card in &store.scorecards {
                    ScorecardCard { scorecard: card.clone() }
                }
            }
        }
    }
}

#[component]
fn ScorecardCard(scorecard: AgentScorecard) -> Element {
    rsx! {
        div {
            style: "{SCORECARD_STYLE}",
            div {
                style: "font-size: 15px; font-weight: 600; color: #e0e0e0; margin-bottom: 12px;",
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
            style: "display: flex; justify-content: space-between; padding: 4px 0; \
                    border-bottom: 1px solid #2a2a3a;",
            span { style: "font-size: 12px; color: #888;", "{label}" }
            span { style: "font-size: 12px; color: #e0e0e0; font-weight: 500;", "{value}" }
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
            span { style: "color: {color}; margin-right: 8px;", "{arrow}" }
            "{msg}"
        }
    }
}

// -- Radar/spider chart -------------------------------------------------------

#[component]
fn RadarChart(scorecards: Vec<AgentScorecard>) -> Element {
    let axis_count = 5;
    let angle_step = std::f64::consts::TAU / axis_count as f64;

    // WHY: Compute polygon vertices for each ring level.
    let ring_levels = [0.25, 0.5, 0.75, 1.0];

    let viewbox = format!("0 0 {RADAR_SIZE} {RADAR_SIZE}");

    rsx! {
        div {
            style: "display: flex; align-items: flex-start; gap: 16px; flex-wrap: wrap;",
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
                                stroke: "#2a2a3a",
                                stroke_width: "1",
                            }
                        }
                    }
                }

                // NOTE: Axis lines.
                for i in 0..axis_count {
                    {
                        let angle = (i as f64 * angle_step) - std::f64::consts::FRAC_PI_2;
                        let x = RADAR_CENTER + RADAR_RADIUS * angle.cos();
                        let y = RADAR_CENTER + RADAR_RADIUS * angle.sin();
                        rsx! {
                            line {
                                x1: "{RADAR_CENTER:.1}",
                                y1: "{RADAR_CENTER:.1}",
                                x2: "{x:.1}",
                                y2: "{y:.1}",
                                stroke: "#333",
                                stroke_width: "1",
                            }
                        }
                    }
                }

                // NOTE: Axis labels.
                for (i , label) in RADAR_AXIS_LABELS.iter().enumerate() {
                    {
                        let angle = (i as f64 * angle_step) - std::f64::consts::FRAC_PI_2;
                        let label_r = RADAR_RADIUS + 18.0;
                        let x = RADAR_CENTER + label_r * angle.cos();
                        let y = RADAR_CENTER + label_r * angle.sin();
                        rsx! {
                            text {
                                x: "{x:.1}",
                                y: "{y:.1}",
                                fill: "#888",
                                font_size: "10",
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
                                let angle = (i as f64 * angle_step) - std::f64::consts::FRAC_PI_2;
                                let r = RADAR_RADIUS * axes[i];
                                let x = RADAR_CENTER + r * angle.cos();
                                let y = RADAR_CENTER + r * angle.sin();
                                format!("{x:.1},{y:.1}")
                            })
                            .collect::<Vec<_>>()
                            .join(" ");
                        let color = RADAR_COLORS[idx % RADAR_COLORS.len()];
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
                style: "display: flex; flex-direction: column; gap: 6px;",
                for (idx , card) in scorecards.iter().enumerate() {
                    {
                        let color = RADAR_COLORS[idx % RADAR_COLORS.len()];
                        rsx! {
                            div {
                                style: "display: flex; align-items: center; gap: 8px;",
                                div {
                                    style: "width: 12px; height: 12px; border-radius: 2px; background: {color};",
                                }
                                span { style: "font-size: 12px; color: #aaa;", "{card.agent_name}" }
                            }
                        }
                    }
                }
            }
        }
    }
}
