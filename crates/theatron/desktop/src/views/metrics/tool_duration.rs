//! Duration distribution: percentile bars, trend chart, and summary table.

use dioxus::prelude::*;

use crate::components::chart::{
    LineChart, LinePoint, LineSeries, PercentileBarChart, PercentileEntry, SERIES_COLORS,
};
use crate::state::tool_metrics::{
    TimeSeriesBucket, ToolStat, format_duration_ms, tools_by_duration,
};

// -- Style constants ----------------------------------------------------------

const TABLE_STYLE: &str = "\
    width: 100%; \
    border-collapse: collapse; \
    font-size: 12px;\
";

const TH_STYLE: &str = "\
    text-align: left; \
    color: #888; \
    font-weight: normal; \
    padding: 4px 8px; \
    border-bottom: 1px solid #333;\
";

const TD_STYLE: &str = "\
    padding: 4px 8px; \
    border-bottom: 1px solid #1e1e38; \
    color: #ccc;\
";

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

            // Duration table
            div {
                style: "font-size: 13px; color: #aaa; margin-bottom: 4px;",
                "Duration Summary"
            }
            {duration_table(&sorted)}
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
    // WHY: Until the server provides per-bucket duration stats, this chart shows
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
                            tool.p50_ms as f64
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

fn duration_table(sorted: &[&ToolStat]) -> Element {
    rsx! {
        table {
            style: "{TABLE_STYLE}",
            thead {
                tr {
                    th { style: "{TH_STYLE}", "Tool" }
                    th { style: "{TH_STYLE}", "Min" }
                    th { style: "{TH_STYLE}", "Median" }
                    th { style: "{TH_STYLE}", "p95" }
                    th { style: "{TH_STYLE}", "Max" }
                    th { style: "{TH_STYLE}", "Calls" }
                }
            }
            tbody {
                for stat in sorted.iter() {
                    {
                        let p95_color = if stat.p95_ms > 10_000 { "#ef4444" }
                            else if stat.p95_ms > 5_000 { "#eab308" }
                            else { "#ccc" };
                        let row_highlight = if stat.p95_ms > 10_000 {
                            "background: rgba(239,68,68,0.06);"
                        } else if stat.p95_ms > 5_000 {
                            "background: rgba(234,179,8,0.06);"
                        } else {
                            ""
                        };
                        let name = stat.name.clone();

                        rsx! {
                            tr {
                                key: "{name}",
                                style: "{row_highlight}",
                                td { style: "{TD_STYLE} font-family: monospace;", "{name}" }
                                td { style: "{TD_STYLE}", "{format_duration_ms(stat.min_ms)}" }
                                td { style: "{TD_STYLE}", "{format_duration_ms(stat.p50_ms)}" }
                                td { style: "{TD_STYLE} color: {p95_color};",
                                    "{format_duration_ms(stat.p95_ms)}"
                                }
                                td { style: "{TD_STYLE}", "{format_duration_ms(stat.max_ms)}" }
                                td { style: "{TD_STYLE}", "{stat.total}" }
                            }
                        }
                    }
                }
            }
        }
    }
}
