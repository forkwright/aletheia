//! Tools tab: summary cards, date range selector, and sub-view host.

use dioxus::prelude::*;

use crate::api::client::authenticated_client;
use crate::state::connection::ConnectionConfig;
use crate::state::fetch::FetchState;
use crate::state::tool_metrics::{DateRange, ToolStatsResponse, format_delta, format_duration_ms};

/// Local store for the tools overview view.
#[derive(Debug, Clone)]
#[expect(dead_code, reason = "used by ToolsOverview component")]
struct ToolsOverviewStore {
    data: FetchState<ToolStatsResponse>,
    date_range: DateRange,
    selected_tool: Option<String>,
}

use super::tool_detail::ToolDetailView;
use super::tool_duration::ToolDurationView;
use super::tool_frequency::ToolFrequencyView;
use super::tool_results::ToolResultsView;

// -- Component ----------------------------------------------------------------

#[component]
pub(crate) fn ToolsOverview(date_range: DateRange) -> Element {
    let config: Signal<ConnectionConfig> = use_context();
    let mut store = use_signal(|| ToolsOverviewStore {
        data: FetchState::Loading,
        date_range,
        selected_tool: None,
    });

    let mut do_fetch = move || {
        let cfg = config.read().clone();
        let range = store.read().date_range;
        store.write().data = FetchState::Loading;

        spawn(async move {
            let client = authenticated_client(&cfg);
            let base = cfg.server_url.trim_end_matches('/');
            let days = range.days();
            let url = format!("{base}/api/tool-stats?days={days}");

            match client.get(&url).send().await {
                Ok(resp) if resp.status().is_success() => {
                    match resp.json::<ToolStatsResponse>().await {
                        Ok(data) => store.write().data = FetchState::Loaded(data),
                        Err(e) => {
                            store.write().data = FetchState::Error(format!("parse error: {e}"));
                        }
                    }
                }
                Ok(resp) => {
                    store.write().data =
                        FetchState::Error(format!("tool-stats returned {}", resp.status()));
                }
                Err(e) => {
                    store.write().data = FetchState::Error(format!("connection error: {e}"));
                }
            }
        });
    };

    use_effect(move || {
        do_fetch();
    });

    // Read selected tool for drill-down routing.
    let selected_tool = store.read().selected_tool.clone();

    rsx! {
        div {
            style: "display: flex; flex-direction: column; gap: 16px; flex: 1;",

            // Date range selector + refresh
            div {
                style: "display: flex; align-items: center; gap: 8px;",
                span { style: "font-size: 12px; color: #888;", "Range:" }
                for range in [DateRange::Last7Days] {
                    {
                        let is_active = store.read().date_range == range;
                        let btn_style = if is_active {
                            "background: #2a2a4a; color: #e0e0e0; border: 1px solid #4a4aff; border-radius: 4px; padding: 2px 10px; font-size: 12px; cursor: pointer;"
                        } else {
                            "background: transparent; color: #666; border: 1px solid #333; border-radius: 4px; padding: 2px 10px; font-size: 12px; cursor: pointer;"
                        };
                        rsx! {
                            button {
                                key: "{range.days()}d",
                                style: "{btn_style}",
                                onclick: move |_| {
                                    store.write().date_range = range;
                                    do_fetch();
                                },
                                "{range.days()}d"
                            }
                        }
                    }
                }
                button {
                    style: "background: #2a2a4a; color: #e0e0e0; border: 1px solid #444; border-radius: 6px; padding: 4px 12px; font-size: 12px; cursor: pointer;",
                    onclick: move |_| do_fetch(),
                    "Refresh"
                }
            }

            // If a tool is selected, show detail view; otherwise show overview.
            if let Some(ref tool_name) = selected_tool {
                ToolDetailView {
                    tool_name: tool_name.clone(),
                    date_range: store.read().date_range,
                    on_back: move |_| {
                        store.write().selected_tool = None;
                    },
                }
            } else {
                {render_overview_content(store, move |tool_name| {
                    store.write().selected_tool = Some(tool_name);
                })}
            }
        }
    }
}

#[expect(dead_code, reason = "used by ToolsOverview component")]
fn render_overview_content(
    store: Signal<ToolsOverviewStore>,
    mut on_select_tool: impl FnMut(String) + 'static,
) -> Element {
    match store.read().data.clone() {
        FetchState::Loading => rsx! {
            div {
                style: "display: flex; align-items: center; justify-content: center; flex: 1; color: #888;",
                "Loading tool statistics..."
            }
        },
        FetchState::Error(err) => rsx! {
            div {
                style: "color: #ef4444; padding: 8px;",
                "Error: {err}"
            }
        },
        FetchState::Loaded(data) => {
            let mut on_select = move |tool_name: String| on_select_tool(tool_name);
            rsx! {
                // Summary cards
                div {
                    style: "display: flex; flex-wrap: wrap; gap: 12px;",

                    // Total invocations (weekly)
                    div {
                        style: "background: #1a1a2e; border: 1px solid #333; border-radius: 8px; padding: 16px 20px; min-width: 150px; flex: 1;",
                        div { style: "font-size: 28px; font-weight: bold; color: #e0e0e0; margin-bottom: 2px;", "{data.summary.total_invocations_week}" }
                        div { style: "font-size: 11px; color: #888; text-transform: uppercase; letter-spacing: 0.5px;", "Invocations (7d)" }
                        div {
                            style: "font-size: 11px; margin-top: 6px; color: #888;",
                            "{format_delta(data.summary.delta_week)} vs prior"
                        }
                    }

                    // Overall success rate
                    {
                        #[expect(clippy::as_conversions, reason = "rate 0.0–1.0 to percentage for display")]
                        let rate_pct = (data.summary.success_rate * 100.0) as u64;
                        let rate_color = if data.summary.success_rate > 0.9 { "#22c55e" }
                            else if data.summary.success_rate > 0.7 { "#eab308" }
                            else { "#ef4444" };
                        rsx! {
                            div {
                                style: "background: #1a1a2e; border: 1px solid #333; border-radius: 8px; padding: 16px 20px; min-width: 150px; flex: 1;",
                                div { style: "font-size: 28px; font-weight: bold; color: {rate_color}; margin-bottom: 2px;", "{rate_pct}%" }
                                div { style: "font-size: 11px; color: #888; text-transform: uppercase; letter-spacing: 0.5px;", "Success Rate" }
                            }
                        }
                    }

                    // Average duration
                    {
                        let dur = format_duration_ms(data.summary.avg_duration_ms);
                        rsx! {
                            div {
                                style: "background: #1a1a2e; border: 1px solid #333; border-radius: 8px; padding: 16px 20px; min-width: 150px; flex: 1;",
                                div { style: "font-size: 28px; font-weight: bold; color: #e0e0e0; margin-bottom: 2px;", "{dur}" }
                                div { style: "font-size: 11px; color: #888; text-transform: uppercase; letter-spacing: 0.5px;", "Avg Duration" }
                            }
                        }
                    }

                    // Most used tool
                    if !data.summary.most_used_tool.is_empty() {
                        div {
                            style: "background: #1a1a2e; border: 1px solid #333; border-radius: 8px; padding: 16px 20px; min-width: 150px; flex: 1;",
                            div {
                                style: "font-size: 18px; font-weight: bold; color: #e0e0e0; margin-bottom: 2px; font-family: monospace;",
                                "{data.summary.most_used_tool}"
                            }
                            div { style: "font-size: 11px; color: #888; text-transform: uppercase; letter-spacing: 0.5px;", "Most Used Tool" }
                            div {
                                style: "font-size: 11px; margin-top: 6px; color: #888;",
                                "{data.summary.most_used_count} calls"
                            }
                        }
                    }
                }

                // Frequency chart
                div {
                    style: "background: #1a1a2e; border: 1px solid #333; border-radius: 8px; padding: 16px;",
                    div { style: "font-size: 14px; font-weight: bold; color: #aaa; margin-bottom: 12px;", "Usage Frequency" }
                    ToolFrequencyView {
                        tools: data.tools.clone(),
                        on_click: EventHandler::new(move |name| on_select(name)),
                    }
                }

                // Success/failure chart
                div {
                    style: "background: #1a1a2e; border: 1px solid #333; border-radius: 8px; padding: 16px;",
                    div { style: "font-size: 14px; font-weight: bold; color: #aaa; margin-bottom: 12px;", "Success / Failure Rates" }
                    ToolResultsView {
                        tools: data.tools.clone(),
                        time_series: data.time_series.clone(),
                    }
                }

                // Duration distribution
                div {
                    style: "background: #1a1a2e; border: 1px solid #333; border-radius: 8px; padding: 16px;",
                    div { style: "font-size: 14px; font-weight: bold; color: #aaa; margin-bottom: 12px;", "Duration Distribution" }
                    ToolDurationView { tools: data.tools.clone() }
                }
            }
        }
    }
}
