use crate::app::App;

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

pub(crate) async fn handle_focus_agent(app: &mut App, id: String) {
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
}
