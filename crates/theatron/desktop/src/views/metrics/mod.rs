//! Metrics dashboard: server health, token usage, costs, and tool statistics.
//!
//! Tabs: Tokens (server health + token counts), Costs (cost breakdown), Tools (usage stats).

pub(crate) mod tool_detail;
pub(crate) mod tool_duration;
pub(crate) mod tool_frequency;
pub(crate) mod tool_results;
pub(crate) mod tools;

use dioxus::prelude::*;

use crate::api::client::authenticated_client;
use crate::state::connection::ConnectionConfig;
use crate::state::fetch::FetchState;
use crate::state::tool_metrics::DateRange;

// -- Tab enum -----------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MetricsTab {
    Tokens,
    Costs,
    Tools,
}

// -- API response types -------------------------------------------------------

#[derive(Debug, Clone, Default, serde::Deserialize)]
struct HealthResponse {
    #[serde(default)]
    status: String,
    #[serde(default)]
    uptime_seconds: u64,
    #[serde(default)]
    version: String,
}

#[derive(Debug, Clone, Default, serde::Deserialize)]
struct MetricsResponse {
    #[serde(default)]
    total_sessions: u64,
    #[serde(default)]
    active_sessions: u64,
    #[serde(default)]
    total_turns: u64,
    #[serde(default)]
    total_input_tokens: u64,
    #[serde(default)]
    total_output_tokens: u64,
    #[serde(default)]
    total_cost_usd: f64,
}

#[derive(Debug, Clone)]
struct DashboardData {
    health: HealthResponse,
    metrics: MetricsResponse,
}

// -- Style constants ----------------------------------------------------------

const CONTAINER_STYLE: &str = "\
    display: flex; \
    flex-direction: column; \
    height: 100%; \
    gap: 16px;\
";

const GRID_STYLE: &str = "\
    display: flex; \
    flex-wrap: wrap; \
    gap: 12px;\
";

const CARD_STYLE: &str = "\
    background: #1a1a2e; \
    border: 1px solid #333; \
    border-radius: 8px; \
    padding: 20px; \
    min-width: 160px; \
    flex: 1;\
";

const CARD_VALUE: &str = "\
    font-size: 32px; \
    font-weight: bold; \
    color: #e0e0e0; \
    margin-bottom: 4px;\
";

const CARD_LABEL: &str = "\
    font-size: 12px; \
    color: #888; \
    text-transform: uppercase; \
    letter-spacing: 0.5px;\
";

const CARD_SUB: &str = "\
    font-size: 11px; \
    color: #555; \
    margin-top: 8px;\
";

const STATUS_BADGE: &str = "\
    display: inline-block; \
    padding: 2px 8px; \
    border-radius: 4px; \
    font-size: 12px; \
    font-weight: bold;\
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

const TAB_ACTIVE: &str = "\
    background: #2a2a4a; \
    color: #e0e0e0; \
    border: 1px solid #4a4aff; \
    border-radius: 6px; \
    padding: 4px 14px; \
    font-size: 13px; \
    cursor: pointer;\
";

const TAB_INACTIVE: &str = "\
    background: transparent; \
    color: #888; \
    border: 1px solid #333; \
    border-radius: 6px; \
    padding: 4px 14px; \
    font-size: 13px; \
    cursor: pointer;\
";

// -- Main component -----------------------------------------------------------

#[component]
pub(crate) fn Metrics() -> Element {
    let config: Signal<ConnectionConfig> = use_context();
    let mut active_tab = use_signal(|| MetricsTab::Tokens);
    let mut fetch_state = use_signal(|| FetchState::<DashboardData>::Loading);

    let mut do_refresh = move || {
        let cfg = config.read().clone();
        fetch_state.set(FetchState::Loading);

        spawn(async move {
            let client = authenticated_client(&cfg);
            let base = cfg.server_url.trim_end_matches('/');

            let health_url = format!("{base}/api/health");
            let metrics_url = format!("{base}/metrics");

            let (health_result, metrics_result) = tokio::join!(
                client.get(&health_url).send(),
                client.get(&metrics_url).send()
            );

            let health = match health_result {
                Ok(resp) if resp.status().is_success() => {
                    resp.json::<HealthResponse>().await.unwrap_or_default()
                }
                Ok(resp) => {
                    fetch_state.set(FetchState::Error(format!(
                        "health endpoint returned {}",
                        resp.status()
                    )));
                    return;
                }
                Err(e) => {
                    fetch_state.set(FetchState::Error(format!("connection error: {e}")));
                    return;
                }
            };

            // WHY: /metrics may not exist on all pylon versions; use defaults.
            let metrics = match metrics_result {
                Ok(resp) if resp.status().is_success() => {
                    resp.json::<MetricsResponse>().await.unwrap_or_default()
                }
                _ => MetricsResponse::default(),
            };

            fetch_state.set(FetchState::Loaded(DashboardData { health, metrics }));
        });
    };

    use_effect(move || {
        do_refresh();
    });

    rsx! {
        div {
            style: "{CONTAINER_STYLE}",

            // Header with title, tabs, and refresh
            div {
                style: "display: flex; align-items: center; justify-content: space-between; flex-wrap: wrap; gap: 8px;",
                h2 { style: "font-size: 20px; margin: 0;", "Metrics" }

                div {
                    style: "display: flex; gap: 6px; align-items: center;",

                    button {
                        style: if *active_tab.read() == MetricsTab::Tokens { TAB_ACTIVE } else { TAB_INACTIVE },
                        onclick: move |_| active_tab.set(MetricsTab::Tokens),
                        "Tokens"
                    }
                    button {
                        style: if *active_tab.read() == MetricsTab::Costs { TAB_ACTIVE } else { TAB_INACTIVE },
                        onclick: move |_| active_tab.set(MetricsTab::Costs),
                        "Costs"
                    }
                    button {
                        style: if *active_tab.read() == MetricsTab::Tools { TAB_ACTIVE } else { TAB_INACTIVE },
                        onclick: move |_| active_tab.set(MetricsTab::Tools),
                        "Tools"
                    }

                    if *active_tab.read() != MetricsTab::Tools {
                        button {
                            style: "{REFRESH_BTN}",
                            onclick: move |_| do_refresh(),
                            "Refresh"
                        }
                    }
                }
            }

            match *active_tab.read() {
                MetricsTab::Tokens => rsx! { {render_tokens_tab(&fetch_state)} },
                MetricsTab::Costs => rsx! { {render_costs_tab(&fetch_state)} },
                MetricsTab::Tools => rsx! {
                    tools::ToolsOverview { date_range: DateRange::default() }
                },
            }
        }
    }
}

// -- Tokens tab ---------------------------------------------------------------

fn render_tokens_tab(fetch_state: &Signal<FetchState<DashboardData>>) -> Element {
    match &*fetch_state.read() {
        FetchState::Loading => rsx! {
            div {
                style: "display: flex; align-items: center; justify-content: center; flex: 1; color: #888;",
                "Loading metrics..."
            }
        },
        FetchState::Error(err) => rsx! {
            div {
                style: "display: flex; align-items: center; justify-content: center; flex: 1; color: #ef4444;",
                "Error: {err}"
            }
        },
        FetchState::Loaded(data) => rsx! {
            {render_tokens_content(data)}
        },
    }
}

fn render_tokens_content(data: &DashboardData) -> Element {
    let uptime = format_uptime(data.health.uptime_seconds);
    let status_color = if data.health.status == "ok" {
        "background: #1a3a1a; color: #22c55e;"
    } else {
        "background: #3a1a1a; color: #ef4444;"
    };

    rsx! {
        div {
            style: "{GRID_STYLE}",

            div {
                style: "{CARD_STYLE}",
                div {
                    style: "{STATUS_BADGE} {status_color}",
                    "{data.health.status}"
                }
                div { style: "{CARD_LABEL} margin-top: 8px;", "Server Status" }
                if !data.health.version.is_empty() {
                    div { style: "{CARD_SUB}", "v{data.health.version}" }
                }
            }

            div {
                style: "{CARD_STYLE}",
                div { style: "{CARD_VALUE}", "{uptime}" }
                div { style: "{CARD_LABEL}", "Uptime" }
            }

            div {
                style: "{CARD_STYLE}",
                div { style: "{CARD_VALUE}", "{data.metrics.total_sessions}" }
                div { style: "{CARD_LABEL}", "Total Sessions" }
                div { style: "{CARD_SUB}", "{data.metrics.active_sessions} active" }
            }

            div {
                style: "{CARD_STYLE}",
                div { style: "{CARD_VALUE}", "{data.metrics.total_turns}" }
                div { style: "{CARD_LABEL}", "Total Turns" }
            }
        }

        div {
            style: "{GRID_STYLE}",

            div {
                style: "{CARD_STYLE}",
                div { style: "{CARD_VALUE}", "{format_tokens(data.metrics.total_input_tokens)}" }
                div { style: "{CARD_LABEL}", "Input Tokens" }
            }

            div {
                style: "{CARD_STYLE}",
                div { style: "{CARD_VALUE}", "{format_tokens(data.metrics.total_output_tokens)}" }
                div { style: "{CARD_LABEL}", "Output Tokens" }
            }
        }
    }
}

// -- Costs tab ----------------------------------------------------------------

fn render_costs_tab(fetch_state: &Signal<FetchState<DashboardData>>) -> Element {
    match &*fetch_state.read() {
        FetchState::Loading => rsx! {
            div {
                style: "color: #888; padding: 8px;",
                "Loading cost data..."
            }
        },
        FetchState::Error(err) => rsx! {
            div {
                style: "color: #ef4444; padding: 8px;",
                "Error: {err}"
            }
        },
        FetchState::Loaded(data) => rsx! {
            div {
                style: "{GRID_STYLE}",

                div {
                    style: "{CARD_STYLE}",
                    div {
                        style: "{CARD_VALUE} color: #eab308;",
                        "{format_cost(data.metrics.total_cost_usd)}"
                    }
                    div { style: "{CARD_LABEL}", "Total Cost" }
                }

                div {
                    style: "{CARD_STYLE}",
                    div {
                        style: "{CARD_VALUE}",
                        "{format_cost_per_turn(data.metrics.total_cost_usd, data.metrics.total_turns)}"
                    }
                    div { style: "{CARD_LABEL}", "Cost / Turn" }
                }
            }
        },
    }
}

// -- Formatting helpers -------------------------------------------------------

fn format_uptime(seconds: u64) -> String {
    let days = seconds / 86400;
    let hours = (seconds % 86400) / 3600;
    let mins = (seconds % 3600) / 60;

    if days > 0 {
        format!("{days}d {hours}h")
    } else if hours > 0 {
        format!("{hours}h {mins}m")
    } else {
        format!("{mins}m")
    }
}

fn format_cost(value: f64) -> String {
    format!("${value:.2}")
}

fn format_cost_per_turn(total_cost: f64, total_turns: u64) -> String {
    if total_turns == 0 {
        return "—".to_string();
    }
    #[expect(
        clippy::cast_precision_loss,
        reason = "display-only: turn count fits in f64"
    )]
    let per_turn = total_cost / total_turns as f64;
    format!("${per_turn:.4}")
}

#[expect(
    clippy::cast_precision_loss,
    reason = "display-only: sub-token precision irrelevant"
)]
fn format_tokens(count: u64) -> String {
    const K: u64 = 1_000;
    const M: u64 = 1_000_000;

    if count >= M {
        format!("{:.1}M", count as f64 / M as f64) // kanon:ignore RUST/as-cast
    } else if count >= K {
        format!("{:.1}K", count as f64 / K as f64) // kanon:ignore RUST/as-cast
    } else {
        count.to_string()
    }
}
