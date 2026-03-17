use crate::app::App;

pub(crate) fn handle_char_input(app: &mut App, c: char) {
    if c == '\t' {
        app.handle_tab_completion();
    } else {
        app.interaction.tab_completion = None;
        app.interaction
            .input
            .text
            .insert(app.interaction.input.cursor, c);
        app.interaction.input.cursor += c.len_utf8();
        app.interaction.input.history_index = None;
    }
}

pub(crate) fn handle_backspace(app: &mut App) {
    if app.interaction.input.cursor > 0 {
        let prev = app.prev_char_boundary(app.interaction.input.cursor);
        app.interaction
            .input
            .text
            .drain(prev..app.interaction.input.cursor);
        app.interaction.input.cursor = prev;
    }
}

pub(crate) fn handle_delete(app: &mut App) {
    if app.interaction.input.cursor < app.interaction.input.text.len() {
        let next = app.next_char_boundary(app.interaction.input.cursor);
        app.interaction
            .input
            .text
            .drain(app.interaction.input.cursor..next);
    }
}

pub(crate) fn handle_cursor_left(app: &mut App) {
    if app.interaction.input.cursor > 0 {
        app.interaction.input.cursor = app.prev_char_boundary(app.interaction.input.cursor);
    }
}

pub(crate) fn handle_cursor_right(app: &mut App) {
    if app.interaction.input.cursor < app.interaction.input.text.len() {
        app.interaction.input.cursor = app.next_char_boundary(app.interaction.input.cursor);
    }
}

pub(crate) fn handle_cursor_home(app: &mut App) {
    app.interaction.input.cursor = 0;
}

pub(crate) fn handle_cursor_end(app: &mut App) {
    app.interaction.input.cursor = app.interaction.input.text.len();
}

pub(crate) fn handle_delete_word(app: &mut App) {
    let mut pos = app.interaction.input.cursor;
    while pos > 0 {
        let prev = app.prev_char_boundary(pos);
        let is_ws = app
            .interaction
            .input
            .text
            .get(prev..pos)
            .and_then(|s| s.chars().next())
            .is_some_and(|c| c.is_whitespace());
        if is_ws {
            pos = prev;
        } else {
            break;
        }
    }
    while pos > 0 {
        let prev = app.prev_char_boundary(pos);
        let is_ws = app
            .interaction
            .input
            .text
            .get(prev..pos)
            .and_then(|s| s.chars().next())
            .is_some_and(|c| c.is_whitespace());
        if is_ws {
            break;
        }
        pos = prev;
    }
    app.interaction
        .input
        .text
        .drain(pos..app.interaction.input.cursor);
    app.interaction.input.cursor = pos;
}

pub(crate) fn handle_clear_line(app: &mut App) {
    app.interaction.input.text.clear();
    app.interaction.input.cursor = 0;
}

pub(crate) fn handle_delete_to_end(app: &mut App) {
    app.interaction
        .input
        .text
        .drain(app.interaction.input.cursor..);
}

pub(crate) fn handle_history_up(app: &mut App) {
    if !app.interaction.input.history.is_empty() {
        let idx = match app.interaction.input.history_index {
            Some(i) if i + 1 < app.interaction.input.history.len() => i + 1,
            None => 0,
            Some(i) => i,
        };
        app.interaction.input.history_index = Some(idx);
        app.interaction.input.text =
            app.interaction.input.history[app.interaction.input.history.len() - 1 - idx].clone();
        app.interaction.input.cursor = app.interaction.input.text.len();
    }
}

pub(crate) fn handle_history_down(app: &mut App) {
    match app.interaction.input.history_index {
        Some(0) => {
            app.interaction.input.history_index = None;
            app.interaction.input.text.clear();
            app.interaction.input.cursor = 0;
        }
        Some(i) => {
            let idx = i - 1;
            app.interaction.input.history_index = Some(idx);
            app.interaction.input.text = app.interaction.input.history
                [app.interaction.input.history.len() - 1 - idx]
                .clone();
            app.interaction.input.cursor = app.interaction.input.text.len();
        }
        // NOTE: already at latest input, no history to navigate
        None => {}
    }
}

pub(crate) fn handle_submit(app: &mut App) {
    let text = app.interaction.input.text.trim().to_string();
    if text.is_empty() {
        return;
    }
    app.interaction.input.history.push(text.clone());
    app.interaction.input.text.clear();
    app.interaction.input.cursor = 0;
    app.interaction.input.history_index = None;
    app.send_message(&text);
}

pub(crate) fn handle_copy_last_response(app: &mut App) {
    if let Some(msg) = app
        .dashboard
        .messages
        .iter()
        .rev()
        .find(|m| m.role == "assistant")
        && let Err(e) = crate::clipboard::copy_to_clipboard(&msg.text)
    {
        tracing::error!("clipboard copy failed: {e}");
    }
}

// WHY: blocking is intentional: TUI is suspended so the event loop is paused
pub(crate) fn handle_compose_in_editor(app: &mut App) {
    let editor = std::env::var("EDITOR").unwrap_or_else(|_| "vi".to_string());
    let tmpfile = std::env::temp_dir().join("aletheia-compose.md");
    let _ = std::fs::write(&tmpfile, "");
    ratatui::restore();
    let status = std::process::Command::new(&editor).arg(&tmpfile).status();
    let _ = ratatui::init();
    if let Ok(s) = status
        && s.success()
        && let Ok(text) = std::fs::read_to_string(&tmpfile)
    {
        let text = text.trim().to_string();
        if !text.is_empty() {
            app.send_message(&text);
        }
    }
    let _ = std::fs::remove_file(&tmpfile);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::test_helpers::*;

    #[test]
    fn char_input_inserts_at_cursor() {
        let mut app = test_app();
        handle_char_input(&mut app, 'h');
        handle_char_input(&mut app, 'i');
        assert_eq!(app.interaction.input.text, "hi");
        assert_eq!(app.interaction.input.cursor, 2);
    }

    #[test]
    fn char_input_resets_history_index() {
        let mut app = test_app();
        app.interaction.input.history_index = Some(0);
        handle_char_input(&mut app, 'a');
        assert!(app.interaction.input.history_index.is_none());
    }

    #[test]
    fn backspace_removes_char() {
        let mut app = test_app();
        app.interaction.input.text = "ab".to_string();
        app.interaction.input.cursor = 2;
        handle_backspace(&mut app);
        assert_eq!(app.interaction.input.text, "a");
        assert_eq!(app.interaction.input.cursor, 1);
    }

    #[test]
    fn backspace_at_start_noop() {
        let mut app = test_app();
        app.interaction.input.text = "a".to_string();
        app.interaction.input.cursor = 0;
        handle_backspace(&mut app);
        assert_eq!(app.interaction.input.text, "a");
    }

    #[test]
    fn delete_removes_char_at_cursor() {
        let mut app = test_app();
        app.interaction.input.text = "abc".to_string();
        app.interaction.input.cursor = 1;
        handle_delete(&mut app);
        assert_eq!(app.interaction.input.text, "ac");
        assert_eq!(app.interaction.input.cursor, 1);
    }

    #[test]
    fn delete_at_end_noop() {
        let mut app = test_app();
        app.interaction.input.text = "abc".to_string();
        app.interaction.input.cursor = 3;
        handle_delete(&mut app);
        assert_eq!(app.interaction.input.text, "abc");
    }

    #[test]
    fn cursor_left_decrements() {
        let mut app = test_app();
        app.interaction.input.text = "ab".to_string();
        app.interaction.input.cursor = 2;
        handle_cursor_left(&mut app);
        assert_eq!(app.interaction.input.cursor, 1);
    }

    #[test]
    fn cursor_left_at_start_stays() {
        let mut app = test_app();
        app.interaction.input.text = "a".to_string();
        app.interaction.input.cursor = 0;
        handle_cursor_left(&mut app);
        assert_eq!(app.interaction.input.cursor, 0);
    }

    #[test]
    fn cursor_right_increments() {
        let mut app = test_app();
        app.interaction.input.text = "ab".to_string();
        app.interaction.input.cursor = 0;
        handle_cursor_right(&mut app);
        assert_eq!(app.interaction.input.cursor, 1);
    }

    #[test]
    fn cursor_right_at_end_stays() {
        let mut app = test_app();
        app.interaction.input.text = "ab".to_string();
        app.interaction.input.cursor = 2;
        handle_cursor_right(&mut app);
        assert_eq!(app.interaction.input.cursor, 2);
    }

    #[test]
    fn cursor_home_goes_to_zero() {
        let mut app = test_app();
        app.interaction.input.text = "hello".to_string();
        app.interaction.input.cursor = 3;
        handle_cursor_home(&mut app);
        assert_eq!(app.interaction.input.cursor, 0);
    }

    #[test]
    fn cursor_end_goes_to_len() {
        let mut app = test_app();
        app.interaction.input.text = "hello".to_string();
        app.interaction.input.cursor = 0;
        handle_cursor_end(&mut app);
        assert_eq!(app.interaction.input.cursor, 5);
    }

    #[test]
    fn delete_word_removes_word() {
        let mut app = test_app();
        app.interaction.input.text = "hello world".to_string();
        app.interaction.input.cursor = 11;
        handle_delete_word(&mut app);
        assert_eq!(app.interaction.input.text, "hello ");
        assert_eq!(app.interaction.input.cursor, 6);
    }

    #[test]
    fn delete_word_skips_spaces() {
        let mut app = test_app();
        app.interaction.input.text = "hello   ".to_string();
        app.interaction.input.cursor = 8;
        handle_delete_word(&mut app);
        assert_eq!(app.interaction.input.text, "");
        assert_eq!(app.interaction.input.cursor, 0);
    }

    #[test]
    fn delete_word_handles_unicode_whitespace() {
        let mut app = test_app();
        // Non-breaking space (U+00A0) before the word
        app.interaction.input.text = "hello\u{00A0}world".to_string();
        app.interaction.input.cursor = app.interaction.input.text.len();
        handle_delete_word(&mut app);
        assert_eq!(app.interaction.input.text, "hello\u{00A0}");
    }

    #[test]
    fn delete_word_handles_multibyte_word() {
        let mut app = test_app();
        // CJK characters as the "word"
        app.interaction.input.text = "hello 你好".to_string();
        app.interaction.input.cursor = app.interaction.input.text.len();
        handle_delete_word(&mut app);
        assert_eq!(app.interaction.input.text, "hello ");
    }

    #[test]
    fn clear_line_empties_input() {
        let mut app = test_app();
        app.interaction.input.text = "some text".to_string();
        app.interaction.input.cursor = 5;
        handle_clear_line(&mut app);
        assert!(app.interaction.input.text.is_empty());
        assert_eq!(app.interaction.input.cursor, 0);
    }

    #[test]
    fn history_up_navigates_back() {
        let mut app = test_app();
        app.interaction.input.history = vec!["first".to_string(), "second".to_string()];
        handle_history_up(&mut app);
        assert_eq!(app.interaction.input.text, "second");
        assert_eq!(app.interaction.input.history_index, Some(0));
    }

    #[test]
    fn history_up_twice() {
        let mut app = test_app();
        app.interaction.input.history = vec!["first".to_string(), "second".to_string()];
        handle_history_up(&mut app);
        handle_history_up(&mut app);
        assert_eq!(app.interaction.input.text, "first");
        assert_eq!(app.interaction.input.history_index, Some(1));
    }

    #[test]
    fn history_up_stops_at_oldest() {
        let mut app = test_app();
        app.interaction.input.history = vec!["only".to_string()];
        handle_history_up(&mut app);
        handle_history_up(&mut app);
        assert_eq!(app.interaction.input.text, "only");
        assert_eq!(app.interaction.input.history_index, Some(0));
    }

    #[test]
    fn history_up_empty_noop() {
        let mut app = test_app();
        handle_history_up(&mut app);
        assert!(app.interaction.input.text.is_empty());
        assert!(app.interaction.input.history_index.is_none());
    }

    #[test]
    fn history_down_from_index_zero_clears() {
        let mut app = test_app();
        app.interaction.input.history = vec!["first".to_string()];
        app.interaction.input.history_index = Some(0);
        app.interaction.input.text = "first".to_string();
        handle_history_down(&mut app);
        assert!(app.interaction.input.text.is_empty());
        assert!(app.interaction.input.history_index.is_none());
    }

    #[test]
    fn history_down_navigates_forward() {
        let mut app = test_app();
        app.interaction.input.history = vec!["first".to_string(), "second".to_string()];
        app.interaction.input.history_index = Some(1);
        handle_history_down(&mut app);
        assert_eq!(app.interaction.input.text, "second");
        assert_eq!(app.interaction.input.history_index, Some(0));
    }

    #[test]
    fn history_down_no_index_noop() {
        let mut app = test_app();
        handle_history_down(&mut app);
        assert!(app.interaction.input.text.is_empty());
    }

    #[test]
    fn submit_empty_noop() {
        let mut app = test_app();
        app.interaction.input.text = "   ".to_string();
        handle_submit(&mut app);
        assert!(app.interaction.input.history.is_empty());
    }
}
