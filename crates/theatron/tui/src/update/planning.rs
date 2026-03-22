//! Update handlers for the planning dashboard and retrospective views.

use crate::app::App;
use crate::state::view_stack::View;

/// Open the planning dashboard, pushing it onto the view stack.
pub(crate) fn handle_open(app: &mut App) {
    app.layout.view_stack.push(View::Planning);
    app.layout.planning.loading = false;
}

/// Close the planning dashboard (pop back).
pub(crate) fn handle_close(app: &mut App) {
    if matches!(app.layout.view_stack.current(), View::Planning) {
        app.layout.view_stack.pop();
    }
}

pub(crate) fn handle_tab_next(app: &mut App) {
    app.layout.planning.tab_next();
}

pub(crate) fn handle_tab_prev(app: &mut App) {
    app.layout.planning.tab_prev();
}

pub(crate) fn handle_select_up(app: &mut App) {
    app.layout.planning.select_up();
}

pub(crate) fn handle_select_down(app: &mut App) {
    app.layout.planning.select_down();
}

pub(crate) fn handle_toggle_expand(app: &mut App) {
    app.layout.planning.toggle_phase();
}

pub(crate) fn handle_approve_checkpoint(app: &mut App) {
    app.layout.planning.approve_checkpoint();
}

/// Open the retrospective view.
pub(crate) fn handle_retro_open(app: &mut App) {
    app.layout.view_stack.push(View::Retrospective);
    app.layout.retrospective.loading = false;
}

/// Close the retrospective view (pop back).
pub(crate) fn handle_retro_close(app: &mut App) {
    if matches!(app.layout.view_stack.current(), View::Retrospective) {
        app.layout.view_stack.pop();
    }
}

pub(crate) fn handle_retro_section_next(app: &mut App) {
    app.layout.retrospective.section_next();
}

pub(crate) fn handle_retro_section_prev(app: &mut App) {
    app.layout.retrospective.section_prev();
}

pub(crate) fn handle_retro_scroll_up(app: &mut App) {
    app.layout.retrospective.scroll_offset =
        app.layout.retrospective.scroll_offset.saturating_sub(1);
}

pub(crate) fn handle_retro_scroll_down(app: &mut App) {
    app.layout.retrospective.scroll_offset += 1;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::test_helpers::test_app;

    #[test]
    fn open_pushes_planning_view() {
        let mut app = test_app();
        handle_open(&mut app);
        assert_eq!(app.layout.view_stack.current(), &View::Planning);
    }

    #[test]
    fn close_pops_planning_view() {
        let mut app = test_app();
        handle_open(&mut app);
        handle_close(&mut app);
        assert!(app.layout.view_stack.is_home());
    }

    #[test]
    fn close_does_nothing_if_not_on_planning() {
        let mut app = test_app();
        handle_close(&mut app);
        assert!(app.layout.view_stack.is_home());
    }

    #[test]
    fn tab_next_cycles_tab() {
        let mut app = test_app();
        handle_open(&mut app);
        handle_tab_next(&mut app);
        assert_eq!(
            app.layout.planning.tab,
            crate::state::planning::PlanningTab::Requirements
        );
    }

    #[test]
    fn tab_prev_cycles_tab() {
        let mut app = test_app();
        handle_open(&mut app);
        handle_tab_prev(&mut app);
        assert_eq!(
            app.layout.planning.tab,
            crate::state::planning::PlanningTab::EditHistory
        );
    }

    #[test]
    fn select_up_saturates() {
        let mut app = test_app();
        handle_open(&mut app);
        handle_select_up(&mut app);
        assert_eq!(app.layout.planning.selected_row, 0);
    }

    #[test]
    fn retro_open_pushes_view() {
        let mut app = test_app();
        handle_retro_open(&mut app);
        assert_eq!(app.layout.view_stack.current(), &View::Retrospective);
    }

    #[test]
    fn retro_close_pops_view() {
        let mut app = test_app();
        handle_retro_open(&mut app);
        handle_retro_close(&mut app);
        assert!(app.layout.view_stack.is_home());
    }

    #[test]
    fn retro_section_next_cycles() {
        let mut app = test_app();
        handle_retro_open(&mut app);
        handle_retro_section_next(&mut app);
        assert_eq!(
            app.layout.retrospective.selected_section,
            crate::state::planning::RetrospectiveSection::Blockers
        );
    }

    #[test]
    fn retro_scroll_up_saturates() {
        let mut app = test_app();
        handle_retro_open(&mut app);
        handle_retro_scroll_up(&mut app);
        assert_eq!(app.layout.retrospective.scroll_offset, 0);
    }

    #[test]
    fn retro_scroll_down_increments() {
        let mut app = test_app();
        handle_retro_open(&mut app);
        handle_retro_scroll_down(&mut app);
        assert_eq!(app.layout.retrospective.scroll_offset, 1);
    }
}
