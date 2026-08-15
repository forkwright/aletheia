//! Typed token/cost metrics DTOs mirroring Pylon's wire shapes
//! (`crates/pylon/src/types/insights.rs`). Skene has no dependency on pylon,
//! so these are independent structs kept in sync by the contract tests in
//! `super::tests`.

use serde::{Deserialize, Serialize};

/// A single token time-series point.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[expect(
    missing_docs,
    reason = "fields mirror pylon's TokenSeriesPoint; self-documenting by name"
)]
pub struct TokenSeriesPoint {
    pub date: String,
    pub input_tokens: u64,
    pub output_tokens: u64,
}

/// Per-agent token usage row.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[expect(
    missing_docs,
    reason = "fields mirror pylon's AgentTokenRow; self-documenting by name"
)]
pub struct AgentTokenRow {
    pub id: String,
    pub name: String,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub session_count: u64,
}

/// Per-model token usage row.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[expect(
    missing_docs,
    reason = "fields mirror pylon's ModelTokenRow; self-documenting by name"
)]
pub struct ModelTokenRow {
    pub model: String,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub session_count: u64,
}

/// Response for `GET /api/v1/metrics/tokens`. Canonical backend-wide token
/// telemetry, distinct from any process-local TUI accumulator.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[expect(
    missing_docs,
    reason = "fields mirror pylon's TokenMetricsResponse; self-documenting by name"
)]
pub struct TokenMetricsResponse {
    pub series: Vec<TokenSeriesPoint>,
    pub agents: Vec<AgentTokenRow>,
    pub models: Vec<ModelTokenRow>,
    pub today_input: u64,
    pub today_output: u64,
    pub week_input: u64,
    pub week_output: u64,
    pub month_input: u64,
    pub month_output: u64,
    pub prev_today_input: u64,
    pub prev_today_output: u64,
    pub prev_week_input: u64,
    pub prev_week_output: u64,
    pub prev_month_input: u64,
    pub prev_month_output: u64,
}

/// A single cost time-series point.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[expect(
    missing_docs,
    reason = "fields mirror pylon's CostSeriesPoint; self-documenting by name"
)]
pub struct CostSeriesPoint {
    pub date: String,
    pub cost_usd: f64,
}

/// Per-agent estimated cost row.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[expect(
    missing_docs,
    reason = "fields mirror pylon's AgentCostRow; self-documenting by name"
)]
pub struct AgentCostRow {
    pub id: String,
    pub name: String,
    pub total_cost: f64,
    pub message_count: u64,
    pub session_count: u64,
    pub output_tokens: u64,
    pub prev_period_cost: f64,
}

/// A named metric currently unbacked by any data source, surfaced instead of
/// silently rendering as zero.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[expect(
    missing_docs,
    reason = "fields mirror pylon's UnavailableMetric; self-documenting by name"
)]
pub struct UnavailableMetric {
    pub metric: String,
    pub reason: String,
}

/// Response for `GET /api/v1/metrics/costs`. Canonical backend-wide cost
/// telemetry, distinct from any process-local TUI accumulator.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[expect(
    missing_docs,
    reason = "fields mirror pylon's CostMetricsResponse; self-documenting by name"
)]
pub struct CostMetricsResponse {
    pub series: Vec<CostSeriesPoint>,
    pub agents: Vec<AgentCostRow>,
    pub today_cost: f64,
    pub week_cost: f64,
    pub month_cost: f64,
    pub prev_today_cost: f64,
    pub prev_week_cost: f64,
    pub prev_month_cost: f64,
    #[serde(default)]
    pub data_unavailable: Vec<UnavailableMetric>,
}
