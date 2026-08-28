//! Usage frequency chart: horizontal bars.

use dioxus::prelude::*;

use crate::components::chart::{BarEntry, HorizontalBarChart, SERIES_COLORS};
use crate::state::tool_metrics::ToolStat;

/// Returns the top `limit` tools sorted by total invocations, plus an optional
/// aggregated "Other" entry covering all remaining tools.
fn top_tools(tools: &[ToolStat], limit: usize) -> (Vec<&ToolStat>, Option<ToolStat>) {
    let mut sorted: Vec<&ToolStat> = tools.iter().collect();
    sorted.sort_by_key(|b| std::cmp::Reverse(b.total));

    if sorted.len() <= limit {
        return (sorted, None);
    }

    let (top, rest) = sorted.split_at(limit);
    let other = ToolStat {
        name: "Other".to_string(),
        total: rest.iter().map(|t| t.total).sum(),
        succeeded: rest.iter().map(|t| t.succeeded).sum(),
        failed: rest.iter().map(|t| t.failed).sum(),
        ..Default::default()
    };
    (top.to_vec(), Some(other))
}

#[component]
pub(crate) fn ToolFrequencyView(tools: Vec<ToolStat>, on_click: EventHandler<String>) -> Element {
    if tools.is_empty() {
        return rsx! {
            div { style: "color: var(--text-muted); font-size: var(--text-sm); padding: var(--space-2);", "No tool data available." }
        };
    }

    let (top, other) = top_tools(&tools, 10);

    let mut bar_entries: Vec<BarEntry> = top
        .iter()
        .enumerate()
        .map(|(i, t)| BarEntry {
            label: t.name.clone(),
            value: t.total,
            color: Some(SERIES_COLORS[i % SERIES_COLORS.len()].to_string()),
        })
        .collect();

    if let Some(ref o) = other {
        bar_entries.push(BarEntry {
            label: o.name.clone(),
            value: o.total,
            color: Some("var(--text-muted)".to_string()),
        });
    }

    rsx! {
        div { style: "display: flex; flex-direction: column; gap: var(--space-4);",

            // Horizontal bar chart
            HorizontalBarChart {
                entries: bar_entries,
                max_value: None,
                on_click: Some(EventHandler::new(move |name: String| {
                    // NOTE: "Other" is not a real tool; suppress drill-down.
                    if name != "Other" {
                        on_click.call(name);
                    }
                })),
            }

            if let Some(ref o) = other {
                div {
                    style: "font-size: var(--text-xs); color: var(--text-muted); padding: var(--space-1) 0;",
                    "\"Other\" groups {tools.len() - 10} additional tools (total {o.total} calls). Click any named tool to drill down."
                }
            }
        }
    }
}
