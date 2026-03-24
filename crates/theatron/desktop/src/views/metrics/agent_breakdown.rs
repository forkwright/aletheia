//! Agent token usage breakdown: bar chart + sortable table.

use dioxus::prelude::*;

use crate::components::chart::{ChartEntry, HorizBarChart};
use crate::state::metrics::{
    agent_color, format_tokens, sort_agent_token_rows, AgentTokenRow, AgentTokenSort, SortDir,
};

/// Per-agent token breakdown with filter support.
#[component]
pub(crate) fn AgentBreakdown(
    agents: Vec<AgentTokenRow>,
    grand_total: u64,
    active_filter: Option<String>,
    on_filter: EventHandler<Option<String>>,
) -> Element {
    let sort_col = use_signal(|| AgentTokenSort::Total);
    let sort_dir = use_signal(|| SortDir::Desc);

    let mut sorted = agents.clone();
    sort_agent_token_rows(&mut sorted, *sort_col.read(), *sort_dir.read(), grand_total);

    let bar_entries: Vec<ChartEntry> = sorted
        .iter()
        .enumerate()
        .map(|(i, a)| ChartEntry {
            label: a.name.clone(),
            value: a.total() as f64,
            color: agent_color(i).to_string(),
            sub_label: Some(format_tokens(a.total())),
        })
        .collect();

    let filter_for_click = active_filter.clone();

    rsx! {
        div {
            style: "display: flex; flex-direction: column; gap: 16px;",

            HorizBarChart {
                entries: bar_entries,
                max_value: 0.0,
                show_value: true,
                on_click: move |name: String| {
                    if filter_for_click.as_deref() == Some(&name) {
                        on_filter.call(None);
                    } else {
                        on_filter.call(Some(name));
                    }
                },
            }

            // Sortable table
            div {
                style: "overflow-x: auto;",
                table {
                    style: "width: 100%; border-collapse: collapse; font-size: 12px; font-family: 'IBM Plex Mono', monospace;",
                    thead {
                        tr {
                            style: "border-bottom: 1px solid #2a2724;",
                            { sort_th("Agent", AgentTokenSort::Name, sort_col, sort_dir, grand_total) }
                            { sort_th("Input", AgentTokenSort::Input, sort_col, sort_dir, grand_total) }
                            { sort_th("Output", AgentTokenSort::Output, sort_col, sort_dir, grand_total) }
                            { sort_th("Total", AgentTokenSort::Total, sort_col, sort_dir, grand_total) }
                            { sort_th("% of Total", AgentTokenSort::PctOfTotal, sort_col, sort_dir, grand_total) }
                            { sort_th("Avg/Session", AgentTokenSort::AvgPerSession, sort_col, sort_dir, grand_total) }
                        }
                    }
                    tbody {
                        for (idx, agent) in sorted.iter().enumerate() {
                            {
                                let is_active = active_filter.as_deref() == Some(&agent.name);
                                let pct = agent.pct_of_total(grand_total);
                                let color = agent_color(idx);
                                rsx! {
                                    tr {
                                        key: "{agent.id}",
                                        style: if is_active {
                                            "border-bottom: 1px solid #2a2724; background: #1e1c1a;"
                                        } else {
                                            "border-bottom: 1px solid #2a2724;"
                                        },
                                        td {
                                            style: "padding: 6px 8px; color: {color}; white-space: nowrap;",
                                            "{agent.name}"
                                        }
                                        td { style: "padding: 6px 8px; color: #a8a49e; text-align: right;", "{format_tokens(agent.input_tokens)}" }
                                        td { style: "padding: 6px 8px; color: #a8a49e; text-align: right;", "{format_tokens(agent.output_tokens)}" }
                                        td { style: "padding: 6px 8px; color: #e8e6e3; text-align: right; font-weight: 600;", "{format_tokens(agent.total())}" }
                                        td { style: "padding: 6px 8px; color: #706c66; text-align: right;", "{pct:.1}%" }
                                        td { style: "padding: 6px 8px; color: #706c66; text-align: right;", "{format_tokens(agent.avg_per_session())}" }
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

fn sort_th(
    label: &str,
    col: AgentTokenSort,
    mut sort_col: Signal<AgentTokenSort>,
    mut sort_dir: Signal<SortDir>,
    _grand_total: u64,
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
