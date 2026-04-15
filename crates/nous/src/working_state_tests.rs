#![expect(clippy::unwrap_used, reason = "test assertions")]
#![expect(
    clippy::indexing_slicing,
    reason = "test assertions on known-length slices"
)]

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
        vec![
            sample_tool_definition("zeta_tool"),
            sample_tool_definition("alpha_tool"),
        ],
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
