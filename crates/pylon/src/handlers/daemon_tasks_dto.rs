//! Daemon-task admin endpoint request and response wire shapes. (#7206)

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// One registered daemon task's persisted state, as seen by
/// `GET /api/v1/system/daemon/tasks`.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct DaemonTaskEntry {
    /// Which `TaskRunner` this task belongs to: `"system"` (the shared
    /// maintenance scheduler) or an agent id (per-nous prosoche/self-prompt
    /// tasks).
    pub runner: String,
    /// Stable task identifier, e.g. `"routing-store-refresh"`.
    pub task_id: String,
    /// Human-readable name from the maintenance registry, when known;
    /// falls back to `task_id` for tasks outside the registry (e.g.
    /// per-agent prosoche tasks).
    pub name: String,
    /// Whether the task is currently enabled.
    pub enabled: bool,
    /// Why the task is disabled: `"auto_failure"` or `"operator"`. `None`
    /// when `enabled` is `true`.
    pub cause: Option<String>,
    /// Current streak of consecutive failures (resets on success).
    pub consecutive_failures: u32,
    /// Most recent error message, if the last execution failed.
    pub last_error: Option<String>,
    /// Terminal outcome of the last execution: `success`, `failed`, or `skipped`.
    pub last_outcome: Option<String>,
    /// ISO 8601 timestamp of the last execution, if any.
    pub last_run: Option<String>,
    /// ISO 8601 timestamp until which the task is in backoff, if any.
    pub backoff_until: Option<String>,
}

/// Response body for `GET /api/v1/system/daemon/tasks`.
#[derive(Debug, Serialize, ToSchema)]
pub struct DaemonTaskListResponse {
    /// One entry per task with persisted execution history, across every
    /// attached runner.
    pub tasks: Vec<DaemonTaskEntry>,
}

/// Request body for `POST /api/v1/system/daemon/tasks/{runner}/{task_id}/disable`.
#[derive(Debug, Default, Deserialize, ToSchema)]
pub struct DisableDaemonTaskRequest {
    /// Optional operator-facing reason, recorded as the task's `last_error`.
    #[serde(default)]
    pub reason: Option<String>,
}
