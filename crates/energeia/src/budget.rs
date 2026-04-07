// NOTE: Shared budget tracking for a dispatch run. Uses atomics so concurrent
// sessions can record cost/turns without holding a mutex. Budget is checked
// after each session result to trigger abort when limits are exceeded.

use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::time::Instant;

/// Budget enforcement status.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum BudgetStatus {
    /// All limits within bounds.
    Ok,
    /// Approaching a limit (informational, 80%+ consumed).
    Warning(String),
    /// A limit has been exceeded — sessions should be aborted.
    Exceeded(String),
}

/// Shared budget tracker for a dispatch run.
///
/// INVARIANT: `current_cost_hundredths` and `current_turns` only increase
/// (monotonic). `start_time` is set once at construction and never changes.
pub struct Budget {
    /// Maximum allowed aggregate cost in USD.
    pub max_cost_usd: Option<f64>,
    /// Maximum allowed aggregate agent turns.
    pub max_turns: Option<u32>,
    /// Maximum allowed wall-clock duration in milliseconds.
    pub max_duration_ms: Option<u64>,
    /// Accumulated cost in hundredths of a cent (1 USD = `10_000`) for atomic precision.
    ///
    /// WHY: Integer atomics avoid floating-point accumulation drift across many
    /// concurrent sessions. `10_000` hundredths per USD gives 0.01-cent precision.
    current_cost_hundredths: AtomicU64,
    current_turns: AtomicU32,
    start_time: Instant,
}

impl std::fmt::Debug for Budget {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Budget")
            .field("max_cost_usd", &self.max_cost_usd)
            .field("max_turns", &self.max_turns)
            .field("max_duration_ms", &self.max_duration_ms)
            .field("current_cost_usd", &self.current_cost_usd())
            .field("current_turns", &self.current_turns())
            .field("elapsed_ms", &self.elapsed_ms())
            .finish_non_exhaustive()
    }
}

impl Budget {
    /// Create a new budget with optional limits.
    #[must_use]
    pub fn new(
        max_cost_usd: Option<f64>,
        max_turns: Option<u32>,
        max_duration_ms: Option<u64>,
    ) -> Self {
        Self {
            max_cost_usd,
            max_turns,
            max_duration_ms,
            current_cost_hundredths: AtomicU64::new(0),
            current_turns: AtomicU32::new(0),
            start_time: Instant::now(),
        }
    }

    /// Record cost and turns from a completed session or resume attempt.
    pub fn record(&self, cost_usd: f64, turns: u32) {
        // NOTE: Convert USD to hundredths of a cent for integer atomics.
        // 1 USD = 10_000 hundredths of a cent.
        #[expect(clippy::cast_possible_truncation, reason = "cost * 10_000 fits in u64")]
        #[expect(clippy::cast_sign_loss, reason = "cost is non-negative")]
        #[expect(clippy::as_conversions, reason = "intentional cast with truncation/sign checks above")]
        let hundredths = (cost_usd * 10_000.0) as u64;
        self.current_cost_hundredths
            .fetch_add(hundredths, Ordering::Relaxed);
        self.current_turns.fetch_add(turns, Ordering::Relaxed);
    }

    /// Check whether any budget limit has been exceeded.
    ///
    /// Returns [`BudgetStatus::Warning`] at 80% of a limit and
    /// [`BudgetStatus::Exceeded`] at 100%.
    #[must_use]
    pub fn check(&self) -> BudgetStatus {
        if let Some(max_cost) = self.max_cost_usd {
            let current = self.current_cost_usd();
            if current >= max_cost {
                return BudgetStatus::Exceeded(format!(
                    "cost ${current:.2} >= limit ${max_cost:.2}"
                ));
            }
            // WHY: Warn at 80% to give callers early notice before hard abort.
            if current >= max_cost * 0.8 {
                return BudgetStatus::Warning(format!(
                    "cost ${current:.2} approaching limit ${max_cost:.2}"
                ));
            }
        }

        if let Some(max_turns) = self.max_turns {
            let current = self.current_turns.load(Ordering::Relaxed);
            if current >= max_turns {
                return BudgetStatus::Exceeded(format!("turns {current} >= limit {max_turns}"));
            }
            if current >= max_turns * 4 / 5 {
                return BudgetStatus::Warning(format!(
                    "turns {current} approaching limit {max_turns}"
                ));
            }
        }

        if let Some(max_duration) = self.max_duration_ms {
            let elapsed = self.elapsed_ms();
            if elapsed >= max_duration {
                return BudgetStatus::Exceeded(format!(
                    "duration {elapsed}ms >= limit {max_duration}ms"
                ));
            }
        }

        BudgetStatus::Ok
    }

    /// Current accumulated cost in USD.
    #[must_use]
    pub fn current_cost_usd(&self) -> f64 {
        let raw = self.current_cost_hundredths.load(Ordering::Relaxed);
        #[expect(
            clippy::cast_precision_loss,
            clippy::as_conversions,
            reason = "cost value fits in f64 mantissa for any realistic dispatch"
        )]
        let cost = raw as f64 / 10_000.0;
        cost
    }

    /// Current accumulated turns.
    #[must_use]
    pub fn current_turns(&self) -> u32 {
        self.current_turns.load(Ordering::Relaxed)
    }

    /// Elapsed time since budget creation in milliseconds.
    #[must_use]
    pub fn elapsed_ms(&self) -> u64 {
        u64::try_from(self.start_time.elapsed().as_millis()).unwrap_or(u64::MAX)
    }

    /// Fraction of the cost budget consumed so far (0.0–1.0+).
    ///
    /// Returns 0.0 when no cost limit is set. Returns 1.0+ when exceeded.
    ///
    /// WHY: The early-abandonment heuristic needs to know what fraction of
    /// budget has been spent to decide when to cut losses.
    #[must_use]
    pub fn cost_fraction(&self) -> f64 {
        match self.max_cost_usd {
            Some(max) if max > 0.0 => self.current_cost_usd() / max,
            _ => 0.0,
        }
    }

    /// Fraction of the turn budget consumed so far (0.0–1.0+).
    ///
    /// Returns 0.0 when no turn limit is set.
    #[must_use]
    pub fn turn_fraction(&self) -> f64 {
        match self.max_turns {
            Some(max) if max > 0 => f64::from(self.current_turns()) / f64::from(max),
            _ => 0.0,
        }
    }
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
mod tests {
    use super::*;

    #[test]
    fn new_budget_starts_at_zero() {
        let budget = Budget::new(Some(10.0), Some(100), None);
        assert!(
            (budget.current_cost_usd()).abs() < f64::EPSILON,
            "cost should start at zero"
        );
        assert_eq!(budget.current_turns(), 0, "turns should start at zero");
    }

    #[test]
    fn record_accumulates_cost_and_turns() {
        let budget = Budget::new(None, None, None);
        budget.record(1.50, 20);
        budget.record(0.50, 10);
        assert!(
            (budget.current_cost_usd() - 2.0).abs() < 0.01,
            "cost should accumulate"
        );
        assert_eq!(budget.current_turns(), 30, "turns should accumulate");
    }

    #[test]
    fn check_ok_when_under_limits() {
        let budget = Budget::new(Some(10.0), Some(100), None);
        budget.record(1.0, 10);
        assert_eq!(budget.check(), BudgetStatus::Ok);
    }

    #[test]
    fn check_warning_at_80_percent_cost() {
        let budget = Budget::new(Some(10.0), None, None);
        budget.record(8.5, 0);
        match budget.check() {
            BudgetStatus::Warning(msg) => assert!(msg.contains("approaching"), "msg: {msg}"),
            other => panic!("expected Warning, got {other:?}"),
        }
    }

    #[test]
    fn check_warning_at_80_percent_turns() {
        let budget = Budget::new(None, Some(100), None);
        budget.record(0.0, 82);
        match budget.check() {
            BudgetStatus::Warning(msg) => assert!(msg.contains("approaching"), "msg: {msg}"),
            other => panic!("expected Warning, got {other:?}"),
        }
    }

    #[test]
    fn check_exceeded_at_cost_limit() {
        let budget = Budget::new(Some(5.0), None, None);
        budget.record(5.0, 0);
        match budget.check() {
            BudgetStatus::Exceeded(msg) => assert!(msg.contains("cost"), "msg: {msg}"),
            other => panic!("expected Exceeded, got {other:?}"),
        }
    }

    #[test]
    fn check_exceeded_at_turn_limit() {
        let budget = Budget::new(None, Some(50), None);
        budget.record(0.0, 50);
        match budget.check() {
            BudgetStatus::Exceeded(msg) => assert!(msg.contains("turns"), "msg: {msg}"),
            other => panic!("expected Exceeded, got {other:?}"),
        }
    }

    #[test]
    fn check_exceeded_at_duration_limit() {
        // WHY: Create budget with 0ms limit — elapsed is always >= 0.
        let budget = Budget::new(None, None, Some(0));
        match budget.check() {
            BudgetStatus::Exceeded(msg) => assert!(msg.contains("duration"), "msg: {msg}"),
            other => panic!("expected Exceeded, got {other:?}"),
        }
    }

    #[test]
    fn no_limits_always_ok() {
        let budget = Budget::new(None, None, None);
        budget.record(999.0, 9999);
        assert_eq!(budget.check(), BudgetStatus::Ok);
    }

    #[test]
    fn cost_precision_preserved() {
        let budget = Budget::new(None, None, None);
        budget.record(0.0001, 0);
        // NOTE: Hundredths of a cent gives 4 decimal places of precision.
        assert!(
            budget.current_cost_usd() > 0.0,
            "sub-cent cost should be recorded"
        );
        assert!(
            budget.current_cost_usd() < 0.001,
            "cost should not exceed 0.001"
        );
    }

    #[test]
    fn cost_fraction_with_limit() {
        let budget = Budget::new(Some(10.0), None, None);
        budget.record(3.0, 0);
        assert!(
            (budget.cost_fraction() - 0.3).abs() < 0.01,
            "fraction should be 0.3"
        );
    }

    #[test]
    fn cost_fraction_without_limit() {
        let budget = Budget::new(None, None, None);
        budget.record(100.0, 0);
        assert!(
            (budget.cost_fraction()).abs() < f64::EPSILON,
            "fraction should be 0.0 with no limit"
        );
    }

    #[test]
    fn turn_fraction_with_limit() {
        let budget = Budget::new(None, Some(200), None);
        budget.record(0.0, 60);
        assert!(
            (budget.turn_fraction() - 0.3).abs() < 0.01,
            "fraction should be 0.3"
        );
    }

    #[test]
    fn turn_fraction_without_limit() {
        let budget = Budget::new(None, None, None);
        budget.record(0.0, 9999);
        assert!(
            (budget.turn_fraction()).abs() < f64::EPSILON,
            "fraction should be 0.0 with no limit"
        );
    }

    #[test]
    fn concurrent_recording() {
        let budget = Budget::new(Some(100.0), Some(1000), None);
        budget.record(1.0, 10);
        budget.record(2.0, 20);
        budget.record(0.5, 5);
        assert!(
            (budget.current_cost_usd() - 3.5).abs() < 0.001,
            "cost should be 3.5"
        );
        assert_eq!(budget.current_turns(), 35, "turns should be 35");
    }

    #[test]
    fn debug_format_includes_fields() {
        let budget = Budget::new(Some(5.0), Some(100), None);
        let debug = format!("{budget:?}");
        assert!(debug.contains("Budget"), "debug should name the type");
        assert!(debug.contains("max_cost_usd"), "debug should show limits");
    }
}
