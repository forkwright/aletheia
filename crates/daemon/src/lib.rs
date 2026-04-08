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
/// Minimal jiff-native cron expression parser (replaces external `cron` + `chrono` crates).
pub(crate) mod cron_expr;
/// Error types for task execution, scheduling, and maintenance operations.
pub mod error;
/// Task action execution: commands, builtins, prompts, and knowledge maintenance.
mod execution;
/// Instance maintenance services: trace rotation, drift detection, DB monitoring, retention.
pub mod maintenance;
/// Prometheus metric definitions for daemon task execution and watchdog monitoring.
pub mod metrics;
/// Adversarial self-probing: consistency, boundary, and recall probe evaluation.
///
/// Dokimion (δοκίμιον): "test, assay, proof." Periodic pipeline health checks
/// that detect capability drift before external QA surfaces it.
pub mod probe;
/// Prosoche (directed attention) periodic check-in for calendar, tasks, and system health.
pub mod prosoche;
/// Per-nous background task runner with cron scheduling, failure tracking, and graceful shutdown.
pub mod runner;
/// Scheduling primitives: cron, interval, one-shot, jitter, and active time windows.
pub mod schedule;
/// Self-prompting: daemon-initiated follow-up actions with rate limiting.
pub mod self_prompt;
/// SQLite-backed persistence, workspace config, and single-instance locking.
pub mod state;
/// Event-driven activation: file watchers and webhook receiver.
pub mod triggers;
/// Watchdog process monitor with heartbeat tracking and auto-recovery.
pub mod watchdog;

#[cfg(test)]
mod assertions {
    const _: fn() = || {
        fn assert_send<T: Send>() {}
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send::<super::runner::TaskRunner>();
        assert_send_sync::<super::prosoche::ProsocheCheck>();
        assert_send_sync::<super::schedule::TaskDef>();
        assert_send_sync::<super::maintenance::TraceRotator>();
        assert_send_sync::<super::maintenance::DriftDetector>();
        assert_send_sync::<super::maintenance::DbMonitor>();
        assert_send::<super::watchdog::Watchdog>();
        assert_send_sync::<super::watchdog::WatchdogConfig>();
    };
    assert_impl_all!(super::state::DaemonConfig: Send, Sync);
    assert_impl_all!(super::coordination::Coordinator: Send);
    assert_impl_all!(super::triggers::TriggerRouter: Send, Sync);
    assert_impl_all!(super::self_prompt::SelfPromptConfig: Send, Sync);
}
