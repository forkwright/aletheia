//! Usage frequency chart: horizontal bars + time series.

use dioxus::prelude::*;

use crate::components::chart::{
    BarEntry, HorizontalBarChart, LineChart, LinePoint, LineSeries, SERIES_COLORS,
};
use crate::state::tool_metrics::{TimeSeriesBucket, ToolStat};

/// Returns the top `limit` tools sorted by total invocations, plus an optional
/// aggregated "Other" entry covering all remaining tools.
fn top_tools(tools: &[ToolStat], limit: usize) -> (Vec<&ToolStat>, Option<ToolStat>) {
    let mut sorted: Vec<&ToolStat> = tools.iter().collect();
    sorted.sort_by(|a, b| b.total.cmp(&a.total));

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

/// Time series variant: builds line series from raw time-series buckets.
///
/// Used by the overview when time series data is available.
#[component]
pub(crate) fn ToolTimeSeriesView(
    tools: Vec<ToolStat>,
    time_series: Vec<TimeSeriesBucket>,
) -> Element {
    if time_series.is_empty() {
        return rsx! {
            div { style: "color: var(--text-muted); font-size: var(--text-sm); padding: var(--space-2);", "No time series data." }
        };
    }

    // Top 10 tools by total invocations for the legend.
    let (top_tools_ref, _) = top_tools(&tools, 10);
    let top_names: Vec<String> = top_tools_ref.iter().map(|t| t.name.clone()).collect();

    let series: Vec<LineSeries> = top_names
        .iter()
        .enumerate()
        .map(|(i, name)| {
            let points = time_series
                .iter()
                .map(|bucket| LinePoint {
                    label: bucket.date.clone(),
                    #[expect(clippy::as_conversions, reason = "u64 call count to f64 for chart point")]
                    value: *bucket.counts.get(name).unwrap_or(&0) as f64,
                })
                .collect();
            LineSeries {
                name: name.clone(),
                color: SERIES_COLORS[i % SERIES_COLORS.len()].to_string(),
                points,
            }
        })
        .collect();

    // "Other" series aggregates remaining tools.
    let other_series = {
        let other_points = time_series
            .iter()
            .map(|bucket| {
                let top_sum: u64 = top_names
                    .iter()
                    .map(|n| bucket.counts.get(n).unwrap_or(&0))
                    .sum();
                let total_bucket: u64 = bucket.counts.values().sum();
                LinePoint {
                    label: bucket.date.clone(),
                    #[expect(clippy::as_conversions, reason = "u64 count to f64 for chart point")]
                    value: total_bucket.saturating_sub(top_sum) as f64,
                }
            })
            .collect();
        LineSeries {
            name: "Other".to_string(),
            color: "var(--text-muted)".to_string(),
            points: other_points,
        }
    };

    let mut all_series = series;
    all_series.push(other_series);

    rsx! {
        LineChart {
            series: all_series,
            height: 180,
        }
    }
}
