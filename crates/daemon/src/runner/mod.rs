//! Per-nous background task runner with cron scheduling, failure tracking, and graceful shutdown.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use taxis::config::DaemonBehaviorConfig;
use tokio_util::sync::CancellationToken;

use crate::bridge::DaemonBridge;
use crate::error::Result;
use crate::maintenance::{KnowledgeMaintenanceExecutor, MaintenanceConfig, RetentionExecutor};
use crate::schedule::{Schedule, TaskAction, TaskDef};
use crate::watchdog::{ProcessStatus, WatchdogConfig};
// WHY: tests use `use super::*` and reference BuiltinTask directly.
#[cfg(test)]
use crate::schedule::BuiltinTask;

/// Output mode for daemon logging.
///
/// WHY(#4948): daemon command output can contain secrets, prompts, session
/// text, or private paths. The default is metadata-only; full output requires
/// explicit opt-in and still passes through best-effort redaction.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DaemonOutputMode {
    /// Metadata only: status when available, byte counts, line counts, and
    /// stable digests. No output excerpt is included.
    #[default]
    Summary,
    /// Summary metadata plus a small redacted excerpt.
    Brief,
    /// Full redacted output. Unsafe/private: use only for controlled
    /// diagnostics, because redaction cannot prove every sensitive value.
    Full,
}

mod inflight;
mod lifecycle;
mod output;
mod persistence;
mod registration;
mod supervision;
/// Systemd notify integration for daemon lifecycle signaling.
pub mod systemd;
mod tracking;
#[cfg(test)]
pub(crate) use output::truncate_output;
pub(crate) use output::{
    command_context, process_output_report, redact_task_text, safe_output_for_mode,
};
use supervision::TaskWatchdog;

// kanon:ignore RUST/struct-too-many-fields — TaskRunner is a cohesive actor struct: all fields are required for per-nous task scheduling, execution, and lifecycle management
/// Per-nous background task runner.
pub struct TaskRunner {
    nous_id: String,
    tasks: Vec<RegisteredTask>,
    shutdown: CancellationToken,
    bridge: Option<Arc<dyn DaemonBridge>>,
    maintenance: Option<MaintenanceConfig>,
    retention_executor: Option<Arc<dyn RetentionExecutor>>,
    knowledge_executor: Option<Arc<dyn KnowledgeMaintenanceExecutor>>,
    #[cfg(feature = "knowledge-store")]
    knowledge_store: Option<Arc<episteme::knowledge_store::KnowledgeStore>>,
    /// In-flight tasks: `task_id` → [`InFlightTask`].
    in_flight: HashMap<String, InFlightTask>,
    /// Optional fjall-backed state store for cross-restart persistence.
    state_store: Option<crate::state::TaskStateStore>,
    /// Output mode: full or brief (truncated).
    output_mode: DaemonOutputMode,
    /// Deployment-tunable daemon behavior.
    daemon_behavior: DaemonBehaviorConfig,
    /// Self-prompt rate limiter (tracks per-agent dispatch counts).
    self_prompt_limiter: crate::self_prompt::SelfPromptLimiter,
    /// Self-prompt configuration (enabled, rate limits).
    self_prompt_config: crate::self_prompt::SelfPromptConfig,
    /// Optional per-task watchdog supervisor.
    watchdog: Option<TaskWatchdog>,
    /// Tracked self-prompt tasks so panics surface as `JoinError`s instead of
    /// disappearing silently when the returned handle is dropped.
    self_prompt_tasks: tokio::task::JoinSet<()>,
}

/// Tracks a task that is currently executing.
struct InFlightTask {
    handle: tokio::task::JoinHandle<Result<ExecutionResult>>,
    /// Daemon-owned cancellation token for this task. Cancelling it propagates
    /// to the child token passed into the task future, which the bridge forwards
    /// into the actor turn.
    cancel: CancellationToken,
    started_at: Instant,
    timeout: Duration,
    warned: bool,
}

struct RegisteredTask {
    def: TaskDef,
    next_run: Option<jiff::Timestamp>,
    last_run: Option<jiff::Timestamp>,
    run_count: u64,
    consecutive_failures: u32,
    /// If SET, the task is in backoff and should not run before this instant.
    backoff_until: Option<Instant>,
    /// Most recent error message, if the last execution failed. (#2212)
    last_error: Option<String>,
    /// Number of non-fatal errors reported by the last execution.
    last_errors: u32,
}

/// Terminal outcome classification for a single task action.
///
/// WHY(#5129): a bare `success: bool` conflates "ran and succeeded" with
/// "could not run because a dependency was not configured". A soft-skip must
/// not be recorded as a success (it inflates run counts and masks
/// misconfiguration) nor as a failure (it would trip the backoff/auto-disable
/// machinery for a benign no-op).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TaskOutcome {
    /// The task ran and completed without error.
    Success,
    /// The task ran and failed.
    Failed,
    /// The task did not run because a required dependency was absent.
    Skipped,
}

/// Outcome of executing a single task action.
///
/// WHY(#5129): no `Default` derive — a task outcome must be an explicit
/// `Success`/`Failed`/`Skipped` classification (constructed via the
/// `success`/`failed`/`skipped` constructors), never a silent default that
/// would mask misclassification.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionResult {
    /// Terminal outcome classification for the task.
    pub outcome: TaskOutcome,
    /// Task output or diagnostic message.
    pub output: Option<String>,
    /// Number of non-fatal errors encountered by the task implementation.
    /// Used by maintenance tasks that report partial success (e.g. knowledge
    /// graph decay refresh with per-fact persistence failures).
    #[serde(default)]
    pub errors: u32,
}

impl ExecutionResult {
    /// Build an execution result from a knowledge-maintenance report.
    ///
    /// Maps [`MaintenanceOutcome::Success`] to [`TaskOutcome::Success`]; degraded
    /// and failure outcomes become [`TaskOutcome::Failed`] per existing task policy,
    /// while the non-fatal error count is preserved for status and metrics.
    pub fn from_maintenance_report(
        report: &crate::maintenance::MaintenanceReport,
        output: String,
    ) -> Self {
        use crate::maintenance::MaintenanceOutcome;
        Self {
            outcome: if report.outcome() == MaintenanceOutcome::Success {
                TaskOutcome::Success
            } else {
                TaskOutcome::Failed
            },
            output: Some(output),
            errors: report.errors,
        }
    }
}

impl ExecutionResult {
    /// Construct a successful result.
    #[must_use]
    pub fn success(output: Option<String>) -> Self {
        Self {
            outcome: TaskOutcome::Success,
            output,
            errors: 0,
        }
    }

    /// Construct a failed result.
    #[must_use]
    pub fn failed(output: Option<String>) -> Self {
        Self {
            outcome: TaskOutcome::Failed,
            output,
            errors: 0,
        }
    }

    /// Construct a soft-skip result (dependency not configured).
    #[must_use]
    pub fn skipped(output: Option<String>) -> Self {
        Self {
            outcome: TaskOutcome::Skipped,
            output,
            errors: 0,
        }
    }

    /// Whether the task ran and completed successfully.
    #[must_use]
    pub fn is_success(&self) -> bool {
        self.outcome == TaskOutcome::Success
    }
}

impl TaskRunner {
    /// Create a runner for the given nous, listening for shutdown on the cancellation token.
    pub fn new(nous_id: impl Into<String>, shutdown: CancellationToken) -> Self {
        Self {
            nous_id: nous_id.into(),
            tasks: Vec::new(),
            shutdown,
            bridge: None,
            maintenance: None,
            retention_executor: None,
            knowledge_executor: None,
            #[cfg(feature = "knowledge-store")]
            knowledge_store: None,
            in_flight: HashMap::new(),
            state_store: None,
            output_mode: DaemonOutputMode::Summary,
            daemon_behavior: DaemonBehaviorConfig::default(),
            self_prompt_limiter: crate::self_prompt::SelfPromptLimiter::new(1),
            self_prompt_config: crate::self_prompt::SelfPromptConfig::default(),
            watchdog: None,
            self_prompt_tasks: tokio::task::JoinSet::new(),
        }
    }

    /// Create a runner with a bridge for nous communication.
    pub fn with_bridge(
        nous_id: impl Into<String>,
        shutdown: CancellationToken,
        bridge: Arc<dyn DaemonBridge>,
    ) -> Self {
        Self {
            nous_id: nous_id.into(),
            tasks: Vec::new(),
            shutdown,
            bridge: Some(bridge),
            maintenance: None,
            retention_executor: None,
            knowledge_executor: None,
            #[cfg(feature = "knowledge-store")]
            knowledge_store: None,
            in_flight: HashMap::new(),
            state_store: None,
            output_mode: DaemonOutputMode::Summary,
            daemon_behavior: DaemonBehaviorConfig::default(),
            self_prompt_limiter: crate::self_prompt::SelfPromptLimiter::new(1),
            self_prompt_config: crate::self_prompt::SelfPromptConfig::default(),
            watchdog: None,
            self_prompt_tasks: tokio::task::JoinSet::new(),
        }
    }

    /// Attach maintenance configuration.
    #[must_use]
    pub fn with_maintenance(mut self, mut config: MaintenanceConfig) -> Self {
        if config.after_action_store.is_none()
            && let Some(store) = self
                .maintenance
                .as_ref()
                .and_then(|maintenance| maintenance.after_action_store.clone())
        {
            config.after_action_store = Some(store);
        }
        self.maintenance = Some(config);
        self
    }

    /// Attach a retention executor for data lifecycle management.
    #[must_use]
    pub fn with_retention(
        mut self,
        executor: Arc<dyn crate::maintenance::RetentionExecutor>,
    ) -> Self {
        self.retention_executor = Some(executor);
        self
    }

    /// Attach a knowledge maintenance executor for graph operations.
    #[must_use]
    pub fn with_knowledge_maintenance(
        mut self,
        executor: Arc<dyn KnowledgeMaintenanceExecutor>,
    ) -> Self {
        self.knowledge_executor = Some(executor);
        self
    }

    /// Attach an optional knowledge maintenance executor for graph operations.
    ///
    /// Convenience helper for callers (e.g. the CLI) that may or may not have
    /// a knowledge store available.
    #[must_use]
    pub fn with_knowledge_maintenance_opt(
        mut self,
        executor: Option<Arc<dyn KnowledgeMaintenanceExecutor>>,
    ) -> Self {
        self.knowledge_executor = executor;
        self
    }

    /// Attach a knowledge store for Prosoche memory consistency checks.
    #[cfg(feature = "knowledge-store")]
    #[must_use]
    pub fn with_knowledge_store(
        mut self,
        store: Arc<episteme::knowledge_store::KnowledgeStore>,
    ) -> Self {
        self.knowledge_store = Some(store);
        self
    }

    /// Attach the empirical routing after-action store for periodic refresh.
    #[must_use]
    pub fn with_after_action_store(
        mut self,
        store: Arc<aletheia_routing::AfterActionStore>,
    ) -> Self {
        let mut maintenance = self.maintenance.take().unwrap_or_default();
        maintenance.after_action_store = Some(store);
        self.maintenance = Some(maintenance);
        self
    }

    /// Attach a fjall state store for task execution persistence.
    ///
    /// State is loaded on the first call to [`Self::run`] (before catch-up),
    /// and saved after every task completion or failure.
    #[must_use]
    pub fn with_state_store(mut self, store: crate::state::TaskStateStore) -> Self {
        self.state_store = Some(store);
        self
    }

    /// Set the output mode (full or brief).
    #[must_use]
    pub fn with_output_mode(mut self, mode: DaemonOutputMode) -> Self {
        self.output_mode = mode;
        self
    }

    /// Apply deployment-tunable daemon behavior.
    #[must_use]
    pub fn with_daemon_behavior(mut self, behavior: DaemonBehaviorConfig) -> Self {
        self.daemon_behavior = behavior;
        self
    }

    /// Configure the per-task watchdog from deployment settings.
    #[must_use]
    pub fn with_watchdog_settings(mut self, settings: &taxis::config::WatchdogSettings) -> Self {
        if settings.enabled {
            let config =
                WatchdogConfig::from_settings(settings).with_daemon_behavior(&self.daemon_behavior);
            self.watchdog = Some(TaskWatchdog::new(config, self.shutdown.child_token()));
        }
        self
    }

    /// Return current watchdog process statuses.
    #[must_use]
    pub fn watchdog_status(&self) -> Vec<ProcessStatus> {
        self.watchdog
            .as_ref()
            .map(TaskWatchdog::status)
            .unwrap_or_default()
    }

    /// Return the number of watchdog restart events recorded by this runner.
    #[must_use]
    pub fn watchdog_restart_count(&self) -> usize {
        self.watchdog
            .as_ref()
            .map(|watchdog| watchdog.restart_log().len())
            .unwrap_or_default()
    }

    /// Whether a daemon bridge is attached.
    #[must_use]
    pub fn has_bridge(&self) -> bool {
        self.bridge.is_some()
    }

    /// Whether a retention executor is attached.
    #[must_use]
    pub fn has_retention_executor(&self) -> bool {
        self.retention_executor.is_some()
    }

    /// Whether a knowledge maintenance executor is attached.
    #[must_use]
    pub fn has_knowledge_executor(&self) -> bool {
        self.knowledge_executor.is_some()
    }

    /// Configure self-prompting behavior (rate-limited daemon-initiated follow-ups).
    ///
    /// WHY: self-prompting enables proactive work when prosoche checks identify
    /// items needing attention. Must be explicitly enabled with rate limits.
    #[must_use]
    pub fn with_self_prompt(mut self, config: crate::self_prompt::SelfPromptConfig) -> Self {
        self.self_prompt_limiter = crate::self_prompt::SelfPromptLimiter::new(config.max_per_hour);
        self.self_prompt_config = config;
        self
    }

    /// Register the top open issue as a recurring self-prompt task.
    ///
    /// Returns the registered task id when an open issue was available.
    pub fn register_top_issue_self_prompt(
        &mut self,
        issues: &[crate::self_prompt::OpenIssue],
        schedule: Schedule,
    ) -> Option<String> {
        let prompt_task = crate::self_prompt::prompt_task_from_top_open_issue(issues)?;
        let task_id = prompt_task.id.clone();
        self.register(TaskDef {
            id: prompt_task.id,
            name: prompt_task.name,
            nous_id: self.nous_id.clone(),
            schedule,
            action: TaskAction::SelfPrompt(prompt_task.prompt),
            enabled: true,
            catch_up: false,
            ..TaskDef::default()
        });
        Some(task_id)
    }
}

#[cfg(test)]
#[path = "../runner_tests/mod.rs"]
mod tests;
