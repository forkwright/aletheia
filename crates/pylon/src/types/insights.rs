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
