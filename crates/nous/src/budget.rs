//! Token estimation and budget management.

/// Estimate token count from text.
///
/// Implementations must be `Send + Sync` for use across async boundaries.
/// The default [`CharEstimator`] uses a character-based heuristic.
/// Future implementations can wrap tiktoken or the Anthropic token counting API.
pub trait TokenEstimator: Send + Sync {
    /// Estimate the number of tokens in the given text.
    fn estimate(&self, text: &str) -> u64;
}

/// Character-based token estimator: 1 token ≈ 4 characters (ceiling division).
///
/// Conservative estimate suitable for budget planning. Actual token counts
/// from the Anthropic API will be lower, giving natural headroom.
pub struct CharEstimator;

impl TokenEstimator for CharEstimator {
    fn estimate(&self, text: &str) -> u64 {
        (text.len() as u64).div_ceil(4)
    }
}

/// Token budget for a single turn's system prompt assembly.
///
/// Partitions the model's context window into three zones:
/// - **System budget**: for bootstrap content (SOUL.md, USER.md, etc.)
/// - **History budget**: for conversation history
/// - **Turn reserve**: for output tokens and extended thinking
///
/// The system budget is capped at `bootstrap_cap` (from [`NousConfig::bootstrap_max_tokens`]).
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
            reason = "context_window fits in f64 mantissa for practical model sizes"
        )]
        let reserved_for_history = (context_window as f64 * history_ratio) as u64;
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

#[cfg(test)]
mod tests {
    use super::*;

    // --- CharEstimator ---

    #[test]
    fn char_estimator_empty_string() {
        assert_eq!(CharEstimator.estimate(""), 0);
    }

    #[test]
    fn char_estimator_exact_multiple() {
        // 8 chars / 4 = 2 tokens
        assert_eq!(CharEstimator.estimate("abcdefgh"), 2);
    }

    #[test]
    fn char_estimator_rounds_up() {
        // 5 chars -> ceil(5/4) = 2
        assert_eq!(CharEstimator.estimate("hello"), 2);
        // 1 char -> ceil(1/4) = 1
        assert_eq!(CharEstimator.estimate("a"), 1);
        // 3 chars -> ceil(3/4) = 1
        assert_eq!(CharEstimator.estimate("abc"), 1);
    }

    #[test]
    fn char_estimator_single_char() {
        assert_eq!(CharEstimator.estimate("x"), 1);
    }

    // --- TokenBudget ---

    #[test]
    fn budget_computes_system_budget() {
        // 200k window, 0.6 history ratio (120k), 16k turn reserve
        // computed = 200k - 16k - 120k = 64k
        // cap = 40k -> system_budget = 40k
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
        // Try to consume 10k more when only 5k remaining
        assert!(!budget.consume(10_000));
        // Budget unchanged after failed consume
        assert_eq!(budget.consumed(), 35_000);
        assert_eq!(budget.remaining(), 5_000);
    }

    #[test]
    fn budget_can_fit_boundary() {
        let mut budget = TokenBudget::new(100_000, 0.0, 0, 50_000);
        assert!(budget.consume(49_999));
        // Exactly 1 token remaining
        assert!(budget.can_fit(1));
        assert!(!budget.can_fit(2));
    }

    #[test]
    fn budget_saturating_sub_prevents_underflow() {
        // turn_reserve larger than context_window
        let budget = TokenBudget::new(1000, 0.5, 2000, 500);
        assert_eq!(budget.system_budget(), 0);
        assert_eq!(budget.remaining(), 0);
    }

    #[test]
    fn budget_cap_limits_system_budget() {
        // computed = 100k - 0 - 0 = 100k, but cap = 5000
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

    // --- Static assertions ---

    #[test]
    fn char_estimator_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<CharEstimator>();
    }
}
