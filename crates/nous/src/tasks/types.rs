//! Task types, state, and progress events.

use std::collections::VecDeque;
use std::fmt;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;
use tokio_util::sync::CancellationToken;

/// Maximum number of recent tool call summaries kept per task.
const ACTIVITY_WINDOW_SIZE: usize = 5;

/// Capacity of the per-task progress broadcast channel.
///
/// WHY: Bounded to apply backpressure on slow subscribers rather than growing
/// memory unboundedly. 64 events is enough for bursty tool execution without
/// dropping under normal conditions.
pub(crate) const PROGRESS_CHANNEL_CAPACITY: usize = 64;

// ---------------------------------------------------------------------------
// TaskId
// ---------------------------------------------------------------------------

/// Stable task identifier wrapping a UUID v4.
///
/// WHY: Tasks need identity that survives across progress updates, GC checks,
/// and UI refreshes. UUID provides collision-free generation without coordination.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TaskId(uuid::Uuid);

impl TaskId {
    /// Generate a new random task ID.
    pub fn new() -> Self {
        Self(uuid::Uuid::new_v4())
    }
}

impl Default for TaskId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for TaskId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

// ---------------------------------------------------------------------------
// TaskType
// ---------------------------------------------------------------------------

/// Task variant with type-specific metadata.
///
/// WHY: Task display and lifecycle differ by type. Shell tasks carry a command
/// string, agent tasks carry an agent ID and prompt, etc. Discriminating at the
/// type level lets callers match exhaustively.
#[non_exhaustive]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TaskType {
    /// A shell command execution.
    Shell {
        /// The command being run.
        command: String,
    },
    /// A sub-agent running an autonomous loop.
    Agent {
        /// Identity of the spawned agent.
        agent_id: String,
        /// The prompt given to the agent.
        prompt: String,
    },
    /// A multi-step workflow execution.
    Workflow {
        /// Human-readable workflow name.
        name: String,
    },
    /// Memory consolidation (dream) task.
    Consolidation {
        /// Number of sessions being consolidated.
        sessions_count: usize,
    },
    /// Background monitoring task (e.g. MCP health).
    Monitor {
        /// What is being monitored.
        target: String,
    },
}

impl fmt::Display for TaskType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Shell { command } => write!(f, "shell: {command}"),
            Self::Agent { agent_id, .. } => write!(f, "agent: {agent_id}"),
            Self::Workflow { name } => write!(f, "workflow: {name}"),
            Self::Consolidation { sessions_count } => {
                write!(f, "consolidation: {sessions_count} sessions")
            }
            Self::Monitor { target } => write!(f, "monitor: {target}"),
        }
    }
}

// ---------------------------------------------------------------------------
// TaskStatus
// ---------------------------------------------------------------------------

/// Task status lifecycle.
///
/// ```text
/// Pending -> Running -> Completed
///                    -> Failed
///                    -> Killed
/// ```
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TaskStatus {
    /// Registered but not yet started.
    Pending,
    /// Actively executing.
    Running,
    /// Finished successfully.
    Completed,
    /// Terminated due to an error.
    Failed,
    /// Explicitly cancelled via `kill()`.
    Killed,
}

impl TaskStatus {
    /// Whether this status is terminal (no further transitions allowed).
    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Failed | Self::Killed)
    }
}

impl fmt::Display for TaskStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Pending => f.write_str("pending"),
            Self::Running => f.write_str("running"),
            Self::Completed => f.write_str("completed"),
            Self::Failed => f.write_str("failed"),
            Self::Killed => f.write_str("killed"),
        }
    }
}

// ---------------------------------------------------------------------------
// ToolCallSummary
// ---------------------------------------------------------------------------

/// Summary of a single tool call for the rolling activity window.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallSummary {
    /// Name of the tool that was called.
    pub tool_name: String,
    /// Wall-clock duration of the tool execution.
    pub elapsed: jiff::SignedDuration,
}

// ---------------------------------------------------------------------------
// ProgressEvent
// ---------------------------------------------------------------------------

/// Progress event emitted on a task's broadcast channel.
///
/// WHY: Subscribers (UI, logging, parent agents) need typed events to decide
/// what to display. A single enum keeps the channel monomorphic.
#[non_exhaustive]
#[derive(Debug, Clone)]
pub enum ProgressEvent {
    /// Task transitioned between statuses.
    StatusChanged {
        /// Previous status.
        from: TaskStatus,
        /// New status.
        to: TaskStatus,
    },
    /// A tool call completed.
    ToolActivity(ToolCallSummary),
    /// A chunk of output was produced.
    OutputChunk(Vec<u8>),
    /// An error snapshot for diagnostics.
    Error(String),
}

// ---------------------------------------------------------------------------
// TaskEntry
// ---------------------------------------------------------------------------

/// A task entry in the registry.
///
/// Contains all state needed for status queries, progress streaming,
/// cancellation, and GC eligibility.
pub struct TaskEntry {
    /// Unique task identifier.
    pub id: TaskId,
    /// What kind of task this is.
    pub task_type: TaskType,
    /// Current lifecycle status.
    pub status: TaskStatus,
    /// Human-readable description.
    pub description: String,
    /// When the task was registered.
    pub created_at: jiff::Timestamp,
    /// When the task reached a terminal status.
    pub completed_at: Option<jiff::Timestamp>,
    /// Rolling window of recent tool call summaries (last 5).
    pub recent_activity: VecDeque<ToolCallSummary>,
    /// Token for cooperative cancellation.
    pub cancellation_token: CancellationToken,
    /// Broadcast sender for progress events.
    ///
    /// WHY: `broadcast::Sender` is kept alive as long as the entry exists so
    /// late-joining subscribers can still receive future events. Capacity is
    /// bounded at [`PROGRESS_CHANNEL_CAPACITY`].
    pub progress_tx: broadcast::Sender<ProgressEvent>,
    /// Path to the disk-backed output file, if created.
    pub output_path: Option<PathBuf>,
    /// Last error message, if any.
    pub error_snapshot: Option<String>,
}

impl TaskEntry {
    /// Create a new task entry with the given type and description.
    pub(crate) fn new(task_type: TaskType, description: String) -> Self {
        let (progress_tx, _) = broadcast::channel(PROGRESS_CHANNEL_CAPACITY);
        Self {
            id: TaskId::new(),
            task_type,
            status: TaskStatus::Pending,
            description,
            created_at: jiff::Timestamp::now(),
            completed_at: None,
            recent_activity: VecDeque::with_capacity(ACTIVITY_WINDOW_SIZE),
            cancellation_token: CancellationToken::new(),
            progress_tx,
            output_path: None,
            error_snapshot: None,
        }
    }

    /// Record a tool call in the rolling activity window.
    ///
    /// Evicts the oldest entry when the window is full.
    pub(crate) fn record_tool_call(&mut self, summary: ToolCallSummary) {
        if self.recent_activity.len() >= ACTIVITY_WINDOW_SIZE {
            self.recent_activity.pop_front();
        }
        self.recent_activity.push_back(summary);
    }
}

impl fmt::Debug for TaskEntry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TaskEntry")
            .field("id", &self.id)
            .field("task_type", &self.task_type)
            .field("status", &self.status)
            .field("description", &self.description)
            .field("created_at", &self.created_at)
            .field("completed_at", &self.completed_at)
            .field("recent_activity", &self.recent_activity)
            .field("output_path", &self.output_path)
            .field("error_snapshot", &self.error_snapshot)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn task_id_display_is_uuid_format() {
        let id = TaskId::new();
        let s = id.to_string();
        // WHY: UUID v4 has a specific format with hyphens
        assert_eq!(s.len(), 36);
        assert_eq!(s.chars().filter(|c| *c == '-').count(), 4);
    }

    #[test]
    fn task_id_uniqueness() {
        let a = TaskId::new();
        let b = TaskId::new();
        assert_ne!(a, b);
    }

    #[test]
    fn task_status_terminal_states() {
        assert!(!TaskStatus::Pending.is_terminal());
        assert!(!TaskStatus::Running.is_terminal());
        assert!(TaskStatus::Completed.is_terminal());
        assert!(TaskStatus::Failed.is_terminal());
        assert!(TaskStatus::Killed.is_terminal());
    }

    #[test]
    fn task_entry_rolling_window_caps_at_five() {
        let mut entry = TaskEntry::new(
            TaskType::Shell {
                command: "echo test".into(),
            },
            "test task".into(),
        );

        for i in 0..7 {
            entry.record_tool_call(ToolCallSummary {
                tool_name: format!("tool_{i}"),
                elapsed: jiff::SignedDuration::from_secs(1),
            });
        }

        assert_eq!(entry.recent_activity.len(), 5);
        // WHY: First two should have been evicted
        assert_eq!(
            entry.recent_activity.front().map(|s| s.tool_name.as_str()),
            Some("tool_2")
        );
        assert_eq!(
            entry.recent_activity.back().map(|s| s.tool_name.as_str()),
            Some("tool_6")
        );
    }

    #[test]
    fn task_entry_starts_pending() {
        let entry = TaskEntry::new(
            TaskType::Agent {
                agent_id: "alice".into(),
                prompt: "research topic".into(),
            },
            "agent task".into(),
        );
        assert_eq!(entry.status, TaskStatus::Pending);
        assert!(entry.completed_at.is_none());
        assert!(entry.error_snapshot.is_none());
    }

    #[test]
    fn task_type_display() {
        let shell = TaskType::Shell {
            command: "ls -la".into(),
        };
        assert_eq!(shell.to_string(), "shell: ls -la");

        let agent = TaskType::Agent {
            agent_id: "bob".into(),
            prompt: "search".into(),
        };
        assert_eq!(agent.to_string(), "agent: bob");

        let consolidation = TaskType::Consolidation { sessions_count: 3 };
        assert_eq!(consolidation.to_string(), "consolidation: 3 sessions");
    }

    #[test]
    fn task_status_display() {
        assert_eq!(TaskStatus::Pending.to_string(), "pending");
        assert_eq!(TaskStatus::Running.to_string(), "running");
        assert_eq!(TaskStatus::Completed.to_string(), "completed");
        assert_eq!(TaskStatus::Failed.to_string(), "failed");
        assert_eq!(TaskStatus::Killed.to_string(), "killed");
    }
}
