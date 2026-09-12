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

/// Server-computed memory-health metrics from `GET /api/v1/knowledge/health`.
///
/// WHY(#6823): pylon computes the same avg-confidence/orphan/staleness
/// inputs from the knowledge store directly (the source its Prometheus
/// gauges read). When this fetch succeeds, its snapshot replaces the
/// client-side recomputation in `assembly.rs`.
#[derive(Debug, Clone, Copy, Default, serde::Deserialize)]
struct MemoryHealthApiResponse {
    #[serde(default)]
    avg_confidence: f64,
    #[serde(default)]
    orphan_ratio: f64,
    #[serde(default)]
    staleness_ratio: f64,
    #[serde(default)]
    health_score: f64,
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
    #[serde(default)]
    nous_id: String,
    #[serde(default)]
    message_count: u32,
    #[serde(default)]
    created_at: String,
    #[serde(default)]
    updated_at: String,
}

impl SessionEntry {
    /// Best-available activity timestamp for day/hour bucketing.
    ///
    /// NOTE: the paginated session list carries only `updated_at`;
    /// `created_at` is preferred when a future shape provides it.
    fn activity_timestamp(&self) -> &str {
        if self.created_at.is_empty() {
            &self.updated_at
        } else {
            &self.created_at
        }
    }
}

#[derive(Debug, Clone, Default, serde::Deserialize)]
struct AgentEntry {
    #[serde(default)]
    id: String,
    #[serde(default)]
    name: String,
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

/// Local mirror of `skene::api::types::JournalResponse`.
///
/// WHY(#4565, ruling B / aletheia#7187): before this migration, this type had
/// its own hand-rolled `Deserialize` impl that parsed pylon's raw JSON body
/// directly -- rejecting a bare JSON sequence rather than silently accepting
/// one as an empty envelope (`#4486`). That parsing boundary now lives in
/// `skene::api::client::ApiClient::journal` / `skene::api::types::
/// JournalResponse` instead (its `events` field has no `#[serde(default)]`,
/// so the same bare-`[]` shape it used to special-case already fails to
/// deserialize there); this struct is populated purely via the `From` impl
/// below and never deserializes JSON on its own account.
#[derive(Debug, Clone, Default)]
struct JournalResponseEntry {
    events: Vec<JournalEventEntry>,
    /// Non-empty when the backend has no data source for a claimed metric
    /// (currently always `["journal"]`: pylon has no persistent event
    /// journal yet). Distinguishes genuinely-empty from unimplemented.
    data_unavailable: Vec<UnavailableMetricEntry>,
}

#[derive(Debug, Clone, Default, serde::Deserialize)]
struct UnavailableMetricEntry {
    #[serde(default)]
    metric: String,
    #[serde(default)]
    reason: String,
}

// ── Conversions from skene's typed insights DTOs ──
//
// WHY(#4565, ruling B / aletheia#7187): every source `fetch_meta_data` below
// fetches now goes through `skene::api::client::ApiClient` instead of a
// hand-built `/api/v1/...` literal on a raw client. These wire shapes mirror
// pylon's DTOs exactly on both sides (skene's typed DTOs and this module's
// `*Entry`/`*ApiResponse` types), so the conversions are a plain field-by-
// field remap -- kept local rather than switching every downstream
// `assemble_meta_data` consumer onto skene's types directly.

impl From<skene::api::types::TimeSeriesPoint> for TimeSeriesPointEntry {
    fn from(point: skene::api::types::TimeSeriesPoint) -> Self {
        Self {
            date: point.date,
            value: point.value,
        }
    }
}

impl From<skene::api::types::AnomalyAlert> for AnomalyEntry {
    fn from(alert: skene::api::types::AnomalyAlert) -> Self {
        Self {
            agent_id: alert.agent_id,
            agent_name: alert.agent_name,
            metric_name: alert.metric_name,
            current_value: alert.current_value,
            baseline_mean: alert.baseline_mean,
            deviation_pct: alert.deviation_pct,
            direction: alert.direction,
        }
    }
}

impl From<skene::api::types::AgentPerformance> for AgentPerformanceEntry {
    fn from(perf: skene::api::types::AgentPerformance) -> Self {
        Self {
            agent_id: perf.agent_id,
            agent_name: perf.agent_name,
            avg_tokens_per_response: perf.avg_tokens_per_response,
            tool_calls_per_session: perf.tool_calls_per_session,
            tool_success_rate: perf.tool_success_rate,
            distillation_frequency: perf.distillation_frequency,
            avg_context_before_distill: perf.avg_context_before_distill,
            messages_per_session: perf.messages_per_session,
            sessions_per_day: perf.sessions_per_day,
            errors_per_session: perf.errors_per_session,
            tokens_per_response_series: perf
                .tokens_per_response_series
                .into_iter()
                .map(Into::into)
                .collect(),
        }
    }
}

impl From<skene::api::types::AgentPerformanceListResponse> for AgentPerformanceApiResponse {
    fn from(resp: skene::api::types::AgentPerformanceListResponse) -> Self {
        Self {
            agents: resp.agents.into_iter().map(Into::into).collect(),
            anomalies: resp.anomalies.into_iter().map(Into::into).collect(),
        }
    }
}

impl From<skene::api::types::QualitySeries> for QualitySeriesEntry {
    fn from(series: skene::api::types::QualitySeries) -> Self {
        Self {
            avg_turn_length: series.avg_turn_length.into_iter().map(Into::into).collect(),
            response_to_question_ratio: series
                .response_to_question_ratio
                .into_iter()
                .map(Into::into)
                .collect(),
            tool_call_density: series
                .tool_call_density
                .into_iter()
                .map(Into::into)
                .collect(),
            thinking_time_ratio: series
                .thinking_time_ratio
                .into_iter()
                .map(Into::into)
                .collect(),
        }
    }
}

impl From<skene::api::types::QualityMetricsResponse> for QualityMetricsApiResponse {
    fn from(resp: skene::api::types::QualityMetricsResponse) -> Self {
        Self {
            series: resp.series.into(),
        }
    }
}

impl From<skene::api::types::JournalEvent> for JournalEventEntry {
    fn from(event: skene::api::types::JournalEvent) -> Self {
        Self {
            timestamp: event.timestamp,
            event_type: event.event_type,
            message: event.message,
        }
    }
}

impl From<skene::api::types::UnavailableMetric> for UnavailableMetricEntry {
    fn from(metric: skene::api::types::UnavailableMetric) -> Self {
        Self {
            metric: metric.metric,
            reason: metric.reason,
        }
    }
}

impl From<skene::api::types::JournalResponse> for JournalResponseEntry {
    fn from(resp: skene::api::types::JournalResponse) -> Self {
        Self {
            events: resp.events.into_iter().map(Into::into).collect(),
            data_unavailable: resp.data_unavailable.into_iter().map(Into::into).collect(),
        }
    }
}

// WHY(#4565): the remaining sources this view fetched directly (health,
// tokens, costs, knowledge facts/entities/timeline/health, sessions,
// agents) now go through skene's typed `ApiClient` the same way; these
// `From` impls are the same boundary the block above already established
// for performance/quality/journal.
impl From<skene::api::types::HealthResponse> for HealthApiResponse {
    fn from(resp: skene::api::types::HealthResponse) -> Self {
        Self {
            uptime_seconds: resp.uptime_seconds,
        }
    }
}

impl From<skene::api::types::TokenSeriesPoint> for TokenBucketEntry {
    fn from(point: skene::api::types::TokenSeriesPoint) -> Self {
        Self {
            input_tokens: point.input_tokens,
            output_tokens: point.output_tokens,
        }
    }
}

impl From<skene::api::types::AgentTokenRow> for AgentTokenRowEntry {
    fn from(row: skene::api::types::AgentTokenRow) -> Self {
        Self {
            session_count: row.session_count,
        }
    }
}

impl From<skene::api::types::TokenMetricsResponse> for TokenMetricsApiResponse {
    fn from(resp: skene::api::types::TokenMetricsResponse) -> Self {
        Self {
            series: resp.series.into_iter().map(Into::into).collect(),
            agents: resp.agents.into_iter().map(Into::into).collect(),
        }
    }
}

impl From<skene::api::types::CostSeriesPoint> for CostBucketEntry {
    fn from(point: skene::api::types::CostSeriesPoint) -> Self {
        Self {
            cost_usd: point.cost_usd,
        }
    }
}

impl From<skene::api::types::CostMetricsResponse> for CostMetricsApiResponse {
    fn from(resp: skene::api::types::CostMetricsResponse) -> Self {
        Self {
            series: resp.series.into_iter().map(Into::into).collect(),
        }
    }
}

impl From<skene::api::types::Fact> for FactEntry {
    fn from(fact: skene::api::types::Fact) -> Self {
        Self {
            confidence: fact.confidence,
            recorded_at: fact.recorded_at,
            is_forgotten: fact.is_forgotten,
            last_accessed_at: fact.last_accessed_at.unwrap_or_default(),
            stability_hours: fact.stability_hours,
            valid_to: fact.valid_to,
        }
    }
}

impl From<skene::api::types::EntityListItem> for EntityEntry {
    fn from(item: skene::api::types::EntityListItem) -> Self {
        Self {
            name: item.name,
            entity_type: item.entity_type,
            relationship_count: item.relationship_count,
            updated_at: item.updated_at,
        }
    }
}

impl From<skene::api::types::MemoryHealthResponse> for MemoryHealthApiResponse {
    fn from(resp: skene::api::types::MemoryHealthResponse) -> Self {
        Self {
            avg_confidence: resp.avg_confidence,
            orphan_ratio: resp.orphan_ratio,
            staleness_ratio: resp.staleness_ratio,
            health_score: resp.health_score,
        }
    }
}

impl From<skene::api::types::Session> for SessionEntry {
    fn from(session: skene::api::types::Session) -> Self {
        Self {
            nous_id: session.nous_id.to_string(),
            message_count: session.message_count,
            // NOTE: the paginated session list carries only `updated_at`;
            // `created_at` has no skene/wire equivalent, matching the
            // pre-migration `SessionEntry` (whose own field never had a
            // `created_at` source either -- see `activity_timestamp`).
            created_at: String::new(),
            updated_at: session.updated_at.unwrap_or_default(),
        }
    }
}

impl From<skene::api::types::Agent> for AgentEntry {
    fn from(agent: skene::api::types::Agent) -> Self {
        Self {
            id: agent.id.to_string(),
            name: agent.display_name().to_owned(),
        }
    }
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
            role: "region",
            "aria-label": "Meta-Insights",
            div {
                style: "{HEADER_STYLE}",
                h2 { style: "font-size: var(--text-xl); margin: 0;", "Meta-Insights" }
                button {
                    style: "{REFRESH_BTN}",
                    "aria-label": "Refresh meta-insights",
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
                role: "button",
                tabindex: "0",
                "aria-expanded": if expanded { "true" } else { "false" },
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
    let client = match skene::api::client::ApiClient::new(&cfg.server_url, cfg.auth_token.clone()) {
        Ok(client) => client,
        Err(err) => return FetchState::Error(err.to_string()),
    };

    let facts_params = skene::api::types::KnowledgeFactsRequest {
        limit: Some(1000),
        include_forgotten: true,
        ..Default::default()
    };
    let entities_params = skene::api::types::KnowledgeEntitiesRequest::default();
    let sessions_params = skene::api::types::ListSessionsRequest::default();

    // WHY: Fetch all endpoints in parallel to minimize latency.
    let (
        health_res,
        tokens_res,
        costs_res,
        facts_res,
        entities_res,
        timeline_res,
        memory_health_res,
        sessions_res,
        agents_res,
        perf_res,
        quality_res,
        journal_res,
    ) = tokio::join!(
        client.health_details(),
        client.token_metrics(None, None, None),
        client.cost_metrics(None, None, None),
        client.knowledge_facts(&facts_params),
        client.knowledge_entities(&entities_params),
        client.knowledge_timeline(),
        client.knowledge_health(),
        client.sessions_paginated(&sessions_params),
        client.agents(),
        client.agent_performance(),
        client.quality_metrics(),
        client.journal(),
    );

    // WHY(#6732): `/api/health` is unauthenticated liveness only (`status`
    // only); `uptime_seconds` requires the operator-only detailed route
    // `health_details` hits.
    let health: HealthApiResponse = match health_res {
        Ok(data) => data.into(),
        Err(err) => {
            return FetchState::Error(err.to_string());
        }
    };

    let tokens: TokenMetricsApiResponse = tokens_res.map(Into::into).unwrap_or_default();
    let costs: CostMetricsApiResponse = costs_res.map(Into::into).unwrap_or_default();

    let facts: Vec<FactEntry> = facts_res
        .map(|resp| resp.facts.into_iter().map(Into::into).collect())
        .unwrap_or_default();

    let entities: Vec<EntityEntry> = entities_res
        .map(|resp| resp.entities.into_iter().map(Into::into).collect())
        .unwrap_or_default();

    let timeline: Vec<TimelineEntry> = timeline_res
        .map(|resp| bucket_timeline_events(&resp.events))
        .unwrap_or_default();

    let sessions: Vec<SessionEntry> = sessions_res
        .map(|resp| resp.items.into_iter().map(Into::into).collect())
        .unwrap_or_default();

    let agents: Vec<AgentEntry> = agents_res
        .map(|resp| resp.into_iter().map(Into::into).collect())
        .unwrap_or_default();

    // WHY(#6823): when the server route responds, its store-derived snapshot
    // replaces the client-side recomputation from the fact/entity lists
    // above (see `assemble_meta_data`). A failure (older server or a
    // disabled knowledge store) degrades to `(default, false)`, which keeps
    // that client-side computation as the fallback.
    let (server_memory_health, server_memory_health_available): (MemoryHealthApiResponse, bool) =
        match memory_health_res {
            Ok(resp) => (resp.into(), true),
            Err(err) => {
                tracing::warn!(error = %err, "failed to load server memory health");
                (MemoryHealthApiResponse::default(), false)
            }
        };

    // WHY: every source here returns `Result<_, ApiError>` from skene, not
    // an HTTP response to inspect -- `Ok` already means "2xx and parsed",
    // so a plain `match` is the "genuinely usable" condition each `_res`
    // above needed by hand before this migration.
    let (perf, perf_available): (AgentPerformanceApiResponse, bool) = match perf_res {
        Ok(resp) => (resp.into(), true),
        Err(err) => {
            tracing::warn!(error = %err, "failed to load agent performance");
            (AgentPerformanceApiResponse::default(), false)
        }
    };

    let (quality, quality_available): (QualityMetricsApiResponse, bool) = match quality_res {
        Ok(resp) => (resp.into(), true),
        Err(err) => {
            tracing::warn!(error = %err, "failed to load quality metrics");
            (QualityMetricsApiResponse::default(), false)
        }
    };

    // WHY(#4486): pylon's `/api/v1/journal` returns an envelope
    // (`{events, data_unavailable}`), not a bare array, and marks itself
    // unavailable in-band via `data_unavailable` rather than an HTTP error
    // (no persistent event journal backs the route yet). `skene::api::types::
    // JournalResponse` already models that envelope, so no bare-array/wrapped
    // fallback parsing is needed here the way the still-raw sources above
    // need it.
    let (journal_response, journal_fetched): (JournalResponseEntry, bool) = match journal_res {
        Ok(resp) => (resp.into(), true),
        Err(err) => {
            tracing::warn!(error = %err, "failed to load journal");
            (JournalResponseEntry::default(), false)
        }
    };
    let journal_available = journal_fetched && journal_response.data_unavailable.is_empty();
    // WHY: surface pylon's own reason instead of a generic message -- the
    // whole point of parsing the envelope (see #4486 above) was to know
    // *why* the source is unavailable, not just that it is.
    let journal_unavailable_reason = journal_response
        .data_unavailable
        .first()
        .map(|entry| format!("{}: {}", entry.metric, entry.reason));
    let journal = journal_response.events;

    let data = assemble_meta_data(
        health,
        tokens,
        costs,
        facts,
        entities,
        timeline,
        server_memory_health_available.then_some(server_memory_health),
        sessions,
        agents,
        perf,
        quality,
        journal,
        perf_available,
        quality_available,
        journal_available,
        journal_unavailable_reason,
    );
    FetchState::Loaded(data)
}

/// Bucket per-fact timeline events into per-date counts client-side.
///
/// WHY(#4565): `skene::api::types::TimelineEvent` is the typed wire shape
/// `ApiClient::knowledge_timeline` returns -- no bare-array/wrapped
/// fallback parsing is needed here the way the pre-migration raw-text
/// version needed, since skene owns that deserialize boundary now.
fn bucket_timeline_events(events: &[skene::api::types::TimelineEvent]) -> Vec<TimelineEntry> {
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

#[cfg(test)]
mod tests {
    use super::*;

    /// `#4486`'s original regression: pylon's `/api/v1/journal` returns an
    /// envelope (`{events, data_unavailable}`), not a bare array. The
    /// conversion from skene's typed response must carry a populated
    /// `data_unavailable` through unchanged (pylon has no persistent event
    /// journal yet, so every real response currently carries one).
    #[test]
    fn journal_response_conversion_preserves_data_unavailable() {
        let resp = skene::api::types::JournalResponse {
            events: Vec::new(),
            data_unavailable: vec![skene::api::types::UnavailableMetric {
                metric: "journal".to_string(),
                reason: "no persistent event journal is available in pylon".to_string(),
            }],
        };

        let parsed: JournalResponseEntry = resp.into();

        assert!(parsed.events.is_empty());
        assert_eq!(parsed.data_unavailable.len(), 1);
        assert_eq!(
            parsed.data_unavailable.first().map(|u| u.metric.as_str()),
            Some("journal")
        );
    }

    /// A genuinely populated journal converts with its events intact and an
    /// empty `data_unavailable`.
    #[test]
    fn journal_response_conversion_carries_events_when_available() {
        let resp = skene::api::types::JournalResponse {
            events: vec![skene::api::types::JournalEvent {
                timestamp: "2026-01-01T00:00:00Z".to_string(),
                event_type: "config".to_string(),
                message: "reloaded".to_string(),
            }],
            data_unavailable: Vec::new(),
        };

        let parsed: JournalResponseEntry = resp.into();

        assert_eq!(parsed.events.len(), 1);
        assert!(parsed.data_unavailable.is_empty());
    }

    /// The old bare-array shape must not silently pass. That parsing
    /// boundary now lives in `skene::api::types::JournalResponse` rather
    /// than in this crate (see the `JournalResponseEntry` doc comment
    /// above) -- `events` has no `#[serde(default)]` there, so a bare `[]`
    /// still fails to deserialize rather than the client quietly reporting
    /// empty-and-available.
    #[test]
    fn skene_journal_response_rejects_bare_array() {
        let body = serde_json::json!([]).to_string();
        let result: Result<skene::api::types::JournalResponse, _> = serde_json::from_str(&body);
        assert!(
            result.is_err(),
            "a bare array must not parse as skene's journal response envelope"
        );
    }
}
