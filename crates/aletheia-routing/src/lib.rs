//! Shared routing trait and empirical success-rate storage.
//!
//! This crate defines the [`Router`] trait and supporting types that are used
//! by both the dispatch path (`energeia`) and the interactive path (`nous`).
//! Sharing the trait and storage backend ensures that empirical learnings from
//! dispatch sessions and interactive turns feed the same success-rate model.
//!
//! # Precedence (interactive path)
//!
//! The interactive pipeline uses a two-layer routing strategy:
//!
//! 1. **Complexity router** (`hermeneus::complexity`) — fast-path default.
//!    Scores query complexity and maps it to a model tier (Haiku/Sonnet/Opus).
//!    Zero I/O, runs synchronously on every turn.
//!
//! 2. **Empirical feedback** (`Router::after_action`) — augments the above.
//!    After each turn completes, the outcome is recorded into the shared
//!    [`AfterActionStore`] so future dispatch-side routing benefits from
//!    interactive-path data (and vice versa). The empirical layer does *not*
//!    replace the complexity router; it feeds the dispatch path's
//!    `energeia` empirical router with a richer signal set. Persistence is
//!    durable, not merely in-memory-until-the-next-refresh (#4519): see
//!    the module docs on [`store`] for the exact semantics.
//!
//! The interactive actor is wired with a real [`EmpiricalRouter`] wrapped in
//! a [`FallthroughRouter`] over a static default (#3969), so `route()` now
//! computes a genuine data-driven decision from the same store — but no
//! interactive call site consults that decision for model selection yet;
//! today it is exercised only via `after_action` recording and its own
//! tests. Wiring `route()`'s output into turn model selection (alongside the
//! complexity router above) is separate, still-open follow-up work, not a
//! claim this module makes.

#![deny(missing_docs)]

pub mod router;
pub mod store;
pub mod types;

pub use router::{BoxFuture, Router};
pub use store::{AfterActionStore, DEFAULT_ROUTING_WINDOW};
pub use types::{
    AppliedBoundary, BoundarySource, DecisionOrigin, DecisionProvenance, FallbackReason,
    IngressSource, InteractiveOutcome, ProviderId, RequestFeatures, RouterError, RoutingBoundary,
    RoutingDecision, TurnOutcome,
};

use std::sync::Arc;
use std::time::Duration;

use tokio::sync::mpsc;
use tokio::task::JoinHandle;

/// A no-op router used when no empirical router is configured.
///
/// Always returns the configured static provider and discards after-action
/// records. Satisfies `Arc<dyn Router>` without requiring fjall.
pub struct NoOpRouter {
    /// The static provider returned for all requests.
    pub provider: Arc<str>,
}

impl Router for NoOpRouter {
    fn route<'a>(&'a self, features: &'a RequestFeatures) -> BoxFuture<'a, RoutingDecision> {
        let provider = self.provider.clone();
        Box::pin(
            async move { RoutingDecision::new(provider, None).with_request_provenance(features) },
        )
    }

    fn after_action(
        &self,
        _decision: &RoutingDecision,
        _outcome: &TurnOutcome,
    ) -> Result<(), RouterError> {
        Ok(())
    }
}

/// A static router that records after-action outcomes into a shared store.
///
/// This is the interactive-runtime counterpart to the richer dispatch
/// empirical routers: it does not change provider selection, but it prevents
/// completed turns from being discarded when the binary has not enabled an
/// empirical selection policy.
pub struct RecordingRouter {
    /// Static provider/model returned for route calls.
    provider: Arc<str>,
    /// Sender to the background outcome-drain task.
    outcome_tx: mpsc::UnboundedSender<TurnOutcome>,
    /// Background task handle kept alive for the router lifetime.
    ///
    /// WHY(#5740): owning the handle is the point. Dropping it — as the
    /// previous per-outcome `tokio::spawn` did — leaves a task nothing can
    /// cancel, observe or bound, so a panic in the write path vanishes and
    /// shutdown races the writes. Held here, the task is cancelled when the
    /// router is dropped, and a closed channel becomes a reportable error.
    _outcome_drain: JoinHandle<()>,
}

impl RecordingRouter {
    /// Create a router that records outcomes while preserving static routing.
    #[must_use]
    pub fn new(store: Arc<AfterActionStore>, provider: impl Into<Arc<str>>) -> Self {
        // WHY: mirrors `energeia::routing::empirical::EmpiricalRouter` — a
        // single drain task serializes the async store writes so the sync
        // `after_action` trait method never blocks the response path, and the
        // two implementations of the same trait method do not diverge in how
        // they treat the write.
        let (outcome_tx, mut outcome_rx) = mpsc::unbounded_channel::<TurnOutcome>();
        let outcome_drain = tokio::spawn(async move {
            while let Some(outcome) = outcome_rx.recv().await {
                store.record_outcome(&outcome).await;
            }
        });

        Self {
            provider: provider.into(),
            outcome_tx,
            _outcome_drain: outcome_drain,
        }
    }
}

impl Router for RecordingRouter {
    fn route<'a>(&'a self, features: &'a RequestFeatures) -> BoxFuture<'a, RoutingDecision> {
        let provider = self.provider.clone();
        Box::pin(
            async move { RoutingDecision::new(provider, None).with_request_provenance(features) },
        )
    }

    fn after_action(
        &self,
        _decision: &RoutingDecision,
        outcome: &TurnOutcome,
    ) -> Result<(), RouterError> {
        // WHY: a closed channel is the one way this write can actually fail —
        // it means the drain task panicked or the runtime is shutting down.
        // Reporting it to the caller is what distinguishes "recorded" from
        // "silently discarded", which the previous fire-and-forget could not.
        self.outcome_tx
            .send(outcome.clone())
            .map_err(|_closed| RouterError::AfterActionWrite {
                message: "after-action outcome channel closed (drain task panicked \
                    or runtime shutting down)"
                    .to_string(),
            })?;
        Ok(())
    }
}

/// A router that selects the empirically best-performing candidate from a
/// shared [`AfterActionStore`], falling back to a static provider when data
/// is absent, insufficient, or not a confident win.
///
/// WHY(#3969): the interactive-runtime counterpart to
/// `energeia::routing::empirical::EmpiricalRouter` — both delegate to
/// [`AfterActionStore::pick_winner`] so "confident enough to switch" is
/// defined in exactly one place regardless of which path is asking. Unlike
/// [`RecordingRouter`] (which always returns the static provider), `route()`
/// here makes a real empirical decision.
pub struct EmpiricalRouter {
    /// Shared success-rate store, read for the decision and written by
    /// `after_action`.
    store: Arc<AfterActionStore>,
    /// Provider returned when data is absent, thin, or not a confident win.
    static_choice: types::ProviderId,
    /// Minimum rolling-window record count before a candidate can win.
    min_samples: u64,
    /// Rolling window for record queries.
    window: Duration,
    /// Minimum success-rate gap (winner − static) required to switch away
    /// from `static_choice`.
    confidence_threshold: f64,
    /// Sender to the background outcome-drain task.
    outcome_tx: mpsc::UnboundedSender<TurnOutcome>,
    /// Background task handle kept alive for the router lifetime (see
    /// [`RecordingRouter::_outcome_drain`] for why this must be owned).
    _outcome_drain: JoinHandle<()>,
}

impl EmpiricalRouter {
    /// Create a new empirical router over `store`.
    #[must_use]
    pub fn new(
        store: Arc<AfterActionStore>,
        static_choice: impl Into<Arc<str>>,
        min_samples: u64,
        window: Duration,
        confidence_threshold: f64,
    ) -> Self {
        let static_choice = types::ProviderId::new(static_choice.into());
        // WHY: mirrors RecordingRouter's drain task — see its constructor.
        let (outcome_tx, mut outcome_rx) = mpsc::unbounded_channel::<TurnOutcome>();
        let drain_store = Arc::clone(&store);
        let outcome_drain = tokio::spawn(async move {
            while let Some(outcome) = outcome_rx.recv().await {
                drain_store.record_outcome(&outcome).await;
            }
        });

        Self {
            store,
            static_choice,
            min_samples,
            window,
            confidence_threshold,
            outcome_tx,
            _outcome_drain: outcome_drain,
        }
    }
}

impl Router for EmpiricalRouter {
    fn route<'a>(&'a self, features: &'a RequestFeatures) -> BoxFuture<'a, RoutingDecision> {
        Box::pin(async move {
            let category = features.effective_category();
            let candidates = features
                .candidates
                .iter()
                .filter(|provider| features.candidate_allowed_by_boundary(provider))
                .cloned()
                .collect::<Vec<_>>();
            let static_allowed = features.candidate_allowed_by_boundary(&self.static_choice);

            let chosen = match self
                .store
                .pick_winner(
                    &category,
                    &candidates,
                    &self.static_choice,
                    self.min_samples,
                    self.window,
                    self.confidence_threshold,
                    static_allowed,
                )
                .await
            {
                Ok(chosen) => chosen,
                Err(error) => {
                    tracing::error!(
                        error = %error,
                        category = %category,
                        "empirical router unavailable, using static fallback"
                    );
                    self.static_choice.clone()
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

            RoutingDecision::new(chosen.0, confidence).with_request_provenance(features)
        })
    }

    fn after_action(
        &self,
        _decision: &RoutingDecision,
        outcome: &TurnOutcome,
    ) -> Result<(), RouterError> {
        self.outcome_tx
            .send(outcome.clone())
            .map_err(|_closed| RouterError::AfterActionWrite {
                message: "after-action outcome channel closed (drain task panicked \
                    or runtime shutting down)"
                    .to_string(),
            })?;
        Ok(())
    }
}

/// A router combinator that falls through to a secondary router when the
/// primary router's confidence is below a threshold.
///
/// WHY(#3969): a learned or empirical router needs a way to defer to a
/// static or rule-based fallback when it has insufficient data to make a
/// high-confidence decision. `FallthroughRouter` is that combinator: it runs
/// the primary router first, and if `confidence < threshold` (or the primary
/// returns `None` confidence), delegates to the secondary. Production use:
/// [`crate::EmpiricalRouter`] wrapped as primary over a static fallback on
/// the interactive nous actor path (see `aletheia::runtime` wiring).
///
/// Every returned decision carries [`DecisionOrigin`] provenance (#5218):
/// `Primary` when the primary's decision was accepted, or
/// `Fallback { primary_provider, reason }` naming what the stack fell back
/// *from* and *why*. `after_action` forwards the outcome to the router that
/// actually handled the request: a fallback-handled outcome goes to the
/// fallback (a `NoOpRouter` discards it; a recording fallback learns from
/// it), so the primary never receives outcome signal for a decision it did
/// not make.
pub struct FallthroughRouter {
    /// Primary router — queried first on every `route` call.
    primary: Arc<dyn Router>,
    /// Fallback router — used when primary confidence is below threshold.
    fallback: Arc<dyn Router>,
    /// Minimum confidence required to accept the primary decision.
    ///
    /// Must be in `[0.0, 1.0]`. A value of `0.0` means always accept the
    /// primary decision; `1.0` means always fall through.
    threshold: f64,
}

impl FallthroughRouter {
    /// Create a new `FallthroughRouter`.
    ///
    /// `threshold` is clamped to `[0.0, 1.0]`.
    #[must_use]
    pub fn new(primary: Arc<dyn Router>, fallback: Arc<dyn Router>, threshold: f64) -> Self {
        Self {
            primary,
            fallback,
            threshold: threshold.clamp(0.0, 1.0),
        }
    }

    /// Configured fallthrough confidence threshold.
    #[must_use]
    pub fn threshold(&self) -> f64 {
        self.threshold
    }
}

impl Router for FallthroughRouter {
    fn route<'a>(&'a self, features: &'a RequestFeatures) -> BoxFuture<'a, RoutingDecision> {
        Box::pin(async move {
            let decision = self.primary.route(features).await;
            // WHY(#3969): fall through when confidence is absent or below
            // the threshold. A primary that returns None confidence is treated
            // as having zero confidence so pure static routers always fall
            // through, letting the secondary handle the request.
            let confidence = decision.confidence.unwrap_or(0.0);
            if confidence >= self.threshold {
                decision.with_origin(types::DecisionOrigin::Primary)
            } else {
                // WHY(#5218): the fallback's decision must say what it fell
                // back from and why, so after-action attribution and audit
                // can distinguish "the primary chose this" from "the fallback
                // handled this because the primary could not".
                let reason = match decision.confidence {
                    None => types::FallbackReason::ConfidenceAbsent,
                    Some(reported) => types::FallbackReason::ConfidenceBelowThreshold {
                        confidence: reported,
                        threshold: self.threshold,
                    },
                };
                let primary_provider = decision.provider.clone();
                self.fallback
                    .route(features)
                    .await
                    .with_origin(types::DecisionOrigin::Fallback {
                        primary_provider,
                        reason,
                    })
            }
        })
    }

    fn after_action(
        &self,
        decision: &RoutingDecision,
        outcome: &TurnOutcome,
    ) -> Result<(), RouterError> {
        // WHY(#5218): the outcome goes to the router that actually handled
        // the request. Forwarding a fallback-handled outcome to the primary
        // taught the primary success or failure for a decision it did not
        // make, while the fallback never learned. Decisions without
        // fallthrough provenance (e.g. fabricated at an after-action-only
        // call site) keep the historical behaviour of landing on the primary.
        match decision.provenance.origin {
            types::DecisionOrigin::Fallback { .. } => self.fallback.after_action(decision, outcome),
            types::DecisionOrigin::Direct | types::DecisionOrigin::Primary => {
                self.primary.after_action(decision, outcome)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use crate::types::{ProviderId, TaskCategory};

    use super::*;

    #[tokio::test]
    async fn recording_router_preserves_static_route() {
        let store = Arc::new(AfterActionStore::in_memory());
        let router = RecordingRouter::new(store, "claude-sonnet");
        let decision = router
            .route(&RequestFeatures::new(Vec::new(), None, None))
            .await;

        assert_eq!(decision.provider.as_ref(), "claude-sonnet");
        assert_eq!(decision.confidence, None);
    }

    #[tokio::test]
    async fn recording_router_records_after_action_into_store() {
        let store = Arc::new(AfterActionStore::in_memory());
        let router = RecordingRouter::new(Arc::clone(&store), "claude-sonnet");
        let provider = ProviderId::new("claude-sonnet");
        let outcome = TurnOutcome::new(provider.clone(), TaskCategory::Feature, true, true);
        let decision = RoutingDecision::new("claude-sonnet", None);

        assert!(router.after_action(&decision, &outcome).is_ok());

        for _ in 0..10 {
            match store
                .rolling_stats(&provider, &TaskCategory::Feature, Duration::from_hours(168))
                .await
            {
                Ok(Some(stats)) => {
                    assert_eq!(stats.successes, 1);
                    assert_eq!(stats.total, 1);
                    return;
                }
                Ok(None) => {}
                Err(error) => panic!("rolling stats query failed: {error}"),
            }
            tokio::task::yield_now().await;
        }

        panic!("recording router did not write outcome");
    }

    /// WHY(#5740): the drain task must absorb a burst without losing records.
    /// The per-outcome `tokio::spawn` this replaced had no ordering or
    /// completion guarantee between concurrently spawned writes, and nothing
    /// bounded how many were in flight at once.
    #[tokio::test]
    async fn recording_router_drains_every_outcome_through_one_task() {
        let store = Arc::new(AfterActionStore::in_memory());
        let router = RecordingRouter::new(Arc::clone(&store), "claude-sonnet");
        let provider = ProviderId::new("claude-sonnet");
        let decision = RoutingDecision::new("claude-sonnet", None);

        for i in 0..64u32 {
            let outcome =
                TurnOutcome::new(provider.clone(), TaskCategory::Feature, i % 2 == 0, true);
            assert!(
                router.after_action(&decision, &outcome).is_ok(),
                "drain channel must accept the outcome"
            );
        }

        for _ in 0..1000 {
            if let Ok(Some(stats)) = store
                .rolling_stats(&provider, &TaskCategory::Feature, Duration::from_hours(168))
                .await
                && stats.total == 64
            {
                assert_eq!(stats.successes, 32);
                assert_eq!(stats.failures, 32);
                return;
            }
            tokio::task::yield_now().await;
        }

        panic!("recording router dropped outcomes on the way to the store");
    }

    /// WHY(#5740): this is the failure the issue asked for a test of, in the
    /// only form the code can actually produce. `AfterActionStore::record_outcome`
    /// writes three in-memory maps and cannot fail, so no store-write error is
    /// injectable; the reachable failure is a drain task that is gone, which
    /// the previous fire-and-forget reported as success.
    #[tokio::test]
    #[expect(
        clippy::used_underscore_binding,
        reason = "test deliberately reaches into the leading-underscore field to abort the \
            drain task directly; the underscore only signals 'held, not read' to production code"
    )]
    async fn recording_router_reports_a_closed_drain_channel() {
        let mut router =
            RecordingRouter::new(Arc::new(AfterActionStore::in_memory()), "claude-sonnet");

        // Abort the drain task and wait for the abort to land, so the receiver
        // is deterministically gone before `after_action` runs below.
        // `runtime.shutdown_timeout` (the previous approach here) only bounds
        // how long shutdown waits on *blocking* tasks; it can return before
        // the scheduler has actually dropped a parked async task, which raced
        // this test's assertion against the drain task's real teardown.
        router._outcome_drain.abort();
        let _ = (&mut router._outcome_drain).await;

        let outcome = TurnOutcome::new(
            ProviderId::new("claude-sonnet"),
            TaskCategory::Feature,
            true,
            true,
        );
        let Err(error) =
            router.after_action(&RoutingDecision::new("claude-sonnet", None), &outcome)
        else {
            panic!("a dropped drain task must not report a recorded outcome");
        };

        let RouterError::AfterActionWrite { message } = error;
        assert!(
            message.contains("channel closed"),
            "unexpected message: {message}"
        );
    }

    // WHY(#3969): FallthroughRouter must accept the primary decision when its
    // confidence meets or exceeds the threshold.
    #[tokio::test]
    async fn fallthrough_router_uses_primary_when_confidence_meets_threshold() {
        // A mock router that always returns a fixed decision with confidence 0.8.
        struct ConfidentRouter;
        impl Router for ConfidentRouter {
            fn route<'a>(
                &'a self,
                _features: &'a RequestFeatures,
            ) -> BoxFuture<'a, RoutingDecision> {
                Box::pin(async { RoutingDecision::new("primary", Some(0.8)) })
            }
            fn after_action(
                &self,
                _decision: &RoutingDecision,
                _outcome: &TurnOutcome,
            ) -> Result<(), RouterError> {
                Ok(())
            }
        }
        let primary = Arc::new(ConfidentRouter);
        let fallback = Arc::new(NoOpRouter {
            provider: Arc::from("fallback"),
        });
        let router = FallthroughRouter::new(primary, fallback, 0.5);
        let decision = router
            .route(&RequestFeatures::new(Vec::new(), None, None))
            .await;
        assert_eq!(decision.provider.as_ref(), "primary");
        assert_eq!(decision.confidence, Some(0.8));
    }

    // WHY(#3969): FallthroughRouter must delegate to the fallback when the
    // primary confidence is below the threshold.
    #[tokio::test]
    async fn fallthrough_router_uses_fallback_when_confidence_below_threshold() {
        struct LowConfidenceRouter;
        impl Router for LowConfidenceRouter {
            fn route<'a>(
                &'a self,
                _features: &'a RequestFeatures,
            ) -> BoxFuture<'a, RoutingDecision> {
                Box::pin(async { RoutingDecision::new("primary", Some(0.2)) })
            }
            fn after_action(
                &self,
                _decision: &RoutingDecision,
                _outcome: &TurnOutcome,
            ) -> Result<(), RouterError> {
                Ok(())
            }
        }
        let primary = Arc::new(LowConfidenceRouter);
        let fallback = Arc::new(NoOpRouter {
            provider: Arc::from("fallback"),
        });
        let router = FallthroughRouter::new(primary, fallback, 0.5);
        let decision = router
            .route(&RequestFeatures::new(Vec::new(), None, None))
            .await;
        assert_eq!(decision.provider.as_ref(), "fallback");
    }

    // WHY(#5218): after-action attribution must follow the router that
    // actually handled the request. A fallback-handled outcome recorded
    // against the primary taught it outcomes for decisions it never made,
    // while the fallback never learned.
    struct CountingRouter {
        provider: &'static str,
        confidence: Option<f64>,
        decisions: std::sync::Mutex<Vec<String>>,
    }

    impl CountingRouter {
        fn fixed(provider: &'static str, confidence: Option<f64>) -> Self {
            Self {
                provider,
                confidence,
                decisions: std::sync::Mutex::new(Vec::new()),
            }
        }

        fn recorded(&self) -> Vec<String> {
            self.decisions
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .clone()
        }
    }

    impl Router for CountingRouter {
        fn route<'a>(&'a self, _features: &'a RequestFeatures) -> BoxFuture<'a, RoutingDecision> {
            let provider = self.provider;
            let confidence = self.confidence;
            Box::pin(async move { RoutingDecision::new(provider, confidence) })
        }
        fn after_action(
            &self,
            decision: &RoutingDecision,
            _outcome: &TurnOutcome,
        ) -> Result<(), RouterError> {
            self.decisions
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .push(decision.provider.to_string());
            Ok(())
        }
    }

    #[tokio::test]
    async fn fallthrough_after_action_reaches_fallback_when_fallback_handled() {
        let primary = Arc::new(CountingRouter::fixed("primary", Some(0.2)));
        let fallback = Arc::new(CountingRouter::fixed("fallback", None));
        let router = FallthroughRouter::new(primary.clone(), fallback.clone(), 0.5);

        let features = RequestFeatures::new(Vec::new(), None, None);
        let decision = router.route(&features).await;

        assert_eq!(decision.provider.as_ref(), "fallback");
        let types::DecisionOrigin::Fallback {
            primary_provider,
            reason,
        } = &decision.provenance.origin
        else {
            panic!(
                "fallback-handled decision must carry Fallback origin, got {:?}",
                decision.provenance.origin
            );
        };
        assert_eq!(primary_provider.as_ref(), "primary");
        assert_eq!(
            *reason,
            types::FallbackReason::ConfidenceBelowThreshold {
                confidence: 0.2,
                threshold: 0.5,
            }
        );

        let outcome = TurnOutcome::new(ProviderId::new("fallback"), TaskCategory::Bug, true, true);
        assert!(router.after_action(&decision, &outcome).is_ok());

        assert_eq!(
            fallback.recorded(),
            vec!["fallback".to_owned()],
            "the fallback handled the request, so the fallback learns"
        );
        assert!(
            primary.recorded().is_empty(),
            "the primary must not receive signal for a decision it did not make"
        );
    }

    #[tokio::test]
    async fn fallthrough_after_action_reaches_primary_when_primary_handled() {
        let primary = Arc::new(CountingRouter::fixed("primary", Some(0.9)));
        let fallback = Arc::new(CountingRouter::fixed("fallback", None));
        let router = FallthroughRouter::new(primary.clone(), fallback.clone(), 0.5);

        let features = RequestFeatures::new(Vec::new(), None, None);
        let decision = router.route(&features).await;

        assert_eq!(decision.provider.as_ref(), "primary");
        assert_eq!(decision.provenance.origin, types::DecisionOrigin::Primary);

        let outcome = TurnOutcome::new(ProviderId::new("primary"), TaskCategory::Bug, true, true);
        assert!(router.after_action(&decision, &outcome).is_ok());

        assert_eq!(primary.recorded(), vec!["primary".to_owned()]);
        assert!(fallback.recorded().is_empty());
    }

    // WHY(#5218): a primary that reports no confidence at all falls through
    // with ConfidenceAbsent — the reason is part of the durable decision
    // record, not a log line.
    #[tokio::test]
    async fn fallthrough_records_confidence_absent_as_fallback_reason() {
        let primary = Arc::new(CountingRouter::fixed("primary", None));
        let fallback = Arc::new(CountingRouter::fixed("fallback", None));
        let router = FallthroughRouter::new(primary, fallback, 0.5);

        let decision = router
            .route(&RequestFeatures::new(Vec::new(), None, None))
            .await;

        let types::DecisionOrigin::Fallback { reason, .. } = &decision.provenance.origin else {
            panic!(
                "expected Fallback origin, got {:?}",
                decision.provenance.origin
            );
        };
        assert_eq!(*reason, types::FallbackReason::ConfidenceAbsent);
    }

    // WHY(#5218): a decision fabricated by an after-action-only call site
    // (origin Direct — e.g. nous's finalize-time record) keeps the historical
    // behaviour of landing on the primary.
    #[tokio::test]
    async fn fallthrough_after_action_defaults_direct_origin_to_primary() {
        let primary = Arc::new(CountingRouter::fixed("primary", Some(0.9)));
        let fallback = Arc::new(CountingRouter::fixed("fallback", None));
        let router = FallthroughRouter::new(primary.clone(), fallback.clone(), 0.5);

        let fabricated = RoutingDecision::new("primary", None);
        let outcome = TurnOutcome::new(ProviderId::new("primary"), TaskCategory::Bug, true, true);
        assert!(router.after_action(&fabricated, &outcome).is_ok());

        assert_eq!(primary.recorded(), vec!["primary".to_owned()]);
        assert!(fallback.recorded().is_empty());
    }

    // WHY(#3969): a primary that returns None confidence (e.g. static router)
    // should always fall through — None is treated as 0.0.
    #[tokio::test]
    async fn fallthrough_router_treats_none_confidence_as_zero() {
        let primary = Arc::new(NoOpRouter {
            provider: Arc::from("primary"),
        });
        let fallback = Arc::new(NoOpRouter {
            provider: Arc::from("fallback"),
        });
        // threshold > 0 so None confidence always falls through.
        let router = FallthroughRouter::new(primary, fallback, 0.1);
        let decision = router
            .route(&RequestFeatures::new(Vec::new(), None, None))
            .await;
        assert_eq!(decision.provider.as_ref(), "fallback");
    }

    // WHY(#3969): threshold clamping at 0.0 means always accept primary.
    #[tokio::test]
    async fn fallthrough_router_threshold_zero_always_accepts_primary() {
        let primary = Arc::new(NoOpRouter {
            provider: Arc::from("primary"),
        });
        let fallback = Arc::new(NoOpRouter {
            provider: Arc::from("fallback"),
        });
        let router = FallthroughRouter::new(primary, fallback, 0.0);
        let decision = router
            .route(&RequestFeatures::new(Vec::new(), None, None))
            .await;
        // NoOpRouter returns None confidence; threshold 0.0 means
        // None(=0.0) >= 0.0 is true, so primary wins.
        assert_eq!(decision.provider.as_ref(), "primary");
    }

    #[test]
    fn fallthrough_router_threshold_getter_returns_clamped_value() {
        let primary = Arc::new(NoOpRouter {
            provider: Arc::from("primary"),
        });
        let fallback = Arc::new(NoOpRouter {
            provider: Arc::from("fallback"),
        });
        let router = FallthroughRouter::new(primary, fallback, 2.0);

        assert!((router.threshold() - 1.0).abs() < f64::EPSILON);
    }

    // WHY(#3969): EmpiricalRouter is the interactive-runtime counterpart to
    // energeia's dispatch-side EmpiricalRouter — these tests mirror
    // energeia's own `router_trait_route_returns_winner_with_confidence` /
    // `router_falls_through_when_below_min_samples` /
    // `route_excludes_cloud_candidate_for_local_hosted_boundary` coverage to
    // prove the shared `AfterActionStore::pick_winner` policy behaves
    // identically from this side.

    #[tokio::test]
    async fn empirical_router_picks_winner_with_confidence() {
        let store = Arc::new(AfterActionStore::in_memory());
        let winner = ProviderId::new("winner");
        let loser = ProviderId::new("loser");
        for i in 0..10u32 {
            store
                .record_outcome(&TurnOutcome::new(
                    winner.clone(),
                    TaskCategory::Feature,
                    i != 0,
                    true,
                ))
                .await;
        }
        for i in 0..10u32 {
            store
                .record_outcome(&TurnOutcome::new(
                    loser.clone(),
                    TaskCategory::Feature,
                    i < 2,
                    true,
                ))
                .await;
        }

        let router = EmpiricalRouter::new(
            Arc::clone(&store),
            "loser",
            5,
            Duration::from_hours(168),
            0.1,
        );
        let features = RequestFeatures::new(
            vec![winner.clone(), loser.clone()],
            Some(TaskCategory::Feature),
            None,
        );
        let decision = router.route(&features).await;

        assert_eq!(decision.provider.as_ref(), "winner");
        assert!(
            decision.confidence.is_some_and(|c| c > 0.8),
            "expected high confidence for the empirical winner, got {:?}",
            decision.confidence
        );
    }

    #[tokio::test]
    async fn empirical_router_falls_back_to_static_when_no_data() {
        let store = Arc::new(AfterActionStore::in_memory());
        let router = EmpiricalRouter::new(
            Arc::clone(&store),
            "default",
            5,
            Duration::from_hours(168),
            0.1,
        );
        let features = RequestFeatures::new(
            vec![ProviderId::new("some-candidate")],
            Some(TaskCategory::Feature),
            None,
        );
        let decision = router.route(&features).await;

        assert_eq!(decision.provider.as_ref(), "default");
    }

    #[tokio::test]
    async fn empirical_router_excludes_cloud_candidate_for_local_hosted_boundary() {
        let store = Arc::new(AfterActionStore::in_memory());
        let cloud_only = ProviderId::new("cloud-only");
        let local = ProviderId::new("local");
        for _ in 0..10u32 {
            store
                .record_outcome(&TurnOutcome::new(
                    cloud_only.clone(),
                    TaskCategory::Feature,
                    true,
                    true,
                ))
                .await;
        }
        for i in 0..10u32 {
            store
                .record_outcome(&TurnOutcome::new(
                    local.clone(),
                    TaskCategory::Feature,
                    i < 6,
                    true,
                ))
                .await;
        }

        let router = EmpiricalRouter::new(
            Arc::clone(&store),
            "cloud-only",
            5,
            Duration::from_hours(168),
            0.1,
        );
        let features = RequestFeatures::new(
            vec![cloud_only.clone(), local.clone()],
            Some(TaskCategory::Feature),
            None,
        )
        .with_deployment_target(RoutingBoundary::LocalHosted)
        .with_candidate_deployment_target("cloud-only", RoutingBoundary::Cloud)
        .with_candidate_deployment_target("local", RoutingBoundary::LocalHosted);

        let decision = router.route(&features).await;

        assert_eq!(decision.provider.as_ref(), "local");
    }

    /// WHY(#3969): proves the exact production wiring shape used by the
    /// interactive nous actor path (`aletheia::runtime`) — a
    /// `FallthroughRouter` over `EmpiricalRouter` primary and a static
    /// `NoOpRouter` secondary — degrades to the static provider on a cold
    /// store and picks the empirical winner once data exists, with a single
    /// `.route()` call site exercising both routers together.
    #[tokio::test]
    async fn fallthrough_over_empirical_and_static_matches_production_wiring() {
        let store = Arc::new(AfterActionStore::in_memory());
        let static_provider = "static-default";
        let router = FallthroughRouter::new(
            Arc::new(EmpiricalRouter::new(
                Arc::clone(&store),
                static_provider,
                5,
                Duration::from_hours(168),
                0.1,
            )),
            Arc::new(NoOpRouter {
                provider: Arc::from(static_provider),
            }),
            0.0,
        );
        let features = RequestFeatures::new(
            vec![
                ProviderId::new("learned-winner"),
                ProviderId::new(static_provider),
            ],
            Some(TaskCategory::Feature),
            None,
        );

        // Cold store: no data anywhere, so both the empirical primary and the
        // static secondary agree on the same provider.
        let cold_decision = router.route(&features).await;
        assert_eq!(cold_decision.provider.as_ref(), static_provider);

        // Seed a clear winner, then reroute the same request.
        let winner = ProviderId::new("learned-winner");
        for i in 0..10u32 {
            store
                .record_outcome(&TurnOutcome::new(
                    winner.clone(),
                    TaskCategory::Feature,
                    i != 0,
                    true,
                ))
                .await;
        }
        let warm_decision = router.route(&features).await;
        assert_eq!(warm_decision.provider.as_ref(), "learned-winner");
    }
}
