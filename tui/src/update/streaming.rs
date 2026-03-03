use crate::api::types::{Plan, TurnOutcome};
use crate::app::App;
use crate::msg::ErrorToast;
use crate::state::{
    AgentStatus, ChatMessage, Overlay, PlanApprovalOverlay, PlanStepApproval, ToolApprovalOverlay,
    ToolCallInfo,
};

pub(crate) fn handle_stream_turn_start(app: &mut App, turn_id: String, nous_id: String) {
    app.active_turn_id = Some(turn_id);
    app.streaming_text.clear();
    app.streaming_thinking.clear();
    app.streaming_tool_calls.clear();
    app.cached_markdown_text.clear();
    app.cached_markdown_lines.clear();
    if let Some(agent) = app.agents.iter_mut().find(|a| a.id == nous_id) {
        agent.status = AgentStatus::Streaming;
    }
}

pub(crate) fn handle_stream_text_delta(app: &mut App, text: String) {
    app.streaming_text.push_str(&text);
    let delta = app.streaming_text.len() as i64 - app.cached_markdown_text.len() as i64;
    if delta >= 64 || text.contains('\n') {
        let width = 120;
        app.cached_markdown_lines = crate::markdown::render(
            &app.streaming_text,
            width,
            &app.theme,
            &app.highlighter,
        );
        app.cached_markdown_text = app.streaming_text.clone();
    }
    if app.auto_scroll {
        app.scroll_offset = 0;
    }
}

pub(crate) fn handle_stream_thinking_delta(app: &mut App, text: String) {
    app.streaming_thinking.push_str(&text);
}

pub(crate) fn handle_stream_tool_start(app: &mut App, tool_name: String) {
    app.streaming_tool_calls.push(ToolCallInfo {
        name: tool_name.clone(),
        duration_ms: None,
        is_error: false,
    });
    if let Some(ref agent_id) = app.focused_agent {
        if let Some(agent) = app.agents.iter_mut().find(|a| a.id == *agent_id) {
            agent.active_tool = Some(tool_name);
            agent.tool_started_at = Some(std::time::Instant::now());
        }
    }
}

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
    if let Some(ref agent_id) = app.focused_agent {
        if let Some(agent) = app.agents.iter_mut().find(|a| a.id == *agent_id) {
            agent.active_tool = None;
            agent.tool_started_at = None;
        }
    }
}

pub(crate) fn handle_stream_tool_approval_required(
    app: &mut App,
    turn_id: String,
    tool_name: String,
    tool_id: String,
    input: serde_json::Value,
    risk: String,
    reason: String,
) {
    app.overlay = Some(Overlay::ToolApproval(ToolApprovalOverlay {
        turn_id,
        tool_id,
        tool_name,
        input,
        risk,
        reason,
    }));
}

pub(crate) fn handle_stream_tool_approval_resolved(app: &mut App) {
    if app.is_tool_approval_overlay() {
        app.overlay = None;
    }
}

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
                label: s.label,
                role: s.role,
                checked: true,
            })
            .collect(),
    }));
}

pub(crate) async fn handle_stream_turn_complete(app: &mut App, outcome: TurnOutcome) {
    if !app.streaming_text.is_empty() {
        app.messages.push(ChatMessage {
            role: "assistant".to_string(),
            text: app.streaming_text.clone(),
            timestamp: None,
            model: Some(outcome.model),
            is_streaming: false,
            tool_calls: std::mem::take(&mut app.streaming_tool_calls),
        });
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
    if let Ok(cents) = app.client.today_cost_cents().await {
        app.daily_cost_cents = cents;
    }
}

pub(crate) fn handle_stream_turn_abort(app: &mut App, reason: String) {
    tracing::info!("turn aborted: {reason}");
    app.streaming_text.clear();
    app.streaming_thinking.clear();
    app.active_turn_id = None;
    app.stream_rx = None;
}

pub(crate) fn handle_stream_error(app: &mut App, msg: String) {
    tracing::error!("stream error: {msg}");
    app.error_toast = Some(ErrorToast::new(msg));
    app.active_turn_id = None;
    app.stream_rx = None;
}
