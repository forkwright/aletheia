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
    app.connection.stream_phase = crate::state::StreamPhase::Requesting;
    app.connection.streaming_text.clear();
    app.connection.streaming_thinking.clear();
    app.connection.streaming_tool_calls.clear();
    app.connection.stream_last_event_at = Some(std::time::Instant::now());
    app.connection.stall_warned = false;
    app.connection.stall_message = None;
    app.connection.streaming_line_buffer.clear();
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
    app.connection.stream_phase = crate::state::StreamPhase::Streaming;
    let clean = sanitize_for_display(&text);
    // PERF: Line-by-line streaming. Buffer partial lines and flush only complete
    // lines (ending in `\n`) to streaming_text. The incomplete tail stays in the
    // line buffer. This prevents markdown re-parses on every token within a line.
    app.connection.streaming_line_buffer.push_str(&clean);
    if let Some(last_newline) = app.connection.streaming_line_buffer.rfind('\n') {
        let complete = app.connection.streaming_line_buffer[..=last_newline].to_string();
        let remainder = app.connection.streaming_line_buffer[last_newline + 1..].to_string();
        app.connection.streaming_text.push_str(&complete);
        app.connection.streaming_line_buffer = remainder;
    }
    if app.viewport.render.auto_scroll {
        app.viewport.render.scroll_offset = 0;
    }
}

#[tracing::instrument(skip_all, fields(len = text.len()))]
// SAFETY: sanitized at ingestion: thinking text from LLM API.
pub(crate) fn handle_stream_thinking_delta(app: &mut App, text: String) {
    app.connection.stream_phase = crate::state::StreamPhase::Thinking;
    let clean = sanitize_for_display(&text);
    app.connection.streaming_thinking.push_str(&clean);
    app.layout.ops.push_thinking(&clean);
}

#[tracing::instrument(skip_all, fields(%tool_name))]
// SAFETY: sanitized at ingestion: tool names from stream API.
pub(crate) fn handle_stream_tool_start(
    app: &mut App,
    tool_name: String,
    tool_id: ToolId,
    input: Option<serde_json::Value>,
) {
    app.connection.stream_phase = crate::state::StreamPhase::Streaming;
    let clean_name = sanitize_for_display(&tool_name).into_owned();
    app.connection.streaming_tool_calls.push(ToolCallInfo {
        name: clean_name.clone(),
        tool_id: Some(tool_id),
        duration_ms: None,
        is_error: false,
        output: None,
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
    tool_id: ToolId,
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
        tc.output = result.clone();
    }
    // WHY: Auto-expand failed tool cards so errors are immediately visible.
    if is_error {
        app.interaction.tool_expanded.insert(tool_id);
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
    app.connection.stream_phase = crate::state::StreamPhase::Waiting;
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
    app.connection.stream_phase = crate::state::StreamPhase::Done;
    // Flush any remaining partial line from the line buffer.
    if !app.connection.streaming_line_buffer.is_empty() {
        let remaining = std::mem::take(&mut app.connection.streaming_line_buffer);
        app.connection.streaming_text.push_str(&remaining);
    }
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
            kind: crate::state::MessageKind::default(),
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
        // WHY: clamped to [0, 100] by .min(100); u64 → u8 is safe here.
        let pct = ((u64::from(ctx_used) * 100) / u64::from(ctx_total)).min(100) as u8;
        app.dashboard.context_usage_pct = Some(pct);
    }
    if let Ok(cents) = app.client.today_cost_cents().await {
        app.dashboard.daily_cost_cents = cents;
    }

    // WHY: Record token usage after every completed turn so the metrics dashboard
    // reflects cumulative spend without requiring a separate API poll.
    app.layout.metrics.record_turn(
        &outcome.nous_id,
        outcome.input_tokens,
        outcome.output_tokens,
        outcome.cache_read_tokens,
        outcome.cache_write_tokens,
    );

    // WHY: auto-send the next queued message now that the turn is complete
    crate::update::input::send_next_queued(app);
    app.connection.stream_phase = crate::state::StreamPhase::Idle;
}

#[tracing::instrument(skip_all)]
pub(crate) fn handle_stream_turn_abort(app: &mut App, reason: String) {
    tracing::info!("turn aborted: {reason}");
    app.connection.stream_phase = crate::state::StreamPhase::Idle;
    app.connection.streaming_text.clear();
    app.connection.streaming_line_buffer.clear();
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
    app.connection.stream_phase = crate::state::StreamPhase::Error;
    app.connection.streaming_line_buffer.clear();
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
    let span = tracing::info_span!("abort_turn", turn_id = %id);
    tokio::spawn(
        async move {
            if let Err(e) = client.abort_turn(&id).await {
                tracing::warn!(error = %e, "abort_turn request failed");
            }
        }
        .instrument(span),
    );

    app.connection.stream_phase = crate::state::StreamPhase::Idle;
    // Flush line buffer into streaming text before committing.
    if !app.connection.streaming_line_buffer.is_empty() {
        let remaining = std::mem::take(&mut app.connection.streaming_line_buffer);
        app.connection.streaming_text.push_str(&remaining);
    }
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
        kind: crate::state::MessageKind::default(),
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
mod tests;
