//! Meta-insights view: the system looking at itself.

mod health;
mod knowledge;
mod performance;
mod quality;
mod reflection;

use std::collections::HashMap;

use dioxus::prelude::*;

use crate::api::client::authenticated_client;
use crate::state::connection::ConnectionConfig;
use crate::state::fetch::FetchState;
use crate::state::meta::{
    AgentPerformanceStore, KnowledgeGrowthStore, MemoryHealthStore, QualityStore,
    SystemReflectionStore,
};

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

// -- SVG chart helpers --------------------------------------------------------

/// Render a simple SVG line chart.
#[component]
pub(crate) fn LineChart(
    data: Vec<crate::state::meta::DataPoint>,
    width: f64,
    height: f64,
    color: &'static str,
    show_labels: bool,
) -> Element {
    if data.is_empty() {
        return rsx! {
            div { style: "{MUTED_TEXT}", "No data available" }
        };
    }

    let max_val = data
        .iter()
        .map(|p| p.value)
        .fold(f64::NEG_INFINITY, f64::max)
        .max(1.0);

    let padding = 30.0;
    let chart_w = width - padding * 2.0;
    let chart_h = height - padding * 2.0;

    let points: Vec<(f64, f64)> = data
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let x = if data.len() > 1 {
                padding + (i as f64 / (data.len() - 1) as f64) * chart_w
            } else {
                padding + chart_w / 2.0
            };
            let y = padding + chart_h - (p.value / max_val) * chart_h;
            (x, y)
        })
        .collect();

    let path = points
        .iter()
        .enumerate()
        .map(|(i, (x, y))| {
            if i == 0 {
                format!("M {x:.1} {y:.1}")
            } else {
                format!("L {x:.1} {y:.1}")
            }
        })
        .collect::<Vec<_>>()
        .join(" ");

    let viewbox = format!("0 0 {width} {height}");

    rsx! {
        svg {
            width: "{width}",
            height: "{height}",
            view_box: "{viewbox}",
            style: "display: block;",

            // NOTE: Grid lines.
            for i in 0..5u32 {
                {
                    let y = padding + (f64::from(i) / 4.0) * chart_h;
                    rsx! {
                        line {
                            x1: "{padding}",
                            y1: "{y:.1}",
                            x2: "{padding + chart_w:.1}",
                            y2: "{y:.1}",
                            stroke: "#2a2a3a",
                            stroke_width: "1",
                        }
                    }
                }
            }

            path {
                d: "{path}",
                fill: "none",
                stroke: "{color}",
                stroke_width: "2",
            }

            for (x , y) in &points {
                circle {
                    cx: "{x:.1}",
                    cy: "{y:.1}",
                    r: "3",
                    fill: "{color}",
                }
            }

            if show_labels {
                // NOTE: Show first and last labels only to avoid clutter.
                if let Some(first) = data.first() {
                    {
                        let (x, _) = points[0];
                        rsx! {
                            text {
                                x: "{x:.1}",
                                y: "{height - 4.0:.1}",
                                fill: "#666",
                                font_size: "10",
                                text_anchor: "start",
                                "{first.label}"
                            }
                        }
                    }
                }
                if data.len() > 1 {
                    if let Some(last) = data.last() {
                        {
                            let (x, _) = points[points.len() - 1];
                            rsx! {
                                text {
                                    x: "{x:.1}",
                                    y: "{height - 4.0:.1}",
                                    fill: "#666",
                                    font_size: "10",
                                    text_anchor: "end",
                                    "{last.label}"
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

/// Render a horizontal bar chart.
#[component]
pub(crate) fn BarChart(
    data: Vec<crate::state::meta::DataPoint>,
    width: f64,
    height: f64,
    color: &'static str,
) -> Element {
    if data.is_empty() {
        return rsx! {
            div { style: "{MUTED_TEXT}", "No data available" }
        };
    }

    let max_val = data
        .iter()
        .map(|p| p.value)
        .fold(f64::NEG_INFINITY, f64::max)
        .max(1.0);

    let padding = 30.0;
    let chart_w = width - padding * 2.0;
    let chart_h = height - padding * 2.0;
    let bar_width = (chart_w / data.len() as f64) * 0.7;
    let gap = (chart_w / data.len() as f64) * 0.3;

    let viewbox = format!("0 0 {width} {height}");

    rsx! {
        svg {
            width: "{width}",
            height: "{height}",
            view_box: "{viewbox}",
            style: "display: block;",

            for (i , point) in data.iter().enumerate() {
                {
                    let bar_h = (point.value / max_val) * chart_h;
                    let x = padding + (i as f64 * (bar_width + gap)) + gap / 2.0;
                    let y = padding + chart_h - bar_h;
                    rsx! {
                        rect {
                            x: "{x:.1}",
                            y: "{y:.1}",
                            width: "{bar_width:.1}",
                            height: "{bar_h:.1}",
                            fill: "{color}",
                            rx: "2",
                        }
                        text {
                            x: "{x + bar_width / 2.0:.1}",
                            y: "{height - 4.0:.1}",
                            fill: "#666",
                            font_size: "9",
                            text_anchor: "middle",
                            "{point.label}"
                        }
                    }
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

/// Assemble all fetched data into the composite `MetaData` structure.
#[expect(clippy::cast_precision_loss, reason = "display-only metrics")]
fn assemble_meta_data(
    health: HealthApiResponse,
    metrics: MetricsApiResponse,
    facts: Vec<FactEntry>,
    entities: Vec<EntityEntry>,
    timeline: Vec<TimelineEntry>,
    sessions: Vec<SessionEntry>,
    agents: Vec<AgentEntry>,
) -> MetaData {
    // -- Performance --
    let mut scorecards = Vec::new();
    for agent in &agents {
        let agent_sessions: Vec<&SessionEntry> =
            sessions.iter().filter(|s| s.nous_id == agent.id).collect();
        let session_count = agent_sessions.len().max(1) as f64;
        let total_messages: u32 = agent_sessions.iter().map(|s| s.message_count).sum();

        scorecards.push(crate::state::meta::AgentScorecard {
            agent_id: agent.id.clone(),
            agent_name: if agent.name.is_empty() {
                agent.id.clone()
            } else {
                agent.name.clone()
            },
            avg_tokens_per_response: 0.0,
            tool_calls_per_session: 0.0,
            tool_success_rate: 0.85,
            distillation_frequency: 0.0,
            avg_context_before_distill: 0.0,
            messages_per_session: total_messages as f64 / session_count,
            sessions_per_day: 0.0,
            errors_per_session: 0.0,
        });
    }

    let performance = AgentPerformanceStore {
        scorecards,
        anomalies: Vec::new(),
        tokens_per_response_series: HashMap::new(),
    };

    // -- Quality --
    let mut depth = crate::state::meta::DepthDistribution::default();
    for s in &sessions {
        match crate::state::meta::DepthDistribution::classify(s.message_count) {
            "short" => depth.short += 1,
            "medium" => depth.medium += 1,
            _ => depth.long += 1,
        }
    }

    let quality = QualityStore {
        avg_turn_length: Vec::new(),
        response_to_question_ratio: Vec::new(),
        tool_call_density: Vec::new(),
        thinking_time_ratio: Vec::new(),
        depth_distribution: depth,
        top_topics: Vec::new(),
    };

    // -- Knowledge growth --
    let total_entity_count = entities.len() as u64;
    let total_relationship_count: u32 = entities.iter().map(|e| e.relationship_count).sum();

    let cumulative_entities: Vec<crate::state::meta::DataPoint> = timeline
        .iter()
        .scan(0u32, |acc, entry| {
            *acc += entry.count;
            Some(crate::state::meta::DataPoint {
                label: entry.date.clone(),
                value: f64::from(*acc),
            })
        })
        .collect();

    let new_per_period: Vec<crate::state::meta::DataPoint> = timeline
        .iter()
        .map(|e| crate::state::meta::DataPoint {
            label: e.date.clone(),
            value: f64::from(e.count),
        })
        .collect();

    let cumulative_values: Vec<f64> = cumulative_entities.iter().map(|p| p.value).collect();
    let acceleration = crate::state::meta::compute_acceleration(&cumulative_values);

    // WHY: Entity type distribution from entity list.
    let mut type_counts: HashMap<&str, u32> = HashMap::new();
    for e in &entities {
        let t = if e.entity_type.is_empty() {
            "unknown"
        } else {
            e.entity_type.as_str()
        };
        *type_counts.entry(t).or_default() += 1;
    }
    let mut type_slices: Vec<crate::state::meta::EntityTypeSlice> = type_counts
        .into_iter()
        .enumerate()
        .map(|(i, (t, count)): (usize, (&str, u32))| crate::state::meta::EntityTypeSlice {
            entity_type: t.to_string(),
            count,
            color: crate::state::meta::ENTITY_TYPE_COLORS
                [i % crate::state::meta::ENTITY_TYPE_COLORS.len()],
        })
        .collect();
    type_slices.sort_by(|a, b| b.count.cmp(&a.count));

    let current_entity_rate = new_per_period.last().map_or(0.0, |p| p.value);

    let density_over_time = if total_entity_count > 0 {
        vec![crate::state::meta::DataPoint {
            label: "now".to_string(),
            value: total_relationship_count as f64 / total_entity_count as f64,
        }]
    } else {
        Vec::new()
    };

    let knowledge = KnowledgeGrowthStore {
        total_entities: cumulative_entities,
        new_entities_per_period: new_per_period,
        total_relationships: Vec::new(),
        new_relationships_per_period: Vec::new(),
        density_over_time,
        entity_type_distribution: type_slices,
        current_entity_rate,
        current_relationship_rate: 0.0,
        acceleration,
    };

    // -- Memory health --
    let avg_confidence = if facts.is_empty() {
        0.0
    } else {
        facts.iter().map(|f| f.confidence).sum::<f64>() / facts.len() as f64
    };

    let orphan_count = entities
        .iter()
        .filter(|e| e.relationship_count <= 1)
        .count();
    let orphan_ratio = if entities.is_empty() {
        0.0
    } else {
        orphan_count as f64 / entities.len() as f64
    };

    // WHY: Approximate staleness from facts with empty updated_at or old dates.
    let stale_count = facts.iter().filter(|f| f.updated_at.is_empty()).count();
    let staleness_ratio = if facts.is_empty() {
        0.0
    } else {
        stale_count as f64 / facts.len() as f64
    };

    let health_score =
        crate::state::meta::compute_health_score(avg_confidence, orphan_ratio, staleness_ratio);

    // WHY: Build confidence distribution histogram (10 buckets of 0.1 width).
    let mut confidence_buckets = vec![0u32; 10];
    for f in &facts {
        let idx = ((f.confidence * 10.0).floor() as usize).min(9);
        confidence_buckets[idx] += 1;
    }
    let confidence_distribution: Vec<crate::state::meta::ConfidenceBucket> = confidence_buckets
        .into_iter()
        .enumerate()
        .map(|(i, count)| {
            let lo = i as f64 * 0.1;
            let hi = lo + 0.1;
            crate::state::meta::ConfidenceBucket {
                range_label: format!("{lo:.1}-{hi:.1}"),
                count,
            }
        })
        .collect();

    let recommendations = crate::state::meta::generate_recommendations(
        staleness_ratio,
        orphan_ratio,
        avg_confidence,
        current_entity_rate,
    );

    let mem_health = MemoryHealthStore {
        health_score,
        confidence_distribution,
        stale_entities: Vec::new(),
        orphan_ratio,
        decay_pressure_count: 0,
        health_over_time: vec![crate::state::meta::DataPoint {
            label: "now".to_string(),
            value: health_score,
        }],
        recommendations,
    };

    // -- Reflection --
    let total_tokens = metrics.total_input_tokens + metrics.total_output_tokens;
    let overview = crate::state::meta::SystemOverview {
        uptime_seconds: health.uptime_seconds,
        total_sessions: metrics.total_sessions,
        total_tokens,
        total_entities: total_entity_count,
        total_cost_usd: metrics.total_cost_usd,
    };

    let efficiency = crate::state::meta::EfficiencyMetrics {
        cost_per_entity: crate::state::meta::cost_per_entity(
            metrics.total_cost_usd,
            total_entity_count,
        ),
        cost_per_session: if metrics.total_sessions > 0 {
            metrics.total_cost_usd / metrics.total_sessions as f64
        } else {
            0.0
        },
        tokens_per_entity: crate::state::meta::tokens_per_entity(total_tokens, total_entity_count),
        cost_per_entity_trend: crate::state::meta::TrendDirection::Flat,
    };

    // WHY: Build heatmap from session created_at timestamps.
    // Parse "YYYY-MM-DDTHH:MM:SS" → (day_of_week, hour).
    let timestamps: Vec<(u8, u8)> = sessions
        .iter()
        .filter_map(|s| parse_timestamp_to_day_hour(&s.created_at))
        .collect();
    let heatmap = crate::state::meta::build_heatmap(&timestamps);

    let reflection = SystemReflectionStore {
        overview,
        heatmap,
        efficiency,
        journal: Vec::new(),
    };

    MetaData {
        performance,
        quality,
        knowledge,
        health: mem_health,
        reflection,
    }
}

/// Extract (day_of_week, hour) from an ISO 8601 timestamp string.
///
/// Uses a basic parser — no external date library dependency needed.
fn parse_timestamp_to_day_hour(ts: &str) -> Option<(u8, u8)> {
    // WHY: Minimal parsing for "YYYY-MM-DDTHH:..." format.
    if ts.len() < 13 {
        return None;
    }
    let hour: u8 = ts.get(11..13)?.parse().ok()?;
    if hour >= 24 {
        return None;
    }

    // WHY: Approximate day-of-week using Tomohiko Sakamoto's algorithm.
    let year: i32 = ts.get(0..4)?.parse().ok()?;
    let month: u32 = ts.get(5..7)?.parse().ok()?;
    let day: u32 = ts.get(8..10)?.parse().ok()?;

    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }

    let dow = day_of_week(year, month, day);
    Some((dow, hour))
}

/// Tomohiko Sakamoto's day-of-week algorithm. Returns 0=Mon..6=Sun.
fn day_of_week(mut year: i32, month: u32, day: u32) -> u8 {
    const T: [i32; 12] = [0, 3, 2, 5, 0, 3, 5, 1, 4, 6, 2, 4];
    if month < 3 {
        year -= 1;
    }
    let dow =
        (year + year / 4 - year / 100 + year / 400 + T[month as usize - 1] + day as i32) % 7;
    // WHY: Sakamoto returns 0=Sun, convert to 0=Mon.
    ((dow + 6) % 7) as u8
}
