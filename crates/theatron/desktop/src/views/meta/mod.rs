//! Meta-insights view: the system looking at itself.

mod assembly;
mod charts;
mod health;
mod knowledge;
mod performance;
mod quality;
mod reflection;

use dioxus::prelude::*;

use crate::api::client::authenticated_client;
use crate::state::connection::ConnectionConfig;
use crate::state::fetch::FetchState;
use crate::state::meta::{
    AgentPerformanceStore, KnowledgeGrowthStore, MemoryHealthStore, QualityStore,
    SystemReflectionStore,
};

pub(crate) use charts::{BarChart, LineChart};

use assembly::assemble_meta_data;
use health::MemoryHealthSection;
use knowledge::KnowledgeGrowthSection;
use performance::AgentPerformanceSection;
use quality::ConversationQualitySection;
use reflection::SystemReflectionSection;

// -- Fetch response types -----------------------------------------------------

#[derive(Debug, Clone, Default, serde::Deserialize)]
struct HealthApiResponse {
    #[expect(dead_code, reason = "deserialized from API for future use")]
    #[serde(default)]
    status: String,
    #[serde(default)]
    uptime_seconds: u64,
    #[expect(dead_code, reason = "deserialized from API for future use")]
    #[serde(default)]
    version: String,
}

#[derive(Debug, Clone, Default, serde::Deserialize)]
struct MetricsApiResponse {
    #[serde(default)]
    total_sessions: u64,
    #[expect(dead_code, reason = "deserialized from API for future use")]
    #[serde(default)]
    active_sessions: u64,
    #[expect(dead_code, reason = "deserialized from API for future use")]
    #[serde(default)]
    total_turns: u64,
    #[serde(default)]
    total_input_tokens: u64,
    #[serde(default)]
    total_output_tokens: u64,
    #[serde(default)]
    total_cost_usd: f64,
}

#[derive(Debug, Clone, Default, serde::Deserialize)]
struct KnowledgeFactsResponse {
    #[serde(default)]
    facts: Vec<FactEntry>,
    #[expect(dead_code, reason = "deserialized from API for future pagination")]
    #[serde(default)]
    total_count: u64,
}

#[derive(Debug, Clone, Default, serde::Deserialize)]
struct FactEntry {
    #[expect(dead_code, reason = "deserialized; used when stale entity table is populated")]
    #[serde(default)]
    entity: String,
    #[expect(dead_code, reason = "deserialized; used when topic extraction is wired")]
    #[serde(default)]
    content: String,
    #[serde(default)]
    confidence: f64,
    #[expect(dead_code, reason = "deserialized; used when type-based filtering is added")]
    #[serde(default)]
    fact_type: String,
    #[expect(dead_code, reason = "deserialized; used when creation timeline is built")]
    #[serde(default)]
    created_at: String,
    #[serde(default)]
    updated_at: String,
}

#[derive(Debug, Clone, Default, serde::Deserialize)]
struct EntityEntry {
    #[expect(dead_code, reason = "deserialized; used when entity detail view is added")]
    #[serde(default)]
    name: String,
    #[serde(default)]
    entity_type: String,
    #[serde(default)]
    relationship_count: u32,
}

#[derive(Debug, Clone, Default, serde::Deserialize)]
struct TimelineEntry {
    #[serde(default)]
    date: String,
    #[serde(default)]
    count: u32,
}

#[derive(Debug, Clone, Default, serde::Deserialize)]
struct SessionEntry {
    #[expect(dead_code, reason = "deserialized; used when session detail linking is added")]
    #[serde(default)]
    id: String,
    #[serde(default)]
    nous_id: String,
    #[serde(default)]
    message_count: u32,
    #[expect(dead_code, reason = "deserialized; used when status filtering is added")]
    #[serde(default)]
    status: String,
    #[serde(default)]
    created_at: String,
    #[expect(dead_code, reason = "deserialized; used when last-updated sorting is added")]
    #[serde(default)]
    updated_at: String,
}

#[derive(Debug, Clone, Default, serde::Deserialize)]
struct AgentEntry {
    #[serde(default)]
    id: String,
    #[serde(default)]
    name: String,
}

/// Composite data fetched from multiple API endpoints.
#[derive(Debug, Clone)]
struct MetaData {
    performance: AgentPerformanceStore,
    quality: QualityStore,
    knowledge: KnowledgeGrowthStore,
    health: MemoryHealthStore,
    reflection: SystemReflectionStore,
}

// -- Styles -------------------------------------------------------------------

const CONTAINER_STYLE: &str = "\
    display: flex; \
    flex-direction: column; \
    height: 100%; \
    overflow-y: auto; \
    padding: 0 4px;\
";

const HEADER_STYLE: &str = "\
    display: flex; \
    align-items: center; \
    justify-content: space-between; \
    padding: 0 0 12px 0; \
    position: sticky; \
    top: 0; \
    background: #0f0f1a; \
    z-index: 10;\
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

const STATUS_STYLE: &str = "\
    display: flex; \
    align-items: center; \
    justify-content: center; \
    min-height: 200px; \
    color: #888; \
    font-size: 14px;\
";

pub(crate) const SECTION_HEADER_STYLE: &str = "\
    display: flex; \
    align-items: center; \
    justify-content: space-between; \
    padding: 12px 16px; \
    background: #1a1a2e; \
    border: 1px solid #333; \
    border-radius: 8px; \
    cursor: pointer; \
    user-select: none; \
    margin-bottom: 2px;\
";

pub(crate) const SECTION_TITLE_STYLE: &str = "\
    font-size: 16px; \
    font-weight: 600; \
    color: #e0e0e0;\
";

pub(crate) const SECTION_BODY_STYLE: &str = "\
    padding: 16px; \
    background: #12121e; \
    border: 1px solid #2a2a3a; \
    border-top: none; \
    border-radius: 0 0 8px 8px; \
    margin-bottom: 12px;\
";

pub(crate) const CARD_STYLE: &str = "\
    background: #1a1a2e; \
    border: 1px solid #333; \
    border-radius: 8px; \
    padding: 16px;\
";

pub(crate) const GRID_STYLE: &str = "\
    display: flex; \
    flex-wrap: wrap; \
    gap: 12px;\
";

pub(crate) const CARD_VALUE: &str = "\
    font-size: 28px; \
    font-weight: bold; \
    color: #e0e0e0;\
";

pub(crate) const CARD_LABEL: &str = "\
    font-size: 12px; \
    color: #888; \
    text-transform: uppercase; \
    letter-spacing: 0.5px; \
    margin-top: 4px;\
";

pub(crate) const CARD_SUB: &str = "\
    font-size: 11px; \
    color: #555; \
    margin-top: 6px;\
";

pub(crate) const MUTED_TEXT: &str = "font-size: 12px; color: #666;";

const AUTO_REFRESH_INTERVAL_MS: u64 = 300_000;

// -- Component ----------------------------------------------------------------

#[component]
pub(crate) fn Meta() -> Element {
    let config: Signal<ConnectionConfig> = use_context();
    let mut fetch_state = use_signal(|| FetchState::<MetaData>::Loading);
    let mut expanded = use_signal(|| [true, true, true, true, true]);

    let mut do_refresh = move || {
        let cfg = config.read().clone();
        fetch_state.set(FetchState::Loading);

        spawn(async move {
            let data = fetch_meta_data(&cfg).await;
            fetch_state.set(data);
        });
    };

    // NOTE: Initial fetch on mount.
    use_effect(move || {
        do_refresh();
    });

    // NOTE: Auto-refresh every 5 minutes.
    let _auto_refresh = use_coroutine(move |_rx: UnboundedReceiver<()>| {
        let cfg = config;
        async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_millis(AUTO_REFRESH_INTERVAL_MS))
                    .await;
                let cfg = cfg.read().clone();
                let data = fetch_meta_data(&cfg).await;
                fetch_state.set(data);
            }
        }
    });

    let mut toggle_section = move |idx: usize| {
        expanded.with_mut(|arr| arr[idx] = !arr[idx]);
    };

    rsx! {
        div {
            style: "{CONTAINER_STYLE}",
            div {
                style: "{HEADER_STYLE}",
                h2 { style: "font-size: 20px; margin: 0;", "Meta-Insights" }
                button {
                    style: "{REFRESH_BTN}",
                    onclick: move |_| do_refresh(),
                    "Refresh"
                }
            }

            match &*fetch_state.read() {
                FetchState::Loading => rsx! {
                    div { style: "{STATUS_STYLE}", "Loading meta-insights..." }
                },
                FetchState::Error(err) => rsx! {
                    div { style: "{STATUS_STYLE} color: #ef4444;", "Error: {err}" }
                },
                FetchState::Loaded(data) => {
                    let exp = *expanded.read();
                    rsx! {
                        AccordionSection {
                            title: "Agent Performance",
                            expanded: exp[0],
                            on_toggle: move |_| toggle_section(0),
                            AgentPerformanceSection { store: data.performance.clone() }
                        }
                        AccordionSection {
                            title: "Conversation Quality",
                            expanded: exp[1],
                            on_toggle: move |_| toggle_section(1),
                            ConversationQualitySection { store: data.quality.clone() }
                        }
                        AccordionSection {
                            title: "Knowledge Growth",
                            expanded: exp[2],
                            on_toggle: move |_| toggle_section(2),
                            KnowledgeGrowthSection { store: data.knowledge.clone() }
                        }
                        AccordionSection {
                            title: "Memory Health",
                            expanded: exp[3],
                            on_toggle: move |_| toggle_section(3),
                            MemoryHealthSection { store: data.health.clone() }
                        }
                        AccordionSection {
                            title: "System Self-Reflection",
                            expanded: exp[4],
                            on_toggle: move |_| toggle_section(4),
                            SystemReflectionSection { store: data.reflection.clone() }
                        }
                    }
                },
            }
        }
    }
}

// -- Accordion ----------------------------------------------------------------

#[component]
fn AccordionSection(
    title: &'static str,
    expanded: bool,
    on_toggle: EventHandler<MouseEvent>,
    children: Element,
) -> Element {
    let arrow = if expanded { "\u{25bc}" } else { "\u{25b6}" };

    rsx! {
        div {
            div {
                style: "{SECTION_HEADER_STYLE}",
                onclick: move |evt| on_toggle.call(evt),
                span { style: "{SECTION_TITLE_STYLE}", "{title}" }
                span { style: "color: #666; font-size: 14px;", "{arrow}" }
            }
            if expanded {
                div {
                    style: "{SECTION_BODY_STYLE}",
                    {children}
                }
            }
        }
    }
}

// -- Data fetch ---------------------------------------------------------------

async fn fetch_meta_data(cfg: &ConnectionConfig) -> FetchState<MetaData> {
    let client = authenticated_client(cfg);
    let base = cfg.server_url.trim_end_matches('/');

    let health_url = format!("{base}/api/health");
    let metrics_url = format!("{base}/metrics");
    let facts_url = format!("{base}/api/v1/knowledge/facts?limit=1000");
    let entities_url = format!("{base}/api/v1/knowledge/entities");
    let timeline_url = format!("{base}/api/v1/knowledge/timeline");
    let sessions_url = format!("{base}/api/v1/sessions");
    let agents_url = format!("{base}/api/v1/nous");

    // WHY: Fetch all endpoints in parallel to minimize latency.
    let (health_res, metrics_res, facts_res, entities_res, timeline_res, sessions_res, agents_res) =
        tokio::join!(
            client.get(&health_url).send(),
            client.get(&metrics_url).send(),
            client.get(&facts_url).send(),
            client.get(&entities_url).send(),
            client.get(&timeline_url).send(),
            client.get(&sessions_url).send(),
            client.get(&agents_url).send(),
        );

    let health: HealthApiResponse = match health_res {
        Ok(resp) if resp.status().is_success() => {
            resp.json().await.unwrap_or_default()
        }
        Ok(resp) => {
            return FetchState::Error(format!("health endpoint returned {}", resp.status()));
        }
        Err(e) => {
            return FetchState::Error(format!("connection error: {e}"));
        }
    };

    let metrics: MetricsApiResponse = match metrics_res {
        Ok(resp) if resp.status().is_success() => resp.json().await.unwrap_or_default(),
        _ => MetricsApiResponse::default(),
    };

    let facts: Vec<FactEntry> = match facts_res {
        Ok(resp) if resp.status().is_success() => {
            // WHY: API may return bare array or wrapped in { facts: [...] }.
            let text = resp.text().await.unwrap_or_default();
            serde_json::from_str::<Vec<FactEntry>>(&text)
                .or_else(|_| {
                    serde_json::from_str::<KnowledgeFactsResponse>(&text)
                        .map(|r| r.facts)
                })
                .unwrap_or_default()
        }
        _ => Vec::new(),
    };

    let entities: Vec<EntityEntry> = match entities_res {
        Ok(resp) if resp.status().is_success() => resp.json().await.unwrap_or_default(),
        _ => Vec::new(),
    };

    let timeline: Vec<TimelineEntry> = match timeline_res {
        Ok(resp) if resp.status().is_success() => resp.json().await.unwrap_or_default(),
        _ => Vec::new(),
    };

    let sessions: Vec<SessionEntry> = match sessions_res {
        Ok(resp) if resp.status().is_success() => {
            let text = resp.text().await.unwrap_or_default();
            serde_json::from_str::<Vec<SessionEntry>>(&text)
                .or_else(|_| {
                    #[derive(serde::Deserialize)]
                    struct Wrapped {
                        #[serde(default)]
                        sessions: Vec<SessionEntry>,
                    }
                    serde_json::from_str::<Wrapped>(&text).map(|w| w.sessions)
                })
                .unwrap_or_default()
        }
        _ => Vec::new(),
    };

    let agents: Vec<AgentEntry> = match agents_res {
        Ok(resp) if resp.status().is_success() => {
            let text = resp.text().await.unwrap_or_default();
            serde_json::from_str::<Vec<AgentEntry>>(&text)
                .or_else(|_| {
                    #[derive(serde::Deserialize)]
                    struct Wrapped {
                        #[serde(default, alias = "agents")]
                        nous: Vec<AgentEntry>,
                    }
                    serde_json::from_str::<Wrapped>(&text).map(|w| w.nous)
                })
                .unwrap_or_default()
        }
        _ => Vec::new(),
    };

    let data = assemble_meta_data(health, metrics, facts, entities, timeline, sessions, agents);
    FetchState::Loaded(data)
}
