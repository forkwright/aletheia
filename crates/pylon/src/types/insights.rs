// WHY: wire DTO
//! Request and response types for meta-insights endpoints.

use std::collections::HashMap;

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
    /// Series granularity: daily, weekly, or monthly. Defaults to daily.
    ///
    /// Series point keys are `YYYY-MM-DD` for daily, `YYYY-Www` (ISO 8601
    /// week date, week starting Monday) for weekly, and `YYYY-MM` for
    /// monthly. Weekly keys carry the ISO week-year, which differs from the
    /// calendar year for the days either side of a New Year that fall in the
    /// same ISO week.
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

/// Query parameters for `GET /api/tool-stats` (#4484).
#[derive(Debug, Deserialize, utoipa::IntoParams)]
pub struct ToolStatsQuery {
    /// Aggregation window in days, clamped to `1..=90`. Defaults to 7.
    #[serde(default = "default_tool_stats_days")]
    pub days: u32,
    /// Restrict `tools`, `time_series`, and `invocations` to one tool name.
    /// `summary` always reflects every tool regardless of this filter.
    #[serde(default)]
    pub tool: Option<String>,
}

fn default_tool_stats_days() -> u32 {
    7
}

/// Response for `GET /api/tool-stats` (#4484).
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ToolStatsResponse {
    /// Global summary cards, unaffected by the `tool` query filter.
    pub summary: ToolUsageSummary,
    /// Per-tool aggregates within the requested `days` window (and `tool`
    /// filter, when set).
    pub tools: Vec<ToolStat>,
    /// Daily invocation counts per tool within the requested window.
    pub time_series: Vec<ToolTimeSeriesBucket>,
    /// Raw invocation records within the requested window.
    pub invocations: Vec<ToolInvocationRecord>,
    /// Metrics that are currently not measured, or not measured completely,
    /// by any backing data source.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub data_unavailable: Vec<UnavailableMetric>,
}

/// Global tool-usage summary for dashboard cards.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ToolUsageSummary {
    /// Invocations recorded today (UTC calendar date).
    pub total_invocations_today: u64,
    /// Invocations recorded in the trailing 7 days including today.
    pub total_invocations_week: u64,
    /// Invocations recorded in the current calendar month.
    pub total_invocations_month: u64,
    /// `total_invocations_today` minus the prior day's total.
    pub delta_today: i64,
    /// `total_invocations_week` minus the prior 7-day period's total.
    pub delta_week: i64,
    /// `total_invocations_month` minus the prior calendar month's total.
    pub delta_month: i64,
    /// Overall success rate across all tools in the requested `days` window.
    pub success_rate: f64,
    /// Success rate for the equivalent-length period immediately before the
    /// requested window, for trend comparison.
    pub success_rate_prev: f64,
    /// Average execution duration across all tools in the requested window (ms).
    pub avg_duration_ms: u64,
    /// Average duration for the equivalent prior period (ms).
    pub avg_duration_prev_ms: u64,
    /// Tool with the most invocations in the requested window.
    pub most_used_tool: String,
    /// Invocation count for `most_used_tool`.
    pub most_used_count: u64,
}

/// Per-tool aggregated statistics within the requested window.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ToolStat {
    /// Registered tool name.
    pub name: String,
    /// Total invocations in the window.
    pub total: u64,
    /// Invocations that did not report an error.
    pub succeeded: u64,
    /// Invocations that reported an error.
    pub failed: u64,
    /// Fastest observed duration (ms).
    pub min_ms: u64,
    /// 25th-percentile duration (ms), nearest-rank.
    pub p25_ms: u64,
    /// Median duration (ms), nearest-rank.
    pub p50_ms: u64,
    /// 75th-percentile duration (ms), nearest-rank.
    pub p75_ms: u64,
    /// 95th-percentile duration (ms), nearest-rank.
    pub p95_ms: u64,
    /// Slowest observed duration (ms).
    pub max_ms: u64,
    /// Most frequently repeated captured result text among failed calls, if any.
    pub most_common_error: Option<String>,
    /// Timestamp of the most recent failure, if any.
    pub last_failure_at: Option<String>,
}

/// A single time-series bucket: one UTC calendar date, invocation counts per tool.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ToolTimeSeriesBucket {
    /// ISO 8601 date (`YYYY-MM-DD`).
    pub date: String,
    /// Map of tool name to invocation count on this date.
    #[schema(value_type = Object)]
    pub counts: HashMap<String, u64>,
}

/// A single raw tool invocation within the requested window.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ToolInvocationRecord {
    /// Registered tool name.
    pub tool_name: String,
    /// Agent that requested the tool call.
    // kanon:ignore RUST/primitive-for-domain-id — wire DTO field mirrors ToolAuditRecord.nous_id
    pub agent_id: String,
    /// ISO 8601 timestamp of the call.
    pub timestamp: String,
    /// Execution duration in milliseconds.
    pub duration_ms: u64,
    /// `true` when the call did not report an error.
    pub success: bool,
    /// Captured result text when the call reported an error.
    pub error: Option<String>,
}
