//! History stage: loads conversation context from the session store.
//!
//! Retrieves the most recent messages that fit within the remaining token
//! budget, converts them to pipeline messages, and appends the current
//! user message at the end.

use snafu::ResultExt;
use tracing::debug;

use mneme::store::SessionStore;
use mneme::types::Role;

use crate::config::TurnHistoryPolicy;
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
// kanon:ignore TOPOLOGY/shallow-struct — result bag passed across pipeline stage boundary; no in-file behavior by design
#[derive(Debug, Clone)]
pub struct HistoryResult {
    /// Number of messages loaded from store.
    pub messages_loaded: usize,
    /// Total tokens consumed by loaded history.
    pub tokens_consumed: i64,
    /// Whether the `max_messages` count limit dropped eligible messages from
    /// the token-budget window. Sessions limited by the token budget rather
    /// than the count cap report `false` here; inspect `tokens_consumed`
    /// relative to the budget for token-budget observability.
    pub truncated: bool,
    /// The effective history policy that produced this result.
    ///
    /// WHY: run records and diagnostics need to explain why messages were
    /// included or dropped. The policy is cloned here so it survives even
    /// if the upstream config is mutated later.
    pub policy: TurnHistoryPolicy,
}

/// Load conversation history and append the current user message.
///
/// Delegates token-budget enforcement to the store's `get_history_with_budget`
/// API so only the most recent messages fitting within `available` tokens are
/// fetched from storage — O(budget-tokens) rather than O(total-session-messages).
/// A `max_messages` count cap is then applied in memory on the budgeted result.
///
/// System-role messages are always skipped (they're in the system prompt).
/// Tool-result messages are included or skipped based on `config.include_tool_messages`.
#[expect(clippy::cast_possible_wrap, reason = "message length fits in i64")]
pub(crate) fn load_history(
    store: &SessionStore,
    session_id: &str,
    budget: i64,
    config: &HistoryConfig,
    current_message: &str,
) -> error::Result<(Vec<PipelineMessage>, HistoryResult)> {
    #[expect(
        clippy::as_conversions,
        reason = "usize→i64: message length fits in i64"
    )]
    let current_tokens = (current_message.len() as i64 + 3) / 4; // kanon:ignore RUST/as-cast
    let available = budget
        .saturating_sub(config.reserve_for_current)
        .saturating_sub(current_tokens);

    if available <= 0 {
        let messages = vec![PipelineMessage::text(
            "user",
            current_message,
            current_tokens,
        )];
        return Ok((
            messages,
            HistoryResult {
                messages_loaded: 0,
                tokens_consumed: 0,
                truncated: false,
                policy: TurnHistoryPolicy::default(),
            },
        ));
    }

    // Delegate token-budget enforcement to the store so only the newest
    // messages fitting within `available` tokens are read from storage.
    // This bounds the storage scan to O(budget-tokens) regardless of how
    // long the session history is.
    let raw = store
        .get_history_with_budget(session_id, available)
        .context(error::StoreSnafu)?;
    let tool_audit_by_call_id = tool_audit_by_call_id(store, session_id, &raw)?;

    // Count eligible messages within the budgeted window so the `truncated`
    // flag correctly reflects whether the count cap was the limiting factor.
    let total_eligible_in_window = raw
        .iter()
        .filter(|m| m.role != Role::System && should_include_message(m, config))
        .count();

    let mut collected: Vec<PipelineMessage> = Vec::new();
    let mut tokens_consumed: i64 = 0;
    let mut loaded_count: usize = 0;

    for msg in raw.iter().rev() {
        if loaded_count >= config.max_messages {
            break;
        }

        match msg.role {
            Role::System => continue,
            _ if !should_include_message(msg, config) => continue,
            _ => {} // kanon:ignore RUST/empty-match-arm — all other roles proceed to message inclusion below
        }

        let mut pipeline_message = pipeline_message_from_history(msg);
        apply_tool_audit(&mut pipeline_message, msg, &tool_audit_by_call_id);
        collected.push(pipeline_message);
        tokens_consumed += msg.token_estimate;
        loaded_count += 1;
    }

    // WHY: restore chronological order: collected newest-first
    collected.reverse();

    // Truncated reflects only the count-cap limit within the budget window.
    // Token-budget truncation (older messages beyond the window) is observable
    // via `tokens_consumed` relative to `budget_tokens_available` in the log.
    let truncated = total_eligible_in_window > loaded_count;

    collected.push(PipelineMessage::text(
        "user",
        current_message,
        current_tokens,
    ));

    let result = HistoryResult {
        messages_loaded: loaded_count,
        tokens_consumed,
        truncated,
        policy: TurnHistoryPolicy::default(),
    };
    debug!(
        messages_loaded = result.messages_loaded,
        tokens_consumed = result.tokens_consumed,
        budget_tokens_available = available,
        truncated = result.truncated,
        "history loaded"
    );
    Ok((collected, result))
}

fn should_include_message(msg: &mneme::types::Message, config: &HistoryConfig) -> bool {
    config.include_tool_messages || !is_tool_history_message(msg)
}

fn is_tool_history_message(msg: &mneme::types::Message) -> bool {
    msg.role == Role::ToolResult || (msg.role == Role::Assistant && msg.tool_call_id.is_some())
}

fn pipeline_message_from_history(msg: &mneme::types::Message) -> PipelineMessage {
    let mut message = match msg.role {
        Role::Assistant => match (&msg.tool_call_id, &msg.tool_name) {
            (Some(tool_call_id), Some(tool_name)) => PipelineMessage::tool_use(
                msg.content.clone(),
                msg.token_estimate,
                tool_call_id.clone(),
                tool_name.clone(),
            ),
            _ => PipelineMessage::text("assistant", msg.content.clone(), msg.token_estimate),
        },
        Role::ToolResult => match (&msg.tool_call_id, &msg.tool_name) {
            (Some(tool_call_id), Some(tool_name)) => PipelineMessage::tool_result(
                msg.content.clone(),
                msg.token_estimate,
                tool_call_id.clone(),
                tool_name.clone(),
            ),
            _ => PipelineMessage::text("tool_result", msg.content.clone(), msg.token_estimate),
        },
        // WHY: non_exhaustive fallback -- unknown roles mapped to user.
        _ => PipelineMessage::text("user", msg.content.clone(), msg.token_estimate),
    };

    // WHY(#3781): Mark distillation summaries as cache breakpoints so
    // subsequent turns can reuse the cached prefix. Detect by content marker
    // that was added by apply_compaction().
    message.cache_breakpoint = msg
        .content
        .starts_with("[Conversation summary FROM compaction]");
    message
}

fn apply_tool_audit(
    message: &mut PipelineMessage,
    history_message: &mneme::types::Message,
    audit_by_call_id: &std::collections::HashMap<String, mneme::types::ToolAuditRecord>,
) {
    if let Some(tool_call_id) = &history_message.tool_call_id
        && let Some(audit) = audit_by_call_id.get(tool_call_id)
    {
        message.tool_is_error = Some(audit.is_error);
        message.tool_duration_ms = Some(audit.duration_ms);
        message.tool_approval.clone_from(&audit.approval);
        message.tool_receipt.clone_from(&audit.receipt);
    }
}

fn tool_audit_by_call_id(
    store: &SessionStore,
    session_id: &str,
    messages: &[mneme::types::Message],
) -> error::Result<std::collections::HashMap<String, mneme::types::ToolAuditRecord>> {
    if !messages.iter().any(|msg| msg.tool_call_id.is_some()) {
        return Ok(std::collections::HashMap::new());
    }

    let records = store
        .tool_audit_records_for_session(session_id)
        .context(error::StoreSnafu)?;
    Ok(records
        .into_iter()
        .map(|record| (record.tool_call_id.clone(), record))
        .collect())
}

#[expect(
    clippy::indexing_slicing,
    reason = "test: vec indices are valid after asserting len"
)]
#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions may panic on failure")]
#[expect(clippy::expect_used, reason = "test assertions may panic on failure")]
mod tests {
    use mneme::types::Role;

    use super::*;

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

        let (messages, result) =
            load_history(&store, "ses-1", 600, &config, "new message").expect("load");

        // Budget of 500 available — verify bounded load, not total history.
        assert!(
            result.messages_loaded < 6,
            "loaded less than total 6 messages"
        );
        assert!(
            result.tokens_consumed <= 500,
            "tokens within available budget"
        );
        assert_eq!(messages.last().unwrap().role, "user");
        assert_eq!(messages.last().unwrap().content, "new message");
    }

    #[test]
    fn history_filters_tool_messages() {
        let store = setup_store();
        append(&store, Role::User, "use a tool", 50);
        store
            .append_message(
                "ses-1",
                Role::Assistant,
                r#"{"path":"README.md"}"#,
                Some("tc-1"),
                Some("read_file"),
                50,
            )
            .expect("append tool call");
        store
            .append_message(
                "ses-1",
                Role::ToolResult,
                "tool output",
                Some("tc-1"),
                Some("read_file"),
                50,
            )
            .expect("append tool result");
        append(&store, Role::Assistant, "here's the result", 50);

        let config = HistoryConfig {
            include_tool_messages: false,
            ..HistoryConfig::default()
        };

        let (messages, result) =
            load_history(&store, "ses-1", 100_000, &config, "next").expect("load");

        let roles: Vec<&str> = messages.iter().map(|m| m.role.as_str()).collect();
        assert!(!roles.contains(&"tool_result"));
        assert!(
            !messages
                .iter()
                .any(|message| message.tool_call_id.as_deref() == Some("tc-1")),
            "tool-use rows should be filtered with their tool-result rows"
        );
        assert_eq!(result.messages_loaded, 2);
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
    fn history_truncated_by_count_cap() {
        let store = setup_store();
        for i in 0..10 {
            append(&store, Role::User, &format!("msg {i}"), 100);
        }

        let config = HistoryConfig {
            max_messages: 5,
            reserve_for_current: 100,
            ..HistoryConfig::default()
        };

        // Budget is ample; truncation must come from max_messages cap.
        let (_, result) = load_history(&store, "ses-1", 100_000, &config, "current").expect("load");

        assert!(result.truncated, "count cap should trigger truncated flag");
        assert_eq!(result.messages_loaded, 5);

        // With a larger cap all messages load; flag should be clear.
        let config_full = HistoryConfig {
            max_messages: 50,
            reserve_for_current: 100,
            ..HistoryConfig::default()
        };
        let (_, result_full) =
            load_history(&store, "ses-1", 100_000, &config_full, "current").expect("load");

        assert!(!result_full.truncated);
        assert_eq!(result_full.messages_loaded, 10);
    }

    /// Verifies that the budgeted store API bounds the message load for long
    /// sessions: loaded count and token consumption must stay within the
    /// configured limits regardless of total session history size.
    #[test]
    fn long_history_bounded_load() {
        let store = setup_store();
        for i in 0..200 {
            let role = if i % 2 == 0 {
                Role::User
            } else {
                Role::Assistant
            };
            append(&store, role, &format!("message {i}"), 50);
        }

        let config = HistoryConfig {
            max_messages: 20,
            reserve_for_current: 200,
            ..HistoryConfig::default()
        };

        let (messages, result) =
            load_history(&store, "ses-1", 5_000, &config, "current").expect("load");

        // available = 5000 - 200 - ~2 = ~4798; 20 messages × 50 = 1000 tokens loaded.
        assert!(
            result.messages_loaded <= 20,
            "loaded {} messages, max_messages is 20",
            result.messages_loaded
        );
        assert!(
            result.tokens_consumed <= 4_800,
            "consumed {} tokens, available is ~4800",
            result.tokens_consumed
        );
        // Current message is always the last entry.
        assert_eq!(messages.last().unwrap().content, "current");
    }

    #[test]
    fn tool_result_keeps_distinct_pipeline_role() {
        let store = setup_store();
        append(&store, Role::ToolResult, "file contents", 100);

        let config = HistoryConfig::default();
        let (messages, result) =
            load_history(&store, "ses-1", 100_000, &config, "next").expect("load");

        assert_eq!(result.messages_loaded, 1);
        assert_eq!(messages[0].role, "tool_result");
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

    /// Verifies that a non-default turn-history policy deterministically changes
    /// the loaded context: a low message cap drops older messages and disabling
    /// tool-result messages filters them out.
    #[test]
    fn history_policy_controls_loaded_context() {
        let store = setup_store();
        for i in 0..5 {
            append(&store, Role::User, &format!("user {i}"), 10);
            append(&store, Role::Assistant, &format!("assistant {i}"), 10);
        }
        append(&store, Role::ToolResult, "tool output", 10);

        let policy = TurnHistoryPolicy {
            max_messages: 2,
            reserve_for_current: 100,
            include_tool_messages: false,
        };
        let config = HistoryConfig {
            max_messages: policy.max_messages,
            reserve_for_current: policy.reserve_for_current,
            include_tool_messages: policy.include_tool_messages,
        };

        let (messages, result) =
            load_history(&store, "ses-1", 10_000, &config, "current").expect("load");

        assert_eq!(
            result.messages_loaded, 2,
            "policy max_messages should cap load"
        );
        assert!(result.truncated, "count cap should set truncated flag");
        assert!(
            !messages.iter().any(|m| m.content == "tool output"),
            "policy disabled tool-result messages"
        );
    }
}
