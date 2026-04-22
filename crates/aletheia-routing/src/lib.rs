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
