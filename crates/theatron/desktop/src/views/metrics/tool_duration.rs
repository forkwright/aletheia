//! Duration distribution: percentile bars, trend chart, and summary table.

use dioxus::prelude::*;

use crate::components::chart::{
    LineChart, LinePoint, LineSeries, PercentileBarChart, PercentileEntry, SERIES_COLORS,
};
use crate::state::tool_metrics::{
    TimeSeriesBucket, ToolStat, tools_by_duration,
};

// -- Component ----------------------------------------------------------------

#[component]
pub(crate) fn ToolDurationView(tools: Vec<ToolStat>) -> Element {
    if tools.is_empty() {
        return rsx! {
            div { style: "color: #555; font-size: 13px; padding: 8px;", "No tool data available." }
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
        div { style: "display: flex; flex-direction: column; gap: 20px;",

            // Percentile bar chart
            PercentileBarChart { entries: perc_entries }

        }
    }
}

/// Standalone duration trend chart for the top 5 slowest tools.
///
/// Shown in the full tools overview to surface performance degradation.
#[component]
pub(crate) fn DurationTrendView(
    tools: Vec<ToolStat>,
    time_series: Vec<TimeSeriesBucket>,
) -> Element {
    if time_series.is_empty() {
        return rsx! {
            div { style: "color: #555; font-size: 12px;", "No time series data." }
        };
    }

    let sorted = tools_by_duration(&tools);
    let top5: Vec<&ToolStat> = sorted.into_iter().take(5).collect();

    if top5.is_empty() {
        return rsx! {
            div { style: "color: #555; font-size: 12px;", "No duration data." }
        };
    }

    // Use median duration (p50) as the series value. We only have per-bucket
    // invocation counts, so we use a constant p50 as a flat proxy.
    // NOTE: Until the server provides per-bucket duration stats, this chart shows
    // relative volume-weighted duration as an approximation. Swap when available.
    let series: Vec<LineSeries> = top5
        .iter()
        .enumerate()
        .map(|(i, tool)| {
            let points = time_series
                .iter()
                .map(|bucket| {
                    // Proxy: scale p50 by bucket share of total calls.
                    let bucket_count = *bucket.counts.get(&tool.name).unwrap_or(&0);
                    LinePoint {
                        label: bucket.date.clone(),
                        value: if bucket_count > 0 {
                            {
                            #[expect(clippy::as_conversions, reason = "u64 ms to f64 for chart point")]
                            let v = tool.p50_ms as f64;
                            v
                        }
                        } else {
                            0.0
                        },
                    }
                })
                .collect();
            LineSeries {
                name: tool.name.clone(),
                color: SERIES_COLORS[i % SERIES_COLORS.len()].to_string(),
                points,
            }
        })
        .collect();

    rsx! {
        LineChart { series, height: 150 }
    }
}


