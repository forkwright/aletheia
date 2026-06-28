// WHY: wire DTO
//! Request and response types for meta-insights endpoints.

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// A single point in a time series.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct TimeSeriesPoint {
    /// ISO 8601 date (`YYYY-MM-DD`).
    pub date: String,
    /// Numeric value for this date.
    pub value: f64,
}

/// Description of a metric that cannot currently be measured.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct UnavailableMetric {
    /// Metric or field name that is not measured.
    pub metric: String,
    /// Human-readable reason the metric is unavailable.
    pub reason: String,
}

/// Per-agent performance metrics.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct AgentPerformance {
    /// Agent identifier.
    pub agent_id: String,
    /// Human-readable agent name.
    pub agent_name: String,
    /// Average tokens per response.
    pub avg_tokens_per_response: f64,
    /// Tool calls per session.
    pub tool_calls_per_session: f64,
    /// Fraction of tool calls that succeeded (0.0–1.0).
    pub tool_success_rate: f64,
    /// Distillations per session.
    pub distillation_frequency: f64,
    /// Average context tokens before distillation.
    pub avg_context_before_distill: f64,
    /// Messages per session.
    pub messages_per_session: f64,
    /// Sessions per active day.
    pub sessions_per_day: f64,
    /// Errors per session.
    pub errors_per_session: f64,
    /// Daily time series of tokens-per-response.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tokens_per_response_series: Vec<TimeSeriesPoint>,
    /// Metrics that are currently not measured by any backing data source.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub data_unavailable: Vec<UnavailableMetric>,
}

/// Anomaly alert for a single metric.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct AnomalyAlert {
    /// Agent identifier.
    pub agent_id: String,
    /// Human-readable agent name.
    pub agent_name: String,
    /// Metric that triggered the alert.
    pub metric_name: String,
    /// Latest observed value.
    pub current_value: f64,
    /// Mean of the rolling window.
    pub baseline_mean: f64,
    /// Percentage deviation from baseline.
    pub deviation_pct: f64,
    /// Direction of deviation (`"up"` or `"down"`).
    pub direction: String,
}

/// Response for `GET /api/v1/metrics/agents`.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct AgentPerformanceListResponse {
    /// Per-agent performance data.
    pub agents: Vec<AgentPerformance>,
    /// Anomalies detected across all agents.
    pub anomalies: Vec<AnomalyAlert>,
}

/// Quality metric time series bundle.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct QualitySeries {
    /// Average turns per session per day.
    pub avg_turn_length: Vec<TimeSeriesPoint>,
    /// Ratio of assistant responses to user questions per day.
    pub response_to_question_ratio: Vec<TimeSeriesPoint>,
    /// Tool result messages per total messages per day.
    pub tool_call_density: Vec<TimeSeriesPoint>,
    /// Fraction of time spent in thinking mode per day.
    pub thinking_time_ratio: Vec<TimeSeriesPoint>,
}

/// Response for `GET /api/v1/metrics/quality`.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct QualityMetricsResponse {
    /// Time series quality indicators.
    pub series: QualitySeries,
    /// Metrics that are currently not measured by any backing data source.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub data_unavailable: Vec<UnavailableMetric>,
}

/// Query parameters shared by desktop metrics views.
#[derive(Debug, Deserialize, utoipa::IntoParams)]
pub struct MetricsQuery {
    /// Series granularity: daily, weekly, or monthly.
    ///
    /// * `daily` buckets each session by its calendar date (`YYYY-MM-DD`).
    /// * `weekly` buckets by ISO week (`YYYY-Www`), so every date in the same
    ///   ISO week maps to the same key (including across year boundaries).
    /// * `monthly` buckets by calendar month (`YYYY-MM`).
    #[serde(default)]
    pub granularity: Option<String>,
    /// Inclusive start date (`YYYY-MM-DD`).
    #[serde(default)]
    pub from: Option<String>,
    /// Inclusive end date (`YYYY-MM-DD`).
    #[serde(default)]
    pub to: Option<String>,
}

/// A single token time-series point.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct TokenSeriesPoint {
    /// Bucket date (`YYYY-MM-DD`, ISO week, or `YYYY-MM`).
    pub date: String,
    /// Input tokens in this bucket.
    pub input_tokens: u64,
    /// Output tokens in this bucket.
    pub output_tokens: u64,
}

/// Per-agent token usage row.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct AgentTokenRow {
    /// Agent identifier.
    pub id: String,
    /// Human-readable agent name.
    pub name: String,
    /// Input tokens attributed to this agent.
    pub input_tokens: u64,
    /// Output tokens attributed to this agent.
    pub output_tokens: u64,
    /// Sessions attributed to this agent.
    pub session_count: u64,
}

/// Per-model token usage row.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ModelTokenRow {
    /// Model identifier.
    pub model: String,
    /// Input tokens attributed to this model.
    pub input_tokens: u64,
    /// Output tokens attributed to this model.
    pub output_tokens: u64,
    /// Sessions attributed to this model.
    pub session_count: u64,
}

/// Response for `GET /api/v1/metrics/tokens`.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct TokenMetricsResponse {
    /// Token usage over time.
    pub series: Vec<TokenSeriesPoint>,
    /// Token usage grouped by agent.
    pub agents: Vec<AgentTokenRow>,
    /// Token usage grouped by model.
    pub models: Vec<ModelTokenRow>,
    /// Input tokens used today.
    pub today_input: u64,
    /// Output tokens used today.
    pub today_output: u64,
    /// Input tokens used this week.
    pub week_input: u64,
    /// Output tokens used this week.
    pub week_output: u64,
    /// Input tokens used this month.
    pub month_input: u64,
    /// Output tokens used this month.
    pub month_output: u64,
    /// Input tokens used in the previous equivalent day.
    pub prev_today_input: u64,
    /// Output tokens used in the previous equivalent day.
    pub prev_today_output: u64,
    /// Input tokens used in the previous equivalent week.
    pub prev_week_input: u64,
    /// Output tokens used in the previous equivalent week.
    pub prev_week_output: u64,
    /// Input tokens used in the previous equivalent month.
    pub prev_month_input: u64,
    /// Output tokens used in the previous equivalent month.
    pub prev_month_output: u64,
}

/// A single cost time-series point.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CostSeriesPoint {
    /// Bucket date (`YYYY-MM-DD`, ISO week, or `YYYY-MM`).
    pub date: String,
    /// Estimated cost in USD for this bucket.
    pub cost_usd: f64,
}

/// Per-agent estimated cost row.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct AgentCostRow {
    /// Agent identifier.
    pub id: String,
    /// Human-readable agent name.
    pub name: String,
    /// Estimated cost in USD.
    pub total_cost: f64,
    /// Message count attributed to this agent.
    pub message_count: u64,
    /// Sessions attributed to this agent.
    pub session_count: u64,
    /// Output tokens attributed to this agent.
    pub output_tokens: u64,
    /// Cost from the previous equivalent period.
    pub prev_period_cost: f64,
}

/// Response for `GET /api/v1/metrics/costs`.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CostMetricsResponse {
    /// Estimated cost over time.
    pub series: Vec<CostSeriesPoint>,
    /// Estimated costs grouped by agent.
    pub agents: Vec<AgentCostRow>,
    /// Estimated cost today.
    pub today_cost: f64,
    /// Estimated cost this week.
    pub week_cost: f64,
    /// Estimated cost this month.
    pub month_cost: f64,
    /// Estimated cost for the previous equivalent day.
    pub prev_today_cost: f64,
    /// Estimated cost for the previous equivalent week.
    pub prev_week_cost: f64,
    /// Estimated cost for the previous equivalent month.
    pub prev_month_cost: f64,
    /// Cost metrics that are currently not measured by any backing data source.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub data_unavailable: Vec<UnavailableMetric>,
}

/// A single journal event.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct JournalEvent {
    /// ISO 8601 timestamp.
    pub timestamp: String,
    /// Event category (`error`, `distillation`, `config`, `memory`).
    pub event_type: String,
    /// Human-readable description.
    pub message: String,
}

/// Response for `GET /api/v1/journal`.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct JournalResponse {
    /// Journal events matching the query.
    pub events: Vec<JournalEvent>,
    /// Metrics that are currently not measured by any backing data source.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub data_unavailable: Vec<UnavailableMetric>,
}

/// Query parameters for `GET /api/v1/journal`.
#[derive(Debug, Deserialize, utoipa::IntoParams)]
pub struct JournalQuery {
    /// Filter by source subsystem.
    #[serde(default)]
    pub source: Option<String>,
    /// Filter by severity level.
    #[serde(default)]
    pub level: Option<String>,
    /// Only events after this ISO 8601 timestamp.
    #[serde(default)]
    pub since: Option<String>,
    /// Maximum events to return (default 100, max 1000).
    #[serde(default = "default_journal_limit")]
    pub limit: u32,
}

fn default_journal_limit() -> u32 {
    100
}
