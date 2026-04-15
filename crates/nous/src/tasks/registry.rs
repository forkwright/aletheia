//! Concurrent task registry with progress streaming and cooperative cancellation.

use std::collections::HashMap;
use std::sync::{Arc, PoisonError, RwLock};
use std::time::Duration;

use snafu::Snafu;
use tracing::debug;

use super::output;
use super::types::{ProgressEvent, TaskEntry, TaskId, TaskStatus, TaskType, ToolCallSummary};

/// Errors from task registry operations.
#[derive(Debug, Snafu)]
#[snafu(visibility(pub(crate)))]
#[expect(
    missing_docs,
    reason = "snafu error variant fields (task_id, from, to, source, location) are self-documenting via display format"
)]
#[non_exhaustive]
pub enum RegistryError {
    /// Task not found in the registry.
    #[snafu(display("task {task_id} not found"))]
    NotFound {
        task_id: TaskId,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Invalid status transition attempted.
    #[snafu(display("invalid transition for task {task_id}: {from} -> {to}"))]
    InvalidTransition {
        task_id: TaskId,
        from: TaskStatus,
        to: TaskStatus,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Registry lock poisoned.
    #[snafu(display("registry lock poisoned"))]
    LockPoisoned {
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Output file operation failed.
    #[snafu(display("output error: {source}"))]
    Output {
        source: output::OutputError,
        #[snafu(implicit)]
        location: snafu::Location,
    },
}

/// WHY: `PoisonError` carries the guard type which we don't need in the error.
/// Using a named binding satisfies `clippy::map_err_ignore`.
fn lock_poisoned<T>(_poison: PoisonError<T>) -> RegistryError {
    LockPoisonedSnafu.build()
}

/// Snapshot of a task for external consumption.
///
/// WHY: `TaskEntry` contains non-cloneable fields (`broadcast::Sender`,
/// `CancellationToken`). This snapshot carries only the displayable state.
#[derive(Debug, Clone)]
pub struct TaskSnapshot {
    /// Task identifier.
    pub id: TaskId,
    /// Task type with metadata.
    pub task_type: TaskType,
    /// Current lifecycle status.
    pub status: TaskStatus,
    /// Human-readable description.
    pub description: String,
    /// When the task was registered.
    pub created_at: jiff::Timestamp,
    /// When the task reached a terminal status.
    pub completed_at: Option<jiff::Timestamp>,
    /// Recent tool call summaries (up to 5).
    pub recent_activity: Vec<ToolCallSummary>,
    /// Last error message, if any.
    pub error_snapshot: Option<String>,
}

impl From<&TaskEntry> for TaskSnapshot {
    fn from(entry: &TaskEntry) -> Self {
        Self {
            id: entry.id,
            task_type: entry.task_type.clone(),
            status: entry.status,
            description: entry.description.clone(),
            created_at: entry.created_at,
            completed_at: entry.completed_at,
            recent_activity: entry.recent_activity.iter().cloned().collect(),
            error_snapshot: entry.error_snapshot.clone(),
        }
    }
}

/// Concurrent task registry.
///
/// Thread-safe via `Arc<RwLock<...>>`. Uses `std::sync::RwLock` because no lock
/// is held across `.await` boundaries — all mutations are synchronous under the
/// lock, with async work (output file I/O, cancellation) happening after release.
#[derive(Clone)]
pub struct TaskRegistry {
    tasks: Arc<RwLock<HashMap<TaskId, TaskEntry>>>,
    /// How long after completion before GC evicts a task.
    gc_deadline: Duration,
}

impl TaskRegistry {
    /// Create a new task registry with the given GC deadline.
    pub fn new(gc_deadline: Duration) -> Self {
        Self {
            tasks: Arc::new(RwLock::new(HashMap::new())),
            gc_deadline,
        }
    }

    /// Create a registry with the default 30-minute GC deadline.
    pub fn with_default_deadline() -> Self {
        Self::new(Duration::from_mins(30))
    }

    /// The configured GC deadline.
    pub fn gc_deadline(&self) -> Duration {
        self.gc_deadline
    }

    /// Register a new task and return its ID and cancellation token.
    ///
    /// The task starts in [`TaskStatus::Pending`].
    ///
    /// # Errors
    ///
    /// Returns an error if the registry lock is poisoned.
    pub fn register(
        &self,
        task_type: TaskType,
        description: String,
    ) -> Result<(TaskId, tokio_util::sync::CancellationToken), RegistryError> {
        let entry = TaskEntry::new(task_type, description);
        let id = entry.id;
        let token = entry.cancellation_token.clone();

        debug!(%id, "registering task");

        let mut tasks = self.tasks.write().map_err(lock_poisoned)?;
        tasks.insert(id, entry);

        Ok((id, token))
    }

    /// Update a task's status.
    ///
    /// Returns an error if the transition is invalid (e.g. terminal -> running)
    /// or the task doesn't exist.
    ///
    /// # Errors
    ///
    /// Returns an error if the task is not found, if the status transition
    /// is invalid, or if the registry lock is poisoned.
    pub fn update_status(
        &self,
        task_id: TaskId,
        new_status: TaskStatus,
    ) -> Result<(), RegistryError> {
        let mut tasks = self.tasks.write().map_err(lock_poisoned)?;

        let entry = tasks
            .get_mut(&task_id)
            .ok_or_else(|| NotFoundSnafu { task_id }.build())?;

        let old_status = entry.status;

        // WHY: Terminal states are final -- no further transitions allowed.
        if old_status.is_terminal() {
            return InvalidTransitionSnafu {
                task_id,
                from: old_status,
                to: new_status,
            }
            .fail();
        }

        // WHY: Only valid forward transitions are allowed.
        let valid = matches!(
            (old_status, new_status),
            (
                TaskStatus::Pending,
                TaskStatus::Running | TaskStatus::Failed | TaskStatus::Killed
            ) | (
                TaskStatus::Running,
                TaskStatus::Completed | TaskStatus::Failed | TaskStatus::Killed
            )
        );

        if !valid {
            return InvalidTransitionSnafu {
                task_id,
                from: old_status,
                to: new_status,
            }
            .fail();
        }

        entry.status = new_status;

        if new_status.is_terminal() {
            entry.completed_at = Some(jiff::Timestamp::now());
        }

        // NOTE: broadcast send failure is benign -- means no active subscribers.
        let _ = entry.progress_tx.send(ProgressEvent::StatusChanged {
            from: old_status,
            to: new_status,
        });

        debug!(%task_id, %old_status, %new_status, "task status updated");

        Ok(())
    }

    /// Record a tool call for a task's rolling activity window.
    ///
    /// # Errors
    ///
    /// Returns an error if the task is not found or if the registry lock is poisoned.
    pub fn record_tool_call(
        &self,
        task_id: TaskId,
        summary: ToolCallSummary,
    ) -> Result<(), RegistryError> {
        let mut tasks = self.tasks.write().map_err(lock_poisoned)?;

        let entry = tasks
            .get_mut(&task_id)
            .ok_or_else(|| NotFoundSnafu { task_id }.build())?;

        // NOTE: broadcast send failure is benign -- means no active subscribers.
        let _ = entry
            .progress_tx
            .send(ProgressEvent::ToolActivity(summary.clone()));

        entry.record_tool_call(summary);

        Ok(())
    }

    /// Record an error snapshot for a task.
    ///
    /// # Errors
    ///
    /// Returns an error if the task is not found or if the registry lock is poisoned.
    pub fn record_error(&self, task_id: TaskId, error: String) -> Result<(), RegistryError> {
        let mut tasks = self.tasks.write().map_err(lock_poisoned)?;

        let entry = tasks
            .get_mut(&task_id)
            .ok_or_else(|| NotFoundSnafu { task_id }.build())?;

        let _ = entry.progress_tx.send(ProgressEvent::Error(error.clone()));

        entry.error_snapshot = Some(error);

        Ok(())
    }

    /// Set the output file path for a task.
    ///
    /// # Errors
    ///
    /// Returns an error if the task is not found or if the registry lock is poisoned.
    pub fn set_output_path(
        &self,
        task_id: TaskId,
        path: std::path::PathBuf,
    ) -> Result<(), RegistryError> {
        let mut tasks = self.tasks.write().map_err(lock_poisoned)?;

        let entry = tasks
            .get_mut(&task_id)
            .ok_or_else(|| NotFoundSnafu { task_id }.build())?;

        entry.output_path = Some(path);
        Ok(())
    }

    /// Broadcast an output chunk for a task.
    ///
    /// WHY: This only sends the progress event. The actual disk write is done
    /// by the `OutputWriter` that the task owns -- keeping I/O outside the lock.
    ///
    /// # Errors
    ///
    /// Returns an error if the task is not found or if the registry lock is poisoned.
    pub fn broadcast_output_chunk(
        &self,
        task_id: TaskId,
        data: Vec<u8>,
    ) -> Result<(), RegistryError> {
        let tasks = self.tasks.read().map_err(lock_poisoned)?;

        let entry = tasks
            .get(&task_id)
            .ok_or_else(|| NotFoundSnafu { task_id }.build())?;

        let _ = entry.progress_tx.send(ProgressEvent::OutputChunk(data));
        Ok(())
    }

    /// Get a snapshot of a task.
    ///
    /// # Errors
    ///
    /// Returns an error if the task is not found or if the registry lock is poisoned.
    pub fn get(&self, task_id: TaskId) -> Result<TaskSnapshot, RegistryError> {
        let tasks = self.tasks.read().map_err(lock_poisoned)?;

        let entry = tasks
            .get(&task_id)
            .ok_or_else(|| NotFoundSnafu { task_id }.build())?;

        Ok(TaskSnapshot::from(entry))
    }

    /// List snapshots of all tasks, optionally filtered by status.
    ///
    /// # Complexity
    ///
    /// O(n) where n is the number of tasks in the registry.
    ///
    /// # Errors
    ///
    /// Returns an error if the registry lock is poisoned.
    pub fn list(
        &self,
        status_filter: Option<TaskStatus>,
    ) -> Result<Vec<TaskSnapshot>, RegistryError> {
        let tasks = self.tasks.read().map_err(lock_poisoned)?;

        let snapshots = tasks
            .values()
            .filter(|e| status_filter.is_none_or(|s| e.status == s))
            .map(TaskSnapshot::from)
            .collect();

        Ok(snapshots)
    }

    /// Subscribe to progress events for a task.
    ///
    /// WHY: Returns a `broadcast::Receiver` so the subscriber sees all future
    /// events. Past events are not replayed -- subscribers joining late only
    /// see events from their subscription point forward.
    ///
    /// # Errors
    ///
    /// Returns an error if the task is not found or if the registry lock is poisoned.
    pub fn subscribe(
        &self,
        task_id: TaskId,
    ) -> Result<tokio::sync::broadcast::Receiver<ProgressEvent>, RegistryError> {
        let tasks = self.tasks.read().map_err(lock_poisoned)?;

        let entry = tasks
            .get(&task_id)
            .ok_or_else(|| NotFoundSnafu { task_id }.build())?;

        Ok(entry.progress_tx.subscribe())
    }

    /// Kill a task by triggering its cancellation token.
    ///
    /// Sets the task status to `Killed` and cancels the token so the task's
    /// execution loop can observe the cancellation at yield points.
    ///
    /// # Errors
    ///
    /// Returns an error if the task is not found or if the registry lock is poisoned.
    pub fn kill(&self, task_id: TaskId) -> Result<(), RegistryError> {
        let mut tasks = self.tasks.write().map_err(lock_poisoned)?;

        let entry = tasks
            .get_mut(&task_id)
            .ok_or_else(|| NotFoundSnafu { task_id }.build())?;

        if entry.status.is_terminal() {
            debug!(%task_id, status = %entry.status, "kill called on terminal task, ignoring");
            return Ok(());
        }

        let old_status = entry.status;
        entry.status = TaskStatus::Killed;
        entry.completed_at = Some(jiff::Timestamp::now());
        entry.cancellation_token.cancel();

        let _ = entry.progress_tx.send(ProgressEvent::StatusChanged {
            from: old_status,
            to: TaskStatus::Killed,
        });

        debug!(%task_id, "task killed");

        Ok(())
    }

    /// Run a GC sweep, evicting terminal tasks past the deadline.
    ///
    /// Returns the IDs and output paths of evicted tasks so the caller can
    /// clean up output files outside the lock.
    ///
    /// # Complexity
    ///
    /// O(n) where n is the number of tasks in the registry.
    pub(crate) fn gc_sweep(
        &self,
    ) -> Result<Vec<(TaskId, Option<std::path::PathBuf>)>, RegistryError> {
        let now = jiff::Timestamp::now();
        let deadline = jiff::SignedDuration::try_from(self.gc_deadline)
            .unwrap_or_else(|_| jiff::SignedDuration::from_secs(30 * 60));

        let mut tasks = self.tasks.write().map_err(lock_poisoned)?;

        let mut evicted = Vec::new();

        tasks.retain(|id, entry| {
            if let Some(completed_at) = entry.completed_at {
                let elapsed = now.duration_since(completed_at);
                if elapsed >= deadline {
                    debug!(%id, "GC evicting stale task");
                    evicted.push((*id, entry.output_path.clone()));
                    return false;
                }
            }
            true
        });

        Ok(evicted)
    }

    /// Number of tasks currently in the registry.
    ///
    /// # Errors
    ///
    /// Returns an error if the registry lock is poisoned.
    pub fn len(&self) -> Result<usize, RegistryError> {
        let tasks = self.tasks.read().map_err(lock_poisoned)?;
        Ok(tasks.len())
    }

    /// Whether the registry is empty.
    ///
    /// # Errors
    ///
    /// Returns an error if the registry lock is poisoned.
    pub fn is_empty(&self) -> Result<bool, RegistryError> {
        Ok(self.len()? == 0)
    }
}

impl std::fmt::Debug for TaskRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let count = self.tasks.read().map_or(0, |t| t.len());
        f.debug_struct("TaskRegistry")
            .field("task_count", &count)
            .field("gc_deadline", &self.gc_deadline)
            .finish()
    }
}

#[cfg(test)]
#[path = "registry_tests.rs"]
mod registry_tests;
