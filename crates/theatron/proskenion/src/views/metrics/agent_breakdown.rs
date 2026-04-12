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
            #[expect(clippy::as_conversions, reason = "u64 token count to f64 for chart value")]
            value: a.total() as f64,
            color: agent_color(i).to_string(),
            sub_label: Some(format_tokens(a.total())),
        })
        .collect();

    let filter_for_click = active_filter.clone();

    rsx! {
        div {
            style: "display: flex; flex-direction: column; gap: var(--space-4);",

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
                    style: "width: 100%; border-collapse: collapse; font-size: var(--text-xs); font-family: var(--font-mono);",
                    thead {
                        tr {
                            style: "border-bottom: 1px solid var(--border);",
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
                                            "border-bottom: 1px solid var(--border); background: var(--bg-surface-dim);"
                                        } else {
                                            "border-bottom: 1px solid var(--border);"
                                        },
                                        td {
                                            style: "padding: var(--space-2) var(--space-2); color: {color}; white-space: nowrap;",
                                            "{agent.name}"
                                        }
                                        td { style: "padding: var(--space-2) var(--space-2); color: var(--text-secondary); text-align: right;", "{format_tokens(agent.input_tokens)}" }
                                        td { style: "padding: var(--space-2) var(--space-2); color: var(--text-secondary); text-align: right;", "{format_tokens(agent.output_tokens)}" }
                                        td { style: "padding: var(--space-2) var(--space-2); color: var(--text-primary); text-align: right; font-weight: var(--weight-semibold);", "{format_tokens(agent.total())}" }
                                        td { style: "padding: var(--space-2) var(--space-2); color: var(--text-muted); text-align: right;", "{pct:.1}%" }
                                        td { style: "padding: var(--space-2) var(--space-2); color: var(--text-muted); text-align: right;", "{format_tokens(agent.avg_per_session())}" }
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
            style: "padding: var(--space-2) var(--space-2); text-align: right; color: var(--text-muted); cursor: pointer; user-select: none; white-space: nowrap; transition: background-color var(--transition-quick), color var(--transition-quick), border-color var(--transition-quick);",
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
