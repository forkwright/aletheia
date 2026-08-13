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
    AgentCapabilities, AgentCardData, AgentStatusStore, AgentToggle, FeatureFlag,
    ServiceHealthStore, ToggleApplyState, ToggleStore, ToolToggle, health_from_status,
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

/// Build one dashboard card from a `GET /api/v1/nous` list entry and its
/// (possibly absent) per-agent capability fetch result.
///
/// WHY(#4807): pulled out of the dashboard refresh closure so the
/// `health`/`connected` derivation from `entry.status` is unit-testable
/// without the surrounding HTTP fetch machinery. pylon's list endpoint
/// reports the actor's real lifecycle status, or `"unknown"` on a missing
/// handle, actor error, or timeout (see
/// `pylon::handlers::nous::live_status_label`). A missing client-side
/// status is normalized to that same `"unknown"` sentinel here so `health`
/// and `connected` derive from one fact instead of two independent,
/// driftable fallbacks.
fn build_agent_card(entry: &AgentEntry, capabilities: Option<AgentCapabilities>) -> AgentCardData {
    let live_status = entry.status.as_deref().unwrap_or("unknown");
    AgentCardData {
        id: entry.id.as_str().into(),
        name: entry.name.clone().unwrap_or_else(|| entry.id.clone()),
        emoji: entry.emoji.clone(),
        health: health_from_status(live_status),
        model: entry.model.clone().unwrap_or_else(|| "-".to_string()),
        active_turns: 0,
        last_activity: None,
        connected: live_status != "unknown",
        capabilities,
    }
}

/// Capability subset of the `NousStatus` body returned by
/// `GET /api/v1/nous/{id}`.
///
/// WARNING: `pylon::handlers::nous_dto::NousStatus` carries no
/// `rename_all`, so these field names must stay verbatim-identical to the
/// server's Rust field names. A rename on either side silently deserializes
/// every field to its `Default`, rendering a card full of zeroes rather than
/// failing.
#[derive(Debug, Clone, Default, serde::Deserialize)]
struct AgentDetailResp {
    #[serde(default)]
    context_window: u32,
    #[serde(default)]
    max_output_tokens: u32,
    #[serde(default)]
    thinking_enabled: bool,
    #[serde(default)]
    thinking_budget: u32,
    #[serde(default)]
    max_tool_iterations: u32,
}

impl From<AgentDetailResp> for AgentCapabilities {
    fn from(resp: AgentDetailResp) -> Self {
        Self {
            context_window: resp.context_window,
            max_output_tokens: resp.max_output_tokens,
            thinking_enabled: resp.thinking_enabled,
            thinking_budget: resp.thinking_budget,
            max_tool_iterations: resp.max_tool_iterations,
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

const LOADING_PANEL_STYLE: &str = "\
    display: flex; \
    align-items: center; \
    justify-content: center; \
    height: 200px; \
    color: var(--text-muted); \
    font-size: var(--text-sm);\
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

    // ── Dashboard data fetch ──
    let mut refresh_dashboard = move || {
        let cfg = config.read().clone();
        dash_fetch.set(FetchState::Loading);

        spawn(async move {
            let client = match authenticated_client(&cfg) {
                Ok(client) => client,
                Err(err) => {
                    dash_fetch.set(FetchState::Error(err.to_string()));
                    return;
                }
            };
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

            // WHY: capability limits live only on the per-agent detail
            // endpoint, so one request per agent is unavoidable. They are
            // issued concurrently rather than in sequence, and a failure on
            // any single agent degrades that card to `None` instead of
            // failing the whole dashboard refresh.
            let capabilities: Vec<Option<AgentCapabilities>> =
                futures_util::future::join_all(agents_data.iter().map(|a| {
                    let url = format!("{base}/api/v1/nous/{}", a.id);
                    let client = &client;
                    async move {
                        match client.get(&url).send().await {
                            Ok(resp) if resp.status().is_success() => {
                                match resp.json::<AgentDetailResp>().await {
                                    Ok(detail) => Some(detail.into()),
                                    Err(err) => {
                                        tracing::warn!(
                                            error = %err,
                                            "failed to parse nous detail response"
                                        );
                                        None
                                    }
                                }
                            }
                            Ok(resp) => {
                                tracing::warn!(
                                    status = %resp.status(),
                                    "nous detail endpoint returned non-success"
                                );
                                None
                            }
                            Err(err) => {
                                tracing::warn!(error = %err, "nous detail request failed");
                                None
                            }
                        }
                    }
                }))
                .await;

            let cards: Vec<AgentCardData> = agents_data
                .iter()
                .zip(capabilities)
                .map(|(a, capabilities)| build_agent_card(a, capabilities))
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
                    error: None,
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
                        error: None,
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
            let client = match authenticated_client(&cfg) {
                Ok(client) => client,
                Err(err) => {
                    tools_fetch.set(FetchState::Error(err.to_string()));
                    return;
                }
            };
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

            // ── Tab content ──
            match tab {
                OpsTab::Dashboard => rsx! {
                    match &*dash_fetch.read() {
                        FetchState::Loading => rsx! {
                            div { style: "{LOADING_PANEL_STYLE}", "Loading…" }
                        },
                        FetchState::Error(err) => rsx! {
                            div { style: "color: var(--status-error); font-size: var(--text-sm);", "Error: {err}" }
                        },
                        FetchState::Loaded(()) => rsx! {
                            AgentCards { store: agent_store }

                            div {
                                style: "{BOTTOM_ROW}",
                                ServiceHealthPanel { store: health_store }
                                ToggleControlsPanel { store: toggle_store, config }
                            }
                        },
                    }
                },

                OpsTab::Tools => rsx! {
                    match &*tools_fetch.read() {
                        FetchState::Loading => rsx! {
                            div { style: "{LOADING_PANEL_STYLE}", "Loading…" }
                        },
                        FetchState::Error(err) => rsx! {
                            div { style: "color: var(--status-error); font-size: var(--text-sm);", "Error: {err}" }
                        },
                        FetchState::Loaded(()) => rsx! {
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
                    }
                },

                OpsTab::Credentials => rsx! {
                    credentials::CredentialsView {}
                },
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::ops::HealthTier;

    /// WHY: this payload is the `pylon::handlers::nous_dto::NousStatus`
    /// wire shape. `NousStatus` carries no `rename_all`, so serde emits the
    /// Rust field names verbatim. Every field here is deliberately
    /// non-`Default`, so a rename on either side turns this green assertion
    /// red instead of silently producing a card of zeroes.
    const NOUS_STATUS_BODY: &str = r#"{
        "id": "scholiast",
        "model": "claude-opus-5",
        "status": "active",
        "context_window": 200000,
        "max_output_tokens": 64000,
        "thinking_enabled": true,
        "thinking_budget": 10000,
        "max_tool_iterations": 25
    }"#;

    #[test]
    fn agent_detail_resp_matches_server_field_names() {
        let detail: AgentDetailResp =
            serde_json::from_str(NOUS_STATUS_BODY).expect("NousStatus body must deserialize");

        assert_eq!(detail.context_window, 200_000, "context_window must map");
        assert_eq!(
            detail.max_output_tokens, 64_000,
            "max_output_tokens must map"
        );
        assert!(detail.thinking_enabled, "thinking_enabled must map");
        assert_eq!(detail.thinking_budget, 10_000, "thinking_budget must map");
        assert_eq!(
            detail.max_tool_iterations, 25,
            "max_tool_iterations must map"
        );
    }

    #[test]
    fn agent_detail_resp_tolerates_absent_capability_fields() {
        let detail: AgentDetailResp = serde_json::from_str(r#"{"id":"scholiast"}"#)
            .expect("a NousStatus without capability fields must still deserialize");

        assert_eq!(detail.context_window, 0, "absent field falls back to zero");
        assert!(!detail.thinking_enabled, "absent bool falls back to false");
    }

    #[test]
    fn agent_capabilities_conversion_preserves_every_field() {
        let detail: AgentDetailResp =
            serde_json::from_str(NOUS_STATUS_BODY).expect("NousStatus body must deserialize");
        let caps = AgentCapabilities::from(detail);

        assert_eq!(
            caps,
            AgentCapabilities {
                context_window: 200_000,
                max_output_tokens: 64_000,
                thinking_enabled: true,
                thinking_budget: 10_000,
                max_tool_iterations: 25,
            },
            "conversion must not drop or transpose a field"
        );
    }

    #[test]
    fn build_agent_card_maps_a_live_lifecycle_status_to_healthy_and_connected() {
        let entry = AgentEntry {
            id: "scholiast".to_owned(),
            status: Some("active".to_owned()),
            ..AgentEntry::default()
        };

        let card = build_agent_card(&entry, None);

        assert_eq!(card.health, HealthTier::Healthy);
        assert!(
            card.connected,
            "a reported lifecycle status means the actor answered"
        );
    }

    /// Regression test for #4807: the literal defect. pylon's `"unknown"`
    /// sentinel (missing handle, actor error, or timeout — see
    /// `live_status_label`) was discarded at card-build time in favor of a
    /// hardcoded `Healthy`/`connected: true` pair. This fails if that
    /// hardcoding is ever reintroduced.
    #[test]
    fn build_agent_card_never_renders_an_unknown_status_as_healthy() {
        let entry = AgentEntry {
            id: "ghost".to_owned(),
            status: Some("unknown".to_owned()),
            ..AgentEntry::default()
        };

        let card = build_agent_card(&entry, None);

        assert_eq!(card.health, HealthTier::Unknown);
        assert!(
            !card.connected,
            "an unknown status must not render as connected"
        );
    }

    /// A client that has not yet received any status for an agent (missing
    /// field, not an empty string) must be treated identically to pylon's
    /// explicit `"unknown"` — both cases mean "no verified live status" —
    /// so `health` and `connected` derive from one normalized value.
    #[test]
    fn build_agent_card_treats_a_missing_status_the_same_as_unknown() {
        let entry = AgentEntry {
            id: "legacy".to_owned(),
            status: None,
            ..AgentEntry::default()
        };

        let card = build_agent_card(&entry, None);

        assert_eq!(card.health, HealthTier::Unknown);
        assert!(!card.connected);
    }
}
