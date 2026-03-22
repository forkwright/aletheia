//! Update handlers for the metrics dashboard view.

use crate::app::App;
use crate::state::view_stack::View;

/// Open the metrics dashboard and trigger a background health check.
pub(crate) async fn handle_open(app: &mut App) {
    app.layout.view_stack.push(View::Metrics);
    app.layout.metrics.scroll_offset = 0;
    app.layout.metrics.selected_agent = 0;

    // WHY: Fire a health check each time the metrics view opens so the badge
    // reflects current server state rather than the startup snapshot.
    let client = app.client.clone();
    app.layout.metrics.api_healthy = Some(client.health().await.unwrap_or(false));
}

/// Close the metrics dashboard and return to the previous view.
pub(crate) fn handle_close(app: &mut App) {
    app.layout.view_stack.pop();
}

/// Move selection up in the per-agent table.
pub(crate) fn handle_select_up(app: &mut App) {
    let metrics = &mut app.layout.metrics;
    if metrics.selected_agent > 0 {
        metrics.selected_agent -= 1;
        if metrics.selected_agent < metrics.scroll_offset {
            metrics.scroll_offset = metrics.selected_agent;
        }
    }
}

/// Move selection down in the per-agent table.
pub(crate) fn handle_select_down(app: &mut App) {
    let count = app.dashboard.agents.len();
    if count == 0 {
        return;
    }
    let metrics = &mut app.layout.metrics;
    if metrics.selected_agent + 1 < count {
        metrics.selected_agent += 1;
        // NOTE: visible_height is an approximation; exact paging is handled by render.
        // We keep the scroll window trailing the cursor.
        if metrics.selected_agent >= metrics.scroll_offset + 20 {
            metrics.scroll_offset = metrics.selected_agent.saturating_sub(19);
        }
    }
}

/// Apply the result of an async health check.
pub(crate) fn handle_health_loaded(app: &mut App, healthy: bool) {
    app.layout.metrics.api_healthy = Some(healthy);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::test_helpers::test_app;

    #[test]
    fn handle_close_pops_view() {
        let mut app = test_app();
        app.layout.view_stack.push(View::Metrics);
        handle_close(&mut app);
        assert_eq!(app.layout.view_stack.current(), &View::Home);
    }

    #[test]
    fn handle_select_up_saturates_at_zero() {
        let mut app = test_app();
        app.layout.metrics.selected_agent = 0;
        handle_select_up(&mut app);
        assert_eq!(app.layout.metrics.selected_agent, 0);
    }

    #[test]
    fn handle_health_loaded_sets_flag() {
        let mut app = test_app();
        handle_health_loaded(&mut app, true);
        assert_eq!(app.layout.metrics.api_healthy, Some(true));
        handle_health_loaded(&mut app, false);
        assert_eq!(app.layout.metrics.api_healthy, Some(false));
    }
}
