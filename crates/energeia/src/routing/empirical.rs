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

use aletheia_routing::store::AfterActionStoreError;
use aletheia_routing::types::{RequestFeatures, TurnOutcome};
use aletheia_routing::{BoxFuture, Router, RouterError, RoutingDecision};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tracing::Instrument;

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
    outcome_tx: mpsc::UnboundedSender<TurnOutcome>,
    outcome_worker: JoinHandle<()>,
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
    pub(crate) fn new(
        store: Arc<AfterActionStore>,
        fallback: StaticRouter,
        min_samples: u64,
        window: Duration,
        confidence_threshold: f64,
    ) -> Self {
        let (outcome_tx, outcome_rx) = mpsc::unbounded_channel();
        let outcome_worker = spawn_outcome_worker(Arc::clone(&store), outcome_rx);
        Self {
            store,
            fallback,
            outcome_tx,
            outcome_worker,
            min_samples,
            window,
            confidence_threshold,
        }
    }

    /// Select the best provider for `task_category` from `candidates`.
    ///
    /// Returns the static fallback provider when empirical data is absent or
    /// insufficient. If `candidates` is empty the static default is returned.
    /// Returns an error when the after-action store is unavailable, so callers
    /// can distinguish a routing fault from healthy empty history.
    #[cfg(test)]
    pub(crate) async fn pick(
        &self,
        task_category: &TaskCategory,
        candidates: &[ProviderId],
    ) -> Result<ProviderId, AfterActionStoreError> {
        self.pick_with_static_boundary(task_category, candidates, true)
            .await
    }

    async fn pick_with_static_boundary(
        &self,
        task_category: &TaskCategory,
        candidates: &[ProviderId],
        static_allowed: bool,
    ) -> Result<ProviderId, AfterActionStoreError> {
        let static_choice = self.fallback.pick(*task_category);

        if candidates.is_empty() {
            return Ok(static_choice.clone());
        }

        let mut best_provider: Option<&ProviderId> = None;
        let mut best_rate: f64 = -1.0;

        for provider in candidates {
            let stats = self
                .store
                .rolling_stats(provider, task_category, self.window)
                .await
                .inspect_err(|error| {
                    tracing::error!(
                        error = %error,
                        provider = %provider,
                        category = %task_category,
                        "empirical routing stats store unavailable"
                    );
                })?;

            let Some(stats) = stats else {
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
            return if static_allowed {
                Ok(static_choice.clone())
            } else {
                Ok(candidates
                    .first()
                    .cloned()
                    .unwrap_or_else(|| static_choice.clone()))
            };
        };

        // WHY: a disallowed static choice cannot veto an eligible empirical
        // winner; otherwise a cloud default could leak through a local boundary.
        // When the static choice is allowed, preserve the existing confidence
        // threshold before switching away from it.
        let static_rate = if static_allowed {
            self.store
                .rolling_stats(static_choice, task_category, self.window)
                .await
                .inspect_err(|error| {
                    tracing::error!(
                        error = %error,
                        provider = %static_choice,
                        category = %task_category,
                        "empirical routing stats store unavailable for static provider"
                    );
                })?
                .and_then(|s| s.success_rate())
                .unwrap_or(0.0)
        } else {
            0.0
        };

        let gap = best_rate - static_rate;
        if !static_allowed || gap >= self.confidence_threshold {
            tracing::info!(
                empirical_winner = %winner,
                static_choice = %static_choice,
                category = %task_category,
                winner_rate = best_rate,
                static_rate,
                gap,
                "empirical router overriding static choice"
            );
            Ok(winner.clone())
        } else {
            tracing::debug!(
                empirical_winner = %winner,
                static_choice = %static_choice,
                category = %task_category,
                gap,
                threshold = self.confidence_threshold,
                "confidence gap below threshold, using static choice"
            );
            Ok(static_choice.clone())
        }
    }

    /// Return the success rate for a specific (provider, category) pair.
    ///
    /// Returns `None` when the cache has no records for this pair.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) async fn success_rate(
        &self,
        provider: &ProviderId,
        task_category: &TaskCategory,
    ) -> Result<Option<f64>, AfterActionStoreError> {
        let stats = self
            .store
            .rolling_stats(provider, task_category, self.window)
            .await?;
        Ok(stats.and_then(|s| s.success_rate()))
    }
}

fn spawn_outcome_worker(
    store: Arc<AfterActionStore>,
    mut outcome_rx: mpsc::UnboundedReceiver<TurnOutcome>,
) -> JoinHandle<()> {
    tokio::spawn(
        async move {
            while let Some(outcome) = outcome_rx.recv().await {
                let provider = outcome.provider.clone();
                let category = outcome.task_category;
                let success = outcome.success;
                let store = Arc::clone(&store);
                let handle = tokio::spawn(async move { store.record_outcome(&outcome).await });
                match handle.await {
                    Ok(Ok(())) => {}
                    Ok(Err(error)) => {
                        tracing::error!(
                            error = %error,
                            provider = %provider,
                            category = %category,
                            success,
                            "empirical router failed to record after-action outcome"
                        );
                    }
                    Err(error) => {
                        tracing::error!(
                            error = %error,
                            provider = %provider,
                            category = %category,
                            success,
                            "empirical router after-action recorder task failed"
                        );
                    }
                }
            }
        }
        .instrument(tracing::debug_span!("empirical_router_outcome_recorder")),
    )
}

impl Router for EmpiricalRouter {
    /// Route using the empirical success-rate model.
    ///
    /// Delegates to [`pick`](Self::pick) using candidates and category from
    /// `features`. Returns the static fallback with `confidence: None` when
    /// data is insufficient or the store is unavailable; unavailable-store
    /// faults are emitted as error-level tracing events.
    fn route<'a>(&'a self, features: &'a RequestFeatures) -> BoxFuture<'a, RoutingDecision> {
        Box::pin(async move {
            let category = features.effective_category();
            let candidates = features
                .candidates
                .iter()
                .filter(|provider| features.candidate_allowed_by_boundary(provider))
                .cloned()
                .collect::<Vec<_>>();
            let static_allowed =
                features.candidate_allowed_by_boundary(self.fallback.pick(category));
            let chosen = match self
                .pick_with_static_boundary(&category, &candidates, static_allowed)
                .await
            {
                Ok(chosen) => chosen,
                Err(error) => {
                    tracing::error!(
                        error = %error,
                        category = %category,
                        "empirical router unavailable, using static fallback"
                    );
                    self.fallback.pick(category).clone()
                }
            };
            let confidence = match self
                .store
                .rolling_stats(&chosen, &category, self.window)
                .await
            {
                Ok(stats) => stats.and_then(|s| s.success_rate()),
                Err(error) => {
                    tracing::error!(
                        error = %error,
                        provider = %chosen,
                        category = %category,
                        "empirical routing confidence unavailable"
                    );
                    None
                }
            };
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
        if self.outcome_worker.is_finished() {
            let message = "after-action recorder worker stopped".to_owned();
            tracing::error!(
                provider = %outcome.provider,
                category = %outcome.task_category,
                success = outcome.success,
                "empirical router after-action recorder worker stopped"
            );
            return Err(RouterError::AfterActionWrite { message });
        }

        self.outcome_tx.send(outcome.clone()).map_err(|error| {
            let outcome = error.0;
            let message = format!(
                "after-action recorder stopped for provider {}",
                outcome.provider
            );
            tracing::error!(
                provider = %outcome.provider,
                category = %outcome.task_category,
                success = outcome.success,
                "empirical router after-action recorder stopped"
            );
            RouterError::AfterActionWrite { message }
        })
    }
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
mod tests {
    use std::io::Write as _;

    use aletheia_routing::{DEFAULT_ROUTING_WINDOW, RoutingBoundary};

    use super::*;
    use crate::routing::store::AfterActionStore;
    use crate::routing::{ProviderId, StaticRouter, TaskCategory};

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
            DEFAULT_ROUTING_WINDOW,
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
        let chosen = router
            .pick(&TaskCategory::Feature, &candidates)
            .await
            .unwrap();
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
        let chosen = router
            .pick(&TaskCategory::Feature, &candidates)
            .await
            .unwrap();
        assert_eq!(&*chosen.0, "fallback-provider");
    }

    /// Empty candidates → returns static default.
    #[tokio::test]
    async fn router_returns_static_for_empty_candidates() {
        let tmp = tempfile::tempdir().unwrap();
        let router = make_router(tmp.path(), "default", 5, 0.1).await;
        let chosen = router.pick(&TaskCategory::Feature, &[]).await.unwrap();
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
        let chosen = router
            .pick(&TaskCategory::Feature, &candidates)
            .await
            .unwrap();
        assert_eq!(&*chosen.0, "static-choice");
    }

    #[tokio::test]
    async fn router_pick_returns_error_when_stats_store_unavailable() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("not-a-directory");
        std::fs::write(&path, "not jsonl").unwrap();

        let store = Arc::new(AfterActionStore::new_with_window(
            path,
            Duration::from_hours(480),
        ));
        let router = EmpiricalRouter::new(
            store,
            StaticRouter::new(ProviderId::new("fallback")),
            5,
            Duration::from_hours(240),
            0.1,
        );
        let result = router
            .pick(&TaskCategory::Feature, &[ProviderId::new("provider-a")])
            .await;

        assert!(
            matches!(result, Err(AfterActionStoreError::Io { .. })),
            "expected I/O error, got {result:?}"
        );
    }

    /// `success_rate` returns `None` when no data.
    #[tokio::test]
    async fn success_rate_returns_none_for_unknown_provider() {
        let tmp = tempfile::tempdir().unwrap();
        let router = make_router(tmp.path(), "default", 5, 0.1).await;
        let rate = router
            .success_rate(&ProviderId::new("nobody"), &TaskCategory::Feature)
            .await
            .unwrap();
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
            .await
            .unwrap();
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

    #[tokio::test]
    async fn after_action_records_outcome_through_owned_worker() {
        let store = Arc::new(AfterActionStore::in_memory());
        let router = EmpiricalRouter::new(
            Arc::clone(&store),
            StaticRouter::new(ProviderId::new("fallback")),
            1,
            DEFAULT_ROUTING_WINDOW,
            0.1,
        );
        let decision = RoutingDecision::new("provider-a", None);
        let outcome = TurnOutcome::new(
            ProviderId::new("provider-a"),
            TaskCategory::Feature,
            true,
            true,
        );

        router
            .after_action(&decision, &outcome)
            .expect("after_action should enqueue outcome");

        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(1);
        loop {
            if !store.recent_outcomes().await.is_empty() {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "after-action worker did not record outcome"
            );
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }

        let recent = store.recent_outcomes().await;
        assert_eq!(recent.len(), 1);
        assert_eq!(&*recent[0].provider.0, "provider-a");
    }

    #[tokio::test]
    async fn route_excludes_cloud_candidate_for_local_hosted_boundary() {
        let tmp = tempfile::tempdir().unwrap();
        let mut lines = vec![];
        for _ in 0..10 {
            lines.push(session_line("cloud-only", "success", "feature"));
        }
        for _ in 0..6 {
            lines.push(session_line("local", "success", "feature"));
        }
        for _ in 0..4 {
            lines.push(session_line("local", "failed", "feature"));
        }
        write_jsonl(tmp.path(), "2026-04-17.jsonl", &lines);

        let router = make_router(tmp.path(), "cloud-only", 5, 0.1).await;
        let features = RequestFeatures::new(
            vec![ProviderId::new("cloud-only"), ProviderId::new("local")],
            Some(TaskCategory::Feature),
            None,
        )
        .with_deployment_target(RoutingBoundary::LocalHosted)
        .with_candidate_deployment_target("cloud-only", RoutingBoundary::Cloud)
        .with_candidate_deployment_target("local", RoutingBoundary::LocalHosted);

        let decision = router.route(&features).await;

        assert_eq!(&*decision.provider, "local");
    }
}
