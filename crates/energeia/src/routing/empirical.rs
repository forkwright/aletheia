// WHY: EmpiricalRouter replaces static provider selection with data-driven
// routing.  When enough samples exist in the after-action store it picks the
// provider with the highest rolling success rate; when data is insufficient
// it defers to the StaticRouter fallback.  The threshold check (winner_rate -
// static_rate >= confidence_threshold) prevents oscillation when one provider
// has only marginally more data than another.
//
// Implements `aletheia_routing::Router` so that the same trait drives both
// dispatch (this crate) and interactive (nous) after-action recording.

use std::sync::Arc;
use std::time::Duration;

use aletheia_routing::types::{RequestFeatures, TurnOutcome};
use aletheia_routing::{BoxFuture, Router, RouterError, RoutingDecision};

use super::store::AfterActionStore;
use super::{ProviderId, StaticRouter, TaskCategory};

/// Empirical provider router.
///
/// Picks the provider with the highest historical success rate for a given
/// `(provider, task_category)` pair, falling back to the [`StaticRouter`]
/// when:
///
/// - fewer than `min_samples` records exist for any candidate provider, or
/// - the winning provider is already the static choice, or
/// - the confidence gap between winner and static choice is below
///   `confidence_threshold`.
///
/// Implements [`Router`] from `aletheia-routing` so that `after_action` calls
/// from both the dispatch path (energeia) and the interactive path (nous)
/// feed into the same [`AfterActionStore`].
pub(crate) struct EmpiricalRouter {
    store: Arc<AfterActionStore>,
    fallback: StaticRouter,
    /// Minimum sample count before empirical routing overrides static.
    min_samples: u64,
    /// Rolling window for after-action record queries.
    window: Duration,
    /// Minimum success-rate gap required to switch away from the static provider.
    confidence_threshold: f64,
}

impl EmpiricalRouter {
    /// Create a new empirical router.
    ///
    /// # Arguments
    ///
    /// * `store` — shared read/write cache over after-action JSONL logs and
    ///   direct interactive-path outcomes
    /// * `fallback` — static router used when data is insufficient
    /// * `min_samples` — minimum records before empirical choice is made (default 5)
    /// * `window` — rolling window for record weighting (default 7 days)
    /// * `confidence_threshold` — minimum gap (winner − static) to override (default 0.1)
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "binary wiring constructs EmpiricalRouter")
    )]
    pub(crate) fn new(
        store: Arc<AfterActionStore>,
        fallback: StaticRouter,
        min_samples: u64,
        window: Duration,
        confidence_threshold: f64,
    ) -> Self {
        Self {
            store,
            fallback,
            min_samples,
            window,
            confidence_threshold,
        }
    }

    /// Select the best provider for `task_category` from `candidates`.
    ///
    /// Returns the static fallback provider when empirical data is absent or
    /// insufficient.  If `candidates` is empty the static default is returned.
    pub(crate) async fn pick(
        &self,
        task_category: &TaskCategory,
        candidates: &[ProviderId],
    ) -> ProviderId {
        if candidates.is_empty() {
            return self.fallback.pick(*task_category).clone();
        }

        let static_choice = self.fallback.pick(*task_category);

        // Collect success rates for all candidates.
        let mut best_provider: Option<&ProviderId> = None;
        let mut best_rate: f64 = -1.0;

        for provider in candidates {
            let stats = self
                .store
                .rolling_stats(provider, task_category, self.window)
                .await;

            let Some(stats) = stats else {
                // No data — this provider does not qualify for empirical selection.
                continue;
            };

            if stats.total < self.min_samples {
                tracing::debug!(
                    provider = %provider,
                    category = %task_category,
                    total = stats.total,
                    min_samples = self.min_samples,
                    "insufficient samples, skipping empirical candidate"
                );
                continue;
            }

            let Some(rate) = stats.success_rate() else {
                continue;
            };

            if rate > best_rate {
                best_rate = rate;
                best_provider = Some(provider);
            }
        }

        let Some(winner) = best_provider else {
            // No candidate had enough data.
            return static_choice.clone();
        };

        // Only override the static choice if the empirical winner is strictly
        // better by at least `confidence_threshold`.
        let static_rate = self
            .store
            .rolling_stats(static_choice, task_category, self.window)
            .await
            .and_then(|s| s.success_rate())
            .unwrap_or(0.0);

        let gap = best_rate - static_rate;
        if gap >= self.confidence_threshold {
            tracing::info!(
                empirical_winner = %winner,
                static_choice = %static_choice,
                category = %task_category,
                winner_rate = best_rate,
                static_rate,
                gap,
                "empirical router overriding static choice"
            );
            winner.clone()
        } else {
            tracing::debug!(
                empirical_winner = %winner,
                static_choice = %static_choice,
                category = %task_category,
                gap,
                threshold = self.confidence_threshold,
                "confidence gap below threshold, using static choice"
            );
            static_choice.clone()
        }
    }

    /// Return the success rate for a specific (provider, category) pair.
    ///
    /// Returns `None` when the cache has no records for this pair.
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "observability API for binary wiring")
    )]
    pub(crate) async fn success_rate(
        &self,
        provider: &ProviderId,
        task_category: &TaskCategory,
    ) -> Option<f64> {
        let stats = self
            .store
            .rolling_stats(provider, task_category, self.window)
            .await?;
        stats.success_rate()
    }
}

// ---------------------------------------------------------------------------
// Router trait implementation
// ---------------------------------------------------------------------------

impl Router for EmpiricalRouter {
    /// Route using the empirical success-rate model.
    ///
    /// Delegates to [`pick`](Self::pick) using candidates and category from
    /// `features`. Returns the static fallback with `confidence: None` when
    /// data is insufficient; returns `confidence: Some(rate)` when the
    /// empirical winner was selected.
    fn route<'a>(&'a self, features: &'a RequestFeatures) -> BoxFuture<'a, RoutingDecision> {
        Box::pin(async move {
            let category = features.effective_category();
            let chosen = self.pick(&category, &features.candidates).await;
            // Attach confidence when we have stats for the chosen provider.
            let confidence = self
                .store
                .rolling_stats(&chosen, &category, self.window)
                .await
                .and_then(|s| s.success_rate());
            RoutingDecision::new(chosen.0.clone(), confidence)
        })
    }

    /// Record an after-action outcome into the shared store.
    ///
    /// Both dispatch turns (via energeia) and interactive turns (via nous)
    /// call this method, ensuring learnings are pooled in a single backend.
    fn after_action(
        &self,
        _decision: &RoutingDecision,
        outcome: &TurnOutcome,
    ) -> Result<(), RouterError> {
        // WHY: record_outcome is async (takes the write lock), but `after_action`
        // on the trait is sync to keep the trait object-safe without boxing
        // every future. We spawn a fire-and-forget task for the store write
        // so the hot response path is never blocked.
        let store = Arc::clone(&self.store);
        let outcome = outcome.clone();
        tokio::spawn(async move {
            store.record_outcome(&outcome).await;
        });
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
mod tests {
    use std::io::Write as _;
    use std::time::Duration;

    use super::*;
    use crate::routing::store::AfterActionStore;
    use crate::routing::{ProviderId, StaticRouter, TaskCategory};

    const WINDOW: Duration = Duration::from_hours(168);

    fn session_line(model: &str, status: &str, category: &str) -> serde_json::Value {
        serde_json::json!({
            "dispatch_id": "test",
            "ts_start": "2026-04-17T00:00:00Z",
            "ts_end": "2026-04-17T00:01:00Z",
            "duration_ms": 60000,
            "session_outcomes": [{"model": model, "status": status, "category": category}],
            "cost_total_cents": 5,
            "turns_total": 10,
            "stage_latencies_ms": {},
            "qa_verdict": "pass",
            "prompt_hash": "sha256:abc"
        })
    }

    async fn make_router(
        dir: &std::path::Path,
        default: &str,
        min_samples: u64,
        threshold: f64,
    ) -> EmpiricalRouter {
        let store = Arc::new(AfterActionStore::new(dir.to_owned()));
        store.refresh().await.unwrap();
        EmpiricalRouter::new(
            store,
            StaticRouter::new(ProviderId::new(default)),
            min_samples,
            WINDOW,
            threshold,
        )
    }

    fn write_jsonl(dir: &std::path::Path, filename: &str, lines: &[serde_json::Value]) {
        let path = dir.join(filename);
        let mut file = std::fs::File::create(path).unwrap();
        for line in lines {
            writeln!(file, "{line}").unwrap();
        }
    }

    /// Provider A: 9/10 success, Provider B: 1/10 success → router picks A.
    #[tokio::test]
    async fn router_picks_winner_when_clear_winner_exists() {
        let tmp = tempfile::tempdir().unwrap();
        let mut lines = vec![];
        for _ in 0..9 {
            lines.push(session_line("provider-a", "success", "feature"));
        }
        lines.push(session_line("provider-a", "failed", "feature"));
        lines.push(session_line("provider-b", "success", "feature"));
        for _ in 0..9 {
            lines.push(session_line("provider-b", "failed", "feature"));
        }
        write_jsonl(tmp.path(), "2026-04-17.jsonl", &lines);

        let router = make_router(tmp.path(), "provider-b", 5, 0.1).await;
        let candidates = vec![ProviderId::new("provider-a"), ProviderId::new("provider-b")];
        let chosen = router.pick(&TaskCategory::Feature, &candidates).await;
        assert_eq!(&*chosen.0, "provider-a");
    }

    /// Below `min_samples` → fall through to static fallback.
    #[tokio::test]
    async fn router_falls_through_when_below_min_samples() {
        let tmp = tempfile::tempdir().unwrap();
        let lines: Vec<_> = (0..3)
            .map(|_| session_line("provider-a", "success", "feature"))
            .collect();
        write_jsonl(tmp.path(), "2026-04-17.jsonl", &lines);

        let router = make_router(tmp.path(), "fallback-provider", 5, 0.1).await;
        let candidates = vec![ProviderId::new("provider-a")];
        let chosen = router.pick(&TaskCategory::Feature, &candidates).await;
        assert_eq!(&*chosen.0, "fallback-provider");
    }

    /// Empty candidates → returns static default.
    #[tokio::test]
    async fn router_returns_static_for_empty_candidates() {
        let tmp = tempfile::tempdir().unwrap();
        let router = make_router(tmp.path(), "default", 5, 0.1).await;
        let chosen = router.pick(&TaskCategory::Feature, &[]).await;
        assert_eq!(&*chosen.0, "default");
    }

    /// Confidence gap below threshold → use static choice.
    #[tokio::test]
    async fn router_uses_static_when_gap_below_threshold() {
        let tmp = tempfile::tempdir().unwrap();
        let mut lines = vec![];
        for _ in 0..7 {
            lines.push(session_line("provider-a", "success", "feature"));
        }
        for _ in 0..3 {
            lines.push(session_line("provider-a", "failed", "feature"));
        }
        for _ in 0..6 {
            lines.push(session_line("static-choice", "success", "feature"));
        }
        for _ in 0..4 {
            lines.push(session_line("static-choice", "failed", "feature"));
        }
        write_jsonl(tmp.path(), "2026-04-17.jsonl", &lines);

        // threshold = 0.2 → gap 0.10 < 0.2 → static wins
        let router = make_router(tmp.path(), "static-choice", 5, 0.2).await;
        let candidates = vec![
            ProviderId::new("provider-a"),
            ProviderId::new("static-choice"),
        ];
        let chosen = router.pick(&TaskCategory::Feature, &candidates).await;
        assert_eq!(&*chosen.0, "static-choice");
    }

    /// `success_rate` returns `None` when no data.
    #[tokio::test]
    async fn success_rate_returns_none_for_unknown_provider() {
        let tmp = tempfile::tempdir().unwrap();
        let router = make_router(tmp.path(), "default", 5, 0.1).await;
        let rate = router
            .success_rate(&ProviderId::new("nobody"), &TaskCategory::Feature)
            .await;
        assert!(rate.is_none());
    }

    /// `success_rate` returns correct value when data present.
    #[tokio::test]
    async fn success_rate_returns_correct_value() {
        let tmp = tempfile::tempdir().unwrap();
        let mut lines = vec![];
        for _ in 0..8 {
            lines.push(session_line("provider-x", "success", "bug"));
        }
        for _ in 0..2 {
            lines.push(session_line("provider-x", "failed", "bug"));
        }
        write_jsonl(tmp.path(), "2026-04-17.jsonl", &lines);

        let router = make_router(tmp.path(), "default", 5, 0.1).await;
        let rate = router
            .success_rate(&ProviderId::new("provider-x"), &TaskCategory::Bug)
            .await;
        assert!((rate.unwrap() - 0.8).abs() < 0.001);
    }

    /// Router trait impl: `route` returns the empirical winner with confidence.
    #[tokio::test]
    async fn router_trait_route_returns_winner_with_confidence() {
        let tmp = tempfile::tempdir().unwrap();
        let mut lines = vec![];
        for _ in 0..9 {
            lines.push(session_line("winner", "success", "feature"));
        }
        lines.push(session_line("winner", "failed", "feature"));
        for _ in 0..2 {
            lines.push(session_line("loser", "success", "feature"));
        }
        for _ in 0..8 {
            lines.push(session_line("loser", "failed", "feature"));
        }
        write_jsonl(tmp.path(), "2026-04-17.jsonl", &lines);

        let router = make_router(tmp.path(), "loser", 5, 0.1).await;
        let features = RequestFeatures::new(
            vec![ProviderId::new("winner"), ProviderId::new("loser")],
            Some(TaskCategory::Feature),
            None,
        );
        let decision = router.route(&features).await;
        assert_eq!(&*decision.provider, "winner");
        assert!(
            decision.confidence.is_some(),
            "confidence should be present when empirical data exists"
        );
        assert!(decision.confidence.unwrap() > 0.8);
    }
}
