//! Working state management for nous agents.
//!
//! Tracks what the agent is currently doing (task stack), what it is focused
//! on (focus context), and what it is waiting for (wait state). Persisted to
//! SQLite via the session store blackboard so state survives crashes.

use serde::{Deserialize, Serialize};

use crate::bootstrap::{BootstrapSection, SectionPriority};

/// What kind of operation the agent is waiting for.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum WaitKind {
    /// Waiting for a tool to return its result.
    ToolResult,
    /// Waiting for the user to provide input.
    UserInput,
    /// Waiting for a sub-agent to complete work.
    SubAgent,
}

/// A single item in the task stack.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskEntry {
    /// Human-readable description of the task.
    pub description: String,
    /// ISO-8601 timestamp when the task was pushed.
    pub started_at: String,
}

/// What the agent is currently focused on.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FocusContext {
    /// File path the agent is working with.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    /// Function or method name within the file.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub function: Option<String>,
    /// Abstract concept or topic the agent is exploring.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub concept: Option<String>,
}

/// What the agent is waiting for.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WaitState {
    /// Type of pending operation.
    pub kind: WaitKind,
    /// Human-readable description of what is being waited on.
    pub description: String,
    /// ISO-8601 timestamp when the wait began.
    pub since: String,
}

/// Working state: tracks the agent's current activities, focus, and pending operations.
///
/// Designed for persistence and context assembly. The task stack enables
/// session resumption by showing where the agent left off. Focus context
/// influences which memories are recalled. Wait state tracks pending
/// operations for the TUI dashboard.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WorkingState {
    /// Stack of active tasks (most recent at the end).
    pub task_stack: Vec<TaskEntry>,
    /// Current focus context.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub focus: Option<FocusContext>,
    /// Current wait state.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wait: Option<WaitState>,
    /// ISO-8601 timestamp of the last update.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
}

/// Maximum task stack depth before oldest entries are evicted.
const MAX_TASK_STACK: usize = 10;

impl WorkingState {
    /// Create an empty working state.
    #[must_use]
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Push a task onto the stack.
    pub(crate) fn push_task(&mut self, description: impl Into<String>) {
        if self.task_stack.len() >= MAX_TASK_STACK {
            self.task_stack.remove(0);
        }
        self.task_stack.push(TaskEntry {
            description: description.into(),
            started_at: now_iso8601(),
        });
        self.touch();
    }

    /// Pop the most recent task from the stack.
    pub(crate) fn pop_task(&mut self) -> Option<TaskEntry> {
        let task = self.task_stack.pop();
        if task.is_some() {
            self.touch();
        }
        task
    }

    /// Peek at the current (most recent) task.
    #[must_use]
    pub(crate) fn current_task(&self) -> Option<&TaskEntry> {
        self.task_stack.last()
    }

    /// Set the focus context.
    pub(crate) fn set_focus(
        &mut self,
        file: Option<String>,
        function: Option<String>,
        concept: Option<String>,
    ) {
        self.focus = Some(FocusContext {
            file,
            function,
            concept,
        });
        self.touch();
    }

    /// Clear the focus context.
    pub(crate) fn clear_focus(&mut self) {
        self.focus = None;
        self.touch();
    }

    /// Set the wait state.
    pub(crate) fn set_wait(&mut self, kind: WaitKind, description: impl Into<String>) {
        self.wait = Some(WaitState {
            kind,
            description: description.into(),
            since: now_iso8601(),
        });
        self.touch();
    }

    /// Clear the wait state.
    pub(crate) fn clear_wait(&mut self) {
        self.wait = None;
        self.touch();
    }

    /// Whether the working state has any content worth persisting.
    #[must_use]
    pub(crate) fn is_empty(&self) -> bool {
        self.task_stack.is_empty() && self.focus.is_none() && self.wait.is_none()
    }

    /// Generate the blackboard key for persisting this state.
    #[must_use]
    pub(crate) fn persist_key(nous_id: &str, session_id: &str) -> String {
        format!("ws:{nous_id}:{session_id}")
    }

    /// Serialize to JSON for blackboard storage.
    ///
    /// # Errors
    ///
    /// Returns serialization error (should not happen for valid state).
    pub(crate) fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(self)
    }

    /// Deserialize from JSON blackboard value.
    ///
    /// # Errors
    ///
    /// Returns deserialization error if the JSON is malformed.
    pub(crate) fn from_json(json: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(json)
    }

    /// Format as a bootstrap section for inclusion in the system prompt.
    ///
    /// Returns `None` if the working state is empty.
    #[must_use]
    pub(crate) fn to_bootstrap_section(&self) -> Option<BootstrapSection> {
        if self.is_empty() {
            return None;
        }

        let mut parts = Vec::new();

        if let Some(task) = self.current_task() {
            parts.push(format!("**Current task:** {}", task.description));
        }

        if self.task_stack.len() > 1 {
            let stack_desc: Vec<&str> = self
                .task_stack
                .iter()
                .rev()
                .skip(1)
                .map(|t| t.description.as_str())
                .collect();
            parts.push(format!("**Task stack:** {}", stack_desc.join(" → ")));
        }

        if let Some(ref focus) = self.focus {
            let mut focus_parts = Vec::new();
            if let Some(ref file) = focus.file {
                focus_parts.push(format!("file: {file}"));
            }
            if let Some(ref function) = focus.function {
                focus_parts.push(format!("function: {function}"));
            }
            if let Some(ref concept) = focus.concept {
                focus_parts.push(format!("concept: {concept}"));
            }
            if !focus_parts.is_empty() {
                parts.push(format!("**Focus:** {}", focus_parts.join(", ")));
            }
        }

        if let Some(ref wait) = self.wait {
            let kind_str = match wait.kind {
                WaitKind::ToolResult => "tool result",
                WaitKind::UserInput => "user input",
                WaitKind::SubAgent => "sub-agent",
            };
            parts.push(format!(
                "**Waiting for:** {} ({})",
                wait.description, kind_str
            ));
        }

        let content = parts.join("\n");
        let token_estimate = u64::try_from(content.len() / 4).unwrap_or(0);

        Some(BootstrapSection {
            name: "WORKING_STATE".to_owned(),
            priority: SectionPriority::Flexible,
            content,
            tokens: token_estimate,
            truncatable: true,
        })
    }

    fn touch(&mut self) {
        self.updated_at = Some(now_iso8601());
    }
}

/// Blackboard TTL for working state entries (7 days).
pub(crate) const WORKING_STATE_TTL_SECS: i64 = 604_800;

fn now_iso8601() -> String {
    jiff::Timestamp::now().to_string()
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
mod tests {
    use super::*;

    #[test]
    fn empty_state_is_empty() {
        let state = WorkingState::new();
        assert!(state.is_empty(), "new state should be empty");
        assert!(
            state.current_task().is_none(),
            "no current task on empty state"
        );
    }

    #[test]
    fn push_pop_task_lifecycle() {
        let mut state = WorkingState::new();
        state.push_task("implement feature X");
        assert!(!state.is_empty(), "state should not be empty after push");
        assert_eq!(
            state.current_task().unwrap().description,
            "implement feature X"
        );

        state.push_task("write tests for X");
        assert_eq!(state.task_stack.len(), 2);
        assert_eq!(
            state.current_task().unwrap().description,
            "write tests for X"
        );

        let popped = state.pop_task().unwrap();
        assert_eq!(popped.description, "write tests for X");
        assert_eq!(
            state.current_task().unwrap().description,
            "implement feature X"
        );

        state.pop_task();
        assert!(state.is_empty(), "state should be empty after popping all");
    }

    #[test]
    fn task_stack_evicts_at_max_depth() {
        let mut state = WorkingState::new();
        for i in 0..15 {
            state.push_task(format!("task {i}"));
        }
        assert_eq!(
            state.task_stack.len(),
            MAX_TASK_STACK,
            "stack should not exceed max depth"
        );
        assert_eq!(
            state.task_stack.first().unwrap().description,
            "task 5",
            "oldest tasks should be evicted"
        );
    }

    #[test]
    fn set_clear_focus() {
        let mut state = WorkingState::new();
        state.set_focus(
            Some("src/main.rs".to_owned()),
            Some("handle_request".to_owned()),
            None,
        );
        assert!(state.focus.is_some());
        assert!(!state.is_empty());

        let focus = state.focus.as_ref().unwrap();
        assert_eq!(focus.file.as_deref(), Some("src/main.rs"));
        assert_eq!(focus.function.as_deref(), Some("handle_request"));
        assert!(focus.concept.is_none());

        state.clear_focus();
        assert!(state.focus.is_none());
    }

    #[test]
    fn set_clear_wait() {
        let mut state = WorkingState::new();
        state.set_wait(WaitKind::ToolResult, "read_file /tmp/test.txt");
        assert!(state.wait.is_some());
        assert!(!state.is_empty());

        let wait = state.wait.as_ref().unwrap();
        assert_eq!(wait.kind, WaitKind::ToolResult);
        assert_eq!(wait.description, "read_file /tmp/test.txt");

        state.clear_wait();
        assert!(state.wait.is_none());
    }

    #[test]
    fn serde_roundtrip() {
        let mut state = WorkingState::new();
        state.push_task("investigate bug #42");
        state.set_focus(
            Some("src/lib.rs".to_owned()),
            None,
            Some("error handling".to_owned()),
        );
        state.set_wait(WaitKind::SubAgent, "code review agent");

        let json = state.to_json().unwrap();
        let restored = WorkingState::from_json(&json).unwrap();

        assert_eq!(restored.task_stack.len(), 1);
        assert_eq!(
            restored.current_task().unwrap().description,
            "investigate bug #42"
        );
        assert_eq!(
            restored.focus.as_ref().unwrap().concept.as_deref(),
            Some("error handling")
        );
        assert_eq!(restored.wait.as_ref().unwrap().kind, WaitKind::SubAgent);
    }

    #[test]
    fn empty_state_produces_no_bootstrap_section() {
        let state = WorkingState::new();
        assert!(
            state.to_bootstrap_section().is_none(),
            "empty state should produce no section"
        );
    }

    #[test]
    fn populated_state_produces_bootstrap_section() {
        let mut state = WorkingState::new();
        state.push_task("deploy service");
        state.set_focus(None, None, Some("deployment pipeline".to_owned()));

        let section = state.to_bootstrap_section().unwrap();
        assert_eq!(section.name, "WORKING_STATE");
        assert!(
            section.content.contains("deploy service"),
            "section should contain task description"
        );
        assert!(
            section.content.contains("deployment pipeline"),
            "section should contain focus concept"
        );
    }

    #[test]
    fn persist_key_format() {
        let key = WorkingState::persist_key("syn", "ses-123");
        assert_eq!(key, "ws:syn:ses-123");
    }

    #[test]
    fn updated_at_set_on_mutation() {
        let mut state = WorkingState::new();
        assert!(state.updated_at.is_none());

        state.push_task("test");
        assert!(state.updated_at.is_some());
    }

    #[test]
    fn pop_empty_returns_none() {
        let mut state = WorkingState::new();
        assert!(state.pop_task().is_none());
    }

    #[test]
    fn wait_kind_serde_roundtrip() {
        let kinds = [
            WaitKind::ToolResult,
            WaitKind::UserInput,
            WaitKind::SubAgent,
        ];
        for kind in kinds {
            let json = serde_json::to_string(&kind).unwrap();
            let back: WaitKind = serde_json::from_str(&json).unwrap();
            assert_eq!(kind, back);
        }
    }

    #[test]
    fn bootstrap_section_with_task_stack() {
        let mut state = WorkingState::new();
        state.push_task("parent task");
        state.push_task("child task");

        let section = state.to_bootstrap_section().unwrap();
        assert!(
            section.content.contains("Current task"),
            "should show current task"
        );
        assert!(
            section.content.contains("Task stack"),
            "should show task stack with depth > 1"
        );
    }

    #[test]
    fn bootstrap_section_with_wait() {
        let mut state = WorkingState::new();
        state.set_wait(WaitKind::UserInput, "confirmation");

        let section = state.to_bootstrap_section().unwrap();
        assert!(
            section.content.contains("Waiting for"),
            "should show wait state"
        );
        assert!(
            section.content.contains("user input"),
            "should show wait kind"
        );
    }

    #[test]
    fn from_json_invalid_returns_error() {
        let result = WorkingState::from_json("not json");
        assert!(result.is_err());
    }
}
