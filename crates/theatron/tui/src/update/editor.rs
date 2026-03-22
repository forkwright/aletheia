//! Message handlers for the file editor view.

use crate::app::App;
use crate::msg::ErrorToast;
use crate::state::view_stack::View;

pub(crate) fn handle_open(app: &mut App) {
    let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    app.layout.editor = crate::state::editor::EditorState::new(cwd);
    app.layout.view_stack.push(View::FileEditor);
}

pub(crate) fn handle_close(app: &mut App) {
    app.layout.view_stack.pop();
}

pub(crate) fn handle_char_input(app: &mut App, c: char) {
    let editor = &mut app.layout.editor;

    if let Some(ref mut input) = editor.rename_input {
        input.push(c);
        return;
    }
    if let Some(ref mut input) = editor.new_file_input {
        input.push(c);
        return;
    }

    if !editor.tree_focused
        && let Some(tab) = editor.active_tab_mut()
    {
        tab.insert_char(c);
        let vh = usize::from(app.viewport.terminal_height.saturating_sub(4));
        if let Some(tab) = app.layout.editor.active_tab_mut() {
            tab.ensure_cursor_visible(vh);
        }
    }
}

pub(crate) fn handle_newline(app: &mut App) {
    let editor = &mut app.layout.editor;

    if let Some(ref input) = editor.rename_input.clone() {
        execute_rename(app, input);
        return;
    }
    if let Some(ref input) = editor.new_file_input.clone() {
        execute_new_file(app, input);
        return;
    }

    if editor.tree_focused {
        if let Some(entry) = editor.tree.selected_entry().cloned() {
            if entry.is_dir {
                editor.tree.toggle_expand();
            } else {
                editor.open_file(&entry.path);
            }
        }
    } else if let Some(tab) = editor.active_tab_mut() {
        tab.insert_newline();
        let vh = usize::from(app.viewport.terminal_height.saturating_sub(4));
        if let Some(tab) = app.layout.editor.active_tab_mut() {
            tab.ensure_cursor_visible(vh);
        }
    }
}

pub(crate) fn handle_backspace(app: &mut App) {
    let editor = &mut app.layout.editor;

    if let Some(ref mut input) = editor.rename_input {
        input.pop();
        return;
    }
    if let Some(ref mut input) = editor.new_file_input {
        input.pop();
        return;
    }

    if !editor.tree_focused
        && let Some(tab) = editor.active_tab_mut()
    {
        tab.backspace();
    }
}

pub(crate) fn handle_delete(app: &mut App) {
    if !app.layout.editor.tree_focused
        && let Some(tab) = app.layout.editor.active_tab_mut()
    {
        tab.delete_char();
    }
}

pub(crate) fn handle_cursor_up(app: &mut App) {
    let editor = &mut app.layout.editor;
    if editor.tree_focused {
        editor.tree.select_up();
    } else if let Some(tab) = editor.active_tab_mut() {
        tab.cursor_up();
        let vh = usize::from(app.viewport.terminal_height.saturating_sub(4));
        if let Some(tab) = app.layout.editor.active_tab_mut() {
            tab.ensure_cursor_visible(vh);
        }
    }
}

pub(crate) fn handle_cursor_down(app: &mut App) {
    let editor = &mut app.layout.editor;
    if editor.tree_focused {
        editor.tree.select_down();
    } else if let Some(tab) = editor.active_tab_mut() {
        tab.cursor_down();
        let vh = usize::from(app.viewport.terminal_height.saturating_sub(4));
        if let Some(tab) = app.layout.editor.active_tab_mut() {
            tab.ensure_cursor_visible(vh);
        }
    }
}

pub(crate) fn handle_cursor_left(app: &mut App) {
    if !app.layout.editor.tree_focused
        && let Some(tab) = app.layout.editor.active_tab_mut()
    {
        tab.cursor_left();
    }
}

pub(crate) fn handle_cursor_right(app: &mut App) {
    if !app.layout.editor.tree_focused
        && let Some(tab) = app.layout.editor.active_tab_mut()
    {
        tab.cursor_right();
    }
}

pub(crate) fn handle_cursor_home(app: &mut App) {
    if !app.layout.editor.tree_focused
        && let Some(tab) = app.layout.editor.active_tab_mut()
    {
        tab.cursor_home();
    }
}

pub(crate) fn handle_cursor_end(app: &mut App) {
    if !app.layout.editor.tree_focused
        && let Some(tab) = app.layout.editor.active_tab_mut()
    {
        tab.cursor_end();
    }
}

pub(crate) fn handle_page_up(app: &mut App) {
    let page = usize::from(app.viewport.terminal_height.saturating_sub(6));
    let editor = &mut app.layout.editor;
    if editor.tree_focused {
        for _ in 0..page {
            editor.tree.select_up();
        }
    } else if let Some(tab) = editor.active_tab_mut() {
        tab.page_up(page);
        tab.ensure_cursor_visible(page);
    }
}

pub(crate) fn handle_page_down(app: &mut App) {
    let page = usize::from(app.viewport.terminal_height.saturating_sub(6));
    let editor = &mut app.layout.editor;
    if editor.tree_focused {
        for _ in 0..page {
            editor.tree.select_down();
        }
    } else if let Some(tab) = editor.active_tab_mut() {
        tab.page_down(page);
        tab.ensure_cursor_visible(page);
    }
}

pub(crate) fn handle_save(app: &mut App) {
    if let Some(tab) = app.layout.editor.active_tab_mut() {
        match tab.save() {
            Ok(()) => {
                app.viewport.success_toast =
                    Some(ErrorToast::new(format!("Saved {}", tab.file_name())));
                app.layout.editor.tree.refresh();
            }
            Err(msg) => {
                app.viewport.error_toast = Some(ErrorToast::new(msg));
            }
        }
    }
}

pub(crate) fn handle_tab_next(app: &mut App) {
    app.layout.editor.next_tab();
}

pub(crate) fn handle_tab_prev(app: &mut App) {
    app.layout.editor.prev_tab();
}

pub(crate) fn handle_tab_close(app: &mut App) {
    let idx = app.layout.editor.active_tab;
    app.layout.editor.close_tab(idx);
}

pub(crate) fn handle_tree_toggle(app: &mut App) {
    app.layout.editor.tree_visible = !app.layout.editor.tree_visible;
}

pub(crate) fn handle_focus_toggle(app: &mut App) {
    if app.layout.editor.tabs.is_empty() {
        app.layout.editor.tree_focused = true;
    } else {
        app.layout.editor.tree_focused = !app.layout.editor.tree_focused;
    }
}

pub(crate) fn handle_tree_expand(app: &mut App) {
    app.layout.editor.tree.toggle_expand();
}

pub(crate) fn handle_cut(app: &mut App) {
    if let Some(tab) = app.layout.editor.active_tab_mut() {
        let cut = tab.delete_line();
        app.layout.editor.clipboard = cut;
    }
}

pub(crate) fn handle_copy(app: &mut App) {
    if let Some(tab) = app.layout.editor.active_tab() {
        let copied = tab.copy_line();
        app.layout.editor.clipboard = copied;
    }
}

pub(crate) fn handle_paste(app: &mut App) {
    let clipboard = app.layout.editor.clipboard.clone();
    if let Some(tab) = app.layout.editor.active_tab_mut() {
        tab.paste_lines(&clipboard);
    }
}

pub(crate) fn handle_new_file_start(app: &mut App) {
    app.layout.editor.new_file_input = Some(String::new());
}

pub(crate) fn handle_rename_start(app: &mut App) {
    let editor = &mut app.layout.editor;
    if let Some(entry) = editor.tree.selected_entry() {
        let name = entry.name.clone();
        editor.rename_input = Some(name);
    }
}

pub(crate) fn handle_delete_start(app: &mut App) {
    if let Some(entry) = app.layout.editor.tree.selected_entry() {
        let path = entry.path.clone();
        app.layout.editor.confirm_delete = Some(path);
    }
}

pub(crate) fn handle_confirm_delete(app: &mut App, confirmed: bool) {
    if confirmed && let Some(ref path) = app.layout.editor.confirm_delete.clone() {
        let is_dir = path.is_dir();
        let result = if is_dir {
            std::fs::remove_dir_all(path)
        } else {
            std::fs::remove_file(path)
        };
        match result {
            Ok(()) => {
                app.layout.editor.tabs.retain(|t| t.path != *path);
                if app.layout.editor.active_tab >= app.layout.editor.tabs.len()
                    && !app.layout.editor.tabs.is_empty()
                {
                    app.layout.editor.active_tab = app.layout.editor.tabs.len() - 1;
                }
                app.layout.editor.tree.refresh();
                app.viewport.success_toast = Some(ErrorToast::new("Deleted".to_string()));
            }
            Err(e) => {
                app.viewport.error_toast = Some(ErrorToast::new(format!("Delete failed: {e}")));
            }
        }
    }
    app.layout.editor.confirm_delete = None;
}

pub(crate) fn handle_modal_cancel(app: &mut App) {
    app.layout.editor.confirm_delete = None;
    app.layout.editor.rename_input = None;
    app.layout.editor.new_file_input = None;
}

pub(crate) fn handle_refresh_tree(app: &mut App) {
    app.layout.editor.tree.refresh();
}

pub(crate) fn handle_autosave_tick(app: &mut App) {
    app.layout.editor.autosave_tick();
}

pub(crate) fn handle_scroll_tree(app: &mut App, visible_height: usize) {
    app.layout.editor.tree.adjust_scroll(visible_height);
}

fn execute_rename(app: &mut App, new_name: &str) {
    let new_name = new_name.trim().to_string();
    app.layout.editor.rename_input = None;

    if new_name.is_empty() {
        return;
    }

    let Some(entry) = app.layout.editor.tree.selected_entry().cloned() else {
        return;
    };

    let new_path = entry
        .path
        .parent()
        .map(|p| p.join(&new_name))
        .unwrap_or_else(|| std::path::PathBuf::from(&new_name));

    match std::fs::rename(&entry.path, &new_path) {
        Ok(()) => {
            for tab in &mut app.layout.editor.tabs {
                if tab.path == entry.path {
                    tab.path = new_path.clone();
                    tab.language = crate::state::editor::detect_language_pub(&new_path);
                }
            }
            app.layout.editor.tree.refresh();
            app.viewport.success_toast = Some(ErrorToast::new(format!("Renamed to {new_name}")));
        }
        Err(e) => {
            app.viewport.error_toast = Some(ErrorToast::new(format!("Rename failed: {e}")));
        }
    }
}

#[expect(
    clippy::disallowed_methods,
    reason = "editor creates new files on local filesystem via user action"
)]
fn execute_new_file(app: &mut App, name: &str) {
    let name = name.trim().to_string();
    app.layout.editor.new_file_input = None;

    if name.is_empty() {
        return;
    }

    let parent = app
        .layout
        .editor
        .tree
        .selected_entry()
        .and_then(|e| {
            if e.is_dir {
                Some(e.path.clone())
            } else {
                e.path.parent().map(|p| p.to_path_buf())
            }
        })
        .unwrap_or_else(|| app.layout.editor.tree.root.clone());

    let new_path = parent.join(&name);

    if new_path.exists() {
        app.viewport.error_toast = Some(ErrorToast::new(format!("{name} already exists")));
        return;
    }

    match std::fs::write(&new_path, "") {
        Ok(()) => {
            app.layout.editor.tree.refresh();
            app.layout.editor.open_file(&new_path);
            app.viewport.success_toast = Some(ErrorToast::new(format!("Created {name}")));
        }
        Err(e) => {
            app.viewport.error_toast = Some(ErrorToast::new(format!("Create failed: {e}")));
        }
    }
}
