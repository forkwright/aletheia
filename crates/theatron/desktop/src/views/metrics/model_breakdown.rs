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
            value: m.total() as f64,
            color: model_color(&m.model).to_string(),
            sub_label: None,
        })
        .collect();

    let total_display = format_tokens(grand_total);

    rsx! {
        div {
            style: "display: flex; flex-direction: column; gap: 16px;",

            DonutChart {
                segments,
                size_px: 160,
                center_label: total_display,
            }

            // Detail table
            div {
                style: "overflow-x: auto;",
                table {
                    style: "width: 100%; border-collapse: collapse; font-size: 12px; font-family: 'IBM Plex Mono', monospace;",
                    thead {
                        tr {
                            style: "border-bottom: 1px solid #2a2724;",
                            th { style: "padding: 6px 8px; text-align: left; color: #706c66;", "Model" }
                            th { style: "padding: 6px 8px; text-align: right; color: #706c66;", "Input" }
                            th { style: "padding: 6px 8px; text-align: right; color: #706c66;", "Output" }
                            th { style: "padding: 6px 8px; text-align: right; color: #706c66;", "Total" }
                            th { style: "padding: 6px 8px; text-align: right; color: #706c66;", "%" }
                            th { style: "padding: 6px 8px; text-align: right; color: #706c66;", "$/1K out" }
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
                                        style: "border-bottom: 1px solid #2a2724;",
                                        td {
                                            style: "padding: 6px 8px; color: {color}; white-space: nowrap;",
                                            title: "{model.model}",
                                            "{short}"
                                        }
                                        td { style: "padding: 6px 8px; color: #a8a49e; text-align: right;", "{format_tokens(model.input_tokens)}" }
                                        td { style: "padding: 6px 8px; color: #a8a49e; text-align: right;", "{format_tokens(model.output_tokens)}" }
                                        td { style: "padding: 6px 8px; color: #e8e6e3; text-align: right; font-weight: 600;", "{format_tokens(model.total())}" }
                                        td { style: "padding: 6px 8px; color: #706c66; text-align: right;", "{pct:.1}%" }
                                        td { style: "padding: 6px 8px; color: #706c66; text-align: right;", "${price:.4}" }
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
