use crate::app::App;
use crate::id::NousId;

pub(crate) fn handle_scroll_up(app: &mut App) {
    app.scroll_offset = app.scroll_offset.saturating_add(3);
    app.auto_scroll = false;
    clamp_scroll_offset(app);
}

pub(crate) fn handle_scroll_down(app: &mut App) {
    if app.scroll_offset >= 3 {
        app.scroll_offset -= 3;
    } else {
        app.scroll_offset = 0;
        app.auto_scroll = true;
    }
}

/// Approximate chat viewport height from the current terminal height.
///
/// Subtracts fixed chrome: title bar (1) + status bar (2) + minimum input area (3).
/// The result is used as the page-scroll distance so PageUp/PageDown move by one
/// viewport rather than a hard-coded constant.
fn chat_viewport_height(app: &App) -> usize {
    app.terminal_height.saturating_sub(6).max(1) as usize
}

pub(crate) fn handle_scroll_page_up(app: &mut App) {
    let page = chat_viewport_height(app);
    app.scroll_offset = app.scroll_offset.saturating_add(page);
    app.auto_scroll = false;
    clamp_scroll_offset(app);
}

pub(crate) fn handle_scroll_page_down(app: &mut App) {
    let page = chat_viewport_height(app);
    if app.scroll_offset >= page {
        app.scroll_offset -= page;
    } else {
        app.scroll_offset = 0;
        app.auto_scroll = true;
    }
}

pub(crate) fn handle_scroll_to_bottom(app: &mut App) {
    app.scroll_offset = 0;
    app.auto_scroll = true;
}

pub(crate) async fn handle_focus_agent(app: &mut App, id: NousId) {
    app.save_scroll_state();
    if let Some(agent) = app.agents.iter_mut().find(|a| a.id == id) {
        agent.has_notification = false;
    }
    app.focused_agent = Some(id);
    app.load_focused_session().await;
    app.restore_scroll_state();
}

pub(crate) async fn handle_next_agent(app: &mut App) {
    if app.agents.is_empty() {
        return;
    }
    app.save_scroll_state();
    if let Some(ref current) = app.focused_agent
        && let Some(idx) = app.agents.iter().position(|a| a.id == *current)
    {
        let next = (idx + 1) % app.agents.len();
        let id = app.agents[next].id.clone();
        app.focused_agent = Some(id);
        app.load_focused_session().await;
        app.restore_scroll_state();
    }
}

pub(crate) async fn handle_prev_agent(app: &mut App) {
    if app.agents.is_empty() {
        return;
    }
    app.save_scroll_state();
    if let Some(ref current) = app.focused_agent
        && let Some(idx) = app.agents.iter().position(|a| a.id == *current)
    {
        let prev = if idx == 0 {
            app.agents.len() - 1
        } else {
            idx - 1
        };
        let id = app.agents[prev].id.clone();
        app.focused_agent = Some(id);
        app.load_focused_session().await;
        app.restore_scroll_state();
    }
}

pub(crate) fn handle_toggle_sidebar(app: &mut App) {
    app.sidebar_visible = !app.sidebar_visible;
}

pub(crate) fn handle_toggle_thinking(app: &mut App) {
    app.thinking_expanded = !app.thinking_expanded;
}

pub(crate) fn handle_toggle_ops_pane(app: &mut App) {
    app.ops.toggle();
}

pub(crate) fn handle_ops_focus_switch(app: &mut App) {
    app.ops.toggle_focus();
}

/// Clamp `scroll_offset` to the maximum scrollable distance given current content
/// and viewport height. When messages are present and the offset would place the
/// viewport past the top of the content, it is reduced to the valid maximum.
/// If the clamped offset reaches zero, auto-scroll is re-enabled.
///
/// No-op when there are no messages (nothing to bound against) or when
/// auto-scroll is already active.
fn clamp_scroll_offset(app: &mut App) {
    if app.auto_scroll || app.messages.is_empty() {
        return;
    }
    let total = app.virtual_scroll.total_height();
    let vh = chat_viewport_height(app) as u64;
    let max_offset = total.saturating_sub(vh) as usize;
    if app.scroll_offset > max_offset {
        app.scroll_offset = max_offset;
        if max_offset == 0 {
            app.auto_scroll = true;
        }
    }
}

pub(crate) fn handle_resize(app: &mut App, w: u16, h: u16) {
    app.terminal_width = w;
    app.terminal_height = h;
    // Terminal width changed — recalculate message heights for wrapping.
    app.rebuild_virtual_scroll();
    // After rebuild, heights may have changed due to reflow. Cap scroll_offset so
    // the user cannot be stranded past the top of content. If already at the
    // bottom, stay there. Auto-scroll mode is unaffected (it always pins to bottom).
    if !app.auto_scroll {
        let total = app.virtual_scroll.total_height();
        let vh = chat_viewport_height(app) as u64;
        let max_offset = total.saturating_sub(vh) as usize;
        app.scroll_offset = app.scroll_offset.min(max_offset);
        if app.scroll_offset == 0 {
            app.auto_scroll = true;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::test_helpers::*;

    #[test]
    fn scroll_up_increases_offset() {
        let mut app = test_app();
        handle_scroll_up(&mut app);
        assert_eq!(app.scroll_offset, 3);
        assert!(!app.auto_scroll);
    }

    #[test]
    fn scroll_up_accumulates() {
        let mut app = test_app();
        handle_scroll_up(&mut app);
        handle_scroll_up(&mut app);
        assert_eq!(app.scroll_offset, 6);
    }

    #[test]
    fn scroll_down_decreases_offset() {
        let mut app = test_app();
        app.scroll_offset = 10;
        app.auto_scroll = false;
        handle_scroll_down(&mut app);
        assert_eq!(app.scroll_offset, 7);
        assert!(!app.auto_scroll);
    }

    #[test]
    fn scroll_down_to_zero_enables_auto_scroll() {
        let mut app = test_app();
        app.scroll_offset = 2;
        app.auto_scroll = false;
        handle_scroll_down(&mut app);
        assert_eq!(app.scroll_offset, 0);
        assert!(app.auto_scroll);
    }

    #[test]
    fn scroll_page_up_jumps_one_viewport() {
        let mut app = test_app();
        // test_app terminal_height=40 → chat_viewport_height = 40-6 = 34
        let expected = chat_viewport_height(&app);
        handle_scroll_page_up(&mut app);
        assert_eq!(app.scroll_offset, expected);
        assert!(!app.auto_scroll);
    }

    #[test]
    fn scroll_page_down_jumps_one_viewport() {
        let mut app = test_app();
        let page = chat_viewport_height(&app);
        app.scroll_offset = page + 10;
        handle_scroll_page_down(&mut app);
        assert_eq!(app.scroll_offset, 10);
    }

    #[test]
    fn scroll_page_down_to_zero_auto_scrolls() {
        let mut app = test_app();
        app.scroll_offset = 5; // less than a full page
        app.auto_scroll = false;
        handle_scroll_page_down(&mut app);
        assert_eq!(app.scroll_offset, 0);
        assert!(app.auto_scroll);
    }

    #[test]
    fn scroll_to_bottom_resets() {
        let mut app = test_app();
        app.scroll_offset = 50;
        app.auto_scroll = false;
        handle_scroll_to_bottom(&mut app);
        assert_eq!(app.scroll_offset, 0);
        assert!(app.auto_scroll);
    }

    #[test]
    fn toggle_sidebar_flips() {
        let mut app = test_app();
        assert!(app.sidebar_visible);
        handle_toggle_sidebar(&mut app);
        assert!(!app.sidebar_visible);
        handle_toggle_sidebar(&mut app);
        assert!(app.sidebar_visible);
    }

    #[test]
    fn toggle_thinking_flips() {
        let mut app = test_app();
        assert!(!app.thinking_expanded);
        handle_toggle_thinking(&mut app);
        assert!(app.thinking_expanded);
        handle_toggle_thinking(&mut app);
        assert!(!app.thinking_expanded);
    }

    #[test]
    fn resize_updates_dimensions() {
        let mut app = test_app();
        handle_resize(&mut app, 200, 50);
        assert_eq!(app.terminal_width, 200);
        assert_eq!(app.terminal_height, 50);
    }

    #[test]
    fn scroll_up_clamped_to_content_height_with_messages() {
        use crate::app::test_helpers::test_app_with_messages;
        // Two short messages, very tall terminal — content fits, max_offset = 0.
        let mut app = test_app_with_messages(vec![("user", "hi"), ("assistant", "hey")]);
        app.rebuild_virtual_scroll();
        app.auto_scroll = false;
        // Scrolling up when content fits should clamp back to 0.
        handle_scroll_up(&mut app);
        assert_eq!(app.scroll_offset, 0);
        assert!(
            app.auto_scroll,
            "should re-enable auto-scroll when clamped to 0"
        );
    }

    #[test]
    fn scroll_up_does_not_clamp_empty_messages() {
        // Without messages the clamp must not activate — existing behaviour preserved.
        let mut app = test_app();
        handle_scroll_up(&mut app);
        assert_eq!(app.scroll_offset, 3);
        assert!(!app.auto_scroll);
    }

    #[test]
    fn scroll_page_up_clamped_to_content_height_with_messages() {
        use crate::app::test_helpers::test_app_with_messages;
        let mut app = test_app_with_messages(vec![("user", "hi"), ("assistant", "hey")]);
        app.rebuild_virtual_scroll();
        app.auto_scroll = false;
        handle_scroll_page_up(&mut app);
        // Content fits in the tall test terminal — offset must be clamped to 0.
        assert_eq!(app.scroll_offset, 0);
        assert!(app.auto_scroll);
    }

    #[test]
    fn scroll_up_preserves_valid_offset_within_content() {
        use crate::app::test_helpers::test_app_with_messages;
        // Build enough messages to exceed the test viewport (40 rows).
        let msgs: Vec<_> = (0..30)
            .map(|_| ("user", "hello world long enough line"))
            .collect();
        let mut app = test_app_with_messages(msgs);
        app.rebuild_virtual_scroll();
        app.auto_scroll = false;
        let total = app.virtual_scroll.total_height();
        let vh = chat_viewport_height(&app) as u64;
        // Offset within valid range: no clamping expected.
        if total > vh {
            app.scroll_offset = 3;
            handle_scroll_up(&mut app);
            // Offset should be 6 (3 + 3) as long as content allows.
            assert!(app.scroll_offset <= (total.saturating_sub(vh) as usize));
            assert!(!app.auto_scroll);
        }
    }

    #[test]
    fn resize_clamps_scroll_offset_when_content_shrinks() {
        use crate::app::test_helpers::test_app_with_messages;
        let mut app = test_app_with_messages(vec![("user", "hi"), ("assistant", "hello")]);
        app.rebuild_virtual_scroll();
        // Scroll up to a large offset that will exceed content after resize
        app.scroll_offset = 9999;
        app.auto_scroll = false;
        // Resize to a very tall terminal — content now fits, offset must be clamped to 0
        handle_resize(&mut app, 120, 200);
        assert_eq!(app.scroll_offset, 0);
        assert!(app.auto_scroll);
    }

    #[test]
    fn resize_preserves_valid_scroll_offset() {
        use crate::app::test_helpers::test_app_with_messages;
        let msgs: Vec<_> = (0..50)
            .map(|i| {
                (
                    "user",
                    if i % 2 == 0 {
                        "hello world"
                    } else {
                        "response"
                    },
                )
            })
            .collect();
        let mut app = test_app_with_messages(msgs);
        app.rebuild_virtual_scroll();
        app.scroll_offset = 5;
        app.auto_scroll = false;
        // Resize with same height — offset within valid range should be preserved
        handle_resize(&mut app, 120, 40);
        assert!(!app.auto_scroll);
        assert_eq!(app.scroll_offset, 5);
    }

    #[tokio::test]
    async fn next_agent_empty_list_is_noop() {
        let mut app = test_app();
        assert!(app.agents.is_empty());
        handle_next_agent(&mut app).await;
        // No panic, no state change
        assert!(app.focused_agent.is_none());
    }

    #[tokio::test]
    async fn prev_agent_empty_list_is_noop() {
        let mut app = test_app();
        assert!(app.agents.is_empty());
        handle_prev_agent(&mut app).await;
        // No panic, no state change
        assert!(app.focused_agent.is_none());
    }
}
