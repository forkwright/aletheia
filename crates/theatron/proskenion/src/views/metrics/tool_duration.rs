//! Duration distribution: percentile bars and summary table.

use dioxus::prelude::*;

use crate::components::chart::{PercentileBarChart, PercentileEntry};
use crate::state::tool_metrics::{ToolStat, tools_by_duration};

// -- Component ----------------------------------------------------------------

#[component]
pub(crate) fn ToolDurationView(tools: Vec<ToolStat>) -> Element {
    if tools.is_empty() {
        return rsx! {
            div { style: "color: var(--text-muted); font-size: var(--text-sm); padding: var(--space-2);", "No tool data available." }
        };
    }

    let sorted = tools_by_duration(&tools);

    let perc_entries: Vec<PercentileEntry> = sorted
        .iter()
        .map(|t| PercentileEntry {
            label: t.name.clone(),
            min_ms: t.min_ms,
            p25_ms: t.p25_ms,
            p50_ms: t.p50_ms,
            p75_ms: t.p75_ms,
            p95_ms: t.p95_ms,
            max_ms: t.max_ms,
        })
        .collect();

    rsx! {
        div { style: "display: flex; flex-direction: column; gap: var(--space-5);",

            PercentileBarChart { entries: perc_entries }

        }
    }
}
