//! The [`Router`] trait, the core abstraction shared between dispatch and
//! interactive routing paths.

use crate::types::{RequestFeatures, RoutingDecision, RouterError, TurnOutcome};

/// Re-export `BoxFuture` for use in [`Router`] implementations.
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
