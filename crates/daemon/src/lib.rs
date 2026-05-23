#![deny(missing_docs)]
#![deny(clippy::unwrap_used)]
//! aletheia-oikonomos: per-nous background task runner
//!
//! Oikonomos (οἰκονόμος): "the steward." The quiet persistent presence that
//! keeps things running in the background. Manages scheduled tasks, periodic
//! attention checks (prosoche), and maintenance cycles for each nous.
//!
//! Supports KAIROS-style autonomous daemon mode with jitter-aware scheduling,
//! single-instance locking, and systemd notify integration. Child-agent
//! coordination and event-driven triggers are reserved API boundaries, not
//! wired runtime capabilities yet.

/// Bridge trait for daemon-to-nous communication without direct dependency coupling.
pub mod bridge;
/// Reserved child-agent coordination boundary.
pub mod coordination;
/// Periodic cron tasks: evolution, reflection, and graph cleanup.
pub mod cron;
/// Minimal jiff-native cron expression parser (replaces external `cron` + `chrono` crates).
pub mod cron_expr;
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
/// Prosoche self-audit framework: five structured attention-quality checks + audit runner.
///
/// Implements Phase 05 REQ-01 (check types) and REQ-02 (audit runner + persistence).
/// The audit runner is wired into the existing `SelfAudit` builtin heartbeat slot.
pub mod prosoche_audit;
/// Per-nous background task runner with cron scheduling, failure tracking, and graceful shutdown.
pub mod runner;
/// Scheduling primitives: cron, interval, one-shot, jitter, and active time windows.
pub mod schedule;
/// Self-prompting: daemon-initiated follow-up actions with rate limiting.
pub mod self_prompt;
/// Task-state persistence (fjall), workspace config, and single-instance locking.
pub mod state;
/// Reserved external trigger boundary.
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
        assert_send_sync::<super::state::DaemonConfig>();
        assert_send::<super::coordination::Coordinator>();
        assert_send_sync::<super::triggers::TriggerRouter>();
        assert_send_sync::<super::self_prompt::SelfPromptConfig>();
        assert_send::<super::prosoche_audit::ProsocheAuditRunner>();
    };
}
