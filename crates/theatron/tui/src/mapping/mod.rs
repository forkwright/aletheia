//! Event-to-Msg translation: maps terminal, SSE, and stream events to application messages.

mod keyboard;
mod streams;

#[cfg(test)]
mod tests {
    use crate::api::types::SseEvent;
    use crate::app::test_helpers::*;
    use crate::events::{Event, StreamEvent};
    use crate::msg::{MessageActionKind, Msg, OverlayKind};
    use crate::state::Overlay;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    fn key(code: KeyCode) -> crossterm::event::Event {
        crossterm::event::Event::Key(KeyEvent::new(code, KeyModifiers::NONE))
    }

    fn key_mod(code: KeyCode, mods: KeyModifiers) -> crossterm::event::Event {
        crossterm::event::Event::Key(KeyEvent::new(code, mods))
    }

    #[test]
    fn tick_event_maps_to_tick() {
        let app = test_app();
        let msg = app.map_event(Event::Tick);
        assert!(matches!(msg, Some(Msg::Tick)));
    }

    #[test]
    fn ctrl_c_maps_to_quit() {
        let app = test_app();
        let event = Event::Terminal(key_mod(KeyCode::Char('c'), KeyModifiers::CONTROL));
        let msg = app.map_event(event);
        assert!(matches!(msg, Some(Msg::Quit)));
    }

    #[test]
    fn ctrl_q_maps_to_quit() {
        let app = test_app();
        let event = Event::Terminal(key_mod(KeyCode::Char('q'), KeyModifiers::CONTROL));
        let msg = app.map_event(event);
        assert!(matches!(msg, Some(Msg::Quit)));
    }

    #[test]
    fn f1_maps_to_help() {
        let app = test_app();
        let event = Event::Terminal(key(KeyCode::F(1)));
        let msg = app.map_event(event);
        assert!(matches!(msg, Some(Msg::OpenOverlay(OverlayKind::Help))));
    }

    #[test]
    fn ctrl_f_toggles_sidebar() {
        let app = test_app();
        let event = Event::Terminal(key_mod(KeyCode::Char('f'), KeyModifiers::CONTROL));
        let msg = app.map_event(event);
        assert!(matches!(msg, Some(Msg::ToggleSidebar)));
    }

    #[test]
    fn enter_maps_to_submit() {
        let app = test_app();
        let event = Event::Terminal(key(KeyCode::Enter));
        let msg = app.map_event(event);
        assert!(matches!(msg, Some(Msg::Submit)));
    }

    #[test]
    fn question_mark_on_empty_input_opens_help() {
        let app = test_app();
        let event = Event::Terminal(key(KeyCode::Char('?')));
        let msg = app.map_event(event);
        assert!(matches!(msg, Some(Msg::OpenOverlay(OverlayKind::Help))));
    }

    #[test]
    fn question_mark_with_text_is_char_input() {
        let mut app = test_app();
        app.interaction.input.text = "hello".to_string();
        app.interaction.input.cursor = 5;
        let event = Event::Terminal(key(KeyCode::Char('?')));
        let msg = app.map_event(event);
        assert!(matches!(msg, Some(Msg::CharInput('?'))));
    }

    #[test]
    fn colon_on_empty_input_opens_command_palette() {
        let app = test_app();
        let event = Event::Terminal(key_mod(KeyCode::Char(':'), KeyModifiers::SHIFT));
        let msg = app.map_event(event);
        assert!(matches!(msg, Some(Msg::CommandPaletteOpen)));
    }

    #[test]
    fn slash_on_empty_input_opens_session_search() {
        let app = test_app();
        let event = Event::Terminal(key(KeyCode::Char('/')));
        let msg = app.map_event(event);
        assert!(matches!(msg, Some(Msg::SessionSearchOpen)));
    }

    #[test]
    fn up_with_empty_input_and_messages_selects() {
        let mut app = test_app();
        app.dashboard.messages.push(crate::state::ChatMessage {
            role: "user".to_string(),
            text: "hi".to_string(),
            text_lower: "hi".to_string(),
            timestamp: None,
            model: None,
            tool_calls: Vec::new(),
        });
        let event = Event::Terminal(key(KeyCode::Up));
        let msg = app.map_event(event);
        assert!(matches!(msg, Some(Msg::SelectPrev)));
    }

    #[test]
    fn up_with_text_navigates_history() {
        let mut app = test_app();
        app.interaction.input.text = "some text".to_string();
        app.interaction.input.cursor = 9;
        let event = Event::Terminal(key(KeyCode::Up));
        let msg = app.map_event(event);
        assert!(matches!(msg, Some(Msg::HistoryUp)));
    }

    #[test]
    fn resize_event_maps_to_resize() {
        let app = test_app();
        let event = Event::Terminal(crossterm::event::Event::Resize(80, 24));
        let msg = app.map_event(event);
        assert!(matches!(msg, Some(Msg::Resize(80, 24))));
    }

    #[test]
    fn selection_mode_j_moves_next() {
        let mut app = test_app_with_messages(vec![("user", "a"), ("assistant", "b")]);
        app.interaction.selected_message = Some(0);
        let event = Event::Terminal(key(KeyCode::Char('j')));
        let msg = app.map_event(event);
        assert!(matches!(msg, Some(Msg::SelectNext)));
    }

    #[test]
    fn selection_mode_k_moves_prev() {
        let mut app = test_app_with_messages(vec![("user", "a"), ("assistant", "b")]);
        app.interaction.selected_message = Some(1);
        let event = Event::Terminal(key(KeyCode::Char('k')));
        let msg = app.map_event(event);
        assert!(matches!(msg, Some(Msg::SelectPrev)));
    }

    #[test]
    fn selection_mode_esc_deselects() {
        let mut app = test_app_with_messages(vec![("user", "a")]);
        app.interaction.selected_message = Some(0);
        let event = Event::Terminal(key(KeyCode::Esc));
        let msg = app.map_event(event);
        assert!(matches!(msg, Some(Msg::DeselectMessage)));
    }

    #[test]
    fn selection_mode_c_copies() {
        let mut app = test_app_with_messages(vec![("user", "a")]);
        app.interaction.selected_message = Some(0);
        let event = Event::Terminal(key(KeyCode::Char('c')));
        let msg = app.map_event(event);
        assert!(matches!(
            msg,
            Some(Msg::MessageAction(MessageActionKind::Copy))
        ));
    }

    #[test]
    fn palette_esc_closes() {
        let mut app = test_app();
        app.interaction.command_palette.active = true;
        let event = Event::Terminal(key(KeyCode::Esc));
        let msg = app.map_event(event);
        assert!(matches!(msg, Some(Msg::CommandPaletteClose)));
    }

    #[test]
    fn palette_enter_selects() {
        let mut app = test_app();
        app.interaction.command_palette.active = true;
        let event = Event::Terminal(key(KeyCode::Enter));
        let msg = app.map_event(event);
        assert!(matches!(msg, Some(Msg::CommandPaletteSelect)));
    }

    #[test]
    fn palette_char_inputs() {
        let mut app = test_app();
        app.interaction.command_palette.active = true;
        let event = Event::Terminal(key(KeyCode::Char('a')));
        let msg = app.map_event(event);
        assert!(matches!(msg, Some(Msg::CommandPaletteInput('a'))));
    }

    #[test]
    fn filter_editing_esc_closes() {
        let mut app = test_app();
        app.interaction.filter.active = true;
        app.interaction.filter.editing = true;
        let event = Event::Terminal(key(KeyCode::Esc));
        let msg = app.map_event(event);
        assert!(matches!(msg, Some(Msg::FilterClose)));
    }

    #[test]
    fn filter_editing_enter_confirms() {
        let mut app = test_app();
        app.interaction.filter.active = true;
        app.interaction.filter.editing = true;
        let event = Event::Terminal(key(KeyCode::Enter));
        let msg = app.map_event(event);
        assert!(matches!(msg, Some(Msg::FilterConfirm)));
    }

    #[test]
    fn filter_editing_char_inputs() {
        let mut app = test_app();
        app.interaction.filter.active = true;
        app.interaction.filter.editing = true;
        let event = Event::Terminal(key(KeyCode::Char('x')));
        let msg = app.map_event(event);
        assert!(matches!(msg, Some(Msg::FilterInput('x'))));
    }

    #[test]
    fn filter_applied_n_next_match() {
        let mut app = test_app();
        app.interaction.filter.active = true;
        app.interaction.filter.editing = false;
        app.interaction.filter.text = "search".to_string();
        let event = Event::Terminal(key(KeyCode::Char('n')));
        let msg = app.map_event(event);
        assert!(matches!(msg, Some(Msg::FilterNextMatch)));
    }

    #[test]
    fn overlay_esc_closes() {
        let mut app = test_app();
        app.layout.overlay = Some(Overlay::Help);
        let event = Event::Terminal(key(KeyCode::Esc));
        let msg = app.map_event(event);
        assert!(matches!(msg, Some(Msg::CloseOverlay)));
    }

    #[test]
    fn overlay_up_navigates() {
        let mut app = test_app();
        app.layout.overlay = Some(Overlay::AgentPicker { cursor: 0 });
        let event = Event::Terminal(key(KeyCode::Up));
        let msg = app.map_event(event);
        assert!(matches!(msg, Some(Msg::OverlayUp)));
    }

    #[test]
    fn sse_connected_maps() {
        let app = test_app();
        let event = Event::Sse(SseEvent::Connected);
        let msg = app.map_event(event);
        assert!(matches!(msg, Some(Msg::SseConnected)));
    }

    #[test]
    fn sse_ping_maps_to_tick() {
        let app = test_app();
        let event = Event::Sse(SseEvent::Ping);
        let msg = app.map_event(event);
        assert!(matches!(msg, Some(Msg::Tick)));
    }

    #[test]
    fn stream_text_delta_maps() {
        let app = test_app();
        let event = Event::Stream(StreamEvent::TextDelta("hello".to_string()));
        let msg = app.map_event(event);
        assert!(matches!(msg, Some(Msg::StreamTextDelta(_))));
    }

    #[test]
    fn stream_error_maps() {
        let app = test_app();
        let event = Event::Stream(StreamEvent::Error("oops".to_string()));
        let msg = app.map_event(event);
        assert!(matches!(msg, Some(Msg::StreamError(_))));
    }

    #[test]
    fn page_up_maps() {
        let app = test_app();
        let event = Event::Terminal(key(KeyCode::PageUp));
        let msg = app.map_event(event);
        assert!(matches!(msg, Some(Msg::ScrollPageUp)));
    }

    #[test]
    fn shift_up_scrolls_one_line() {
        let app = test_app();
        let event = Event::Terminal(key_mod(KeyCode::Up, KeyModifiers::SHIFT));
        let msg = app.map_event(event);
        assert!(matches!(msg, Some(Msg::ScrollLineUp)));
    }

    #[test]
    fn settings_overlay_up_down() {
        let mut app = test_app();
        let settings = crate::state::settings::SettingsOverlay::from_config(&serde_json::json!({}));
        app.layout.overlay = Some(Overlay::Settings(settings));
        let event = Event::Terminal(key(KeyCode::Up));
        let msg = app.map_event(event);
        assert!(matches!(msg, Some(Msg::OverlayUp)));
    }

    #[test]
    fn settings_overlay_s_key_saves() {
        let mut app = test_app();
        let settings = crate::state::settings::SettingsOverlay::from_config(&serde_json::json!({}));
        app.layout.overlay = Some(Overlay::Settings(settings));
        let event = Event::Terminal(key(KeyCode::Char('s')));
        let msg = app.map_event(event);
        assert!(matches!(msg, Some(Msg::OverlayFilter('s'))));
    }

    #[test]
    fn v_on_empty_input_with_messages_enters_selection() {
        let app = test_app_with_messages(vec![("user", "hello")]);
        let event = Event::Terminal(key(KeyCode::Char('v')));
        let msg = app.map_event(event);
        assert!(matches!(msg, Some(Msg::SelectPrev)));
    }

    #[test]
    fn v_on_empty_input_no_messages_is_char_input() {
        let app = test_app();
        let event = Event::Terminal(key(KeyCode::Char('v')));
        let msg = app.map_event(event);
        assert!(matches!(msg, Some(Msg::CharInput('v'))));
    }

    #[test]
    fn v_with_text_is_char_input() {
        let mut app = test_app_with_messages(vec![("user", "a")]);
        app.interaction.input.text = "typing".to_string();
        app.interaction.input.cursor = 6;
        let event = Event::Terminal(key(KeyCode::Char('v')));
        let msg = app.map_event(event);
        assert!(matches!(msg, Some(Msg::CharInput('v'))));
    }

    #[test]
    fn selection_mode_enter_drills_in() {
        let mut app = test_app_with_messages(vec![("user", "a")]);
        app.interaction.selected_message = Some(0);
        let event = Event::Terminal(key(KeyCode::Enter));
        let msg = app.map_event(event);
        assert!(matches!(msg, Some(Msg::ViewDrillIn)));
    }

    #[test]
    fn context_actions_overlay_j_moves_down() {
        let mut app = test_app();
        app.layout.overlay = Some(Overlay::ContextActions(
            crate::state::ContextActionsOverlay {
                actions: vec![crate::state::ContextAction {
                    label: "Copy",
                    kind: MessageActionKind::Copy,
                }],
                cursor: 0,
            },
        ));
        let event = Event::Terminal(key(KeyCode::Char('j')));
        let msg = app.map_event(event);
        assert!(matches!(msg, Some(Msg::OverlayDown)));
    }

    #[test]
    fn context_actions_overlay_k_moves_up() {
        let mut app = test_app();
        app.layout.overlay = Some(Overlay::ContextActions(
            crate::state::ContextActionsOverlay {
                actions: vec![crate::state::ContextAction {
                    label: "Copy",
                    kind: MessageActionKind::Copy,
                }],
                cursor: 0,
            },
        ));
        let event = Event::Terminal(key(KeyCode::Char('k')));
        let msg = app.map_event(event);
        assert!(matches!(msg, Some(Msg::OverlayUp)));
    }

    #[test]
    fn esc_at_non_home_view_pops_back() {
        let mut app = test_app();
        app.layout.view_stack.push(crate::state::View::Sessions {
            agent_id: "syn".into(),
        });
        let event = Event::Terminal(key(KeyCode::Esc));
        let msg = app.map_event(event);
        assert!(matches!(msg, Some(Msg::ViewPopBack)));
    }

    #[test]
    fn esc_at_home_with_selection_deselects() {
        let mut app = test_app_with_messages(vec![("user", "a")]);
        app.interaction.selected_message = Some(0);
        let event = Event::Terminal(key(KeyCode::Esc));
        let msg = app.map_event(event);
        assert!(matches!(msg, Some(Msg::DeselectMessage)));
    }

    #[test]
    fn esc_at_non_home_with_selection_still_pops() {
        let mut app = test_app_with_messages(vec![("user", "a")]);
        app.interaction.selected_message = Some(0);
        app.layout
            .view_stack
            .push(crate::state::View::MessageDetail { message_index: 0 });
        let event = Event::Terminal(key(KeyCode::Esc));
        let msg = app.map_event(event);
        assert!(matches!(msg, Some(Msg::ViewPopBack)));
    }

    #[test]
    fn end_key_on_empty_input_scrolls_to_bottom() {
        let app = test_app();
        let event = Event::Terminal(key(KeyCode::End));
        let msg = app.map_event(event);
        assert!(matches!(msg, Some(Msg::ScrollToBottom)));
    }

    #[test]
    fn end_key_with_text_moves_cursor_to_end() {
        let mut app = test_app();
        app.interaction.input.text = "hello".to_string();
        app.interaction.input.cursor = 0;
        let event = Event::Terminal(key(KeyCode::End));
        let msg = app.map_event(event);
        assert!(matches!(msg, Some(Msg::CursorEnd)));
    }

    #[test]
    fn tab_on_empty_input_with_no_ops_cycles_next_agent() {
        let app = test_app();
        let event = Event::Terminal(key(KeyCode::Tab));
        let msg = app.map_event(event);
        assert!(matches!(msg, Some(Msg::NextAgent)));
    }

    #[test]
    fn tab_with_at_mention_does_completion_not_agent_cycle() {
        let mut app = test_app();
        app.interaction.input.text = "@al".to_string();
        app.interaction.input.cursor = 3;
        let event = Event::Terminal(key(KeyCode::Tab));
        let msg = app.map_event(event);
        assert!(matches!(msg, Some(Msg::CharInput('\t'))));
    }

    #[test]
    fn shift_tab_cycles_prev_agent() {
        let app = test_app();
        let event = Event::Terminal(key(KeyCode::BackTab));
        let msg = app.map_event(event);
        assert!(matches!(msg, Some(Msg::PrevAgent)));
    }

    #[test]
    fn ctrl_backtab_switches_tab_prev_not_agent() {
        let app = test_app();
        let event = Event::Terminal(key_mod(KeyCode::BackTab, KeyModifiers::CONTROL));
        let msg = app.map_event(event);
        assert!(matches!(msg, Some(Msg::TabPrev)));
    }
}
