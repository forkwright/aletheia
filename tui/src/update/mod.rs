mod api;
mod command;
mod input;
mod navigation;
mod overlay;
mod sse;
mod streaming;

use crate::app::App;
use crate::msg::Msg;

pub(crate) use api::extract_text_content;

pub(crate) async fn update(app: &mut App, msg: Msg) {
    match msg {
        // --- Input ---
        Msg::CharInput(c) => input::handle_char_input(app, c),
        Msg::Backspace => input::handle_backspace(app),
        Msg::Delete => input::handle_delete(app),
        Msg::CursorLeft => input::handle_cursor_left(app),
        Msg::CursorRight => input::handle_cursor_right(app),
        Msg::CursorHome => input::handle_cursor_home(app),
        Msg::CursorEnd => input::handle_cursor_end(app),
        Msg::DeleteWord => input::handle_delete_word(app),
        Msg::ClearLine => input::handle_clear_line(app),
        Msg::HistoryUp => input::handle_history_up(app),
        Msg::HistoryDown => input::handle_history_down(app),
        Msg::Submit => input::handle_submit(app),
        Msg::CopyLastResponse => input::handle_copy_last_response(app),
        Msg::ComposeInEditor => input::handle_compose_in_editor(app),

        // --- Command palette ---
        Msg::CommandPaletteOpen => command::handle_open(app),
        Msg::CommandPaletteClose => command::handle_close(app),
        Msg::CommandPaletteInput(c) => command::handle_input(app, c),
        Msg::CommandPaletteBackspace => command::handle_backspace(app),
        Msg::CommandPaletteDeleteWord => command::handle_delete_word(app),
        Msg::CommandPaletteSelect => command::handle_select(app).await,
        Msg::CommandPaletteUp => command::handle_up(app),
        Msg::CommandPaletteDown => command::handle_down(app),
        Msg::CommandPaletteTab => command::handle_tab(app),

        // --- Navigation ---
        Msg::ScrollUp => navigation::handle_scroll_up(app),
        Msg::ScrollDown => navigation::handle_scroll_down(app),
        Msg::ScrollPageUp => navigation::handle_scroll_page_up(app),
        Msg::ScrollPageDown => navigation::handle_scroll_page_down(app),
        Msg::ScrollToBottom => navigation::handle_scroll_to_bottom(app),
        Msg::FocusAgent(id) => navigation::handle_focus_agent(app, id).await,
        Msg::NextAgent => navigation::handle_next_agent(app).await,
        Msg::PrevAgent => navigation::handle_prev_agent(app).await,
        Msg::ToggleSidebar => navigation::handle_toggle_sidebar(app),
        Msg::ToggleThinking => navigation::handle_toggle_thinking(app),
        Msg::Resize(w, h) => navigation::handle_resize(app, w, h),

        // --- Overlay ---
        Msg::OpenOverlay(kind) => overlay::handle_open_overlay(app, kind),
        Msg::CloseOverlay => overlay::handle_close_overlay(app),
        Msg::OverlayUp => overlay::handle_overlay_up(app),
        Msg::OverlayDown => overlay::handle_overlay_down(app),
        Msg::OverlaySelect => overlay::handle_overlay_select(app).await,
        Msg::OverlayFilter(_) | Msg::OverlayFilterBackspace => {}

        // --- SSE ---
        Msg::SseConnected => sse::handle_sse_connected(app).await,
        Msg::SseDisconnected => sse::handle_sse_disconnected(app),
        Msg::SseInit { active_turns } => sse::handle_sse_init(app, active_turns),
        Msg::SseTurnBefore { nous_id, .. } => sse::handle_sse_turn_before(app, nous_id),
        Msg::SseTurnAfter {
            nous_id,
            session_id,
        } => sse::handle_sse_turn_after(app, nous_id, session_id).await,
        Msg::SseToolCalled {
            nous_id,
            tool_name,
        } => sse::handle_sse_tool_called(app, nous_id, tool_name),
        Msg::SseToolFailed { nous_id, .. } => sse::handle_sse_tool_failed(app, nous_id),
        Msg::SseStatusUpdate { nous_id, status } => {
            sse::handle_sse_status_update(app, nous_id, status)
        }
        Msg::SseSessionCreated { nous_id, .. } => {
            sse::handle_sse_session_created(app, nous_id).await
        }
        Msg::SseSessionArchived {
            nous_id,
            session_id,
        } => sse::handle_sse_session_archived(app, nous_id, session_id),
        Msg::SseDistillBefore { nous_id } => sse::handle_sse_distill_before(app, nous_id),
        Msg::SseDistillStage { nous_id, stage } => {
            sse::handle_sse_distill_stage(app, nous_id, stage)
        }
        Msg::SseDistillAfter { nous_id } => sse::handle_sse_distill_after(app, nous_id).await,

        // --- Streaming ---
        Msg::StreamTurnStart {
            turn_id, nous_id, ..
        } => streaming::handle_stream_turn_start(app, turn_id, nous_id),
        Msg::StreamTextDelta(text) => streaming::handle_stream_text_delta(app, text),
        Msg::StreamThinkingDelta(text) => streaming::handle_stream_thinking_delta(app, text),
        Msg::StreamToolStart { tool_name, .. } => {
            streaming::handle_stream_tool_start(app, tool_name)
        }
        Msg::StreamToolResult {
            tool_name,
            is_error,
            duration_ms,
            ..
        } => streaming::handle_stream_tool_result(app, tool_name, is_error, duration_ms),
        Msg::StreamToolApprovalRequired {
            turn_id,
            tool_name,
            tool_id,
            input,
            risk,
            reason,
        } => streaming::handle_stream_tool_approval_required(
            app, turn_id, tool_name, tool_id, input, risk, reason,
        ),
        Msg::StreamToolApprovalResolved { .. } => {
            streaming::handle_stream_tool_approval_resolved(app)
        }
        Msg::StreamPlanProposed { plan } => streaming::handle_stream_plan_proposed(app, plan),
        Msg::StreamPlanStepStart { .. }
        | Msg::StreamPlanStepComplete { .. }
        | Msg::StreamPlanComplete { .. } => {}
        Msg::StreamTurnComplete { outcome } => {
            streaming::handle_stream_turn_complete(app, outcome).await
        }
        Msg::StreamTurnAbort { reason } => streaming::handle_stream_turn_abort(app, reason),
        Msg::StreamError(msg) => streaming::handle_stream_error(app, msg),

        // --- API ---
        Msg::AgentsLoaded(agents) => api::handle_agents_loaded(app, agents),
        Msg::SessionsLoaded { nous_id, sessions } => {
            api::handle_sessions_loaded(app, nous_id, sessions)
        }
        Msg::HistoryLoaded { messages, .. } => api::handle_history_loaded(app, messages),
        Msg::CostLoaded { daily_total_cents } => api::handle_cost_loaded(app, daily_total_cents),
        Msg::AuthResult(_) | Msg::ApiError(_) => {}
        Msg::NewSession => api::handle_new_session(app).await,
        Msg::ShowError(msg) => api::handle_show_error(app, msg),
        Msg::DismissError => api::handle_dismiss_error(app),
        Msg::Quit => app.should_quit = true,
        Msg::Tick => api::handle_tick(app),
    }
}
