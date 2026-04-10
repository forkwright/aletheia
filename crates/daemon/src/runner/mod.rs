//! Per-nous background task runner with cron scheduling, failure tracking, and graceful shutdown.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;
use crate::bridge::DaemonBridge;
use crate::error::Result;
use crate::maintenance::{KnowledgeMaintenanceExecutor, MaintenanceConfig, RetentionExecutor};
use crate::schedule::TaskDef;
// WHY: tests use `use super::*` and reference Schedule/BuiltinTask/TaskAction directly.
#[cfg(test)]
use crate::schedule::{BuiltinTask, Schedule, TaskAction};

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
mod systemd;
mod tracking;
pub(crate) use output::truncate_output;

/// Per-nous background task runner.
pub struct TaskRunner {
    nous_id: String,
    tasks: Vec<RegisteredTask>,
    shutdown: CancellationToken,
    bridge: Option<Arc<dyn DaemonBridge>>,
    maintenance: Option<MaintenanceConfig>,
    retention_executor: Option<Arc<dyn RetentionExecutor>>,
    knowledge_executor: Option<Arc<dyn KnowledgeMaintenanceExecutor>>,
    /// In-flight tasks: `task_id` → [`InFlightTask`].
    in_flight: HashMap<String, InFlightTask>,
    /// Optional SQLite-backed state store for cross-restart persistence.
    state_store: Option<crate::state::TaskStateStore>,
    /// Output mode: full or brief (truncated).
    output_mode: DaemonOutputMode,
    /// Self-prompt rate limiter (tracks per-agent dispatch counts).
    self_prompt_limiter: crate::self_prompt::SelfPromptLimiter,
    /// Self-prompt configuration (enabled, rate limits).
    self_prompt_config: crate::self_prompt::SelfPromptConfig,
}

/// Tracks a task that is currently executing.
struct InFlightTask {
    handle: tokio::task::JoinHandle<Result<ExecutionResult>>,
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
            in_flight: HashMap::new(),
            state_store: None,
            output_mode: DaemonOutputMode::Full,
            self_prompt_limiter: crate::self_prompt::SelfPromptLimiter::new(1),
            self_prompt_config: crate::self_prompt::SelfPromptConfig::default(),
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
            in_flight: HashMap::new(),
            state_store: None,
            output_mode: DaemonOutputMode::Full,
            self_prompt_limiter: crate::self_prompt::SelfPromptLimiter::new(1),
            self_prompt_config: crate::self_prompt::SelfPromptConfig::default(),
        }
    }

    /// Attach maintenance configuration.
    #[must_use]
    pub fn with_maintenance(mut self, config: MaintenanceConfig) -> Self {
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

    /// Attach a `SQLite` state store for task execution persistence.
    ///
    /// State is loaded on the first call to [`Self::run`] (before catch-up),
    /// and saved after every task completion or failure.
    #[must_use]
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "called by runner_tests::with_state_store_persists_across_restarts; production wiring lives in the binary crate"
        )
    )]
    pub(crate) fn with_state_store(mut self, store: crate::state::TaskStateStore) -> Self {
        self.state_store = Some(store);
        self
    }

    /// Set the output mode (full or brief).
    #[must_use]
    pub fn with_output_mode(mut self, mode: DaemonOutputMode) -> Self {
        self.output_mode = mode;
        self
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

}

#[cfg(test)]
#[path = "../runner_tests.rs"]
mod tests;
