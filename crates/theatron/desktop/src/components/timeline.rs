//! Reusable horizontal timeline with block rendering, dependency arrows, and zoom/pan.

use dioxus::prelude::*;

/// A positioned block in the timeline.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct TimelineBlock {
    /// Unique identifier.
    pub(crate) id: String,
    /// Display label.
    pub(crate) label: String,
    /// Horizontal offset in pixels (pre-zoom).
    pub(crate) x: f64,
    /// Width in pixels (pre-zoom).
    pub(crate) width: f64,
    /// Block fill color.
    pub(crate) color: &'static str,
    /// Border/accent color.
    pub(crate) border_color: &'static str,
    /// Completion percentage 0–100.
    pub(crate) progress: u8,
    /// Whether this is the currently active block.
    pub(crate) active: bool,
    /// Subtitle text (e.g., date range).
    pub(crate) detail: String,
}

/// A dependency arrow between two block indices.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct TimelineDependencyLine {
    pub(crate) from_idx: usize,
    pub(crate) to_idx: usize,
}

const CONTROLS_BAR: &str = "\
    display: flex; \
    align-items: center; \
    gap: 8px; \
    padding: 8px 12px; \
    border-bottom: 1px solid #2a2a3a; \
    background: #1a1a2e;\
";

const ZOOM_BTN: &str = "\
    background: #2a2a4a; \
    color: #e0e0e0; \
    border: 1px solid #444; \
    border-radius: 4px; \
    padding: 2px 10px; \
    font-size: 14px; \
    cursor: pointer;\
";

const BLOCK_HEIGHT: f64 = 56.0;
const BLOCK_Y: f64 = 40.0;
const ROW_HEIGHT: f64 = 120.0;

/// Horizontal timeline with phase blocks, dependency arrows, and zoom controls.
#[component]
pub(crate) fn Timeline(
    blocks: Vec<TimelineBlock>,
    dependencies: Vec<TimelineDependencyLine>,
    zoom: f64,
    on_zoom_change: EventHandler<f64>,
    on_block_click: EventHandler<usize>,
) -> Element {
    let total_width = blocks
        .iter()
        .map(|b| (b.x + b.width) * zoom)
        .fold(0.0f64, f64::max)
        + 80.0;

    let zoomed: Vec<(usize, f64, f64, &TimelineBlock)> = blocks
        .iter()
        .enumerate()
        .map(|(i, b)| (i, b.x * zoom, b.width * zoom, b))
        .collect();

    rsx! {
        div {
            div {
                style: "{CONTROLS_BAR}",
                button {
                    style: "{ZOOM_BTN}",
                    onclick: move |_| on_zoom_change.call((zoom * 0.8).max(0.2)),
                    "-"
                }
                span {
                    style: "font-size: 12px; color: #888; min-width: 48px; text-align: center;",
                    "{format_zoom(zoom)}"
                }
                button {
                    style: "{ZOOM_BTN}",
                    onclick: move |_| on_zoom_change.call((zoom * 1.25).min(5.0)),
                    "+"
                }
                button {
                    style: "{ZOOM_BTN}",
                    onclick: move |_| on_zoom_change.call(1.0),
                    "Fit"
                }
            }

            div {
                style: "overflow-x: auto; overflow-y: hidden; border: 1px solid #2a2a3a; border-radius: 0 0 8px 8px; background: #0f0f1a; cursor: grab;",

                div {
                    style: "position: relative; min-width: {total_width}px; height: {ROW_HEIGHT}px; padding: 0 20px;",

                    // Time axis labels
                    for (i, x, _w, block) in &zoomed {
                        div {
                            key: "label-{i}",
                            style: "position: absolute; left: {x_offset(*x)}px; top: 4px; font-size: 10px; color: #555;",
                            "{block.detail}"
                        }
                    }

                    // Phase blocks
                    for (i, x, w, block) in &zoomed {
                        {
                            let border = if block.active {
                                format!("2px solid {}", block.border_color)
                            } else {
                                format!("1px solid {}", block.border_color)
                            };
                            let progress_w = (*w * f64::from(block.progress) / 100.0).max(0.0);
                            let idx = *i;
                            let bx = x_offset(*x);
                            rsx! {
                                div {
                                    key: "block-{i}",
                                    style: "position: absolute; left: {bx}px; top: {BLOCK_Y}px; width: {w}px; height: {BLOCK_HEIGHT}px; background: {block.color}; border: {border}; border-radius: 6px; padding: 6px 8px; box-sizing: border-box; cursor: pointer; overflow: hidden;",
                                    onclick: move |_| on_block_click.call(idx),

                                    div {
                                        style: "position: absolute; left: 0; top: 0; width: {progress_w}px; height: 100%; background: rgba(255,255,255,0.06); border-radius: 6px 0 0 6px;",
                                    }

                                    div {
                                        style: "font-size: 12px; font-weight: 600; color: #e0e0e0; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; position: relative;",
                                        "{block.label}"
                                    }
                                    div {
                                        style: "font-size: 10px; color: #888; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; position: relative;",
                                        "{block.progress}% complete"
                                    }
                                }
                            }
                        }
                    }

                    // Dependency arrows (SVG overlay)
                    svg {
                        style: "position: absolute; left: 0; top: 0; pointer-events: none;",
                        width: "{total_width}",
                        height: "{ROW_HEIGHT}",

                        for dep in &dependencies {
                            {render_dependency(&zoomed, dep)}
                        }
                    }
                }
            }
        }
    }
}

fn render_dependency(
    zoomed: &[(usize, f64, f64, &TimelineBlock)],
    dep: &TimelineDependencyLine,
) -> Element {
    let from = zoomed.get(dep.from_idx);
    let to = zoomed.get(dep.to_idx);

    let (Some((_fi, fx, fw, _fb)), Some((_ti, tx, _tw, _tb))) = (from, to) else {
        return rsx! {};
    };

    let start_x = x_offset(*fx) + fw;
    let start_y = BLOCK_Y + BLOCK_HEIGHT / 2.0;
    let end_x = x_offset(*tx);
    let end_y = BLOCK_Y + BLOCK_HEIGHT / 2.0;
    let ctrl_dx = (end_x - start_x).abs() / 3.0;

    let curve = format!(
        "M {start_x} {start_y} C {} {start_y}, {} {end_y}, {end_x} {end_y}",
        start_x + ctrl_dx,
        end_x - ctrl_dx,
    );

    // Small arrowhead triangle at endpoint.
    let arrow = format!(
        "M {} {} L {end_x} {end_y} L {} {} Z",
        end_x - 6.0,
        end_y - 3.5,
        end_x - 6.0,
        end_y + 3.5,
    );

    rsx! {
        path {
            key: "line-{dep.from_idx}-{dep.to_idx}",
            d: "{curve}",
            stroke: "#4a9aff",
            stroke_width: "1.5",
            fill: "none",
        }
        path {
            d: "{arrow}",
            fill: "#4a9aff",
            stroke: "none",
        }
    }
}

/// Offset blocks by padding.
fn x_offset(x: f64) -> f64 {
    x + 20.0
}

fn format_zoom(zoom: f64) -> String {
    format!("{}%", (zoom * 100.0) as u32)
}

/// Calculate block positions from date ranges with a fixed pixels-per-day scale.
///
/// Each entry is `(start_date, end_date)`. Returns `(x_offset, width)` pairs.
#[must_use]
pub(crate) fn phase_positions(phases: &[(String, String)], pixels_per_day: f64) -> Vec<(f64, f64)> {
    use crate::state::planning::days_between;

    if phases.is_empty() {
        return Vec::new();
    }

    let earliest = phases.iter().map(|(s, _)| s.as_str()).min().unwrap_or("");

    phases
        .iter()
        .map(|(start, end)| {
            let offset_days = if earliest.is_empty() || start.is_empty() {
                0
            } else {
                days_between(earliest, start)
            };
            let duration_days = days_between(start, end);
            let x = f64::from(offset_days) * pixels_per_day;
            let w = f64::from(duration_days.max(1)) * pixels_per_day;
            (x, w)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_zoom_display() {
        assert_eq!(format_zoom(1.0), "100%");
        assert_eq!(format_zoom(0.5), "50%");
        assert_eq!(format_zoom(2.0), "200%");
    }

    #[test]
    fn phase_positions_empty_returns_empty() {
        assert!(
            phase_positions(&[], 4.0).is_empty(),
            "no phases, no positions"
        );
    }

    #[test]
    fn phase_positions_single_phase() {
        let phases = vec![("2026-01-01".to_string(), "2026-02-01".to_string())];
        let positions = phase_positions(&phases, 4.0);
        assert_eq!(positions.len(), 1, "one phase");
        let (_x, w) = positions[0];
        assert!(w > 80.0, "~30 days * 4px should be > 80px, got {w}");
    }

    #[test]
    fn phase_positions_multiple_ordered() {
        let phases = vec![
            ("2026-01-01".to_string(), "2026-02-01".to_string()),
            ("2026-02-01".to_string(), "2026-03-01".to_string()),
        ];
        let positions = phase_positions(&phases, 4.0);
        assert_eq!(positions.len(), 2, "two phases");
        let (x0, _) = positions[0];
        let (x1, _) = positions[1];
        assert!(x1 > x0, "second phase starts after first: {x0} vs {x1}");
    }

    #[test]
    fn x_offset_adds_padding() {
        assert!((x_offset(0.0) - 20.0).abs() < f64::EPSILON, "20px padding");
        assert!(
            (x_offset(100.0) - 120.0).abs() < f64::EPSILON,
            "100 + 20 = 120"
        );
    }
}
