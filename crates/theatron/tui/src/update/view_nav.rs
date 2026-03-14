//! View stack navigation handlers — drill-in (Enter) and pop-back (Esc).

use crate::app::App;
use crate::state::SavedScrollState;
use crate::state::view_stack::View;

/// Save the current scroll state keyed by the view stack depth before pushing.
fn save_view_scroll(app: &mut App) {
    let depth = app.view_stack.depth();
    app.view_scroll_states.insert(
        depth,
        SavedScrollState {
            scroll_offset: app.scroll_offset,
            auto_scroll: app.auto_scroll,
        },
    );
}

/// Restore scroll state for the current view stack depth after popping.
fn restore_view_scroll(app: &mut App) {
    let depth = app.view_stack.depth();
    if let Some(state) = app.view_scroll_states.remove(&depth) {
        app.scroll_offset = state.scroll_offset;
        app.auto_scroll = state.auto_scroll;
    } else {
        app.scroll_to_bottom();
    }
}

/// Handle Enter — drill into a detail view based on the current context.
///
/// The drill-in target depends on the current view:
/// - Home + agent sidebar focused → Sessions for that agent
/// - Home + message selected → MessageDetail
/// - Sessions + session selected → Conversation
/// - Conversation + message selected → MessageDetail
pub(crate) fn handle_drill_in(app: &mut App) {
    let current = app.view_stack.current().clone();

    match current {
        View::Home => {
            if let Some(idx) = app.selected_message {
                save_view_scroll(app);
                app.view_stack
                    .push(View::MessageDetail { message_index: idx });
                app.scroll_offset = 0;
                app.auto_scroll = true;
                return;
            }

            let agent_id = app.focused_agent.clone();
            if let Some(agent_id) = agent_id {
                save_view_scroll(app);
                app.view_stack.push(View::Sessions { agent_id });
                app.scroll_offset = 0;
                app.auto_scroll = true;
            }
        }
        View::Sessions { ref agent_id } => {
            let agent_id = agent_id.clone();
            let session_id = app.focused_session_id.clone();
            if let Some(session_id) = session_id {
                save_view_scroll(app);
                app.view_stack.push(View::Conversation {
                    agent_id,
                    session_id,
                });
                app.scroll_offset = 0;
                app.auto_scroll = true;
            }
        }
        View::Conversation { .. } => {
            if let Some(idx) = app.selected_message {
                save_view_scroll(app);
                app.view_stack
                    .push(View::MessageDetail { message_index: idx });
                app.scroll_offset = 0;
                app.auto_scroll = true;
            }
        }
        View::MessageDetail { .. } | View::MemoryInspector | View::FactDetail { .. } => {}
    }
}

/// Handle Esc — pop back to the previous view.
///
/// At Home, Esc deselects the message (existing behavior) or does nothing.
pub(crate) fn handle_pop_back(app: &mut App) {
    if app.view_stack.is_home() {
        if app.selected_message.is_some() {
            app.selected_message = None;
        }
        return;
    }

    app.view_stack.pop();
    restore_view_scroll(app);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::test_helpers::*;

    #[test]
    fn drill_in_from_home_with_agent_pushes_sessions() {
        let mut app = test_app();
        app.agents.push(test_agent("syn", "Syn"));
        app.focused_agent = Some("syn".into());

        handle_drill_in(&mut app);

        assert_eq!(
            app.view_stack.current(),
            &View::Sessions {
                agent_id: "syn".into()
            }
        );
        assert_eq!(app.view_stack.depth(), 2);
    }

    #[test]
    fn drill_in_from_home_with_selected_message_pushes_detail() {
        let mut app = test_app_with_messages(vec![("user", "hello"), ("assistant", "hi")]);
        app.selected_message = Some(1);

        handle_drill_in(&mut app);

        assert_eq!(
            app.view_stack.current(),
            &View::MessageDetail { message_index: 1 }
        );
    }

    #[test]
    fn drill_in_from_home_no_agent_is_noop() {
        let mut app = test_app();
        handle_drill_in(&mut app);
        assert!(app.view_stack.is_home());
    }

    #[test]
    fn drill_in_from_sessions_with_session_pushes_conversation() {
        let mut app = test_app();
        app.agents.push(test_agent("syn", "Syn"));
        app.focused_agent = Some("syn".into());
        app.focused_session_id = Some("sess1".into());
        app.view_stack.push(View::Sessions {
            agent_id: "syn".into(),
        });

        handle_drill_in(&mut app);

        assert_eq!(
            app.view_stack.current(),
            &View::Conversation {
                agent_id: "syn".into(),
                session_id: "sess1".into(),
            }
        );
    }

    #[test]
    fn drill_in_from_conversation_with_selected_message() {
        let mut app = test_app_with_messages(vec![("user", "hello")]);
        app.selected_message = Some(0);
        app.view_stack.push(View::Conversation {
            agent_id: "syn".into(),
            session_id: "s1".into(),
        });

        handle_drill_in(&mut app);

        assert_eq!(
            app.view_stack.current(),
            &View::MessageDetail { message_index: 0 }
        );
    }

    #[test]
    fn drill_in_from_leaf_is_noop() {
        let mut app = test_app();
        app.view_stack
            .push(View::MessageDetail { message_index: 0 });
        let depth_before = app.view_stack.depth();

        handle_drill_in(&mut app);

        assert_eq!(app.view_stack.depth(), depth_before);
    }

    #[test]
    fn pop_back_from_sessions_returns_home() {
        let mut app = test_app();
        app.view_stack.push(View::Sessions {
            agent_id: "syn".into(),
        });

        handle_pop_back(&mut app);

        assert!(app.view_stack.is_home());
    }

    #[test]
    fn pop_back_at_home_deselects_message() {
        let mut app = test_app_with_messages(vec![("user", "a")]);
        app.selected_message = Some(0);

        handle_pop_back(&mut app);

        assert!(app.selected_message.is_none());
        assert!(app.view_stack.is_home());
    }

    #[test]
    fn pop_back_at_home_no_selection_is_noop() {
        let mut app = test_app();
        handle_pop_back(&mut app);
        assert!(app.view_stack.is_home());
    }

    #[test]
    fn scroll_state_preserved_across_drill_and_pop() {
        let mut app = test_app();
        app.agents.push(test_agent("syn", "Syn"));
        app.focused_agent = Some("syn".into());
        app.scroll_offset = 42;
        app.auto_scroll = false;

        handle_drill_in(&mut app);
        assert_eq!(app.scroll_offset, 0);
        assert!(app.auto_scroll);

        handle_pop_back(&mut app);
        assert_eq!(app.scroll_offset, 42);
        assert!(!app.auto_scroll);
    }

    #[test]
    fn scroll_state_multi_level_preservation() {
        let mut app = test_app();
        app.agents.push(test_agent("syn", "Syn"));
        app.focused_agent = Some("syn".into());
        app.focused_session_id = Some("s1".into());
        app.scroll_offset = 10;
        app.auto_scroll = false;

        // Home → Sessions
        handle_drill_in(&mut app);
        app.scroll_offset = 20;
        app.auto_scroll = false;

        // Sessions → Conversation
        handle_drill_in(&mut app);
        app.scroll_offset = 30;

        // Pop Conversation → Sessions
        handle_pop_back(&mut app);
        assert_eq!(app.scroll_offset, 20);
        assert!(!app.auto_scroll);

        // Pop Sessions → Home
        handle_pop_back(&mut app);
        assert_eq!(app.scroll_offset, 10);
        assert!(!app.auto_scroll);
    }
}
