mod api;
mod command;
mod diff;
mod editor;
mod filter;
mod input;
pub(crate) mod memory;
pub(crate) mod metrics;
mod navigation;
mod overlay;
mod search;
pub(crate) mod selection;
pub(crate) mod settings;
mod slash;
mod sse;
mod streaming;
pub(crate) mod tabs;
pub(crate) mod view_nav;

use crate::app::App;
use crate::msg::Msg;

pub(crate) use api::extract_text_content;

#[tracing::instrument(skip_all)]
pub(crate) async fn update(app: &mut App, msg: Msg) {
    if !matches!(msg, Msg::GPrefix | Msg::Tick) {
        app.layout.pending_g = false;
    }

    match msg {
        Msg::TabNew => tabs::handle_tab_new(app),
        Msg::TabClose => tabs::handle_tab_close(app),
        Msg::TabNext => tabs::handle_tab_next(app),
        Msg::TabPrev => tabs::handle_tab_prev(app),
        Msg::TabJump(n) => tabs::handle_tab_jump(app, n),
        Msg::GPrefix => tabs::handle_g_prefix(app),

        Msg::SelectPrev => selection::handle_select_prev(app),
        Msg::SelectNext => selection::handle_select_next(app),
        Msg::DeselectMessage => selection::handle_deselect(app),
        Msg::SelectFirst => selection::handle_select_first(app),
        Msg::SelectLast => selection::handle_select_last(app),
        Msg::MessageAction(action) => selection::handle_message_action(app, action),

        Msg::CharInput(c) => {
            if app.interaction.selected_message.is_some() {
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
        Msg::DeleteToEnd => input::handle_delete_to_end(app),
        Msg::HistoryUp => input::handle_history_up(app),
        Msg::HistoryDown => input::handle_history_down(app),
        Msg::Submit => input::handle_submit(app),
        Msg::CopyLastResponse => input::handle_copy_last_response(app),
        Msg::ComposeInEditor => input::handle_compose_in_editor(app),
        Msg::Yank => input::handle_yank(app),
        Msg::YankCycle => input::handle_yank_cycle(app),
        Msg::WordForward => input::handle_word_forward(app),
        Msg::WordBackward => input::handle_word_backward(app),
        Msg::HistorySearchOpen => input::handle_history_search_open(app),
        Msg::HistorySearchClose => input::handle_history_search_close(app),
        Msg::HistorySearchInput(c) => input::handle_history_search_input(app, c),
        Msg::HistorySearchBackspace => input::handle_history_search_backspace(app),
        Msg::HistorySearchNext => input::handle_history_search_next(app),
        Msg::HistorySearchAccept => input::handle_history_search_accept(app),
        Msg::NewlineInsert => input::handle_newline_insert(app),
        Msg::ClearScreen => input::handle_clear_screen(app),
        Msg::ClipboardPaste => input::handle_clipboard_paste(app),
        Msg::QueuedMessageCancel(idx) => input::handle_queued_message_cancel(app, idx),

        Msg::FilterOpen => filter::handle_open(app),
        Msg::FilterClose => filter::handle_close(app),
        Msg::FilterInput(c) => filter::handle_input(app, c),
        Msg::FilterBackspace => filter::handle_backspace(app),
        Msg::FilterClear => filter::handle_clear(app),
        Msg::FilterConfirm => filter::handle_confirm(app),
        Msg::FilterNextMatch => filter::handle_next_match(app),
        Msg::FilterPrevMatch => filter::handle_prev_match(app),

        Msg::CommandPaletteOpen => command::handle_open(app),
        Msg::CommandPaletteClose => command::handle_close(app),
        Msg::CommandPaletteInput(c) => command::handle_input(app, c),
        Msg::CommandPaletteBackspace => command::handle_backspace(app),
        Msg::CommandPaletteDeleteWord => command::handle_delete_word(app),
        Msg::CommandPaletteSelect => command::handle_select(app).await,
        Msg::CommandPaletteUp => command::handle_up(app),
        Msg::CommandPaletteDown => command::handle_down(app),
        Msg::CommandPaletteTab => command::handle_tab(app),

        Msg::ScrollUp => navigation::handle_scroll_up(app),
        Msg::ScrollDown => navigation::handle_scroll_down(app),
        Msg::ScrollLineUp => navigation::handle_scroll_line_up(app),
        Msg::ScrollLineDown => navigation::handle_scroll_line_down(app),
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
        Msg::OpsScrollUp => app.layout.ops.scroll_up(),
        Msg::OpsScrollDown => app.layout.ops.scroll_down(),
        Msg::OpsSelectPrev => app.layout.ops.select_prev(),
        Msg::OpsSelectNext => app.layout.ops.select_next(),
        Msg::OpsToggleExpand => app.layout.ops.toggle_selected(),
        Msg::OpsToggleShowAll => app.layout.ops.toggle_show_all(),
        Msg::Resize(w, h) => navigation::handle_resize(app, w, h),
        Msg::ViewDrillIn => view_nav::handle_drill_in(app),
        Msg::ViewPopBack => view_nav::handle_pop_back(app),

        Msg::OpenOverlay(kind) => overlay::handle_open_overlay(app, kind).await,
        Msg::CloseOverlay => overlay::handle_close_overlay(app),
        Msg::OverlayUp => overlay::handle_overlay_up(app),
        Msg::OverlayDown => overlay::handle_overlay_down(app),
        Msg::OverlaySelect => overlay::handle_overlay_select(app).await,
        Msg::ToolApprovalAlwaysAllow => overlay::handle_tool_approval_always_allow(app),
        Msg::OverlayFilter(c) => {
            if matches!(
                &app.layout.overlay,
                Some(crate::state::Overlay::Settings(_))
            ) {
                if settings::is_editing(app) {
                    settings::handle_edit_char(app, c);
                } else {
                    match c {
                        's' | 'S' => settings::handle_save(app).await,
                        'r' | 'R' => settings::handle_reset(app),
                        _ => {
                            // NOTE: other keys ignored in settings non-edit mode
                        }
                    }
                }
            } else if let Some(crate::state::Overlay::DecisionCard(ref mut card)) =
                app.layout.overlay
            {
                match card.focused_field {
                    crate::state::DecisionField::CustomAnswer => {
                        card.custom_answer.push(c);
                        card.custom_cursor = card.custom_answer.len();
                    }
                    crate::state::DecisionField::Notes => {
                        card.notes.push(c);
                        card.notes_cursor = card.notes.len();
                    }
                    crate::state::DecisionField::Options => {
                        // NOTE: char input in options field has no effect
                    }
                }
            }
        }
        Msg::OverlayFilterBackspace => {
            if matches!(
                &app.layout.overlay,
                Some(crate::state::Overlay::Settings(_))
            ) {
                settings::handle_edit_backspace(app);
            } else if let Some(crate::state::Overlay::DecisionCard(ref mut card)) =
                app.layout.overlay
            {
                match card.focused_field {
                    crate::state::DecisionField::CustomAnswer => {
                        card.custom_answer.pop();
                        card.custom_cursor = card.custom_answer.len();
                    }
                    crate::state::DecisionField::Notes => {
                        card.notes.pop();
                        card.notes_cursor = card.notes.len();
                    }
                    crate::state::DecisionField::Options => {}
                }
            }
        }

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

        Msg::StreamTurnStart {
            turn_id, nous_id, ..
        } => streaming::handle_stream_turn_start(app, turn_id, nous_id),
        Msg::StreamTextDelta(text) => streaming::handle_stream_text_delta(app, text),
        Msg::StreamThinkingDelta(text) => streaming::handle_stream_thinking_delta(app, text),
        Msg::StreamToolStart {
            tool_name, input, ..
        } => streaming::handle_stream_tool_start(app, tool_name, input),
        Msg::StreamToolResult {
            tool_name,
            is_error,
            duration_ms,
            result,
            ..
        } => streaming::handle_stream_tool_result(app, tool_name, is_error, duration_ms, result),
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
        Msg::StreamPlanStepStart { step_id, .. } => {
            streaming::handle_stream_plan_step_start(app, step_id)
        }
        Msg::StreamPlanStepComplete {
            step_id, status, ..
        } => streaming::handle_stream_plan_step_complete(app, step_id, status),
        Msg::StreamPlanComplete { status, .. } => {
            streaming::handle_stream_plan_complete(app, status)
        }
        Msg::StreamTurnComplete { outcome } => {
            streaming::handle_stream_turn_complete(app, outcome).await
        }
        Msg::StreamTurnAbort { reason } => streaming::handle_stream_turn_abort(app, reason),
        Msg::StreamError(msg) => streaming::handle_stream_error(app, msg),
        Msg::CancelTurn => streaming::handle_cancel_turn(app).await,

        Msg::AgentsLoaded(agents) => api::handle_agents_loaded(app, agents),
        Msg::SessionsLoaded { nous_id, sessions } => {
            api::handle_sessions_loaded(app, nous_id, sessions)
        }
        Msg::HistoryLoaded { messages, .. } => api::handle_history_loaded(app, messages),
        Msg::CostLoaded { daily_total_cents } => api::handle_cost_loaded(app, daily_total_cents),
        // NOTE: auth/API errors handled upstream, no local state update needed
        Msg::AuthResult(_) | Msg::ApiError(_) => {}
        Msg::NewSession => api::handle_new_session(app).await,
        Msg::SessionPickerNewSession => api::handle_session_picker_new(app).await,
        Msg::SessionPickerArchive => api::handle_session_picker_archive(app).await,
        Msg::SettingsLoaded(config) => settings::handle_loaded(app, config),
        Msg::SettingsSaved => settings::handle_saved(app),
        Msg::SettingsSaveError(msg) => settings::handle_save_error(app, msg),
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
        // NOTE: memory data variants handled in memory inspector view
        Msg::MemoryEntitiesLoaded(_)
        | Msg::MemoryRelationshipsLoaded(_)
        | Msg::MemoryTimelineLoaded(_) => {}
        Msg::MemorySearchResults(_) => {}
        Msg::MemoryActionResult(msg) => memory::handle_action_result(app, msg),

        Msg::MetricsOpen => metrics::handle_open(app).await,
        Msg::MetricsClose => metrics::handle_close(app),
        Msg::MetricsSelectUp => metrics::handle_select_up(app),
        Msg::MetricsSelectDown => metrics::handle_select_down(app),
        Msg::MetricsHealthLoaded(healthy) => metrics::handle_health_loaded(app, healthy),

        Msg::ExportConversation => command::execute_export_from_msg(app),

        Msg::SlashCompleteOpen => slash::handle_open(app),
        Msg::SlashCompleteClose => slash::handle_close(app),
        Msg::SlashCompleteInput(c) => slash::handle_input(app, c),
        Msg::SlashCompleteBackspace => slash::handle_backspace(app),
        Msg::SlashCompleteUp => slash::handle_up(app),
        Msg::SlashCompleteDown => slash::handle_down(app),
        Msg::SlashCompleteSelect => slash::handle_select(app).await,

        Msg::ToastPush {
            message,
            kind,
            duration_secs,
        } => {
            use crate::state::notification::Toast;
            let toast = Toast::with_duration(message.clone(), kind, duration_secs);
            app.viewport.toasts.push(toast);
            app.layout
                .notifications
                .push(app.dashboard.focused_agent.clone(), message, kind);
        }
        Msg::ErrorBannerSet(msg) => {
            use crate::state::notification::ErrorBanner;
            app.viewport.error_banner = Some(ErrorBanner { message: msg });
        }
        Msg::ErrorBannerDismiss => {
            app.viewport.error_banner = None;
        }

        Msg::SessionSearchOpen => search::handle_open(app),
        Msg::SessionSearchClose => search::handle_close(app),
        Msg::SessionSearchInput(c) => search::handle_input(app, c),
        Msg::SessionSearchBackspace => search::handle_backspace(app),
        // NOTE: submit triggers search via SessionSearchSelect
        Msg::SessionSearchSubmit => {}
        Msg::SessionSearchUp => search::handle_up(app),
        Msg::SessionSearchDown => search::handle_down(app),
        Msg::SessionSearchSelect => search::handle_select(app).await,

        Msg::ShowError(msg) => api::handle_show_error(app, msg),
        Msg::ShowSuccess(msg) => api::handle_show_success(app, msg),
        Msg::DismissError => api::handle_dismiss_error(app),
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

        Msg::DecisionCardNextField => {
            if let Some(crate::state::Overlay::DecisionCard(ref mut card)) = app.layout.overlay {
                card.next_field();
            }
        }
        Msg::DecisionCardPrevField => {
            if let Some(crate::state::Overlay::DecisionCard(ref mut card)) = app.layout.overlay {
                card.prev_field();
            }
        }
        Msg::StreamDecisionRequired { question, options } => {
            let opts: Vec<crate::state::DecisionOption> = options
                .into_iter()
                .map(
                    |(label, description, is_recommendation)| crate::state::DecisionOption {
                        label,
                        description,
                        is_recommendation,
                    },
                )
                .collect();
            app.layout.overlay = Some(crate::state::Overlay::DecisionCard(
                crate::state::DecisionCardOverlay::new(question, opts),
            ));
        }

        Msg::EditorOpen => editor::handle_open(app),
        Msg::EditorClose => editor::handle_close(app),
        Msg::EditorCharInput(c) => editor::handle_char_input(app, c),
        Msg::EditorNewline => editor::handle_newline(app),
        Msg::EditorBackspace => editor::handle_backspace(app),
        Msg::EditorDelete => editor::handle_delete(app),
        Msg::EditorCursorUp => editor::handle_cursor_up(app),
        Msg::EditorCursorDown => editor::handle_cursor_down(app),
        Msg::EditorCursorLeft => editor::handle_cursor_left(app),
        Msg::EditorCursorRight => editor::handle_cursor_right(app),
        Msg::EditorCursorHome => editor::handle_cursor_home(app),
        Msg::EditorCursorEnd => editor::handle_cursor_end(app),
        Msg::EditorPageUp => editor::handle_page_up(app),
        Msg::EditorPageDown => editor::handle_page_down(app),
        Msg::EditorSave => editor::handle_save(app),
        Msg::EditorTabNext => editor::handle_tab_next(app),
        Msg::EditorTabPrev => editor::handle_tab_prev(app),
        Msg::EditorTabClose => editor::handle_tab_close(app),
        Msg::EditorTreeToggle => editor::handle_tree_toggle(app),
        Msg::EditorFocusToggle => editor::handle_focus_toggle(app),
        Msg::EditorTreeExpand => editor::handle_tree_expand(app),
        Msg::EditorCut => editor::handle_cut(app),
        Msg::EditorCopy => editor::handle_copy(app),
        Msg::EditorPaste => editor::handle_paste(app),
        Msg::EditorNewFileStart => editor::handle_new_file_start(app),
        Msg::EditorRenameStart => editor::handle_rename_start(app),
        Msg::EditorDeleteStart => editor::handle_delete_start(app),
        Msg::EditorConfirmDelete(confirmed) => editor::handle_confirm_delete(app, confirmed),
        Msg::EditorModalCancel => editor::handle_modal_cancel(app),
        Msg::EditorRefreshTree => editor::handle_refresh_tree(app),
        Msg::EditorAutosaveTick => editor::handle_autosave_tick(app),
        Msg::EditorScrollTree(h) => editor::handle_scroll_tree(app, h),

        Msg::Quit => app.should_quit = true,
        Msg::Tick => api::handle_tick(app),
    }
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions may panic on failure")]
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
        app.interaction.selected_message = Some(0);
        update(&mut app, Msg::CharInput('a')).await;
        assert!(app.interaction.selected_message.is_none());
    }

    #[tokio::test]
    async fn resize_does_not_panic() {
        let mut app = test_app();
        update(&mut app, Msg::Resize(120, 40)).await;
    }

    #[tokio::test]
    async fn dismiss_error_clears_toast() {
        let mut app = test_app();
        app.viewport.error_toast = Some(crate::msg::ErrorToast::new("oops".to_string()));
        update(&mut app, Msg::DismissError).await;
        assert!(app.viewport.error_toast.is_none());
    }

    #[tokio::test]
    async fn show_error_sets_toast() {
        let mut app = test_app();
        update(&mut app, Msg::ShowError("bad".to_string())).await;
        assert!(app.viewport.error_toast.is_some());
    }

    #[tokio::test]
    async fn show_success_sets_success_toast_not_error_toast() {
        let mut app = test_app();
        update(&mut app, Msg::ShowSuccess("done".to_string())).await;
        assert!(app.viewport.success_toast.is_some());
        assert!(app.viewport.error_toast.is_none());
    }

    #[tokio::test]
    async fn show_success_message_stored() {
        let mut app = test_app();
        update(&mut app, Msg::ShowSuccess("saved".to_string())).await;
        assert_eq!(
            app.viewport.success_toast.as_ref().unwrap().message,
            "saved"
        );
    }

    #[tokio::test]
    async fn metrics_close_does_not_panic() {
        let mut app = test_app();
        update(&mut app, Msg::MetricsClose).await;
    }

    #[tokio::test]
    async fn metrics_select_up_does_not_panic() {
        let mut app = test_app();
        update(&mut app, Msg::MetricsSelectUp).await;
    }

    #[tokio::test]
    async fn metrics_select_down_does_not_panic() {
        let mut app = test_app();
        update(&mut app, Msg::MetricsSelectDown).await;
    }

    #[tokio::test]
    async fn metrics_health_loaded_sets_flag() {
        let mut app = test_app();
        update(&mut app, Msg::MetricsHealthLoaded(true)).await;
        assert_eq!(app.layout.metrics.api_healthy, Some(true));
    }
}
