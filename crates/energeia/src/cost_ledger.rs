// NOTE: Per-blast-radius cost attribution ledger. Tracks cumulative cost, turns,
// and session counts by blast radius to answer "how much did this feature cost?"
//
// Uses Arc<Mutex<HashMap>> for thread-safe accumulation. This is not a hot path
// (recorded once per session completion), so the lock contention is minimal.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

// ---------------------------------------------------------------------------
// BlastRadiusCost
// ---------------------------------------------------------------------------

/// Cost attribution for a single blast radius.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct BlastRadiusCost {
    /// Blast radius identifier (typically a file path or module prefix).
    pub blast_radius: String,
    /// Total cost in USD across all sessions targeting this blast radius.
    pub total_cost_usd: f64,
    /// Total LLM turns consumed across all sessions.
    pub total_turns: u32,
    /// Number of sessions recorded for this blast radius.
    pub session_count: u32,
    /// Cost breakdown by model used.
    pub cost_by_model: HashMap<String, f64>,
}

impl BlastRadiusCost {
    /// Create a new empty cost record for the given blast radius.
    #[must_use]
    fn new(blast_radius: String) -> Self {
        Self {
            blast_radius,
            total_cost_usd: 0.0,
            total_turns: 0,
            session_count: 0,
            cost_by_model: HashMap::new(),
        }
    }

    /// Record a single session's contribution to this blast radius.
    fn record(&mut self, cost_usd: f64, turns: u32, model: &str) {
        self.total_cost_usd += cost_usd;
        self.total_turns += turns;
        self.session_count += 1;

        *self.cost_by_model.entry(model.to_owned()).or_insert(0.0) += cost_usd;
    }
}

// ---------------------------------------------------------------------------
// CostLedger
// ---------------------------------------------------------------------------

/// Thread-safe per-blast-radius cost accumulator.
///
/// Records cost attribution by blast radius to enable "how much did this
/// feature cost?" queries. Designed for concurrent access from multiple
/// session execution tasks.
///
/// # Example
///
/// ```rust,ignore
/// use aletheia_energeia::cost_ledger::CostLedger;
///
/// let ledger = CostLedger::new();
///
/// // Record a session outcome
/// ledger.record("crates/pylon/src/handlers", 0.50, 15, "claude-3-5-sonnet");
///
/// // Query costs for a specific blast radius
/// if let Some(cost) = ledger.query("crates/pylon/src/handlers") {
///     println!("Total cost: ${:.2}", cost.total_cost_usd);
/// }
///
/// // Get all costs
/// let all_costs = ledger.query_all();
/// ```
#[derive(Debug, Clone)]
pub struct CostLedger {
    inner: Arc<Mutex<HashMap<String, BlastRadiusCost>>>,
}

impl Default for CostLedger {
    fn default() -> Self {
        Self::new()
    }
}

impl CostLedger {
    /// Create a new empty cost ledger.
    #[must_use]
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Record cost attribution for a single session.
    ///
    /// - `blast_radius` — the blast radius identifier (e.g., "crates/foo/src/")
    /// - `cost_usd` — session cost in USD
    /// - `turns` — number of LLM turns consumed
    /// - `model` — LLM model identifier (e.g., "claude-3-5-sonnet")
    ///
    /// # Panics
    ///
    /// Panics if the internal mutex is poisoned (another thread panicked
    /// while holding the lock).
    pub fn record(&self, blast_radius: &str, cost_usd: f64, turns: u32, model: &str) {
        // WHY: Fast path — skip recording zero-cost sessions to reduce lock
        // contention and keep the ledger clean.
        if cost_usd <= 0.0 && turns == 0 {
            return;
        }

        let mut guard = self.inner.lock().expect("cost ledger mutex poisoned");
        let entry = guard
            .entry(blast_radius.to_owned())
            .or_insert_with(|| BlastRadiusCost::new(blast_radius.to_owned()));
        entry.record(cost_usd, turns, model);
    }

    /// Record cost attribution for multiple blast radii (for prompts that
    /// affect multiple areas).
    ///
    /// The cost is attributed to each blast radius in full (not divided).
    /// This reflects that the session work benefits/applies to all radii.
    ///
    /// # Panics
    ///
    /// Panics if the internal mutex is poisoned.
    pub fn record_multi(&self, blast_radii: &[String], cost_usd: f64, turns: u32, model: &str) {
        if cost_usd <= 0.0 && turns == 0 {
            return;
        }

        let mut guard = self.inner.lock().expect("cost ledger mutex poisoned");
        for radius in blast_radii {
            let entry = guard
                .entry(radius.clone())
                .or_insert_with(|| BlastRadiusCost::new(radius.clone()));
            entry.record(cost_usd, turns, model);
        }
    }

    /// Query the cost attribution for a specific blast radius.
    ///
    /// Returns `None` if no sessions have been recorded for this radius.
    #[must_use]
    pub fn query(&self, blast_radius: &str) -> Option<BlastRadiusCost> {
        let guard = self.inner.lock().expect("cost ledger mutex poisoned");
        guard.get(blast_radius).cloned()
    }

    /// Query all blast radius cost records.
    ///
    /// Returns a vector of (blast_radius, cost) tuples sorted by blast radius
    /// for deterministic output.
    #[must_use]
    pub fn query_all(&self) -> Vec<(String, BlastRadiusCost)> {
        let guard = self.inner.lock().expect("cost ledger mutex poisoned");
        let mut results: Vec<(String, BlastRadiusCost)> = guard
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        // Sort by blast radius for deterministic output
        results.sort_by(|a, b| a.0.cmp(&b.0));
        results
    }

    /// Query total cost aggregated by model across all blast radii.
    ///
    /// Returns a vector of (model, total_cost_usd) tuples sorted by model name.
    #[must_use]
    pub fn query_by_model(&self) -> Vec<(String, f64)> {
        let guard = self.inner.lock().expect("cost ledger mutex poisoned");
        let mut by_model: HashMap<String, f64> = HashMap::new();

        for cost in guard.values() {
            for (model, model_cost) in &cost.cost_by_model {
                *by_model.entry(model.clone()).or_insert(0.0) += model_cost;
            }
        }

        let mut results: Vec<(String, f64)> = by_model.into_iter().collect();
        results.sort_by(|a, b| a.0.cmp(&b.0));
        results
    }

    /// Get the total cost across all blast radii.
    #[must_use]
    pub fn total_cost(&self) -> f64 {
        let guard = self.inner.lock().expect("cost ledger mutex poisoned");
        guard.values().map(|c| c.total_cost_usd).sum()
    }

    /// Get the total number of sessions recorded across all blast radii.
    #[must_use]
    pub fn total_sessions(&self) -> u32 {
        let guard = self.inner.lock().expect("cost ledger mutex poisoned");
        guard.values().map(|c| c.session_count).sum()
    }

    /// Clear all recorded data.
    ///
    /// # Panics
    ///
    /// Panics if the internal mutex is poisoned.
    pub fn clear(&self) {
        let mut guard = self.inner.lock().expect("cost ledger mutex poisoned");
        guard.clear();
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
mod tests {
    use super::*;

    #[test]
    fn new_ledger_is_empty() {
        let ledger = CostLedger::new();
        assert!(ledger.query_all().is_empty());
        assert!((ledger.total_cost()).abs() < f64::EPSILON);
        assert_eq!(ledger.total_sessions(), 0);
    }

    #[test]
    fn record_single_session() {
        let ledger = CostLedger::new();
        ledger.record("crates/foo/", 1.50, 10, "claude-3-5-sonnet");

        let cost = ledger.query("crates/foo/").unwrap();
        assert_eq!(cost.blast_radius, "crates/foo/");
        assert!((cost.total_cost_usd - 1.50).abs() < 0.001);
        assert_eq!(cost.total_turns, 10);
        assert_eq!(cost.session_count, 1);
        assert_eq!(cost.cost_by_model.get("claude-3-5-sonnet").copied().unwrap(), 1.50);
    }

    #[test]
    fn record_accumulates_multiple_sessions() {
        let ledger = CostLedger::new();
        ledger.record("crates/foo/", 1.00, 10, "claude-3-5-sonnet");
        ledger.record("crates/foo/", 2.00, 15, "claude-3-5-sonnet");
        ledger.record("crates/foo/", 0.50, 5, "claude-3-haiku");

        let cost = ledger.query("crates/foo/").unwrap();
        assert!((cost.total_cost_usd - 3.50).abs() < 0.001);
        assert_eq!(cost.total_turns, 30);
        assert_eq!(cost.session_count, 3);
        assert_eq!(cost.cost_by_model.len(), 2);
        assert!(
            (cost.cost_by_model.get("claude-3-5-sonnet").copied().unwrap() - 3.00).abs() < 0.001
        );
        assert!((cost.cost_by_model.get("claude-3-haiku").copied().unwrap() - 0.50).abs() < 0.001);
    }

    #[test]
    fn record_multiple_blast_radii() {
        let ledger = CostLedger::new();
        ledger.record("crates/foo/", 1.00, 10, "claude-3-5-sonnet");
        ledger.record("crates/bar/", 2.00, 20, "claude-3-5-sonnet");

        let all = ledger.query_all();
        assert_eq!(all.len(), 2);

        let foo_cost = ledger.query("crates/foo/").unwrap();
        let bar_cost = ledger.query("crates/bar/").unwrap();

        assert!((foo_cost.total_cost_usd - 1.00).abs() < 0.001);
        assert!((bar_cost.total_cost_usd - 2.00).abs() < 0.001);
        assert!((ledger.total_cost() - 3.00).abs() < 0.001);
    }

    #[test]
    fn record_multi_attributes_to_all_radii() {
        let ledger = CostLedger::new();
        let radii = vec!["crates/foo/".to_owned(), "crates/bar/".to_owned()];
        ledger.record_multi(&radii, 1.00, 10, "claude-3-5-sonnet");

        let foo_cost = ledger.query("crates/foo/").unwrap();
        let bar_cost = ledger.query("crates/bar/").unwrap();

        // Both get the full cost attributed
        assert!((foo_cost.total_cost_usd - 1.00).abs() < 0.001);
        assert!((bar_cost.total_cost_usd - 1.00).abs() < 0.001);

        // Total cost counts each radius separately (this is expected behavior)
        assert!((ledger.total_cost() - 2.00).abs() < 0.001);
    }

    #[test]
    fn query_nonexistent_returns_none() {
        let ledger = CostLedger::new();
        assert!(ledger.query("does-not-exist").is_none());
    }

    #[test]
    fn query_by_model_aggregates() {
        let ledger = CostLedger::new();
        ledger.record("crates/foo/", 1.00, 10, "claude-3-5-sonnet");
        ledger.record("crates/bar/", 2.00, 20, "claude-3-5-sonnet");
        ledger.record("crates/baz/", 0.50, 5, "claude-3-haiku");

        let by_model = ledger.query_by_model();
        assert_eq!(by_model.len(), 2);

        // Should be sorted by model name
        assert_eq!(by_model[0].0, "claude-3-5-sonnet");
        assert!((by_model[0].1 - 3.00).abs() < 0.001);

        assert_eq!(by_model[1].0, "claude-3-haiku");
        assert!((by_model[1].1 - 0.50).abs() < 0.001);
    }

    #[test]
    fn zero_cost_sessions_are_skipped() {
        let ledger = CostLedger::new();
        ledger.record("crates/foo/", 0.0, 0, "claude-3-5-sonnet");
        assert!(ledger.query("crates/foo/").is_none());
    }

    #[test]
    fn zero_cost_with_turns_is_recorded() {
        let ledger = CostLedger::new();
        ledger.record("crates/foo/", 0.0, 5, "claude-3-5-sonnet");
        let cost = ledger.query("crates/foo/").unwrap();
        assert_eq!(cost.total_turns, 5);
    }

    #[test]
    fn clear_removes_all_data() {
        let ledger = CostLedger::new();
        ledger.record("crates/foo/", 1.00, 10, "claude-3-5-sonnet");
        ledger.clear();
        assert!(ledger.query_all().is_empty());
        assert!((ledger.total_cost()).abs() < f64::EPSILON);
    }

    #[test]
    fn query_all_sorted_by_radius() {
        let ledger = CostLedger::new();
        ledger.record("crates/zulu/", 1.00, 10, "claude-3-5-sonnet");
        ledger.record("crates/alpha/", 2.00, 20, "claude-3-5-sonnet");
        ledger.record("crates/middle/", 0.50, 5, "claude-3-5-sonnet");

        let all = ledger.query_all();
        assert_eq!(all[0].0, "crates/alpha/");
        assert_eq!(all[1].0, "crates/middle/");
        assert_eq!(all[2].0, "crates/zulu/");
    }

    #[test]
    fn ledger_is_send_sync() {
        static_assertions::assert_impl_all!(CostLedger: Send, Sync);
    }

    #[test]
    fn blast_radius_cost_is_send_sync() {
        static_assertions::assert_impl_all!(BlastRadiusCost: Send, Sync);
    }

    #[test]
    fn concurrent_recordings() {
        use std::thread;

        let ledger = CostLedger::new();
        let mut handles = vec![];

        for i in 0..10 {
            let ledger_clone = ledger.clone();
            let handle = thread::spawn(move || {
                let radius = format!("crates/module-{i}/");
                ledger_clone.record(&radius, 1.0, 10, "claude-3-5-sonnet");
            });
            handles.push(handle);
        }

        for handle in handles {
            handle.join().unwrap();
        }

        assert_eq!(ledger.query_all().len(), 10);
        assert!((ledger.total_cost() - 10.0).abs() < 0.001);
    }
}
