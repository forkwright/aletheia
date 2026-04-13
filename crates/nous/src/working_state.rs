//! Working state management for nous agents.
//!
//! Tracks what the agent is currently doing (task stack), what it is focused
//! on (focus context), and what it is waiting for (wait state). Persisted to
//! SQLite via the session store blackboard so state survives crashes.

use std::sync::Arc;

use serde::{Deserialize, Serialize};

use hermeneus::types::{Message, ThinkingConfig, ToolDefinition};

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
    /// Tools are sorted by name to ensure identical cache keys regardless
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
    #[cfg_attr(not(test), expect(dead_code, reason = "working state management for agent context"))]
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Push a task onto the stack.
    #[cfg_attr(not(test), expect(dead_code, reason = "working state management for agent context"))]
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
    #[cfg_attr(not(test), expect(dead_code, reason = "working state management for agent context"))]
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
    #[cfg_attr(not(test), expect(dead_code, reason = "WIP: agent pipeline infrastructure"))]
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
    #[cfg_attr(not(test), expect(dead_code, reason = "WIP: agent pipeline infrastructure"))]
    pub(crate) fn clear_focus(&mut self) {
        self.focus = None;
        self.touch();
    }

    /// Set the wait state.
    #[cfg_attr(not(test), expect(dead_code, reason = "working state management for agent context"))]
    pub(crate) fn set_wait(&mut self, kind: WaitKind, description: impl Into<String>) {
        self.wait = Some(WaitState {
            kind,
            description: description.into(),
            since: now_iso8601(),
        });
        self.touch();
    }

    /// Clear the wait state.
    #[cfg_attr(not(test), expect(dead_code, reason = "working state management for agent context"))]
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
    #[cfg_attr(not(test), expect(dead_code, reason = "working state management for agent context"))]
    pub(crate) fn persist_key(nous_id: &str, session_id: &str) -> String {
        format!("ws:{nous_id}:{session_id}")
    }

    /// Serialize to JSON for blackboard storage.
    ///
    /// # Errors
    ///
    /// Returns serialization error (should not happen for valid state).
    #[cfg_attr(not(test), expect(dead_code, reason = "working state management for agent context"))]
    pub(crate) fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(self)
    }

    /// Deserialize from JSON blackboard value.
    ///
    /// # Errors
    ///
    /// Returns deserialization error if the JSON is malformed.
    #[cfg_attr(not(test), expect(dead_code, reason = "working state management for agent context"))]
    pub(crate) fn from_json(json: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(json)
    }

    /// Format as a bootstrap section for inclusion in the system prompt.
    ///
    /// Returns `None` if the working state is empty.
    #[must_use]
    #[cfg_attr(not(test), expect(dead_code, reason = "working state management for agent context"))]
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

fn now_iso8601() -> String {
    jiff::Timestamp::now().to_string()
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
#[expect(
    clippy::indexing_slicing,
    reason = "test assertions on known-length slices"
)]
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
        let max_task_stack =
            taxis::config::AgentBehaviorDefaults::default().working_state_max_task_stack;
        let mut state = WorkingState::new();
        for i in 0..max_task_stack + 5 {
            state.push_task(format!("task {i}"));
        }
        assert_eq!(
            state.task_stack.len(),
            max_task_stack,
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

    // -- Forked agent cache coherence tests --

    fn sample_tool_definition(name: &str) -> ToolDefinition {
        ToolDefinition {
            name: name.to_owned(),
            description: format!("Test tool {name}"),
            input_schema: serde_json::json!({"type": "object"}),
            disable_passthrough: None,
        }
    }

    fn sample_message(text: &str) -> Message {
        Message {
            role: hermeneus::types::Role::User,
            content: hermeneus::types::Content::Text(text.to_owned()),
        }
    }

    fn sample_cache_params() -> Arc<CacheSafeParams> {
        let messages: Arc<[Message]> =
            Arc::from(vec![sample_message("hello"), sample_message("world")]);
        Arc::new(CacheSafeParams::new(
            "You are a test agent.",
            vec![sample_tool_definition("zeta_tool"), sample_tool_definition("alpha_tool")],
            "claude-opus-4-20250514",
            messages,
            Some(ThinkingConfig {
                enabled: true,
                budget_tokens: 4096,
            }),
        ))
    }

    #[test]
    fn cache_safe_params_preserves_all_fields() {
        let params = sample_cache_params();
        assert_eq!(params.model, "claude-opus-4-20250514");
        assert_eq!(params.system_prompt.as_ref(), "You are a test agent.");
        assert_eq!(params.tools.len(), 2);
        assert_eq!(params.message_prefix.len(), 2);
        let thinking = params.thinking_config.as_ref().unwrap();
        assert!(thinking.enabled);
        assert_eq!(thinking.budget_tokens, 4096);
    }

    #[test]
    fn cache_safe_params_sorts_tools_deterministically() {
        let params = sample_cache_params();
        // WHY: tools were provided as [zeta, alpha] but must be sorted [alpha, zeta]
        assert_eq!(params.tools[0].name, "alpha_tool");
        assert_eq!(params.tools[1].name, "zeta_tool");
    }

    #[test]
    fn clone_for_fork_shares_cache_params_via_arc() {
        let mut parent = WorkingState::new();
        let params = sample_cache_params();
        parent.set_cache_params(Arc::clone(&params));
        parent.push_task("parent work");

        let child = parent.clone_for_fork();

        // WHY: Arc pointer equality proves zero-copy sharing (no deep clone)
        let parent_params = parent.cache_params.as_ref().unwrap();
        let child_params = child.cache_params.as_ref().unwrap();
        assert!(Arc::ptr_eq(parent_params, child_params));

        // NOTE: inner Arc fields are also shared transitively
        assert!(Arc::ptr_eq(
            &parent_params.system_prompt,
            &child_params.system_prompt
        ));
        assert!(Arc::ptr_eq(&parent_params.tools, &child_params.tools));
        assert!(Arc::ptr_eq(
            &parent_params.message_prefix,
            &child_params.message_prefix
        ));
    }

    #[test]
    fn clone_for_fork_deep_clones_mutable_state() {
        let mut parent = WorkingState::new();
        parent.set_cache_params(sample_cache_params());
        parent.push_task("shared context");
        parent.set_focus(Some("src/lib.rs".to_owned()), None, None);

        let mut child = parent.clone_for_fork();

        // NOTE: child starts with parent's task stack and focus
        assert_eq!(child.task_stack.len(), 1);
        assert!(child.focus.is_some());

        // WHY: mutations in child must not affect parent (state isolation)
        child.push_task("child-only work");
        child.set_focus(None, None, Some("child concept".to_owned()));

        assert_eq!(
            parent.task_stack.len(),
            1,
            "parent task stack must be unchanged"
        );
        assert_eq!(child.task_stack.len(), 2, "child should have its own task");
        assert_eq!(
            parent.focus.as_ref().unwrap().file.as_deref(),
            Some("src/lib.rs"),
            "parent focus must be unchanged"
        );
    }

    #[test]
    fn clone_for_fork_resets_wait_state() {
        let mut parent = WorkingState::new();
        parent.set_wait(WaitKind::SubAgent, "research agent");

        let child = parent.clone_for_fork();

        // WHY: child starts with no pending wait; parent's wait is independent
        assert!(child.wait.is_none(), "child should have no wait state");
        assert!(parent.wait.is_some(), "parent wait must be preserved");
    }

    #[test]
    fn clone_for_fork_sets_fresh_timestamp() {
        let mut parent = WorkingState::new();
        parent.push_task("work");
        let parent_ts = parent.updated_at.clone();

        // NOTE: timestamps use jiff::Timestamp::now() so they may be identical
        // if the fork happens within the same tick; we just verify it's set
        let child = parent.clone_for_fork();
        assert!(
            child.updated_at.is_some(),
            "child should have a fresh timestamp"
        );
        assert_eq!(
            parent.updated_at, parent_ts,
            "parent timestamp must not change"
        );
    }

    #[test]
    fn clone_for_fork_without_cache_params() {
        let mut parent = WorkingState::new();
        parent.push_task("work without cache params");

        let child = parent.clone_for_fork();

        assert!(
            child.cache_params.is_none(),
            "child should have no cache params when parent has none"
        );
        assert_eq!(child.task_stack.len(), 1);
    }

    #[test]
    fn shared_prefix_immutability() {
        let messages: Arc<[Message]> = Arc::from(vec![
            sample_message("context message 1"),
            sample_message("context message 2"),
        ]);
        let prefix_clone = Arc::clone(&messages);

        let params = Arc::new(CacheSafeParams::new(
            "system prompt",
            vec![sample_tool_definition("tool_a")],
            "model-v1",
            messages,
            None,
        ));

        // WHY: Arc<[Message]> is immutable once created; verify the shared
        // reference still points to the same data after params construction
        assert!(Arc::ptr_eq(&params.message_prefix, &prefix_clone));
        assert_eq!(params.message_prefix.len(), 2);
    }

    #[test]
    fn cache_params_serde_roundtrip_skips_field() {
        let mut state = WorkingState::new();
        state.push_task("test task");
        state.set_cache_params(sample_cache_params());

        let json = state.to_json().unwrap();
        let restored = WorkingState::from_json(&json).unwrap();

        // WHY: cache_params is #[serde(skip)] — runtime-only, not persisted
        assert!(
            restored.cache_params.is_none(),
            "cache_params should not survive serialization"
        );
        assert_eq!(restored.task_stack.len(), 1, "mutable state should persist");
    }
}
