//! Token estimation and budget management.

/// Character-based token estimator: 1 token ≈ N characters (ceiling division).
///
/// Conservative estimate suitable for budget planning. Actual token counts
/// from the Anthropic API will be lower, giving natural headroom.
/// `chars_per_token` is configurable via `agents.defaults.chars_per_token`
/// in `aletheia.toml`; the default from `koina::defaults::CHARS_PER_TOKEN`
/// preserves prior behaviour.
pub struct CharEstimator {
    pub(crate) chars_per_token: u64,
}

impl CharEstimator {
    /// Create an estimator with an explicit characters-per-token divisor.
    #[must_use]
    pub fn new(chars_per_token: u64) -> Self {
        Self { chars_per_token }
    }

    /// The configured characters-per-token divisor.
    ///
    /// Exposed so cache keys (e.g. the bootstrap file cache) can detect when
    /// a cached token estimate was computed against a different estimator.
    #[must_use]
    pub fn chars_per_token(&self) -> u64 {
        self.chars_per_token
    }

    /// Estimate the number of tokens in the given text.
    #[must_use]
    pub fn estimate(&self, text: &str) -> u64 {
        #[expect(
            clippy::as_conversions,
            reason = "usize→u64: text length always fits in u64"
        )]
        {
            (text.len() as u64).div_ceil(self.chars_per_token) // kanon:ignore RUST/as-cast
        }
    }
}

impl Default for CharEstimator {
    fn default() -> Self {
        Self {
            chars_per_token: u64::from(koina::defaults::CHARS_PER_TOKEN),
        }
    }
}

/// Token budget for a single turn's system prompt assembly.
///
/// Partitions the model's context window into three zones:
/// - **System budget**: for bootstrap content (SOUL.md, USER.md, etc.)
/// - **History budget**: for conversation history
/// - **Turn reserve**: for output tokens and extended thinking
///
/// The system budget is capped at `bootstrap_cap` (from `NousConfig::bootstrap_max_tokens`).
#[derive(Debug, Clone)]
pub struct TokenBudget {
    context_window: u64,
    reserved_for_turn: u64,
    reserved_for_history: u64,
    system_budget: u64,
    consumed: u64,
}

impl TokenBudget {
    /// Create a new token budget.
    ///
    /// - `context_window`: total context window tokens (e.g. 200,000)
    /// - `history_ratio`: fraction of window reserved for history (e.g. 0.6)
    /// - `turn_reserve`: tokens reserved for output + thinking
    /// - `bootstrap_cap`: hard cap from `NousConfig::bootstrap_max_tokens`
    #[must_use]
    pub fn new(
        context_window: u64,
        history_ratio: f64,
        turn_reserve: u64,
        bootstrap_cap: u64,
    ) -> Self {
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            clippy::cast_precision_loss,
            clippy::as_conversions,
            reason = "u64→f64→u64: context_window fits in f64 mantissa for practical model sizes"
        )]
        let reserved_for_history = (context_window as f64 * history_ratio) as u64; // kanon:ignore RUST/as-cast
        let computed = context_window
            .saturating_sub(turn_reserve)
            .saturating_sub(reserved_for_history);
        let system_budget = computed.min(bootstrap_cap);

        Self {
            context_window,
            reserved_for_turn: turn_reserve,
            reserved_for_history,
            system_budget,
            consumed: 0,
        }
    }

    /// Remaining tokens available for bootstrap content.
    #[must_use]
    pub fn remaining(&self) -> u64 {
        self.system_budget.saturating_sub(self.consumed)
    }

    /// Try to consume tokens. Returns `false` if budget would be exceeded.
    pub fn consume(&mut self, tokens: u64) -> bool {
        if self.consumed + tokens > self.system_budget {
            return false;
        }
        self.consumed += tokens;
        true
    }

    /// Consume tokens unconditionally, even when the system budget is exhausted.
    ///
    /// Use this for content that must be included regardless of budget (e.g. Required
    /// sections, file-ref expansion debt) so that downstream callers — history
    /// allocation, recall sizing — see the accurate over-budget amount via
    /// [`Self::adjusted_history_budget`] and [`Self::remaining`].
    pub fn force_consume(&mut self, tokens: u64) {
        self.consumed = self.consumed.saturating_add(tokens);
    }

    /// Tokens reserved for conversation history, reduced by any over-budget debt.
    ///
    /// When bootstrap content exceeds `system_budget` (Required-section inclusion
    /// or file-ref expansion), the excess is charged against the history reserve so
    /// that the total allocation stays within the context window.
    #[must_use]
    pub fn adjusted_history_budget(&self) -> u64 {
        let debt = self.consumed.saturating_sub(self.system_budget);
        self.reserved_for_history.saturating_sub(debt)
    }

    /// Check if the given number of tokens fits in the remaining budget.
    #[must_use]
    pub fn can_fit(&self, tokens: u64) -> bool {
        self.consumed + tokens <= self.system_budget
    }

    /// Total tokens consumed so far.
    #[must_use]
    pub fn consumed(&self) -> u64 {
        self.consumed
    }

    /// The system budget cap (maximum tokens for bootstrap).
    #[must_use]
    pub fn system_budget(&self) -> u64 {
        self.system_budget
    }

    /// Tokens reserved for conversation history.
    #[must_use]
    pub fn history_budget(&self) -> u64 {
        self.reserved_for_history
    }

    /// Total context window size.
    #[must_use]
    pub fn context_window(&self) -> u64 {
        self.context_window
    }

    /// Tokens reserved for turn output.
    #[must_use]
    pub fn turn_reserve(&self) -> u64 {
        self.reserved_for_turn
    }
}

/// Tracks token reclamation across compaction passes.
///
/// Records pre-compaction and post-compaction token counts so callers
/// can measure how much context space was freed. Used by both
/// microcompaction (in-place clearing) and full compaction (summarization).
#[derive(Debug, Clone, Default)]
pub struct CompactionMetrics {
    /// Token count before compaction.
    pub pre_compact_tokens: u64,
    /// Token count after compaction.
    pub post_compact_tokens: u64,
    /// Number of tool results cleared by microcompaction.
    pub results_cleared: u32,
    /// Number of tool results preserved (last-N or unexpired).
    pub results_preserved: u32,
    /// Whether full compaction was triggered.
    pub full_compaction_triggered: bool,
}

impl CompactionMetrics {
    /// Tokens reclaimed by compaction.
    #[must_use]
    pub fn tokens_reclaimed(&self) -> u64 {
        self.pre_compact_tokens
            .saturating_sub(self.post_compact_tokens)
    }
}

use std::time::{Duration, Instant};

use crate::config::StageBudget;

/// Tracks wall-clock time per pipeline stage and enforces limits.
pub(crate) struct TimeBudget {
    pipeline_start: Instant,
    stage_budgets: StageBudget,
    stage_elapsed: Vec<StageTimingRecord>,
    current_stage: Option<(String, Instant)>,
}

/// Timing record for a completed pipeline stage.
#[derive(Debug, Clone)]
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "consumed via summary() in tests and future telemetry"
    )
)]
pub(crate) struct StageTimingRecord {
    /// Stage name (e.g. "context", "execute").
    pub name: String,
    /// Wall-clock time the stage consumed.
    pub elapsed: Duration,
    /// Whether the stage completed normally, timed out, or was skipped.
    pub status: StageTimingStatus,
}

/// How a pipeline stage completed.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum StageTimingStatus {
    Completed,
    /// Stage exceeded its time limit and was cut short.
    TimedOut,
    /// Stage was not executed (e.g. total budget exhausted).
    #[expect(dead_code, reason = "reserved for future skip-stage telemetry")]
    Skipped,
}

impl TimeBudget {
    /// Create a new time budget from per-stage limits.
    #[must_use]
    pub(crate) fn new(stage_budgets: StageBudget) -> Self {
        Self {
            pipeline_start: Instant::now(),
            stage_budgets,
            stage_elapsed: Vec::with_capacity(6),
            current_stage: None,
        }
    }

    /// Returns `true` if the total pipeline time budget has been exceeded.
    #[must_use]
    pub(crate) fn total_exceeded(&self) -> bool {
        if self.stage_budgets.total_secs == 0 {
            return false;
        }
        self.pipeline_start.elapsed()
            >= Duration::from_secs(u64::from(self.stage_budgets.total_secs))
    }

    /// Wall-clock time remaining before the total pipeline budget expires.
    #[must_use]
    pub(crate) fn total_remaining(&self) -> Duration {
        if self.stage_budgets.total_secs == 0 {
            return Duration::from_secs(u64::MAX);
        }
        let total = Duration::from_secs(u64::from(self.stage_budgets.total_secs));
        total.saturating_sub(self.pipeline_start.elapsed())
    }

    /// Absolute deadline for the named stage, capped by total remaining time.
    ///
    /// Returns `None` if both the stage-specific and total budgets are unlimited.
    #[must_use]
    pub(crate) fn stage_deadline(&self, stage_name: &str) -> Option<Instant> {
        self.stage_limit(stage_name)
            .map(|limit| Instant::now() + limit)
    }

    /// Maximum duration for the named stage, capped by total remaining time.
    ///
    /// Returns `None` if both the stage-specific and total budgets are unlimited.
    #[must_use]
    pub(crate) fn stage_limit(&self, stage_name: &str) -> Option<Duration> {
        let stage_secs = match stage_name {
            "context" => self.stage_budgets.context_secs,
            "recall" => self.stage_budgets.recall_secs,
            "history" | "full_compact" => self.stage_budgets.history_secs,
            "guard" => self.stage_budgets.guard_secs,
            "execute" => self.stage_budgets.execute_secs,
            "finalize" => self.stage_budgets.finalize_secs,
            "reflection" => self.stage_budgets.reflection_secs,
            _ => 0,
        };
        if stage_secs == 0 && self.stage_budgets.total_secs == 0 {
            return None;
        }
        let stage_limit = if stage_secs > 0 {
            Duration::from_secs(u64::from(stage_secs))
        } else {
            Duration::from_secs(u64::MAX)
        };
        Some(stage_limit.min(self.total_remaining()))
    }

    /// Elapsed time of the currently-active stage, if any.
    #[must_use]
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "exposed for test assertions and future observability"
        )
    )]
    pub(crate) fn current_stage_elapsed(&self) -> Option<Duration> {
        self.current_stage
            .as_ref()
            .map(|(_, start)| start.elapsed())
    }

    /// Returns `true` if the currently-active stage has exceeded its limit.
    #[must_use]
    pub(crate) fn stage_exceeded(&self, stage_name: &str) -> bool {
        if let Some((ref name, start)) = self.current_stage
            && name == stage_name
        {
            let elapsed = start.elapsed();
            if let Some(limit) = self.stage_limit(stage_name) {
                return elapsed >= limit;
            }
        }
        false
    }

    /// Mark the start of a pipeline stage for timing.
    pub(crate) fn begin_stage(&mut self, name: &str) {
        self.current_stage = Some((name.to_owned(), Instant::now()));
    }

    /// Record the current stage as finished with the given status.
    pub(crate) fn end_stage(&mut self, status: StageTimingStatus) {
        if let Some((name, start)) = self.current_stage.take() {
            self.stage_elapsed.push(StageTimingRecord {
                name,
                elapsed: start.elapsed(),
                status,
            });
        }
    }

    /// Completed stage timing records, in execution order.
    #[must_use]
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "exposed for test assertions and future observability"
        )
    )]
    pub(crate) fn summary(&self) -> &[StageTimingRecord] {
        &self.stage_elapsed
    }

    /// Total wall-clock time since the pipeline started.
    #[must_use]
    pub(crate) fn total_elapsed(&self) -> Duration {
        self.pipeline_start.elapsed()
    }
}

#[expect(
    clippy::indexing_slicing,
    reason = "test: vec indices are valid after asserting len"
)]
#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions may panic on failure")]
mod tests {
    use super::*;

    #[test]
    fn char_estimator_empty_string() {
        assert_eq!(CharEstimator::default().estimate(""), 0);
    }

    #[test]
    fn char_estimator_exact_multiple() {
        assert_eq!(CharEstimator::default().estimate("abcdefgh"), 2);
    }

    #[test]
    fn char_estimator_rounds_up() {
        assert_eq!(CharEstimator::default().estimate("hello"), 2);
        assert_eq!(CharEstimator::default().estimate("a"), 1);
        assert_eq!(CharEstimator::default().estimate("abc"), 1);
    }

    #[test]
    fn char_estimator_single_char() {
        assert_eq!(CharEstimator::default().estimate("x"), 1);
    }

    #[test]
    fn budget_computes_system_budget() {
        let budget = TokenBudget::new(200_000, 0.6, 16_384, 40_000);
        assert_eq!(budget.system_budget(), 40_000);
        assert_eq!(budget.remaining(), 40_000);
        assert_eq!(budget.consumed(), 0);
    }

    #[test]
    fn budget_remaining_decreases() {
        let mut budget = TokenBudget::new(200_000, 0.6, 16_384, 40_000);
        assert!(budget.consume(10_000));
        assert_eq!(budget.remaining(), 30_000);
        assert_eq!(budget.consumed(), 10_000);
    }

    #[test]
    fn budget_consume_returns_false_on_overflow() {
        let mut budget = TokenBudget::new(200_000, 0.6, 16_384, 40_000);
        assert!(budget.consume(35_000));
        assert!(!budget.consume(10_000));
        assert_eq!(budget.consumed(), 35_000);
        assert_eq!(budget.remaining(), 5_000);
    }

    #[test]
    fn budget_can_fit_boundary() {
        let mut budget = TokenBudget::new(100_000, 0.0, 0, 50_000);
        assert!(budget.consume(49_999));
        assert!(budget.can_fit(1));
        assert!(!budget.can_fit(2));
    }

    #[test]
    fn budget_saturating_sub_prevents_underflow() {
        let budget = TokenBudget::new(1000, 0.5, 2000, 500);
        assert_eq!(budget.system_budget(), 0);
        assert_eq!(budget.remaining(), 0);
    }

    #[test]
    fn budget_cap_limits_system_budget() {
        let budget = TokenBudget::new(100_000, 0.0, 0, 5_000);
        assert_eq!(budget.system_budget(), 5_000);
    }

    #[test]
    fn budget_consumed_tracks_total() {
        let mut budget = TokenBudget::new(100_000, 0.0, 0, 50_000);
        assert!(budget.consume(1000));
        assert!(budget.consume(2000));
        assert!(budget.consume(3000));
        assert_eq!(budget.consumed(), 6000);
        assert_eq!(budget.remaining(), 44_000);
    }

    #[test]
    fn char_estimator_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<CharEstimator>();
    }

    #[test]
    fn time_budget_not_exceeded_initially() {
        let tb = TimeBudget::new(StageBudget::default());
        assert!(!tb.total_exceeded());
    }

    #[test]
    fn time_budget_unlimited_when_zero() {
        let tb = TimeBudget::new(StageBudget {
            total_secs: 0,
            ..StageBudget::default()
        });
        assert!(!tb.total_exceeded());
        assert!(tb.total_remaining() > Duration::from_secs(1_000_000));
    }

    #[test]
    fn stage_limit_none_when_both_zero() {
        let tb = TimeBudget::new(StageBudget {
            execute_secs: 0,
            total_secs: 0,
            ..StageBudget::default()
        });
        assert!(tb.stage_limit("execute").is_none());
    }

    #[test]
    fn stage_limit_capped_by_total() {
        let tb = TimeBudget::new(StageBudget {
            recall_secs: 999,
            total_secs: 10,
            ..StageBudget::default()
        });
        let limit = tb.stage_limit("recall").unwrap();
        assert!(limit <= Duration::from_secs(10));
    }

    #[test]
    fn begin_end_stage_records() {
        let mut tb = TimeBudget::new(StageBudget::default());
        tb.begin_stage("context");
        tb.end_stage(StageTimingStatus::Completed);
        assert_eq!(tb.summary().len(), 1);
        assert_eq!(tb.summary()[0].name, "context");
        assert_eq!(tb.summary()[0].status, StageTimingStatus::Completed);
    }

    #[test]
    fn stage_budget_serde_roundtrip() {
        let sb = StageBudget::default();
        let json = serde_json::to_string(&sb).unwrap();
        let back: StageBudget = serde_json::from_str(&json).unwrap();
        assert_eq!(back.total_secs, 300);
        assert_eq!(back.recall_secs, 15);
    }

    #[test]
    fn time_budget_is_send() {
        fn assert_send<T: Send>() {}
        assert_send::<TimeBudget>();
    }

    #[test]
    fn force_consume_tracks_over_budget_tokens() {
        // WHY(#4623): force_consume must record tokens even when system_budget
        // is already exhausted, so downstream stages see the true remaining budget.
        let mut budget = TokenBudget::new(200_000, 0.6, 16_384, 100);
        // consume exactly the system_budget
        assert!(budget.consume(100));
        assert_eq!(budget.remaining(), 0);
        // force_consume pushes consumed past the cap
        budget.force_consume(50);
        assert_eq!(budget.consumed(), 150);
        // remaining saturates at 0 — never underflows
        assert_eq!(budget.remaining(), 0);
    }

    #[test]
    fn adjusted_history_budget_deducts_over_budget_debt() {
        // WHY(#4623): When bootstrap overruns system_budget (Required sections or
        // file-ref expansion), the excess must be deducted from history_budget so
        // downstream stages allocate against the real available window.
        let mut budget = TokenBudget::new(200_000, 0.6, 16_384, 100);
        let original_history = budget.history_budget();
        // force_consume 200 tokens into a 100-token cap — 100-token debt
        budget.force_consume(200);
        let debt = 200_u64.saturating_sub(100);
        assert_eq!(
            budget.adjusted_history_budget(),
            original_history.saturating_sub(debt),
            "history budget must be reduced by the over-budget debt amount"
        );
    }

    #[test]
    fn adjusted_history_budget_equals_history_budget_when_within_cap() {
        let mut budget = TokenBudget::new(200_000, 0.6, 16_384, 40_000);
        assert!(budget.consume(10_000));
        assert_eq!(
            budget.adjusted_history_budget(),
            budget.history_budget(),
            "adjusted_history_budget must equal history_budget when consumption is within cap"
        );
    }

    #[test]
    fn stage_timing_record_fields_are_readable() {
        let mut tb = TimeBudget::new(StageBudget::default());
        tb.begin_stage("context");
        std::thread::sleep(Duration::from_millis(1));
        tb.end_stage(StageTimingStatus::Completed);
        let summary = tb.summary();
        assert!(!summary.is_empty());
        assert_eq!(summary[0].name, "context");
        assert!(summary[0].elapsed > Duration::ZERO);
        assert_eq!(summary[0].status, StageTimingStatus::Completed);
    }

    #[test]
    fn current_stage_elapsed_returns_some_when_active() {
        let mut tb = TimeBudget::new(StageBudget::default());
        tb.begin_stage("execute");
        std::thread::sleep(Duration::from_millis(1));
        let elapsed = tb.current_stage_elapsed();
        assert!(elapsed.is_some_and(|d| d > Duration::ZERO));
        tb.end_stage(StageTimingStatus::TimedOut);
        assert!(tb.current_stage_elapsed().is_none());
    }
}
