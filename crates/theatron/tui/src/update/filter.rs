use crate::app::App;

pub(crate) fn handle_open(app: &mut App) {
    app.filter.open();
    update_match_counts(app);
    // WHY: rebuild ensures virtual scroll reflects current state before the user exits filter mode
    app.rebuild_virtual_scroll();
}

pub(crate) fn handle_close(app: &mut App) {
    app.filter.close();
    // WHY: rebuild before scrolling to bottom so virtual scroll reflects the full message list
    app.rebuild_virtual_scroll();
    app.scroll_to_bottom();
}

pub(crate) fn handle_input(app: &mut App, c: char) {
    app.filter.insert_char(c);
    update_match_counts(app);
    scroll_to_first_match(app);
}

pub(crate) fn handle_backspace(app: &mut App) {
    app.filter.backspace();
    if app.filter.text.is_empty() {
        app.filter.match_count = 0;
        app.filter.total_count = app.messages.len();
    } else {
        update_match_counts(app);
    }
}

pub(crate) fn handle_clear(app: &mut App) {
    app.filter.clear_text();
    app.filter.total_count = app.messages.len();
}

pub(crate) fn handle_confirm(app: &mut App) {
    if app.filter.text.is_empty() {
        app.filter.close();
    } else {
        app.filter.confirm();
    }
    // WHY: rebuild so virtual scroll is consistent with any layout changes during filter mode
    app.rebuild_virtual_scroll();
}

pub(crate) fn handle_next_match(app: &mut App) {
    app.filter.next_match();
}

pub(crate) fn handle_prev_match(app: &mut App) {
    app.filter.prev_match();
}

fn update_match_counts(app: &mut App) {
    let total = app.messages.len();
    let (pattern, inverted) = app.filter.pattern();

    if pattern.is_empty() {
        app.filter.match_count = 0;
        app.filter.total_count = total;
        return;
    }

    let count = app
        .messages
        .iter()
        .filter(|m| {
            let contains = m.text.to_lowercase().contains(pattern);
            if inverted { !contains } else { contains }
        })
        .count();

    app.filter.match_count = count;
    app.filter.total_count = total;
}

fn scroll_to_first_match(app: &mut App) {
    app.auto_scroll = false;
    app.scroll_offset = 0;
    app.filter.current_match = 0;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::test_helpers::*;

    #[test]
    fn handle_open_activates_filter() {
        let mut app = test_app();
        handle_open(&mut app);
        assert!(app.filter.active);
        assert!(app.filter.editing);
    }

    #[test]
    fn handle_close_deactivates_filter() {
        let mut app = test_app();
        handle_open(&mut app);
        handle_close(&mut app);
        assert!(!app.filter.active);
        assert!(!app.filter.editing);
        assert!(app.auto_scroll);
    }

    #[test]
    fn handle_input_counts_matches() {
        let mut app = test_app_with_messages(vec![
            ("user", "hello world"),
            ("assistant", "hello there"),
            ("user", "goodbye"),
        ]);
        handle_open(&mut app);
        handle_input(&mut app, 'h');
        handle_input(&mut app, 'e');
        handle_input(&mut app, 'l');
        // "hel" matches "hello world" and "hello there"
        assert_eq!(app.filter.match_count, 2);
        assert_eq!(app.filter.total_count, 3);
    }

    #[test]
    fn handle_backspace_recounts() {
        let mut app = test_app_with_messages(vec![("user", "hello"), ("assistant", "help")]);
        handle_open(&mut app);
        handle_input(&mut app, 'h');
        handle_input(&mut app, 'e');
        handle_input(&mut app, 'l');
        handle_input(&mut app, 'l');
        handle_input(&mut app, 'o');
        // "hello" matches only 1
        assert_eq!(app.filter.match_count, 1);

        handle_backspace(&mut app);
        handle_backspace(&mut app);
        // "hel" matches both
        assert_eq!(app.filter.match_count, 2);
    }

    #[test]
    fn handle_backspace_empty_clears_match_count() {
        let mut app = test_app_with_messages(vec![("user", "hello")]);
        handle_open(&mut app);
        handle_input(&mut app, 'h');
        handle_backspace(&mut app);
        assert_eq!(app.filter.match_count, 0);
        assert_eq!(app.filter.total_count, 1);
    }

    #[test]
    fn handle_clear_resets() {
        let mut app = test_app_with_messages(vec![("user", "hello")]);
        handle_open(&mut app);
        handle_input(&mut app, 'h');
        handle_clear(&mut app);
        assert!(app.filter.text.is_empty());
        assert_eq!(app.filter.total_count, 1);
    }

    #[test]
    fn handle_confirm_locks_filter() {
        let mut app = test_app();
        handle_open(&mut app);
        handle_input(&mut app, 'x');
        handle_confirm(&mut app);
        assert!(app.filter.active);
        assert!(!app.filter.editing);
    }

    #[test]
    fn handle_confirm_empty_closes() {
        let mut app = test_app();
        handle_open(&mut app);
        handle_confirm(&mut app);
        assert!(!app.filter.active);
    }

    #[test]
    fn inverted_pattern_counts_non_matches() {
        let mut app = test_app_with_messages(vec![("user", "hello"), ("assistant", "world")]);
        handle_open(&mut app);
        handle_input(&mut app, '!');
        handle_input(&mut app, 'h');
        handle_input(&mut app, 'e');
        // "!he" inverts: matches messages NOT containing "he"
        assert_eq!(app.filter.match_count, 1); // "world" matches
    }

    #[test]
    fn handle_open_rebuilds_virtual_scroll() {
        let mut app = test_app_with_messages(vec![("user", "hi"), ("assistant", "hello")]);
        // Simulate a stale virtual scroll by clearing it.
        app.virtual_scroll.clear();
        assert_eq!(app.virtual_scroll.len(), 0);

        handle_open(&mut app);

        assert_eq!(
            app.virtual_scroll.len(),
            app.messages.len(),
            "virtual scroll must be rebuilt on filter open"
        );
    }

    #[test]
    fn handle_close_rebuilds_virtual_scroll() {
        let mut app = test_app_with_messages(vec![("user", "hi"), ("assistant", "hello")]);
        handle_open(&mut app);
        // Corrupt the virtual scroll to simulate stale state.
        app.virtual_scroll.clear();

        handle_close(&mut app);

        assert_eq!(
            app.virtual_scroll.len(),
            app.messages.len(),
            "virtual scroll must be rebuilt on filter close"
        );
    }

    #[test]
    fn handle_confirm_rebuilds_virtual_scroll() {
        let mut app = test_app_with_messages(vec![("user", "hello"), ("assistant", "world")]);
        handle_open(&mut app);
        handle_input(&mut app, 'h');
        app.virtual_scroll.clear();

        handle_confirm(&mut app);

        assert_eq!(
            app.virtual_scroll.len(),
            app.messages.len(),
            "virtual scroll must be rebuilt on filter confirm"
        );
    }
}
