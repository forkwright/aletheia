//! Property-based tests for token budget and estimation.
//!
//! Failing seeds are stored in `tests/proptest-regressions/proptest_budget.txt`
//! (created automatically by proptest on first failure).
//!
//! Run: `cargo test -p aletheia-nous --test proptest_budget`

use aletheia_nous::budget::{CharEstimator, TokenBudget, TokenEstimator};
use proptest::prelude::*;

// ── CharEstimator properties ──────────────────────────────────────────────────

proptest! {
    /// Estimate of empty string is always 0.
    #[test]
    fn char_estimator_empty_is_zero(chars_per_token in 1u64..=16u64) {
        let est = CharEstimator::new(chars_per_token);
        prop_assert_eq!(est.estimate(""), 0, "empty string must estimate to 0 tokens");
    }

    /// Estimate is monotone: longer text never estimates fewer tokens.
    #[test]
    fn char_estimator_monotone(
        base in ".*".prop_map(|s| s.chars().take(256).collect::<String>()),
        extra in "\\PC+".prop_map(|s| s.chars().take(64).collect::<String>()),
    ) {
        let est = CharEstimator::default();
        let base_tokens = est.estimate(&base);
        let extended = format!("{base}{extra}");
        let extended_tokens = est.estimate(&extended);
        prop_assert!(
            extended_tokens >= base_tokens,
            "estimate of extended string must be >= estimate of base string"
        );
    }

    /// Estimate never exceeds text.len() (one token per byte minimum).
    #[test]
    fn char_estimator_at_most_one_token_per_byte(
        text in ".*".prop_map(|s| s.chars().take(512).collect::<String>()),
    ) {
        let est = CharEstimator::default();
        let tokens = est.estimate(&text);
        #[expect(
            clippy::as_conversions,
            reason = "usize→u64: text.len() always fits in u64 for any realistic string"
        )]
        let len = text.len() as u64;
        prop_assert!(
            tokens <= len || text.is_empty(),
            "token estimate must not exceed byte length of text"
        );
    }
}

// ── TokenBudget properties ────────────────────────────────────────────────────

proptest! {
    /// `remaining` + `consumed` == `system_budget` always.
    #[test]
    fn token_budget_remaining_plus_consumed_is_budget(
        context in 1_000u64..=500_000u64,
        ratio in 0.0f64..=1.0f64,
        reserve in 0u64..=20_000u64,
        cap in 1u64..=80_000u64,
    ) {
        let budget = TokenBudget::new(context, ratio, reserve, cap);
        prop_assert_eq!(
            budget.remaining() + budget.consumed(),
            budget.system_budget(),
            "remaining + consumed must equal system_budget"
        );
    }

    /// Consuming exactly `remaining` empties the budget without rejecting.
    #[test]
    fn token_budget_consume_remaining_empties(
        context in 10_000u64..=200_000u64,
        ratio in 0.0f64..=0.8f64,
        reserve in 0u64..=10_000u64,
        cap in 1u64..=50_000u64,
    ) {
        let mut budget = TokenBudget::new(context, ratio, reserve, cap);
        let rem = budget.remaining();
        prop_assume!(rem > 0);
        let accepted = budget.consume(rem);
        prop_assert!(accepted, "consuming exactly remaining() must succeed");
        prop_assert_eq!(budget.remaining(), 0, "remaining() must be 0 after consuming all");
    }

    /// Consuming more than remaining always returns false.
    #[test]
    fn token_budget_overrun_rejected(
        context in 10_000u64..=200_000u64,
        ratio in 0.0f64..=0.8f64,
        reserve in 0u64..=10_000u64,
        cap in 1u64..=50_000u64,
    ) {
        let mut budget = TokenBudget::new(context, ratio, reserve, cap);
        let overrun = budget.remaining().saturating_add(1);
        let accepted = budget.consume(overrun);
        prop_assert!(!accepted, "consuming more than remaining() must be rejected");
    }

    /// `can_fit(n)` and `consume(n)` are consistent: can_fit == consume result.
    #[test]
    fn token_budget_can_fit_consistent_with_consume(
        context in 10_000u64..=200_000u64,
        ratio in 0.0f64..=0.8f64,
        reserve in 0u64..=5_000u64,
        cap in 1u64..=40_000u64,
        amount in 0u64..=50_000u64,
    ) {
        let mut budget = TokenBudget::new(context, ratio, reserve, cap);
        let fits = budget.can_fit(amount);
        let consumed = budget.consume(amount);
        prop_assert_eq!(
            fits, consumed,
            "can_fit() and consume() must agree on whether tokens fit in the budget"
        );
    }
}
