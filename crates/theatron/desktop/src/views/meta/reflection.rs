//! System self-reflection: overview, activity heatmap, efficiency, journal.

use dioxus::prelude::*;

use crate::state::meta::{
    heatmap_color, JournalEvent, JournalEventType, SystemReflectionStore,
};
use crate::views::meta::{CARD_LABEL, CARD_STYLE, CARD_SUB, CARD_VALUE, GRID_STYLE, MUTED_TEXT};

const DAY_LABELS: [&str; 7] = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];

const CELL_SIZE: f64 = 18.0;
const CELL_GAP: f64 = 2.0;

const JOURNAL_ROW_STYLE: &str = "\
    display: flex; \
    align-items: center; \
    gap: 12px; \
    padding: 8px 12px; \
    border-bottom: 1px solid #2a2a3a;\
";

const BADGE_STYLE: &str = "\
    padding: 2px 6px; \
    border-radius: 4px; \
    font-size: 10px; \
    font-weight: 600; \
    text-transform: uppercase;\
";

const FILTER_BTN_STYLE: &str = "\
    background: #2a2a4a; \
    color: #aaa; \
    border: 1px solid #444; \
    border-radius: 4px; \
    padding: 3px 8px; \
    font-size: 11px; \
    cursor: pointer;\
";

#[component]
pub(super) fn SystemReflectionSection(store: SystemReflectionStore) -> Element {
    rsx! {
        // NOTE: System overview.
        h3 { style: "font-size: 14px; color: #aaa; margin: 0 0 12px 0;", "System Overview" }
        div {
            style: "{GRID_STYLE}",
            OverviewCard {
                value: format_uptime(store.overview.uptime_seconds),
                label: "Uptime",
            }
            OverviewCard {
                value: store.overview.total_sessions.to_string(),
                label: "Total Sessions",
            }
            OverviewCard {
                value: format_tokens(store.overview.total_tokens),
                label: "Total Tokens",
            }
            OverviewCard {
                value: store.overview.total_entities.to_string(),
                label: "Knowledge Entities",
            }
            OverviewCard {
                value: format_cost(store.overview.total_cost_usd),
                label: "Total Cost",
            }
        }

        // NOTE: Activity heatmap.
        h3 {
            style: "font-size: 14px; color: #aaa; margin: 16px 0 12px 0;",
            "Activity Heatmap"
        }
        ActivityHeatmap { cells: store.heatmap.clone() }

        // NOTE: Resource efficiency.
        h3 {
            style: "font-size: 14px; color: #aaa; margin: 16px 0 12px 0;",
            "Resource Efficiency"
        }
        EfficiencyPanel {
            cost_per_entity: store.efficiency.cost_per_entity,
            cost_per_session: store.efficiency.cost_per_session,
            tokens_per_entity: store.efficiency.tokens_per_entity,
            trend: store.efficiency.cost_per_entity_trend,
        }

        // NOTE: System journal.
        h3 {
            style: "font-size: 14px; color: #aaa; margin: 16px 0 12px 0;",
            "System Journal"
        }
        SystemJournal { events: store.journal.clone() }
    }
}

#[component]
fn OverviewCard(value: String, label: &'static str) -> Element {
    rsx! {
        div {
            style: "{CARD_STYLE} flex: 1; min-width: 120px; text-align: center;",
            div { style: "{CARD_VALUE}", "{value}" }
            div { style: "{CARD_LABEL}", "{label}" }
        }
    }
}

// -- Activity heatmap ---------------------------------------------------------

#[component]
fn ActivityHeatmap(cells: Vec<crate::state::meta::HeatmapCell>) -> Element {
    if cells.is_empty() {
        return rsx! {
            div { style: "{MUTED_TEXT}", "No activity data available" }
        };
    }

    let max_count = cells.iter().map(|c| c.count).max().unwrap_or(0);

    // WHY: Grid layout — 7 rows (days) x 24 columns (hours).
    let grid_width = 24.0 * (CELL_SIZE + CELL_GAP) + 40.0;
    let grid_height = 7.0 * (CELL_SIZE + CELL_GAP) + 30.0;
    let viewbox = format!("0 0 {grid_width} {grid_height}");

    rsx! {
        div {
            style: "{CARD_STYLE} overflow-x: auto;",
            svg {
                width: "{grid_width}",
                height: "{grid_height}",
                view_box: "{viewbox}",
                style: "display: block;",

                // NOTE: Hour labels along top.
                for h in (0..24u8).step_by(3) {
                    {
                        let x = 40.0 + f64::from(h) * (CELL_SIZE + CELL_GAP) + CELL_SIZE / 2.0;
                        rsx! {
                            text {
                                x: "{x:.1}",
                                y: "10",
                                fill: "#666",
                                font_size: "9",
                                text_anchor: "middle",
                                "{h}:00"
                            }
                        }
                    }
                }

                // NOTE: Day labels along left.
                for (d , label) in DAY_LABELS.iter().enumerate() {
                    {
                        let y = 20.0 + d as f64 * (CELL_SIZE + CELL_GAP) + CELL_SIZE / 2.0;
                        rsx! {
                            text {
                                x: "0",
                                y: "{y:.1}",
                                fill: "#666",
                                font_size: "9",
                                dominant_baseline: "middle",
                                "{label}"
                            }
                        }
                    }
                }

                // NOTE: Heatmap cells.
                for cell in &cells {
                    {
                        let x = 40.0 + f64::from(cell.hour) * (CELL_SIZE + CELL_GAP);
                        let y = 20.0 + f64::from(cell.day) * (CELL_SIZE + CELL_GAP);
                        let color = heatmap_color(cell.count, max_count);
                        rsx! {
                            rect {
                                x: "{x:.1}",
                                y: "{y:.1}",
                                width: "{CELL_SIZE}",
                                height: "{CELL_SIZE}",
                                fill: "{color}",
                                rx: "2",
                            }
                        }
                    }
                }
            }

            // NOTE: Legend.
            div {
                style: "display: flex; align-items: center; gap: 4px; margin-top: 8px; \
                        font-size: 10px; color: #666;",
                span { "Less" }
                for color in &["#1a1a2e", "#1a2a3e", "#2a4a6a", "#4a9aff", "#22c55e"] {
                    div {
                        style: "width: 12px; height: 12px; background: {color}; \
                                border-radius: 2px;",
                    }
                }
                span { "More" }
            }
        }
    }
}

// -- Efficiency panel ---------------------------------------------------------

#[component]
fn EfficiencyPanel(
    cost_per_entity: f64,
    cost_per_session: f64,
    tokens_per_entity: f64,
    trend: crate::state::meta::TrendDirection,
) -> Element {
    let trend_arrow = trend.arrow();
    let trend_color = trend.color();

    rsx! {
        div {
            style: "{GRID_STYLE}",
            div {
                style: "{CARD_STYLE} flex: 1; min-width: 140px; text-align: center;",
                div { style: "{CARD_VALUE}", "{format_cost(cost_per_entity)}" }
                div { style: "{CARD_LABEL}", "Cost / Entity" }
                div {
                    style: "{CARD_SUB} color: {trend_color};",
                    "{trend_arrow} directional indicator"
                }
            }
            div {
                style: "{CARD_STYLE} flex: 1; min-width: 140px; text-align: center;",
                div { style: "{CARD_VALUE}", "{format_cost(cost_per_session)}" }
                div { style: "{CARD_LABEL}", "Cost / Session" }
            }
            div {
                style: "{CARD_STYLE} flex: 1; min-width: 140px; text-align: center;",
                div { style: "{CARD_VALUE}", "{format_tokens(tokens_per_entity as u64)}" }
                div { style: "{CARD_LABEL}", "Tokens / Entity" }
                div { style: "{CARD_SUB}", "Conversation cost per knowledge node" }
            }
        }
    }
}

// -- System journal -----------------------------------------------------------

#[component]
fn SystemJournal(events: Vec<JournalEvent>) -> Element {
    let mut filter = use_signal(|| Option::<JournalEventType>::None);

    let filtered: Vec<&JournalEvent> = events
        .iter()
        .filter(|e| filter.read().is_none_or(|f| e.event_type == f))
        .collect();

    rsx! {
        div {
            style: "{CARD_STYLE}",
            div {
                style: "display: flex; gap: 6px; margin-bottom: 12px;",
                button {
                    style: "{FILTER_BTN_STYLE}",
                    onclick: move |_| filter.set(None),
                    "All"
                }
                button {
                    style: "{FILTER_BTN_STYLE}",
                    onclick: move |_| filter.set(Some(JournalEventType::Error)),
                    "Errors"
                }
                button {
                    style: "{FILTER_BTN_STYLE}",
                    onclick: move |_| filter.set(Some(JournalEventType::Distillation)),
                    "Distillation"
                }
                button {
                    style: "{FILTER_BTN_STYLE}",
                    onclick: move |_| filter.set(Some(JournalEventType::ConfigChange)),
                    "Config"
                }
                button {
                    style: "{FILTER_BTN_STYLE}",
                    onclick: move |_| filter.set(Some(JournalEventType::MemoryMerge)),
                    "Memory"
                }
            }

            if filtered.is_empty() {
                div {
                    style: "{MUTED_TEXT} padding: 16px; text-align: center;",
                    "No journal events recorded yet"
                }
            } else {
                div {
                    style: "max-height: 300px; overflow-y: auto;",
                    for event in &filtered {
                        {
                            let badge_bg = event.event_type.color();
                            let label = event.event_type.label();
                            rsx! {
                                div {
                                    style: "{JOURNAL_ROW_STYLE}",
                                    span {
                                        style: "font-size: 11px; color: #666; width: 140px; flex-shrink: 0;",
                                        "{event.timestamp}"
                                    }
                                    span {
                                        style: "{BADGE_STYLE} background: {badge_bg}20; color: {badge_bg};",
                                        "{label}"
                                    }
                                    span {
                                        style: "font-size: 13px; color: #e0e0e0;",
                                        "{event.message}"
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

// -- Formatters ---------------------------------------------------------------

fn format_uptime(seconds: u64) -> String {
    let days = seconds / 86400;
    let hours = (seconds % 86400) / 3600;
    let mins = (seconds % 3600) / 60;

    if days > 0 {
        format!("{days}d {hours}h")
    } else if hours > 0 {
        format!("{hours}h {mins}m")
    } else {
        format!("{mins}m")
    }
}

fn format_cost(value: f64) -> String {
    if value < 0.01 && value > 0.0 {
        format!("${value:.4}")
    } else {
        format!("${value:.2}")
    }
}

#[expect(
    clippy::cast_precision_loss,
    reason = "display-only: sub-token precision irrelevant"
)]
fn format_tokens(count: u64) -> String {
    const K: u64 = 1_000;
    const M: u64 = 1_000_000;

    if count >= M {
        format!("{:.1}M", count as f64 / M as f64)
    } else if count >= K {
        format!("{:.1}K", count as f64 / K as f64)
    } else {
        count.to_string()
    }
}
