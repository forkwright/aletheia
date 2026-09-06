//! Typed daemon-task admin DTOs mirroring Pylon's wire shapes
//! (`crates/pylon/src/handlers/daemon_tasks_dto.rs`, #7206). Skene has no
//! dependency on pylon or oikonomos, so these are independent structs kept
//! in sync by the contract tests in `super::tests`.

use serde::{Deserialize, Serialize};

/// One registered daemon task's persisted state, as returned by
/// `GET /api/v1/system/daemon/tasks` and each mutation route.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DaemonTask {
    /// Which `TaskRunner` this task belongs to: `"system"` (the shared
    /// maintenance scheduler) or an agent id.
    pub runner: String,
    /// Stable task identifier, e.g. `"routing-store-refresh"`.
    pub task_id: String,
    /// Human-readable name.
    pub name: String,
    /// Whether the task is currently enabled.
    pub enabled: bool,
    /// Why the task is disabled: `"auto_failure"` or `"operator"`. `None`
    /// when `enabled` is `true`.
    #[serde(default)]
    pub cause: Option<String>,
    /// Current streak of consecutive failures (resets on success).
    pub consecutive_failures: u32,
    /// Most recent error message, if the last execution failed.
    #[serde(default)]
    pub last_error: Option<String>,
    /// Terminal outcome of the last execution: `success`, `failed`, or `skipped`.
    #[serde(default)]
    pub last_outcome: Option<String>,
    /// ISO 8601 timestamp of the last execution, if any.
    #[serde(default)]
    pub last_run: Option<String>,
    /// ISO 8601 timestamp until which the task is in backoff, if any.
    #[serde(default)]
    pub backoff_until: Option<String>,
}

/// Wrapper for `GET /api/v1/system/daemon/tasks`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DaemonTaskListResponse {
    /// Every task with persisted execution history, across every attached
    /// runner.
    pub tasks: Vec<DaemonTask>,
}
