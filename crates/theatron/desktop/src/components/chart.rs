//! SVG chart primitives for metrics views.
//!
//! All charts render as inline SVG with inline styles. No external charting
//! library is required. The API is designed to be compatible with chart usage
//! in future metrics views (prompt 114 token/cost charts).

use dioxus::prelude::*;

// -- Shared palette -----------------------------------------------------------

pub(crate) const COLOR_SUCCESS: &str = "#22c55e";
pub(crate) const COLOR_FAILURE: &str = "#ef4444";
pub(crate) const COLOR_PRIMARY: &str = "#4a4aff";
pub(crate) const COLOR_MUTED: &str = "#555";
pub(crate) const COLOR_TEXT: &str = "#e0e0e0";
pub(crate) const COLOR_TEXT_DIM: &str = "#888";
pub(crate) const COLOR_WARN: &str = "#eab308";

/// Ten-color sequential palette for multi-series charts.
pub(crate) const SERIES_COLORS: &[&str] = &[
    "#4a4aff", "#22c55e", "#eab308", "#ef4444", "#06b6d4", "#a855f7", "#f97316", "#ec4899",
    "#84cc16", "#14b8a6",
];

// -- Bar entry ----------------------------------------------------------------

/// A single labeled value for bar charts.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct BarEntry {
    pub label: String,
    pub value: u64,
    /// Optional hex color override. Falls back to `COLOR_PRIMARY`.
    pub color: Option<String>,
}

// -- Horizontal bar chart -----------------------------------------------------

/// Horizontal bar chart: tool name on the left, bar extending right, value label on right.
///
/// Height auto-sizes to `entries.len() * ROW_HEIGHT + PADDING`.
#[component]
pub(crate) fn HorizontalBarChart(
    entries: Vec<BarEntry>,
    /// If `None`, the maximum entry value is used.
    max_value: Option<u64>,
    /// Called with the entry label when a bar is clicked.
    on_click: Option<EventHandler<String>>,
) -> Element {
    const LABEL_W: u64 = 130;
    const VALUE_W: u64 = 50;
    const ROW_H: u64 = 26;
    const PAD_TOP: u64 = 8;
    const TOTAL_W: u64 = 600;
    const BAR_AREA: u64 = TOTAL_W - LABEL_W - VALUE_W;

    let n = entries.len() as u64;
    let chart_h = n * ROW_H + PAD_TOP * 2;

    let effective_max =
        max_value.unwrap_or_else(|| entries.iter().map(|e| e.value).max().unwrap_or(1).max(1));

    rsx! {
        svg {
            width: "100%",
            view_box: "0 0 {TOTAL_W} {chart_h}",
            style: "display: block; overflow: visible;",

            for (i, entry) in entries.iter().enumerate() {
                {
                    let y = PAD_TOP + i as u64 * ROW_H;
                    let bar_w = (entry.value * BAR_AREA) / effective_max;
                    let color = entry.color.as_deref().unwrap_or(COLOR_PRIMARY);
                    let label = entry.label.clone();
                    let clickable = on_click.is_some();
                    let cursor = if clickable { "pointer" } else { "default" };

                    rsx! {
                        g {
                            key: "{label}",
                            style: "cursor: {cursor};",
                            onclick: {
                                let label = label.clone();
                                let on_click = on_click.clone();
                                move |_| {
                                    if let Some(ref handler) = on_click {
                                        handler.call(label.clone());
                                    }
                                }
                            },

                            // Label
                            text {
                                x: "0",
                                y: "{y + ROW_H - 8}",
                                fill: COLOR_TEXT,
                                font_size: "12",
                                font_family: "monospace",
                                text_anchor: "start",
                                "{truncate_label(&label, 18)}"
                            }

                            // Bar background
                            rect {
                                x: "{LABEL_W}",
                                y: "{y + 4}",
                                width: "{BAR_AREA}",
                                height: "{ROW_H - 10}",
                                fill: "#1a1a2e",
                                rx: "3",
                            }

                            // Bar fill
                            if bar_w > 0 {
                                rect {
                                    x: "{LABEL_W}",
                                    y: "{y + 4}",
                                    width: "{bar_w}",
                                    height: "{ROW_H - 10}",
                                    fill: color,
                                    rx: "3",
                                }
                            }

                            // Value label
                            text {
                                x: "{LABEL_W + BAR_AREA + 6}",
                                y: "{y + ROW_H - 8}",
                                fill: COLOR_TEXT_DIM,
                                font_size: "11",
                                font_family: "monospace",
                                "{entry.value}"
                            }
                        }
                    }
                }
            }
        }
    }
}

// -- Stacked horizontal bar ---------------------------------------------------

/// One entry for the stacked bar chart.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct StackedBarEntry {
    pub label: String,
    pub success: u64,
    pub failure: u64,
}

/// Horizontal stacked bar chart: success (green) + failure (red) per tool.
///
/// Sorted by `failure` descending inside the component if `sort_by_failure` is true.
#[component]
pub(crate) fn StackedBarChart(
    entries: Vec<StackedBarEntry>,
    sort_by_failure: bool,
    on_click: Option<EventHandler<String>>,
) -> Element {
    const LABEL_W: u64 = 130;
    const ROW_H: u64 = 26;
    const PAD_TOP: u64 = 8;
    const TOTAL_W: u64 = 600;
    const BAR_AREA: u64 = TOTAL_W - LABEL_W - 60;

    let mut sorted = entries.clone();
    if sort_by_failure {
        sorted.sort_by(|a, b| b.failure.cmp(&a.failure));
    }

    let n = sorted.len() as u64;
    let chart_h = n * ROW_H + PAD_TOP * 2;

    let max_total = sorted
        .iter()
        .map(|e| e.success + e.failure)
        .max()
        .unwrap_or(1)
        .max(1);

    rsx! {
        svg {
            width: "100%",
            view_box: "0 0 {TOTAL_W} {chart_h}",
            style: "display: block; overflow: visible;",

            for (i, entry) in sorted.iter().enumerate() {
                {
                    let y = PAD_TOP + i as u64 * ROW_H;
                    let total = entry.success + entry.failure;
                    let success_w = (entry.success * BAR_AREA) / max_total;
                    let failure_w = (entry.failure * BAR_AREA) / max_total;
                    let label = entry.label.clone();
                    let clickable = on_click.is_some();
                    let cursor = if clickable { "pointer" } else { "default" };
                    let pct_fail = if total > 0 { entry.failure * 100 / total } else { 0 };

                    rsx! {
                        g {
                            key: "{label}",
                            style: "cursor: {cursor};",
                            onclick: {
                                let label = label.clone();
                                let on_click = on_click.clone();
                                move |_| {
                                    if let Some(ref handler) = on_click {
                                        handler.call(label.clone());
                                    }
                                }
                            },

                            text {
                                x: "0",
                                y: "{y + ROW_H - 8}",
                                fill: COLOR_TEXT,
                                font_size: "12",
                                font_family: "monospace",
                                "{truncate_label(&label, 18)}"
                            }

                            // Background
                            rect {
                                x: "{LABEL_W}",
                                y: "{y + 4}",
                                width: "{BAR_AREA}",
                                height: "{ROW_H - 10}",
                                fill: "#1a1a2e",
                                rx: "3",
                            }

                            // Success segment
                            if success_w > 0 {
                                rect {
                                    x: "{LABEL_W}",
                                    y: "{y + 4}",
                                    width: "{success_w}",
                                    height: "{ROW_H - 10}",
                                    fill: COLOR_SUCCESS,
                                    rx: "3",
                                }
                            }

                            // Failure segment
                            if failure_w > 0 {
                                rect {
                                    x: "{LABEL_W + success_w}",
                                    y: "{y + 4}",
                                    width: "{failure_w}",
                                    height: "{ROW_H - 10}",
                                    fill: COLOR_FAILURE,
                                    rx: "3",
                                }
                            }

                            // Failure % label
                            text {
                                x: "{LABEL_W + BAR_AREA + 6}",
                                y: "{y + ROW_H - 8}",
                                fill: if pct_fail > 50 { COLOR_FAILURE } else if pct_fail > 20 { COLOR_WARN } else { COLOR_TEXT_DIM },
                                font_size: "11",
                                font_family: "monospace",
                                "{pct_fail}% fail"
                            }
                        }
                    }
                }
            }
        }
    }
}

// -- Line chart ---------------------------------------------------------------

/// A single data point in a line series.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct LinePoint {
    pub label: String,
    pub value: f64,
}

/// A single named series.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct LineSeries {
    pub name: String,
    pub color: String,
    pub points: Vec<LinePoint>,
}

/// Line chart for time-series data. Supports multiple series.
///
/// X labels are drawn from the first series's point labels.
#[component]
pub(crate) fn LineChart(series: Vec<LineSeries>, height: u32) -> Element {
    const PAD_L: f64 = 40.0;
    const PAD_R: f64 = 10.0;
    const PAD_TOP: f64 = 10.0;
    const PAD_BOT: f64 = 24.0;
    const TOTAL_W: f64 = 580.0;

    if series.is_empty() || series[0].points.is_empty() {
        return rsx! {
            div {
                style: "color: #555; font-size: 12px; padding: 8px;",
                "No data"
            }
        };
    }

    let chart_h = height as f64;
    let plot_w = TOTAL_W - PAD_L - PAD_R;
    let plot_h = chart_h - PAD_TOP - PAD_BOT;

    let all_values: Vec<f64> = series
        .iter()
        .flat_map(|s| s.points.iter().map(|p| p.value))
        .collect();
    let max_val = all_values
        .iter()
        .cloned()
        .fold(f64::NEG_INFINITY, f64::max)
        .max(1.0);

    let n_points = series[0].points.len();
    let step = if n_points > 1 {
        plot_w / (n_points - 1) as f64
    } else {
        plot_w
    };

    rsx! {
        svg {
            width: "100%",
            view_box: "0 0 {TOTAL_W} {chart_h}",
            style: "display: block;",

            // Y-axis gridlines
            for tick in 0..=4u32 {
                {
                    let y = PAD_TOP + plot_h * (1.0 - tick as f64 / 4.0);
                    let val = max_val * tick as f64 / 4.0;
                    rsx! {
                        g {
                            key: "tick-{tick}",
                            line {
                                x1: "{PAD_L}",
                                y1: "{y}",
                                x2: "{TOTAL_W - PAD_R}",
                                y2: "{y}",
                                stroke: "#222",
                                stroke_width: "1",
                            }
                            text {
                                x: "{PAD_L - 4.0}",
                                y: "{y + 4.0}",
                                fill: COLOR_TEXT_DIM,
                                font_size: "10",
                                text_anchor: "end",
                                "{format_axis_val(val)}"
                            }
                        }
                    }
                }
            }

            // X labels (every ~nth point to avoid crowding)
            for (i, pt) in series[0].points.iter().enumerate() {
                if should_show_x_label(i, n_points) {
                    {
                        let x = PAD_L + i as f64 * step;
                        let label = pt.label.clone();
                        rsx! {
                            text {
                                key: "xlabel-{i}",
                                x: "{x}",
                                y: "{chart_h - 4.0}",
                                fill: COLOR_TEXT_DIM,
                                font_size: "9",
                                text_anchor: "middle",
                                "{truncate_label(&label, 8)}"
                            }
                        }
                    }
                }
            }

            // Series lines
            for (si, s) in series.iter().enumerate() {
                {
                    let points_str: String = s
                        .points
                        .iter()
                        .enumerate()
                        .map(|(i, p)| {
                            let x = PAD_L + i as f64 * step;
                            let y = PAD_TOP + plot_h * (1.0 - p.value / max_val);
                            format!("{x:.1},{y:.1}")
                        })
                        .collect::<Vec<_>>()
                        .join(" ");
                    rsx! {
                        polyline {
                            key: "series-{si}",
                            points: "{points_str}",
                            fill: "none",
                            stroke: "{s.color}",
                            stroke_width: "2",
                        }
                    }
                }
            }

            // Legend
            for (si, s) in series.iter().enumerate() {
                {
                    let lx = PAD_L + si as f64 * 100.0;
                    let ly = PAD_TOP - 2.0;
                    let name = truncate_label(&s.name, 12);
                    rsx! {
                        g { key: "legend-{si}",
                            rect {
                                x: "{lx}",
                                y: "{ly}",
                                width: "12",
                                height: "4",
                                fill: "{s.color}",
                            }
                            text {
                                x: "{lx + 14.0}",
                                y: "{ly + 4.0}",
                                fill: COLOR_TEXT_DIM,
                                font_size: "9",
                                "{name}"
                            }
                        }
                    }
                }
            }
        }
    }
}

// -- Percentile bar -----------------------------------------------------------

/// A single tool's percentile distribution for the percentile bar chart.
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

/// Horizontal percentile bars showing distribution of execution times.
///
/// Each row renders: whiskers (min—max), IQR box (p25—p75), median marker.
#[component]
pub(crate) fn PercentileBarChart(entries: Vec<PercentileEntry>) -> Element {
    const LABEL_W: u64 = 130;
    const ROW_H: u64 = 32;
    const PAD_TOP: u64 = 8;
    const TOTAL_W: u64 = 600;
    const BAR_AREA: u64 = TOTAL_W - LABEL_W - 60;

    let n = entries.len() as u64;
    let chart_h = n * ROW_H + PAD_TOP * 2;

    let scale_max = entries.iter().map(|e| e.p95_ms).max().unwrap_or(1).max(1);

    rsx! {
        svg {
            width: "100%",
            view_box: "0 0 {TOTAL_W} {chart_h}",
            style: "display: block; overflow: visible;",

            for (i, entry) in entries.iter().enumerate() {
                {
                    let y = PAD_TOP + i as u64 * ROW_H;
                    let cy = y + ROW_H / 2;

                    let x = |ms: u64| -> u64 {
                        LABEL_W + (ms.min(scale_max) * BAR_AREA) / scale_max
                    };

                    let x_min = x(entry.min_ms);
                    let x_p25 = x(entry.p25_ms);
                    let x_p50 = x(entry.p50_ms);
                    let x_p75 = x(entry.p75_ms);
                    let x_p95 = x(entry.p95_ms);
                    let label = entry.label.clone();
                    let p95_color = if entry.p95_ms > 10_000 { COLOR_FAILURE }
                        else if entry.p95_ms > 5_000 { COLOR_WARN }
                        else { COLOR_SUCCESS };

                    rsx! {
                        g { key: "{label}",
                            // Tool label
                            text {
                                x: "0",
                                y: "{cy + 4}",
                                fill: COLOR_TEXT,
                                font_size: "12",
                                font_family: "monospace",
                                "{truncate_label(&label, 18)}"
                            }

                            // Whisker line (min to p95)
                            line {
                                x1: "{x_min}",
                                y1: "{cy}",
                                x2: "{x_p95}",
                                y2: "{cy}",
                                stroke: COLOR_MUTED,
                                stroke_width: "1",
                            }

                            // IQR box (p25 to p75)
                            rect {
                                x: "{x_p25}",
                                y: "{cy - 6}",
                                width: "{x_p75.saturating_sub(x_p25)}",
                                height: "12",
                                fill: p95_color,
                                fill_opacity: "0.3",
                                stroke: p95_color,
                                stroke_width: "1",
                                rx: "2",
                            }

                            // Median marker
                            line {
                                x1: "{x_p50}",
                                y1: "{cy - 7}",
                                x2: "{x_p50}",
                                y2: "{cy + 7}",
                                stroke: p95_color,
                                stroke_width: "2",
                            }

                            // Min tick
                            line {
                                x1: "{x_min}",
                                y1: "{cy - 4}",
                                x2: "{x_min}",
                                y2: "{cy + 4}",
                                stroke: COLOR_MUTED,
                                stroke_width: "1",
                            }

                            // p95 label
                            text {
                                x: "{x_p95 + 4}",
                                y: "{cy + 4}",
                                fill: p95_color,
                                font_size: "10",
                                font_family: "monospace",
                                "p95:{format_ms_short(entry.p95_ms)}"
                            }
                        }
                    }
                }
            }
        }
    }
}

// -- Internal helpers ---------------------------------------------------------

fn truncate_label(label: &str, max_chars: usize) -> String {
    if label.len() <= max_chars {
        label.to_string()
    } else {
        format!("{}…", &label[..max_chars.saturating_sub(1)])
    }
}

fn format_axis_val(val: f64) -> String {
    if val >= 1_000_000.0 {
        format!("{:.0}M", val / 1_000_000.0)
    } else if val >= 1_000.0 {
        format!("{:.0}K", val / 1_000.0)
    } else if val > 0.0 {
        format!("{val:.0}")
    } else {
        String::new()
    }
}

fn format_ms_short(ms: u64) -> String {
    if ms >= 1_000 {
        format!("{:.1}s", ms as f64 / 1_000.0) // kanon:ignore RUST/as-cast
    } else {
        format!("{ms}ms")
    }
}

fn should_show_x_label(i: usize, n: usize) -> bool {
    if n <= 7 {
        return true;
    }
    let step = n / 6;
    i == 0 || i == n - 1 || i % step == 0
}
