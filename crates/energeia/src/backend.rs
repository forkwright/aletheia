//! Dispatch backend trait: high-level abstraction over dispatch orchestration.
//!
//! [`DispatchBackend`] is the boundary between a control plane (kanon, KAIROS,
//! or an interactive operator) and a dispatch execution engine. Implementations
//! vary in how they execute prompts, manage CI, and persist state, but all
//! expose the same five-operation interface.
//!
//! WHY: kanon currently has two dispatch codepaths — phronesis (legacy) and
//! energeia (new). This trait lets kanon switch between backends via config or
//! feature flag without changing calling code. It also enables aletheia's
//! KAIROS daemon to dispatch through the same interface.

use std::future::Future;
use std::pin::Pin;

use crate::error::Result;
use crate::metrics::cost::CostReport;
use crate::metrics::health::HealthReport;
use crate::metrics::status::StatusDashboard;
use crate::prompt::PromptSpec;
use crate::steward::StewardResult;
use crate::types::DispatchResult;
use crate::types::DispatchSpec;

// ---------------------------------------------------------------------------
// DispatchBackend trait
// ---------------------------------------------------------------------------

/// High-level dispatch orchestration backend.
///
/// Abstracts the full dispatch workflow: execute prompts, manage PRs via
/// steward, query status, check health, and generate reports. Control planes
/// (kanon CLI, KAIROS daemon) depend on this trait, not on concrete
/// implementations.
///
/// # Implementations
///
/// - [`EnergeiaBackend`] — uses energeia's [`Orchestrator`](crate::orchestrator::Orchestrator),
///   steward service, and fjall-backed metrics. This is the production backend
///   in aletheia. Requires the `storage-fjall` feature.
/// - `PhronesisBackend` (in kanon) — wraps kanon's phronesis dispatch engine.
///   Exists for backwards compatibility during the migration period.
pub trait DispatchBackend: Send + Sync {
    /// Execute a batch of prompts according to their dependency DAG.
    ///
    /// Loads prompts by number, builds the dependency graph, executes groups
    /// in topological order with bounded concurrency, and runs QA gates on
    /// completed sessions.
    fn dispatch<'a>(
        &'a self,
        spec: &'a DispatchSpec,
        prompts: &'a [PromptSpec],
    ) -> Pin<Box<dyn Future<Output = Result<DispatchResult>> + Send + 'a>>;

    /// Run a single steward pass: classify PRs, merge green, fix red.
    ///
    /// Returns the steward result summarizing actions taken on open PRs.
    fn steward_pass<'a>(
        &'a self,
        project: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<StewardResult>> + Send + 'a>>;

    /// Query current dispatch state: active sessions, queue depth, recent outcomes.
    fn status<'a>(&'a self) -> Pin<Box<dyn Future<Output = Result<StatusDashboard>> + Send + 'a>>;

    /// Pipeline health metrics: session success rate, cost trends, latency.
    fn health<'a>(
        &'a self,
        window_days: u32,
    ) -> Pin<Box<dyn Future<Output = Result<HealthReport>> + Send + 'a>>;

    /// Cost and velocity report for the given number of days.
    fn report<'a>(
        &'a self,
        days: u32,
    ) -> Pin<Box<dyn Future<Output = Result<CostReport>> + Send + 'a>>;
}

// Trait implementations and EnergeiaBackend are in a separate module
// to avoid trait-impl colocation.
mod backend_impl;

#[cfg(feature = "storage-fjall")]
pub use backend_impl::EnergeiaBackend;

#[cfg(test)]
mod tests {
    use super::*;

    // WHY: compile-time check that DispatchBackend is object-safe.
    // This ensures it can be used as `dyn DispatchBackend` for runtime
    // backend selection via config/feature-flag.
    const _: Option<&dyn DispatchBackend> = None;
}
