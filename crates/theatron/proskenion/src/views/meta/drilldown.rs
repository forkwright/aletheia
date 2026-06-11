//! Chart drill-down: clickable chart cards that expand into a modal overlay.

use dioxus::prelude::*;

use crate::state::meta::DataPoint;

use super::{BarChart, CARD_LABEL, CARD_STYLE, LineChart};

/// Which renderer an expandable chart card uses.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum ChartKind {
    Line,
    Bar,
}

/// Chart currently selected for the enlarged drill-down overlay.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ExpandedChart {
    pub title: &'static str,
    pub kind: ChartKind,
    pub data: Vec<DataPoint>,
    pub color: &'static str,
}

const BACKDROP_STYLE: &str = "\
    position: fixed; \
    top: 0; \
    left: 0; \
    right: 0; \
    bottom: 0; \
    background: var(--bg-overlay); \
    display: flex; \
    align-items: center; \
    justify-content: center; \
    z-index: var(--z-modal);\
";

const DIALOG_STYLE: &str = "\
    background: var(--bg-surface); \
    border: 1px solid var(--border); \
    border-radius: var(--radius-lg); \
    box-shadow: var(--shadow-modal); \
    padding: var(--space-6); \
    width: min(90vw, 1000px); \
    max-height: 85vh; \
    overflow-y: auto;\
";

const DIALOG_HEADER_STYLE: &str = "\
    display: flex; \
    align-items: center; \
    justify-content: space-between; \
    margin-bottom: var(--space-4);\
";

const CLOSE_BTN_STYLE: &str = "\
    background: var(--bg-surface); \
    color: var(--text-secondary); \
    border: 1px solid var(--input-border); \
    border-radius: var(--radius-md); \
    padding: var(--space-1) var(--space-3); \
    font-size: var(--text-xs); \
    cursor: pointer; \
    transition: background-color var(--transition-quick), \
                color var(--transition-quick), \
                border-color var(--transition-quick);\
";

/// Card wrapping a chart; clicking anywhere on it opens the drill-down overlay.
#[component]
pub(crate) fn ChartCard(
    title: &'static str,
    kind: ChartKind,
    data: Vec<DataPoint>,
    color: &'static str,
    width: f64,
    height: f64,
    card_style: &'static str,
) -> Element {
    let mut expanded: Signal<Option<ExpandedChart>> = use_context();
    let modal_data = data.clone();

    rsx! {
        div {
            style: "{CARD_STYLE} cursor: zoom-in; {card_style}",
            title: "Click to enlarge",
            onclick: move |_| {
                expanded.set(Some(ExpandedChart {
                    title,
                    kind,
                    data: modal_data.clone(),
                    color,
                }));
            },
            div {
                style: "display: flex; align-items: center; justify-content: space-between; margin-bottom: var(--space-2);",
                div { style: "{CARD_LABEL} margin-top: 0;", "{title}" }
                span { style: "color: var(--text-muted); font-size: var(--text-xs);", "\u{2922}" }
            }
            ChartByKind { kind, data, color, width, height }
        }
    }
}

/// Dispatch to the line or bar renderer.
#[component]
fn ChartByKind(
    kind: ChartKind,
    data: Vec<DataPoint>,
    color: &'static str,
    width: f64,
    height: f64,
) -> Element {
    match kind {
        ChartKind::Line => rsx! {
            LineChart { data, width, height, color, show_labels: true }
        },
        ChartKind::Bar => rsx! {
            BarChart { data, width, height, color }
        },
    }
}

/// Modal overlay rendering the selected chart at full pane width.
///
/// Closes on backdrop click or the explicit close button.
#[component]
pub(super) fn ChartDrilldown() -> Element {
    let mut expanded: Signal<Option<ExpandedChart>> = use_context();
    let current = expanded.read().clone();

    rsx! {
        if let Some(chart) = current {
            div {
                style: "{BACKDROP_STYLE}",
                onclick: move |_| expanded.set(None),
                div {
                    style: "{DIALOG_STYLE}",
                    onclick: move |e| e.stop_propagation(),
                    div {
                        style: "{DIALOG_HEADER_STYLE}",
                        h3 {
                            style: "font-size: var(--text-lg); color: var(--text-primary); margin: 0;",
                            "{chart.title}"
                        }
                        button {
                            style: "{CLOSE_BTN_STYLE}",
                            onclick: move |_| expanded.set(None),
                            "Close"
                        }
                    }
                    ChartByKind {
                        kind: chart.kind,
                        data: chart.data.clone(),
                        color: chart.color,
                        width: 940.0,
                        height: 420.0,
                    }
                }
            }
        }
    }
}
