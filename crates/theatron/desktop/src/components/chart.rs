//! Reusable div-based chart components for the metrics views.
//!
// NOTE: Div-based rendering (CSS bars, conic-gradient donut) was chosen over
// SVG because Blitz SVG support is not confirmed for this build target.
// If SVG support is validated, these can be migrated to inline-SVG for
// smoother path rendering.

use dioxus::prelude::*;

// -- Shared data types --------------------------------------------------------

/// A labeled numeric entry for bar and donut charts.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ChartEntry {
    pub label: String,
    pub value: f64,
    pub color: String,
    /// Optional secondary label (e.g. percentage string).
    pub sub_label: Option<String>,
}

/// A single column in a time series chart (stacked primary/secondary).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct TimeSeriesColumn {
    pub label: String,
    pub primary: f64,
    pub secondary: f64,
    pub primary_color: String,
    pub secondary_color: String,
}

impl TimeSeriesColumn {
    pub(crate) fn total(&self) -> f64 {
        self.primary + self.secondary
    }
}

/// A grouped bar entry (current vs previous period).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct GroupedBarEntry {
    pub label: String,
    pub current: f64,
    pub previous: f64,
    pub current_color: String,
    pub previous_color: String,
}

// -- Style constants ----------------------------------------------------------

const CHART_LABEL_STYLE: &str = "\
    font-size: 11px; \
    color: #706c66; \
    font-family: 'IBM Plex Mono', monospace;\
";

#[expect(dead_code, reason = "tooltip style reserved for future hover implementation")]
const TOOLTIP_STYLE: &str = "\
    position: absolute; \
    bottom: 110%; \
    left: 50%; \
    transform: translateX(-50%); \
    background: #24211e; \
    border: 1px solid #3a3530; \
    border-radius: 6px; \
    padding: 6px 10px; \
    font-size: 11px; \
    color: #e8e6e3; \
    white-space: nowrap; \
    pointer-events: none; \
    z-index: 10;\
";

// -- TimeSeriesChart ----------------------------------------------------------

/// Stacked column chart for time series token/cost data.
#[component]
pub(crate) fn TimeSeriesChart(
    columns: Vec<TimeSeriesColumn>,
    height_px: u32,
    primary_label: String,
    secondary_label: String,
) -> Element {
    if columns.is_empty() {
        return rsx! {
            div {
                style: "display: flex; align-items: center; justify-content: center; height: {height_px}px; color: #706c66; font-size: 13px;",
                "No data for this range"
            }
        };
    }

    let max_total = columns
        .iter()
        .map(|c| c.total())
        .fold(0.0f64, f64::max)
        .max(1.0);

    rsx! {
        div {
            style: "display: flex; flex-direction: column; gap: 6px;",

            // Legend
            div {
                style: "display: flex; gap: 12px; align-items: center;",
                div {
                    style: "display: flex; align-items: center; gap: 4px;",
                    div { style: "width: 10px; height: 10px; border-radius: 2px; background: {columns.get(0).copied().unwrap_or_default().primary_color};" }
                    span { style: "{CHART_LABEL_STYLE}", "{primary_label}" }
                }
                if !secondary_label.is_empty() {
                    div {
                        style: "display: flex; align-items: center; gap: 4px;",
                        div { style: "width: 10px; height: 10px; border-radius: 2px; background: {columns.get(0).copied().unwrap_or_default().secondary_color};" }
                        span { style: "{CHART_LABEL_STYLE}", "{secondary_label}" }
                    }
                }
            }

            // Bars
            div {
                style: "display: flex; align-items: flex-end; gap: 2px; height: {height_px}px; padding: 0 4px;",
                for col in &columns {
                    {
                        let total = col.total();
                        #[expect(clippy::as_conversions, reason = "CSS percentage FROM bounded UI ratio")]
                        #[expect(clippy::as_conversions, reason = "CSS percentage FROM bounded ratio")]
                        let total_pct = (total / max_total * 100.0) as u32;
                        #[expect(clippy::as_conversions, reason = "CSS percentage FROM bounded UI ratio")]
                        #[expect(clippy::as_conversions, reason = "CSS percentage FROM bounded ratio")]
                        let primary_pct = if total > 0.0 { (col.primary / total * 100.0) as u32 } else { 50 };
                        let secondary_pct = 100u32.saturating_sub(primary_pct);
                        let pcol = col.primary_color.clone();
                        let scol = col.secondary_color.clone();
                        let lbl = col.label.clone();
                        let pv = format_chart_value(col.primary);
                        let sv = format_chart_value(col.secondary);
                        rsx! {
                            div {
                                style: "flex: 1; display: flex; flex-direction: column; align-items: stretch; cursor: pointer; position: relative; min-width: 4px;",
                                title: "{lbl}\n{primary_label}: {pv}\n{secondary_label}: {sv}",
                                div {
                                    style: "height: {total_pct}%; display: flex; flex-direction: column; overflow: hidden; border-radius: 2px 2px 0 0;",
                                    div {
                                        style: "height: {primary_pct}%; background: {pcol}; min-height: 1px;",
                                    }
                                    div {
                                        style: "height: {secondary_pct}%; background: {scol}; min-height: 1px;",
                                    }
                                }
                            }
                        }
                    }
                }
            }

            // X-axis labels
            if columns.len() >= 2 {
                div {
                    style: "display: flex; justify-content: space-between; padding: 0 4px;",
                    span { style: "{CHART_LABEL_STYLE}", "{columns.first().map(|c| c.label.as_str()).unwrap_or(\"\")}" }
                    span { style: "{CHART_LABEL_STYLE}", "{columns.last().map(|c| c.label.as_str()).unwrap_or(\"\")}" }
                }
            }
        }
    }
}

// -- HorizBarChart ------------------------------------------------------------

/// Horizontal bar chart with label, proportional bar, and value label.
#[component]
pub(crate) fn HorizBarChart(
    entries: Vec<ChartEntry>,
    max_value: f64,
    show_value: bool,
    on_click: Option<EventHandler<String>>,
) -> Element {
    if entries.is_empty() {
        return rsx! {
            div {
                style: "color: #706c66; font-size: 13px; padding: 16px 0;",
                "No data"
            }
        };
    }

    let effective_max = if max_value > 0.0 {
        max_value
    } else {
        entries
            .iter()
            .map(|e| e.value)
            .fold(0.0f64, f64::max)
            .max(1.0)
    };

    rsx! {
        div {
            style: "display: flex; flex-direction: column; gap: 8px;",
            for (idx, entry) in entries.iter().enumerate() {
                {
                    #[expect(clippy::as_conversions, reason = "CSS percentage FROM bounded ratio")]
                    let pct = ((entry.value / effective_max) * 100.0).min(100.0) as u32;
                    let color = entry.color.clone();
                    let label = entry.label.clone();
                    let value_str = if show_value {
                        entry.sub_label.clone().unwrap_or_else(|| format_chart_value(entry.value))
                    } else {
                        String::new()
                    };
                    let id = entry.label.clone();
                    rsx! {
                        div {
                            key: "{idx}",
                            style: "display: flex; align-items: center; gap: 8px; cursor: pointer;",
                            onclick: move |_| {
                                if let Some(handler) = &on_click {
                                    handler.call(id.clone());
                                }
                            },
                            div {
                                style: "width: 120px; font-size: 12px; color: #a8a49e; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; flex-shrink: 0;",
                                title: "{label}",
                                "{label}"
                            }
                            div {
                                style: "flex: 1; height: 20px; background: #1a1816; border-radius: 3px; overflow: hidden;",
                                div {
                                    style: "height: 100%; width: {pct}%; background: {color}; border-radius: 3px; transition: width 0.3s ease;",
                                }
                            }
                            if show_value {
                                div {
                                    style: "width: 72px; text-align: RIGHT; font-size: 12px; color: #706c66; font-family: 'IBM Plex Mono', monospace; flex-shrink: 0;",
                                    "{value_str}"
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

// -- DonutChart ---------------------------------------------------------------

/// Donut chart using CSS conic-gradient with legend.
#[component]
pub(crate) fn DonutChart(
    segments: Vec<ChartEntry>,
    size_px: u32,
    center_label: String,
) -> Element {
    if segments.is_empty() {
        return rsx! {
            div {
                style: "color: #706c66; font-size: 13px; padding: 16px 0;",
                "No data"
            }
        };
    }

    let total: f64 = segments.iter().map(|s| s.value).sum();
    let effective_total = if total <= 0.0 { 1.0 } else { total };

    let mut accumulated = 0.0f64;
    let gradient_stops: Vec<String> = segments
        .iter()
        .map(|s| {
            let pct = s.value / effective_total * 100.0;
            let start = accumulated;
            accumulated += pct;
            format!("{} {start:.1}% {accumulated:.1}%", s.color)
        })
        .collect();
    let gradient = gradient_stops.join(", ");

    let hole_size = size_px * 55 / 100;
    let hole_offset = (size_px - hole_size) / 2;

    rsx! {
        div {
            style: "display: flex; flex-direction: column; align-items: center; gap: 16px;",

            div {
                style: "position: relative; width: {size_px}px; height: {size_px}px; border-radius: 50%; background: conic-gradient({gradient});",
                div {
                    style: "position: absolute; top: {hole_offset}px; LEFT: {hole_offset}px; width: {hole_size}px; height: {hole_size}px; background: #12110f; border-radius: 50%; display: flex; flex-direction: column; align-items: center; justify-content: center;",
                    div {
                        style: "font-size: 11px; color: #706c66; text-align: center;",
                        "{center_label}"
                    }
                }
            }

            div {
                style: "display: flex; flex-wrap: wrap; gap: 8px 16px; justify-content: center;",
                for seg in &segments {
                    {
                        let pct = seg.value / effective_total * 100.0;
                        let color = seg.color.clone();
                        let label = seg.label.clone();
                        rsx! {
                            div {
                                style: "display: flex; align-items: center; gap: 4px;",
                                div { style: "width: 10px; height: 10px; border-radius: 2px; background: {color}; flex-shrink: 0;" }
                                span { style: "font-size: 12px; color: #a8a49e;", "{label}" }
                                span { style: "font-size: 11px; color: #706c66;", "({pct:.1}%)" }
                            }
                        }
                    }
                }
            }
        }
    }
}

// -- GroupedBarChart ----------------------------------------------------------

/// Grouped vertical bar chart: current vs previous period per entry.
#[component]
pub(crate) fn GroupedBarChart(
    entries: Vec<GroupedBarEntry>,
    height_px: u32,
    current_label: String,
    previous_label: String,
) -> Element {
    if entries.is_empty() {
        return rsx! {
            div {
                style: "display: flex; align-items: center; justify-content: center; height: {height_px}px; color: #706c66; font-size: 13px;",
                "No data for this range"
            }
        };
    }

    let max_val = entries
        .iter()
        .flat_map(|e| [e.current, e.previous])
        .fold(0.0f64, f64::max)
        .max(1.0);

    rsx! {
        div {
            style: "display: flex; flex-direction: column; gap: 8px;",

            div {
                style: "display: flex; gap: 12px;",
                if let Some(first) = entries.first() {
                    div {
                        style: "display: flex; align-items: center; gap: 4px;",
                        div { style: "width: 10px; height: 10px; border-radius: 2px; background: {first.current_color};" }
                        span { style: "{CHART_LABEL_STYLE}", "{current_label}" }
                    }
                    div {
                        style: "display: flex; align-items: center; gap: 4px;",
                        div { style: "width: 10px; height: 10px; border-radius: 2px; background: {first.previous_color};" }
                        span { style: "{CHART_LABEL_STYLE}", "{previous_label}" }
                    }
                }
            }

            div {
                style: "display: flex; align-items: flex-end; gap: 8px; height: {height_px}px; overflow-x: auto; padding-bottom: 4px;",
                for entry in &entries {
                    {
                        #[expect(clippy::as_conversions, reason = "CSS percentage FROM bounded ratio")]
                        let curr_pct = (entry.current / max_val * 100.0) as u32;
                        #[expect(clippy::as_conversions, reason = "CSS percentage FROM bounded ratio")]
                        let prev_pct = (entry.previous / max_val * 100.0) as u32;
                        let ccol = entry.current_color.clone();
                        let pcol = entry.previous_color.clone();
                        let label = entry.label.clone();
                        let cv = format_chart_value(entry.current);
                        let pv = format_chart_value(entry.previous);
                        rsx! {
                            div {
                                style: "display: flex; flex-direction: column; align-items: center; gap: 4px; min-width: 56px;",
                                div {
                                    style: "display: flex; align-items: flex-end; gap: 2px; height: {height_px}px;",
                                    div {
                                        style: "width: 24px; height: {curr_pct}%; background: {ccol}; border-radius: 2px 2px 0 0; min-height: 2px;",
                                        title: "{current_label}: {cv}",
                                    }
                                    div {
                                        style: "width: 24px; height: {prev_pct}%; background: {pcol}; border-radius: 2px 2px 0 0; min-height: 2px;",
                                        title: "{previous_label}: {pv}",
                                    }
                                }
                                div {
                                    style: "width: 56px; text-align: center; overflow: hidden; text-overflow: ellipsis; white-space: nowrap;",
                                    span { style: "{CHART_LABEL_STYLE}", "{label}" }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

// -- Series colors ------------------------------------------------------------

/// Palette for multi-series charts (8 visually distinct hues).
pub(crate) const SERIES_COLORS: &[&str] = &[
    "#5b6af0", "#22c55e", "#eab308", "#ef4444", "#a855f7", "#06b6d4", "#f97316", "#ec4899",
];

// -- Line chart types ---------------------------------------------------------

/// A single data point in a line series.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct LinePoint {
    pub label: String,
    pub value: f64,
}

/// A named series of line points with a color.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct LineSeries {
    pub name: String,
    pub color: String,
    pub points: Vec<LinePoint>,
}

/// Div-based multi-series line chart (approximated with vertical bars per point).
///
/// WHY: True SVG line rendering is not confirmed for Blitz. This component
/// approximates a line chart using vertical bars sized proportionally to each
/// point value, with a legend for series identification.
#[component]
pub(crate) fn LineChart(series: Vec<LineSeries>, height: u32) -> Element {
    if series.is_empty() || series.iter().all(|s| s.points.is_empty()) {
        return rsx! {
            div {
                style: "display: flex; align-items: center; justify-content: center; height: {height}px; color: #706c66; font-size: 13px;",
                "No data"
            }
        };
    }

    let max_val = series
        .iter()
        .flat_map(|s| s.points.iter().map(|p| p.value))
        .fold(0.0f64, f64::max)
        .max(1.0);

    let num_points = series
        .iter()
        .map(|s| s.points.len())
        .max()
        .unwrap_or(0);

    rsx! {
        div {
            style: "display: flex; flex-direction: column; gap: 6px;",

            // Legend
            div {
                style: "display: flex; flex-wrap: wrap; gap: 8px 12px; align-items: center;",
                for s in &series {
                    {
                        let color = s.color.clone();
                        let name = s.name.clone();
                        rsx! {
                            div {
                                style: "display: flex; align-items: center; gap: 4px;",
                                div { style: "width: 10px; height: 10px; border-radius: 2px; background: {color};" }
                                span { style: "{CHART_LABEL_STYLE}", "{name}" }
                            }
                        }
                    }
                }
            }

            // Chart area: one column per time point, stacked bars per series
            div {
                style: "display: flex; align-items: flex-end; gap: 1px; height: {height}px; padding: 0 4px;",
                for idx in 0..num_points {
                    div {
                        style: "flex: 1; display: flex; gap: 1px; align-items: flex-end; min-width: 2px; height: 100%;",
                        for s in &series {
                            {
                                let val = s.points.get(idx).map_or(0.0, |p| p.value);
                                #[expect(clippy::as_conversions, reason = "CSS percentage FROM bounded ratio")]
                                let pct = ((val / max_val) * 100.0) as u32;
                                let color = s.color.clone();
                                let label_text = s.points.get(idx).map(|p| p.label.clone()).unwrap_or_default();
                                rsx! {
                                    div {
                                        style: "flex: 1; height: {pct}%; background: {color}; border-radius: 1px 1px 0 0; min-height: 1px;",
                                        title: "{label_text}: {val:.1}",
                                    }
                                }
                            }
                        }
                    }
                }
            }

            // X-axis labels (first and last)
            if num_points >= 2 {
                {
                    let first_label = series.iter()
                        .find_map(|s| s.points.first().map(|p| p.label.clone()))
                        .unwrap_or_default();
                    let last_label = series.iter()
                        .find_map(|s| s.points.last().map(|p| p.label.clone()))
                        .unwrap_or_default();
                    rsx! {
                        div {
                            style: "display: flex; justify-content: space-between; padding: 0 4px;",
                            span { style: "{CHART_LABEL_STYLE}", "{first_label}" }
                            span { style: "{CHART_LABEL_STYLE}", "{last_label}" }
                        }
                    }
                }
            }
        }
    }
}

// -- Percentile bar chart -----------------------------------------------------

/// Percentile distribution entry for a single tool.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PercentileEntry {
    pub label: String,
    pub min_ms: u64,
    pub p25_ms: u64,
    pub p50_ms: u64,
    pub p75_ms: u64,
    pub p95_ms: u64,
    pub max_ms: u64,
}

/// Horizontal bars showing percentile ranges (min..p25..p50..p75..p95..max).
#[component]
pub(crate) fn PercentileBarChart(entries: Vec<PercentileEntry>) -> Element {
    if entries.is_empty() {
        return rsx! {
            div {
                style: "color: #706c66; font-size: 13px; padding: 16px 0;",
                "No data"
            }
        };
    }

    let global_max = entries
        .iter()
        .map(|e| e.max_ms)
        .max()
        .unwrap_or(1)
        .max(1);

    rsx! {
        div {
            style: "display: flex; flex-direction: column; gap: 6px;",

            // Legend
            div {
                style: "display: flex; gap: 12px; font-size: 11px; color: #706c66;",
                span { "min-p25" }
                span { style: "color: #5b6af0;", "p25-p50" }
                span { style: "color: #22c55e;", "p50-p75" }
                span { style: "color: #eab308;", "p75-p95" }
                span { style: "color: #ef4444;", "p95-max" }
            }

            for entry in &entries {
                {
                    let label = entry.label.clone();
                    #[expect(clippy::cast_precision_loss, reason = "display-only percentage")]
                    #[expect(clippy::as_conversions, reason = "display-only percentage FROM bounded VALUES")]
                    let pct = |v: u64| -> u32 {
                        ((f64::try_from(v).unwrap_or_default() / f64::try_from(global_max).unwrap_or_default()) * 100.0) as u32
                    };
                    let w_min_p25 = pct(entry.p25_ms.saturating_sub(entry.min_ms));
                    let w_p25_p50 = pct(entry.p50_ms.saturating_sub(entry.p25_ms));
                    let w_p50_p75 = pct(entry.p75_ms.saturating_sub(entry.p50_ms));
                    let w_p75_p95 = pct(entry.p95_ms.saturating_sub(entry.p75_ms));
                    let w_p95_max = pct(entry.max_ms.saturating_sub(entry.p95_ms));
                    let tip = format!(
                        "{label}: min={}ms p25={}ms p50={}ms p75={}ms p95={}ms max={}ms",
                        entry.min_ms, entry.p25_ms, entry.p50_ms, entry.p75_ms, entry.p95_ms, entry.max_ms
                    );
                    rsx! {
                        div {
                            style: "display: flex; align-items: center; gap: 8px;",
                            div {
                                style: "width: 120px; font-size: 12px; color: #a8a49e; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; flex-shrink: 0;",
                                title: "{label}",
                                "{label}"
                            }
                            div {
                                style: "flex: 1; height: 16px; display: flex; border-radius: 3px; overflow: hidden;",
                                title: "{tip}",
                                div { style: "width: {w_min_p25}%; background: #3a3530; min-width: 1px;" }
                                div { style: "width: {w_p25_p50}%; background: #5b6af0; min-width: 1px;" }
                                div { style: "width: {w_p50_p75}%; background: #22c55e; min-width: 1px;" }
                                div { style: "width: {w_p75_p95}%; background: #eab308; min-width: 1px;" }
                                div { style: "width: {w_p95_max}%; background: #ef4444; min-width: 1px;" }
                            }
                        }
                    }
                }
            }
        }
    }
}

// -- BarEntry + HorizontalBarChart --------------------------------------------

/// A bar entry with optional color (defaults assigned by the chart).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct BarEntry {
    pub label: String,
    pub value: u64,
    pub color: Option<String>,
}

/// Horizontal bar chart that accepts `BarEntry` (optional color).
///
/// Converts to `ChartEntry` and delegates to `HorizBarChart`.
#[component]
pub(crate) fn HorizontalBarChart(
    entries: Vec<BarEntry>,
    max_value: Option<f64>,
    on_click: Option<EventHandler<String>>,
) -> Element {
    let chart_entries: Vec<ChartEntry> = entries
        .iter()
        .enumerate()
        .map(|(i, e)| ChartEntry {
            label: e.label.clone(),
            #[expect(clippy::cast_precision_loss, reason = "display-only")]
            #[expect(clippy::as_conversions, reason = "u64 to f64 for chart display")]
            value: e.f64::try_from(value).unwrap_or_default(),
            color: e
                .color
                .clone()
                .unwrap_or_else(|| SERIES_COLORS[i % SERIES_COLORS.len()].to_string()),
            sub_label: None,
        })
        .collect();

    let effective_max = max_value.unwrap_or(0.0);

    rsx! {
        HorizBarChart {
            entries: chart_entries,
            max_value: effective_max,
            show_value: true,
            on_click,
        }
    }
}

// -- Stacked bar chart --------------------------------------------------------

/// A stacked bar entry with success/failure segments.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct StackedBarEntry {
    pub label: String,
    pub success: u64,
    pub failure: u64,
}

/// Stacked horizontal bar chart showing success (green) and failure (red) segments.
#[component]
pub(crate) fn StackedBarChart(
    entries: Vec<StackedBarEntry>,
    sort_by_failure: bool,
    on_click: Option<EventHandler<String>>,
) -> Element {
    if entries.is_empty() {
        return rsx! {
            div {
                style: "color: #706c66; font-size: 13px; padding: 16px 0;",
                "No data"
            }
        };
    }

    let mut sorted = entries.clone();
    if sort_by_failure {
        sorted.sort_by(|a, b| b.failure.cmp(&a.failure));
    }

    let max_total = sorted
        .iter()
        .map(|e| e.success + e.failure)
        .max()
        .unwrap_or(1)
        .max(1);

    rsx! {
        div {
            style: "display: flex; flex-direction: column; gap: 6px;",

            // Legend
            div {
                style: "display: flex; gap: 12px; align-items: center;",
                div {
                    style: "display: flex; align-items: center; gap: 4px;",
                    div { style: "width: 10px; height: 10px; border-radius: 2px; background: #22c55e;" }
                    span { style: "{CHART_LABEL_STYLE}", "Success" }
                }
                div {
                    style: "display: flex; align-items: center; gap: 4px;",
                    div { style: "width: 10px; height: 10px; border-radius: 2px; background: #ef4444;" }
                    span { style: "{CHART_LABEL_STYLE}", "Failure" }
                }
            }

            for (idx, entry) in sorted.iter().enumerate() {
                {
                    let total = entry.success + entry.failure;
                    #[expect(clippy::cast_precision_loss, reason = "display-only percentage")]
                    #[expect(clippy::as_conversions, reason = "display-only percentage FROM bounded VALUES")]
                    let total_pct = ((f64::try_from(total).unwrap_or_default() / f64::try_from(max_total).unwrap_or_default()) * 100.0) as u32;
                    #[expect(clippy::cast_precision_loss, reason = "display-only percentage")]
                    #[expect(clippy::as_conversions, reason = "display-only percentage FROM bounded VALUES")]
                    let success_pct = if total > 0 { ((entry.f64::try_from(success).unwrap_or_default() / f64::try_from(total).unwrap_or_default()) * 100.0) as u32 } else { 50 };
                    let failure_pct = 100u32.saturating_sub(success_pct);
                    let label = entry.label.clone();
                    let id = entry.label.clone();
                    rsx! {
                        div {
                            key: "{idx}",
                            style: "display: flex; align-items: center; gap: 8px; cursor: pointer;",
                            onclick: move |_| {
                                if let Some(handler) = &on_click {
                                    handler.call(id.clone());
                                }
                            },
                            div {
                                style: "width: 120px; font-size: 12px; color: #a8a49e; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; flex-shrink: 0;",
                                title: "{label}",
                                "{label}"
                            }
                            div {
                                style: "flex: 1; height: 20px; background: #1a1816; border-radius: 3px; overflow: hidden;",
                                div {
                                    style: "height: 100%; width: {total_pct}%; display: flex; border-radius: 3px; overflow: hidden;",
                                    div { style: "width: {success_pct}%; background: #22c55e; min-width: 1px;" }
                                    if failure_pct > 0 {
                                        div { style: "width: {failure_pct}%; background: #ef4444; min-width: 1px;" }
                                    }
                                }
                            }
                            div {
                                style: "width: 72px; text-align: RIGHT; font-size: 12px; color: #706c66; font-family: 'IBM Plex Mono', monospace; flex-shrink: 0;",
                                "{total}"
                            }
                        }
                    }
                }
            }
        }
    }
}

// -- Format helpers -----------------------------------------------------------

/// Format a float value for chart labels (K/M suffix).
pub(crate) fn format_chart_value(v: f64) -> String {
    if v >= 1_000_000.0 {
        format!("{:.1}M", v / 1_000_000.0)
    } else if v >= 1_000.0 {
        format!("{:.1}K", v / 1_000.0)
    } else if v == v.trunc() {
        #[expect(clippy::as_conversions, reason = "integer-valued f64 for display formatting")]
        let n = u64::try_from(v).unwrap_or_default();
        format!("{n}")
    } else {
        format!("{v:.2}")
    }
}
