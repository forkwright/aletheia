//! Operations view: tool call history and active tool monitoring.

use dioxus::prelude::*;

use crate::api::client::authenticated_client;
use crate::state::connection::ConnectionConfig;
use crate::state::fetch::FetchState;

#[derive(Debug, Clone, Default)]
struct ToolStats {
    total: u32,
    succeeded: u32,
    failed: u32,
    active: Vec<ActiveTool>,
    history: Vec<ToolHistoryEntry>,
}

#[derive(Debug, Clone)]
struct ActiveTool {
    name: String,
    id: String,
}

#[derive(Debug, Clone)]
struct ToolHistoryEntry {
    name: String,
    is_error: bool,
    duration_ms: u64,
}

#[derive(Debug, Clone, serde::Deserialize)]
struct OpsResponse {
    #[serde(default)]
    active_tools: Vec<OpsActiveTool>,
    #[serde(default)]
    tool_history: Vec<OpsToolHistory>,
    #[serde(default)]
    total_calls: u32,
    #[serde(default)]
    total_errors: u32,
}

#[derive(Debug, Clone, serde::Deserialize)]
struct OpsActiveTool {
    #[serde(default)]
    name: String,
    #[serde(default)]
    id: String,
}

#[derive(Debug, Clone, serde::Deserialize)]
struct OpsToolHistory {
    #[serde(default)]
    name: String,
    #[serde(default)]
    is_error: bool,
    #[serde(default)]
    duration_ms: u64,
}

const CONTAINER_STYLE: &str = "\
    display: flex; \
    flex-direction: column; \
    height: 100%; \
    gap: 16px;\
";

const CARDS_STYLE: &str = "\
    display: flex; \
    gap: 12px; \
    flex-wrap: wrap;\
";

const CARD_STYLE: &str = "\
    background: #1a1a2e; \
    border: 1px solid #333; \
    border-radius: 8px; \
    padding: 16px 20px; \
    min-width: 120px; \
    text-align: center;\
";

const CARD_VALUE: &str = "\
    font-size: 28px; \
    font-weight: bold; \
    color: #e0e0e0;\
";

const CARD_LABEL: &str = "\
    font-size: 12px; \
    color: #888; \
    margin-top: 4px;\
";

const SECTION_STYLE: &str = "\
    background: #1a1a2e; \
    border: 1px solid #333; \
    border-radius: 8px; \
    padding: 16px; \
    flex: 1; \
    overflow-y: auto;\
";

const SECTION_TITLE: &str = "\
    font-size: 14px; \
    font-weight: bold; \
    color: #aaa; \
    margin-bottom: 12px;\
";

const ENTRY_STYLE: &str = "\
    display: flex; \
    align-items: center; \
    gap: 8px; \
    padding: 6px 0; \
    border-bottom: 1px solid #222; \
    font-size: 13px;\
";

const ACTIVE_DOT: &str = "\
    width: 8px; \
    height: 8px; \
    border-radius: 50%; \
    background: #4a4aff;\
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

#[component]
pub(crate) fn Ops() -> Element {
    let config: Signal<ConnectionConfig> = use_context();
    let mut stats = use_signal(ToolStats::default);
    let mut fetch_state = use_signal(|| FetchState::<()>::Loading);

    let mut do_refresh = move || {
        let cfg = config.read().clone();
        fetch_state.set(FetchState::Loading);

        spawn(async move {
            let client = authenticated_client(&cfg);
            let url = format!("{}/api/v1/ops/tools", cfg.server_url.trim_end_matches('/'));

            match client.get(&url).send().await {
                Ok(resp) if resp.status().is_success() => match resp.json::<OpsResponse>().await {
                    Ok(data) => {
                        let succeeded = data.total_calls.saturating_sub(data.total_errors);
                        stats.set(ToolStats {
                            total: data.total_calls,
                            succeeded,
                            failed: data.total_errors,
                            active: data
                                .active_tools
                                .into_iter()
                                .map(|t| ActiveTool {
                                    name: t.name,
                                    id: t.id,
                                })
                                .collect(),
                            history: data
                                .tool_history
                                .into_iter()
                                .map(|t| ToolHistoryEntry {
                                    name: t.name,
                                    is_error: t.is_error,
                                    duration_ms: t.duration_ms,
                                })
                                .collect(),
                        });
                        fetch_state.set(FetchState::Loaded(()));
                    }
                    Err(e) => {
                        fetch_state.set(FetchState::Error(format!("parse error: {e}")));
                    }
                },
                Ok(resp) => {
                    let status = resp.status();
                    fetch_state.set(FetchState::Error(format!("server returned {status}")));
                }
                Err(e) => {
                    fetch_state.set(FetchState::Error(format!("connection error: {e}")));
                }
            }
        });
    };

    use_effect(move || {
        do_refresh();
    });

    let current_stats = stats.read();

    rsx! {
        div {
            style: "{CONTAINER_STYLE}",
            div {
                style: "display: flex; align-items: center; justify-content: space-between;",
                h2 { style: "font-size: 20px; margin: 0;", "Operations" }
                button {
                    style: "{REFRESH_BTN}",
                    onclick: move |_| do_refresh(),
                    "Refresh"
                }
            }

            if let FetchState::Error(err) = &*fetch_state.read() {
                div { style: "color: #ef4444; font-size: 13px;", "Error: {err}" }
            }

            div {
                style: "{CARDS_STYLE}",
                div {
                    style: "{CARD_STYLE}",
                    div { style: "{CARD_VALUE}", "{current_stats.total}" }
                    div { style: "{CARD_LABEL}", "Total Calls" }
                }
                div {
                    style: "{CARD_STYLE}",
                    div { style: "{CARD_VALUE} color: #22c55e;", "{current_stats.succeeded}" }
                    div { style: "{CARD_LABEL}", "Succeeded" }
                }
                div {
                    style: "{CARD_STYLE}",
                    div { style: "{CARD_VALUE} color: #ef4444;", "{current_stats.failed}" }
                    div { style: "{CARD_LABEL}", "Failed" }
                }
                div {
                    style: "{CARD_STYLE}",
                    div { style: "{CARD_VALUE} color: #4a4aff;",
                        "{current_stats.active.len()}"
                    }
                    div { style: "{CARD_LABEL}", "Active" }
                }
            }

            if !current_stats.active.is_empty() {
                div {
                    style: "{SECTION_STYLE}",
                    div { style: "{SECTION_TITLE}", "Active Tools" }
                    for tool in &current_stats.active {
                        div {
                            style: "{ENTRY_STYLE}",
                            span { style: "{ACTIVE_DOT}" }
                            span { style: "color: #e0e0e0;", "{tool.name}" }
                            span { style: "color: #555; font-size: 11px;", "{tool.id}" }
                        }
                    }
                }
            }

            div {
                style: "{SECTION_STYLE}",
                div { style: "{SECTION_TITLE}", "Tool History" }
                if current_stats.history.is_empty() {
                    div { style: "color: #555; font-size: 13px;", "No tool calls recorded" }
                }
                for (i , entry) in current_stats.history.iter().enumerate() {
                    div {
                        key: "{i}",
                        style: "{ENTRY_STYLE}",
                        span { style: "color: {status_color(entry.is_error)};",
                            if entry.is_error { "[x]" } else { "[v]" } // kanon:ignore RUST/indexing-slicing
                        }
                        span { style: "color: #e0e0e0; flex: 1;", "{entry.name}" }
                        span { style: "color: #666; font-size: 11px;",
                            "{entry.duration_ms}ms"
                        }
                    }
                }
            }
        }
    }
}

fn status_color(is_error: bool) -> &'static str {
    if is_error { "#ef4444" } else { "#22c55e" }
}
