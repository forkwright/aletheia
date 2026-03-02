//! Finalize stage — persists the pipeline result to durable storage.
//!
//! After the execute stage produces a `TurnResult`, the finalize stage:
//! 1. Persists the user's message
//! 2. Persists tool call/result messages
//! 3. Persists the assistant's response
//! 4. Records token usage

use snafu::ResultExt;
use tracing::instrument;

use aletheia_mneme::store::SessionStore;
use aletheia_mneme::types::{Role, UsageRecord};

use crate::error;
use crate::pipeline::TurnResult;
use crate::session::SessionState;

/// Configuration for the finalize stage.
#[derive(Debug, Clone)]
pub struct FinalizeConfig {
    /// Whether to persist messages to the session store.
    pub persist_messages: bool,
    /// Whether to record usage metrics.
    pub record_usage: bool,
}

impl Default for FinalizeConfig {
    fn default() -> Self {
        Self {
            persist_messages: true,
            record_usage: true,
        }
    }
}

/// Result of the finalize stage.
#[derive(Debug, Clone)]
pub struct FinalizeResult {
    /// Number of messages persisted.
    pub messages_persisted: usize,
    /// Whether usage was recorded.
    pub usage_recorded: bool,
}

/// Persist turn results to the session store.
///
/// Errors from the store are propagated but callers should treat them as
/// non-fatal — the user already has their response.
#[instrument(skip_all, fields(session_id = %session.id))]
pub fn finalize(
    store: &SessionStore,
    session: &SessionState,
    input_content: &str,
    result: &TurnResult,
    config: &FinalizeConfig,
) -> error::Result<FinalizeResult> {
    let mut messages_persisted = 0usize;

    if config.persist_messages {
        // User message
        #[expect(clippy::cast_possible_wrap, reason = "message length fits in i64")]
        let input_token_estimate = input_content.len() as i64 / 4;
        store
            .append_message(&session.id, Role::User, input_content, None, None, input_token_estimate)
            .context(error::StoreSnafu)?;
        messages_persisted += 1;

        // Tool call/result pairs
        for tc in &result.tool_calls {
            let input_json = serde_json::to_string(&tc.input).unwrap_or_default();
            store
                .append_message(
                    &session.id,
                    Role::Assistant,
                    &input_json,
                    Some(&tc.id),
                    Some(&tc.name),
                    0,
                )
                .context(error::StoreSnafu)?;
            messages_persisted += 1;

            let tool_output = tc.result.as_deref().unwrap_or("");
            store
                .append_message(
                    &session.id,
                    Role::ToolResult,
                    tool_output,
                    Some(&tc.id),
                    Some(&tc.name),
                    0,
                )
                .context(error::StoreSnafu)?;
            messages_persisted += 1;
        }

        // Assistant response
        let output_tokens = i64::try_from(result.usage.output_tokens).unwrap_or(0);
        store
            .append_message(&session.id, Role::Assistant, &result.content, None, None, output_tokens)
            .context(error::StoreSnafu)?;
        messages_persisted += 1;
    }

    let mut usage_recorded = false;
    if config.record_usage {
        #[expect(clippy::cast_possible_wrap, reason = "token counts fit in i64")]
        let record = UsageRecord {
            session_id: session.id.clone(),
            turn_seq: session.turn as i64,
            input_tokens: result.usage.input_tokens as i64,
            output_tokens: result.usage.output_tokens as i64,
            cache_read_tokens: result.usage.cache_read_tokens as i64,
            cache_write_tokens: result.usage.cache_write_tokens as i64,
            model: Some(session.model.clone()),
        };
        store.record_usage(&record).context(error::StoreSnafu)?;
        usage_recorded = true;
    }

    Ok(FinalizeResult {
        messages_persisted,
        usage_recorded,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::NousConfig;
    use crate::pipeline::{ToolCall, TurnUsage};

    fn test_store_and_session() -> (SessionStore, SessionState) {
        let store = SessionStore::open_in_memory().expect("in-memory store");
        store
            .create_session("ses-1", "test-nous", "main", None, Some("test-model"))
            .expect("create session");
        let config = NousConfig {
            id: "test-nous".to_owned(),
            model: "test-model".to_owned(),
            ..NousConfig::default()
        };
        let session = SessionState::new("ses-1".to_owned(), "main".to_owned(), &config);
        (store, session)
    }

    fn simple_result() -> TurnResult {
        TurnResult {
            content: "Hello!".to_owned(),
            tool_calls: vec![],
            usage: TurnUsage {
                input_tokens: 100,
                output_tokens: 50,
                ..TurnUsage::default()
            },
            stop_reason: "end_turn".to_owned(),
            signals: vec![],
        }
    }

    fn result_with_tools() -> TurnResult {
        TurnResult {
            content: "Done.".to_owned(),
            tool_calls: vec![ToolCall {
                id: "tc-1".to_owned(),
                name: "read_file".to_owned(),
                input: serde_json::json!({"path": "/tmp/test.txt"}),
                result: Some("file contents here".to_owned()),
                is_error: false,
                duration_ms: 42,
            }],
            usage: TurnUsage {
                input_tokens: 200,
                output_tokens: 80,
                cache_read_tokens: 50,
                cache_write_tokens: 10,
                llm_calls: 2,
            },
            stop_reason: "end_turn".to_owned(),
            signals: vec![],
        }
    }

    #[test]
    fn finalize_persists_user_and_assistant_messages() {
        let (store, session) = test_store_and_session();
        let result = simple_result();
        let config = FinalizeConfig::default();

        finalize(&store, &session, "Hi there", &result, &config).expect("finalize");

        let history = store.get_history("ses-1", None).expect("history");
        assert_eq!(history.len(), 2);
        assert_eq!(history[0].role, Role::User);
        assert_eq!(history[0].content, "Hi there");
        assert_eq!(history[1].role, Role::Assistant);
        assert_eq!(history[1].content, "Hello!");
    }

    #[test]
    fn finalize_persists_tool_call_messages() {
        let (store, session) = test_store_and_session();
        let result = result_with_tools();
        let config = FinalizeConfig::default();

        finalize(&store, &session, "Read the file", &result, &config).expect("finalize");

        let history = store.get_history("ses-1", None).expect("history");
        // user + tool_call(assistant) + tool_result + assistant = 4
        assert_eq!(history.len(), 4);
        assert_eq!(history[0].role, Role::User);
        assert_eq!(history[1].role, Role::Assistant);
        assert!(history[1].tool_call_id.is_some());
        assert_eq!(history[1].tool_name.as_deref(), Some("read_file"));
        assert_eq!(history[2].role, Role::ToolResult);
        assert_eq!(history[2].content, "file contents here");
        assert_eq!(history[3].role, Role::Assistant);
        assert_eq!(history[3].content, "Done.");
    }

    #[test]
    fn finalize_records_usage() {
        let (store, session) = test_store_and_session();
        let result = simple_result();
        let config = FinalizeConfig::default();

        let fr = finalize(&store, &session, "Hi", &result, &config).expect("finalize");
        assert!(fr.usage_recorded);
    }

    #[test]
    fn finalize_disabled_skips_persistence() {
        let (store, session) = test_store_and_session();
        let result = simple_result();
        let config = FinalizeConfig {
            persist_messages: false,
            record_usage: false,
        };

        let fr = finalize(&store, &session, "Hi", &result, &config).expect("finalize");
        assert_eq!(fr.messages_persisted, 0);

        let history = store.get_history("ses-1", None).expect("history");
        assert!(history.is_empty());
    }

    #[test]
    fn finalize_disabled_skips_usage() {
        let (store, session) = test_store_and_session();
        let result = simple_result();
        let config = FinalizeConfig {
            persist_messages: true,
            record_usage: false,
        };

        let fr = finalize(&store, &session, "Hi", &result, &config).expect("finalize");
        assert!(!fr.usage_recorded);
        // Messages should still be persisted
        assert_eq!(fr.messages_persisted, 2);
    }

    #[test]
    fn finalize_returns_correct_counts() {
        let (store, session) = test_store_and_session();

        // No tool calls: user + assistant = 2
        let result = simple_result();
        let config = FinalizeConfig::default();
        let fr = finalize(&store, &session, "Hi", &result, &config).expect("finalize");
        assert_eq!(fr.messages_persisted, 2);

        // New session for tool calls test
        store
            .create_session("ses-2", "test-nous", "main-2", None, Some("test-model"))
            .expect("create session");
        let mut session2 = session.clone();
        session2.id = "ses-2".to_owned();

        // One tool call: user + tool_call + tool_result + assistant = 4
        let result = result_with_tools();
        let fr = finalize(&store, &session2, "Read it", &result, &config).expect("finalize");
        assert_eq!(fr.messages_persisted, 4);
    }
}
