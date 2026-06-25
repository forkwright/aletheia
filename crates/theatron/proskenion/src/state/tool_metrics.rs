//! Tool usage metrics state: aggregated stats, stores, and helpers.

use std::{collections::HashMap, time::Duration};

// ── API response types ──

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
    // kanon:ignore RUST/primitive-for-domain-id — ToolInvocation agent_id mirrors the external API string identifier
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

// ── UI state types ──

/// Time period selector shared across all metrics tabs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[expect(
    clippy::enum_variant_names,
    reason = "Days suffix is semantically necessary for these time-period variants"
)]
pub(crate) enum DateRange {
    #[default]
    Last7Days,
    #[cfg_attr(not(test), expect(dead_code, reason = "reserved for future use"))]
    Last30Days,
    #[cfg_attr(not(test), expect(dead_code, reason = "reserved for future use"))]
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
}

/// Tools sorted by failure count (most failures first), for the results view.
pub(crate) fn tools_by_failure(tools: &[ToolStat]) -> Vec<&ToolStat> {
    let mut sorted: Vec<&ToolStat> = tools.iter().collect();
    sorted.sort_by_key(|b| std::cmp::Reverse(b.failed));
    sorted
}

/// Tools sorted by median duration (slowest first), for the duration view.
pub(crate) fn tools_by_duration(tools: &[ToolStat]) -> Vec<&ToolStat> {
    let mut sorted: Vec<&ToolStat> = tools.iter().collect();
    sorted.sort_by_key(|b| std::cmp::Reverse(b.p50_ms));
    sorted
}

/// Trend arrow: ↑ if current is >1% above prev, ↓ if >1% below, → otherwise.
#[cfg_attr(not(test), expect(dead_code, reason = "reserved for future use"))]
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
        format!("{:.1}m", Duration::from_millis(ms).as_secs_f64() / 60.0)
    } else if ms >= 1_000 {
        format!("{:.1}s", Duration::from_millis(ms).as_secs_f64())
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
    items.get(start..end).unwrap_or(&[])
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
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "used in tests; reserved for raw-log path")
)]
pub(crate) fn percentile_nearest_rank(sorted_values: &[u64], p: f64) -> u64 {
    if sorted_values.is_empty() {
        return 0;
    }
    #[expect(
        clippy::cast_precision_loss,
        reason = "display-only: fractional index is fine for N < 2^53"
    )]
    #[expect(
        clippy::as_conversions,
        reason = "length to f64 and rank to usize for percentile"
    )]
    let rank = (p * sorted_values.len() as f64).ceil() as usize;
    let idx = rank.saturating_sub(1).min(sorted_values.len() - 1);
    sorted_values.get(idx).copied().unwrap_or(0)
}

// ── Tests ──

#[cfg(test)]
mod tests {
    use super::*;

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
    fn percentile_nearest_rank_empty() {
        assert_eq!(percentile_nearest_rank(&[], 0.5), 0);
    }

    #[test]
    fn percentile_nearest_rank_single_value() {
        assert_eq!(percentile_nearest_rank(&[42], 0.5), 42);
        assert_eq!(percentile_nearest_rank(&[42], 0.95), 42);
    }

    #[test]
    fn paginate_beyond_end_returns_empty() {
        let items: Vec<u32> = (0..10).collect();
        let page = paginate(&items, 5, 20);
        assert!(page.is_empty());
    }

    #[test]
    fn paginate_zero_per_page_returns_empty() {
        let items: Vec<u32> = (0..10).collect();
        // start = 0 * 0 = 0, end = (0+0).min(10) = 0
        let page = paginate(&items, 0, 0);
        assert!(page.is_empty());
    }

    #[test]
    fn page_count_zero_per_page() {
        assert_eq!(page_count(50, 0), 0);
    }

    #[test]
    fn page_count_zero_items() {
        assert_eq!(page_count(0, 20), 0);
    }

    #[test]
    fn date_range_days_for_each_variant() {
        assert_eq!(DateRange::Last7Days.days(), 7);
        assert_eq!(DateRange::Last30Days.days(), 30);
        assert_eq!(DateRange::Last90Days.days(), 90);
    }

    #[test]
    fn date_range_default_is_7days() {
        assert_eq!(DateRange::default(), DateRange::Last7Days);
    }

    #[test]
    fn format_delta_positive() {
        assert_eq!(format_delta(0), "+0");
        assert_eq!(format_delta(5), "+5");
        assert_eq!(format_delta(1234), "+1234");
    }

    #[test]
    fn format_delta_negative() {
        assert_eq!(format_delta(-1), "-1");
        assert_eq!(format_delta(-100), "-100");
    }

    #[test]
    fn format_duration_ms_units() {
        assert_eq!(format_duration_ms(500), "500ms");
        assert_eq!(format_duration_ms(999), "999ms");
        assert_eq!(format_duration_ms(1000), "1.0s");
        assert_eq!(format_duration_ms(2500), "2.5s");
        assert_eq!(format_duration_ms(60_000), "1.0m");
        assert_eq!(format_duration_ms(120_000), "2.0m");
    }

    #[test]
    fn trend_arrow_up_down_stable() {
        // current > 1.01 * prev → up
        assert_eq!(trend_arrow(102.0, 100.0), "↑");
        // current < 0.99 * prev → down
        assert_eq!(trend_arrow(98.0, 100.0), "↓");
        // within ±1% → stable
        assert_eq!(trend_arrow(100.0, 100.0), "→");
        assert_eq!(trend_arrow(100.5, 100.0), "→");
    }

    #[test]
    fn trend_arrow_zero_prev_stable() {
        assert_eq!(trend_arrow(50.0, 0.0), "→");
    }

    #[test]
    fn tools_by_failure_descending() {
        let tools = vec![
            ToolStat {
                name: "low".to_string(),
                failed: 1,
                ..Default::default()
            },
            ToolStat {
                name: "high".to_string(),
                failed: 100,
                ..Default::default()
            },
            ToolStat {
                name: "mid".to_string(),
                failed: 10,
                ..Default::default()
            },
        ];
        let sorted = tools_by_failure(&tools);
        assert_eq!(sorted[0].name, "high");
        assert_eq!(sorted[1].name, "mid");
        assert_eq!(sorted[2].name, "low");
    }

    #[test]
    fn tools_by_duration_descending_by_p50() {
        let tools = vec![
            ToolStat {
                name: "fast".to_string(),
                p50_ms: 5,
                ..Default::default()
            },
            ToolStat {
                name: "slow".to_string(),
                p50_ms: 500,
                ..Default::default()
            },
        ];
        let sorted = tools_by_duration(&tools);
        assert_eq!(sorted[0].name, "slow");
    }

    #[test]
    fn tools_by_failure_empty_input() {
        let sorted = tools_by_failure(&[]);
        assert!(sorted.is_empty());
    }

    #[test]
    fn tool_stats_response_deserializes_partial() {
        let json = r#"{"summary": {"total_invocations_today": 10, "success_rate": 0.95}}"#;
        let resp: ToolStatsResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.summary.total_invocations_today, 10);
        assert!((resp.summary.success_rate - 0.95).abs() < 0.001);
        assert!(resp.tools.is_empty());
        assert!(resp.invocations.is_empty());
    }

    #[test]
    fn tool_stats_response_deserializes_empty_object() {
        let resp: ToolStatsResponse = serde_json::from_str("{}").unwrap();
        assert_eq!(resp.summary.total_invocations_today, 0);
        assert!(resp.tools.is_empty());
    }

    #[test]
    fn tool_stat_default_zero_values() {
        let s = ToolStat::default();
        assert_eq!(s.total, 0);
        assert_eq!(s.succeeded, 0);
        assert_eq!(s.failed, 0);
        assert!(s.most_common_error.is_none());
    }

    #[test]
    fn tool_invocation_default_unsuccessful() {
        let inv = ToolInvocation::default();
        assert!(!inv.success);
        assert!(inv.error.is_none());
    }

    #[test]
    fn time_series_bucket_counts_deserialize() {
        let json = r#"{"date": "2024-01-01", "counts": {"web_search": 5, "file_read": 2}}"#;
        let bucket: TimeSeriesBucket = serde_json::from_str(json).unwrap();
        assert_eq!(bucket.date, "2024-01-01");
        assert_eq!(bucket.counts.get("web_search"), Some(&5));
        assert_eq!(bucket.counts.get("file_read"), Some(&2));
    }
}
