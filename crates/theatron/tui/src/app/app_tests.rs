#[cfg(test)]
#[expect(
    clippy::indexing_slicing,
    reason = "test assertions use direct indexing for clarity"
)]
mod tests {
    use super::super::test_helpers::*;
    use super::super::{DEFAULT_TERMINAL_HEIGHT, DEFAULT_TERMINAL_WIDTH};
    use crate::state::{ChatMessage, OpsState};

    #[test]
    fn app_constructs_with_defaults() {
        let app = test_app();
        assert!(!app.should_quit);
        assert!(app.viewport.render.auto_scroll);
        assert!(app.layout.sidebar_visible);
        assert!(!app.layout.thinking_expanded);
        assert!(app.layout.overlay.is_none());
        assert!(app.dashboard.messages.is_empty());
        assert!(app.dashboard.agents.is_empty());
        assert_eq!(app.viewport.render.scroll_offset, 0);
        assert_eq!(app.viewport.terminal_width, DEFAULT_TERMINAL_WIDTH);
        assert_eq!(app.viewport.terminal_height, DEFAULT_TERMINAL_HEIGHT);
        assert!(!app.connection.sse_connected);
        assert!(app.connection.sse_disconnected_at.is_none());
    }

    #[test]
    fn app_with_messages_populates_dashboard_correctly() {
        let app = test_app_with_messages(vec![("user", "hello"), ("assistant", "hi there")]);
        assert_eq!(app.dashboard.messages.len(), 2);
        assert_eq!(app.dashboard.messages[0].role, "user");
        assert_eq!(app.dashboard.messages[1].text, "hi there");
    }

    #[test]
    fn markdown_cache_fields_exist_for_session_switch_clearing() {
        // Verifies that the fields cleared on session switch are present and
        // behave as expected when the caller clears them.
        let mut app = test_app();
        app.viewport.render.markdown_cache.text = "stale content from previous session".to_string();
        app.viewport.render.markdown_cache.lines = vec![ratatui::text::Line::raw("stale line")];

        // Simulate the clearing that load_focused_session performs on history load.
        app.viewport.render.markdown_cache.clear();

        assert!(
            app.viewport.render.markdown_cache.text.is_empty(),
            "markdown text cache must be cleared on session switch"
        );
        assert!(
            app.viewport.render.markdown_cache.lines.is_empty(),
            "markdown line cache must be cleared on session switch"
        );
    }

    #[test]
    fn take_restore_sse_roundtrip() {
        let mut app = test_app();
        assert!(app.take_sse().is_none());
        app.restore_sse(None);
    }

    #[test]
    fn take_restore_stream_roundtrip() {
        let mut app = test_app();
        assert!(app.take_stream().is_none());
        app.restore_stream(None);
    }

    #[test]
    fn tab_state_save_restore_roundtrip() {
        let mut app = test_app();
        let agent = test_agent("syn", "Syn");
        let agent_id = agent.id.clone();
        app.dashboard.agents.push(agent);
        app.dashboard.focused_agent = Some(agent_id.clone());

        // Create two tabs
        let idx0 = app.layout.tab_bar.create_tab(agent_id.clone(), "tab0");
        app.layout.tab_bar.active = idx0;

        // Set up state in tab0
        app.dashboard.messages = vec![ChatMessage {
            role: "user".to_string(),
            text: "hello from tab0".to_string(),
            text_lower: "hello from tab0".to_string(),
            timestamp: None,
            model: None,
            tool_calls: Vec::new(),
        }]
        .into();
        app.viewport.render.scroll_offset = 42;
        app.viewport.render.auto_scroll = false;
        app.interaction.input.text = "typing in tab0".to_string();
        app.layout.ops.thinking.text = "thinking in tab0".to_string();
        app.layout
            .ops
            .push_tool_start("read_file".to_string(), None);
        app.save_to_active_tab();

        // Create tab1 with different state
        let idx1 = app.layout.tab_bar.create_tab(agent_id, "tab1");
        app.layout.tab_bar.active = idx1;
        app.dashboard.messages = vec![ChatMessage {
            role: "assistant".to_string(),
            text: "hello from tab1".to_string(),
            text_lower: "hello from tab1".to_string(),
            timestamp: None,
            model: None,
            tool_calls: Vec::new(),
        }]
        .into();
        app.viewport.render.scroll_offset = 10;
        app.viewport.render.auto_scroll = true;
        app.interaction.input.text = "typing in tab1".to_string();
        app.layout.ops = OpsState::default();
        app.save_to_active_tab();

        // Switch back to tab0 and verify state restored
        app.layout.tab_bar.active = idx0;
        app.restore_from_active_tab();

        assert_eq!(app.dashboard.messages.len(), 1);
        assert_eq!(app.dashboard.messages[0].text, "hello from tab0");
        assert_eq!(app.viewport.render.scroll_offset, 42);
        assert!(!app.viewport.render.auto_scroll);
        assert_eq!(app.interaction.input.text, "typing in tab0");
        assert_eq!(app.layout.ops.thinking.text, "thinking in tab0");
        assert_eq!(app.layout.ops.tool_calls.len(), 1);
        assert_eq!(app.layout.ops.tool_calls[0].name, "read_file");

        // Switch to tab1 and verify its state
        app.save_to_active_tab();
        app.layout.tab_bar.active = idx1;
        app.restore_from_active_tab();

        assert_eq!(app.dashboard.messages.len(), 1);
        assert_eq!(app.dashboard.messages[0].text, "hello from tab1");
        assert_eq!(app.viewport.render.scroll_offset, 10);
        assert!(app.viewport.render.auto_scroll);
        assert_eq!(app.interaction.input.text, "typing in tab1");
        assert!(app.layout.ops.thinking.text.is_empty());
        assert!(app.layout.ops.tool_calls.is_empty());
    }

    #[test]
    fn tab_switch_messages_copy_on_write_isolated() {
        // After save_to_active_tab, the tab and the app share Arc storage.
        // A push to app.dashboard.messages triggers COW: the tab's snapshot is unaffected.
        let mut app = test_app_with_messages(vec![("user", "hello"), ("assistant", "world")]);
        let agent = test_agent("syn", "Syn");
        let agent_id = agent.id.clone();
        app.dashboard.agents.push(agent);
        app.dashboard.focused_agent = Some(agent_id.clone());

        let idx0 = app.layout.tab_bar.create_tab(agent_id, "tab0");
        app.layout.tab_bar.active = idx0;
        app.save_to_active_tab();

        // Snapshot: 2 messages in both app and tab.
        assert_eq!(app.dashboard.messages.len(), 2);
        assert_eq!(app.layout.tab_bar.tabs[0].state.messages.len(), 2);

        // Mutation diverges app from the saved snapshot.
        app.dashboard.messages.push(ChatMessage {
            role: "user".to_string(),
            text: "new".to_string(),
            text_lower: "new".to_string(),
            timestamp: None,
            model: None,
            tool_calls: Vec::new(),
        });

        // App grew; tab snapshot is unchanged (COW semantics).
        assert_eq!(app.dashboard.messages.len(), 3);
        assert_eq!(
            app.layout.tab_bar.tabs[0].state.messages.len(),
            2,
            "tab snapshot must not be affected by app mutation"
        );
    }

    #[test]
    fn dirty_starts_true_so_first_frame_renders() {
        let app = test_app();
        assert!(
            app.viewport.dirty,
            "new App must be dirty so first frame renders"
        );
    }
}
