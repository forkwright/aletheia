use crate::app::App;

pub(crate) fn handle_open(app: &mut App) {
    app.filter.open();
    update_match_counts(app);
}

pub(crate) fn handle_close(app: &mut App) {
    app.filter.close();
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

    let pattern_lower = pattern.to_lowercase();
    let count = app
        .messages
        .iter()
        .filter(|m| {
            let contains = m.text.to_lowercase().contains(&pattern_lower);
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
