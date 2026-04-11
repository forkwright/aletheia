//! Success/failure rate charts and summary table.

use dioxus::prelude::*;

use crate::components::chart::{StackedBarChart, StackedBarEntry};
use crate::state::tool_metrics::{TimeSeriesBucket, ToolStat, tools_by_failure};

// -- Main component -----------------------------------------------------------

#[component]
pub(crate) fn ToolResultsView(tools: Vec<ToolStat>, time_series: Vec<TimeSeriesBucket>) -> Element {
    // time_series is unused - kept for API compatibility
    let _ = time_series;
    if tools.is_empty() {
        return rsx! {
            div { style: "color: var(--text-muted); font-size: var(--text-sm); padding: var(--space-2);", "No tool data available." }
        };
    }

    let sorted = tools_by_failure(&tools);

    let stacked_entries: Vec<StackedBarEntry> = sorted
        .iter()
        .map(|t| StackedBarEntry {
            label: t.name.clone(),
            success: t.succeeded,
            failure: t.failed,
        })
        .collect();

    rsx! {
        div { style: "display: flex; flex-direction: column; gap: var(--space-5);",

            // Stacked bar chart
            StackedBarChart {
                entries: stacked_entries,
                sort_by_failure: true,
                on_click: None,
            }
        }
    }
}
