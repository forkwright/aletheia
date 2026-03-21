//! Finalize stage: persists the pipeline result to durable storage.
//!
//! After the execute stage produces a `TurnResult`, the finalize stage:
//! 1. Persists the user's message
//! 2. Persists tool call/result messages
//! 3. Persists the assistant's response
//! 4. Records token usage

use snafu::ResultExt;
use tracing::{debug, instrument, warn};
use ulid::Ulid;

use aletheia_mneme::store::SessionStore;
use aletheia_mneme::types::{Role, UsageRecord};

use crate::error;
use crate::pipeline::TurnResult;
use crate::session::SessionState;

/// Convert a ULID to a globally unique `i64` for use as `turn_seq` in the
/// usage table.
///
/// Uses the upper 63 bits of the 128-bit ULID value. A ULID's upper 48 bits
/// are a millisecond timestamp, so the result is monotonically increasing
/// within each millisecond and practically unique across restarts.
fn turn_seq_from_ulid(ulid: &Ulid) -> i64 {
    // NOTE: ULID is 128-bit: shift right 65 bits to keep upper 63 bits (47-bit timestamp + 16-bit randomness prefix); mask ensures sign bit is zero so cast to i64 never wraps
    let raw = u128::from(*ulid);
    #[expect(
        clippy::as_conversions,
        reason = "u128→i64: mask ensures sign bit is zero so cast never wraps"
    )]
    {
        ((raw >> 65) & 0x7FFF_FFFF_FFFF_FFFF) as i64 // kanon:ignore RUST/as-cast
    }
}

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
/// non-fatal: the user already has their response.
///
/// # Session guarantee
///
/// The nous actor creates sessions in memory (not in `SQLite`). Before
/// appending messages we ensure the session record exists in the store,
/// avoiding a FOREIGN KEY constraint violation on the `messages` table.
// NOTE(#940): 120 lines: sequential persistence pipeline: persist assistant message,
// store tool results, update session, emit events. One cohesive commit sequence.
#[expect(
    clippy::too_many_lines,
    reason = "sequential persist pipeline with dedup guard adds a few lines over the limit"
)]
#[instrument(skip_all, fields(session_id = %session.id))]
pub(crate) fn finalize(
    store: &SessionStore,
    session: &SessionState,
    input_content: &str,
    result: &TurnResult,
    config: &FinalizeConfig,
) -> error::Result<FinalizeResult> {
    // INVARIANT: dedup guard: skip if this turn was already finalized (usage record exists)
    //
    // WHY: turn_id (fresh ULID) is the dedup key rather than session_id: see issue #1036
    let turn_seq = turn_seq_from_ulid(&session.turn_id);
    if store
        .usage_exists_for_turn(&session.id, turn_seq)
        .context(error::StoreSnafu)?
    {
        warn!(
            turn = session.turn,
            turn_id = %session.turn_id,
            "finalize called twice for same turn, skipping duplicate"
        );
        return Ok(FinalizeResult {
            messages_persisted: 0,
            usage_recorded: false,
        });
    }

    let mut messages_persisted = 0usize;

    if config.persist_messages {
        // WHY: ensure session row exists before child messages: FK constraint on messages table would otherwise fail
        store
            .find_or_create_session(
                &session.id,
                &session.nous_id,
                &session.session_key,
                Some(&session.model),
                None,
            )
            .context(error::StoreSnafu)?;
        #[expect(
            clippy::cast_possible_wrap,
            clippy::as_conversions,
            reason = "usize→i64: message length fits in i64"
        )]
        let input_token_estimate = (input_content.len() as i64 + 3) / 4; // kanon:ignore RUST/as-cast
        store
            .append_message(
                &session.id,
                Role::User,
                input_content,
                None,
                None,
                input_token_estimate,
            )
            .context(error::StoreSnafu)?;
        messages_persisted += 1;

        for tc in &result.tool_calls {
            let input_json = serde_json::to_string(&tc.input).unwrap_or_else(|e| {
                warn!(error = %e, tool = %tc.name, "failed to serialize tool call input");
                String::new()
            });
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

        let output_tokens = i64::try_from(result.usage.output_tokens).unwrap_or(0);
        store
            .append_message(
                &session.id,
                Role::Assistant,
                &result.content,
                None,
                None,
                output_tokens,
            )
            .context(error::StoreSnafu)?;
        messages_persisted += 1;
    }

    let mut usage_recorded = false;
    if config.record_usage {
        #[expect(
            clippy::cast_possible_wrap,
            clippy::as_conversions,
            reason = "u64→i64: token counts fit in i64"
        )]
        let record = UsageRecord {
            session_id: session.id.clone(),
            turn_seq,
            input_tokens: result.usage.input_tokens as i64, // kanon:ignore RUST/as-cast
            output_tokens: result.usage.output_tokens as i64, // kanon:ignore RUST/as-cast
            cache_read_tokens: result.usage.cache_read_tokens as i64, // kanon:ignore RUST/as-cast
            cache_write_tokens: result.usage.cache_write_tokens as i64, // kanon:ignore RUST/as-cast
            model: Some(session.model.clone()),
        };
        store.record_usage(&record).context(error::StoreSnafu)?;
        usage_recorded = true;
    }

    debug!(messages_persisted, usage_recorded, "finalize complete");
    Ok(FinalizeResult {
        messages_persisted,
        usage_recorded,
    })
}

#[expect(
    clippy::indexing_slicing,
    reason = "test: vec indices are valid after asserting len"
)]
#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use super::*;
    use crate::config::NousConfig;
    use crate::pipeline::{ToolCall, TurnUsage};

    fn make_store_and_session() -> (SessionStore, SessionState) {
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
        let (store, session) = make_store_and_session();
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
        let (store, session) = make_store_and_session();
        let result = result_with_tools();
        let config = FinalizeConfig::default();

        finalize(&store, &session, "Read the file", &result, &config).expect("finalize");

        let history = store.get_history("ses-1", None).expect("history");
        // NOTE: user + tool_call(assistant) + tool_result + assistant = 4
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
        let (store, session) = make_store_and_session();
        let result = simple_result();
        let config = FinalizeConfig::default();

        let fr = finalize(&store, &session, "Hi", &result, &config).expect("finalize");
        assert!(fr.usage_recorded);
    }

    #[test]
    fn finalize_disabled_skips_persistence() {
        let (store, session) = make_store_and_session();
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

    /// Regression test for #747: finalize must succeed even when the session
    /// record does not yet exist in `SQLite`. The nous actor creates sessions in
    /// memory, so the first call to finalize is responsible for ensuring the
    /// row exists before inserting child messages (FOREIGN KEY constraint).
    #[test]
    fn finalize_creates_session_if_missing() {
        let store = SessionStore::open_in_memory().expect("in-memory store");
        // WHY: Do NOT call store.create_session: the actor wouldn't have done so.
        let config_nous = NousConfig {
            id: "test-nous".to_owned(),
            model: "test-model".to_owned(),
            ..NousConfig::default()
        };
        let session = SessionState::new("ses-orphan".to_owned(), "main".to_owned(), &config_nous);
        let result = simple_result();
        let config = FinalizeConfig::default();

        // NOTE: This would previously fail with FOREIGN KEY constraint error
        finalize(&store, &session, "Hi from orphan", &result, &config).expect("finalize");

        let history = store.get_history("ses-orphan", None).expect("history");
        assert_eq!(history.len(), 2);
        assert_eq!(history[0].role, Role::User);
        assert_eq!(history[1].role, Role::Assistant);
    }

    #[test]
    fn finalize_disabled_skips_usage() {
        let (store, session) = make_store_and_session();
        let result = simple_result();
        let config = FinalizeConfig {
            persist_messages: true,
            record_usage: false,
        };

        let fr = finalize(&store, &session, "Hi", &result, &config).expect("finalize");
        assert!(!fr.usage_recorded);
        assert_eq!(fr.messages_persisted, 2);
    }

    #[test]
    fn finalize_dedup_guard_skips_duplicate() {
        let (store, session) = make_store_and_session();
        let result = simple_result();
        let config = FinalizeConfig::default();

        let fr = finalize(&store, &session, "Hi", &result, &config).expect("finalize");
        assert_eq!(fr.messages_persisted, 2);
        assert!(fr.usage_recorded);

        let fr2 = finalize(&store, &session, "Hi again", &result, &config).expect("finalize");
        assert_eq!(fr2.messages_persisted, 0);
        assert!(!fr2.usage_recorded);

        let history = store.get_history("ses-1", None).expect("history");
        assert_eq!(history.len(), 2);
    }

    #[test]
    fn finalize_returns_correct_counts() {
        let (store, session) = make_store_and_session();

        // NOTE: No tool calls: user + assistant = 2
        let result = simple_result();
        let config = FinalizeConfig::default();
        let fr = finalize(&store, &session, "Hi", &result, &config).expect("finalize");
        assert_eq!(fr.messages_persisted, 2);

        store
            .create_session("ses-2", "test-nous", "main-2", None, Some("test-model"))
            .expect("create session");
        let mut session2 = session.clone();
        session2.id = "ses-2".to_owned();

        // NOTE: One tool call: user + tool_call + tool_result + assistant = 4
        let result = result_with_tools();
        let fr = finalize(&store, &session2, "Read it", &result, &config).expect("finalize");
        assert_eq!(fr.messages_persisted, 4);
    }

    /// Regression test for #758/#916/#923: session ID divergence.
    ///
    /// Simulates the scenario where pylon creates a session with DB ID "A",
    /// but the actor's `SessionState` holds a different ID "B". Before the fix,
    /// `find_or_create_session` would find the existing session by
    /// (`nous_id`, `session_key`) and return ID "A", but `append_message` would
    /// use the actor's ID "B": causing an FK constraint violation.
    ///
    /// After the fix, the actor adopts the DB session ID so both match.
    #[test]
    fn finalize_with_matching_session_id_no_fk_violation() {
        let store = SessionStore::open_in_memory().expect("in-memory store");
        let db_session_id = "db-ses-from-pylon";

        store
            .create_session(db_session_id, "test-nous", "main", None, Some("test-model"))
            .expect("create session");

        // WHY: Actor's SessionState must use the SAME ID as the database.
        // Before the fix, the actor would generate a different ULID here.
        let config = NousConfig {
            id: "test-nous".to_owned(),
            model: "test-model".to_owned(),
            ..NousConfig::default()
        };
        let session = SessionState::new(db_session_id.to_owned(), "main".to_owned(), &config);

        let result = simple_result();
        let finalize_config = FinalizeConfig::default();

        // NOTE: This must succeed: no FK violation because IDs match.
        let fr = finalize(&store, &session, "Hello", &result, &finalize_config)
            .expect("finalize should not fail with matching session IDs");
        assert_eq!(fr.messages_persisted, 2);
        assert!(fr.usage_recorded);

        let history = store.get_history(db_session_id, None).expect("history");
        assert_eq!(history.len(), 2);
        assert_eq!(history[0].role, Role::User);
        assert_eq!(history[0].content, "Hello");
        assert_eq!(history[1].role, Role::Assistant);
        assert_eq!(history[1].content, "Hello!");
    }

    /// Regression test for #758: verify that when the actor uses a divergent
    /// session ID, `find_or_create_session` finds by (`nous_id`, `session_key`)
    /// but `append_message` would use the wrong ID. This test documents the
    /// failure mode that the session-id-adoption fix prevents.
    #[test]
    fn divergent_session_id_causes_fk_violation() {
        let store = SessionStore::open_in_memory().expect("in-memory store");

        store
            .create_session("pylon-id", "test-nous", "main", None, Some("test-model"))
            .expect("create session");

        // NOTE: Actor would have generated a DIFFERENT ID (before the fix)
        let config = NousConfig {
            id: "test-nous".to_owned(),
            model: "test-model".to_owned(),
            ..NousConfig::default()
        };
        let divergent_session =
            SessionState::new("actor-generated-id".to_owned(), "main".to_owned(), &config);

        let result = simple_result();
        let finalize_config = FinalizeConfig::default();

        // WHY: find_or_create_session finds "pylon-id" by (nous_id, session_key).
        // But append_message uses "actor-generated-id" which has no DB row.
        // finalize internally calls find_or_create_session which ensures a
        // row exists matching the session_key, then tries append_message with
        // the actor's ID. The find_or_create returns the existing "pylon-id"
        // row, but append_message uses "actor-generated-id".
        //
        // The finalize function calls find_or_create_session with the actor's
        // session.id as the `id` param. Since an active session already exists
        // for (nous_id, session_key), it returns that existing session: but
        // does NOT create a new row with the actor's ID. Subsequent
        // append_message calls use the actor's ID, which has no row → FK error.
        let result = finalize(
            &store,
            &divergent_session,
            "Hello",
            &result,
            &finalize_config,
        );

        // NOTE: This should fail with a database error due to FK constraint
        assert!(
            result.is_err(),
            "divergent session ID should cause FK violation"
        );
    }
}
