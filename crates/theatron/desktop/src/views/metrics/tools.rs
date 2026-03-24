//! Tools tab: summary cards, date range selector, and sub-view host.

use dioxus::prelude::*;

use crate::api::client::authenticated_client;
use crate::state::connection::ConnectionConfig;
use crate::state::fetch::FetchState;
use crate::state::tool_metrics::{
    DateRange, ToolMetricsStore, ToolStatsResponse, format_delta, format_duration_ms, trend_arrow,
};

use super::tool_detail::ToolDetailView;
use super::tool_duration::ToolDurationView;
use super::tool_frequency::ToolFrequencyView;
use super::tool_results::ToolResultsView;

// -- Style constants ----------------------------------------------------------

const CARD_STYLE: &str = "\
    background: #1a1a2e; \
    border: 1px solid #333; \
    border-radius: 8px; \
    padding: 16px 20px; \
    min-width: 150px; \
    flex: 1;\
";

const CARD_VALUE: &str = "\
    font-size: 28px; \
    font-weight: bold; \
    color: #e0e0e0; \
    margin-bottom: 2px;\
";

const CARD_LABEL: &str = "\
    font-size: 11px; \
    color: #888; \
    text-transform: uppercase; \
    letter-spacing: 0.5px;\
";

const CARD_DELTA: &str = "\
    font-size: 11px; \
    margin-top: 6px;\
";

const RANGE_BTN_ACTIVE: &str = "\
    background: #2a2a4a; \
    color: #e0e0e0; \
    border: 1px solid #4a4aff; \
    border-radius: 4px; \
    padding: 2px 10px; \
    font-size: 12px; \
    cursor: pointer;\
";

const RANGE_BTN_INACTIVE: &str = "\
    background: transparent; \
    color: #666; \
    border: 1px solid #333; \
    border-radius: 4px; \
    padding: 2px 10px; \
    font-size: 12px; \
    cursor: pointer;\
";

const SECTION_STYLE: &str = "\
    background: #1a1a2e; \
    border: 1px solid #333; \
    border-radius: 8px; \
    padding: 16px;\
";

const SECTION_TITLE: &str = "\
    font-size: 14px; \
    font-weight: bold; \
    color: #aaa; \
    margin-bottom: 12px;\
";

const REFRESH_BTN: &str = "\
    background: #2a2a4a; \
    color: #e0e0e0; \
    border: 1px solid #444; \
    border-radius: 6px; \
    padding: 4px 12px; \
    font-size: 12px; \
    cursor: pointer;\
";

// -- Component ----------------------------------------------------------------

#[component]
pub(crate) fn ToolsOverview(date_range: DateRange) -> Element {
    let config: Signal<ConnectionConfig> = use_context();
    let mut store = use_signal(|| ToolMetricsStore {
        date_range,
        ..Default::default()
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
                for range in [DateRange::Last7Days, DateRange::Last30Days, DateRange::Last90Days] {
                    {
                        let is_active = store.read().date_range == range;
                        rsx! {
                            button {
                                key: "{range.label()}",
                                style: if is_active { RANGE_BTN_ACTIVE } else { RANGE_BTN_INACTIVE },
                                onclick: move |_| {
                                    store.write().date_range = range;
                                    do_fetch();
                                },
                                "{range.label()}"
                            }
                        }
                    }
                }
                button {
                    style: "{REFRESH_BTN}",
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
                {render_overview(store, move |tool_name| {
                    store.write().selected_tool = Some(tool_name);
                })}
            }
        }
    }
}

fn render_overview(
    store: Signal<ToolMetricsStore>,
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
                        style: "{CARD_STYLE}",
                        div { style: "{CARD_VALUE}", "{data.summary.total_invocations_week}" }
                        div { style: "{CARD_LABEL}", "Invocations (7d)" }
                        div {
                            style: "{CARD_DELTA} color: #888;",
                            "{format_delta(data.summary.delta_week)} vs prior"
                        }
                    }

                    // Overall success rate
                    {
                        let rate_pct = (data.summary.success_rate * 100.0) as u64;
                        let trend = trend_arrow(data.summary.success_rate, data.summary.success_rate_prev);
                        let rate_color = if data.summary.success_rate > 0.9 { "#22c55e" }
                            else if data.summary.success_rate > 0.7 { "#eab308" }
                            else { "#ef4444" };
                        rsx! {
                            div {
                                style: "{CARD_STYLE}",
                                div { style: "{CARD_VALUE} color: {rate_color};", "{rate_pct}%" }
                                div { style: "{CARD_LABEL}", "Success Rate" }
                                div { style: "{CARD_DELTA} color: #888;", "{trend} trend" }
                            }
                        }
                    }

                    // Average duration
                    {
                        let dur = format_duration_ms(data.summary.avg_duration_ms);
                        let trend = trend_arrow(
                            data.summary.avg_duration_ms as f64,
                            data.summary.avg_duration_prev_ms as f64,
                        );
                        rsx! {
                            div {
                                style: "{CARD_STYLE}",
                                div { style: "{CARD_VALUE}", "{dur}" }
                                div { style: "{CARD_LABEL}", "Avg Duration" }
                                div { style: "{CARD_DELTA} color: #888;", "{trend} trend" }
                            }
                        }
                    }

                    // Most used tool
                    if !data.summary.most_used_tool.is_empty() {
                        div {
                            style: "{CARD_STYLE}",
                            div {
                                style: "font-size: 18px; font-weight: bold; color: #e0e0e0; margin-bottom: 2px; font-family: monospace;",
                                "{data.summary.most_used_tool}"
                            }
                            div { style: "{CARD_LABEL}", "Most Used Tool" }
                            div {
                                style: "{CARD_DELTA} color: #888;",
                                "{data.summary.most_used_count} calls"
                            }
                        }
                    }
                }

                // Frequency chart
                div {
                    style: "{SECTION_STYLE}",
                    div { style: "{SECTION_TITLE}", "Usage Frequency" }
                    ToolFrequencyView {
                        tools: data.tools.clone(),
                        on_click: EventHandler::new(move |name| on_select(name)),
                    }
                }

                // Success/failure chart
                div {
                    style: "{SECTION_STYLE}",
                    div { style: "{SECTION_TITLE}", "Success / Failure Rates" }
                    ToolResultsView {
                        tools: data.tools.clone(),
                        time_series: data.time_series.clone(),
                    }
                }

                // Duration distribution
                div {
                    style: "{SECTION_STYLE}",
                    div { style: "{SECTION_TITLE}", "Duration Distribution" }
                    ToolDurationView { tools: data.tools.clone() }
                }
            }
        }
    }
}
