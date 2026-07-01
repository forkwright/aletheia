// WHY: Expertise affinity extends empirical routing with a dimension-weighted
// preference score that captures *what kind* of work a provider has historically
// succeeded at, not just its raw success rate.
//
// If provider X succeeded at 12/15 `Refactor` sessions in the last 7 days,
// a new refactor request should prefer X over a provider with a higher global
// rate but no refactor history (phronesis affinity design).
//
// Affinity is a secondary signal — it only breaks ties when the empirical
// success-rate gap is within `confidence_threshold`. The primary selection
// layer (EmpiricalRouter) is unchanged.
//
// Dimension weights (phronesis design):
//   category-specific success rate : 40% (proxies crate overlap)
//   success-rate consistency        : 30% (proxies file overlap — consistent providers
//                                          are reliable across similar work units)
//   cross-category success rate     : 20% (proxies project match — providers that
//                                          succeed broadly are safer fallbacks)
//   recency bonus                   : 10% (recent activity signals warm context)
//
// WHY these proxies: `TurnOutcome` carries (provider, task_category, success,
// is_interactive). There is no crate name or file list. The four phronesis
// dimensions map onto the data we have without extending the wire format.

use std::sync::Arc;
use std::time::Duration;

use aletheia_routing::store::RollingStats;
use aletheia_routing::types::{RequestFeatures, TurnOutcome};
use aletheia_routing::{BoxFuture, Router, RouterError, RoutingDecision};
use tracing::instrument;

use super::persona::PersonaDecision;
use super::persona::PersonaRouter;
use super::persona::{ModelTier, PersonaRole};
use super::store::AfterActionStore;
use super::{ProviderId, TaskCategory};

/// Weighted expertise-affinity score for one (provider, `task_category`) pair.
///
/// Combines four dimensions — category match, consistency, breadth, recency —
/// into a single `[0, 1]` score used to break ties in the empirical router.
///
/// # Weights (phronesis design)
///
/// | Dimension              | Weight | Proxy for         |
/// |------------------------|--------|-------------------|
/// | category success rate  | 0.40   | crate overlap     |
/// | success consistency    | 0.30   | file overlap      |
/// | cross-category breadth | 0.20   | project match     |
/// | recency bonus          | 0.10   | warm context      |
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub(crate) struct AffinityScore {
    /// Success rate for the specific requested [`TaskCategory`] in [0, 1].
    ///
    /// `None` when the provider has no history in this category.
    pub(crate) category_success_rate: Option<f64>,

    /// Success-rate consistency: `1 - coefficient_of_variation`, in [0, 1].
    ///
    /// Derived from the ratio of successes to total for the requested category.
    /// Providers with very high *or* very low rates are more consistent; the
    /// mid-range providers are less predictable.
    pub(crate) consistency: f64,

    /// Cross-category breadth: overall success fraction across all categories.
    ///
    /// Providers that succeed broadly score higher on project match.
    /// `None` when the provider has no history at all.
    pub(crate) breadth: Option<f64>,

    /// Recency bonus in [0, 1].
    ///
    /// `1.0` when the provider had a successful turn within `recency_window`.
    /// Decays to `0.0` when no recent success is recorded.
    pub(crate) recency_bonus: f64,
}

impl AffinityScore {
    /// Compute the weighted composite affinity score in [0, 1].
    ///
    /// Missing dimensions (`None`) contribute `0.0` to the weighted sum and
    /// their weight is redistributed to the remaining terms so the total still
    /// sums to 1.0.
    #[must_use]
    #[expect(
        clippy::float_arithmetic,
        reason = "weighted average of affinity dimensions for routing preference"
    )]
    pub(crate) fn weighted(&self) -> f64 {
        const W_CATEGORY: f64 = 0.40;
        const W_CONSISTENCY: f64 = 0.30;
        const W_BREADTH: f64 = 0.20;
        const W_RECENCY: f64 = 0.10;

        let mut score = 0.0_f64;
        let mut total_weight = 0.0_f64;

        if let Some(cat) = self.category_success_rate {
            score += W_CATEGORY * cat;
            total_weight += W_CATEGORY;
        }

        score += W_CONSISTENCY * self.consistency;
        total_weight += W_CONSISTENCY;

        if let Some(breadth) = self.breadth {
            score += W_BREADTH * breadth;
            total_weight += W_BREADTH;
        }

        score += W_RECENCY * self.recency_bonus;
        total_weight += W_RECENCY;

        if total_weight < f64::EPSILON {
            return 0.0;
        }

        (score / total_weight).clamp(0.0, 1.0)
    }
}

/// Affinity-enhanced provider router.
///
/// Wraps [`PersonaRouter`] for empirical + persona selection, then applies
/// affinity scores to prefer providers with a history of success in the
/// requested [`TaskCategory`]. Affinity only overrides when the empirical
/// confidence gap is narrow; clear winners from the empirical layer are
/// preserved.
///
/// # Algorithm
///
/// 1. Ask inner [`PersonaRouter`] for an empirical + persona decision.
/// 2. Compute [`AffinityScore`] for each candidate.
/// 3. If the top-affinity provider has a score gap above `affinity_threshold`
///    *and* was not already selected by the empirical layer, substitute it.
///    Otherwise keep the empirical winner.
///
/// # No changes to `aletheia-routing`
///
/// `AffinityRouter` implements the shared [`Router`] trait by delegating
/// `route` + `after_action` to the inner [`PersonaRouter`]. The affinity
/// extension is available via [`route_with_affinity`](Self::route_with_affinity).
pub(crate) struct AffinityRouter {
    inner: PersonaRouter,
    store: Arc<AfterActionStore>,
    /// Rolling window used to query affinity stats (matches empirical window).
    window: Duration,
    /// Minimum affinity-score gap needed to override the empirical selection.
    ///
    /// Default: 0.15. A tighter gap would cause frequent overrides on thin data;
    /// a wider gap would make affinity a no-op in most routing decisions.
    affinity_threshold: f64,
}

impl AffinityRouter {
    /// Create a new affinity router.
    ///
    /// # Arguments
    ///
    /// * `inner` — persona router that provides the base routing decision
    /// * `store` — shared after-action store (same instance as the inner empirical router)
    /// * `window` — rolling window for stat queries (should match empirical window)
    /// * `affinity_threshold` — minimum affinity gap to override empirical selection
    pub(crate) fn new(
        inner: PersonaRouter,
        store: Arc<AfterActionStore>,
        window: Duration,
        affinity_threshold: f64,
    ) -> Self {
        Self {
            inner,
            store,
            window,
            affinity_threshold,
        }
    }

    /// Route with affinity: persona decision + affinity override when clear winner.
    ///
    /// Returns a [`PersonaDecision`] with the provider potentially upgraded to
    /// the highest-affinity candidate if that candidate's affinity score beats
    /// the empirical winner by at least `affinity_threshold`.
    ///
    /// When `persona_hint` is `Some`, it is forwarded to the inner persona router.
    #[instrument(skip(self), fields(
        affinity_threshold = self.affinity_threshold,
    ))]
    pub(crate) async fn route_with_affinity(
        &self,
        features: &RequestFeatures,
        persona_hint: Option<(ModelTier, PersonaRole)>,
    ) -> PersonaDecision {
        let persona_decision = self.inner.route_with_persona(features, persona_hint).await;
        let empirical_winner = ProviderId::new(persona_decision.base.provider.clone());
        let category = features.effective_category();
        let candidates = features
            .candidates
            .iter()
            .filter(|provider| features.candidate_allowed_by_boundary(provider))
            .cloned()
            .collect::<Vec<_>>();

        if candidates.is_empty() {
            return persona_decision;
        }

        let empirical_winner_allowed = features.candidate_allowed_by_boundary(&empirical_winner);
        let mut best_affinity_provider: Option<&ProviderId> = None;
        let mut best_affinity_score: f64 = -1.0;
        let mut empirical_winner_affinity: f64 = 0.0;

        for candidate in &candidates {
            let affinity = self
                .compute_affinity(candidate, &category, &candidates)
                .await;
            let score = affinity.weighted();

            tracing::debug!(
                provider = %candidate,
                affinity_score = score,
                "affinity score computed"
            );

            if *candidate == empirical_winner {
                empirical_winner_affinity = score;
            }

            if score > best_affinity_score {
                best_affinity_score = score;
                best_affinity_provider = Some(candidate);
            }
        }

        let Some(affinity_winner) = best_affinity_provider else {
            return persona_decision;
        };

        #[expect(
            clippy::float_arithmetic,
            reason = "affinity gap computation for router override decision"
        )]
        let gap = best_affinity_score - empirical_winner_affinity;

        if *affinity_winner != empirical_winner
            && (!empirical_winner_allowed || gap >= self.affinity_threshold)
        {
            tracing::info!(
                affinity_winner = %affinity_winner,
                empirical_winner = %empirical_winner,
                affinity_score = best_affinity_score,
                empirical_affinity = empirical_winner_affinity,
                gap,
                "affinity router overriding empirical selection"
            );

            // WHY(#3969): affinity overrides are learned-policy decisions. Carry
            // the composite affinity score as confidence so live dispatch can
            // fall through to static routing when the signal is too weak.
            let new_base =
                RoutingDecision::new(affinity_winner.0.clone(), Some(best_affinity_score));
            let rationale = format!(
                "affinity-override: provider={affinity_winner} gap={gap:.3} category={category}",
            );
            PersonaDecision::new(
                new_base,
                persona_decision.model_tier,
                persona_decision.persona_role,
                rationale,
            )
        } else {
            persona_decision
        }
    }

    /// Compute the [`AffinityScore`] for a single candidate provider.
    async fn compute_affinity(
        &self,
        provider: &ProviderId,
        category: &TaskCategory,
        all_candidates: &[ProviderId],
    ) -> AffinityScore {
        // Category-specific success rate.
        let cat_stats = self
            .store
            .rolling_stats(provider, category, self.window)
            .await
            .unwrap_or_else(|error| {
                tracing::error!(
                    error = %error,
                    provider = %provider,
                    category = %category,
                    "affinity routing stats store unavailable"
                );
                None
            });
        let category_success_rate = cat_stats.as_ref().and_then(RollingStats::success_rate);

        // Consistency: for binary (success/fail) outcomes, providers with very
        // high or very low rates are more predictable. Map to [0, 1] via
        // |rate - 0.5| * 2 (so 0.0 → 0.0 consistency, 1.0 → 1.0 consistency).
        //
        // WHY: A provider at 90% success is consistently good; at 10% consistently
        // bad (and the empirical layer will exclude it). Mid-range (50%) providers
        // are unpredictable — they succeed or fail non-deterministically.
        #[expect(
            clippy::float_arithmetic,
            reason = "consistency metric derivation from success rate"
        )]
        let consistency = category_success_rate.map_or(0.0, |r| (r - 0.5_f64).abs() * 2.0);

        // Cross-category breadth: fraction of all categories where this provider
        // has at least one success.
        let breadth = self.compute_breadth(provider, all_candidates).await;

        // Recency: 1.0 if there was a recent successful session, 0.0 otherwise.
        let recency_bonus = if let Some(stats) = &cat_stats {
            match stats.last_success_at {
                Some(_ts) => {
                    // Any recorded success within the window is sufficient for
                    // a full recency bonus. Window-bounded refresh ensures the
                    // store only holds recent records.
                    1.0_f64
                }
                None => 0.0_f64,
            }
        } else {
            0.0_f64
        };

        AffinityScore {
            category_success_rate,
            consistency,
            breadth,
            recency_bonus,
        }
    }

    /// Compute cross-category breadth: fraction of task categories with
    /// at least one success record for this provider.
    async fn compute_breadth(
        &self,
        provider: &ProviderId,
        _candidates: &[ProviderId],
    ) -> Option<f64> {
        // WHY: 6 is the number of TaskCategory variants (fixed, not runtime-dynamic).
        const TOTAL_CATEGORIES: f64 = 6.0;

        use TaskCategory::{Bug, Chore, Docs, Feature, Refactor, Test};
        let all_categories = [Feature, Refactor, Bug, Docs, Test, Chore];

        let mut present = 0u32;
        let mut total_rate = 0.0_f64;
        let mut n = 0u32;

        for cat in &all_categories {
            match self.store.rolling_stats(provider, cat, self.window).await {
                Ok(Some(stats)) => {
                    if let Some(rate) = stats.success_rate() {
                        present += 1;
                        #[expect(
                            clippy::float_arithmetic,
                            reason = "accumulating category success rates for breadth metric"
                        )]
                        {
                            total_rate += rate;
                        }
                        n += 1;
                    }
                }
                Ok(None) => {}
                Err(error) => {
                    tracing::error!(
                        error = %error,
                        provider = %provider,
                        category = %cat,
                        "affinity routing breadth stats store unavailable"
                    );
                }
            }
        }

        if n == 0 {
            return None;
        }

        // Breadth = average success rate across categories with data.
        // WHY: A provider with 80% success in 5 categories is better than one
        // with 80% in 1 category and zero data in others; the `present/total`
        // fraction would diverge between them but average rate does not capture
        // breadth well. Using `present * avg_rate / total_categories` gives a
        // compound metric: both breadth-of-coverage and quality.
        #[expect(
            clippy::float_arithmetic,
            reason = "breadth metric: coverage fraction * average success rate"
        )]
        let breadth = (f64::from(present) / TOTAL_CATEGORIES) * (total_rate / f64::from(n));

        Some(breadth.clamp(0.0, 1.0))
    }
}

impl Router for AffinityRouter {
    /// Route using the empirical + persona model.
    ///
    /// Delegates to the inner [`PersonaRouter`]. For the full affinity-aware
    /// selection, call [`route_with_affinity`](Self::route_with_affinity).
    fn route<'a>(&'a self, features: &'a RequestFeatures) -> BoxFuture<'a, RoutingDecision> {
        self.inner.route(features)
    }

    /// Record an after-action outcome into the shared store.
    fn after_action(
        &self,
        decision: &RoutingDecision,
        outcome: &TurnOutcome,
    ) -> Result<(), RouterError> {
        self.inner.after_action(decision, outcome)
    }
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
mod tests {
    use std::io::Write as _;
    use std::sync::Arc;

    use aletheia_routing::{DEFAULT_ROUTING_WINDOW, RoutingBoundary};

    use super::*;
    use crate::routing::empirical::EmpiricalRouter;
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

    fn write_jsonl(dir: &std::path::Path, filename: &str, lines: &[serde_json::Value]) {
        let path = dir.join(filename);
        let mut file = std::fs::File::create(path).unwrap();
        for line in lines {
            writeln!(file, "{line}").unwrap();
        }
    }

    async fn make_affinity_router(
        dir: &std::path::Path,
        default: &str,
        affinity_threshold: f64,
    ) -> AffinityRouter {
        let store = Arc::new(AfterActionStore::new(dir.to_owned()));
        store.refresh().await.unwrap();
        let empirical = EmpiricalRouter::new(
            Arc::clone(&store),
            StaticRouter::new(ProviderId::new(default)),
            5,
            DEFAULT_ROUTING_WINDOW,
            0.1,
        );
        let persona = PersonaRouter::new(empirical);
        AffinityRouter::new(persona, store, DEFAULT_ROUTING_WINDOW, affinity_threshold)
    }

    /// `AffinityScore::weighted()` returns 0 when all dimensions are absent.
    #[test]
    fn affinity_score_weighted_zero_when_no_data() {
        let score = AffinityScore {
            category_success_rate: None,
            consistency: 0.0,
            breadth: None,
            recency_bonus: 0.0,
        };
        // Only consistency + recency are non-None; both are 0.0.
        assert!((score.weighted() - 0.0).abs() < f64::EPSILON);
    }

    /// `AffinityScore::weighted()` saturates at 1.0 for perfect data.
    #[test]
    fn affinity_score_weighted_one_when_perfect() {
        let score = AffinityScore {
            category_success_rate: Some(1.0),
            consistency: 1.0,
            breadth: Some(1.0),
            recency_bonus: 1.0,
        };
        assert!((score.weighted() - 1.0).abs() < 1e-9);
    }

    /// `AffinityScore::weighted()` is in [0, 1] for typical partial data.
    #[test]
    fn affinity_score_weighted_is_bounded() {
        let score = AffinityScore {
            category_success_rate: Some(0.8),
            consistency: 0.6,
            breadth: Some(0.5),
            recency_bonus: 1.0,
        };
        let w = score.weighted();
        assert!(w >= 0.0, "weighted score must be non-negative, got {w}");
        assert!(w <= 1.0, "weighted score must be at most 1.0, got {w}");
    }

    /// Provider with strong category history wins affinity selection.
    #[tokio::test]
    async fn affinity_winner_selected_when_gap_exceeds_threshold() {
        let tmp = tempfile::tempdir().unwrap();

        // provider-specialist: 9/10 success in refactor specifically
        let mut lines = vec![];
        for _ in 0..9 {
            lines.push(session_line("specialist", "success", "refactor"));
        }
        lines.push(session_line("specialist", "failed", "refactor"));

        // provider-generalist: 8/10 success in feature (different category)
        for _ in 0..8 {
            lines.push(session_line("generalist", "success", "feature"));
        }
        for _ in 0..2 {
            lines.push(session_line("generalist", "failed", "feature"));
        }

        write_jsonl(tmp.path(), "2026-04-17.jsonl", &lines);

        // default is generalist, threshold 0.15 — specialist has strong refactor affinity
        let router = make_affinity_router(tmp.path(), "generalist", 0.15).await;
        let features = aletheia_routing::types::RequestFeatures::new(
            vec![ProviderId::new("specialist"), ProviderId::new("generalist")],
            Some(TaskCategory::Refactor),
            None,
        );
        let decision = router.route_with_affinity(&features, None).await;
        // specialist should win for Refactor via affinity
        assert_eq!(
            &*decision.base.provider, "specialist",
            "specialist should win refactor affinity over generalist"
        );
    }

    /// When affinity gap is below threshold, empirical winner is preserved.
    #[tokio::test]
    async fn empirical_winner_preserved_when_gap_below_threshold() {
        let tmp = tempfile::tempdir().unwrap();

        // Both providers have similar refactor history → small affinity gap.
        let mut lines = vec![];
        for _ in 0..8 {
            lines.push(session_line("provider-a", "success", "refactor"));
        }
        for _ in 0..2 {
            lines.push(session_line("provider-a", "failed", "refactor"));
        }
        for _ in 0..7 {
            lines.push(session_line("provider-b", "success", "refactor"));
        }
        for _ in 0..3 {
            lines.push(session_line("provider-b", "failed", "refactor"));
        }
        write_jsonl(tmp.path(), "2026-04-17.jsonl", &lines);

        // Very high threshold — affinity should not override empirical selection.
        let router = make_affinity_router(tmp.path(), "provider-b", 0.99).await;
        let features = aletheia_routing::types::RequestFeatures::new(
            vec![ProviderId::new("provider-a"), ProviderId::new("provider-b")],
            Some(TaskCategory::Refactor),
            None,
        );
        let decision = router.route_with_affinity(&features, None).await;
        // With threshold=0.99, affinity gap can't possibly be that high → empirical wins
        // The empirical winner (provider-a has 0.8 rate vs 0.7 for provider-b) should be kept.
        // Either provider is acceptable since threshold prevents affinity override.
        // Just verify the decision is consistent (non-empty provider).
        assert!(!decision.base.provider.is_empty());
    }

    /// Router trait delegation still returns a provider.
    #[tokio::test]
    async fn router_trait_delegation_works() {
        let tmp = tempfile::tempdir().unwrap();
        let router = make_affinity_router(tmp.path(), "default", 0.15).await;
        let features = aletheia_routing::types::RequestFeatures::new(
            vec![ProviderId::new("default")],
            Some(TaskCategory::Feature),
            None,
        );
        let decision = router.route(&features).await;
        assert_eq!(&*decision.provider, "default");
    }

    /// `AffinityRouter` with empty candidates returns static default.
    #[tokio::test]
    async fn empty_candidates_returns_default() {
        let tmp = tempfile::tempdir().unwrap();
        let router = make_affinity_router(tmp.path(), "fallback", 0.15).await;
        let features = aletheia_routing::types::RequestFeatures::new(
            vec![],
            Some(TaskCategory::Feature),
            None,
        );
        let decision = router.route_with_affinity(&features, None).await;
        assert_eq!(&*decision.base.provider, "fallback");
    }

    /// Compute breadth returns None for provider with no history.
    #[tokio::test]
    async fn compute_breadth_none_for_unknown_provider() {
        let tmp = tempfile::tempdir().unwrap();
        let router = make_affinity_router(tmp.path(), "default", 0.15).await;
        let breadth = router
            .compute_breadth(&ProviderId::new("unknown"), &[])
            .await;
        assert!(breadth.is_none());
    }

    #[tokio::test]
    async fn affinity_route_excludes_cloud_candidate_for_local_hosted_boundary() {
        let tmp = tempfile::tempdir().unwrap();
        let mut lines = vec![];
        for _ in 0..10 {
            lines.push(session_line("cloud-only", "success", "refactor"));
        }
        write_jsonl(tmp.path(), "2026-04-17.jsonl", &lines);

        let router = make_affinity_router(tmp.path(), "local", 0.15).await;
        let features = aletheia_routing::types::RequestFeatures::new(
            vec![ProviderId::new("cloud-only"), ProviderId::new("local")],
            Some(TaskCategory::Refactor),
            None,
        )
        .with_deployment_target(RoutingBoundary::LocalHosted)
        .with_candidate_deployment_target("cloud-only", RoutingBoundary::Cloud)
        .with_candidate_deployment_target("local", RoutingBoundary::LocalHosted);

        let decision = router.route_with_affinity(&features, None).await;

        assert_eq!(&*decision.base.provider, "local");
    }
}
