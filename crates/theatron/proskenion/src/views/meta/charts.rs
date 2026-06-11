//! SVG chart helper components for meta-insights view.

use dioxus::prelude::*;

use super::MUTED_TEXT;

const PAD: f64 = 30.0;
/// Left padding leaves room for right-anchored y-axis value ticks.
const PAD_LEFT: f64 = 44.0;

/// WHY: SVGs declare intrinsic width/height for aspect ratio but render at
/// 100% of the containing card, so charts fill the pane in both themes and
/// scale with the window.
const RESPONSIVE_SVG_STYLE: &str = "display: block; width: 100%; height: auto;";

/// Render a simple SVG line chart.
#[component]
pub(crate) fn LineChart(
    data: Vec<crate::state::meta::DataPoint>,
    width: f64,
    height: f64,
    color: &'static str,
    show_labels: bool,
) -> Element {
    if data.is_empty() {
        return rsx! {
            div { style: "{MUTED_TEXT}", "No data available" }
        };
    }

    let max_val = data
        .iter()
        .map(|p| p.value)
        .fold(f64::NEG_INFINITY, f64::max)
        .max(1.0);

    let chart_w = width - PAD_LEFT - PAD;
    let chart_h = height - PAD * 2.0;

    let points: Vec<(f64, f64)> = data
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let x = if data.len() > 1 {
                {
                    #[expect(
                        clippy::as_conversions,
                        reason = "point index to f64 for SVG coordinate"
                    )]
                    let x = PAD_LEFT + (i as f64 / (data.len() - 1) as f64) * chart_w;
                    x
                }
            } else {
                PAD_LEFT + chart_w / 2.0
            };
            let y = PAD + chart_h - (p.value / max_val) * chart_h;
            (x, y)
        })
        .collect();

    let path = points
        .iter()
        .enumerate()
        .map(|(i, (x, y))| {
            if i == 0 {
                format!("M {x:.1} {y:.1}")
            } else {
                format!("L {x:.1} {y:.1}")
            }
        })
        .collect::<Vec<_>>()
        .join(" ");

    let viewbox = format!("0 0 {width} {height}");

    rsx! {
        svg {
            width: "{width}",
            height: "{height}",
            view_box: "{viewbox}",
            style: "{RESPONSIVE_SVG_STYLE}",

            {grid_elements(max_val, chart_w, chart_h)}

            path {
                d: "{path}",
                fill: "none",
                stroke: "{color}",
                stroke_width: "2",
            }

            for (x , y) in &points {
                circle {
                    cx: "{x:.1}",
                    cy: "{y:.1}",
                    r: "3",
                    fill: "{color}",
                }
            }

            if show_labels {
                {label_elements(&data, &points, height)}
            }
        }
    }
}

/// Build SVG text label elements for the first and last data points.
///
/// WHY: Extracted because Dioxus 0.7 rsx! macro cannot parse SVG `text`
/// elements inside `if let Some(...)` conditional blocks -- the `text`
/// identifier conflicts with the text-node parser. A standalone function
/// with its own top-level rsx! avoids the ambiguity.
fn label_elements(
    data: &[crate::state::meta::DataPoint],
    points: &[(f64, f64)],
    height: f64,
) -> Element {
    let first_label = data.first().map(|p| p.label.clone());
    let first_x = points.first().map(|&(x, _)| x);
    let last_label = if data.len() > 1 {
        data.last().map(|p| p.label.clone())
    } else {
        None
    };
    let last_x = if data.len() > 1 {
        points.last().map(|&(x, _)| x)
    } else {
        None
    };

    let y_pos = format!("{:.1}", height - 4.0);

    rsx! {
        if let (Some(label), Some(x)) = (first_label, first_x) {
            text {
                x: "{x:.1}",
                y: "{y_pos}",
                fill: "var(--text-muted)",
                font_size: "11",
                text_anchor: "start",
                "{label}"
            }
        }
        if let (Some(label), Some(x)) = (last_label, last_x) {
            text {
                x: "{x:.1}",
                y: "{y_pos}",
                fill: "var(--text-muted)",
                font_size: "11",
                text_anchor: "end",
                "{label}"
            }
        }
    }
}

/// Build horizontal gridlines with right-anchored y-axis value ticks.
///
/// WHY: Extracted for the same rsx-parser reason as `label_elements`; shared
/// by the line and bar charts so both carry readable value axes.
fn grid_elements(max_val: f64, chart_w: f64, chart_h: f64) -> Element {
    rsx! {
        for i in 0..5u32 {
            {
                let y = PAD + (f64::from(i) / 4.0) * chart_h;
                let tick = format_axis_value(max_val * (1.0 - f64::from(i) / 4.0));
                rsx! {
                    line {
                        x1: "{PAD_LEFT:.1}",
                        y1: "{y:.1}",
                        x2: "{PAD_LEFT + chart_w:.1}",
                        y2: "{y:.1}",
                        stroke: "var(--border)",
                        stroke_width: "1",
                    }
                    text {
                        x: "{PAD_LEFT - 6.0:.1}",
                        y: "{y + 3.0:.1}",
                        fill: "var(--text-muted)",
                        font_size: "10",
                        text_anchor: "end",
                        "{tick}"
                    }
                }
            }
        }
    }
}

/// Compact axis tick formatting (K/M suffix, short decimals).
fn format_axis_value(v: f64) -> String {
    if v >= 1_000_000.0 {
        format!("{:.1}M", v / 1_000_000.0)
    } else if v >= 1_000.0 {
        format!("{:.1}K", v / 1_000.0)
    } else if v >= 10.0 || v == 0.0 {
        format!("{v:.0}")
    } else {
        format!("{v:.1}")
    }
}

/// Render a horizontal bar chart.
#[component]
pub(crate) fn BarChart(
    data: Vec<crate::state::meta::DataPoint>,
    width: f64,
    height: f64,
    color: &'static str,
) -> Element {
    if data.is_empty() {
        return rsx! {
            div { style: "{MUTED_TEXT}", "No data available" }
        };
    }

    let max_val = data
        .iter()
        .map(|p| p.value)
        .fold(f64::NEG_INFINITY, f64::max)
        .max(1.0);

    let chart_w = width - PAD_LEFT - PAD;
    let chart_h = height - PAD * 2.0;
    #[expect(clippy::as_conversions, reason = "data count to f64 for SVG bar width")]
    let bar_width = (chart_w / data.len() as f64) * 0.7;
    #[expect(clippy::as_conversions, reason = "data count to f64 for SVG gap")]
    let gap = (chart_w / data.len() as f64) * 0.3;

    let viewbox = format!("0 0 {width} {height}");

    // WHY: Label only every Nth bar so x-axis labels never overlap on dense
    // series; ~70 viewBox units per label keeps date strings legible.
    #[expect(clippy::as_conversions, reason = "bounded label stride for display")]
    let label_step = ((data.len() as f64) * 70.0 / chart_w).ceil().max(1.0) as usize;

    rsx! {
        svg {
            width: "{width}",
            height: "{height}",
            view_box: "{viewbox}",
            style: "{RESPONSIVE_SVG_STYLE}",

            {grid_elements(max_val, chart_w, chart_h)}

            for (i , point) in data.iter().enumerate() {
                {
                    let bar_h = (point.value / max_val) * chart_h;
                    #[expect(clippy::as_conversions, reason = "bar index to f64 for SVG position")]
                    let x = PAD_LEFT + (i as f64 * (bar_width + gap)) + gap / 2.0;
                    let y = PAD + chart_h - bar_h;
                    // NOTE: Empty label skips rendering without an rsx `if`
                    // around a `text` node (parser limitation, see above).
                    let label = if i % label_step == 0 {
                        point.label.as_str()
                    } else {
                        ""
                    };
                    rsx! {
                        rect {
                            x: "{x:.1}",
                            y: "{y:.1}",
                            width: "{bar_width:.1}",
                            height: "{bar_h:.1}",
                            fill: "{color}",
                            rx: "2",
                        }
                        text {
                            x: "{x + bar_width / 2.0:.1}",
                            y: "{height - 4.0:.1}",
                            fill: "var(--text-muted)",
                            font_size: "10",
                            text_anchor: "middle",
                            "{label}"
                        }
                    }
                }
            }
        }
    }
}
