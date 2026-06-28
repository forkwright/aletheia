#![expect(clippy::unwrap_used, reason = "test assertions may panic on failure")]
#![expect(
    clippy::indexing_slicing,
    reason = "test assertions use direct indexing for clarity"
)]

use super::*;
use crate::api::types::PlanStep;
use crate::app::test_helpers::*;

#[test]
fn turn_start_sets_state() {
    let mut app = test_app();
    app.dashboard.agents.push(test_agent("syn", "Syn"));
    app.dashboard.focused_agent = Some("syn".into());
    app.connection.streaming_text = "leftover".to_string();

    handle_stream_turn_start(&mut app, "t1".into(), "syn".into());

    assert!(app.connection.active_turn_id.as_ref().unwrap() == "t1");
    assert!(app.connection.streaming_text.is_empty());
    assert!(app.connection.streaming_thinking.is_empty());
    assert!(app.connection.streaming_tool_calls.is_empty());
    assert_eq!(app.dashboard.agents[0].status, AgentStatus::Streaming);
}

#[test]
fn text_delta_appends() {
    let mut app = test_app();
    // PERF: line buffering -- partial lines stay in streaming_line_buffer
    // until a newline flushes them to streaming_text.
    handle_stream_text_delta(&mut app, "hello ".to_string());
    handle_stream_text_delta(&mut app, "world\n".to_string());
    assert_eq!(app.connection.streaming_text, "hello world\n");
    assert!(app.connection.streaming_line_buffer.is_empty());
}

#[test]
fn thinking_delta_appends() {
    let mut app = test_app();
    handle_stream_thinking_delta(&mut app, "thinking...".to_string());
    assert_eq!(app.connection.streaming_thinking, "thinking...");
}

#[test]
fn tool_start_adds_tool_call() {
    let mut app = test_app();
    app.dashboard.agents.push(test_agent("syn", "Syn"));
    app.dashboard.focused_agent = Some("syn".into());

    handle_stream_tool_start(
        &mut app,
        "read_file".to_string(),
        ToolId::from("t1".to_string()),
        None,
    );

    assert_eq!(app.connection.streaming_tool_calls.len(), 1);
    assert_eq!(app.connection.streaming_tool_calls[0].name, "read_file");
    assert!(app.connection.streaming_tool_calls[0].duration_ms.is_none());
    assert_eq!(
        app.dashboard.agents[0]
            .active_tool
            .as_ref()
            .map(|t| t.name.as_str()),
        Some("read_file")
    );
}

#[test]
fn tool_result_updates_tool_call() {
    let mut app = test_app();
    app.dashboard.agents.push(test_agent("syn", "Syn"));
    app.dashboard.focused_agent = Some("syn".into());

    let tid = ToolId::from("t1".to_string());
    handle_stream_tool_start(&mut app, "read_file".to_string(), tid.clone(), None);
    handle_stream_tool_result(&mut app, "read_file".to_string(), tid, false, 150, None);

    assert_eq!(
        app.connection.streaming_tool_calls[0].duration_ms,
        Some(150)
    );
    assert!(!app.connection.streaming_tool_calls[0].is_error);
    assert!(app.dashboard.agents[0].active_tool.is_none());
}

#[test]
fn tool_result_error_flag() {
    let mut app = test_app();
    app.dashboard.agents.push(test_agent("syn", "Syn"));
    app.dashboard.focused_agent = Some("syn".into());

    let tid = ToolId::from("t2".to_string());
    handle_stream_tool_start(&mut app, "write_file".to_string(), tid.clone(), None);
    handle_stream_tool_result(&mut app, "write_file".to_string(), tid, true, 50, None);

    assert!(app.connection.streaming_tool_calls[0].is_error);
}

#[test]
fn tool_result_matches_duplicate_tool_names_by_id() {
    let mut app = test_app();

    let first_id = ToolId::from("tool-1".to_string());
    let second_id = ToolId::from("tool-2".to_string());
    handle_stream_tool_start(&mut app, "read_file".to_string(), first_id.clone(), None);
    handle_stream_tool_start(&mut app, "read_file".to_string(), second_id.clone(), None);

    handle_stream_tool_result(
        &mut app,
        "read_file".to_string(),
        first_id,
        false,
        125,
        Some("first result".to_string()),
    );

    assert_eq!(
        app.connection.streaming_tool_calls[0].duration_ms,
        Some(125)
    );
    assert_eq!(
        app.connection.streaming_tool_calls[0].output.as_deref(),
        Some("first result")
    );
    assert!(app.connection.streaming_tool_calls[1].duration_ms.is_none());
    assert_eq!(
        app.layout.ops.tool_calls[0].status,
        crate::state::ops::OpsToolStatus::Complete
    );
    assert_eq!(
        app.layout.ops.tool_calls[1].status,
        crate::state::ops::OpsToolStatus::Running
    );

    handle_stream_tool_result(
        &mut app,
        "read_file".to_string(),
        second_id,
        false,
        200,
        Some("second result".to_string()),
    );

    assert_eq!(
        app.connection.streaming_tool_calls[1].duration_ms,
        Some(200)
    );
    assert_eq!(
        app.connection.streaming_tool_calls[1].output.as_deref(),
        Some("second result")
    );
    assert_eq!(
        app.layout.ops.tool_calls[1].status,
        crate::state::ops::OpsToolStatus::Complete
    );
}

#[test]
fn tool_approval_opens_overlay() {
    let mut app = test_app();

    handle_stream_tool_approval_required(
        &mut app,
        super::StreamToolApprovalRequest {
            turn_id: "t1".into(),
            tool_name: "dangerous_tool".to_string(),
            tool_id: "tool1".into(),
            input: serde_json::json!({"path": "/etc/passwd"}),
            risk: "high".to_string(),
            reason: "writes to system files".to_string(),
            timeout_secs: 45,
            default_decision: "denied".to_string(),
        },
    );

    assert!(matches!(app.layout.overlay, Some(Overlay::ToolApproval(_))));
    if let Some(Overlay::ToolApproval(ref approval)) = app.layout.overlay {
        assert_eq!(approval.tool_name, "dangerous_tool");
        assert_eq!(approval.risk, "high");
        assert_eq!(approval.timeout_secs, 45);
    }
}

#[test]
fn tool_approval_resolved_closes_overlay() {
    let mut app = test_app();
    app.layout.overlay = Some(Overlay::ToolApproval(ToolApprovalOverlay {
        turn_id: "t1".into(),
        tool_id: "tool1".into(),
        tool_name: "test".to_string(),
        input: serde_json::Value::Null,
        risk: "low".to_string(),
        reason: "test".to_string(),
        timeout_secs: 120,
        default_decision: "denied".to_string(),
        status: crate::state::ControlMutationStatus::Idle,
    }));

    handle_stream_tool_approval_resolved(&mut app);
    assert!(app.layout.overlay.is_none());
}

#[test]
fn tool_approval_resolved_ignores_non_approval_overlay() {
    let mut app = test_app();
    app.layout.overlay = Some(Overlay::Help);

    handle_stream_tool_approval_resolved(&mut app);
    assert!(matches!(app.layout.overlay, Some(Overlay::Help)));
}

#[test]
fn plan_proposed_opens_overlay() {
    let mut app = test_app();
    let plan = Plan {
        id: "plan1".into(),
        session_id: "s1".into(),
        nous_id: "syn".into(),
        steps: vec![PlanStep {
            id: 1,
            label: "Step 1".to_string(),
            role: "analyst".to_string(),
            parallel: None,
            status: "pending".to_string(),
            result: None,
        }],
        total_estimated_cost_cents: 50,
        status: "proposed".to_string(),
    };

    handle_stream_plan_proposed(&mut app, plan);

    assert!(matches!(app.layout.overlay, Some(Overlay::PlanApproval(_))));
    if let Some(Overlay::PlanApproval(ref plan_overlay)) = app.layout.overlay {
        assert_eq!(plan_overlay.steps.len(), 1);
        assert!(plan_overlay.steps[0].checked);
        assert_eq!(plan_overlay.total_cost_cents, 50);
    }
}

#[test]
fn plan_step_start_adds_ops_entry() {
    let mut app = test_app();
    handle_stream_plan_step_start(&mut app, 1);
    assert_eq!(app.layout.ops.tool_calls.len(), 1);
    assert_eq!(app.layout.ops.tool_calls[0].name, "plan step 1");
    assert_eq!(
        app.layout.ops.tool_calls[0].status,
        crate::state::ops::OpsToolStatus::Running
    );
}

#[test]
fn plan_step_complete_marks_done() {
    let mut app = test_app();
    handle_stream_plan_step_start(&mut app, 2);
    handle_stream_plan_step_complete(&mut app, 2, "done".to_string());
    assert_eq!(
        app.layout.ops.tool_calls[0].status,
        crate::state::ops::OpsToolStatus::Complete
    );
}

#[test]
fn plan_step_complete_marks_failed_on_error_status() {
    let mut app = test_app();
    handle_stream_plan_step_start(&mut app, 3);
    handle_stream_plan_step_complete(&mut app, 3, "failed".to_string());
    assert_eq!(
        app.layout.ops.tool_calls[0].status,
        crate::state::ops::OpsToolStatus::Failed
    );
}

#[test]
fn plan_complete_adds_completed_ops_entry() {
    let mut app = test_app();
    handle_stream_plan_complete(&mut app, "done".to_string());
    assert_eq!(app.layout.ops.tool_calls.len(), 1);
    assert_eq!(app.layout.ops.tool_calls[0].name, "plan: done");
    assert_eq!(
        app.layout.ops.tool_calls[0].status,
        crate::state::ops::OpsToolStatus::Complete
    );
}

#[test]
fn turn_abort_preserves_partial_text_with_terminal_reason() {
    let mut app = test_app();
    app.dashboard.agents.push(test_agent("syn", "Syn"));
    app.dashboard.focused_agent = Some("syn".into());
    app.connection.active_turn_id = Some("t1".into());
    app.connection.streaming_text = "partial".to_string();
    app.connection.streaming_tool_calls.push(ToolCallInfo {
        name: "read_file".to_string(),
        duration_ms: None,
        is_error: false,
        tool_id: None,
        output: None,
    });
    app.dashboard.agents[0].status = AgentStatus::Working;

    handle_stream_turn_abort(&mut app, "user cancelled".to_string());

    assert_eq!(app.dashboard.messages.len(), 1);
    assert_eq!(
        app.dashboard.messages[0].text,
        "partial\n\n[turn aborted: user cancelled]"
    );
    assert_eq!(app.dashboard.messages[0].tool_calls.len(), 1);
    assert!(app.connection.active_turn_id.is_none());
    assert!(app.connection.streaming_text.is_empty());
    assert!(app.connection.streaming_tool_calls.is_empty());
    assert!(app.connection.stream_rx.is_none());
    assert_eq!(app.dashboard.agents[0].status, AgentStatus::Idle);
}

#[test]
fn turn_abort_without_text_commits_terminal_record() {
    let mut app = test_app();
    app.dashboard.agents.push(test_agent("syn", "Syn"));
    app.dashboard.focused_agent = Some("syn".into());
    app.connection.active_turn_id = Some("t1".into());

    handle_stream_turn_abort(&mut app, "timeout".to_string());

    assert_eq!(app.dashboard.messages.len(), 1);
    assert_eq!(app.dashboard.messages[0].text, "[turn aborted: timeout]");
    assert!(app.connection.active_turn_id.is_none());
    assert!(app.connection.streaming_text.is_empty());
}

#[test]
fn stream_error_shows_toast() {
    let mut app = test_app();
    app.connection.active_turn_id = Some("t1".into());

    handle_stream_error(&mut app, "connection lost".to_string());

    assert!(app.viewport.error_toast.is_some());
    assert_eq!(
        app.viewport.error_toast.as_ref().unwrap().message,
        "connection lost"
    );
    assert!(app.connection.active_turn_id.is_none());
}

#[test]
fn stream_error_clears_tool_calls_and_resets_agent() {
    let mut app = test_app();
    app.dashboard.agents.push(test_agent("syn", "Syn"));
    app.dashboard.focused_agent = Some("syn".into());
    app.connection.active_turn_id = Some("t1".into());
    app.connection.streaming_text = "partial response".to_string();
    app.connection.streaming_tool_calls.push(ToolCallInfo {
        name: "grep".to_string(),
        duration_ms: None,
        is_error: false,
        tool_id: None,
        output: None,
    });
    app.dashboard.agents[0].status = AgentStatus::Working;
    app.dashboard.agents[0].active_tool = Some(ActiveTool {
        name: "grep".to_string(),
        started_at: std::time::Instant::now(),
    });

    handle_stream_error(&mut app, "connection reset".to_string());

    // Partial text preserved for user inspection
    assert_eq!(app.connection.streaming_text, "partial response");
    // Tool calls cleared so no stale spinners
    assert!(app.connection.streaming_tool_calls.is_empty());
    assert_eq!(app.dashboard.agents[0].status, AgentStatus::Idle);
    assert!(app.dashboard.agents[0].active_tool.is_none());
    assert!(app.viewport.error_toast.is_some());
}

#[test]
fn text_delta_defers_markdown_cache() {
    let mut app = test_app();
    handle_stream_text_delta(&mut app, "hello\n".to_string());
    // PERF: markdown cache is no longer updated per-delta; it is refreshed
    // once per frame in App::refresh_streaming_markdown_cache.
    assert!(
        app.viewport.render.markdown_cache.text.is_empty(),
        "cache must not update on delta (deferred to frame boundary)"
    );
    assert_eq!(app.connection.streaming_text, "hello\n");
}

#[test]
fn refresh_markdown_cache_updates_after_delta() {
    let mut app = test_app();
    app.viewport.terminal_width = 80;
    // PERF: "hello\n" flushes to streaming_text; "world\n" completes the second line.
    handle_stream_text_delta(&mut app, "hello\nworld\n".to_string());
    app.refresh_streaming_markdown_cache();
    assert_eq!(app.viewport.render.markdown_cache.text, "hello\nworld\n");
    assert!(!app.viewport.render.markdown_cache.lines.is_empty());
    // Width should be terminal_width - 4 (matching the view's inner_width - 2)
    assert_eq!(app.viewport.render.markdown_cache.width, 76);
}

fn make_outcome() -> TurnOutcome {
    TurnOutcome {
        text: String::new(),
        nous_id: "syn".into(),
        session_id: "s1".into(),
        model: Some("claude".to_string()),
        tool_calls: 0,
        input_tokens: 0,
        output_tokens: 0,
        cache_read_tokens: 0,
        cache_write_tokens: 0,
        stop_reason: None,
        error: None,
    }
}

#[tokio::test]
async fn turn_complete_auto_scroll_stays_at_bottom() {
    let mut app = test_app();
    app.dashboard.agents.push(test_agent("syn", "Syn"));
    app.dashboard.focused_agent = Some("syn".into());
    app.viewport.render.auto_scroll = true;
    app.viewport.render.scroll_offset = 0;
    app.connection.streaming_text = "hello world".to_string();
    handle_stream_turn_complete(&mut app, make_outcome()).await;
    assert!(app.viewport.render.auto_scroll);
    assert_eq!(app.viewport.render.scroll_offset, 0);
}

#[tokio::test]
async fn turn_complete_commits_outcome_text_without_deltas() {
    let mut app = test_app();
    app.dashboard.agents.push(test_agent("syn", "Syn"));
    app.dashboard.focused_agent = Some("syn".into());
    let mut outcome = make_outcome();
    outcome.text = "authoritative final text".to_string();

    handle_stream_turn_complete(&mut app, outcome).await;

    assert_eq!(app.dashboard.messages.len(), 1);
    assert_eq!(app.dashboard.messages[0].text, "authoritative final text");
}

#[tokio::test]
async fn turn_complete_replaces_incomplete_delta_buffer() {
    let mut app = test_app();
    app.dashboard.agents.push(test_agent("syn", "Syn"));
    app.dashboard.focused_agent = Some("syn".into());
    app.connection.streaming_text = "partial".to_string();
    let mut outcome = make_outcome();
    outcome.text = "partial plus final token".to_string();

    handle_stream_turn_complete(&mut app, outcome).await;

    assert_eq!(app.dashboard.messages.len(), 1);
    assert_eq!(app.dashboard.messages[0].text, "partial plus final token");
}

#[tokio::test]
async fn turn_complete_error_with_no_text_commits_terminal_record() {
    let mut app = test_app();
    app.dashboard.agents.push(test_agent("syn", "Syn"));
    app.dashboard.focused_agent = Some("syn".into());
    let mut outcome = make_outcome();
    outcome.model = Some("claude-opus-4-6".to_string());
    outcome.input_tokens = 11;
    outcome.output_tokens = 7;
    outcome.stop_reason = Some("error".to_string());
    outcome.error = Some("provider unavailable".to_string());

    handle_stream_turn_complete(&mut app, outcome).await;

    assert_eq!(app.dashboard.messages.len(), 1);
    assert_eq!(
        app.dashboard.messages[0].text,
        "[turn failed: error: provider unavailable]"
    );
    assert_eq!(
        app.dashboard.messages[0].model.as_deref(),
        Some("claude-opus-4-6")
    );
    assert_eq!(app.layout.metrics.total_input_tokens, 11);
    assert_eq!(app.layout.metrics.total_output_tokens, 7);
}

#[tokio::test]
async fn turn_complete_scroll_lock_preserves_offset() {
    let mut app = test_app();
    app.dashboard.agents.push(test_agent("syn", "Syn"));
    app.dashboard.focused_agent = Some("syn".into());
    app.viewport.render.auto_scroll = false;
    app.viewport.render.scroll_offset = 30;
    app.rebuild_virtual_scroll();
    app.connection.streaming_text = "a new message with some text".to_string();
    let offset_before = app.viewport.render.scroll_offset;
    handle_stream_turn_complete(&mut app, make_outcome()).await;
    // Offset must increase so the viewport stays anchored while new content lands below.
    assert!(!app.viewport.render.auto_scroll);
    assert!(
        app.viewport.render.scroll_offset > offset_before,
        "scroll_offset should increase when new message arrives while scrolled up"
    );
}

#[tokio::test]
async fn turn_complete_no_text_does_not_change_scroll() {
    let mut app = test_app();
    app.dashboard.agents.push(test_agent("syn", "Syn"));
    app.dashboard.focused_agent = Some("syn".into());
    app.viewport.render.auto_scroll = false;
    app.viewport.render.scroll_offset = 10;
    // streaming_text is empty: no message is committed, offset unchanged
    handle_stream_turn_complete(&mut app, make_outcome()).await;
    assert_eq!(app.viewport.render.scroll_offset, 10);
    assert!(!app.viewport.render.auto_scroll);
}

#[tokio::test]
async fn turn_complete_cross_agent_does_not_pollute_focused_agent() {
    // WHY: If the user switches agents while a turn is streaming, the completing
    // turn belongs to the old agent.  Its message must not be pushed to the new
    // focused agent's message buffer.
    let mut app = test_app();
    app.dashboard.agents.push(test_agent("alpha", "Alpha"));
    app.dashboard.agents.push(test_agent("beta", "Beta"));
    // User has switched to "beta", but the completing outcome is for "alpha".
    app.dashboard.focused_agent = Some("beta".into());
    app.connection.streaming_text = "alpha's response".to_string();
    let mut outcome = make_outcome();
    outcome.nous_id = "alpha".into();

    handle_stream_turn_complete(&mut app, outcome).await;

    // Message must NOT appear in the (now-beta-focused) buffer.
    assert!(
        app.dashboard.messages.is_empty(),
        "cross-agent turn completion must not push to focused agent's buffer"
    );
    // Streaming state must still be cleared.
    assert!(app.connection.streaming_text.is_empty());
    assert!(app.connection.active_turn_id.is_none());
}
