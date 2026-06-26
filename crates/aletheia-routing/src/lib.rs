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
//!    `energeia` empirical router with a richer signal set.

#![deny(missing_docs)]

pub mod router;
pub mod store;
pub mod types;

pub use router::{BoxFuture, Router};
pub use store::{AfterActionStore, DEFAULT_ROUTING_WINDOW};
pub use types::{InteractiveOutcome, RequestFeatures, RouterError, RoutingBoundary, RoutingDecision, TurnOutcome};

use std::sync::Arc;

use tracing::Instrument;

/// A no-op router used when no empirical router is configured.
///
/// Always returns the configured static provider and discards after-action
/// records. Satisfies `Arc<dyn Router>` without requiring fjall.
pub struct NoOpRouter {
    /// The static provider returned for all requests.
    pub provider: Arc<str>,
}

impl Router for NoOpRouter {
    fn route<'a>(&'a self, _features: &'a RequestFeatures) -> BoxFuture<'a, RoutingDecision> {
        let provider = self.provider.clone();
        Box::pin(async move { RoutingDecision::new(provider, None) })
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
    /// Shared empirical outcome store.
    store: Arc<AfterActionStore>,
    /// Static provider/model returned for route calls.
    provider: Arc<str>,
}

impl RecordingRouter {
    /// Create a router that records outcomes while preserving static routing.
    #[must_use]
    pub fn new(store: Arc<AfterActionStore>, provider: impl Into<Arc<str>>) -> Self {
        Self {
            store,
            provider: provider.into(),
        }
    }
}

impl Router for RecordingRouter {
    fn route<'a>(&'a self, _features: &'a RequestFeatures) -> BoxFuture<'a, RoutingDecision> {
        let provider = self.provider.clone();
        Box::pin(async move { RoutingDecision::new(provider, None) })
    }

    fn after_action(
        &self,
        _decision: &RoutingDecision,
        outcome: &TurnOutcome,
    ) -> Result<(), RouterError> {
        let store = Arc::clone(&self.store);
        let outcome = outcome.clone();
        tokio::spawn(
            async move {
                if let Err(error) = store.record_outcome(&outcome).await {
                    tracing::error!(
                        error = %error,
                        provider = %outcome.provider,
                        category = %outcome.task_category,
                        success = outcome.success,
                        "recording router failed to store after-action outcome"
                    );
                }
            }
            .instrument(tracing::Span::current()),
        );
        Ok(())
    }
}

/// A router combinator that falls through to a secondary router when the
/// primary router's confidence is below a threshold.
///
/// WHY(#3969): the Q-learner (and any learned router) needs a way to defer to
/// a static or rule-based fallback when it has insufficient data to make a
/// high-confidence decision. `FallthroughRouter` is that combinator: it runs
/// the primary router first, and if `confidence < threshold` (or the primary
/// returns `None` confidence), delegates to the secondary.
///
/// Both `after_action` calls are forwarded to the primary router only. The
/// secondary is a read-only fallback; recording against it would corrupt the
/// primary's training signal.
///
#[cfg(test)]
pub(crate) struct FallthroughRouter {
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

#[cfg(test)]
impl FallthroughRouter {
    /// Create a new `FallthroughRouter`.
    ///
    /// `threshold` is clamped to `[0.0, 1.0]`.
    #[must_use]
    pub(crate) fn new(primary: Arc<dyn Router>, fallback: Arc<dyn Router>, threshold: f64) -> Self {
        Self {
            primary,
            fallback,
            threshold: threshold.clamp(0.0, 1.0),
        }
    }

    /// Configured fallthrough confidence threshold.
    #[must_use]
    pub(crate) fn threshold(&self) -> f64 {
        self.threshold
    }
}

#[cfg(test)]
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
                decision
            } else {
                self.fallback.route(features).await
            }
        })
    }

    fn after_action(
        &self,
        decision: &RoutingDecision,
        outcome: &TurnOutcome,
    ) -> Result<(), RouterError> {
        // WHY: only the primary receives after-action records so the learning
        // signal is not diluted by fallback decisions.
        self.primary.after_action(decision, outcome)
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
}
