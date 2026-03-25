use crate::app::App;
use crate::state::{ImageAttachment, QueuedMessage, YankSpan};

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
        // WHY: any non-yank input invalidates the yank span so Alt+Y won't replace stale text
        app.interaction.input.kill_ring.last_yank = None;
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

/// Ctrl+U: clear entire line, storing killed text in the kill ring.
pub(crate) fn handle_clear_line(app: &mut App) {
    let killed = std::mem::take(&mut app.interaction.input.text);
    app.interaction.input.cursor = 0;
    app.interaction.input.kill_ring.push(killed);
}

/// Ctrl+K: delete from cursor to end of line, storing killed text in the kill ring.
pub(crate) fn handle_delete_to_end(app: &mut App) {
    let killed: String = app
        .interaction
        .input
        .text
        .drain(app.interaction.input.cursor..)
        .collect();
    app.interaction.input.kill_ring.push(killed);
}

/// Ctrl+Y: yank (paste) the most recent kill ring entry at the cursor.
pub(crate) fn handle_yank(app: &mut App) {
    let text = match app.interaction.input.kill_ring.last() {
        Some(t) => t.to_string(),
        None => return,
    };
    let start = app.interaction.input.cursor;
    app.interaction.input.text.insert_str(start, &text);
    let end = start + text.len();
    app.interaction.input.cursor = end;
    let ring_index = app
        .interaction
        .input
        .kill_ring
        .entries
        .len()
        .saturating_sub(1);
    app.interaction.input.kill_ring.last_yank = Some(YankSpan {
        start,
        end,
        ring_index,
    });
}

/// Alt+Y: cycle through kill ring entries, replacing the last yanked text.
pub(crate) fn handle_yank_cycle(app: &mut App) {
    let yank = match app.interaction.input.kill_ring.last_yank.clone() {
        Some(y) => y,
        None => return,
    };
    if let Some((new_index, new_text)) = app.interaction.input.kill_ring.cycle(yank.ring_index) {
        let new_text = new_text.to_string();
        app.interaction.input.text.drain(yank.start..yank.end);
        app.interaction.input.text.insert_str(yank.start, &new_text);
        let new_end = yank.start + new_text.len();
        app.interaction.input.cursor = new_end;
        app.interaction.input.kill_ring.last_yank = Some(YankSpan {
            start: yank.start,
            end: new_end,
            ring_index: new_index,
        });
    }
}

/// Alt+F: move cursor forward one word.
pub(crate) fn handle_word_forward(app: &mut App) {
    let mut pos = app.interaction.input.cursor;
    let len = app.interaction.input.text.len();
    // Skip non-alphanumeric characters
    while pos < len {
        let c = app
            .interaction
            .input
            .text
            .get(pos..)
            .and_then(|s| s.chars().next());
        match c {
            Some(ch) if ch.is_alphanumeric() => break,
            Some(ch) => pos += ch.len_utf8(),
            None => break,
        }
    }
    // Skip alphanumeric characters
    while pos < len {
        let c = app
            .interaction
            .input
            .text
            .get(pos..)
            .and_then(|s| s.chars().next());
        match c {
            Some(ch) if !ch.is_alphanumeric() => break,
            Some(ch) => pos += ch.len_utf8(),
            None => break,
        }
    }
    app.interaction.input.cursor = pos;
    app.interaction.input.kill_ring.last_yank = None;
}

/// Alt+B: move cursor backward one word.
pub(crate) fn handle_word_backward(app: &mut App) {
    let mut pos = app.interaction.input.cursor;
    // Skip non-alphanumeric characters backwards
    while pos > 0 {
        let prev = app.prev_char_boundary(pos);
        let c = app
            .interaction
            .input
            .text
            .get(prev..pos)
            .and_then(|s| s.chars().next());
        match c {
            Some(ch) if ch.is_alphanumeric() => break,
            Some(_) => pos = prev,
            None => break,
        }
    }
    // Skip alphanumeric characters backwards
    while pos > 0 {
        let prev = app.prev_char_boundary(pos);
        let c = app
            .interaction
            .input
            .text
            .get(prev..pos)
            .and_then(|s| s.chars().next());
        match c {
            Some(ch) if !ch.is_alphanumeric() => break,
            Some(_) => pos = prev,
            None => break,
        }
    }
    app.interaction.input.cursor = pos;
    app.interaction.input.kill_ring.last_yank = None;
}

/// Ctrl+R: open reverse incremental history search.
pub(crate) fn handle_history_search_open(app: &mut App) {
    app.interaction.input.history_search = Some(crate::state::HistorySearchState {
        query: String::new(),
        match_index: None,
    });
}

/// Close history search without accepting.
pub(crate) fn handle_history_search_close(app: &mut App) {
    app.interaction.input.history_search = None;
}

/// Type a character into the history search query.
pub(crate) fn handle_history_search_input(app: &mut App, c: char) {
    if let Some(ref mut search) = app.interaction.input.history_search {
        search.query.push(c);
    }
    update_history_search(&mut app.interaction.input);
}

/// Backspace in history search query.
pub(crate) fn handle_history_search_backspace(app: &mut App) {
    let should_close = if let Some(ref mut search) = app.interaction.input.history_search {
        search.query.pop();
        search.query.is_empty()
    } else {
        false
    };
    if should_close {
        app.interaction.input.history_search = None;
    } else {
        update_history_search(&mut app.interaction.input);
    }
}

/// Ctrl+R again: find the next (older) match.
pub(crate) fn handle_history_search_next(app: &mut App) {
    let current = app
        .interaction
        .input
        .history_search
        .as_ref()
        .and_then(|s| s.match_index);
    let query = match app.interaction.input.history_search.as_ref() {
        Some(s) if !s.query.is_empty() => s.query.clone(),
        _ => return,
    };
    let new_match = find_history_match(&app.interaction.input.history, &query, current);
    if let Some(ref mut search) = app.interaction.input.history_search
        && new_match.is_some()
    {
        search.match_index = new_match;
    }
    apply_history_search_match(&mut app.interaction.input);
}

/// Accept the current history search result and close search.
pub(crate) fn handle_history_search_accept(app: &mut App) {
    app.interaction.input.history_search = None;
    // Text is already set from the match; cursor is at end.
}

/// Ctrl+J or backslash+Enter: insert a newline character at cursor position.
/// If the text ends with a backslash (from backslash+Enter), removes it first.
pub(crate) fn handle_newline_insert(app: &mut App) {
    // WHY: backslash+Enter sends NewlineInsert with a trailing '\' -- strip it
    if app.interaction.input.text.ends_with('\\') {
        app.interaction.input.text.pop();
        if app.interaction.input.cursor > app.interaction.input.text.len() {
            app.interaction.input.cursor = app.interaction.input.text.len();
        }
    }
    app.interaction
        .input
        .text
        .insert(app.interaction.input.cursor, '\n');
    app.interaction.input.cursor += 1;
    app.interaction.input.history_index = None;
    app.interaction.input.kill_ring.last_yank = None;
}

/// Ctrl+L: clear the screen and request a full redraw.
pub(crate) fn handle_clear_screen(app: &mut App) {
    app.viewport.frame_cache = None;
    app.viewport.dirty = true;
}

/// Ctrl+V: paste from clipboard. Text goes into input; images become attachments.
pub(crate) fn handle_clipboard_paste(app: &mut App) {
    match crate::clipboard::read_from_clipboard() {
        crate::clipboard::ClipboardContent::Text(text) => {
            app.interaction
                .input
                .text
                .insert_str(app.interaction.input.cursor, &text);
            app.interaction.input.cursor += text.len();
            app.interaction.input.history_index = None;
            app.interaction.input.kill_ring.last_yank = None;
        }
        crate::clipboard::ClipboardContent::Image {
            png_data,
            width,
            height,
        } => {
            app.interaction
                .input
                .image_attachments
                .push(ImageAttachment {
                    data: png_data,
                    mime_type: "image/png".to_string(),
                    width,
                    height,
                });
        }
        crate::clipboard::ClipboardContent::Empty => {}
    }
}

/// Cancel a queued message, restoring its text to the input for editing.
pub(crate) fn handle_queued_message_cancel(app: &mut App, index: usize) {
    if index < app.interaction.queued_messages.len() {
        let msg = app.interaction.queued_messages.remove(index);
        app.interaction.input.text = msg.text;
        app.interaction.input.cursor = app.interaction.input.text.len();
    }
}

#[expect(
    clippy::indexing_slicing,
    reason = "idx < history.len() is guaranteed by the match arms; the subtraction produces a valid reverse-index"
)]
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

#[expect(
    clippy::indexing_slicing,
    reason = "idx = i - 1 where Some(i) implies i was a previously stored idx < history.len(), so the reverse-index is valid"
)]
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
    app.interaction.input.image_attachments.clear();

    // WHY: if a turn is already streaming, queue the message instead of sending immediately
    if app.connection.active_turn_id.is_some() {
        app.interaction.queued_messages.push(QueuedMessage { text });
        return;
    }

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
    #[expect(
        clippy::disallowed_methods,
        reason = "theatron TUI reads configuration and exports from disk in synchronous initialization paths"
    )]
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

/// Send the next queued message after a turn completes.
pub(crate) fn send_next_queued(app: &mut App) {
    if app.connection.active_turn_id.is_some() {
        return;
    }
    if let Some(queued) = app.interaction.queued_messages.first() {
        let text = queued.text.clone();
        app.interaction.queued_messages.remove(0);
        app.send_message(&text);
    }
}

// -- private helpers --

fn find_history_match(
    history: &[String],
    query: &str,
    before_index: Option<usize>,
) -> Option<usize> {
    if query.is_empty() {
        return None;
    }
    let query_lower = query.to_lowercase();
    let end = before_index.unwrap_or(history.len());
    for i in (0..end).rev() {
        if let Some(entry) = history.get(i)
            && entry.to_lowercase().contains(&query_lower)
        {
            return Some(i);
        }
    }
    None
}

fn update_history_search(input: &mut crate::state::InputState) {
    let query = match input.history_search.as_ref() {
        Some(s) if !s.query.is_empty() => s.query.clone(),
        _ => return,
    };
    let new_match = find_history_match(&input.history, &query, None);
    if let Some(ref mut search) = input.history_search {
        search.match_index = new_match;
    }
    apply_history_search_match(input);
}

fn apply_history_search_match(input: &mut crate::state::InputState) {
    let idx = match input.history_search.as_ref().and_then(|s| s.match_index) {
        Some(i) => i,
        None => return,
    };
    if let Some(text) = input.history.get(idx) {
        input.text = text.clone();
        input.cursor = input.text.len();
    }
}

#[cfg(test)]
mod tests;
