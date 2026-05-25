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
//!    [`energeia::routing::EmpiricalRouter`] with a richer signal set.

#![deny(missing_docs)]

pub mod store;
pub mod types;

pub use store::AfterActionStore;
pub use types::{RequestFeatures, RouterError, RoutingDecision, TurnOutcome};

use std::sync::Arc;

/// Re-export `BoxFuture` for use in `Router` implementations.
///
/// WHY: `async fn` in traits is not dyn-compatible in Rust (the vtable cannot
/// hold a future of unknown size). Using `BoxFuture` in the trait signature
/// makes the trait dyn-compatible so `Arc<dyn Router>` works. Implementors
/// box their futures in the `route` body using `Box::pin(async { ... })`.
pub use futures::future::BoxFuture;

/// A provider/model router that supports empirical feedback.
///
/// Implementors select a provider or model based on [`RequestFeatures`] and
/// accept [`TurnOutcome`] records after each interaction so the router can
/// improve over time.
///
/// # Dyn compatibility
///
/// `route` returns a [`BoxFuture`] rather than using `async fn` so the trait
/// is dyn-compatible and can be stored as `Arc<dyn Router>`. Implementors
/// return `Box::pin(async move { ... })` from `route`.
pub trait Router: Send + Sync {
    /// Select the best provider/model for the given request features.
    ///
    /// Called once per dispatch or interactive turn, before the LLM is
    /// invoked. Implementations must be low-latency (no synchronous I/O on
    /// the hot path).
    fn route<'a>(&'a self, features: &'a RequestFeatures) -> BoxFuture<'a, RoutingDecision>;

    /// Record the outcome of a completed turn.
    ///
    /// Called once per turn after the LLM response (and any tool iterations)
    /// complete. Used to update empirical success-rate statistics.
    ///
    /// WHY: sync so the call-site in `finalize_turn` does not need to be async.
    /// Implementations spawn any store writes as background tasks.
    fn after_action(
        &self,
        decision: &RoutingDecision,
        outcome: &TurnOutcome,
    ) -> Result<(), RouterError>;
}

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
        tokio::spawn(async move {
            store.record_outcome(&outcome).await;
        });
        Ok(())
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
            if let Some(stats) = store
                .rolling_stats(
                    &provider,
                    &TaskCategory::Feature,
                    Duration::from_secs(7 * 24 * 60 * 60),
                )
                .await
            {
                assert_eq!(stats.successes, 1);
                assert_eq!(stats.total, 1);
                return;
            }
            tokio::task::yield_now().await;
        }

        panic!("recording router did not write outcome");
    }
}
