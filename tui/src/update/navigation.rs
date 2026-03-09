use crate::app::App;
use crate::id::NousId;

pub(crate) fn handle_scroll_up(app: &mut App) {
    app.scroll_offset = app.scroll_offset.saturating_add(3);
    app.auto_scroll = false;
}

pub(crate) fn handle_scroll_down(app: &mut App) {
    if app.scroll_offset >= 3 {
        app.scroll_offset -= 3;
    } else {
        app.scroll_offset = 0;
        app.auto_scroll = true;
    }
}

pub(crate) fn handle_scroll_page_up(app: &mut App) {
    app.scroll_offset = app.scroll_offset.saturating_add(20);
    app.auto_scroll = false;
}

pub(crate) fn handle_scroll_page_down(app: &mut App) {
    if app.scroll_offset >= 20 {
        app.scroll_offset -= 20;
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
    app.save_scroll_state();
    if let Some(ref current) = app.focused_agent {
        if let Some(idx) = app.agents.iter().position(|a| a.id == *current) {
            let next = (idx + 1) % app.agents.len();
            let id = app.agents[next].id.clone();
            app.focused_agent = Some(id);
            app.load_focused_session().await;
            app.restore_scroll_state();
        }
    }
}

pub(crate) async fn handle_prev_agent(app: &mut App) {
    app.save_scroll_state();
    if let Some(ref current) = app.focused_agent {
        if let Some(idx) = app.agents.iter().position(|a| a.id == *current) {
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
}

pub(crate) fn handle_toggle_sidebar(app: &mut App) {
    app.sidebar_visible = !app.sidebar_visible;
}

pub(crate) fn handle_toggle_thinking(app: &mut App) {
    app.thinking_expanded = !app.thinking_expanded;
}

pub(crate) fn handle_resize(app: &mut App, w: u16, h: u16) {
    app.terminal_width = w;
    app.terminal_height = h;
    // Terminal width changed — recalculate message heights for wrapping.
    app.rebuild_virtual_scroll();
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
    fn scroll_page_up_jumps_20() {
        let mut app = test_app();
        handle_scroll_page_up(&mut app);
        assert_eq!(app.scroll_offset, 20);
        assert!(!app.auto_scroll);
    }

    #[test]
    fn scroll_page_down_jumps_20() {
        let mut app = test_app();
        app.scroll_offset = 30;
        handle_scroll_page_down(&mut app);
        assert_eq!(app.scroll_offset, 10);
    }

    #[test]
    fn scroll_page_down_to_zero_auto_scrolls() {
        let mut app = test_app();
        app.scroll_offset = 15;
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
}
