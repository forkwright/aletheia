//! Operations dashboard: agent status, service health, toggle controls,
//! tool history, and credential management.

mod agents;
pub(crate) mod credentials;
mod health;
mod toggles;

use std::collections::HashMap;

use dioxus::prelude::*;

use crate::api::client::authenticated_client;
use crate::state::connection::ConnectionConfig;
use crate::state::events::EventState;
use crate::state::fetch::FetchState;
use crate::state::ops::{
    AgentCardData, AgentStatusStore, AgentToggle, CronJobInfo, DaemonTaskInfo, FeatureFlag,
    HealthTier, JobResult, ServiceHealthStore, TaskStatus, ToggleStore, ToolToggle, Trend,
    health_from_status,
};

use self::agents::AgentCards;
use self::health::ServiceHealthPanel;
use self::toggles::ToggleControlsPanel;

// -- Tab enum -----------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OpsTab {
    Dashboard,
    Tools,
    Credentials,
}

// -- API response types -------------------------------------------------------

#[derive(Debug, Clone, Default, serde::Deserialize)]
struct AgentEntry {
    #[serde(default)]
    id: String,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    emoji: Option<String>,
    #[serde(default)]
    enabled: Option<bool>,
    #[serde(default)]
    tools: Vec<ToolEntryResp>,
}

#[derive(Debug, Clone, Default, serde::Deserialize)]
struct ToolEntryResp {
    #[serde(default)]
    name: String,
    #[serde(default)]
    enabled: bool,
}

#[derive(Debug, Clone, Default, serde::Deserialize)]
struct HealthApiResponse {
    #[expect(dead_code, reason = "deserialized from API but not inspected here")]
    #[serde(default)]
    status: String,
    #[serde(default)]
    cron_jobs: Vec<CronJobEntry>,
    #[serde(default)]
    daemon_tasks: Vec<DaemonTaskEntry>,
    #[serde(default)]
    failure_count: u32,
    #[serde(default)]
    failure_trend: Option<String>,
}

#[derive(Debug, Clone, Default, serde::Deserialize)]
struct CronJobEntry {
    #[serde(default)]
    name: String,
    #[serde(default)]
    schedule: String,
    #[serde(default)]
    last_run: Option<String>,
    #[serde(default)]
    next_run: Option<String>,
    #[serde(default)]
    last_result: Option<String>,
}

#[derive(Debug, Clone, Default, serde::Deserialize)]
struct DaemonTaskEntry {
    #[serde(default)]
    name: String,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    uptime: Option<String>,
    #[serde(default)]
    restart_count: u32,
}

#[derive(Debug, Clone, Default, serde::Deserialize)]
struct ConfigResponse {
    #[serde(default)]
    feature_flags: Vec<FeatureFlagEntry>,
}

#[derive(Debug, Clone, Default, serde::Deserialize)]
struct FeatureFlagEntry {
    #[serde(default)]
    key: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    enabled: bool,
}

// -- Tool stats (preserved from original) -------------------------------------

#[derive(Debug, Clone, Default)]
struct ToolStats {
    total: u32,
    succeeded: u32,
    failed: u32,
    active: Vec<ActiveToolEntry>,
    history: Vec<ToolHistoryEntry>,
}

#[derive(Debug, Clone)]
struct ActiveToolEntry {
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

// -- Style constants ----------------------------------------------------------

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

const BOTTOM_ROW: &str = "\
    display: flex; \
    gap: 16px; \
    flex: 1; \
    min-height: 0;\
";

const AUTO_REFRESH_SECS: u64 = 30;

// -- Main component -----------------------------------------------------------

#[component]
pub(crate) fn Ops() -> Element {
    let config: Signal<ConnectionConfig> = use_context();
    let event_state: Signal<EventState> = use_context();
    let mut active_tab = use_signal(|| OpsTab::Dashboard);

    // -- Dashboard-specific state ---------------------------------------------
    let mut agent_store = use_signal(AgentStatusStore::new);
    let mut health_store = use_signal(ServiceHealthStore::new);
    let mut toggle_store = use_signal(ToggleStore::new);
    let mut dash_fetch = use_signal(|| FetchState::<()>::Loading);

    // -- Tools-tab state (preserved from original) ----------------------------
    let mut stats = use_signal(ToolStats::default);
    let mut tools_fetch = use_signal(|| FetchState::<()>::Loading);

    // -- Dashboard data fetch -------------------------------------------------
    let mut refresh_dashboard = move || {
        let cfg = config.read().clone();
        dash_fetch.set(FetchState::Loading);

        spawn(async move {
            let client = authenticated_client(&cfg);
            let base = cfg.server_url.trim_end_matches('/');

            let agents_url = format!("{base}/api/v1/nous");
            let health_url = format!("{base}/api/health");
            let config_url = format!("{base}/api/v1/config");

            let (agents_res, health_res, config_res) = tokio::join!(
                client.get(&agents_url).send(),
                client.get(&health_url).send(),
                client.get(&config_url).send(),
            );

            let agents_data: Vec<AgentEntry> = match agents_res {
                Ok(resp) if resp.status().is_success() => {
                    resp.json::<Vec<AgentEntry>>().await.unwrap_or_default()
                }
                Ok(resp) => {
                    dash_fetch.set(FetchState::Error(format!(
                        "agents endpoint returned {}",
                        resp.status()
                    )));
                    return;
                }
                Err(e) => {
                    dash_fetch.set(FetchState::Error(format!("connection error: {e}")));
                    return;
                }
            };

            let cards: Vec<AgentCardData> = agents_data
                .iter()
                .map(|a| AgentCardData {
                    id: a.id.as_str().into(),
                    name: a.name.clone().unwrap_or_else(|| a.id.clone()),
                    emoji: a.emoji.clone(),
                    health: HealthTier::Healthy,
                    model: a.model.clone().unwrap_or_else(|| "\u{2014}".to_string()),
                    active_turns: 0,
                    last_activity: None,
                    connected: true,
                })
                .collect();
            agent_store.write().load(cards);

            let agent_toggles: Vec<AgentToggle> = agents_data
                .iter()
                .map(|a| AgentToggle {
                    id: a.id.as_str().into(),
                    name: a.name.clone().unwrap_or_else(|| a.id.clone()),
                    enabled: a.enabled.unwrap_or(true),
                    pending: false,
                })
                .collect();

            let tool_toggles: Vec<ToolToggle> = agents_data
                .iter()
                .flat_map(|a| {
                    let aid: theatron_core::id::NousId = a.id.as_str().into();
                    a.tools.iter().map(move |t| ToolToggle {
                        agent_id: aid.clone(),
                        tool_name: t.name.clone(),
                        enabled: t.enabled,
                        pending: false,
                    })
                })
                .collect();

            // -- Health -------------------------------------------------------
            let health_data = match health_res {
                Ok(resp) if resp.status().is_success() => {
                    resp.json::<HealthApiResponse>().await.unwrap_or_default()
                }
                _ => HealthApiResponse::default(),
            };

            health_store.set(ServiceHealthStore {
                cron_jobs: health_data
                    .cron_jobs
                    .into_iter()
                    .map(|j| CronJobInfo {
                        name: j.name,
                        schedule: j.schedule,
                        last_run: j.last_run,
                        next_run: j.next_run,
                        last_result: match j.last_result.as_deref() {
                            Some("success") => JobResult::Success,
                            Some("failure" | "error") => JobResult::Failure,
                            _ => JobResult::Unknown,
                        },
                    })
                    .collect(),
                daemon_tasks: health_data
                    .daemon_tasks
                    .into_iter()
                    .map(|t| DaemonTaskInfo {
                        name: t.name,
                        status: match t.status.as_deref() {
                            Some("running") => TaskStatus::Running,
                            Some("stopped") => TaskStatus::Stopped,
                            Some("failed" | "error") => TaskStatus::Failed,
                            _ => TaskStatus::Running,
                        },
                        uptime: t.uptime,
                        restart_count: t.restart_count,
                    })
                    .collect(),
                failure_count: health_data.failure_count,
                failure_trend: match health_data.failure_trend.as_deref() {
                    Some("up") => Trend::Up,
                    Some("down") => Trend::Down,
                    _ => Trend::Stable,
                },
            });

            // -- Feature flags ------------------------------------------------
            let feature_flags: Vec<FeatureFlag> = match config_res {
                Ok(resp) if resp.status().is_success() => resp
                    .json::<ConfigResponse>()
                    .await
                    .map(|c| {
                        c.feature_flags
                            .into_iter()
                            .map(|f| FeatureFlag {
                                key: f.key,
                                description: f.description,
                                enabled: f.enabled,
                                pending: false,
                            })
                            .collect()
                    })
                    .unwrap_or_default(),
                _ => Vec::new(),
            };

            {
                let mut ts = toggle_store.write();
                ts.agent_toggles = agent_toggles;
                ts.tool_toggles = tool_toggles;
                ts.feature_flags = feature_flags;
            }

            dash_fetch.set(FetchState::Loaded(()));
        });
    };

    // -- Tools data fetch (preserved from original) ---------------------------
    let mut refresh_tools = move || {
        let cfg = config.read().clone();
        tools_fetch.set(FetchState::Loading);

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
                                .map(|t| ActiveToolEntry {
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
                        tools_fetch.set(FetchState::Loaded(()));
                    }
                    Err(e) => tools_fetch.set(FetchState::Error(format!("parse error: {e}"))),
                },
                Ok(resp) => {
                    let status = resp.status();
                    tools_fetch.set(FetchState::Error(format!("server returned {status}")));
                }
                Err(e) => {
                    tools_fetch.set(FetchState::Error(format!("connection error: {e}")));
                }
            }
        });
    };

    // -- Mount fetch ----------------------------------------------------------
    use_effect(move || {
        refresh_dashboard();
        refresh_tools();
    });

    // -- Auto-refresh for non-SSE data ----------------------------------------
    use_effect(move || {
        spawn(async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(AUTO_REFRESH_SECS)).await;
                refresh_dashboard();
            }
        });
    });

    // -- Wire SSE events into agent store -------------------------------------
    let events = event_state.read();
    {
        let mut store = agent_store.write();
        let mut turn_counts: HashMap<theatron_core::id::NousId, u32> = HashMap::new();
        for turn in &events.active_turns {
            *turn_counts.entry(turn.nous_id.clone()).or_default() += 1;
        }
        let ids: Vec<theatron_core::id::NousId> = store.order.clone();
        for id in &ids {
            store.set_active_turns(id, turn_counts.get(id).copied().unwrap_or(0));
        }
        for (id, status) in &events.agent_statuses {
            store.set_health(id, health_from_status(status));
        }
    }

    // -- Render ---------------------------------------------------------------
    let tab = *active_tab.read();
    let current_stats = stats.read();

    rsx! {
        div {
            style: "{CONTAINER_STYLE}",

            // -- Header with tabs ---------------------------------------------
            div {
                style: "display: flex; align-items: center; justify-content: space-between;",
                h2 { style: "font-size: 20px; margin: 0;", "Operations" }
                div {
                    style: "display: flex; align-items: center; gap: 8px;",
                    button {
                        style: if tab == OpsTab::Dashboard { TAB_ACTIVE } else { TAB_INACTIVE },
                        onclick: move |_| active_tab.set(OpsTab::Dashboard),
                        "Dashboard"
                    }
                    button {
                        style: if tab == OpsTab::Tools { TAB_ACTIVE } else { TAB_INACTIVE },
                        onclick: move |_| active_tab.set(OpsTab::Tools),
                        "Tools"
                    }
                    button {
                        style: if tab == OpsTab::Credentials { TAB_ACTIVE } else { TAB_INACTIVE },
                        onclick: move |_| active_tab.set(OpsTab::Credentials),
                        "Credentials"
                    }
                    button {
                        style: "{REFRESH_BTN}",
                        onclick: move |_| {
                            match tab {
                                OpsTab::Dashboard => refresh_dashboard(),
                                OpsTab::Tools => refresh_tools(),
                                OpsTab::Credentials => {},
                            }
                        },
                        "Refresh"
                    }
                }
            }

            // -- Tab content --------------------------------------------------
            match tab {
                OpsTab::Dashboard => rsx! {
                    if let FetchState::Error(err) = &*dash_fetch.read() {
                        div { style: "color: #ef4444; font-size: 13px;", "Error: {err}" }
                    }

                    AgentCards { store: agent_store }

                    div {
                        style: "{BOTTOM_ROW}",
                        ServiceHealthPanel { store: health_store }
                        ToggleControlsPanel { store: toggle_store, config }
                    }
                },

                OpsTab::Tools => rsx! {
                    if let FetchState::Error(err) = &*tools_fetch.read() {
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
                            {render_tool_history_row(i, entry)}
                        }
                    }
                },

                OpsTab::Credentials => rsx! {
                    credentials::CredentialsView {}
                },
            }
        }
    }
}

fn render_tool_history_row(i: usize, entry: &ToolHistoryEntry) -> Element {
    let color = if entry.is_error { "#ef4444" } else { "#22c55e" };
    let icon = if entry.is_error { "[x]" } else { "[v]" };
    let icon_style = format!("color: {color};");

    rsx! {
        div {
            key: "{i}",
            style: "{ENTRY_STYLE}",
            span { style: "{icon_style}", "{icon}" }
            span { style: "color: #e0e0e0; flex: 1;", "{entry.name}" }
            span { style: "color: #666; font-size: 11px;", "{entry.duration_ms}ms" }
        }
    }
}
