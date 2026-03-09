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
    let key = crate::id::ToolId::from(format!("msg:{idx}"));
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
        // Handle markdown link syntax: [text](url)
        if let Some(paren_start) = word.find("](") {
            let url_part = &word[paren_start + 2..];
            let candidate = url_part
                .trim_end_matches(')')
                .trim_end_matches(',')
                .trim_end_matches('.');
            if candidate.starts_with("http://") || candidate.starts_with("https://") {
                urls.push(candidate.to_string());
                continue;
            }
        }

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::test_helpers::*;

    #[test]
    fn select_prev_enters_selection_on_last() {
        let mut app = test_app_with_messages(vec![("user", "a"), ("assistant", "b")]);
        handle_select_prev(&mut app);
        assert_eq!(app.selected_message, Some(1));
        assert!(!app.auto_scroll);
    }

    #[test]
    fn select_prev_decrements() {
        let mut app = test_app_with_messages(vec![("user", "a"), ("assistant", "b")]);
        app.selected_message = Some(1);
        handle_select_prev(&mut app);
        assert_eq!(app.selected_message, Some(0));
    }

    #[test]
    fn select_prev_stops_at_zero() {
        let mut app = test_app_with_messages(vec![("user", "a")]);
        app.selected_message = Some(0);
        handle_select_prev(&mut app);
        assert_eq!(app.selected_message, Some(0));
    }

    #[test]
    fn select_prev_empty_messages_noop() {
        let mut app = test_app();
        handle_select_prev(&mut app);
        assert!(app.selected_message.is_none());
    }

    #[test]
    fn select_next_enters_selection_on_last() {
        let mut app = test_app_with_messages(vec![("user", "a"), ("assistant", "b")]);
        handle_select_next(&mut app);
        assert_eq!(app.selected_message, Some(1));
    }

    #[test]
    fn select_next_increments() {
        let mut app = test_app_with_messages(vec![("user", "a"), ("assistant", "b")]);
        app.selected_message = Some(0);
        handle_select_next(&mut app);
        assert_eq!(app.selected_message, Some(1));
    }

    #[test]
    fn select_next_stops_at_end() {
        let mut app = test_app_with_messages(vec![("user", "a"), ("assistant", "b")]);
        app.selected_message = Some(1);
        handle_select_next(&mut app);
        assert_eq!(app.selected_message, Some(1));
    }

    #[test]
    fn deselect_clears_selection() {
        let mut app = test_app_with_messages(vec![("user", "a")]);
        app.selected_message = Some(0);
        app.auto_scroll = false;
        handle_deselect(&mut app);
        assert!(app.selected_message.is_none());
        assert!(app.auto_scroll);
        assert_eq!(app.selection, SelectionContext::None);
    }

    #[test]
    fn select_first_goes_to_zero() {
        let mut app = test_app_with_messages(vec![("user", "a"), ("assistant", "b")]);
        app.selected_message = Some(1);
        handle_select_first(&mut app);
        assert_eq!(app.selected_message, Some(0));
    }

    #[test]
    fn select_first_empty_noop() {
        let mut app = test_app();
        handle_select_first(&mut app);
        assert!(app.selected_message.is_none());
    }

    #[test]
    fn select_last_goes_to_end() {
        let mut app = test_app_with_messages(vec![("user", "a"), ("assistant", "b")]);
        app.selected_message = Some(0);
        handle_select_last(&mut app);
        assert_eq!(app.selected_message, Some(1));
    }

    #[test]
    fn sync_selection_context_user_message() {
        let mut app = test_app_with_messages(vec![("user", "hello")]);
        app.selected_message = Some(0);
        sync_selection_context(&mut app);
        assert!(matches!(
            app.selection,
            SelectionContext::UserMessage { index: 0 }
        ));
    }

    #[test]
    fn sync_selection_context_agent_response_with_code() {
        let mut app = test_app_with_messages(vec![("assistant", "here is ```code```")]);
        app.selected_message = Some(0);
        sync_selection_context(&mut app);
        match &app.selection {
            SelectionContext::AgentResponse {
                index,
                has_code,
                has_links,
            } => {
                assert_eq!(*index, 0);
                assert!(*has_code);
                assert!(!*has_links);
            }
            other => panic!("expected AgentResponse, got {:?}", other),
        }
    }

    #[test]
    fn sync_selection_context_agent_response_with_links() {
        let mut app = test_app_with_messages(vec![("assistant", "see https://example.com")]);
        app.selected_message = Some(0);
        sync_selection_context(&mut app);
        match &app.selection {
            SelectionContext::AgentResponse {
                has_links, ..
            } => {
                assert!(*has_links);
            }
            other => panic!("expected AgentResponse, got {:?}", other),
        }
    }

    #[test]
    fn extract_first_code_block_basic() {
        let text = "Some text\n```rust\nlet x = 1;\n```\nMore text";
        let code = extract_first_code_block(text);
        assert_eq!(code.as_deref(), Some("let x = 1;"));
    }

    #[test]
    fn extract_first_code_block_multiline() {
        let text = "```\nline1\nline2\n```";
        let code = extract_first_code_block(text);
        assert_eq!(code.as_deref(), Some("line1\nline2"));
    }

    #[test]
    fn extract_first_code_block_none_when_no_block() {
        let text = "no code blocks here";
        assert!(extract_first_code_block(text).is_none());
    }

    #[test]
    fn extract_first_code_block_unclosed() {
        let text = "```\ncode without closing";
        assert!(extract_first_code_block(text).is_none());
    }

    #[test]
    fn extract_urls_basic() {
        let text = "Visit https://example.com for more";
        let urls = extract_urls(text);
        assert_eq!(urls, vec!["https://example.com"]);
    }

    #[test]
    fn extract_urls_multiple() {
        let text = "See http://a.com and https://b.com";
        let urls = extract_urls(text);
        assert_eq!(urls.len(), 2);
    }

    #[test]
    fn extract_urls_markdown_syntax() {
        let text = "[link](https://example.com)";
        let urls = extract_urls(text);
        assert_eq!(urls, vec!["https://example.com"]);
    }

    #[test]
    fn extract_urls_trailing_punctuation() {
        let text = "See https://example.com.";
        let urls = extract_urls(text);
        assert_eq!(urls, vec!["https://example.com"]);
    }

    #[test]
    fn extract_urls_no_urls() {
        let text = "No links here, just text";
        let urls = extract_urls(text);
        assert!(urls.is_empty());
    }

    #[test]
    fn action_edit_only_user_messages() {
        let mut app = test_app_with_messages(vec![("assistant", "can't edit this")]);
        app.selected_message = Some(0);
        handle_message_action(&mut app, MessageActionKind::Edit);
        // Should show toast, not modify input
        assert!(app.error_toast.is_some());
        assert!(app.input.text.is_empty());
    }

    #[test]
    fn action_edit_user_message() {
        let mut app = test_app_with_messages(vec![("user", "edit me")]);
        app.selected_message = Some(0);
        handle_message_action(&mut app, MessageActionKind::Edit);
        assert_eq!(app.input.text, "edit me");
        assert_eq!(app.input.cursor, 7);
        assert!(app.selected_message.is_none());
        assert!(app.messages.is_empty());
    }

    #[test]
    fn action_delete_user_message() {
        let mut app = test_app_with_messages(vec![
            ("user", "delete me"),
            ("assistant", "response"),
        ]);
        app.selected_message = Some(0);
        handle_message_action(&mut app, MessageActionKind::Delete);
        assert_eq!(app.messages.len(), 1);
        assert_eq!(app.messages[0].role, "assistant");
    }

    #[test]
    fn action_delete_non_user_message() {
        let mut app = test_app_with_messages(vec![("assistant", "can't delete")]);
        app.selected_message = Some(0);
        handle_message_action(&mut app, MessageActionKind::Delete);
        assert_eq!(app.messages.len(), 1); // unchanged
        assert!(app.error_toast.is_some());
    }

    #[test]
    fn action_delete_last_message_clears_selection() {
        let mut app = test_app_with_messages(vec![("user", "only one")]);
        app.selected_message = Some(0);
        handle_message_action(&mut app, MessageActionKind::Delete);
        assert!(app.messages.is_empty());
        assert!(app.selected_message.is_none());
        assert_eq!(app.selection, SelectionContext::None);
    }

    #[test]
    fn action_inspect_toggles_tool_expanded() {
        let mut app = test_app_with_messages(vec![("assistant", "response")]);
        app.messages[0].tool_calls.push(crate::state::ToolCallInfo {
            name: "test_tool".to_string(),
            duration_ms: Some(100),
            is_error: false,
        });
        app.selected_message = Some(0);
        handle_message_action(&mut app, MessageActionKind::Inspect);
        assert_eq!(app.tool_expanded.len(), 1);
        // Toggle off
        handle_message_action(&mut app, MessageActionKind::Inspect);
        assert_eq!(app.tool_expanded.len(), 0);
    }

    #[test]
    fn action_inspect_no_tool_calls_shows_toast() {
        let mut app = test_app_with_messages(vec![("assistant", "no tools")]);
        app.selected_message = Some(0);
        handle_message_action(&mut app, MessageActionKind::Inspect);
        assert!(app.error_toast.is_some());
    }

    #[test]
    fn handle_message_action_out_of_bounds_noop() {
        let mut app = test_app_with_messages(vec![("user", "a")]);
        app.selected_message = Some(5); // out of bounds
        handle_message_action(&mut app, MessageActionKind::Copy);
        // Should not panic
    }

    #[test]
    fn handle_message_action_no_selection_noop() {
        let mut app = test_app_with_messages(vec![("user", "a")]);
        handle_message_action(&mut app, MessageActionKind::Copy);
        // Should not panic
    }
}
