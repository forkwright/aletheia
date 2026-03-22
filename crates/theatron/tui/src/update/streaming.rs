use tracing::Instrument;

use crate::api::types::{Plan, TurnOutcome};
use crate::app::App;
use crate::id::{NousId, ToolId, TurnId};
use crate::msg::ErrorToast;
use crate::sanitize::sanitize_for_display;
use crate::state::virtual_scroll::estimate_message_height;
use crate::state::{
    ActiveTool, AgentStatus, ChatMessage, Overlay, PlanApprovalOverlay, PlanStepApproval,
    ToolApprovalOverlay, ToolCallInfo,
};

/// Context window size in tokens for the given model.
/// All current Claude models use 200K context.
fn model_context_window(_model: &str) -> u32 {
    200_000
}

#[tracing::instrument(skip_all, fields(%turn_id, %nous_id))]
pub(crate) fn handle_stream_turn_start(app: &mut App, turn_id: TurnId, nous_id: NousId) {
    app.connection.active_turn_id = Some(turn_id);
    app.connection.streaming_text.clear();
    app.connection.streaming_thinking.clear();
    app.connection.streaming_tool_calls.clear();
    app.connection.stream_last_event_at = Some(std::time::Instant::now());
    app.connection.stall_warned = false;
    app.connection.stall_message = None;
    app.viewport.render.markdown_cache.clear();
    if let Some(agent) = app.dashboard.agents.iter_mut().find(|a| a.id == nous_id) {
        agent.status = AgentStatus::Streaming;
    }
    app.layout.ops.clear_turn();
    app.layout.ops.auto_show_if_configured();
}

#[tracing::instrument(skip_all, fields(len = text.len()))]
// SAFETY: sanitized at ingestion: streaming text from LLM API.
pub(crate) fn handle_stream_text_delta(app: &mut App, text: String) {
    let clean = sanitize_for_display(&text);
    app.connection.streaming_text.push_str(&clean);
    // PERF: Markdown re-rendering is deferred to the frame boundary (app.view)
    // so that multiple text deltas arriving between frames are batched into a
    // single markdown::render call. At 100 tokens/sec with 60fps rendering,
    // this reduces markdown parses from ~100/sec to at most 60/sec.
    if app.viewport.render.auto_scroll {
        app.viewport.render.scroll_offset = 0;
    }
}

#[tracing::instrument(skip_all, fields(len = text.len()))]
// SAFETY: sanitized at ingestion: thinking text from LLM API.
pub(crate) fn handle_stream_thinking_delta(app: &mut App, text: String) {
    let clean = sanitize_for_display(&text);
    app.connection.streaming_thinking.push_str(&clean);
    app.layout.ops.push_thinking(&clean);
}

#[tracing::instrument(skip_all, fields(%tool_name))]
// SAFETY: sanitized at ingestion: tool names from stream API.
pub(crate) fn handle_stream_tool_start(
    app: &mut App,
    tool_name: String,
    input: Option<serde_json::Value>,
) {
    let clean_name = sanitize_for_display(&tool_name).into_owned();
    app.connection.streaming_tool_calls.push(ToolCallInfo {
        name: clean_name.clone(),
        duration_ms: None,
        is_error: false,
    });
    let input_json = input.map(|v| v.to_string());
    app.layout
        .ops
        .push_tool_start(clean_name.clone(), input_json);
    if let Some(ref agent_id) = app.dashboard.focused_agent
        && let Some(agent) = app.dashboard.agents.iter_mut().find(|a| a.id == *agent_id)
    {
        agent.active_tool = Some(ActiveTool {
            name: clean_name,
            started_at: std::time::Instant::now(),
        });
    }
}

#[tracing::instrument(skip_all, fields(%tool_name, is_error, duration_ms))]
pub(crate) fn handle_stream_tool_result(
    app: &mut App,
    tool_name: String,
    is_error: bool,
    duration_ms: u64,
    result: Option<String>,
) {
    if let Some(tc) = app
        .connection
        .streaming_tool_calls
        .iter_mut()
        .rev()
        .find(|t| t.name == tool_name)
    {
        tc.duration_ms = Some(duration_ms);
        tc.is_error = is_error;
    }
    app.layout
        .ops
        .complete_tool(&tool_name, is_error, duration_ms, result);
    if let Some(ref agent_id) = app.dashboard.focused_agent
        && let Some(agent) = app.dashboard.agents.iter_mut().find(|a| a.id == *agent_id)
    {
        agent.active_tool = None;
    }
}

#[tracing::instrument(skip_all, fields(%tool_name, %risk))]
// SAFETY: sanitized at ingestion: tool approval data from stream API.
pub(crate) fn handle_stream_tool_approval_required(
    app: &mut App,
    turn_id: TurnId,
    tool_name: String,
    tool_id: ToolId,
    input: serde_json::Value,
    risk: String,
    reason: String,
) {
    // WHY: If the user previously chose "always allow" for this tool, auto-approve
    // without presenting the dialog again.
    if app
        .interaction
        .always_allowed_tools
        .contains(tool_name.as_str())
    {
        let client = app.client.clone();
        let span = tracing::info_span!("auto_approve_tool", %turn_id, %tool_id, %tool_name);
        tokio::spawn(
            // kanon:ignore RUST/spawn-no-instrument
            async move {
                if let Err(e) = client.approve_tool(&turn_id, &tool_id).await {
                    tracing::error!("failed to auto-approve tool: {e}");
                }
            }
            .instrument(span),
        );
        return;
    }

    app.layout.overlay = Some(Overlay::ToolApproval(ToolApprovalOverlay {
        turn_id,
        tool_id,
        tool_name: sanitize_for_display(&tool_name).into_owned(),
        input,
        risk: sanitize_for_display(&risk).into_owned(),
        reason: sanitize_for_display(&reason).into_owned(),
    }));
}

#[tracing::instrument(skip_all)]
pub(crate) fn handle_stream_tool_approval_resolved(app: &mut App) {
    if app.is_tool_approval_overlay() {
        app.layout.overlay = None;
    }
}

#[tracing::instrument(skip_all, fields(step_id))]
pub(crate) fn handle_stream_plan_step_start(app: &mut App, step_id: u32) {
    app.layout
        .ops
        .push_tool_start(format!("plan step {step_id}"), None);
}

#[tracing::instrument(skip_all, fields(step_id, %status))]
pub(crate) fn handle_stream_plan_step_complete(app: &mut App, step_id: u32, status: String) {
    let name = format!("plan step {step_id}");
    let is_error = matches!(status.as_str(), "failed" | "error");
    app.layout.ops.complete_tool(&name, is_error, 0, None);
}

#[tracing::instrument(skip_all, fields(%status))]
pub(crate) fn handle_stream_plan_complete(app: &mut App, status: String) {
    let is_error = matches!(status.as_str(), "failed" | "error");
    let label = format!("plan: {status}");
    app.layout.ops.push_tool_start(label.clone(), None);
    app.layout.ops.complete_tool(&label, is_error, 0, None);
}

#[tracing::instrument(skip_all)]
// SAFETY: sanitized at ingestion: plan step labels and roles from stream API.
pub(crate) fn handle_stream_plan_proposed(app: &mut App, plan: Plan) {
    app.layout.overlay = Some(Overlay::PlanApproval(PlanApprovalOverlay {
        plan_id: plan.id,
        total_cost_cents: plan.total_estimated_cost_cents,
        cursor: 0,
        steps: plan
            .steps
            .into_iter()
            .map(|s| PlanStepApproval {
                id: s.id,
                label: sanitize_for_display(&s.label).into_owned(),
                role: sanitize_for_display(&s.role).into_owned(),
                checked: true,
            })
            .collect(),
    }));
}

#[tracing::instrument(skip_all)]
// SAFETY: sanitized at ingestion: streaming_text already sanitized via handle_stream_text_delta,
// model name from API is sanitized here.
pub(crate) async fn handle_stream_turn_complete(app: &mut App, outcome: TurnOutcome) {
    // WHY: Only commit the streamed message when the completing turn belongs to the
    // currently focused agent.  If the user switched agents mid-stream the message
    // belongs to the old agent's session; pushing it here would corrupt the new
    // agent's in-memory message buffer.  The server has already persisted the
    // message, so it will appear when the user navigates back via load_focused_session.
    let belongs_to_focused = app
        .dashboard
        .focused_agent
        .as_ref()
        .is_some_and(|id| *id == outcome.nous_id);

    if belongs_to_focused && !app.connection.streaming_text.is_empty() {
        let text = app.connection.streaming_text.clone();
        let text_lower = text.to_lowercase();
        let tool_calls = std::mem::take(&mut app.connection.streaming_tool_calls);
        let width = app
            .viewport
            .render
            .virtual_scroll
            .cached_width()
            .max(app.viewport.terminal_width.saturating_sub(2).max(1));
        let h = estimate_message_height(text.len(), width);
        app.dashboard.messages.push(ChatMessage {
            role: "assistant".to_string(),
            text,
            text_lower,
            timestamp: None,
            model: Some(sanitize_for_display(&outcome.model).into_owned()),
            tool_calls,
        });
        app.viewport.render.virtual_scroll.push_item(h);
        // WHY: Keep the viewport anchored when scrolled up by compensating the
        // scroll offset for the new content added below the current position.
        if !app.viewport.render.auto_scroll {
            app.viewport.render.scroll_offset = app
                .viewport
                .render
                .scroll_offset
                .saturating_add(usize::from(h));
        }
    }
    app.connection.streaming_text.clear();
    app.connection.streaming_thinking.clear();
    app.connection.streaming_tool_calls.clear();
    app.connection.stream_last_event_at = None;
    app.connection.stall_warned = false;
    app.connection.stall_message = None;
    app.viewport.render.markdown_cache.clear();
    app.connection.active_turn_id = None;
    app.connection.stream_rx = None;
    if let Some(ref agent_id) = app.dashboard.focused_agent
        && let Some(agent) = app.dashboard.agents.iter_mut().find(|a| a.id == *agent_id)
    {
        agent.status = AgentStatus::Idle;
        agent.active_tool = None;
    }
    app.layout.ops.auto_hide_if_configured();
    let ctx_used = outcome
        .input_tokens
        .saturating_add(outcome.cache_read_tokens);
    if ctx_used > 0 {
        let ctx_total = model_context_window(&outcome.model);
        app.dashboard.context_tokens_used = Some(ctx_used);
        app.dashboard.context_tokens_total = Some(ctx_total);
        #[allow(clippy::cast_possible_truncation)]
        let pct = ((u64::from(ctx_used) * 100) / u64::from(ctx_total)).min(100) as u8;
        app.dashboard.context_usage_pct = Some(pct);
    }
    if let Ok(cents) = app.client.today_cost_cents().await {
        app.dashboard.daily_cost_cents = cents;
    }
}

#[tracing::instrument(skip_all)]
pub(crate) fn handle_stream_turn_abort(app: &mut App, reason: String) {
    tracing::info!("turn aborted: {reason}");
    app.connection.streaming_text.clear();
    app.connection.streaming_thinking.clear();
    app.connection.streaming_tool_calls.clear();
    app.connection.stream_last_event_at = None;
    app.connection.stall_warned = false;
    app.connection.stall_message = None;
    app.connection.active_turn_id = None;
    app.connection.stream_rx = None;
    if let Some(ref agent_id) = app.dashboard.focused_agent
        && let Some(agent) = app.dashboard.agents.iter_mut().find(|a| a.id == *agent_id)
    {
        agent.status = AgentStatus::Idle;
        agent.active_tool = None;
    }
}

#[tracing::instrument(skip_all)]
// SAFETY: sanitized at ingestion: error messages may contain external data.
pub(crate) fn handle_stream_error(app: &mut App, msg: String) {
    tracing::error!("stream error: {msg}");
    app.viewport.error_toast = Some(ErrorToast::new(sanitize_for_display(&msg).into_owned()));
    app.connection.active_turn_id = None;
    app.connection.stream_rx = None;
    app.connection.stream_last_event_at = None;
    app.connection.stall_warned = false;
    app.connection.stall_message = None;
    // WHY: Clear tool calls to remove stale spinners; preserve streaming_text
    // so the user can read any partial response received before the error.
    app.connection.streaming_tool_calls.clear();
    if let Some(ref agent_id) = app.dashboard.focused_agent
        && let Some(agent) = app.dashboard.agents.iter_mut().find(|a| a.id == *agent_id)
    {
        agent.status = AgentStatus::Idle;
        agent.active_tool = None;
    }
}

#[tracing::instrument(skip_all)]
// SAFETY: sanitized at ingestion: streaming_text already sanitized via handle_stream_text_delta.
pub(crate) async fn handle_cancel_turn(app: &mut App) {
    let turn_id = match app.connection.active_turn_id.take() {
        Some(id) => id,
        None => return,
    };

    // Fire-and-forget: tell the server to abort. Errors are non-fatal; the
    // stream receiver being dropped is sufficient to stop local processing.
    let client = app.client.clone();
    let id = turn_id.to_string();
    tokio::spawn(async move {
        if let Err(e) = client.abort_turn(&id).await {
            tracing::warn!(error = %e, "abort_turn request failed");
        }
    });

    // Commit partial streaming text as an incomplete turn marker.
    let partial = std::mem::take(&mut app.connection.streaming_text);
    let marker = if partial.is_empty() {
        "[interrupted by user]".to_string()
    } else {
        format!("{partial}\n\n[interrupted by user]")
    };
    let marker_lower = marker.to_lowercase();
    let tool_calls = std::mem::take(&mut app.connection.streaming_tool_calls);
    let width = app
        .viewport
        .render
        .virtual_scroll
        .cached_width()
        .max(app.viewport.terminal_width.saturating_sub(2).max(1));
    let h = estimate_message_height(marker.len(), width);
    app.dashboard.messages.push(ChatMessage {
        role: "assistant".to_string(),
        text: marker,
        text_lower: marker_lower,
        timestamp: None,
        model: None,
        tool_calls,
    });
    app.viewport.render.virtual_scroll.push_item(h);

    app.connection.streaming_thinking.clear();
    app.connection.stream_rx = None;
    app.connection.stream_last_event_at = None;
    app.connection.stall_warned = false;
    app.connection.stall_message = None;
    app.viewport.render.markdown_cache.clear();

    if let Some(ref agent_id) = app.dashboard.focused_agent
        && let Some(agent) = app.dashboard.agents.iter_mut().find(|a| a.id == *agent_id)
    {
        agent.status = AgentStatus::Idle;
        agent.active_tool = None;
    }
    app.layout.ops.auto_hide_if_configured();
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions may panic on failure")]
#[expect(
    clippy::indexing_slicing,
    reason = "test assertions use direct indexing for clarity"
)]
mod tests {
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
        handle_stream_text_delta(&mut app, "hello ".to_string());
        handle_stream_text_delta(&mut app, "world".to_string());
        assert_eq!(app.connection.streaming_text, "hello world");
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

        handle_stream_tool_start(&mut app, "read_file".to_string(), None);

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

        handle_stream_tool_start(&mut app, "read_file".to_string(), None);
        handle_stream_tool_result(&mut app, "read_file".to_string(), false, 150, None);

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

        handle_stream_tool_start(&mut app, "write_file".to_string(), None);
        handle_stream_tool_result(&mut app, "write_file".to_string(), true, 50, None);

        assert!(app.connection.streaming_tool_calls[0].is_error);
    }

    #[test]
    fn tool_approval_opens_overlay() {
        let mut app = test_app();

        handle_stream_tool_approval_required(
            &mut app,
            "t1".into(),
            "dangerous_tool".to_string(),
            "tool1".into(),
            serde_json::json!({"path": "/etc/passwd"}),
            "high".to_string(),
            "writes to system files".to_string(),
        );

        assert!(matches!(app.layout.overlay, Some(Overlay::ToolApproval(_))));
        if let Some(Overlay::ToolApproval(ref approval)) = app.layout.overlay {
            assert_eq!(approval.tool_name, "dangerous_tool");
            assert_eq!(approval.risk, "high");
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
    fn turn_abort_clears_state() {
        let mut app = test_app();
        app.dashboard.agents.push(test_agent("syn", "Syn"));
        app.dashboard.focused_agent = Some("syn".into());
        app.connection.active_turn_id = Some("t1".into());
        app.connection.streaming_text = "partial".to_string();
        app.connection.streaming_tool_calls.push(ToolCallInfo {
            name: "read_file".to_string(),
            duration_ms: None,
            is_error: false,
        });
        app.dashboard.agents[0].status = AgentStatus::Working;

        handle_stream_turn_abort(&mut app, "user cancelled".to_string());

        assert!(app.connection.active_turn_id.is_none());
        assert!(app.connection.streaming_text.is_empty());
        assert!(app.connection.streaming_tool_calls.is_empty());
        assert!(app.connection.stream_rx.is_none());
        assert_eq!(app.dashboard.agents[0].status, AgentStatus::Idle);
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
        // Agent back to idle
        assert_eq!(app.dashboard.agents[0].status, AgentStatus::Idle);
        assert!(app.dashboard.agents[0].active_tool.is_none());
        // Error toast displayed
        assert!(app.viewport.error_toast.is_some());
    }

    #[test]
    fn text_delta_defers_markdown_cache() {
        let mut app = test_app();
        handle_stream_text_delta(&mut app, "hello".to_string());
        // PERF: markdown cache is no longer updated per-delta; it is refreshed
        // once per frame in App::refresh_streaming_markdown_cache.
        assert!(
            app.viewport.render.markdown_cache.text.is_empty(),
            "cache must not update on delta (deferred to frame boundary)"
        );
        assert_eq!(app.connection.streaming_text, "hello");
    }

    #[test]
    fn refresh_markdown_cache_updates_after_delta() {
        let mut app = test_app();
        app.viewport.terminal_width = 80;
        handle_stream_text_delta(&mut app, "hello\nworld".to_string());
        app.refresh_streaming_markdown_cache();
        assert_eq!(app.viewport.render.markdown_cache.text, "hello\nworld");
        assert!(!app.viewport.render.markdown_cache.lines.is_empty());
        // Width should be terminal_width - 4 (matching the view's inner_width - 2)
        assert_eq!(app.viewport.render.markdown_cache.width, 76);
    }

    fn make_outcome() -> TurnOutcome {
        TurnOutcome {
            text: String::new(),
            nous_id: "syn".into(),
            session_id: "s1".into(),
            model: "claude".to_string(),
            tool_calls: 0,
            input_tokens: 0,
            output_tokens: 0,
            cache_read_tokens: 0,
            cache_write_tokens: 0,
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
}
