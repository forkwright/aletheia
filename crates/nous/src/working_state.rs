//! Working state management for nous agents.
//!
//! Tracks what the agent is currently doing (task stack), what it is focused
//! on (focus context), and what it is waiting for (wait state). Persisted to
//! SQLite via the session store blackboard so state survives crashes.

use std::sync::Arc;

use serde::{Deserialize, Serialize};

use hermeneus::types::{Message, ThinkingConfig, ToolDefinition};

use crate::bootstrap::{BootstrapSection, BootstrapSlot, SectionPriority};

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

/// Cache-critical parameters for forked agent prompt cache sharing.
///
/// Captures the five fields that compose the Anthropic API cache key.
/// Forked agents clone these from the parent via [`Arc`] so child API calls
/// hit the parent's prompt cache, reducing input token costs by ~90%.
///
/// WHY: the Anthropic API keys its prompt cache on system prompt, tools list,
/// model, message prefix, and thinking config. Sharing these identically
/// between parent and child ensures cache hits on the child's first call.
#[derive(Debug, Clone)]
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "forked agent cache coherence — fields read by spawn path and pipeline"
    )
)]
pub(crate) struct CacheSafeParams {
    /// Complete system prompt (not a template).
    pub(crate) system_prompt: Arc<str>,
    /// Tools list sorted deterministically.
    /// WHY: tool order affects the Anthropic API cache key.
    pub(crate) tools: Arc<[ToolDefinition]>,
    /// Model identifier (e.g., `claude-opus-4-20250514`).
    pub(crate) model: String,
    /// Shared immutable conversation prefix from the parent.
    /// WHY: `Arc<[Message]>` avoids deep-cloning the message history on fork.
    pub(crate) message_prefix: Arc<[Message]>,
    /// Thinking config (`budget_tokens`), if applicable.
    pub(crate) thinking_config: Option<ThinkingConfig>,
}

impl CacheSafeParams {
    /// Create cache-safe params with deterministically sorted tools.
    ///
    /// Tools are sorted by name to guarantee identical cache keys regardless
    /// of registration order.
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "forked agent cache coherence — wired from spawn path"
        )
    )]
    pub(crate) fn new(
        system_prompt: impl Into<Arc<str>>,
        mut tools: Vec<ToolDefinition>,
        model: impl Into<String>,
        message_prefix: Arc<[Message]>,
        thinking_config: Option<ThinkingConfig>,
    ) -> Self {
        // WHY: tool order affects the Anthropic API cache key; deterministic sorting
        // ensures forked agents produce identical cache keys.
        tools.sort_by(|a, b| a.name.cmp(&b.name));
        Self {
            system_prompt: system_prompt.into(),
            tools: Arc::from(tools),
            model: model.into(),
            message_prefix,
            thinking_config,
        }
    }
}

/// Working state: tracks the agent's current activities, focus, and pending operations.
///
/// Designed for persistence and context assembly. The task stack enables
/// session resumption by showing where the agent left off. Focus context
/// influences which memories are recalled. Wait state tracks pending
/// operations for the TUI dashboard.
#[derive(Debug, Clone, Default, Serialize)]
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
    /// Cache-safe parameters shared with forked agents.
    /// WHY: runtime-only field; not persisted because `Arc` references are session-scoped.
    #[serde(skip)]
    pub(crate) cache_params: Option<Arc<CacheSafeParams>>,
}

/// Raw deserialization type for [`WorkingState`].
#[derive(Debug, Clone, Default, Deserialize)]
struct WorkingStateRaw {
    task_stack: Vec<TaskEntry>,
    #[serde(skip_serializing_if = "Option::is_none")]
    focus: Option<FocusContext>,
    #[serde(skip_serializing_if = "Option::is_none")]
    wait: Option<WaitState>,
    #[serde(skip_serializing_if = "Option::is_none")]
    updated_at: Option<String>,
}

impl From<WorkingStateRaw> for WorkingState {
    fn from(raw: WorkingStateRaw) -> Self {
        Self {
            task_stack: raw.task_stack,
            focus: raw.focus,
            wait: raw.wait,
            updated_at: raw.updated_at,
            cache_params: None,
        }
    }
}

// WHY: Manual Deserialize to satisfy RUST/serde-bypass-constructor:
// `WorkingState::new()` exists as a constructor, so serde must route through
// a conversion (`From<WorkingStateRaw>`) rather than populating fields directly.
impl<'de> Deserialize<'de> for WorkingState {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        WorkingStateRaw::deserialize(deserializer).map(Self::from)
    }
}

impl WorkingState {
    /// Create an empty working state.
    #[must_use]
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "working state management for agent context")
    )]
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Push a task onto the stack.
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "working state management for agent context")
    )]
    pub(crate) fn push_task(&mut self, description: impl Into<String>) {
        let max_task_stack =
            taxis::config::AgentBehaviorDefaults::default().working_state_max_task_stack;
        tracing::debug!(
            max_task_stack,
            "push_task: max stack depth from AgentBehaviorDefaults"
        );
        if self.task_stack.len() >= max_task_stack {
            self.task_stack.remove(0);
        }
        self.task_stack.push(TaskEntry {
            description: description.into(),
            started_at: now_iso8601(),
        });
        self.touch();
    }

    /// Pop the most recent task from the stack.
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "working state management for agent context")
    )]
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
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "WIP: agent pipeline infrastructure")
    )]
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
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "WIP: agent pipeline infrastructure")
    )]
    pub(crate) fn clear_focus(&mut self) {
        self.focus = None;
        self.touch();
    }

    /// Set the wait state.
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "working state management for agent context")
    )]
    pub(crate) fn set_wait(&mut self, kind: WaitKind, description: impl Into<String>) {
        self.wait = Some(WaitState {
            kind,
            description: description.into(),
            since: now_iso8601(),
        });
        self.touch();
    }

    /// Clear the wait state.
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "working state management for agent context")
    )]
    pub(crate) fn clear_wait(&mut self) {
        self.wait = None;
        self.touch();
    }

    /// Whether the working state has any content worth persisting.
    #[must_use]
    pub(crate) fn is_empty(&self) -> bool {
        self.task_stack.is_empty() && self.focus.is_none() && self.wait.is_none()
    }

    /// Set cache-safe parameters for prompt cache sharing with forked agents.
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "forked agent cache coherence — wired from spawn path"
        )
    )]
    pub(crate) fn set_cache_params(&mut self, params: Arc<CacheSafeParams>) {
        self.cache_params = Some(params);
    }

    /// Create a child working state for a forked agent.
    ///
    /// Shares cache-safe params via [`Arc`] (zero-copy) and deep-clones
    /// mutable state. The child starts with no pending wait state.
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "forked agent cache coherence — wired from spawn path"
        )
    )]
    pub(crate) fn clone_for_fork(&self) -> Self {
        Self {
            // WHY: deep clone mutable state so child mutations don't affect parent
            task_stack: self.task_stack.clone(),
            focus: self.focus.clone(),
            // WHY: child starts with no pending wait; it will set its own
            wait: None,
            updated_at: Some(now_iso8601()),
            // WHY: Arc clone is zero-copy; child shares parent's cache key
            cache_params: self.cache_params.clone(),
        }
    }

    /// Generate the blackboard key for persisting this state.
    #[must_use]
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "working state management for agent context")
    )]
    pub(crate) fn persist_key(nous_id: &str, session_id: &str) -> String {
        format!("ws:{nous_id}:{session_id}")
    }

    /// Serialize to JSON for blackboard storage.
    ///
    /// # Errors
    ///
    /// Returns serialization error (should not happen for valid state).
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "working state management for agent context")
    )]
    pub(crate) fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(self)
    }

    /// Deserialize from JSON blackboard value.
    ///
    /// # Errors
    ///
    /// Returns deserialization error if the JSON is malformed.
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "working state management for agent context")
    )]
    pub(crate) fn from_json(json: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(json)
    }

    /// Format as a bootstrap section for inclusion in the system prompt.
    ///
    /// Returns `None` if the working state is empty.
    #[must_use]
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "working state management for agent context")
    )]
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
            slot: BootstrapSlot::Context,
        })
    }

    fn touch(&mut self) {
        self.updated_at = Some(now_iso8601());
    }
}

fn now_iso8601() -> String {
    jiff::Timestamp::now().to_string()
}

#[cfg(test)]
#[path = "working_state_tests.rs"]
mod working_state_tests;
