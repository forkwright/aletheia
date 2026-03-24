//! Agent cost breakdown: grouped bar chart (current vs previous) + efficiency table.

use dioxus::prelude::*;

use crate::components::chart::{GroupedBarChart, GroupedBarEntry};
use crate::state::metrics::{
    agent_color, format_cost, sort_agent_cost_rows, AgentCostRow, AgentCostSort, SortDir,
};

/// Per-agent cost comparison with grouped bar chart and efficiency metrics.
#[component]
pub(crate) fn AgentCosts(agents: Vec<AgentCostRow>) -> Element {
    let sort_col = use_signal(|| AgentCostSort::TotalCost);
    let sort_dir = use_signal(|| SortDir::Desc);

    let mut sorted = agents.clone();
    sort_agent_cost_rows(&mut sorted, *sort_col.read(), *sort_dir.read());

    let max_cost = sorted
        .iter()
        .flat_map(|a| [a.total_cost, a.prev_period_cost])
        .fold(0.0f64, f64::max);

    // Most expensive = highest total_cost (first when sorted desc by TotalCost)
    let most_expensive_id = sorted.first().map(|a| a.id.clone());
    // Most efficient = lowest cost_per_1k_output among agents with output tokens
    let most_efficient_id = sorted
        .iter()
        .filter(|a| a.output_tokens > 0 && a.total_cost > 0.0)
        .min_by(|a, b| {
            a.cost_per_1k_output()
                .partial_cmp(&b.cost_per_1k_output())
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .map(|a| a.id.clone());

    let bar_entries: Vec<GroupedBarEntry> = sorted
        .iter()
        .enumerate()
        .map(|(i, a)| GroupedBarEntry {
            label: a.name.clone(),
            current: a.total_cost,
            previous: a.prev_period_cost,
            current_color: agent_color(i).to_string(),
            previous_color: muted_color(i),
        })
        .collect();

    rsx! {
        div {
            style: "display: flex; flex-direction: column; gap: 16px;",

            if max_cost > 0.0 {
                GroupedBarChart {
                    entries: bar_entries,
                    height_px: 160,
                    current_label: "Current".to_string(),
                    previous_label: "Previous".to_string(),
                }
            }

            // Efficiency table
            div {
                style: "overflow-x: auto;",
                table {
                    style: "width: 100%; border-collapse: collapse; font-size: 12px; font-family: 'IBM Plex Mono', monospace;",
                    thead {
                        tr {
                            style: "border-bottom: 1px solid #2a2724;",
                            { cost_th("Agent", AgentCostSort::Name, sort_col, sort_dir) }
                            { cost_th("Total Cost", AgentCostSort::TotalCost, sort_col, sort_dir) }
                            { cost_th("$/Session", AgentCostSort::CostPerSession, sort_col, sort_dir) }
                            { cost_th("$/Message", AgentCostSort::CostPerMessage, sort_col, sort_dir) }
                            { cost_th("$/1K out", AgentCostSort::CostPer1k, sort_col, sort_dir) }
                        }
                    }
                    tbody {
                        for (idx, agent) in sorted.iter().enumerate() {
                            {
                                let color = agent_color(idx);
                                let is_expensive = most_expensive_id.as_deref() == Some(&agent.id);
                                let is_efficient = most_efficient_id.as_deref() == Some(&agent.id);
                                let per_1k = format!("{:.4}", agent.cost_per_1k_output());
                                rsx! {
                                    tr {
                                        key: "{agent.id}",
                                        style: "border-bottom: 1px solid #2a2724;",
                                        td {
                                            style: "padding: 6px 8px; color: {color}; white-space: nowrap;",
                                            "{agent.name}"
                                            if is_expensive {
                                                span { style: "margin-left: 6px; font-size: 10px; background: #7f1d1d; color: #fca5a5; padding: 1px 4px; border-radius: 3px;", "highest" }
                                            }
                                            if is_efficient && !is_expensive {
                                                span { style: "margin-left: 6px; font-size: 10px; background: #14532d; color: #86efac; padding: 1px 4px; border-radius: 3px;", "efficient" }
                                            }
                                        }
                                        td { style: "padding: 6px 8px; color: #eab308; text-align: right; font-weight: 600;", "{format_cost(agent.total_cost)}" }
                                        td { style: "padding: 6px 8px; color: #a8a49e; text-align: right;", "{format_cost(agent.cost_per_session())}" }
                                        td { style: "padding: 6px 8px; color: #a8a49e; text-align: right;", "{format_cost(agent.cost_per_message())}" }
                                        td { style: "padding: 6px 8px; color: #706c66; text-align: right;", "${per_1k}" }
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

fn cost_th(
    label: &str,
    col: AgentCostSort,
    mut sort_col: Signal<AgentCostSort>,
    mut sort_dir: Signal<SortDir>,
) -> Element {
    let is_active = *sort_col.read() == col;
    let indicator = if is_active {
        if *sort_dir.read() == SortDir::Desc { " ↓" } else { " ↑" }
    } else {
        ""
    };
    let label = label.to_string();
    rsx! {
        th {
            style: "padding: 6px 8px; text-align: right; color: #706c66; cursor: pointer; user-select: none; white-space: nowrap;",
            onclick: move |_| {
                if *sort_col.read() == col {
                    let new_dir = sort_dir.read().flip();
                    sort_dir.set(new_dir);
                } else {
                    sort_col.set(col);
                    sort_dir.set(SortDir::Desc);
                }
            },
            "{label}{indicator}"
        }
    }
}

/// Muted variant of an accent color for previous-period bars.
///
/// WHY: previous-period bars use 40% opacity to visually recede behind current-period bars.
fn muted_color(index: usize) -> String {
    const PALETTE: &[&str] = &[
        "rgba(91,106,240,0.4)",
        "rgba(16,185,129,0.4)",
        "rgba(245,158,11,0.4)",
        "rgba(244,63,94,0.4)",
        "rgba(14,165,233,0.4)",
        "rgba(139,92,246,0.4)",
        "rgba(236,72,153,0.4)",
        "rgba(20,184,166,0.4)",
    ];
    PALETTE[index % PALETTE.len()].to_string()
}
