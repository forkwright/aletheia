//! Operations dashboard: agent status, service health, toggle controls,
//! tool catalog, live invocation state, and credential management.

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
    AgentCardData, AgentStatusStore, AgentToggle, FeatureFlag, HealthTier, ServiceHealthStore,
    ToggleApplyState, ToggleStore, ToolToggle, health_from_status,
};

use self::agents::AgentCards;
use self::health::ServiceHealthPanel;
use self::toggles::ToggleControlsPanel;

// ── Tab enum ──

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OpsTab {
    Dashboard,
    Tools,
    Credentials,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OpsRefreshTarget {
    Dashboard,
    Tools,
    Credentials,
}

fn refresh_target_for_tab(tab: OpsTab) -> OpsRefreshTarget {
    match tab {
        OpsTab::Dashboard => OpsRefreshTarget::Dashboard,
        OpsTab::Tools => OpsRefreshTarget::Tools,
        OpsTab::Credentials => OpsRefreshTarget::Credentials,
    }
}

// ── API response types ──

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
    status: Option<String>,
    #[serde(default)]
    tools: Vec<ToolEntryResp>,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(untagged)]
enum AgentListResponse {
    Wrapped {
        #[serde(default)]
        nous: Vec<AgentEntry>,
    },
    Bare(Vec<AgentEntry>),
}

impl AgentListResponse {
    fn into_agents(self) -> Vec<AgentEntry> {
        match self {
            Self::Wrapped { nous } => nous,
            Self::Bare(agents) => agents,
        }
    }
}

#[derive(Debug, Clone, Default, serde::Deserialize)]
struct ToolEntryResp {
    #[serde(default)]
    name: String,
    #[serde(default)]
    enabled: bool,
}

#[derive(Debug, Clone, Default, serde::Deserialize)]
struct ConfigResponse {
    #[serde(default)]
    feature_flags: Vec<FeatureFlagEntry>,
}

#[derive(Debug, Clone, Default, serde::Deserialize)]
struct FeatureFlagEntry {
    #[serde(default)]
    key: String, // kanon:ignore RUST/plain-string-secret -- feature flag identifier, not credential material
    #[serde(default)]
    description: String,
    #[serde(default)]
    enabled: bool,
}

// ── Tool stats ──

#[derive(Debug, Clone, Default)]
struct ToolStats {
    total: u64,
    succeeded: u64,
    failed: u64,
    catalog: Vec<ToolCatalogEntry>,
    live_invocations: Vec<LiveInvocationEntry>,
    history_unavailable: bool,
}

#[derive(Debug, Clone)]
struct ToolCatalogEntry {
    name: String,
    id: String,
    description: String,
}

#[derive(Debug, Clone)]
struct LiveInvocationEntry {
    id: u64,
    tool_name: String,
    elapsed_ms: u64,
}

#[derive(Debug, Clone, serde::Deserialize)]
struct OpsResponse {
    #[serde(default)]
    catalog: Vec<OpsCatalogTool>,
    #[serde(default)]
    live_invocations: Vec<OpsLiveInvocation>,
    #[serde(default)]
    total_calls: u64,
    #[serde(default)]
    total_errors: u64,
    #[serde(default)]
    history_unavailable: bool,
}

#[derive(Debug, Clone, serde::Deserialize)]
struct OpsCatalogTool {
    #[serde(default)]
    name: String,
    #[serde(default)]
    id: String,
    #[serde(default)]
    description: String,
}

#[derive(Debug, Clone, serde::Deserialize)]
struct OpsLiveInvocation {
    #[serde(default)]
    id: u64,
    #[serde(default)]
    tool_name: String,
    #[serde(default)]
    elapsed_ms: u64,
}

// ── Style constants ──

const CONTAINER_STYLE: &str = "\
    display: flex; \
    flex-direction: column; \
    height: 100%; \
    gap: var(--space-4);\
";

const CARDS_STYLE: &str = "\
    display: flex; \
    gap: var(--space-3); \
    flex-wrap: wrap;\
";

const CARD_STYLE: &str = "\
    background: var(--bg-surface); \
    border: 1px solid var(--border); \
    border-radius: var(--radius-md); \
    padding: var(--space-4) var(--space-5); \
    min-width: 120px; \
    text-align: center;\
";

const CARD_VALUE: &str = "\
    font-size: var(--text-2xl); \
    font-weight: var(--weight-bold); \
    color: var(--text-primary);\
";

const CARD_LABEL: &str = "\
    font-size: var(--text-xs); \
    color: var(--text-secondary); \
    margin-top: var(--space-1);\
";

const SECTION_STYLE: &str = "\
    background: var(--bg-surface); \
    border: 1px solid var(--border); \
    border-radius: var(--radius-md); \
    padding: var(--space-4); \
    flex: 1; \
    overflow-y: auto;\
";

const SECTION_TITLE: &str = "\
    font-size: var(--text-base); \
    font-weight: var(--weight-bold); \
    color: var(--text-secondary); \
    margin-bottom: var(--space-3);\
";

const ENTRY_STYLE: &str = "\
    display: flex; \
    align-items: center; \
    gap: var(--space-2); \
    padding: var(--space-2) 0; \
    border-bottom: 1px solid var(--border-separator); \
    font-size: var(--text-sm);\
";

const ACTIVE_DOT: &str = "\
    width: 8px; \
    height: 8px; \
    border-radius: 50%; \
    background: var(--accent);\
";

const REFRESH_BTN: &str = "\
    background: var(--border); \
    color: var(--text-primary); \
    border: 1px solid var(--border); \
    border-radius: var(--radius-md); \
    padding: var(--space-1) var(--space-3); \
    font-size: var(--text-xs); \
    cursor: pointer;\
    transition: background-color var(--transition-quick), \
                color var(--transition-quick), \
                border-color var(--transition-quick);\
";

const TAB_ACTIVE: &str = "\
    background: var(--border); \
    color: var(--text-primary); \
    border: 1px solid var(--accent); \
    border-radius: var(--radius-md); \
    padding: var(--space-1) var(--space-4); \
    font-size: var(--text-sm); \
    cursor: pointer;\
    transition: background-color var(--transition-quick), \
                color var(--transition-quick), \
                border-color var(--transition-quick);\
";

const TAB_INACTIVE: &str = "\
    background: transparent; \
    color: var(--text-secondary); \
    border: 1px solid var(--border); \
    border-radius: var(--radius-md); \
    padding: var(--space-1) var(--space-4); \
    font-size: var(--text-sm); \
    cursor: pointer;\
    transition: background-color var(--transition-quick), \
                color var(--transition-quick), \
                border-color var(--transition-quick);\
";

const BOTTOM_ROW: &str = "\
    display: flex; \
    gap: var(--space-4); \
    flex: 1; \
    min-height: 0;\
";

const AUTO_REFRESH_SECS: u64 = 30;

// ── Main component ──

#[component]
pub(crate) fn Ops() -> Element {
    let config: Signal<ConnectionConfig> = use_context();
    let event_state: Signal<EventState> = use_context();
    let mut active_tab = use_signal(|| OpsTab::Dashboard);

    // ── Dashboard-specific state ──
    let mut agent_store = use_signal(AgentStatusStore::new);
    let mut health_store = use_signal(ServiceHealthStore::new);
    let mut toggle_store = use_signal(ToggleStore::new);
    let mut dash_fetch = use_signal(|| FetchState::<()>::Loading);

    // ── Tools-tab state ──
    let mut stats = use_signal(ToolStats::default);
    let mut tools_fetch = use_signal(|| FetchState::<()>::Loading);
    let mut credentials_refresh = use_signal(|| 0u32);

    // ── Dashboard data fetch ──
    let mut refresh_dashboard = move || {
        let cfg = config.read().clone();
        dash_fetch.set(FetchState::Loading);

        spawn(async move {
            let client = authenticated_client(&cfg);
            let base = cfg.server_url.trim_end_matches('/');

            let agents_url = format!("{base}/api/v1/nous");
            let health_url = format!("{base}/api/v1/system/health");
            let config_url = format!("{base}/api/v1/config");

            let (agents_res, health_res, config_res) = tokio::join!(
                client.get(&agents_url).send(),
                client.get(&health_url).send(),
                client.get(&config_url).send(),
            );

            let agents_data: Vec<AgentEntry> = match agents_res {
                Ok(resp) if resp.status().is_success() => {
                    match resp.json::<AgentListResponse>().await {
                        Ok(data) => data.into_agents(),
                        Err(err) => {
                            tracing::warn!(error = %err, "failed to parse ops agent response");
                            Vec::new()
                        }
                    }
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
                    model: a.model.clone().unwrap_or_else(|| "-".to_string()),
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
                    apply_state: ToggleApplyState::Synced,
                    live_status: a.status.clone(),
                })
                .collect();

            let tool_toggles: Vec<ToolToggle> = agents_data
                .iter()
                .flat_map(|a| {
                    let aid: skene::id::NousId = a.id.as_str().into();
                    a.tools.iter().map(move |t| ToolToggle {
                        agent_id: aid.clone(),
                        tool_name: t.name.clone(),
                        enabled: t.enabled,
                        pending: false,
                        apply_state: ToggleApplyState::Synced,
                    })
                })
                .collect();

            // Health
            // WHY: accept both 2xx and 503 responses because the health endpoint
            // returns a JSON body even when the backend is unhealthy. Parse failures
            // and non-2xx/unparseable responses are stored as reachability errors
            // so the UI distinguishes server reachability from backend health.
            let health_store_data =
                match crate::api::health::fetch_health_response(health_res).await {
                    Ok(data) => ServiceHealthStore::from_response(data),
                    Err(err) => ServiceHealthStore::unreachable(err.to_string()),
                };

            health_store.set(health_store_data);

            // ── Feature flags ──
            let feature_flags: Vec<FeatureFlag> = match config_res {
                Ok(resp) if resp.status().is_success() => {
                    match resp.json::<ConfigResponse>().await {
                        Ok(c) => c
                            .feature_flags
                            .into_iter()
                            .map(|f| FeatureFlag {
                                key: f.key,
                                description: f.description,
                                enabled: f.enabled,
                                pending: false,
                                error: None,
                            })
                            .collect(),
                        Err(err) => {
                            tracing::warn!(error = %err, "failed to parse ops config response");
                            Vec::new()
                        }
                    }
                }
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

    // ── Tools data fetch ──
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
                            catalog: data
                                .catalog
                                .into_iter()
                                .map(|t| ToolCatalogEntry {
                                    name: t.name,
                                    id: t.id,
                                    description: t.description,
                                })
                                .collect(),
                            live_invocations: data
                                .live_invocations
                                .into_iter()
                                .map(|i| LiveInvocationEntry {
                                    id: i.id,
                                    tool_name: i.tool_name,
                                    elapsed_ms: i.elapsed_ms,
                                })
                                .collect(),
                            history_unavailable: data.history_unavailable,
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

    // ── Mount fetch ──
    use_effect(move || {
        refresh_dashboard();
        refresh_tools();
    });

    // ── Auto-refresh for non-SSE data ──
    // WHY: use_future is cancelled on unmount; use_effect+spawn leaks a loop.
    use_future(move || async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(AUTO_REFRESH_SECS)).await;
            refresh_dashboard();
        }
    });

    // ── Wire SSE events into agent store ──
    // WHY: Writing to a Signal inside the render body causes a panic in Dioxus;
    // use_effect defers the write to after the render pass completes.
    use_effect(move || {
        let events = event_state.read();
        let mut store = agent_store.write();
        let mut turn_counts: HashMap<skene::id::NousId, u32> = HashMap::new();
        for turn in &events.active_turns {
            *turn_counts.entry(turn.nous_id.clone()).or_default() += 1;
        }
        let ids: Vec<skene::id::NousId> = store.order.clone();
        for id in &ids {
            store.set_active_turns(id, turn_counts.get(id).copied().unwrap_or(0));
        }
        for (id, status) in &events.agent_statuses {
            store.set_health(id, health_from_status(status));
        }
    });

    // ── Render ──
    let tab = *active_tab.read();
    let current_stats = stats.read();

    rsx! {
        div {
            style: "{CONTAINER_STYLE}",

            // ── Header with tabs ──
            div {
                style: "display: flex; align-items: center; justify-content: space-between;",
                h2 { style: "font-size: var(--text-xl); margin: 0;", "Operations" }
                div {
                    style: "display: flex; align-items: center; gap: var(--space-2);",
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
                            match refresh_target_for_tab(tab) {
                                OpsRefreshTarget::Dashboard => refresh_dashboard(),
                                OpsRefreshTarget::Tools => refresh_tools(),
                                OpsRefreshTarget::Credentials => {
                                    credentials_refresh.set(credentials_refresh() + 1);
                                },
                            }
                        },
                        "Refresh"
                    }
                }
            }

            // ── Tab content ──
            match tab {
                OpsTab::Dashboard => rsx! {
                    if let FetchState::Loading = &*dash_fetch.read() {
                        div { style: "display: flex; align-items: center; justify-content: center; height: 200px; color: var(--text-muted); font-size: var(--text-sm);", "Loading\u{2026}" }
                    }
                    if let FetchState::Error(err) = &*dash_fetch.read() {
                        div { style: "color: var(--status-error); font-size: var(--text-sm);", "Error: {err}" }
                    }

                    AgentCards { store: agent_store }

                    div {
                        style: "{BOTTOM_ROW}",
                        ServiceHealthPanel { store: health_store }
                        ToggleControlsPanel { store: toggle_store, config }
                    }
                },

                OpsTab::Tools => rsx! {
                    if let FetchState::Loading = &*tools_fetch.read() {
                        div { style: "display: flex; align-items: center; justify-content: center; height: 200px; color: var(--text-muted); font-size: var(--text-sm);", "Loading\u{2026}" }
                    }
                    if let FetchState::Error(err) = &*tools_fetch.read() {
                        div { style: "color: var(--status-error); font-size: var(--text-sm);", "Error: {err}" }
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
                            div { style: "{CARD_VALUE} color: var(--status-success);", "{current_stats.succeeded}" }
                            div { style: "{CARD_LABEL}", "Succeeded" }
                        }
                        div {
                            style: "{CARD_STYLE}",
                            div { style: "{CARD_VALUE} color: var(--status-error);", "{current_stats.failed}" }
                            div { style: "{CARD_LABEL}", "Failed" }
                        }
                        div {
                            style: "{CARD_STYLE}",
                            div { style: "{CARD_VALUE} color: var(--accent);",
                                "{current_stats.catalog.len()}"
                            }
                            div { style: "{CARD_LABEL}", "Catalog" }
                        }
                        div {
                            style: "{CARD_STYLE}",
                            div { style: "{CARD_VALUE} color: var(--accent);",
                                "{current_stats.live_invocations.len()}"
                            }
                            div { style: "{CARD_LABEL}", "Live" }
                        }
                    }

                    div {
                        style: "{SECTION_STYLE}",
                        div { style: "{SECTION_TITLE}", "Tool Catalog" }
                        if current_stats.catalog.is_empty() {
                            div { style: "color: var(--text-muted); font-size: var(--text-sm);", "No tools registered" }
                        }
                        for tool in &current_stats.catalog {
                            div {
                                style: "{ENTRY_STYLE}",
                                span { style: "{ACTIVE_DOT}" }
                                span { style: "color: var(--text-primary);", "{tool.name}" }
                                span { style: "color: var(--text-muted); font-size: var(--text-xs);", "{tool.description}" }
                                span { style: "color: var(--text-muted); font-size: var(--text-xs);", "{tool.id}" }
                            }
                        }
                    }

                    div {
                        style: "{SECTION_STYLE}",
                        div { style: "{SECTION_TITLE}", "Live Invocations" }
                        if current_stats.live_invocations.is_empty() {
                            div { style: "color: var(--text-muted); font-size: var(--text-sm);", "No tool invocations running" }
                        }
                        for invocation in &current_stats.live_invocations {
                            div {
                                style: "{ENTRY_STYLE}",
                                span { style: "{ACTIVE_DOT}" }
                                span { style: "color: var(--text-primary); flex: 1;", "{invocation.tool_name}" }
                                span { style: "color: var(--text-muted); font-size: var(--text-xs);", "#{invocation.id}" }
                                span { style: "color: var(--text-muted); font-size: var(--text-xs);", "{invocation.elapsed_ms}ms" }
                            }
                        }
                    }

                    if current_stats.history_unavailable {
                        div {
                            style: "{SECTION_STYLE}",
                            div { style: "{SECTION_TITLE}", "Tool History" }
                            div { style: "color: var(--text-muted); font-size: var(--text-sm);", "Tool history unavailable: calls are not persisted yet" }
                        }
                    }
                },

                OpsTab::Credentials => rsx! {
                    credentials::CredentialsView {
                        refresh_token: *credentials_refresh.read()
                    }
                },
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refresh_target_includes_credentials_tab() {
        assert_eq!(
            refresh_target_for_tab(OpsTab::Dashboard),
            OpsRefreshTarget::Dashboard
        );
        assert_eq!(refresh_target_for_tab(OpsTab::Tools), OpsRefreshTarget::Tools);
        assert_eq!(
            refresh_target_for_tab(OpsTab::Credentials),
            OpsRefreshTarget::Credentials
        );
    }
}
