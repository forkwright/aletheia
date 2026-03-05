/// Message selection handlers — navigation, actions, and SelectionContext sync.
use crate::app::App;
use crate::msg::{ErrorToast, MessageActionKind};
use crate::state::SelectionContext;

pub(crate) fn handle_select_prev(app: &mut App) {
    let count = app.messages.len();
    if count == 0 {
        return;
    }
    match app.selected_message {
        None => {
            // Enter selection mode on last message
            let idx = count - 1;
            app.selected_message = Some(idx);
            app.auto_scroll = false;
        }
        Some(idx) => {
            if idx > 0 {
                app.selected_message = Some(idx - 1);
            }
        }
    }
    sync_selection_context(app);
}

pub(crate) fn handle_select_next(app: &mut App) {
    let count = app.messages.len();
    if count == 0 {
        return;
    }
    match app.selected_message {
        None => {
            // Enter selection mode on last message
            let idx = count - 1;
            app.selected_message = Some(idx);
            app.auto_scroll = false;
        }
        Some(idx) => {
            if idx + 1 < count {
                app.selected_message = Some(idx + 1);
            }
        }
    }
    sync_selection_context(app);
}

pub(crate) fn handle_deselect(app: &mut App) {
    app.selected_message = None;
    app.selection = SelectionContext::None;
    app.scroll_to_bottom();
}

pub(crate) fn handle_select_first(app: &mut App) {
    if app.messages.is_empty() {
        return;
    }
    app.selected_message = Some(0);
    app.auto_scroll = false;
    sync_selection_context(app);
}

pub(crate) fn handle_select_last(app: &mut App) {
    if app.messages.is_empty() {
        return;
    }
    app.selected_message = Some(app.messages.len() - 1);
    app.auto_scroll = false;
    sync_selection_context(app);
}

pub(crate) fn handle_message_action(app: &mut App, action: MessageActionKind) {
    let idx = match app.selected_message {
        Some(i) if i < app.messages.len() => i,
        _ => return,
    };

    match action {
        MessageActionKind::Copy => action_copy(app, idx),
        MessageActionKind::YankCodeBlock => action_yank_code_block(app, idx),
        MessageActionKind::Edit => action_edit(app, idx),
        MessageActionKind::Delete => action_delete(app, idx),
        MessageActionKind::OpenLinks => action_open_links(app, idx),
        MessageActionKind::Inspect => action_inspect(app, idx),
    }
}

fn action_copy(app: &mut App, idx: usize) {
    let text = &app.messages[idx].text;
    match crate::clipboard::copy_to_clipboard(text) {
        Ok(()) => show_toast(app, "Copied to clipboard"),
        Err(e) => {
            tracing::error!("clipboard copy failed: {e}");
            show_toast(app, "Clipboard copy failed");
        }
    }
}

fn action_yank_code_block(app: &mut App, idx: usize) {
    let text = &app.messages[idx].text;
    if let Some(code) = extract_first_code_block(text) {
        match crate::clipboard::copy_to_clipboard(&code) {
            Ok(()) => show_toast(app, "Code block copied"),
            Err(e) => {
                tracing::error!("clipboard copy failed: {e}");
                show_toast(app, "Clipboard copy failed");
            }
        }
    } else {
        // No code block — copy full message
        match crate::clipboard::copy_to_clipboard(text) {
            Ok(()) => show_toast(app, "No code block found — copied full message"),
            Err(e) => {
                tracing::error!("clipboard copy failed: {e}");
                show_toast(app, "Clipboard copy failed");
            }
        }
    }
}

fn action_edit(app: &mut App, idx: usize) {
    if app.messages[idx].role != "user" {
        show_toast(app, "Can only edit user messages");
        return;
    }
    let text = app.messages[idx].text.clone();
    app.messages.remove(idx);
    app.selected_message = None;
    app.selection = SelectionContext::None;
    app.input.text = text;
    app.input.cursor = app.input.text.len();
}

fn action_delete(app: &mut App, idx: usize) {
    if app.messages[idx].role != "user" {
        show_toast(app, "Can only delete user messages");
        return;
    }
    app.messages.remove(idx);
    // Fix selection index after removal
    let count = app.messages.len();
    if count == 0 {
        app.selected_message = None;
        app.selection = SelectionContext::None;
    } else if idx >= count {
        app.selected_message = Some(count - 1);
        sync_selection_context(app);
    } else {
        sync_selection_context(app);
    }
    show_toast(app, "Message deleted");
}

fn action_open_links(app: &mut App, idx: usize) {
    let text = &app.messages[idx].text;
    let urls = extract_urls(text);
    match urls.len() {
        0 => show_toast(app, "No links found"),
        1 => {
            if let Err(e) = open::that(&urls[0]) {
                tracing::error!("failed to open URL: {e}");
                show_toast(app, "Failed to open link");
            }
        }
        n => {
            // Open the first link and notify about the rest
            if let Err(e) = open::that(&urls[0]) {
                tracing::error!("failed to open URL: {e}");
                show_toast(app, "Failed to open link");
            } else {
                show_toast(app, &format!("Opened 1 of {} links", n));
            }
        }
    }
}

fn action_inspect(app: &mut App, idx: usize) {
    let msg = &app.messages[idx];
    if msg.tool_calls.is_empty() {
        show_toast(app, "No tool calls to inspect");
        return;
    }
    // Toggle expanded state for all tool calls on this message
    let key = format!("msg:{idx}");
    if app.tool_expanded.contains(&key) {
        app.tool_expanded.remove(&key);
    } else {
        app.tool_expanded.insert(key);
    }
}

fn sync_selection_context(app: &mut App) {
    app.selection = match app.selected_message {
        Some(idx) if idx < app.messages.len() => {
            let msg = &app.messages[idx];
            match msg.role.as_str() {
                "user" => SelectionContext::UserMessage { index: idx },
                "assistant" => SelectionContext::AgentResponse {
                    index: idx,
                    has_code: msg.text.contains("```"),
                    has_links: msg.text.contains("http"),
                },
                _ => SelectionContext::None,
            }
        }
        _ => SelectionContext::None,
    };
}

fn extract_first_code_block(text: &str) -> Option<String> {
    let mut lines = text.lines();
    let mut in_block = false;
    let mut code = String::new();

    for line in &mut lines {
        if !in_block {
            if line.trim_start().starts_with("```") {
                in_block = true;
                continue;
            }
        } else if line.trim_start().starts_with("```") {
            return Some(code);
        } else {
            if !code.is_empty() {
                code.push('\n');
            }
            code.push_str(line);
        }
    }
    None
}

fn extract_urls(text: &str) -> Vec<String> {
    let mut urls = Vec::new();
    for word in text.split_whitespace() {
        // Handle URLs that may be wrapped in markdown link syntax
        let candidate = word
            .trim_start_matches('(')
            .trim_start_matches('[')
            .trim_end_matches(')')
            .trim_end_matches(']')
            .trim_end_matches(',')
            .trim_end_matches('.');
        if candidate.starts_with("http://") || candidate.starts_with("https://") {
            urls.push(candidate.to_string());
        }
    }
    urls
}

fn show_toast(app: &mut App, message: &str) {
    app.error_toast = Some(ErrorToast::new(message.to_string()));
}
