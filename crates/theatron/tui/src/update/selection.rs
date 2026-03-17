/// Message selection handlers: navigation, actions, and SelectionContext sync.
use crate::app::App;
use crate::msg::{ErrorToast, MessageActionKind};
use crate::state::SelectionContext;

pub(crate) fn handle_select_prev(app: &mut App) {
    let count = app.dashboard.messages.len();
    if count == 0 {
        return;
    }
    match app.interaction.selected_message {
        None => {
            let idx = count - 1;
            app.interaction.selected_message = Some(idx);
            app.viewport.render.auto_scroll = false;
        }
        Some(idx) => {
            if idx > 0 {
                app.interaction.selected_message = Some(idx - 1);
            }
        }
    }
    sync_selection_context(app);
}

pub(crate) fn handle_select_next(app: &mut App) {
    let count = app.dashboard.messages.len();
    if count == 0 {
        return;
    }
    match app.interaction.selected_message {
        None => {
            let idx = count - 1;
            app.interaction.selected_message = Some(idx);
            app.viewport.render.auto_scroll = false;
        }
        Some(idx) => {
            if idx + 1 < count {
                app.interaction.selected_message = Some(idx + 1);
            }
        }
    }
    sync_selection_context(app);
}

pub(crate) fn handle_deselect(app: &mut App) {
    app.interaction.selected_message = None;
    app.interaction.selection = SelectionContext::None;
    app.scroll_to_bottom();
}

pub(crate) fn handle_select_first(app: &mut App) {
    if app.dashboard.messages.is_empty() {
        return;
    }
    app.interaction.selected_message = Some(0);
    app.viewport.render.auto_scroll = false;
    sync_selection_context(app);
}

pub(crate) fn handle_select_last(app: &mut App) {
    if app.dashboard.messages.is_empty() {
        return;
    }
    app.interaction.selected_message = Some(app.dashboard.messages.len() - 1);
    app.viewport.render.auto_scroll = false;
    sync_selection_context(app);
}

pub(crate) fn handle_message_action(app: &mut App, action: MessageActionKind) {
    let idx = match app.interaction.selected_message {
        Some(i) if i < app.dashboard.messages.len() => i,
        _ => return,
    };

    match action {
        MessageActionKind::Copy => action_copy(app, idx),
        MessageActionKind::YankCodeBlock => action_yank_code_block(app, idx),
        MessageActionKind::Edit => action_edit(app, idx),
        MessageActionKind::Delete => action_delete(app, idx),
        MessageActionKind::OpenLinks => action_open_links(app, idx),
        MessageActionKind::Inspect => action_inspect(app, idx),
        MessageActionKind::QuoteInReply => action_quote_in_reply(app, idx),
        MessageActionKind::RateResponse => show_toast(app, "Response rated — thank you"),
        MessageActionKind::FlagForReview => show_toast(app, "Flagged for review"),
    }
}

fn action_copy(app: &mut App, idx: usize) {
    let text = &app.dashboard.messages[idx].text;
    match crate::clipboard::copy_to_clipboard(text) {
        Ok(()) => show_toast(app, "Copied to clipboard"),
        Err(e) => {
            tracing::error!("clipboard copy failed: {e}");
            show_toast(app, "Clipboard copy failed");
        }
    }
}

fn action_yank_code_block(app: &mut App, idx: usize) {
    let text = &app.dashboard.messages[idx].text;
    if let Some(code) = extract_first_code_block(text) {
        match crate::clipboard::copy_to_clipboard(&code) {
            Ok(()) => show_toast(app, "Code block copied"),
            Err(e) => {
                tracing::error!("clipboard copy failed: {e}");
                show_toast(app, "Clipboard copy failed");
            }
        }
    } else {
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
    if app.dashboard.messages[idx].role != "user" {
        show_toast(app, "Can only edit user messages");
        return;
    }
    let text = app.dashboard.messages[idx].text.clone();
    app.dashboard.messages.remove(idx);
    app.interaction.selected_message = None;
    app.interaction.selection = SelectionContext::None;
    app.interaction.input.text = text;
    app.interaction.input.cursor = app.interaction.input.text.len();
}

fn action_delete(app: &mut App, idx: usize) {
    if app.dashboard.messages[idx].role != "user" {
        show_toast(app, "Can only delete user messages");
        return;
    }
    app.dashboard.messages.remove(idx);
    let count = app.dashboard.messages.len();
    if count == 0 {
        app.interaction.selected_message = None;
        app.interaction.selection = SelectionContext::None;
    } else if idx >= count {
        app.interaction.selected_message = Some(count - 1);
        sync_selection_context(app);
    } else {
        sync_selection_context(app);
    }
    show_toast(app, "Message deleted");
}

fn action_open_links(app: &mut App, idx: usize) {
    let text = &app.dashboard.messages[idx].text;
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
    let msg = &app.dashboard.messages[idx];
    if msg.tool_calls.is_empty() {
        show_toast(app, "No tool calls to inspect");
        return;
    }
    let key = crate::id::ToolId::from(format!("msg:{idx}"));
    if app.interaction.tool_expanded.contains(&key) {
        app.interaction.tool_expanded.remove(&key);
    } else {
        app.interaction.tool_expanded.insert(key);
    }
}

fn action_quote_in_reply(app: &mut App, idx: usize) {
    let text = &app.dashboard.messages[idx].text;
    let quoted: String = text.lines().map(|l| format!("> {l}\n")).collect();
    if app.interaction.input.text.is_empty() {
        app.interaction.input.text = quoted;
    } else {
        app.interaction.input.text.push('\n');
        app.interaction.input.text.push_str(&quoted);
    }
    app.interaction.input.cursor = app.interaction.input.text.len();
    show_toast(app, "Quoted in reply");
}

fn sync_selection_context(app: &mut App) {
    app.interaction.selection = match app.interaction.selected_message {
        Some(idx) if idx < app.dashboard.messages.len() => {
            let msg = &app.dashboard.messages[idx];
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
        // NOTE: handle markdown link syntax [text](url) before falling through to plain URL detection
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
    app.viewport.error_toast = Some(ErrorToast::new(message.to_string()));
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions may panic on failure")]
mod tests {
    use super::*;
    use crate::app::test_helpers::*;

    #[test]
    fn select_prev_enters_selection_on_last() {
        let mut app = test_app_with_messages(vec![("user", "a"), ("assistant", "b")]);
        handle_select_prev(&mut app);
        assert_eq!(app.interaction.selected_message, Some(1));
        assert!(!app.viewport.render.auto_scroll);
    }

    #[test]
    fn select_prev_decrements() {
        let mut app = test_app_with_messages(vec![("user", "a"), ("assistant", "b")]);
        app.interaction.selected_message = Some(1);
        handle_select_prev(&mut app);
        assert_eq!(app.interaction.selected_message, Some(0));
    }

    #[test]
    fn select_prev_stops_at_zero() {
        let mut app = test_app_with_messages(vec![("user", "a")]);
        app.interaction.selected_message = Some(0);
        handle_select_prev(&mut app);
        assert_eq!(app.interaction.selected_message, Some(0));
    }

    #[test]
    fn select_prev_empty_messages_noop() {
        let mut app = test_app();
        handle_select_prev(&mut app);
        assert!(app.interaction.selected_message.is_none());
    }

    #[test]
    fn select_next_enters_selection_on_last() {
        let mut app = test_app_with_messages(vec![("user", "a"), ("assistant", "b")]);
        handle_select_next(&mut app);
        assert_eq!(app.interaction.selected_message, Some(1));
    }

    #[test]
    fn select_next_increments() {
        let mut app = test_app_with_messages(vec![("user", "a"), ("assistant", "b")]);
        app.interaction.selected_message = Some(0);
        handle_select_next(&mut app);
        assert_eq!(app.interaction.selected_message, Some(1));
    }

    #[test]
    fn select_next_stops_at_end() {
        let mut app = test_app_with_messages(vec![("user", "a"), ("assistant", "b")]);
        app.interaction.selected_message = Some(1);
        handle_select_next(&mut app);
        assert_eq!(app.interaction.selected_message, Some(1));
    }

    #[test]
    fn deselect_clears_selection() {
        let mut app = test_app_with_messages(vec![("user", "a")]);
        app.interaction.selected_message = Some(0);
        app.viewport.render.auto_scroll = false;
        handle_deselect(&mut app);
        assert!(app.interaction.selected_message.is_none());
        assert!(app.viewport.render.auto_scroll);
        assert_eq!(app.interaction.selection, SelectionContext::None);
    }

    #[test]
    fn select_first_goes_to_zero() {
        let mut app = test_app_with_messages(vec![("user", "a"), ("assistant", "b")]);
        app.interaction.selected_message = Some(1);
        handle_select_first(&mut app);
        assert_eq!(app.interaction.selected_message, Some(0));
    }

    #[test]
    fn select_first_empty_noop() {
        let mut app = test_app();
        handle_select_first(&mut app);
        assert!(app.interaction.selected_message.is_none());
    }

    #[test]
    fn select_last_goes_to_end() {
        let mut app = test_app_with_messages(vec![("user", "a"), ("assistant", "b")]);
        app.interaction.selected_message = Some(0);
        handle_select_last(&mut app);
        assert_eq!(app.interaction.selected_message, Some(1));
    }

    #[test]
    fn sync_selection_context_user_message() {
        let mut app = test_app_with_messages(vec![("user", "hello")]);
        app.interaction.selected_message = Some(0);
        sync_selection_context(&mut app);
        assert!(matches!(
            app.interaction.selection,
            SelectionContext::UserMessage { index: 0 }
        ));
    }

    #[test]
    fn sync_selection_context_agent_response_with_code() {
        let mut app = test_app_with_messages(vec![("assistant", "here is ```code```")]);
        app.interaction.selected_message = Some(0);
        sync_selection_context(&mut app);
        match &app.interaction.selection {
            SelectionContext::AgentResponse {
                index,
                has_code,
                has_links,
            } => {
                assert_eq!(*index, 0);
                assert!(*has_code);
                assert!(!*has_links);
            }
            other => unreachable!("expected AgentResponse, got {:?}", other),
        }
    }

    #[test]
    fn sync_selection_context_agent_response_with_links() {
        let mut app = test_app_with_messages(vec![("assistant", "see https://example.com")]);
        app.interaction.selected_message = Some(0);
        sync_selection_context(&mut app);
        match &app.interaction.selection {
            SelectionContext::AgentResponse { has_links, .. } => {
                assert!(*has_links);
            }
            other => unreachable!("expected AgentResponse, got {:?}", other),
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
        app.interaction.selected_message = Some(0);
        handle_message_action(&mut app, MessageActionKind::Edit);
        // Should show toast, not modify input
        assert!(app.viewport.error_toast.is_some());
        assert!(app.interaction.input.text.is_empty());
    }

    #[test]
    fn action_edit_user_message() {
        let mut app = test_app_with_messages(vec![("user", "edit me")]);
        app.interaction.selected_message = Some(0);
        handle_message_action(&mut app, MessageActionKind::Edit);
        assert_eq!(app.interaction.input.text, "edit me");
        assert_eq!(app.interaction.input.cursor, 7);
        assert!(app.interaction.selected_message.is_none());
        assert!(app.dashboard.messages.is_empty());
    }

    #[test]
    fn action_delete_user_message() {
        let mut app =
            test_app_with_messages(vec![("user", "delete me"), ("assistant", "response")]);
        app.interaction.selected_message = Some(0);
        handle_message_action(&mut app, MessageActionKind::Delete);
        assert_eq!(app.dashboard.messages.len(), 1);
        assert_eq!(app.dashboard.messages[0].role, "assistant");
    }

    #[test]
    fn action_delete_non_user_message() {
        let mut app = test_app_with_messages(vec![("assistant", "can't delete")]);
        app.interaction.selected_message = Some(0);
        handle_message_action(&mut app, MessageActionKind::Delete);
        assert_eq!(app.dashboard.messages.len(), 1); // unchanged
        assert!(app.viewport.error_toast.is_some());
    }

    #[test]
    fn action_delete_last_message_clears_selection() {
        let mut app = test_app_with_messages(vec![("user", "only one")]);
        app.interaction.selected_message = Some(0);
        handle_message_action(&mut app, MessageActionKind::Delete);
        assert!(app.dashboard.messages.is_empty());
        assert!(app.interaction.selected_message.is_none());
        assert_eq!(app.interaction.selection, SelectionContext::None);
    }

    #[test]
    fn action_inspect_toggles_tool_expanded() {
        let mut app = test_app_with_messages(vec![("assistant", "response")]);
        app.dashboard.messages[0]
            .tool_calls
            .push(crate::state::ToolCallInfo {
                name: "test_tool".to_string(),
                duration_ms: Some(100),
                is_error: false,
            });
        app.interaction.selected_message = Some(0);
        handle_message_action(&mut app, MessageActionKind::Inspect);
        assert_eq!(app.interaction.tool_expanded.len(), 1);
        // Toggle off
        handle_message_action(&mut app, MessageActionKind::Inspect);
        assert_eq!(app.interaction.tool_expanded.len(), 0);
    }

    #[test]
    fn action_inspect_no_tool_calls_shows_toast() {
        let mut app = test_app_with_messages(vec![("assistant", "no tools")]);
        app.interaction.selected_message = Some(0);
        handle_message_action(&mut app, MessageActionKind::Inspect);
        assert!(app.viewport.error_toast.is_some());
    }

    #[test]
    fn handle_message_action_out_of_bounds_noop() {
        let mut app = test_app_with_messages(vec![("user", "a")]);
        app.interaction.selected_message = Some(5); // out of bounds
        handle_message_action(&mut app, MessageActionKind::Copy);
        // Should not panic
    }

    #[test]
    fn handle_message_action_no_selection_noop() {
        let mut app = test_app_with_messages(vec![("user", "a")]);
        handle_message_action(&mut app, MessageActionKind::Copy);
        // Should not panic
    }

    #[test]
    fn quote_in_reply_populates_input() {
        let mut app = test_app_with_messages(vec![("assistant", "line1\nline2")]);
        app.interaction.selected_message = Some(0);
        handle_message_action(&mut app, MessageActionKind::QuoteInReply);
        assert!(app.interaction.input.text.contains("> line1"));
        assert!(app.interaction.input.text.contains("> line2"));
        assert_eq!(
            app.interaction.input.cursor,
            app.interaction.input.text.len()
        );
    }

    #[test]
    fn quote_in_reply_appends_to_existing_input() {
        let mut app = test_app_with_messages(vec![("assistant", "quoted")]);
        app.interaction.input.text = "existing".to_string();
        app.interaction.input.cursor = 8;
        app.interaction.selected_message = Some(0);
        handle_message_action(&mut app, MessageActionKind::QuoteInReply);
        assert!(app.interaction.input.text.starts_with("existing\n"));
        assert!(app.interaction.input.text.contains("> quoted"));
    }

    #[test]
    fn rate_response_shows_toast() {
        let mut app = test_app_with_messages(vec![("assistant", "response")]);
        app.interaction.selected_message = Some(0);
        handle_message_action(&mut app, MessageActionKind::RateResponse);
        assert!(app.viewport.error_toast.is_some());
        assert!(app.viewport.error_toast.unwrap().message.contains("rated"));
    }

    #[test]
    fn flag_for_review_shows_toast() {
        let mut app = test_app_with_messages(vec![("assistant", "response")]);
        app.interaction.selected_message = Some(0);
        handle_message_action(&mut app, MessageActionKind::FlagForReview);
        assert!(app.viewport.error_toast.is_some());
        assert!(
            app.viewport
                .error_toast
                .unwrap()
                .message
                .contains("Flagged")
        );
    }
}
