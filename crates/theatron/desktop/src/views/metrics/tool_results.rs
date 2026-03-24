//! Success/failure rate charts and summary table.

use dioxus::prelude::*;

use crate::components::chart::{
    LineChart, LinePoint, LineSeries, SERIES_COLORS, StackedBarChart, StackedBarEntry,
};
use crate::state::tool_metrics::{TimeSeriesBucket, ToolStat, failure_rate, tools_by_failure};

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

// -- Main component -----------------------------------------------------------

#[component]
pub(crate) fn ToolResultsView(tools: Vec<ToolStat>, time_series: Vec<TimeSeriesBucket>) -> Element {
    if tools.is_empty() {
        return rsx! {
            div { style: "color: #555; font-size: 13px; padding: 8px;", "No tool data available." }
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
        div { style: "display: flex; flex-direction: column; gap: 20px;",

            // Stacked bar chart
            StackedBarChart {
                entries: stacked_entries,
                sort_by_failure: true,
                on_click: None,
            }

            // Failure rate trend (top 5 most-failing tools)
            if !time_series.is_empty() {
                div {
                    div {
                        style: "font-size: 13px; color: #aaa; margin-bottom: 8px;",
                        "Failure Rate Trend (top 5 most-failing tools)"
                    }
                    {failure_trend_chart(&tools, &time_series)}
                }
            }

            // Summary table
            div {
                style: "font-size: 13px; color: #aaa; margin-bottom: 4px;",
                "Failure Summary"
            }
            {failure_table(&sorted)}
        }
    }
}

fn failure_trend_chart(tools: &[ToolStat], time_series: &[TimeSeriesBucket]) -> Element {
    let mut by_fail = tools_by_failure(tools);
    by_fail.retain(|t| t.failed > 0);
    let top5: Vec<&ToolStat> = by_fail.into_iter().take(5).collect();

    if top5.is_empty() {
        return rsx! {
            div { style: "color: #555; font-size: 12px;", "No failures recorded." }
        };
    }

    // Compute failure rate per bucket per tool.
    // WHY: We only have aggregate counts per tool per bucket; failure rate is
    // approximated as failed/(total) per bucket for each named tool.
    // Time series only tracks call counts, not outcomes per bucket; so we show
    // relative call volume as a proxy. If the server provides per-tool
    // success/fail per bucket in the future, swap `counts` for separate fields.
    let series: Vec<LineSeries> = top5
        .iter()
        .enumerate()
        .map(|(i, tool)| {
            let tool_total = tool.total.max(1);
            let points = time_series
                .iter()
                .map(|bucket| {
                    let bucket_count = *bucket.counts.get(&tool.name).unwrap_or(&0);
                    // Approximate failure rate using overall failure ratio.
                    #[expect(clippy::cast_precision_loss, reason = "display-only approximation")]
                    let approx_fail_rate =
                        (tool.failed as f64 / tool_total as f64) * bucket_count as f64;
                    LinePoint {
                        label: bucket.date.clone(),
                        value: approx_fail_rate,
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

fn failure_table(sorted: &[&ToolStat]) -> Element {
    rsx! {
        table {
            style: "{TABLE_STYLE}",
            thead {
                tr {
                    th { style: "{TH_STYLE}", "Tool" }
                    th { style: "{TH_STYLE}", "Calls" }
                    th { style: "{TH_STYLE}", "Failures" }
                    th { style: "{TH_STYLE}", "Fail %" }
                    th { style: "{TH_STYLE}", "Last Error" }
                    th { style: "{TH_STYLE}", "Last Failure" }
                }
            }
            tbody {
                for stat in sorted.iter() {
                    {
                        let rate = failure_rate(stat);
                        let row_bg = if rate > 50.0 {
                            "background: rgba(239,68,68,0.08);"
                        } else if rate > 20.0 {
                            "background: rgba(234,179,8,0.08);"
                        } else {
                            ""
                        };
                        let rate_color = if rate > 50.0 { "#ef4444" }
                            else if rate > 20.0 { "#eab308" }
                            else { "#888" };
                        let name = stat.name.clone();
                        let last_error = stat.most_common_error.as_deref().unwrap_or("—");
                        let last_fail = stat.last_failure_at.as_deref().unwrap_or("—");

                        rsx! {
                            tr {
                                key: "{name}",
                                style: "{row_bg}",
                                td { style: "{TD_STYLE} font-family: monospace;", "{name}" }
                                td { style: "{TD_STYLE}", "{stat.total}" }
                                td { style: "{TD_STYLE}", "{stat.failed}" }
                                td { style: "{TD_STYLE} color: {rate_color};", "{rate:.1}%" }
                                td {
                                    style: "{TD_STYLE} font-size: 11px; max-width: 180px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap;",
                                    "{last_error}"
                                }
                                td { style: "{TD_STYLE} font-size: 11px; color: #666;", "{last_fail}" }
                            }
                        }
                    }
                }
            }
        }
    }
}
