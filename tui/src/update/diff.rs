//! Update handlers for the diff viewer.

use crate::app::App;
use crate::diff::{self, DiffViewState};
use crate::msg::ErrorToast;
use crate::state::Overlay;

/// Open the diff viewer with uncommitted changes (git diff).
pub(crate) async fn handle_diff_open(app: &mut App) {
    let output = tokio::task::spawn_blocking(|| {
        std::process::Command::new("git")
            .args(["diff", "HEAD"])
            .output()
    })
    .await;

    let output = match output {
        Ok(inner) => inner,
        Err(e) => {
            app.error_toast = Some(ErrorToast::new(format!("git diff task failed: {e}")));
            return;
        }
    };

    match output {
        Ok(result) => {
            let stdout = String::from_utf8_lossy(&result.stdout);
            if stdout.trim().is_empty() {
                app.error_toast = Some(ErrorToast::new("No uncommitted changes".into()));
                return;
            }
            let files = diff::parse_git_diff(&stdout);
            app.overlay = Some(Overlay::DiffView(DiffViewState::new(files)));
        }
        Err(e) => {
            app.error_toast = Some(ErrorToast::new(format!("git diff failed: {e}")));
        }
    }
}

/// Close the diff viewer overlay.
pub(crate) fn handle_diff_close(app: &mut App) {
    if matches!(&app.overlay, Some(Overlay::DiffView(_))) {
        app.overlay = None;
    }
}

/// Cycle between diff display modes.
pub(crate) fn handle_diff_cycle_mode(app: &mut App) {
    if let Some(Overlay::DiffView(ref mut state)) = app.overlay {
        state.cycle_mode();
    }
}

/// Scroll up in the diff viewer.
pub(crate) fn handle_diff_scroll_up(app: &mut App) {
    if let Some(Overlay::DiffView(ref mut state)) = app.overlay {
        state.scroll_up(1);
    }
}

/// Scroll down in the diff viewer.
pub(crate) fn handle_diff_scroll_down(app: &mut App) {
    if let Some(Overlay::DiffView(ref mut state)) = app.overlay {
        state.scroll_down(1);
    }
}

/// Page up in the diff viewer.
pub(crate) fn handle_diff_page_up(app: &mut App) {
    if let Some(Overlay::DiffView(ref mut state)) = app.overlay {
        state.scroll_up(20);
    }
}

/// Page down in the diff viewer.
pub(crate) fn handle_diff_page_down(app: &mut App) {
    if let Some(Overlay::DiffView(ref mut state)) = app.overlay {
        state.scroll_down(20);
    }
}

/// Handle an auto-triggered diff from a file modification tool result.
pub(crate) fn handle_diff_from_tool_result(
    app: &mut App,
    path: &str,
    old_content: &str,
    new_content: &str,
) {
    let file_diff = diff::compute_diff(path, old_content, new_content);
    if file_diff.hunks.is_empty() {
        return; // no actual changes
    }
    app.overlay = Some(Overlay::DiffView(DiffViewState::new(vec![file_diff])));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::test_helpers::test_app;

    #[test]
    fn handle_diff_close_clears_overlay() {
        let mut app = test_app();
        let state = DiffViewState::new(vec![]);
        app.overlay = Some(Overlay::DiffView(state));
        handle_diff_close(&mut app);
        assert!(app.overlay.is_none());
    }

    #[test]
    fn handle_diff_close_ignores_other_overlays() {
        let mut app = test_app();
        app.overlay = Some(Overlay::Help);
        handle_diff_close(&mut app);
        assert!(matches!(app.overlay, Some(Overlay::Help)));
    }

    #[test]
    fn handle_diff_cycle_mode_cycles() {
        let mut app = test_app();
        let state = DiffViewState::new(vec![]);
        app.overlay = Some(Overlay::DiffView(state));
        handle_diff_cycle_mode(&mut app);
        if let Some(Overlay::DiffView(ref s)) = app.overlay {
            assert_eq!(s.mode, diff::DiffMode::SideBySide);
        } else {
            panic!("expected DiffView");
        }
    }

    #[test]
    fn handle_diff_scroll_up_down() {
        let mut app = test_app();
        let mut state = DiffViewState::new(vec![]);
        state.total_lines = 100;
        app.overlay = Some(Overlay::DiffView(state));
        handle_diff_scroll_down(&mut app);
        handle_diff_scroll_down(&mut app);
        if let Some(Overlay::DiffView(ref s)) = app.overlay {
            assert_eq!(s.scroll_offset, 2);
        }
        handle_diff_scroll_up(&mut app);
        if let Some(Overlay::DiffView(ref s)) = app.overlay {
            assert_eq!(s.scroll_offset, 1);
        }
    }

    #[test]
    fn handle_diff_from_tool_result_no_changes() {
        let mut app = test_app();
        handle_diff_from_tool_result(&mut app, "test.rs", "same\n", "same\n");
        assert!(app.overlay.is_none());
    }

    #[test]
    fn handle_diff_from_tool_result_with_changes() {
        let mut app = test_app();
        handle_diff_from_tool_result(&mut app, "test.rs", "old\n", "new\n");
        assert!(matches!(app.overlay, Some(Overlay::DiffView(_))));
    }
}
