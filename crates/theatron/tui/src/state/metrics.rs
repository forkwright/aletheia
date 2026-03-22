//! Runtime metrics state for the metrics dashboard view.

use std::collections::HashMap;
use std::time::Instant;

use crate::id::NousId;

/// Maximum number of recent turns tracked for the sparkline.
const SPARKLINE_CAPACITY: usize = 30;

/// Per-turn token usage recorded for the sparkline.
#[derive(Debug, Clone, Copy)]
pub struct TurnTokens {
    pub(crate) input: u32,
    pub(crate) output: u32,
    pub(crate) cache_read: u32,
}

/// Cumulative token statistics for a single agent.
#[derive(Debug, Clone, Default)]
pub struct AgentMetrics {
    /// Number of completed turns.
    pub(crate) turns: u32,
    /// Total input tokens.
    pub(crate) input_tokens: u64,
    /// Total output tokens.
    pub(crate) output_tokens: u64,
    /// Total cache-read tokens.
    pub(crate) cache_read_tokens: u64,
}

/// Runtime state for the metrics dashboard view.
#[derive(Debug)]
pub struct MetricsState {
    /// When the TUI app started, for uptime calculation.
    pub(crate) started_at: Instant,
    /// Cumulative input tokens across all turns since startup.
    pub(crate) total_input_tokens: u64,
    /// Cumulative output tokens across all turns since startup.
    pub(crate) total_output_tokens: u64,
    /// Cumulative cache-read tokens across all turns since startup.
    pub(crate) total_cache_read_tokens: u64,
    /// Cumulative cache-write tokens across all turns since startup.
    pub(crate) total_cache_write_tokens: u64,
    /// Per-agent statistics keyed by agent ID.
    pub(crate) agent_stats: HashMap<NousId, AgentMetrics>,
    /// Recent turn token totals for the sparkline, capped at SPARKLINE_CAPACITY.
    pub(crate) turn_history: Vec<TurnTokens>,
    /// Whether the last health check returned OK.
    pub(crate) api_healthy: Option<bool>,
    /// Scroll offset in the per-agent table.
    pub(crate) scroll_offset: usize,
    /// Selected agent row index in the per-agent table.
    pub(crate) selected_agent: usize,
}

impl MetricsState {
    pub(crate) fn new() -> Self {
        Self {
            started_at: Instant::now(),
            total_input_tokens: 0,
            total_output_tokens: 0,
            total_cache_read_tokens: 0,
            total_cache_write_tokens: 0,
            agent_stats: HashMap::new(),
            turn_history: Vec::with_capacity(SPARKLINE_CAPACITY),
            api_healthy: None,
            scroll_offset: 0,
            selected_agent: 0,
        }
    }

    /// Record token usage from a completed turn and update the sparkline.
    pub(crate) fn record_turn(
        &mut self,
        nous_id: &NousId,
        input_tokens: u32,
        output_tokens: u32,
        cache_read_tokens: u32,
        cache_write_tokens: u32,
    ) {
        self.total_input_tokens += u64::from(input_tokens);
        self.total_output_tokens += u64::from(output_tokens);
        self.total_cache_read_tokens += u64::from(cache_read_tokens);
        self.total_cache_write_tokens += u64::from(cache_write_tokens);

        let stats = self.agent_stats.entry(nous_id.clone()).or_default();
        stats.turns += 1;
        stats.input_tokens += u64::from(input_tokens);
        stats.output_tokens += u64::from(output_tokens);
        stats.cache_read_tokens += u64::from(cache_read_tokens);

        let entry = TurnTokens {
            input: input_tokens,
            output: output_tokens,
            cache_read: cache_read_tokens,
        };
        if self.turn_history.len() >= SPARKLINE_CAPACITY {
            self.turn_history.remove(0);
        }
        self.turn_history.push(entry);
    }

    /// Cache hit rate as a value in 0.0–1.0.
    pub(crate) fn cache_hit_rate(&self) -> f64 {
        let total = self.total_input_tokens + self.total_cache_read_tokens;
        if total == 0 {
            return 0.0;
        }
        #[expect(
            clippy::cast_precision_loss,
            reason = "token counts fit comfortably in f64 mantissa at realistic usage levels"
        )]
        let rate = self.total_cache_read_tokens as f64 / total as f64;
        rate
    }

    /// Format a token count as "1.2M", "34.5k", or "34".
    pub(crate) fn format_tokens(tokens: u64) -> String {
        if tokens >= 1_000_000 {
            #[expect(
                clippy::cast_precision_loss,
                reason = "display formatting; sub-unit precision is not meaningful"
            )]
            return format!("{:.1}M", tokens as f64 / 1_000_000.0);
        }
        if tokens >= 1_000 {
            #[expect(
                clippy::cast_precision_loss,
                reason = "display formatting; sub-unit precision is not meaningful"
            )]
            return format!("{:.1}k", tokens as f64 / 1_000.0);
        }
        tokens.to_string()
    }

    /// Formatted uptime string: "2h 15m", "45m 30s", or "12s".
    pub(crate) fn uptime_string(&self) -> String {
        let secs = self.started_at.elapsed().as_secs();
        let hours = secs / 3600;
        let minutes = (secs % 3600) / 60;
        let seconds = secs % 60;
        if hours > 0 {
            format!("{hours}h {minutes}m")
        } else if minutes > 0 {
            format!("{minutes}m {seconds}s")
        } else {
            format!("{seconds}s")
        }
    }
}

/// Render a sparkline string using Unicode block characters for a series of values.
///
/// Returns a string of width `width` using chars from the block range ▁▂▃▄▅▆▇█.
pub(crate) fn sparkline(values: &[u32], width: usize) -> String {
    const BLOCKS: &[char] = &['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];

    if values.is_empty() || width == 0 {
        return " ".repeat(width);
    }

    let max = *values.iter().max().unwrap_or(&1);
    let max = max.max(1);

    // Downsample or pad the values to fit exactly `width` columns.
    let display: Vec<u32> = if values.len() >= width {
        // Subsample evenly.
        (0..width)
            .map(|i| {
                let src = i * values.len() / width;
                values.get(src).copied().unwrap_or(0)
            })
            .collect()
    } else {
        // Left-pad with zeros so the most recent value is at the right edge.
        let padding = width - values.len();
        let mut v: Vec<u32> = std::iter::repeat_n(0, padding).collect();
        v.extend_from_slice(values);
        v
    };

    display
        .iter()
        .map(|&v| {
            #[expect(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                reason = "clamped to 0..=7 before cast"
            )]
            let idx = ((v as f64 / max as f64) * 7.0).round().clamp(0.0, 7.0) as usize;
            BLOCKS[idx]
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_starts_with_zero_counts() {
        let s = MetricsState::new();
        assert_eq!(s.total_input_tokens, 0);
        assert_eq!(s.total_output_tokens, 0);
        assert_eq!(s.total_cache_read_tokens, 0);
        assert!(s.turn_history.is_empty());
    }

    #[test]
    fn record_turn_accumulates() {
        let mut s = MetricsState::new();
        let id: NousId = "agent1".into();
        s.record_turn(&id, 100, 50, 20, 5);
        assert_eq!(s.total_input_tokens, 100);
        assert_eq!(s.total_output_tokens, 50);
        assert_eq!(s.total_cache_read_tokens, 20);
        assert_eq!(s.turn_history.len(), 1);
    }

    #[test]
    fn record_turn_multiple_agents() {
        let mut s = MetricsState::new();
        let a: NousId = "a".into();
        let b: NousId = "b".into();
        s.record_turn(&a, 100, 50, 0, 0);
        s.record_turn(&b, 200, 80, 10, 0);
        assert_eq!(s.total_input_tokens, 300);
        assert_eq!(s.agent_stats[&a].turns, 1);
        assert_eq!(s.agent_stats[&b].turns, 1);
        assert_eq!(s.agent_stats[&b].input_tokens, 200);
    }

    #[test]
    fn sparkline_capacity_cap() {
        let mut s = MetricsState::new();
        let id: NousId = "a".into();
        for i in 0..40 {
            s.record_turn(&id, i, 0, 0, 0);
        }
        assert_eq!(s.turn_history.len(), 30);
    }

    #[test]
    fn cache_hit_rate_zero_when_no_tokens() {
        let s = MetricsState::new();
        assert!((s.cache_hit_rate() - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn cache_hit_rate_correct() {
        let mut s = MetricsState::new();
        let id: NousId = "a".into();
        s.record_turn(&id, 100, 50, 100, 0);
        // cache_read=100, total = input(100) + cache_read(100) = 200, rate = 50%
        assert!((s.cache_hit_rate() - 0.5).abs() < 0.001);
    }

    #[test]
    fn format_tokens_small() {
        assert_eq!(MetricsState::format_tokens(0), "0");
        assert_eq!(MetricsState::format_tokens(999), "999");
    }

    #[test]
    fn format_tokens_kilo() {
        assert_eq!(MetricsState::format_tokens(1500), "1.5k");
    }

    #[test]
    fn format_tokens_mega() {
        assert_eq!(MetricsState::format_tokens(2_000_000), "2.0M");
    }

    #[test]
    fn uptime_string_seconds() {
        let s = MetricsState::new();
        let uptime = s.uptime_string();
        assert!(
            uptime.ends_with('s'),
            "expected seconds format, got: {uptime}"
        );
    }

    #[test]
    fn sparkline_empty_values_returns_spaces() {
        let result = sparkline(&[], 10);
        assert_eq!(result.len(), 10);
        assert!(result.chars().all(|c| c == ' '));
    }

    #[test]
    fn sparkline_uniform_values_all_same_height() {
        let vals: Vec<u32> = vec![5, 5, 5, 5];
        let result = sparkline(&vals, 4);
        let chars: Vec<char> = result.chars().collect();
        assert_eq!(chars.len(), 4);
        assert!(chars.windows(2).all(|w| w[0] == w[1]));
    }

    #[test]
    fn sparkline_zero_width_returns_empty() {
        let result = sparkline(&[1, 2, 3], 0);
        assert_eq!(result, "");
    }

    #[test]
    fn sparkline_width_equals_values() {
        let vals: Vec<u32> = vec![1, 4, 2, 8, 5];
        let result = sparkline(&vals, 5);
        assert_eq!(result.chars().count(), 5);
    }

    #[test]
    fn sparkline_wider_than_values_pads_left() {
        let vals: Vec<u32> = vec![8, 8];
        let result = sparkline(&vals, 6);
        let chars: Vec<char> = result.chars().collect();
        assert_eq!(chars.len(), 6);
        // Left padding should be the lowest block (0 value → '▁')
        assert_eq!(chars[0], '▁');
        assert_eq!(chars[1], '▁');
        assert_eq!(chars[2], '▁');
        assert_eq!(chars[3], '▁');
    }
}
