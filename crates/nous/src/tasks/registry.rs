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
        Self::new(Duration::from_secs(30 * 60))
    }

    /// The configured GC deadline.
    pub fn gc_deadline(&self) -> Duration {
        self.gc_deadline
    }

    /// Register a new task and return its ID and cancellation token.
    ///
    /// The task starts in [`TaskStatus::Pending`].
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
    pub fn get(&self, task_id: TaskId) -> Result<TaskSnapshot, RegistryError> {
        let tasks = self.tasks.read().map_err(lock_poisoned)?;

        let entry = tasks
            .get(&task_id)
            .ok_or_else(|| NotFoundSnafu { task_id }.build())?;

        Ok(TaskSnapshot::from(entry))
    }

    /// List snapshots of all tasks, optionally filtered by status.
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
    pub fn len(&self) -> Result<usize, RegistryError> {
        let tasks = self.tasks.read().map_err(lock_poisoned)?;
        Ok(tasks.len())
    }

    /// Whether the registry is empty.
    pub fn is_empty(&self) -> Result<bool, RegistryError> {
        Ok(self.len()? == 0)
    }
}

impl std::fmt::Debug for TaskRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let count = self.tasks.read().map(|t| t.len()).unwrap_or(0);
        f.debug_struct("TaskRegistry")
            .field("task_count", &count)
            .field("gc_deadline", &self.gc_deadline)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    fn make_registry() -> TaskRegistry {
        TaskRegistry::new(Duration::from_secs(1))
    }

    #[test]
    fn register_and_get() {
        let reg = make_registry();
        let (id, _token) = reg
            .register(
                TaskType::Shell {
                    command: "echo hello".into(),
                },
                "test shell".into(),
            )
            .expect("register");

        let snap = reg.get(id).expect("get");
        assert_eq!(snap.id, id);
        assert_eq!(snap.status, TaskStatus::Pending);
        assert_eq!(snap.description, "test shell");
    }

    #[test]
    fn lifecycle_pending_to_running_to_completed() {
        let reg = make_registry();
        let (id, _token) = reg
            .register(
                TaskType::Agent {
                    agent_id: "alice".into(),
                    prompt: "research".into(),
                },
                "agent task".into(),
            )
            .expect("register");

        reg.update_status(id, TaskStatus::Running)
            .expect("to running");
        assert_eq!(reg.get(id).expect("get").status, TaskStatus::Running);

        reg.update_status(id, TaskStatus::Completed)
            .expect("to completed");
        let snap = reg.get(id).expect("get");
        assert_eq!(snap.status, TaskStatus::Completed);
        assert!(snap.completed_at.is_some());
    }

    #[test]
    fn lifecycle_pending_to_running_to_failed() {
        let reg = make_registry();
        let (id, _token) = reg
            .register(
                TaskType::Shell {
                    command: "false".into(),
                },
                "failing task".into(),
            )
            .expect("register");

        reg.update_status(id, TaskStatus::Running)
            .expect("to running");
        reg.update_status(id, TaskStatus::Failed)
            .expect("to failed");

        let snap = reg.get(id).expect("get");
        assert_eq!(snap.status, TaskStatus::Failed);
        assert!(snap.completed_at.is_some());
    }

    #[test]
    fn terminal_to_running_is_invalid() {
        let reg = make_registry();
        let (id, _token) = reg
            .register(
                TaskType::Monitor {
                    target: "health".into(),
                },
                "monitor".into(),
            )
            .expect("register");

        reg.update_status(id, TaskStatus::Running)
            .expect("to running");
        reg.update_status(id, TaskStatus::Completed)
            .expect("to completed");

        let result = reg.update_status(id, TaskStatus::Running);
        assert!(result.is_err());
    }

    #[test]
    fn pending_to_completed_is_invalid() {
        let reg = make_registry();
        let (id, _token) = reg
            .register(
                TaskType::Workflow {
                    name: "deploy".into(),
                },
                "workflow".into(),
            )
            .expect("register");

        // WHY: Can't skip Running to reach Completed.
        let result = reg.update_status(id, TaskStatus::Completed);
        assert!(result.is_err());
    }

    #[test]
    fn pending_to_failed_is_valid() {
        let reg = make_registry();
        let (id, _token) = reg
            .register(
                TaskType::Shell {
                    command: "bad-cmd".into(),
                },
                "fail fast".into(),
            )
            .expect("register");

        // WHY: Tasks can fail before they start (e.g. validation failure).
        reg.update_status(id, TaskStatus::Failed)
            .expect("to failed");
        assert_eq!(reg.get(id).expect("get").status, TaskStatus::Failed);
    }

    #[test]
    fn kill_sets_status_and_cancels_token() {
        let reg = make_registry();
        let (id, token) = reg
            .register(
                TaskType::Agent {
                    agent_id: "bob".into(),
                    prompt: "long task".into(),
                },
                "killable".into(),
            )
            .expect("register");

        reg.update_status(id, TaskStatus::Running)
            .expect("to running");
        assert!(!token.is_cancelled());

        reg.kill(id).expect("kill");
        assert!(token.is_cancelled());
        assert_eq!(reg.get(id).expect("get").status, TaskStatus::Killed);
    }

    #[test]
    fn kill_terminal_task_is_noop() {
        let reg = make_registry();
        let (id, _token) = reg
            .register(
                TaskType::Shell {
                    command: "done".into(),
                },
                "done task".into(),
            )
            .expect("register");

        reg.update_status(id, TaskStatus::Running)
            .expect("to running");
        reg.update_status(id, TaskStatus::Completed)
            .expect("to completed");

        // WHY: Killing an already-completed task should be silently ignored.
        reg.kill(id).expect("kill noop");
        assert_eq!(reg.get(id).expect("get").status, TaskStatus::Completed);
    }

    #[test]
    fn get_nonexistent_returns_not_found() {
        let reg = make_registry();
        let fake_id = TaskId::new();
        let result = reg.get(fake_id);
        assert!(result.is_err());
    }

    #[test]
    fn list_all_tasks() {
        let reg = make_registry();
        reg.register(
            TaskType::Shell {
                command: "a".into(),
            },
            "task a".into(),
        )
        .expect("register a");
        reg.register(
            TaskType::Shell {
                command: "b".into(),
            },
            "task b".into(),
        )
        .expect("register b");

        let all = reg.list(None).expect("list");
        assert_eq!(all.len(), 2);
    }

    #[test]
    fn list_with_status_filter() {
        let reg = make_registry();

        let (id_a, _) = reg
            .register(
                TaskType::Shell {
                    command: "a".into(),
                },
                "task a".into(),
            )
            .expect("register a");
        let (_id_b, _) = reg
            .register(
                TaskType::Shell {
                    command: "b".into(),
                },
                "task b".into(),
            )
            .expect("register b");

        reg.update_status(id_a, TaskStatus::Running)
            .expect("to running");

        let running = reg.list(Some(TaskStatus::Running)).expect("list running");
        assert_eq!(running.len(), 1);
        assert_eq!(running[0].id, id_a);

        let pending = reg.list(Some(TaskStatus::Pending)).expect("list pending");
        assert_eq!(pending.len(), 1);
    }

    #[test]
    fn record_tool_call_updates_activity() {
        let reg = make_registry();
        let (id, _) = reg
            .register(
                TaskType::Agent {
                    agent_id: "alice".into(),
                    prompt: "work".into(),
                },
                "agent".into(),
            )
            .expect("register");

        reg.record_tool_call(
            id,
            ToolCallSummary {
                tool_name: "read_file".into(),
                elapsed: jiff::SignedDuration::from_millis(150),
            },
        )
        .expect("record");

        let snap = reg.get(id).expect("get");
        assert_eq!(snap.recent_activity.len(), 1);
        assert_eq!(snap.recent_activity[0].tool_name, "read_file");
    }

    #[test]
    fn record_error_sets_snapshot() {
        let reg = make_registry();
        let (id, _) = reg
            .register(
                TaskType::Shell {
                    command: "oops".into(),
                },
                "erroring".into(),
            )
            .expect("register");

        reg.record_error(id, "something went wrong".into())
            .expect("record error");

        let snap = reg.get(id).expect("get");
        assert_eq!(snap.error_snapshot.as_deref(), Some("something went wrong"));
    }

    #[tokio::test]
    async fn progress_subscribe_receives_events() {
        let reg = make_registry();
        let (id, _) = reg
            .register(
                TaskType::Consolidation { sessions_count: 5 },
                "consolidate".into(),
            )
            .expect("register");

        let mut rx = reg.subscribe(id).expect("subscribe");

        reg.update_status(id, TaskStatus::Running)
            .expect("to running");

        let event = rx.recv().await.expect("recv");
        match event {
            ProgressEvent::StatusChanged { from, to } => {
                assert_eq!(from, TaskStatus::Pending);
                assert_eq!(to, TaskStatus::Running);
            }
            other => panic!("unexpected event: {other:?}"),
        }
    }

    #[tokio::test]
    async fn progress_subscribe_receives_tool_activity() {
        let reg = make_registry();
        let (id, _) = reg
            .register(
                TaskType::Agent {
                    agent_id: "alice".into(),
                    prompt: "work".into(),
                },
                "agent".into(),
            )
            .expect("register");

        let mut rx = reg.subscribe(id).expect("subscribe");

        reg.record_tool_call(
            id,
            ToolCallSummary {
                tool_name: "search".into(),
                elapsed: jiff::SignedDuration::from_millis(200),
            },
        )
        .expect("record");

        let event = rx.recv().await.expect("recv");
        match event {
            ProgressEvent::ToolActivity(summary) => {
                assert_eq!(summary.tool_name, "search");
            }
            other => panic!("unexpected event: {other:?}"),
        }
    }

    #[test]
    fn concurrent_register_from_multiple_threads() {
        let reg = make_registry();
        let reg_clone = reg.clone();

        let handles: Vec<_> = (0..10)
            .map(|i| {
                let r = reg_clone.clone();
                std::thread::spawn(move || {
                    r.register(
                        TaskType::Shell {
                            command: format!("cmd_{i}"),
                        },
                        format!("task {i}"),
                    )
                    .expect("register");
                })
            })
            .collect();

        for h in handles {
            h.join().expect("thread join");
        }

        assert_eq!(reg.len().expect("len"), 10);
    }

    #[test]
    fn gc_sweep_evicts_stale_tasks() {
        // WHY: Use a zero deadline so tasks are immediately eligible.
        let reg = TaskRegistry::new(Duration::from_secs(0));

        let (id, _) = reg
            .register(
                TaskType::Shell {
                    command: "done".into(),
                },
                "stale".into(),
            )
            .expect("register");

        reg.update_status(id, TaskStatus::Running)
            .expect("to running");
        reg.update_status(id, TaskStatus::Completed)
            .expect("to completed");

        let evicted = reg.gc_sweep().expect("gc sweep");
        assert_eq!(evicted.len(), 1);
        assert_eq!(evicted[0].0, id);

        // WHY: After GC, the task should no longer be in the registry.
        assert!(reg.get(id).is_err());
    }

    #[test]
    fn gc_sweep_retains_non_terminal_tasks() {
        let reg = TaskRegistry::new(Duration::from_secs(0));

        let (id, _) = reg
            .register(
                TaskType::Shell {
                    command: "running".into(),
                },
                "active".into(),
            )
            .expect("register");

        reg.update_status(id, TaskStatus::Running)
            .expect("to running");

        let evicted = reg.gc_sweep().expect("gc sweep");
        assert!(evicted.is_empty());
        assert!(reg.get(id).is_ok());
    }

    #[test]
    fn len_and_is_empty() {
        let reg = make_registry();
        assert!(reg.is_empty().expect("is_empty"));
        assert_eq!(reg.len().expect("len"), 0);

        reg.register(
            TaskType::Monitor {
                target: "mcp".into(),
            },
            "monitor".into(),
        )
        .expect("register");

        assert!(!reg.is_empty().expect("is_empty"));
        assert_eq!(reg.len().expect("len"), 1);
    }
}
