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
/// WHY: daemon logs should be scannable; full model responses and tool results
/// flood the log when running in production.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DaemonOutputMode {
    /// Full output  -  all tool results and model responses logged verbatim.
    #[default]
    Full,
    /// Brief output  -  tool results truncated to first/last N lines, model
    /// responses logged at info level with truncation.
    Brief,
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
pub(crate) use output::truncate_output;
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
}

/// Outcome of executing a single task action.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionResult {
    /// Whether the task completed without error.
    pub success: bool,
    /// Task output or diagnostic message.
    pub output: Option<String>,
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
            output_mode: DaemonOutputMode::Full,
            daemon_behavior: DaemonBehaviorConfig::default(),
            self_prompt_limiter: crate::self_prompt::SelfPromptLimiter::new(1),
            self_prompt_config: crate::self_prompt::SelfPromptConfig::default(),
            watchdog: None,
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
            output_mode: DaemonOutputMode::Full,
            daemon_behavior: DaemonBehaviorConfig::default(),
            self_prompt_limiter: crate::self_prompt::SelfPromptLimiter::new(1),
            self_prompt_config: crate::self_prompt::SelfPromptConfig::default(),
            watchdog: None,
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
