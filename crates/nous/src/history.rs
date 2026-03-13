//! History stage — loads conversation context from the session store.
//!
//! Retrieves the most recent messages that fit within the remaining token
//! budget, converts them to pipeline messages, and appends the current
//! user message at the end.

use snafu::ResultExt;
use tracing::debug;

use aletheia_mneme::store::SessionStore;
use aletheia_mneme::types::Role;

use crate::error;
use crate::pipeline::PipelineMessage;

/// Configuration for the history stage.
#[derive(Debug, Clone)]
pub struct HistoryConfig {
    /// Maximum number of history messages to load.
    pub max_messages: usize,
    /// Reserve tokens for the user's current message.
    pub reserve_for_current: i64,
    /// Whether to include tool-result messages.
    pub include_tool_messages: bool,
}

impl Default for HistoryConfig {
    fn default() -> Self {
        Self {
            max_messages: 50,
            reserve_for_current: 4000,
            include_tool_messages: true,
        }
    }
}

/// Result of the history stage.
#[derive(Debug, Clone)]
pub struct HistoryResult {
    /// Number of messages loaded from store.
    pub messages_loaded: usize,
    /// Total tokens consumed by loaded history.
    pub tokens_consumed: i64,
    /// Whether history was truncated to fit budget.
    pub truncated: bool,
}

/// Load conversation history and append the current user message.
///
/// Retrieves the most recent messages from the session store that fit within
/// the given token budget, converts them to [`PipelineMessage`]s, and appends
/// the current user message at the end.
///
/// System-role messages are always skipped (they're in the system prompt).
/// Tool-result messages are included or skipped based on `config.include_tool_messages`.
#[expect(clippy::cast_possible_wrap, reason = "message length fits in i64")]
pub fn load_history(
    store: &SessionStore,
    session_id: &str,
    budget: i64,
    config: &HistoryConfig,
    current_message: &str,
) -> error::Result<(Vec<PipelineMessage>, HistoryResult)> {
    let current_tokens = current_message.len() as i64 / 4;
    let available = budget - config.reserve_for_current - current_tokens;

    if available <= 0 {
        let messages = vec![PipelineMessage {
            role: "user".to_owned(),
            content: current_message.to_owned(),
            token_estimate: current_tokens,
        }];
        return Ok((
            messages,
            HistoryResult {
                messages_loaded: 0,
                tokens_consumed: 0,
                truncated: false,
            },
        ));
    }

    let budget_messages = store
        .get_history_with_budget(session_id, available)
        .context(error::StoreSnafu)?;
    let all_messages = store
        .get_history(session_id, None)
        .context(error::StoreSnafu)?;

    let total_in_store = all_messages
        .iter()
        .filter(|m| m.role != Role::System)
        .count();

    let mut messages = Vec::new();
    let mut tokens_consumed: i64 = 0;
    let mut loaded_count: usize = 0;

    for msg in &budget_messages {
        if loaded_count >= config.max_messages {
            break;
        }

        match msg.role {
            Role::System => continue,
            Role::ToolResult if !config.include_tool_messages => continue,
            _ => {}
        }

        let role = match msg.role {
            Role::User | Role::ToolResult => "user",
            Role::Assistant => "assistant",
            Role::System => unreachable!(),
        };

        messages.push(PipelineMessage {
            role: role.to_owned(),
            content: msg.content.clone(),
            token_estimate: msg.token_estimate,
        });
        tokens_consumed += msg.token_estimate;
        loaded_count += 1;
    }

    let truncated = total_in_store > loaded_count;

    messages.push(PipelineMessage {
        role: "user".to_owned(),
        content: current_message.to_owned(),
        token_estimate: current_tokens,
    });

    let result = HistoryResult {
        messages_loaded: loaded_count,
        tokens_consumed,
        truncated,
    };
    debug!(
        messages_loaded = result.messages_loaded,
        tokens_consumed = result.tokens_consumed,
        truncated = result.truncated,
        "history loaded"
    );
    Ok((messages, result))
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use super::*;
    use aletheia_mneme::types::Role;

    fn setup_store() -> SessionStore {
        let store = SessionStore::open_in_memory().expect("open in-memory store");
        store
            .create_session("ses-1", "test-agent", "main", None, Some("test-model"))
            .expect("create session");
        store
    }

    fn append(store: &SessionStore, role: Role, content: &str, tokens: i64) {
        store
            .append_message("ses-1", role, content, None, None, tokens)
            .expect("append");
    }

    #[test]
    fn empty_history_returns_just_current_message() {
        let store = setup_store();
        let config = HistoryConfig::default();

        let (messages, result) =
            load_history(&store, "ses-1", 100_000, &config, "Hello").expect("load");

        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].role, "user");
        assert_eq!(messages[0].content, "Hello");
        assert_eq!(result.messages_loaded, 0);
        assert_eq!(result.tokens_consumed, 0);
        assert!(!result.truncated);
    }

    #[test]
    fn history_respects_budget() {
        let store = setup_store();
        // Each message ~200 tokens, budget will only fit some
        append(&store, Role::User, "old message 1", 200);
        append(&store, Role::Assistant, "reply 1", 200);
        append(&store, Role::User, "old message 2", 200);
        append(&store, Role::Assistant, "reply 2", 200);
        append(&store, Role::User, "old message 3", 200);
        append(&store, Role::Assistant, "reply 3", 200);

        let config = HistoryConfig {
            reserve_for_current: 100,
            ..HistoryConfig::default()
        };

        // Budget 600 - 100 reserve - current tokens = ~499 available
        // Should fit only ~2 messages (400 tokens)
        let (messages, result) =
            load_history(&store, "ses-1", 600, &config, "new message").expect("load");

        // Budget-limited: fewer messages loaded than total in store
        assert!(result.messages_loaded < 6);
        assert!(result.tokens_consumed <= 500);
        assert!(result.truncated);
        // Current message is always last
        assert_eq!(messages.last().unwrap().role, "user");
        assert_eq!(messages.last().unwrap().content, "new message");
    }

    #[test]
    fn history_filters_tool_messages() {
        let store = setup_store();
        append(&store, Role::User, "use a tool", 50);
        append(&store, Role::Assistant, "calling tool", 50);
        append(&store, Role::ToolResult, "tool output", 50);
        append(&store, Role::Assistant, "here's the result", 50);

        let config = HistoryConfig {
            include_tool_messages: false,
            ..HistoryConfig::default()
        };

        let (messages, result) =
            load_history(&store, "ses-1", 100_000, &config, "next").expect("load");

        let roles: Vec<&str> = messages.iter().map(|m| m.role.as_str()).collect();
        assert!(!roles.contains(&"tool_result"));
        // 3 messages loaded (user, assistant, assistant) — tool_result skipped
        assert_eq!(result.messages_loaded, 3);
    }

    #[test]
    fn history_skips_system_messages() {
        let store = setup_store();
        append(&store, Role::System, "system instruction", 100);
        append(&store, Role::User, "hello", 50);
        append(&store, Role::Assistant, "hi there", 50);

        let config = HistoryConfig::default();
        let (messages, result) =
            load_history(&store, "ses-1", 100_000, &config, "next").expect("load");

        let roles: Vec<&str> = messages.iter().map(|m| m.role.as_str()).collect();
        assert!(!roles.contains(&"system"));
        // Only user + assistant loaded from history (system skipped)
        assert_eq!(result.messages_loaded, 2);
    }

    #[test]
    fn history_preserves_order() {
        let store = setup_store();
        append(&store, Role::User, "first", 50);
        append(&store, Role::Assistant, "second", 50);
        append(&store, Role::User, "third", 50);
        append(&store, Role::Assistant, "fourth", 50);

        let config = HistoryConfig::default();
        let (messages, _) = load_history(&store, "ses-1", 100_000, &config, "fifth").expect("load");

        assert_eq!(messages[0].content, "first");
        assert_eq!(messages[1].content, "second");
        assert_eq!(messages[2].content, "third");
        assert_eq!(messages[3].content, "fourth");
        assert_eq!(messages[4].content, "fifth");
    }

    #[test]
    fn history_truncated_flag() {
        let store = setup_store();
        for i in 0..10 {
            append(&store, Role::User, &format!("msg {i}"), 100);
        }

        let config = HistoryConfig {
            reserve_for_current: 100,
            ..HistoryConfig::default()
        };

        // Tight budget: only fits a few of 10 messages
        let (_, result) = load_history(&store, "ses-1", 500, &config, "current").expect("load");

        assert!(result.truncated);
        assert!(result.messages_loaded < 10);

        // Large budget: fits all
        let (_, result_full) =
            load_history(&store, "ses-1", 100_000, &config, "current").expect("load");

        assert!(!result_full.truncated);
        assert_eq!(result_full.messages_loaded, 10);
    }

    #[test]
    fn tool_result_mapped_to_user_role() {
        let store = setup_store();
        append(&store, Role::ToolResult, "file contents", 100);

        let config = HistoryConfig::default();
        let (messages, result) =
            load_history(&store, "ses-1", 100_000, &config, "next").expect("load");

        assert_eq!(result.messages_loaded, 1);
        assert_eq!(messages[0].role, "user");
        assert_eq!(messages[0].content, "file contents");
    }

    #[test]
    fn token_estimates_preserved() {
        let store = setup_store();
        append(&store, Role::User, "q", 42);
        append(&store, Role::Assistant, "a", 99);

        let config = HistoryConfig::default();
        let (messages, result) =
            load_history(&store, "ses-1", 100_000, &config, "next").expect("load");

        assert_eq!(result.tokens_consumed, 141);
        assert_eq!(messages[0].token_estimate, 42);
        assert_eq!(messages[1].token_estimate, 99);
    }

    #[test]
    fn zero_budget_returns_current_only() {
        let store = setup_store();
        append(&store, Role::User, "old", 50);
        append(&store, Role::Assistant, "reply", 50);

        let config = HistoryConfig::default();
        let (messages, result) =
            load_history(&store, "ses-1", 0, &config, "current").expect("load");

        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].content, "current");
        assert_eq!(result.messages_loaded, 0);
        assert_eq!(result.tokens_consumed, 0);
        assert!(!result.truncated);
    }
}
