//! Meta-insights view: the system looking at itself.

mod assembly;
mod charts;
mod drilldown;
mod health;
mod knowledge;
mod performance;
mod quality;
mod reflection;

use dioxus::prelude::*;
use serde::de::DeserializeOwned;

use crate::api::client::authenticated_client;
use crate::state::connection::ConnectionConfig;
use crate::state::fetch::FetchState;
use crate::state::meta::{
    AgentPerformanceStore, KnowledgeGrowthStore, MemoryHealthStore, QualityStore,
    SystemReflectionStore,
};

pub(crate) use charts::{BarChart, LineChart};
pub(crate) use drilldown::{ChartCard, ChartKind};

use assembly::assemble_meta_data;
use drilldown::{ChartDrilldown, ExpandedChart};
use health::MemoryHealthSection;
use knowledge::KnowledgeGrowthSection;
use performance::AgentPerformanceSection;
use quality::ConversationQualitySection;
use reflection::SystemReflectionSection;

// ── Fetch response types ──

#[derive(Debug, Clone, Default, serde::Deserialize)]
struct HealthApiResponse {
    #[serde(default)]
    uptime_seconds: u64,
}

#[derive(Debug, Clone, Default, serde::Deserialize)]
struct TokenMetricsApiResponse {
    #[serde(default)]
    series: Vec<TokenBucketEntry>,
    #[serde(default)]
    agents: Vec<AgentTokenRowEntry>,
}

#[derive(Debug, Clone, Default, serde::Deserialize)]
struct TokenBucketEntry {
    #[serde(default)]
    input_tokens: u64,
    #[serde(default)]
    output_tokens: u64,
}

#[derive(Debug, Clone, Default, serde::Deserialize)]
struct AgentTokenRowEntry {
    #[serde(default)]
    session_count: u64,
}

#[derive(Debug, Clone, Default, serde::Deserialize)]
struct CostMetricsApiResponse {
    #[serde(default)]
    series: Vec<CostBucketEntry>,
}

#[derive(Debug, Clone, Default, serde::Deserialize)]
struct CostBucketEntry {
    #[serde(default)]
    cost_usd: f64,
}

#[derive(Debug, Clone, Default, serde::Deserialize)]
struct KnowledgeFactsResponse {
    #[serde(default)]
    facts: Vec<FactEntry>,
}

#[derive(Debug, Clone, Default, serde::Deserialize)]
struct FactEntry {
    #[serde(default)]
    confidence: f64,
    // NOTE: facts carry `recorded_at` (system recording time), not `updated_at`.
    #[serde(default)]
    recorded_at: String,
    #[serde(default)]
    is_forgotten: bool,
    #[serde(default)]
    last_accessed_at: String,
    #[serde(default)]
    stability_hours: f64,
    #[serde(default)]
    valid_to: String,
}

#[derive(Debug, Clone, Default, serde::Deserialize)]
struct EntityEntry {
    #[serde(default)]
    name: String,
    #[serde(default)]
    entity_type: String,
    #[serde(default)]
    relationship_count: u32,
    #[serde(default)]
    updated_at: String,
}

#[derive(Debug, Clone, Default, serde::Deserialize)]
struct TimelineEntry {
    #[serde(default)]
    date: String,
    #[serde(default)]
    count: u32,
}

type SessionEntry = skene::api::types::Session;

#[derive(Debug, Clone, Default, serde::Deserialize)]
struct AgentEntry {
    #[serde(default)]
    id: String,
    #[serde(default)]
    name: String,
}

#[derive(Debug, Clone, Default, serde::Deserialize)]
struct SessionsApiResponse {
    // NOTE: the paginated endpoint wraps the list as `items`.
    #[serde(default, alias = "items")]
    sessions: Vec<SessionEntry>,
}

#[derive(Debug, Clone, Default, serde::Deserialize)]
struct KnowledgeEntitiesResponse {
    #[serde(default)]
    entities: Vec<EntityEntry>,
}

#[derive(Debug, Clone, Default, serde::Deserialize)]
struct TimelineEventsResponse {
    #[serde(default)]
    events: Vec<TimelineEventApiEntry>,
}

#[derive(Debug, Clone, Default, serde::Deserialize)]
struct TimelineEventApiEntry {
    #[serde(default)]
    timestamp: String,
}

#[derive(Debug, Clone, Default, serde::Deserialize)]
struct AgentsApiResponse {
    #[serde(default, alias = "agents")]
    nous: Vec<AgentEntry>,
}

// ── New insights endpoint response types ──

#[derive(Debug, Clone, Default, serde::Deserialize)]
struct AgentPerformanceApiResponse {
    #[serde(default)]
    agents: Vec<AgentPerformanceEntry>,
    #[serde(default)]
    anomalies: Vec<AnomalyEntry>,
}

#[derive(Debug, Clone, Default, serde::Deserialize)]
struct AgentPerformanceEntry {
    #[serde(default)]
    agent_id: String,
    #[serde(default)]
    agent_name: String,
    #[serde(default)]
    avg_tokens_per_response: f64,
    #[serde(default)]
    tool_calls_per_session: f64,
    #[serde(default)]
    tool_success_rate: f64,
    #[serde(default)]
    distillation_frequency: f64,
    #[serde(default)]
    avg_context_before_distill: f64,
    #[serde(default)]
    messages_per_session: f64,
    #[serde(default)]
    sessions_per_day: f64,
    #[serde(default)]
    errors_per_session: f64,
    #[serde(default)]
    tokens_per_response_series: Vec<TimeSeriesPointEntry>,
}

#[derive(Debug, Clone, Default, serde::Deserialize)]
struct AnomalyEntry {
    #[expect(dead_code, reason = "deserialized for completeness")]
    #[serde(default)]
    agent_id: String,
    #[serde(default)]
    agent_name: String,
    #[serde(default)]
    metric_name: String,
    #[serde(default)]
    current_value: f64,
    #[serde(default)]
    baseline_mean: f64,
    #[serde(default)]
    deviation_pct: f64,
    #[serde(default)]
    direction: String,
}

#[derive(Debug, Clone, Default, serde::Deserialize)]
struct QualityMetricsApiResponse {
    #[serde(default)]
    series: QualitySeriesEntry,
}

#[derive(Debug, Clone, Default, serde::Deserialize)]
struct QualitySeriesEntry {
    #[serde(default)]
    avg_turn_length: Vec<TimeSeriesPointEntry>,
    #[serde(default)]
    response_to_question_ratio: Vec<TimeSeriesPointEntry>,
    #[serde(default)]
    tool_call_density: Vec<TimeSeriesPointEntry>,
    #[serde(default)]
    thinking_time_ratio: Vec<TimeSeriesPointEntry>,
}

#[derive(Debug, Clone, Default, serde::Deserialize)]
struct TimeSeriesPointEntry {
    #[serde(default)]
    date: String,
    #[serde(default)]
    value: f64,
}

#[derive(Debug, Clone, Default, serde::Deserialize)]
struct JournalEventEntry {
    #[serde(default)]
    timestamp: String,
    #[serde(default)]
    event_type: String,
    #[serde(default)]
    message: String,
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

// ── Styles ──

const CONTAINER_STYLE: &str = "\
    display: flex; \
    flex-direction: column; \
    height: 100%; \
    overflow-y: auto; \
    padding: 0 var(--space-1);\
";

const HEADER_STYLE: &str = "\
    display: flex; \
    align-items: center; \
    justify-content: space-between; \
    padding: 0 0 var(--space-3) 0; \
    position: sticky; \
    top: 0; \
    background: var(--bg); \
    z-index: 10;\
";

const REFRESH_BTN: &str = "\
    background: var(--bg-surface); \
    color: var(--text-primary); \
    border: 1px solid var(--input-border); \
    border-radius: var(--radius-md); \
    padding: var(--space-1) var(--space-3); \
    font-size: var(--text-xs); \
    cursor: pointer; \
    transition: background-color var(--transition-quick), \
                color var(--transition-quick), \
                border-color var(--transition-quick);\
";

const STATUS_STYLE: &str = "\
    display: flex; \
    align-items: center; \
    justify-content: center; \
    min-height: 200px; \
    color: var(--text-secondary); \
    font-size: var(--text-base);\
";

pub(crate) const SECTION_HEADER_STYLE: &str = "\
    display: flex; \
    align-items: center; \
    justify-content: space-between; \
    padding: var(--space-3) var(--space-4); \
    background: var(--bg-surface); \
    border: 1px solid var(--border); \
    border-radius: var(--radius-md); \
    cursor: pointer; \
    user-select: none; \
    margin-bottom: var(--space-1); \
    transition: background-color var(--transition-quick), \
                color var(--transition-quick), \
                border-color var(--transition-quick);\
";

pub(crate) const SECTION_TITLE_STYLE: &str = "\
    font-size: var(--text-md); \
    font-weight: var(--weight-semibold); \
    color: var(--text-primary);\
";

pub(crate) const SECTION_BODY_STYLE: &str = "\
    padding: var(--space-4); \
    background: var(--bg-surface-dim); \
    border: 1px solid var(--border); \
    border-top: none; \
    border-radius: 0 0 var(--radius-md) var(--radius-md); \
    margin-bottom: var(--space-3);\
";

pub(crate) const CARD_STYLE: &str = "\
    background: var(--bg-surface); \
    border: 1px solid var(--border); \
    border-radius: var(--radius-md); \
    padding: var(--space-4);\
";

pub(crate) const GRID_STYLE: &str = "\
    display: flex; \
    flex-wrap: wrap; \
    gap: var(--space-3);\
";

pub(crate) const CARD_VALUE: &str = "\
    font-size: var(--text-2xl); \
    font-weight: var(--weight-bold); \
    color: var(--text-primary);\
";

pub(crate) const CARD_LABEL: &str = "\
    font-size: var(--text-xs); \
    color: var(--text-secondary); \
    text-transform: uppercase; \
    letter-spacing: 0.5px; \
    margin-top: var(--space-1);\
";

pub(crate) const CARD_SUB: &str = "\
    font-size: var(--text-xs); \
    color: var(--text-muted); \
    margin-top: var(--space-2);\
";

pub(crate) const MUTED_TEXT: &str = "font-size: var(--text-xs); color: var(--text-muted);";

/// Theme-token palette for multi-series meta charts.
///
/// WHY: Every entry resolves through `[data-theme]` so series hues stay
/// legible and on-palette in both dark and light themes.
pub(crate) const META_SERIES_COLORS: &[&str] = &[
    "var(--natural)",
    "var(--aporia)",
    "var(--status-info)",
    "var(--aima)",
    "var(--thanatochromia)",
    "var(--status-warning)",
    "var(--status-success)",
    "var(--accent)",
];

const AUTO_REFRESH_INTERVAL_MS: u64 = 300_000;

// ── Component ──

#[component]
pub(crate) fn Meta() -> Element {
    let config: Signal<ConnectionConfig> = use_context();
    let mut fetch_state = use_signal(|| FetchState::<MetaData>::Loading);
    let mut expanded = use_signal(|| [true, true, true, true, true]);
    use_context_provider(|| Signal::new(Option::<ExpandedChart>::None));

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
        expanded.with_mut(|arr| {
            if let Some(section) = arr.get_mut(idx) {
                *section = !*section;
            }
        });
    };

    rsx! {
        div {
            style: "{CONTAINER_STYLE}",
            div {
                style: "{HEADER_STYLE}",
                h2 { style: "font-size: var(--text-xl); margin: 0;", "Meta-Insights" }
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
                    div { style: "{STATUS_STYLE} color: var(--status-error);", "Error: {err}" }
                },
                FetchState::Loaded(data) => {
                    let exp = *expanded.read();
                    rsx! {
                        AccordionSection {
                            title: "Agent Performance",
                            expanded: exp.first().copied().unwrap_or(false),
                            on_toggle: move |_| toggle_section(0),
                            AgentPerformanceSection { store: data.performance.clone() }
                        }
                        AccordionSection {
                            title: "Conversation Quality",
                            expanded: exp.get(1).copied().unwrap_or(false),
                            on_toggle: move |_| toggle_section(1),
                            ConversationQualitySection { store: data.quality.clone() }
                        }
                        AccordionSection {
                            title: "Knowledge Growth",
                            expanded: exp.get(2).copied().unwrap_or(false),
                            on_toggle: move |_| toggle_section(2),
                            KnowledgeGrowthSection { store: data.knowledge.clone() }
                        }
                        AccordionSection {
                            title: "Memory Health",
                            expanded: exp.get(3).copied().unwrap_or(false),
                            on_toggle: move |_| toggle_section(3),
                            MemoryHealthSection { store: data.health.clone() }
                        }
                        AccordionSection {
                            title: "System Self-Reflection",
                            expanded: exp.get(4).copied().unwrap_or(false),
                            on_toggle: move |_| toggle_section(4),
                            SystemReflectionSection { store: data.reflection.clone() }
                        }
                    }
                },
            }

            ChartDrilldown {}
        }
    }
}

// ── Accordion ──

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
                span { style: "color: var(--text-muted); font-size: var(--text-base);", "{arrow}" }
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

// ── Data fetch ──

async fn fetch_meta_data(cfg: &ConnectionConfig) -> FetchState<MetaData> {
    let client = authenticated_client(cfg);
    let base = cfg.server_url.trim_end_matches('/');

    let health_url = format!("{base}/api/health");
    // WHY: /metrics serves Prometheus text; system totals come from the
    // JSON token/cost insights endpoints instead.
    let tokens_url = format!("{base}/api/v1/metrics/tokens");
    let costs_url = format!("{base}/api/v1/metrics/costs");
    let facts_url = format!("{base}/api/v1/knowledge/facts?limit=1000&include_forgotten=true");
    let entities_url = format!("{base}/api/v1/knowledge/entities");
    let timeline_url = format!("{base}/api/v1/knowledge/timeline");
    let sessions_url = format!("{base}/api/v1/sessions");
    let agents_url = format!("{base}/api/v1/nous");
    let perf_url = format!("{base}/api/v1/metrics/agents");
    let quality_url = format!("{base}/api/v1/metrics/quality");
    let journal_url = format!("{base}/api/v1/journal");

    // WHY: Fetch all endpoints in parallel to minimize latency.
    let (
        health_res,
        tokens_res,
        costs_res,
        facts_res,
        entities_res,
        timeline_res,
        sessions_res,
        agents_res,
        perf_res,
        quality_res,
        journal_res,
    ) = tokio::join!(
        client.get(&health_url).send(),
        client.get(&tokens_url).send(),
        client.get(&costs_url).send(),
        client.get(&facts_url).send(),
        client.get(&entities_url).send(),
        client.get(&timeline_url).send(),
        client.get(&sessions_url).send(),
        client.get(&agents_url).send(),
        client.get(&perf_url).send(),
        client.get(&quality_url).send(),
        client.get(&journal_url).send(),
    );

    let health: HealthApiResponse =
        match crate::api::health::fetch_health_response(health_res).await {
            Ok(data) => HealthApiResponse {
                uptime_seconds: data.uptime_seconds,
            },
            Err(err) => {
                return FetchState::Error(err.to_string());
            }
        };

    let tokens: TokenMetricsApiResponse = match tokens_res {
        Ok(resp) if resp.status().is_success() => optional_json(resp, "token metrics").await,
        _ => TokenMetricsApiResponse::default(),
    };

    let costs: CostMetricsApiResponse = match costs_res {
        Ok(resp) if resp.status().is_success() => optional_json(resp, "cost metrics").await,
        _ => CostMetricsApiResponse::default(),
    };

    let facts: Vec<FactEntry> = match facts_res {
        Ok(resp) if resp.status().is_success() => {
            // WHY: API may return bare array or wrapped in { facts: [...] }.
            match optional_text(resp, "facts").await {
                Some(text) => parse_facts_response(&text),
                None => Vec::new(),
            }
        }
        _ => Vec::new(),
    };

    let entities: Vec<EntityEntry> = match entities_res {
        Ok(resp) if resp.status().is_success() => match optional_text(resp, "entities").await {
            Some(text) => parse_entities_response(&text),
            None => Vec::new(),
        },
        _ => Vec::new(),
    };

    let timeline: Vec<TimelineEntry> = match timeline_res {
        Ok(resp) if resp.status().is_success() => match optional_text(resp, "timeline").await {
            Some(text) => parse_timeline_response(&text),
            None => Vec::new(),
        },
        _ => Vec::new(),
    };

    let sessions: Vec<SessionEntry> = match sessions_res {
        Ok(resp) if resp.status().is_success() => match optional_text(resp, "sessions").await {
            Some(text) => parse_sessions_response(&text),
            None => Vec::new(),
        },
        _ => Vec::new(),
    };

    let agents: Vec<AgentEntry> = match agents_res {
        Ok(resp) if resp.status().is_success() => match optional_text(resp, "agents").await {
            Some(text) => parse_agents_response(&text),
            None => Vec::new(),
        },
        _ => Vec::new(),
    };

    let perf: AgentPerformanceApiResponse = match perf_res {
        Ok(resp) if resp.status().is_success() => optional_json(resp, "agent performance").await,
        _ => AgentPerformanceApiResponse::default(),
    };

    let quality: QualityMetricsApiResponse = match quality_res {
        Ok(resp) if resp.status().is_success() => optional_json(resp, "quality").await,
        _ => QualityMetricsApiResponse::default(),
    };

    let journal: Vec<JournalEventEntry> = match journal_res {
        Ok(resp) if resp.status().is_success() => optional_json(resp, "journal").await,
        _ => Vec::new(),
    };

    let data = assemble_meta_data(
        health, tokens, costs, facts, entities, timeline, sessions, agents, perf, quality, journal,
    );
    FetchState::Loaded(data)
}

async fn optional_json<T>(resp: reqwest::Response, endpoint: &'static str) -> T
where
    T: DeserializeOwned + Default,
{
    match resp.json().await {
        Ok(value) => value,
        Err(err) => {
            tracing::warn!(endpoint, error = %err, "failed to parse optional meta response");
            T::default()
        }
    }
}

async fn optional_text(resp: reqwest::Response, endpoint: &'static str) -> Option<String> {
    match resp.text().await {
        Ok(text) => Some(text),
        Err(err) => {
            tracing::warn!(endpoint, error = %err, "failed to read optional meta response");
            None
        }
    }
}

fn parse_facts_response(text: &str) -> Vec<FactEntry> {
    match serde_json::from_str::<Vec<FactEntry>>(text) {
        Ok(facts) => facts,
        Err(array_err) => match serde_json::from_str::<KnowledgeFactsResponse>(text) {
            Ok(response) => response.facts,
            Err(wrapped_err) => {
                tracing::warn!(
                    array_error = %array_err,
                    wrapped_error = %wrapped_err,
                    "failed to parse facts response"
                );
                Vec::new()
            }
        },
    }
}

fn parse_entities_response(text: &str) -> Vec<EntityEntry> {
    match serde_json::from_str::<Vec<EntityEntry>>(text) {
        Ok(entities) => entities,
        Err(array_err) => match serde_json::from_str::<KnowledgeEntitiesResponse>(text) {
            Ok(response) => response.entities,
            Err(wrapped_err) => {
                tracing::warn!(
                    array_error = %array_err,
                    wrapped_error = %wrapped_err,
                    "failed to parse entities response"
                );
                Vec::new()
            }
        },
    }
}

fn parse_timeline_response(text: &str) -> Vec<TimelineEntry> {
    // WHY: the endpoint returns per-fact events `{events: [...], total}`;
    // bucket timestamps into per-date counts client-side. A bare
    // `[{date, count}]` array is accepted as a pre-bucketed fallback.
    match serde_json::from_str::<TimelineEventsResponse>(text) {
        Ok(response) => bucket_timeline_events(&response.events),
        Err(wrapped_err) => match serde_json::from_str::<Vec<TimelineEntry>>(text) {
            Ok(timeline) => timeline,
            Err(array_err) => {
                tracing::warn!(
                    wrapped_error = %wrapped_err,
                    array_error = %array_err,
                    "failed to parse timeline response"
                );
                Vec::new()
            }
        },
    }
}

fn bucket_timeline_events(events: &[TimelineEventApiEntry]) -> Vec<TimelineEntry> {
    let mut counts: std::collections::BTreeMap<String, u32> = std::collections::BTreeMap::new();
    for event in events {
        if let Some(date) = event.timestamp.get(..10) {
            let bucket = counts.entry(date.to_string()).or_default();
            *bucket = bucket.saturating_add(1);
        }
    }
    counts
        .into_iter()
        .map(|(date, count)| TimelineEntry { date, count })
        .collect()
}

fn parse_sessions_response(text: &str) -> Vec<SessionEntry> {
    match serde_json::from_str::<Vec<SessionEntry>>(text) {
        Ok(sessions) => sessions,
        Err(array_err) => match serde_json::from_str::<SessionsApiResponse>(text) {
            Ok(response) => response.sessions,
            Err(wrapped_err) => {
                tracing::warn!(
                    array_error = %array_err,
                    wrapped_error = %wrapped_err,
                    "failed to parse sessions response"
                );
                Vec::new()
            }
        },
    }
}

fn parse_agents_response(text: &str) -> Vec<AgentEntry> {
    match serde_json::from_str::<Vec<AgentEntry>>(text) {
        Ok(agents) => agents,
        Err(array_err) => match serde_json::from_str::<AgentsApiResponse>(text) {
            Ok(response) => response.nous,
            Err(wrapped_err) => {
                tracing::warn!(
                    array_error = %array_err,
                    wrapped_error = %wrapped_err,
                    "failed to parse agents response"
                );
                Vec::new()
            }
        },
    }
}
