//! Conversation export: convert chat messages to markdown for clipboard.

use crate::state::chat::{ChatMessage, Role};

/// Convert a slice of chat messages into markdown text.
///
/// Format:
/// ```text
/// ## User
/// message content
///
/// ---
///
/// ## Assistant
/// response content
///
/// ---
/// ```
#[must_use]
pub(crate) fn messages_to_markdown(messages: &[ChatMessage]) -> String {
    let mut out = String::new();

    for (i, msg) in messages.iter().enumerate() {
        let heading = match msg.role {
            Role::User => "User",
            Role::Assistant => "Assistant",
            Role::System => "System",
        };

        out.push_str("## ");
        out.push_str(heading);
        out.push('\n');
        out.push_str(&msg.content);
        out.push('\n');

        if i + 1 < messages.len() {
            out.push_str("\n---\n\n");
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn user_msg(content: &str) -> ChatMessage {
        ChatMessage {
            id: 1,
            role: Role::User,
            content: content.to_string(),
            timestamp: 0,
            agent_id: None,
            tool_calls: 0,
            thinking_content: None,
            is_streaming: false,
            model: None,
            input_tokens: 0,
            output_tokens: 0,
        }
    }

    fn assistant_msg(content: &str) -> ChatMessage {
        ChatMessage {
            id: 2,
            role: Role::Assistant,
            content: content.to_string(),
            timestamp: 0,
            agent_id: None,
            tool_calls: 0,
            thinking_content: None,
            is_streaming: false,
            model: None,
            input_tokens: 0,
            output_tokens: 0,
        }
    }

    #[test]
    fn empty_messages() {
        assert_eq!(messages_to_markdown(&[]), "");
    }

    #[test]
    fn single_message() {
        let msgs = vec![user_msg("hello")];
        let md = messages_to_markdown(&msgs);
        assert_eq!(md, "## User\nhello\n");
    }

    #[test]
    fn conversation_pair() {
        let msgs = vec![user_msg("hello"), assistant_msg("hi there")];
        let md = messages_to_markdown(&msgs);
        assert_eq!(md, "## User\nhello\n\n---\n\n## Assistant\nhi there\n");
    }
}
