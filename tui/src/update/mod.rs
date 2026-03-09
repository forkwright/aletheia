mod api;
mod command;
mod diff;
mod filter;
mod input;
pub(crate) mod memory;
mod navigation;
mod overlay;
pub(crate) mod selection;
pub(crate) mod settings;
mod sse;
mod streaming;
pub(crate) mod tabs;
pub(crate) mod view_nav;

use crate::app::App;
use crate::msg::Msg;

pub(crate) use api::extract_text_content;

#[tracing::instrument(skip_all)]
pub(crate) async fn update(app: &mut App, msg: Msg) {
    // Clear pending_g on any message except GPrefix itself
    if !matches!(msg, Msg::GPrefix | Msg::Tick) {
        app.pending_g = false;
    }

    match msg {
        // --- Tabs ---
        Msg::TabNew => tabs::handle_tab_new(app),
        Msg::TabClose => tabs::handle_tab_close(app),
        Msg::TabNext => tabs::handle_tab_next(app),
        Msg::TabPrev => tabs::handle_tab_prev(app),
        Msg::TabJump(n) => tabs::handle_tab_jump(app, n),
        Msg::GPrefix => tabs::handle_g_prefix(app),

        // --- Message selection ---
        Msg::SelectPrev => selection::handle_select_prev(app),
        Msg::SelectNext => selection::handle_select_next(app),
        Msg::DeselectMessage => selection::handle_deselect(app),
        Msg::SelectFirst => selection::handle_select_first(app),
        Msg::SelectLast => selection::handle_select_last(app),
        Msg::OpenContextActions => selection::handle_open_context_actions(app),
        Msg::MessageAction(action) => selection::handle_message_action(app, action),

        // --- Input ---
        Msg::CharInput(c) => {
            // If a message is selected and a non-action char arrives, deselect first
            if app.selected_message.is_some() {
                selection::handle_deselect(app);
            }
            input::handle_char_input(app, c);
        }
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

        // --- Filter ---
        Msg::FilterOpen => filter::handle_open(app),
        Msg::FilterClose => filter::handle_close(app),
        Msg::FilterInput(c) => filter::handle_input(app, c),
        Msg::FilterBackspace => filter::handle_backspace(app),
        Msg::FilterClear => filter::handle_clear(app),
        Msg::FilterConfirm => filter::handle_confirm(app),
        Msg::FilterNextMatch => filter::handle_next_match(app),
        Msg::FilterPrevMatch => filter::handle_prev_match(app),

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
        Msg::ToggleOpsPane => navigation::handle_toggle_ops_pane(app),
        Msg::OpsFocusSwitch => navigation::handle_ops_focus_switch(app),
        Msg::OpsScrollUp => app.ops.scroll_up(),
        Msg::OpsScrollDown => app.ops.scroll_down(),
        Msg::OpsSelectPrev => app.ops.select_prev(),
        Msg::OpsSelectNext => app.ops.select_next(),
        Msg::OpsToggleExpand => app.ops.toggle_selected(),
        Msg::Resize(w, h) => navigation::handle_resize(app, w, h),
        Msg::ViewDrillIn => view_nav::handle_drill_in(app),
        Msg::ViewPopBack => view_nav::handle_pop_back(app),

        // --- Overlay ---
        Msg::OpenOverlay(kind) => overlay::handle_open_overlay(app, kind).await,
        Msg::CloseOverlay => overlay::handle_close_overlay(app),
        Msg::OverlayUp => overlay::handle_overlay_up(app),
        Msg::OverlayDown => overlay::handle_overlay_down(app),
        Msg::OverlaySelect => overlay::handle_overlay_select(app).await,
        Msg::OverlayFilter(c) => {
            if matches!(&app.overlay, Some(crate::state::Overlay::Settings(_))) {
                if settings::is_editing(app) {
                    settings::handle_edit_char(app, c);
                } else {
                    match c {
                        's' | 'S' => settings::handle_save(app).await,
                        'r' | 'R' => settings::handle_reset(app),
                        _ => {}
                    }
                }
            }
        }
        Msg::OverlayFilterBackspace => {
            if matches!(&app.overlay, Some(crate::state::Overlay::Settings(_))) {
                settings::handle_edit_backspace(app);
            }
        }

        // --- SSE ---
        Msg::SseConnected => sse::handle_sse_connected(app).await,
        Msg::SseDisconnected => sse::handle_sse_disconnected(app),
        Msg::SseInit { active_turns } => sse::handle_sse_init(app, active_turns),
        Msg::SseTurnBefore { nous_id, .. } => sse::handle_sse_turn_before(app, nous_id),
        Msg::SseTurnAfter {
            nous_id,
            session_id,
        } => sse::handle_sse_turn_after(app, nous_id, session_id).await,
        Msg::SseToolCalled { nous_id, tool_name } => {
            sse::handle_sse_tool_called(app, nous_id, tool_name)
        }
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
        Msg::SessionPickerNewSession => api::handle_session_picker_new(app).await,
        Msg::SessionPickerArchive => api::handle_session_picker_archive(app).await,
        Msg::SettingsLoaded(config) => settings::handle_loaded(app, config),
        Msg::SettingsSaved => settings::handle_saved(app),
        Msg::SettingsSaveError(msg) => settings::handle_save_error(app, msg),
        // --- Memory inspector ---
        Msg::MemoryOpen => memory::handle_open(app).await,
        Msg::MemoryClose => memory::handle_close(app),
        Msg::MemoryTabNext => memory::handle_tab_next(app),
        Msg::MemoryTabPrev => memory::handle_tab_prev(app),
        Msg::MemorySelectUp => memory::handle_select_up(app),
        Msg::MemorySelectDown => memory::handle_select_down(app),
        Msg::MemorySelectFirst => memory::handle_select_first(app),
        Msg::MemorySelectLast => memory::handle_select_last(app),
        Msg::MemorySortCycle => memory::handle_sort_cycle(app),
        Msg::MemoryFilterOpen => memory::handle_filter_open(app),
        Msg::MemoryFilterClose => memory::handle_filter_close(app),
        Msg::MemoryFilterInput(c) => memory::handle_filter_input(app, c),
        Msg::MemoryFilterBackspace => memory::handle_filter_backspace(app),
        Msg::MemoryDrillIn => memory::handle_drill_in(app).await,
        Msg::MemoryPopBack => memory::handle_pop_back(app),
        Msg::MemoryForget => memory::handle_forget(app).await,
        Msg::MemoryRestore => memory::handle_restore(app).await,
        Msg::MemoryEditConfidence => memory::handle_edit_confidence_start(app),
        Msg::MemoryConfidenceInput(c) => memory::handle_confidence_input(app, c),
        Msg::MemoryConfidenceBackspace => memory::handle_confidence_backspace(app),
        Msg::MemoryConfidenceSubmit => memory::handle_confidence_submit(app).await,
        Msg::MemoryConfidenceCancel => memory::handle_confidence_cancel(app),
        Msg::MemorySearchOpen => memory::handle_search_open(app),
        Msg::MemorySearchInput(c) => memory::handle_search_input(app, c),
        Msg::MemorySearchBackspace => memory::handle_search_backspace(app),
        Msg::MemorySearchSubmit => memory::handle_search_submit(app).await,
        Msg::MemorySearchClose => memory::handle_search_close(app),
        Msg::MemoryPageDown => memory::handle_page_down(app),
        Msg::MemoryPageUp => memory::handle_page_up(app),
        Msg::MemoryFactsLoaded { facts, total } => memory::handle_facts_loaded(app, facts, total),
        Msg::MemoryDetailLoaded(detail) => memory::handle_detail_loaded(app, *detail),
        Msg::MemoryEntitiesLoaded(_)
        | Msg::MemoryRelationshipsLoaded(_)
        | Msg::MemoryTimelineLoaded(_) => {}
        Msg::MemorySearchResults(_) => {}
        Msg::MemoryActionResult(msg) => memory::handle_action_result(app, msg),

        Msg::ShowError(msg) => api::handle_show_error(app, msg),
        Msg::ShowSuccess(msg) => api::handle_show_error(app, msg),
        Msg::DismissError => api::handle_dismiss_error(app),
        // --- Diff viewer ---
        Msg::DiffOpen => diff::handle_diff_open(app).await,
        Msg::DiffClose => diff::handle_diff_close(app),
        Msg::DiffCycleMode => diff::handle_diff_cycle_mode(app),
        Msg::DiffScrollUp => diff::handle_diff_scroll_up(app),
        Msg::DiffScrollDown => diff::handle_diff_scroll_down(app),
        Msg::DiffPageUp => diff::handle_diff_page_up(app),
        Msg::DiffPageDown => diff::handle_diff_page_down(app),
        Msg::DiffFromToolResult {
            path,
            old_content,
            new_content,
        } => diff::handle_diff_from_tool_result(app, &path, &old_content, &new_content),

        Msg::Quit => app.should_quit = true,
        Msg::Tick => api::handle_tick(app),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::test_helpers::test_app;

    #[tokio::test]
    async fn quit_sets_should_quit() {
        let mut app = test_app();
        update(&mut app, Msg::Quit).await;
        assert!(app.should_quit);
    }

    #[tokio::test]
    async fn tick_does_not_panic() {
        let mut app = test_app();
        update(&mut app, Msg::Tick).await;
    }

    #[tokio::test]
    async fn char_input_with_selection_deselects_first() {
        let mut app = test_app();
        app.selected_message = Some(0);
        update(&mut app, Msg::CharInput('a')).await;
        assert!(app.selected_message.is_none());
    }

    #[tokio::test]
    async fn resize_does_not_panic() {
        let mut app = test_app();
        update(&mut app, Msg::Resize(120, 40)).await;
    }

    #[tokio::test]
    async fn dismiss_error_clears_toast() {
        let mut app = test_app();
        app.error_toast = Some(crate::msg::ErrorToast::new("oops".to_string()));
        update(&mut app, Msg::DismissError).await;
        assert!(app.error_toast.is_none());
    }

    #[tokio::test]
    async fn show_error_sets_toast() {
        let mut app = test_app();
        update(&mut app, Msg::ShowError("bad".to_string())).await;
        assert!(app.error_toast.is_some());
    }
}
