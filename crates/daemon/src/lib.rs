#![deny(missing_docs)]
//! aletheia-oikonomos: per-nous background task runner
//!
//! Oikonomos (οἰκονόμος): "the steward." The quiet persistent presence that
//! keeps things running in the background. Manages scheduled tasks, periodic
//! attention checks (prosoche), and maintenance cycles for each nous.

/// Bridge trait for daemon-to-nous communication without direct dependency coupling.
pub mod bridge;
/// Error types for task execution, scheduling, and maintenance operations.
pub mod error;
/// Task action execution: commands, builtins, prompts, and knowledge maintenance.
mod execution;
/// Instance maintenance services: trace rotation, drift detection, DB monitoring, retention.
pub mod maintenance;
/// Prosoche (directed attention) periodic check-in for calendar, tasks, and system health.
pub mod prosoche;
/// Per-nous background task runner with cron scheduling, failure tracking, and graceful shutdown.
pub mod runner;
/// Scheduling primitives: cron, interval, one-shot, and active time windows.
pub mod schedule;
/// SQLite-backed persistence for daemon task execution state.
pub mod state;

#[cfg(test)]
mod assertions {
    use static_assertions::assert_impl_all;

    assert_impl_all!(super::runner::TaskRunner: Send);
    assert_impl_all!(super::prosoche::ProsocheCheck: Send, Sync);
    assert_impl_all!(super::schedule::TaskDef: Send, Sync);
    assert_impl_all!(super::maintenance::TraceRotator: Send, Sync);
    assert_impl_all!(super::maintenance::DriftDetector: Send, Sync);
    assert_impl_all!(super::maintenance::DbMonitor: Send, Sync);
}
