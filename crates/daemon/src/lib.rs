#![deny(missing_docs)]
//! aletheia-oikonomos: per-nous background task runner
//!
//! Oikonomos (οἰκονόμος): "the steward." The quiet persistent presence that
//! keeps things running in the background. Manages scheduled tasks, periodic
//! attention checks (prosoche), and maintenance cycles for each nous.
//!
//! Supports KAIROS-style autonomous daemon mode with jitter-aware scheduling,
//! single-instance locking, child agent coordination, event-driven triggers,
//! and systemd notify integration.

/// Bridge trait for daemon-to-nous communication without direct dependency coupling.
pub mod bridge;
/// Team coordination: child agent spawning with concurrency limits.
pub mod coordination;
/// Periodic cron tasks: evolution, reflection, and graph cleanup.
pub mod cron;
/// Error types for task execution, scheduling, and maintenance operations.
pub mod error;
/// Task action execution: commands, builtins, prompts, and knowledge maintenance.
mod execution;
/// Instance maintenance services: trace rotation, drift detection, DB monitoring, retention.
pub mod maintenance;
/// Prometheus metric definitions for daemon task execution and watchdog monitoring.
pub mod metrics;
/// Prosoche (directed attention) periodic check-in for calendar, tasks, and system health.
pub mod prosoche;
/// Per-nous background task runner with cron scheduling, failure tracking, and graceful shutdown.
pub mod runner;
/// Scheduling primitives: cron, interval, one-shot, jitter, and active time windows.
pub mod schedule;
/// SQLite-backed persistence, workspace config, and single-instance locking.
pub mod state;
/// Event-driven activation: file watchers and webhook receiver.
pub mod triggers;
/// Watchdog process monitor with heartbeat tracking and auto-recovery.
pub mod watchdog;

#[cfg(test)]
mod assertions {
    use static_assertions::assert_impl_all;

    assert_impl_all!(super::runner::TaskRunner: Send);
    assert_impl_all!(super::prosoche::ProsocheCheck: Send, Sync);
    assert_impl_all!(super::schedule::TaskDef: Send, Sync);
    assert_impl_all!(super::maintenance::TraceRotator: Send, Sync);
    assert_impl_all!(super::maintenance::DriftDetector: Send, Sync);
    assert_impl_all!(super::maintenance::DbMonitor: Send, Sync);
    assert_impl_all!(super::watchdog::Watchdog: Send);
    assert_impl_all!(super::watchdog::WatchdogConfig: Send, Sync);
    assert_impl_all!(super::state::DaemonConfig: Send, Sync);
    assert_impl_all!(super::coordination::Coordinator: Send);
    assert_impl_all!(super::triggers::TriggerRouter: Send, Sync);
}
