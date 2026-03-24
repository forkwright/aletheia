//! SVG chart helper components for meta-insights view.

use dioxus::prelude::*;

use super::MUTED_TEXT;

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

    let padding = 30.0;
    let chart_w = width - padding * 2.0;
    let chart_h = height - padding * 2.0;

    let points: Vec<(f64, f64)> = data
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let x = if data.len() > 1 {
                padding + (i as f64 / (data.len() - 1) as f64) * chart_w
            } else {
                padding + chart_w / 2.0
            };
            let y = padding + chart_h - (p.value / max_val) * chart_h;
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
            style: "display: block;",

            // NOTE: Grid lines.
            for i in 0..5u32 {
                {
                    let y = padding + (f64::from(i) / 4.0) * chart_h;
                    rsx! {
                        line {
                            x1: "{padding}",
                            y1: "{y:.1}",
                            x2: "{padding + chart_w:.1}",
                            y2: "{y:.1}",
                            stroke: "#2a2a3a",
                            stroke_width: "1",
                        }
                    }
                }
            }

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
                // NOTE: Show first and last labels only to avoid clutter.
                if let Some((first, &(x, _))) = data.first().zip(points.first()) {
                    rsx! {
                        text {
                            x: "{x:.1}",
                            y: "{height - 4.0:.1}",
                            fill: "#666",
                            font_size: "10",
                            text_anchor: "start",
                            "{first.label}"
                        }
                    }
                }
                if data.len() > 1 {
                    if let Some((last, &(x, _))) = data.last().zip(points.last()) {
                        rsx! {
                            text {
                                x: "{x:.1}",
                                y: "{height - 4.0:.1}",
                                fill: "#666",
                                font_size: "10",
                                text_anchor: "end",
                                "{last.label}"
                            }
                        }
                    }
                }
            }
        }
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

    let padding = 30.0;
    let chart_w = width - padding * 2.0;
    let chart_h = height - padding * 2.0;
    let bar_width = (chart_w / data.len() as f64) * 0.7;
    let gap = (chart_w / data.len() as f64) * 0.3;

    let viewbox = format!("0 0 {width} {height}");

    rsx! {
        svg {
            width: "{width}",
            height: "{height}",
            view_box: "{viewbox}",
            style: "display: block;",

            for (i , point) in data.iter().enumerate() {
                {
                    let bar_h = (point.value / max_val) * chart_h;
                    let x = padding + (i as f64 * (bar_width + gap)) + gap / 2.0;
                    let y = padding + chart_h - bar_h;
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
                            fill: "#666",
                            font_size: "9",
                            text_anchor: "middle",
                            "{point.label}"
                        }
                    }
                }
            }
        }
    }
}
