//! Model token usage breakdown: donut chart + table.

use dioxus::prelude::*;

use crate::components::chart::{ChartEntry, DonutChart};
use crate::state::metrics::{cost_per_1k_output, format_tokens, model_color, ModelTokenRow};

/// Per-model token breakdown with donut chart.
#[component]
pub(crate) fn ModelBreakdown(models: Vec<ModelTokenRow>, grand_total: u64) -> Element {
    let segments: Vec<ChartEntry> = models
        .iter()
        .map(|m| ChartEntry {
            label: short_model_name(&m.model),
            #[expect(clippy::as_conversions, reason = "u64 token count to f64 for chart value")]
            value: m.total() as f64,
            color: model_color(&m.model).to_string(),
            sub_label: None,
        })
        .collect();

    let total_display = format_tokens(grand_total);

    rsx! {
        div {
            style: "display: flex; flex-direction: column; gap: var(--space-4);",

            DonutChart {
                segments,
                size_px: 160,
                center_label: total_display,
            }

            // Detail table
            div {
                style: "overflow-x: auto;",
                table {
                    style: "width: 100%; border-collapse: collapse; font-size: var(--text-xs); font-family: var(--font-mono);",
                    thead {
                        tr {
                            style: "border-bottom: 1px solid var(--border);",
                            th { style: "padding: var(--space-2) var(--space-2); text-align: left; color: var(--text-muted);", "Model" }
                            th { style: "padding: var(--space-2) var(--space-2); text-align: right; color: var(--text-muted);", "Input" }
                            th { style: "padding: var(--space-2) var(--space-2); text-align: right; color: var(--text-muted);", "Output" }
                            th { style: "padding: var(--space-2) var(--space-2); text-align: right; color: var(--text-muted);", "Total" }
                            th { style: "padding: var(--space-2) var(--space-2); text-align: right; color: var(--text-muted);", "%" }
                            th { style: "padding: var(--space-2) var(--space-2); text-align: right; color: var(--text-muted);", "$/1K out" }
                        }
                    }
                    tbody {
                        for model in &models {
                            {
                                let pct = model.pct_of_total(grand_total);
                                let price = cost_per_1k_output(&model.model);
                                let color = model_color(&model.model);
                                let short = short_model_name(&model.model);
                                rsx! {
                                    tr {
                                        key: "{model.model}",
                                        style: "border-bottom: 1px solid var(--border);",
                                        td {
                                            style: "padding: var(--space-2) var(--space-2); color: {color}; white-space: nowrap;",
                                            title: "{model.model}",
                                            "{short}"
                                        }
                                        td { style: "padding: var(--space-2) var(--space-2); color: var(--text-secondary); text-align: right;", "{format_tokens(model.input_tokens)}" }
                                        td { style: "padding: var(--space-2) var(--space-2); color: var(--text-secondary); text-align: right;", "{format_tokens(model.output_tokens)}" }
                                        td { style: "padding: var(--space-2) var(--space-2); color: var(--text-primary); text-align: right; font-weight: var(--weight-semibold);", "{format_tokens(model.total())}" }
                                        td { style: "padding: var(--space-2) var(--space-2); color: var(--text-muted); text-align: right;", "{pct:.1}%" }
                                        td { style: "padding: var(--space-2) var(--space-2); color: var(--text-muted); text-align: right;", "${price:.4}" }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

/// Strip provider prefix from a model ID for compact display.
fn short_model_name(model: &str) -> String {
    model
        .split('/')
        .last()
        .unwrap_or(model)
        .to_string()
}
