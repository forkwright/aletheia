//! Single chat message component with role-based styling.

use dioxus::prelude::*;

use super::markdown::Markdown;
use crate::state::chat::{ChatMessage, Role, relative_time};

/// Whether two consecutive messages should be visually grouped.
///
/// Messages group when they share the same role and no more than
/// 60 seconds separate them.
#[must_use]
pub(crate) fn should_group(prev: &ChatMessage, current: &ChatMessage) -> bool {
    prev.role == current.role && (current.timestamp - prev.timestamp).unsigned_abs() <= 60
}

/// Render a single chat message with role-based styling.
#[component]
pub(crate) fn MessageBubble(
    message: ChatMessage,
    is_grouped: bool,
    agent_name: Option<String>,
) -> Element {
    let is_user = message.role == Role::User;
    let is_system = message.role == Role::System;

    let role_label = match message.role {
        Role::User => "You".to_string(),
        Role::Assistant => agent_name.unwrap_or_else(|| "Assistant".to_string()),
        Role::System => "System".to_string(),
    };

    let container_style = if is_user {
        "\
            display: flex; \
            flex-direction: column; \
            align-items: flex-end; \
            padding: 0 var(--space-4); \
        "
    } else {
        "\
            display: flex; \
            flex-direction: column; \
            align-items: flex-start; \
            padding: 0 var(--space-4); \
        "
    };

    let bubble_style = if is_user {
        "\
            background: var(--bg-surface); \
            border: 1px solid var(--border); \
            border-left: 3px solid var(--accent); \
            border-radius: var(--radius-lg); \
            padding: var(--space-3) var(--space-4); \
            max-width: min(75%, 720px); \
            color: var(--text-primary); \
            box-shadow: inset 0 1px 0 rgb(255 255 255 / 0.04); \
            animation: message-in 0.2s cubic-bezier(0.16, 1, 0.3, 1); \
        "
    } else if is_system {
        "\
            background: var(--bg-surface-dim); \
            border: 1px solid var(--border-separator); \
            border-left: 3px solid var(--role-system); \
            border-radius: var(--radius-lg); \
            padding: var(--space-2) var(--space-3); \
            max-width: min(85%, 720px); \
            color: var(--text-muted); \
            font-size: var(--text-sm); \
        "
    } else {
        // NOTE: assistant -- shaded elevated surface per design spec
        "\
            background: var(--bg-surface-bright); \
            border: 1px solid var(--border); \
            border-left: 3px solid var(--role-assistant); \
            border-radius: var(--radius-lg); \
            padding: var(--space-3) var(--space-4); \
            max-width: min(85%, 720px); \
            color: var(--text-primary); \
            box-shadow: inset 0 1px 0 rgb(255 255 255 / 0.04); \
            animation: message-in 0.2s cubic-bezier(0.16, 1, 0.3, 1); \
        "
    };

    let margin_top = if is_grouped {
        "margin-top: var(--space-1);"
    } else {
        "margin-top: var(--space-3);"
    };

    let timestamp = relative_time(message.timestamp);

    rsx! {
        div {
            style: "{container_style} {margin_top}",
            // Role label (hidden for grouped messages)
            if !is_grouped {
                div {
                    style: "
                        font-size: var(--text-xs);
                        color: {role_color(message.role)};
                        font-weight: var(--weight-semibold);
                        letter-spacing: 0.04em;
                        text-transform: uppercase;
                        margin-bottom: var(--space-1);
                        font-family: var(--font-body);
                    ",
                    "{role_label}"
                }
            }
            // Message bubble
            div {
                style: "{bubble_style}",
                if is_user || is_system {
                    // WHY: user/system messages render as plain text (no markdown)
                    div {
                        style: "white-space: pre-wrap; word-wrap: break-word; word-break: break-word; overflow-wrap: break-word; line-height: var(--leading-relaxed);",
                        "{message.content}"
                    }
                } else {
                    Markdown { content: message.content.clone() }
                }
            }
            // Timestamp and metadata footer
            div {
                style: "
                    display: flex;
                    gap: var(--space-2);
                    align-items: center;
                    margin-top: var(--space-1);
                    font-size: var(--text-xs);
                    color: var(--text-muted);
                    font-family: var(--font-body);
                    opacity: 0.5;
                    transition: opacity 150ms ease;
                ",
                span { "{timestamp}" }
                if message.role == Role::Assistant {
                    if let Some(model) = &message.model {
                        span {
                            style: "color: var(--text-muted);",
                            "{model}"
                        }
                    }
                    if message.tool_calls > 0 {
                        span {
                            style: "color: var(--accent-dim);",
                            "{message.tool_calls} tools"
                        }
                    }
                }
            }
        }
    }
}

fn role_color(role: Role) -> &'static str {
    match role {
        Role::User => "var(--role-user)",
        Role::Assistant => "var(--role-assistant)",
        Role::System => "var(--role-system)",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_message(role: Role, ts: i64) -> ChatMessage {
        ChatMessage {
            id: 1,
            role,
            content: "test".to_string(),
            timestamp: ts,
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
    fn should_group_same_role_close_time() {
        let a = make_message(Role::User, 1000);
        let b = make_message(Role::User, 1030);
        assert!(should_group(&a, &b));
    }

    #[test]
    fn should_not_group_different_roles() {
        let a = make_message(Role::User, 1000);
        let b = make_message(Role::Assistant, 1010);
        assert!(!should_group(&a, &b));
    }

    #[test]
    fn should_not_group_far_apart() {
        let a = make_message(Role::User, 1000);
        let b = make_message(Role::User, 1200);
        assert!(!should_group(&a, &b));
    }

    #[test]
    fn role_color_values() {
        assert_eq!(role_color(Role::User), "var(--role-user)");
        assert_eq!(role_color(Role::Assistant), "var(--role-assistant)");
        assert_eq!(role_color(Role::System), "var(--role-system)");
    }
}
