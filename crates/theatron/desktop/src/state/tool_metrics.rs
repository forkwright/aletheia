//! Tool usage metrics state: aggregated stats, stores, and helpers.

use std::collections::HashMap;

use crate::state::fetch::FetchState;

// -- API response types -------------------------------------------------------

/// Top-level response from `/api/tool-stats`.
///
/// All fields use `#[serde(default)]` so partial responses degrade gracefully
/// whether the server returns pre-aggregated stats or raw invocation logs.
#[derive(Debug, Clone, Default, PartialEq, serde::Deserialize)]
pub(crate) struct ToolStatsResponse {
    #[serde(default)]
    pub summary: ToolSummary,
    #[serde(default)]
    pub tools: Vec<ToolStat>,
    #[serde(default)]
    pub time_series: Vec<TimeSeriesBucket>,
    #[serde(default)]
    pub invocations: Vec<ToolInvocation>,
}

/// Aggregate summary values for summary cards.
#[derive(Debug, Clone, Default, PartialEq, serde::Deserialize)]
pub(crate) struct ToolSummary {
    #[serde(default)]
    pub total_invocations_today: u64,
    #[serde(default)]
    pub total_invocations_week: u64,
    #[serde(default)]
    pub total_invocations_month: u64,
    /// Absolute delta vs. prior period (positive = more calls).
    #[serde(default)]
    pub delta_today: i64,
    #[serde(default)]
    pub delta_week: i64,
    #[serde(default)]
    pub delta_month: i64,
    /// Overall success rate [0.0, 1.0].
    #[serde(default)]
    pub success_rate: f64,
    /// Previous-period success rate for trend comparison.
    #[serde(default)]
    pub success_rate_prev: f64,
    /// Average execution duration across all tools (ms).
    #[serde(default)]
    pub avg_duration_ms: u64,
    /// Previous-period average duration for trend comparison.
    #[serde(default)]
    pub avg_duration_prev_ms: u64,
    #[serde(default)]
    pub most_used_tool: String,
    #[serde(default)]
    pub most_used_count: u64,
}

/// Per-tool aggregated statistics.
#[derive(Debug, Clone, Default, PartialEq, serde::Deserialize)]
pub(crate) struct ToolStat {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub total: u64,
    #[serde(default)]
    pub succeeded: u64,
    #[serde(default)]
    pub failed: u64,
    #[serde(default)]
    pub min_ms: u64,
    #[serde(default)]
    pub p25_ms: u64,
    #[serde(default)]
    pub p50_ms: u64,
    #[serde(default)]
    pub p75_ms: u64,
    #[serde(default)]
    pub p95_ms: u64,
    #[serde(default)]
    pub max_ms: u64,
    #[serde(default)]
    pub most_common_error: Option<String>,
    #[serde(default)]
    pub last_failure_at: Option<String>,
}

/// A single time-series bucket (one date, counts per tool).
#[derive(Debug, Clone, Default, PartialEq, serde::Deserialize)]
pub(crate) struct TimeSeriesBucket {
    #[serde(default)]
    pub date: String,
    /// Map of tool_name → invocation count in this bucket.
    #[serde(default)]
    pub counts: HashMap<String, u64>,
}

/// A single raw invocation record.
#[derive(Debug, Clone, Default, PartialEq, serde::Deserialize)]
pub(crate) struct ToolInvocation {
    #[serde(default)]
    pub tool_name: String,
    #[serde(default)]
    pub agent_id: String,
    #[serde(default)]
    pub timestamp: String,
    #[serde(default)]
    pub duration_ms: u64,
    #[serde(default)]
    pub success: bool,
    #[serde(default)]
    pub error: Option<String>,
}

// -- UI state types -----------------------------------------------------------

/// Time period selector shared across all metrics tabs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum DateRange {
    #[default]
    Last7Days,
    Last30Days,
    Last90Days,
}

impl DateRange {
    pub(crate) fn days(self) -> u32 {
        match self {
            Self::Last7Days => 7,
            Self::Last30Days => 30,
            Self::Last90Days => 90,
        }
    }

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Last7Days => "7d",
            Self::Last30Days => "30d",
            Self::Last90Days => "90d",
        }
    }
}

/// Combined store for the tool metrics view.
#[derive(Debug, Clone)]
pub(crate) struct ToolMetricsStore {
    pub data: FetchState<ToolStatsResponse>,
    pub date_range: DateRange,
    /// Tool selected for drill-down; `None` means overview.
    pub selected_tool: Option<String>,
}

impl Default for ToolMetricsStore {
    fn default() -> Self {
        Self {
            data: FetchState::Loading,
            date_range: DateRange::default(),
            selected_tool: None,
        }
    }
}

// -- Aggregation helpers ------------------------------------------------------

/// Returns the top `limit` tools sorted by total invocations, plus an optional
/// aggregated "Other" entry covering all remaining tools.
pub(crate) fn top_tools(tools: &[ToolStat], limit: usize) -> (Vec<&ToolStat>, Option<ToolStat>) {
    let mut sorted: Vec<&ToolStat> = tools.iter().collect();
    sorted.sort_by(|a, b| b.total.cmp(&a.total));

    if sorted.len() <= limit {
        return (sorted, None);
    }

    let (top, rest) = sorted.split_at(limit);
    let other = ToolStat {
        name: "Other".to_string(),
        total: rest.iter().map(|t| t.total).sum(),
        succeeded: rest.iter().map(|t| t.succeeded).sum(),
        failed: rest.iter().map(|t| t.failed).sum(),
        ..Default::default()
    };
    (top.to_vec(), Some(other))
}

/// Tools sorted by failure count (most failures first), for the results view.
pub(crate) fn tools_by_failure(tools: &[ToolStat]) -> Vec<&ToolStat> {
    let mut sorted: Vec<&ToolStat> = tools.iter().collect();
    sorted.sort_by(|a, b| b.failed.cmp(&a.failed));
    sorted
}

/// Tools sorted by median duration (slowest first), for the duration view.
pub(crate) fn tools_by_duration(tools: &[ToolStat]) -> Vec<&ToolStat> {
    let mut sorted: Vec<&ToolStat> = tools.iter().collect();
    sorted.sort_by(|a, b| b.p50_ms.cmp(&a.p50_ms));
    sorted
}

/// Failure rate as a percentage [0.0, 100.0].
pub(crate) fn failure_rate(stat: &ToolStat) -> f64 {
    if stat.total == 0 {
        return 0.0;
    }
    #[expect(
        clippy::cast_precision_loss,
        reason = "display-only: sub-percent precision irrelevant"
    )]
    let rate = stat.failed as f64 / stat.total as f64 * 100.0;
    rate
}

/// Trend arrow: ↑ if current is >1% above prev, ↓ if >1% below, → otherwise.
pub(crate) fn trend_arrow(current: f64, prev: f64) -> &'static str {
    if prev == 0.0 {
        return "→";
    }
    let ratio = current / prev;
    if ratio > 1.01 {
        "↑"
    } else if ratio < 0.99 {
        "↓"
    } else {
        "→"
    }
}

/// Formats a delta value with + or − prefix.
pub(crate) fn format_delta(delta: i64) -> String {
    if delta >= 0 {
        format!("+{delta}")
    } else {
        format!("{delta}")
    }
}

/// Formats a duration in milliseconds as a human-readable string.
pub(crate) fn format_duration_ms(ms: u64) -> String {
    if ms >= 60_000 {
        format!("{:.1}m", ms as f64 / 60_000.0) // kanon:ignore RUST/as-cast
    } else if ms >= 1_000 {
        format!("{:.1}s", ms as f64 / 1_000.0) // kanon:ignore RUST/as-cast
    } else {
        format!("{ms}ms")
    }
}

/// Paginate a slice: returns items for page `page` with `per_page` items each.
pub(crate) fn paginate<T>(items: &[T], page: usize, per_page: usize) -> &[T] {
    let start = page * per_page;
    if start >= items.len() {
        return &[];
    }
    let end = (start + per_page).min(items.len());
    &items[start..end]
}

/// Total number of pages for `total_items` items at `per_page` per page.
pub(crate) fn page_count(total_items: usize, per_page: usize) -> usize {
    if per_page == 0 {
        return 0;
    }
    total_items.div_ceil(per_page)
}

/// Nearest-rank percentile: index = ceil(p * N) - 1 (clamped).
///
/// Used in tests and available for client-side percentile computation from
/// raw invocation logs when the server doesn't return pre-aggregated stats.
#[expect(dead_code, reason = "used in tests; reserved for raw-log path")]
pub(crate) fn percentile_nearest_rank(sorted_values: &[u64], p: f64) -> u64 {
    if sorted_values.is_empty() {
        return 0;
    }
    #[expect(
        clippy::cast_precision_loss,
        reason = "display-only: fractional index is fine for N < 2^53"
    )]
    let rank = (p * sorted_values.len() as f64).ceil() as usize;
    let idx = rank.saturating_sub(1).min(sorted_values.len() - 1);
    sorted_values[idx]
}

// -- Tests --------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn make_stat(name: &str, total: u64, succeeded: u64, p50_ms: u64) -> ToolStat {
        ToolStat {
            name: name.to_string(),
            total,
            succeeded,
            failed: total.saturating_sub(succeeded),
            p50_ms,
            ..Default::default()
        }
    }

    #[test]
    fn top_tools_within_limit_no_other() {
        let tools = vec![
            make_stat("bash", 500, 490, 200),
            make_stat("read", 300, 295, 100),
        ];
        let (top, other) = top_tools(&tools, 10);
        assert_eq!(top.len(), 2);
        assert!(other.is_none());
    }

    #[test]
    fn top_tools_beyond_limit_creates_other() {
        let tools: Vec<ToolStat> = (0..12)
            .map(|i| make_stat(&format!("tool_{i}"), (12 - i) as u64 * 10, 0, 0))
            .collect();
        let (top, other) = top_tools(&tools, 10);
        assert_eq!(top.len(), 10);
        let other = other.expect("should have Other entry");
        assert_eq!(other.name, "Other");
        // sum of ranks 10 and 11 (values 20 and 10)
        assert_eq!(other.total, 30);
    }

    #[test]
    fn top_tools_sorted_by_count_descending() {
        let tools = vec![
            make_stat("low", 10, 10, 0),
            make_stat("high", 500, 490, 0),
            make_stat("mid", 100, 90, 0),
        ];
        let (top, _) = top_tools(&tools, 10);
        assert_eq!(top[0].name, "high");
        assert_eq!(top[1].name, "mid");
        assert_eq!(top[2].name, "low");
    }

    #[test]
    fn failure_rate_zero_when_no_calls() {
        let stat = make_stat("bash", 0, 0, 0);
        assert_eq!(failure_rate(&stat), 0.0);
    }

    #[test]
    fn failure_rate_percentage() {
        let stat = make_stat("bash", 100, 75, 0);
        assert!((failure_rate(&stat) - 25.0).abs() < 0.001);
    }

    #[test]
    fn trend_arrow_directions() {
        assert_eq!(trend_arrow(110.0, 100.0), "↑");
        assert_eq!(trend_arrow(90.0, 100.0), "↓");
        assert_eq!(trend_arrow(100.5, 100.0), "→");
    }

    #[test]
    fn percentile_nearest_rank_median() {
        let values = &[1u64, 2, 3, 4, 5, 6, 7, 8, 9, 10];
        // p50 of 10 values: ceil(0.5*10)=5, idx=4, value=5
        assert_eq!(percentile_nearest_rank(values, 0.50), 5);
    }

    #[test]
    fn percentile_nearest_rank_p95() {
        let values: Vec<u64> = (1..=100).collect();
        // p95 of 100: ceil(0.95*100)=95, idx=94, value=95
        assert_eq!(percentile_nearest_rank(&values, 0.95), 95);
    }

    #[test]
    fn paginate_first_page() {
        let items: Vec<u32> = (0..50).collect();
        let page = paginate(&items, 0, 20);
        assert_eq!(page.len(), 20);
        assert_eq!(page[0], 0);
    }

    #[test]
    fn paginate_last_partial_page() {
        let items: Vec<u32> = (0..25).collect();
        let page = paginate(&items, 1, 20);
        assert_eq!(page.len(), 5);
    }

    #[test]
    fn page_count_exact_multiple() {
        assert_eq!(page_count(40, 20), 2);
    }

    #[test]
    fn page_count_partial_last_page() {
        assert_eq!(page_count(41, 20), 3);
    }
}
