use crate::app::App;

pub(crate) fn handle_char_input(app: &mut App, c: char) {
    if c == '\t' {
        app.handle_tab_completion();
    } else {
        app.tab_completion = None;
        app.input.text.insert(app.input.cursor, c);
        app.input.cursor += c.len_utf8();
        app.input.history_index = None;
    }
}

pub(crate) fn handle_backspace(app: &mut App) {
    if app.input.cursor > 0 {
        let prev = app.prev_char_boundary(app.input.cursor);
        app.input.text.drain(prev..app.input.cursor);
        app.input.cursor = prev;
    }
}

pub(crate) fn handle_delete(app: &mut App) {
    if app.input.cursor < app.input.text.len() {
        let next = app.next_char_boundary(app.input.cursor);
        app.input.text.drain(app.input.cursor..next);
    }
}

pub(crate) fn handle_cursor_left(app: &mut App) {
    if app.input.cursor > 0 {
        app.input.cursor = app.prev_char_boundary(app.input.cursor);
    }
}

pub(crate) fn handle_cursor_right(app: &mut App) {
    if app.input.cursor < app.input.text.len() {
        app.input.cursor = app.next_char_boundary(app.input.cursor);
    }
}

pub(crate) fn handle_cursor_home(app: &mut App) {
    app.input.cursor = 0;
}

pub(crate) fn handle_cursor_end(app: &mut App) {
    app.input.cursor = app.input.text.len();
}

pub(crate) fn handle_delete_word(app: &mut App) {
    let mut pos = app.input.cursor;
    while pos > 0 && app.input.text.as_bytes().get(pos - 1) == Some(&b' ') {
        pos -= 1;
    }
    while pos > 0 && app.input.text.as_bytes().get(pos - 1) != Some(&b' ') {
        pos -= 1;
    }
    app.input.text.drain(pos..app.input.cursor);
    app.input.cursor = pos;
}

pub(crate) fn handle_clear_line(app: &mut App) {
    app.input.text.clear();
    app.input.cursor = 0;
}

pub(crate) fn handle_history_up(app: &mut App) {
    if !app.input.history.is_empty() {
        let idx = match app.input.history_index {
            Some(i) if i + 1 < app.input.history.len() => i + 1,
            None => 0,
            Some(i) => i,
        };
        app.input.history_index = Some(idx);
        app.input.text = app.input.history[app.input.history.len() - 1 - idx].clone();
        app.input.cursor = app.input.text.len();
    }
}

pub(crate) fn handle_history_down(app: &mut App) {
    match app.input.history_index {
        Some(0) => {
            app.input.history_index = None;
            app.input.text.clear();
            app.input.cursor = 0;
        }
        Some(i) => {
            let idx = i - 1;
            app.input.history_index = Some(idx);
            app.input.text = app.input.history[app.input.history.len() - 1 - idx].clone();
            app.input.cursor = app.input.text.len();
        }
        None => {}
    }
}

pub(crate) fn handle_submit(app: &mut App) {
    let text = app.input.text.trim().to_string();
    if text.is_empty() {
        return;
    }
    app.input.history.push(text.clone());
    app.input.text.clear();
    app.input.cursor = 0;
    app.input.history_index = None;
    app.send_message(&text);
}

pub(crate) fn handle_copy_last_response(app: &mut App) {
    if let Some(msg) = app.messages.iter().rev().find(|m| m.role == "assistant") {
        if let Err(e) = crate::clipboard::copy_to_clipboard(&msg.text) {
            tracing::error!("clipboard copy failed: {e}");
        }
    }
}

pub(crate) fn handle_compose_in_editor(app: &mut App) {
    let editor = std::env::var("EDITOR").unwrap_or_else(|_| "vi".to_string());
    let tmpfile = std::env::temp_dir().join("aletheia-compose.md");
    let _ = std::fs::write(&tmpfile, "");
    ratatui::restore();
    let status = std::process::Command::new(&editor).arg(&tmpfile).status();
    let _ = ratatui::init();
    if let Ok(s) = status {
        if s.success() {
            if let Ok(text) = std::fs::read_to_string(&tmpfile) {
                let text = text.trim().to_string();
                if !text.is_empty() {
                    app.send_message(&text);
                }
            }
        }
    }
    let _ = std::fs::remove_file(&tmpfile);
}
