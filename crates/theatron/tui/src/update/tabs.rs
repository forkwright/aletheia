//! Tab management update handlers.

use crate::app::App;
use crate::msg::ErrorToast;
use crate::state::{Overlay, SessionPickerOverlay};

pub(crate) fn handle_tab_new(app: &mut App) {
    app.layout.overlay = Some(Overlay::SessionPicker(SessionPickerOverlay {
        cursor: 0,
        show_archived: false,
    }));
}

pub(crate) fn handle_tab_close(app: &mut App) {
    if app.layout.tab_bar.len() <= 1 {
        app.viewport.error_toast = Some(ErrorToast::new("Cannot close last tab".into()));
        return;
    }

    app.layout.tab_bar.close_active();

    if app.layout.tab_bar.is_empty() {
        return;
    }

    app.layout.tab_bar.clear_active_unread();
    app.restore_from_active_tab();
}

pub(crate) fn handle_tab_next(app: &mut App) {
    if app.layout.tab_bar.len() <= 1 {
        return;
    }
    app.save_to_active_tab();
    app.layout.tab_bar.next_tab();
    app.layout.tab_bar.clear_active_unread();
    app.restore_from_active_tab();
}

pub(crate) fn handle_tab_prev(app: &mut App) {
    if app.layout.tab_bar.len() <= 1 {
        return;
    }
    app.save_to_active_tab();
    app.layout.tab_bar.prev_tab();
    app.layout.tab_bar.clear_active_unread();
    app.restore_from_active_tab();
}

pub(crate) fn handle_tab_jump(app: &mut App, index: usize) {
    if index >= app.layout.tab_bar.len() || index == app.layout.tab_bar.active {
        return;
    }
    app.switch_to_tab(index);
}

pub(crate) fn handle_g_prefix(app: &mut App) {
    app.layout.pending_g = true;
}

/// Handle :tab command: switch to tab by name/partial match.
pub(crate) fn handle_tab_command(app: &mut App, args: &str) {
    if args.is_empty() {
        app.viewport.error_toast = Some(ErrorToast::new("Usage: :tab <name>".into()));
        return;
    }

    if let Some(idx) = app.layout.tab_bar.find_by_title(args) {
        app.switch_to_tab(idx);
    } else {
        app.viewport.error_toast = Some(ErrorToast::new(format!("No tab matching: {args}")));
    }
}
