use crate::api::types::{Plan, TurnOutcome};
use crate::app::App;
use crate::id::{NousId, ToolId, TurnId};
use crate::msg::ErrorToast;
use crate::sanitize::sanitize_for_display;
use crate::state::virtual_scroll::estimate_message_height;
use crate::state::{
    AgentStatus, ChatMessage, Overlay, PlanApprovalOverlay, PlanStepApproval, ToolApprovalOverlay,
    ToolCallInfo,
};

#[tracing::instrument(skip_all, fields(%turn_id, %nous_id))]
pub(crate) fn handle_stream_turn_start(app: &mut App, turn_id: TurnId, nous_id: NousId) {
    app.active_turn_id = Some(turn_id);
    app.streaming_text.clear();
    app.streaming_thinking.clear();
    app.streaming_tool_calls.clear();
    app.cached_markdown_text.clear();
    app.cached_markdown_lines.clear();
    if let Some(agent) = app.agents.iter_mut().find(|a| a.id == nous_id) {
        agent.status = AgentStatus::Streaming;
    }
    // Operations pane: clear previous turn data and auto-show
    app.ops.clear_turn();
    app.ops.auto_show_if_configured();
}

#[tracing::instrument(skip_all, fields(len = text.len()))]
// SAFETY: sanitized at ingestion — streaming text from LLM API.
pub(crate) fn handle_stream_text_delta(app: &mut App, text: String) {
    let clean = sanitize_for_display(&text);
    app.streaming_text.push_str(&clean);
    let delta = app.streaming_text.len() as i64 - app.cached_markdown_text.len() as i64;
    if delta >= 64 || text.contains('\n') {
        let width = 120;
        app.cached_markdown_lines =
            crate::markdown::render(&app.streaming_text, width, &app.theme, &app.highlighter).0;
        app.cached_markdown_text = app.streaming_text.clone();
    }
    if app.auto_scroll {
        app.scroll_offset = 0;
    }
}

#[tracing::instrument(skip_all, fields(len = text.len()))]
// SAFETY: sanitized at ingestion — thinking text from LLM API.
pub(crate) fn handle_stream_thinking_delta(app: &mut App, text: String) {
    let clean = sanitize_for_display(&text);
    app.streaming_thinking.push_str(&clean);
    app.ops.push_thinking(&clean);
}

#[tracing::instrument(skip_all, fields(%tool_name))]
// SAFETY: sanitized at ingestion — tool names from stream API.
pub(crate) fn handle_stream_tool_start(app: &mut App, tool_name: String) {
    let clean_name = sanitize_for_display(&tool_name).into_owned();
    app.streaming_tool_calls.push(ToolCallInfo {
        name: clean_name.clone(),
        duration_ms: None,
        is_error: false,
    });
    // Operations pane: add tool call entry
    app.ops.push_tool_start(clean_name.clone(), None);
    if let Some(ref agent_id) = app.focused_agent {
        if let Some(agent) = app.agents.iter_mut().find(|a| a.id == *agent_id) {
            agent.active_tool = Some(clean_name);
            agent.tool_started_at = Some(std::time::Instant::now());
        }
    }
}

#[tracing::instrument(skip_all, fields(%tool_name, is_error, duration_ms))]
pub(crate) fn handle_stream_tool_result(
    app: &mut App,
    tool_name: String,
    is_error: bool,
    duration_ms: u64,
) {
    if let Some(tc) = app
        .streaming_tool_calls
        .iter_mut()
        .rev()
        .find(|t| t.name == tool_name)
    {
        tc.duration_ms = Some(duration_ms);
        tc.is_error = is_error;
    }
    // Operations pane: complete tool call
    app.ops
        .complete_tool(&tool_name, is_error, duration_ms, None);
    if let Some(ref agent_id) = app.focused_agent {
        if let Some(agent) = app.agents.iter_mut().find(|a| a.id == *agent_id) {
            agent.active_tool = None;
            agent.tool_started_at = None;
        }
    }
}

#[tracing::instrument(skip_all, fields(%tool_name, %risk))]
// SAFETY: sanitized at ingestion — tool approval data from stream API.
pub(crate) fn handle_stream_tool_approval_required(
    app: &mut App,
    turn_id: TurnId,
    tool_name: String,
    tool_id: ToolId,
    input: serde_json::Value,
    risk: String,
    reason: String,
) {
    app.overlay = Some(Overlay::ToolApproval(ToolApprovalOverlay {
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
        app.overlay = None;
    }
}

#[tracing::instrument(skip_all)]
// SAFETY: sanitized at ingestion — plan step labels and roles from stream API.
pub(crate) fn handle_stream_plan_proposed(app: &mut App, plan: Plan) {
    app.overlay = Some(Overlay::PlanApproval(PlanApprovalOverlay {
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
// SAFETY: sanitized at ingestion — streaming_text already sanitized via handle_stream_text_delta,
// model name from API is sanitized here.
pub(crate) async fn handle_stream_turn_complete(app: &mut App, outcome: TurnOutcome) {
    if !app.streaming_text.is_empty() {
        let text = app.streaming_text.clone();
        let text_lower = text.to_lowercase();
        let tool_calls = std::mem::take(&mut app.streaming_tool_calls);
        let has_tools = !tool_calls.is_empty();
        let width = app
            .virtual_scroll
            .cached_width()
            .max(app.terminal_width.saturating_sub(2).max(1));
        let h = estimate_message_height(text.len(), has_tools, width);
        app.messages.push(ChatMessage {
            role: "assistant".to_string(),
            text,
            text_lower,
            timestamp: None,
            model: Some(sanitize_for_display(&outcome.model).into_owned()),
            is_streaming: false,
            tool_calls,
        });
        app.virtual_scroll.push_item(h);
    }
    app.streaming_text.clear();
    app.streaming_thinking.clear();
    app.streaming_tool_calls.clear();
    app.cached_markdown_text.clear();
    app.cached_markdown_lines.clear();
    app.active_turn_id = None;
    app.stream_rx = None;
    if let Some(ref agent_id) = app.focused_agent {
        if let Some(agent) = app.agents.iter_mut().find(|a| a.id == *agent_id) {
            agent.status = AgentStatus::Idle;
            agent.active_tool = None;
        }
    }
    // Operations pane: auto-hide when turn completes
    app.ops.auto_hide_if_configured();
    if let Ok(cents) = app.client.today_cost_cents().await {
        app.daily_cost_cents = cents;
    }
}

#[tracing::instrument(skip_all)]
pub(crate) fn handle_stream_turn_abort(app: &mut App, reason: String) {
    tracing::info!("turn aborted: {reason}");
    app.streaming_text.clear();
    app.streaming_thinking.clear();
    app.active_turn_id = None;
    app.stream_rx = None;
}

#[tracing::instrument(skip_all)]
// SAFETY: sanitized at ingestion — error messages may contain external data.
pub(crate) fn handle_stream_error(app: &mut App, msg: String) {
    tracing::error!("stream error: {msg}");
    app.error_toast = Some(ErrorToast::new(sanitize_for_display(&msg).into_owned()));
    app.active_turn_id = None;
    app.stream_rx = None;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::types::PlanStep;
    use crate::app::test_helpers::*;

    #[test]
    fn turn_start_sets_state() {
        let mut app = test_app();
        app.agents.push(test_agent("syn", "Syn"));
        app.focused_agent = Some("syn".into());
        app.streaming_text = "leftover".to_string();

        handle_stream_turn_start(&mut app, "t1".into(), "syn".into());

        assert!(app.active_turn_id.as_ref().unwrap() == "t1");
        assert!(app.streaming_text.is_empty());
        assert!(app.streaming_thinking.is_empty());
        assert!(app.streaming_tool_calls.is_empty());
        assert_eq!(app.agents[0].status, AgentStatus::Streaming);
    }

    #[test]
    fn text_delta_appends() {
        let mut app = test_app();
        handle_stream_text_delta(&mut app, "hello ".to_string());
        handle_stream_text_delta(&mut app, "world".to_string());
        assert_eq!(app.streaming_text, "hello world");
    }

    #[test]
    fn thinking_delta_appends() {
        let mut app = test_app();
        handle_stream_thinking_delta(&mut app, "thinking...".to_string());
        assert_eq!(app.streaming_thinking, "thinking...");
    }

    #[test]
    fn tool_start_adds_tool_call() {
        let mut app = test_app();
        app.agents.push(test_agent("syn", "Syn"));
        app.focused_agent = Some("syn".into());

        handle_stream_tool_start(&mut app, "read_file".to_string());

        assert_eq!(app.streaming_tool_calls.len(), 1);
        assert_eq!(app.streaming_tool_calls[0].name, "read_file");
        assert!(app.streaming_tool_calls[0].duration_ms.is_none());
        assert_eq!(app.agents[0].active_tool.as_deref(), Some("read_file"));
    }

    #[test]
    fn tool_result_updates_tool_call() {
        let mut app = test_app();
        app.agents.push(test_agent("syn", "Syn"));
        app.focused_agent = Some("syn".into());

        handle_stream_tool_start(&mut app, "read_file".to_string());
        handle_stream_tool_result(&mut app, "read_file".to_string(), false, 150);

        assert_eq!(app.streaming_tool_calls[0].duration_ms, Some(150));
        assert!(!app.streaming_tool_calls[0].is_error);
        assert!(app.agents[0].active_tool.is_none());
    }

    #[test]
    fn tool_result_error_flag() {
        let mut app = test_app();
        app.agents.push(test_agent("syn", "Syn"));
        app.focused_agent = Some("syn".into());

        handle_stream_tool_start(&mut app, "write_file".to_string());
        handle_stream_tool_result(&mut app, "write_file".to_string(), true, 50);

        assert!(app.streaming_tool_calls[0].is_error);
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

        assert!(matches!(app.overlay, Some(Overlay::ToolApproval(_))));
        if let Some(Overlay::ToolApproval(ref approval)) = app.overlay {
            assert_eq!(approval.tool_name, "dangerous_tool");
            assert_eq!(approval.risk, "high");
        }
    }

    #[test]
    fn tool_approval_resolved_closes_overlay() {
        let mut app = test_app();
        app.overlay = Some(Overlay::ToolApproval(ToolApprovalOverlay {
            turn_id: "t1".into(),
            tool_id: "tool1".into(),
            tool_name: "test".to_string(),
            input: serde_json::Value::Null,
            risk: "low".to_string(),
            reason: "test".to_string(),
        }));

        handle_stream_tool_approval_resolved(&mut app);
        assert!(app.overlay.is_none());
    }

    #[test]
    fn tool_approval_resolved_ignores_non_approval_overlay() {
        let mut app = test_app();
        app.overlay = Some(Overlay::Help);

        handle_stream_tool_approval_resolved(&mut app);
        assert!(matches!(app.overlay, Some(Overlay::Help)));
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

        assert!(matches!(app.overlay, Some(Overlay::PlanApproval(_))));
        if let Some(Overlay::PlanApproval(ref plan_overlay)) = app.overlay {
            assert_eq!(plan_overlay.steps.len(), 1);
            assert!(plan_overlay.steps[0].checked);
            assert_eq!(plan_overlay.total_cost_cents, 50);
        }
    }

    #[test]
    fn turn_abort_clears_state() {
        let mut app = test_app();
        app.active_turn_id = Some("t1".into());
        app.streaming_text = "partial".to_string();

        handle_stream_turn_abort(&mut app, "user cancelled".to_string());

        assert!(app.active_turn_id.is_none());
        assert!(app.streaming_text.is_empty());
        assert!(app.stream_rx.is_none());
    }

    #[test]
    fn stream_error_shows_toast() {
        let mut app = test_app();
        app.active_turn_id = Some("t1".into());

        handle_stream_error(&mut app, "connection lost".to_string());

        assert!(app.error_toast.is_some());
        assert_eq!(app.error_toast.as_ref().unwrap().message, "connection lost");
        assert!(app.active_turn_id.is_none());
    }

    #[test]
    fn text_delta_triggers_markdown_cache_on_newline() {
        let mut app = test_app();
        handle_stream_text_delta(&mut app, "line1\nline2".to_string());
        // newline should trigger markdown re-render
        assert!(!app.cached_markdown_text.is_empty());
    }
}
