//! Metrics state: token usage, cost tracking, and budget management.

// -- Enums --------------------------------------------------------------------

/// Time granularity for metric series.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub(crate) enum Granularity {
    #[default]
    Daily,
    Weekly,
    Monthly,
}

impl Granularity {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Daily => "Daily",
            Self::Weekly => "Weekly",
            Self::Monthly => "Monthly",
        }
    }

    pub(crate) fn url_param(self) -> &'static str {
        match self {
            Self::Daily => "daily",
            Self::Weekly => "weekly",
            Self::Monthly => "monthly",
        }
    }
}

/// Preset date range for metric queries.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[non_exhaustive]
pub(crate) enum DateRange {
    #[default]
    Last7Days,
    Last30Days,
    Last90Days,
    Custom { from: String, to: String },
}

impl DateRange {
    pub(crate) fn label(&self) -> &'static str {
        match self {
            Self::Last7Days => "7 days",
            Self::Last30Days => "30 days",
            Self::Last90Days => "90 days",
            Self::Custom { .. } => "Custom",
        }
    }

    /// Compute (from, to) date strings for API query parameters.
    pub(crate) fn to_query_dates(&self) -> (String, String) {
        match self {
            Self::Custom { from, to } => (from.clone(), to.clone()),
            Self::Last7Days => (date_minus_days(7), today_date_str()),
            Self::Last30Days => (date_minus_days(30), today_date_str()),
            Self::Last90Days => (date_minus_days(90), today_date_str()),
        }
    }
}

/// Top-level tab selection for the metrics view.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum MetricsTab {
    #[default]
    Tokens,
    Costs,
}

/// Sort direction for data tables.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum SortDir {
    #[default]
    Desc,
    Asc,
}

impl SortDir {
    pub(crate) fn flip(self) -> Self {
        match self {
            Self::Asc => Self::Desc,
            Self::Desc => Self::Asc,
        }
    }
}

/// Sort column for the agent token table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum AgentTokenSort {
    #[default]
    Total,
    Name,
    Input,
    Output,
    PctOfTotal,
    AvgPerSession,
}

/// Sort column for the agent cost table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum AgentCostSort {
    #[default]
    TotalCost,
    Name,
    CostPerSession,
    CostPerMessage,
    CostPer1k,
}

// -- API response types -------------------------------------------------------

/// A single data point in a token usage time series.
#[derive(Debug, Clone, PartialEq, Default, serde::Deserialize)]
pub(crate) struct TokenSeriesPoint {
    #[serde(default)]
    pub date: String,
    #[serde(default)]
    pub input_tokens: u64,
    #[serde(default)]
    pub output_tokens: u64,
}

impl TokenSeriesPoint {
    #[expect(dead_code, reason = "used when series filtering by agent/model is implemented")]
    pub(crate) fn total(&self) -> u64 {
        self.input_tokens.saturating_add(self.output_tokens)
    }
}

/// Per-agent token usage row from the metrics API.
#[derive(Debug, Clone, PartialEq, Default, serde::Deserialize)]
pub(crate) struct AgentTokenRow {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub input_tokens: u64,
    #[serde(default)]
    pub output_tokens: u64,
    #[serde(default)]
    pub session_count: u64,
}

impl AgentTokenRow {
    pub(crate) fn total(&self) -> u64 {
        self.input_tokens.saturating_add(self.output_tokens)
    }

    pub(crate) fn avg_per_session(&self) -> u64 {
        if self.session_count == 0 {
            0
        } else {
            self.total() / self.session_count
        }
    }

    #[expect(
        clippy::cast_precision_loss,
        reason = "display-only: percentage rounded for rendering"
    )]
    pub(crate) fn pct_of_total(&self, grand_total: u64) -> f64 {
        if grand_total == 0 {
            0.0
        } else {
            self.total() as f64 / grand_total as f64 * 100.0
        }
    }
}

/// Per-model token usage row from the metrics API.
#[derive(Debug, Clone, PartialEq, Default, serde::Deserialize)]
pub(crate) struct ModelTokenRow {
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub input_tokens: u64,
    #[serde(default)]
    pub output_tokens: u64,
    #[serde(default)]
    pub session_count: u64,
}

impl ModelTokenRow {
    pub(crate) fn total(&self) -> u64 {
        self.input_tokens.saturating_add(self.output_tokens)
    }

    #[expect(
        clippy::cast_precision_loss,
        reason = "display-only: percentage rounded for rendering"
    )]
    pub(crate) fn pct_of_total(&self, grand_total: u64) -> f64 {
        if grand_total == 0 {
            0.0
        } else {
            self.total() as f64 / grand_total as f64 * 100.0
        }
    }
}

/// Full token metrics API response envelope.
#[derive(Debug, Clone, PartialEq, Default, serde::Deserialize)]
pub(crate) struct TokenMetricsResponse {
    #[serde(default)]
    pub series: Vec<TokenSeriesPoint>,
    #[serde(default)]
    pub agents: Vec<AgentTokenRow>,
    #[serde(default)]
    pub models: Vec<ModelTokenRow>,
    #[serde(default)]
    pub today_input: u64,
    #[serde(default)]
    pub today_output: u64,
    #[serde(default)]
    pub week_input: u64,
    #[serde(default)]
    pub week_output: u64,
    #[serde(default)]
    pub month_input: u64,
    #[serde(default)]
    pub month_output: u64,
    #[serde(default)]
    pub prev_today_input: u64,
    #[serde(default)]
    pub prev_today_output: u64,
    #[serde(default)]
    pub prev_week_input: u64,
    #[serde(default)]
    pub prev_week_output: u64,
    #[serde(default)]
    pub prev_month_input: u64,
    #[serde(default)]
    pub prev_month_output: u64,
}

impl TokenMetricsResponse {
    pub(crate) fn today_total(&self) -> u64 {
        self.today_input.saturating_add(self.today_output)
    }

    pub(crate) fn week_total(&self) -> u64 {
        self.week_input.saturating_add(self.week_output)
    }

    pub(crate) fn month_total(&self) -> u64 {
        self.month_input.saturating_add(self.month_output)
    }

    pub(crate) fn prev_today_total(&self) -> u64 {
        self.prev_today_input.saturating_add(self.prev_today_output)
    }

    pub(crate) fn prev_week_total(&self) -> u64 {
        self.prev_week_input.saturating_add(self.prev_week_output)
    }

    pub(crate) fn prev_month_total(&self) -> u64 {
        self.prev_month_input.saturating_add(self.prev_month_output)
    }

    pub(crate) fn grand_total_tokens(&self) -> u64 {
        self.agents.iter().map(|a| a.total()).sum()
    }
}

/// A single data point in a cost time series.
#[derive(Debug, Clone, PartialEq, Default, serde::Deserialize)]
pub(crate) struct CostSeriesPoint {
    #[serde(default)]
    pub date: String,
    #[serde(default)]
    pub cost_usd: f64,
}

/// Per-agent cost row from the costs API.
#[derive(Debug, Clone, PartialEq, Default, serde::Deserialize)]
pub(crate) struct AgentCostRow {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub total_cost: f64,
    #[serde(default)]
    pub message_count: u64,
    #[serde(default)]
    pub session_count: u64,
    #[serde(default)]
    pub output_tokens: u64,
    /// Cost from the previous equivalent period (for comparison chart).
    #[serde(default)]
    pub prev_period_cost: f64,
}

impl AgentCostRow {
    pub(crate) fn cost_per_session(&self) -> f64 {
        if self.session_count == 0 {
            0.0
        } else {
            self.total_cost / self.session_count as f64
        }
    }

    pub(crate) fn cost_per_message(&self) -> f64 {
        if self.message_count == 0 {
            0.0
        } else {
            self.total_cost / self.message_count as f64
        }
    }

    pub(crate) fn cost_per_1k_output(&self) -> f64 {
        if self.output_tokens == 0 {
            0.0
        } else {
            self.total_cost / (self.output_tokens as f64 / 1000.0)
        }
    }
}

/// Full cost metrics API response envelope.
#[derive(Debug, Clone, PartialEq, Default, serde::Deserialize)]
pub(crate) struct CostMetricsResponse {
    #[serde(default)]
    pub series: Vec<CostSeriesPoint>,
    #[serde(default)]
    pub agents: Vec<AgentCostRow>,
    #[serde(default)]
    pub today_cost: f64,
    #[serde(default)]
    pub week_cost: f64,
    #[serde(default)]
    pub month_cost: f64,
    #[serde(default)]
    pub prev_today_cost: f64,
    #[serde(default)]
    pub prev_week_cost: f64,
    #[serde(default)]
    pub prev_month_cost: f64,
}

// -- Budget config (local-only) -----------------------------------------------

/// Monthly spend budget configured locally.
#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct BudgetConfig {
    /// Monthly limit in USD. Zero means no budget set.
    pub monthly_limit_usd: f64,
}

// -- Computed display types ---------------------------------------------------

/// Summary card data: current value with delta vs previous period.
#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct SummaryDelta {
    pub value: f64,
    pub delta_pct: f64,
    pub is_up: bool,
}

// -- Helper functions ---------------------------------------------------------

/// Compute a summary delta from current and previous u64 values.
#[expect(
    clippy::cast_precision_loss,
    reason = "display-only: sub-unit precision irrelevant for delta"
)]
pub(crate) fn compute_delta_u64(current: u64, previous: u64) -> SummaryDelta {
    compute_delta_f64(current as f64, previous as f64)
}

/// Compute a summary delta from current and previous float values.
pub(crate) fn compute_delta_f64(current: f64, previous: f64) -> SummaryDelta {
    let delta_pct = if previous == 0.0 {
        0.0
    } else {
        ((current - previous) / previous) * 100.0
    };
    SummaryDelta {
        value: current,
        delta_pct: delta_pct.abs(),
        is_up: current >= previous,
    }
}

/// Budget progress as a clamped percentage (0–100).
pub(crate) fn budget_progress_pct(spent: f64, limit: f64) -> f64 {
    if limit <= 0.0 {
        0.0
    } else {
        (spent / limit * 100.0).min(100.0)
    }
}

/// Color for budget progress bar based on spend percentage.
pub(crate) fn budget_bar_color(pct: f64) -> &'static str {
    if pct >= 90.0 {
        "#ef4444"
    } else if pct >= 70.0 {
        "#eab308"
    } else {
        "#22c55e"
    }
}

/// Project month-end spend via linear extrapolation.
///
/// Returns 0.0 if `day_of_month` is 0 (guard against division by zero).
pub(crate) fn project_month_end(
    current_spend: f64,
    day_of_month: u32,
    days_in_month: u32,
) -> f64 {
    if day_of_month == 0 || days_in_month == 0 {
        return 0.0;
    }
    let daily_rate = current_spend / f64::from(day_of_month);
    daily_rate * f64::from(days_in_month)
}

// -- Display formatters -------------------------------------------------------

/// Format a token count with K/M suffix.
#[expect(
    clippy::cast_precision_loss,
    reason = "display-only: sub-token precision irrelevant"
)]
pub(crate) fn format_tokens(count: u64) -> String {
    const K: u64 = 1_000;
    const M: u64 = 1_000_000;
    if count >= M {
        format!("{:.1}M", count as f64 / M as f64)
    } else if count >= K {
        format!("{:.1}K", count as f64 / K as f64)
    } else {
        count.to_string()
    }
}

/// Format a USD cost value.
pub(crate) fn format_cost(value: f64) -> String {
    format!("${value:.2}")
}

// -- Date helpers -------------------------------------------------------------

/// Today's date as an ISO-8601 string (YYYY-MM-DD).
pub(crate) fn today_date_str() -> String {
    let (y, m, d) = epoch_secs_to_ymd(unix_seconds());
    format!("{y:04}-{m:02}-{d:02}")
}

/// A date `offset` days before today as an ISO-8601 string.
pub(crate) fn date_minus_days(offset: u32) -> String {
    let secs = unix_seconds().saturating_sub(u64::from(offset) * 86400);
    let (y, m, d) = epoch_secs_to_ymd(secs);
    format!("{y:04}-{m:02}-{d:02}")
}

/// Current day of month (1-based).
pub(crate) fn day_of_month_today() -> u32 {
    let (_, _, d) = epoch_secs_to_ymd(unix_seconds());
    d
}

/// Number of days in the current calendar month.
pub(crate) fn days_in_current_month() -> u32 {
    let (y, m, _) = epoch_secs_to_ymd(unix_seconds());
    days_in_month(y, m)
}

fn unix_seconds() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Convert Unix seconds to (year, month, day) using the civil calendar algorithm.
///
/// INVARIANT: Algorithm from Howard Hinnant's date library. Correct for all
/// dates representable in a u64 Unix timestamp.
fn epoch_secs_to_ymd(secs: u64) -> (u32, u32, u32) {
    let days = (secs / 86400) as i64;
    let z = days + 719468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y as u32, m as u32, d as u32)
}

fn days_in_month(year: u32, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            if year % 4 == 0 && (year % 100 != 0 || year % 400 == 0) {
                29
            } else {
                28
            }
        }
        _ => 30,
    }
}

// -- Pricing table (client-side fallback) -------------------------------------

/// Cost per 1K output tokens for a model (USD).
///
/// NOTE: Pricing as of 2026-03. Used only when the API does not return costs.
pub(crate) fn cost_per_1k_output(model: &str) -> f64 {
    if model.contains("opus-4") {
        0.075
    } else if model.contains("sonnet-4") {
        0.015
    } else if model.contains("haiku-4") {
        0.00125
    } else if model.contains("opus-3") {
        0.060
    } else if model.contains("sonnet-3-5") || model.contains("sonnet-3.5") {
        0.015
    } else if model.contains("sonnet-3") {
        0.015
    } else if model.contains("haiku-3") {
        0.00125
    } else {
        0.015
    }
}

// -- Color helpers ------------------------------------------------------------

/// Consistent accent color for an agent by list position.
pub(crate) fn agent_color(index: usize) -> &'static str {
    const PALETTE: &[&str] = &[
        "#5b6af0", "#10b981", "#f59e0b", "#f43f5e",
        "#0ea5e9", "#8b5cf6", "#ec4899", "#14b8a6",
    ];
    PALETTE[index % PALETTE.len()]
}

/// Accent color for a model name.
pub(crate) fn model_color(model: &str) -> &'static str {
    if model.contains("opus") {
        "#8b5cf6"
    } else if model.contains("sonnet") {
        "#5b6af0"
    } else if model.contains("haiku") {
        "#10b981"
    } else {
        "#9A7B4F"
    }
}

// -- Sort helpers -------------------------------------------------------------

/// Sort agent token rows in-place.
pub(crate) fn sort_agent_token_rows(
    rows: &mut Vec<AgentTokenRow>,
    col: AgentTokenSort,
    dir: SortDir,
    grand_total: u64,
) {
    rows.sort_by(|a, b| {
        let cmp = match col {
            AgentTokenSort::Name => a.name.cmp(&b.name),
            AgentTokenSort::Total => a.total().cmp(&b.total()),
            AgentTokenSort::Input => a.input_tokens.cmp(&b.input_tokens),
            AgentTokenSort::Output => a.output_tokens.cmp(&b.output_tokens),
            AgentTokenSort::PctOfTotal => a
                .pct_of_total(grand_total)
                .partial_cmp(&b.pct_of_total(grand_total))
                .unwrap_or(std::cmp::Ordering::Equal),
            AgentTokenSort::AvgPerSession => a.avg_per_session().cmp(&b.avg_per_session()),
        };
        if dir == SortDir::Desc {
            cmp.reverse()
        } else {
            cmp
        }
    });
}

/// Sort agent cost rows in-place.
pub(crate) fn sort_agent_cost_rows(
    rows: &mut Vec<AgentCostRow>,
    col: AgentCostSort,
    dir: SortDir,
) {
    rows.sort_by(|a, b| {
        let cmp = match col {
            AgentCostSort::Name => a.name.cmp(&b.name),
            AgentCostSort::TotalCost => a
                .total_cost
                .partial_cmp(&b.total_cost)
                .unwrap_or(std::cmp::Ordering::Equal),
            AgentCostSort::CostPerSession => a
                .cost_per_session()
                .partial_cmp(&b.cost_per_session())
                .unwrap_or(std::cmp::Ordering::Equal),
            AgentCostSort::CostPerMessage => a
                .cost_per_message()
                .partial_cmp(&b.cost_per_message())
                .unwrap_or(std::cmp::Ordering::Equal),
            AgentCostSort::CostPer1k => a
                .cost_per_1k_output()
                .partial_cmp(&b.cost_per_1k_output())
                .unwrap_or(std::cmp::Ordering::Equal),
        };
        if dir == SortDir::Desc {
            cmp.reverse()
        } else {
            cmp
        }
    });
}

// -- Tests --------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compute_delta_up_direction() {
        let d = compute_delta_f64(120.0, 100.0);
        assert!(d.is_up, "120 vs 100 should be up");
        assert!((d.delta_pct - 20.0).abs() < 0.01, "delta must be 20%");
    }

    #[test]
    fn compute_delta_down_direction() {
        let d = compute_delta_f64(80.0, 100.0);
        assert!(!d.is_up, "80 vs 100 should be down");
        assert!((d.delta_pct - 20.0).abs() < 0.01, "delta magnitude must be 20%");
    }

    #[test]
    fn compute_delta_zero_previous() {
        let d = compute_delta_f64(50.0, 0.0);
        assert_eq!(d.delta_pct, 0.0, "zero previous must return 0% delta");
    }

    #[test]
    fn budget_progress_normal() {
        let pct = budget_progress_pct(70.0, 100.0);
        assert!((pct - 70.0).abs() < 0.01, "70/100 must be 70%");
    }

    #[test]
    fn budget_progress_capped_at_100() {
        let pct = budget_progress_pct(150.0, 100.0);
        assert!((pct - 100.0).abs() < 0.01, "overshoot must cap at 100%");
    }

    #[test]
    fn budget_progress_zero_limit() {
        let pct = budget_progress_pct(50.0, 0.0);
        assert_eq!(pct, 0.0, "zero limit must return 0%");
    }

    #[test]
    fn budget_bar_color_green() {
        assert_eq!(budget_bar_color(50.0), "#22c55e");
    }

    #[test]
    fn budget_bar_color_amber() {
        assert_eq!(budget_bar_color(75.0), "#eab308");
    }

    #[test]
    fn budget_bar_color_red() {
        assert_eq!(budget_bar_color(95.0), "#ef4444");
    }

    #[test]
    fn project_month_end_linear() {
        let projected = project_month_end(15.0, 15, 30);
        assert!((projected - 30.0).abs() < 0.01, "half-month should project to double");
    }

    #[test]
    fn project_month_end_zero_day() {
        assert_eq!(project_month_end(10.0, 0, 30), 0.0);
    }

    #[test]
    fn format_tokens_units() {
        assert_eq!(format_tokens(500), "500");
        assert_eq!(format_tokens(1500), "1.5K");
        assert_eq!(format_tokens(2_500_000), "2.5M");
    }

    #[test]
    fn format_cost_two_decimals() {
        assert_eq!(format_cost(1.5), "$1.50");
        assert_eq!(format_cost(0.001), "$0.00");
    }

    #[test]
    fn sort_dir_flip() {
        assert_eq!(SortDir::Asc.flip(), SortDir::Desc);
        assert_eq!(SortDir::Desc.flip(), SortDir::Asc);
    }

    #[test]
    fn agent_token_row_totals() {
        let row = AgentTokenRow {
            input_tokens: 1000,
            output_tokens: 500,
            session_count: 5,
            ..Default::default()
        };
        assert_eq!(row.total(), 1500);
        assert_eq!(row.avg_per_session(), 300);
    }

    #[test]
    fn agent_cost_row_efficiency() {
        let row = AgentCostRow {
            total_cost: 1.0,
            output_tokens: 10_000,
            session_count: 2,
            message_count: 10,
            ..Default::default()
        };
        assert!((row.cost_per_1k_output() - 0.1).abs() < 0.001);
        assert!((row.cost_per_session() - 0.5).abs() < 0.001);
        assert!((row.cost_per_message() - 0.1).abs() < 0.001);
    }
}
