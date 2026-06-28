//! Finalize stage: persists the pipeline result to durable storage.
//!
//! After the execute stage produces a `TurnResult`, the finalize stage:
//! 1. Persists the user's message
//! 2. Persists tool call/result messages
//! 3. Persists the assistant's response
//! 4. Records token usage
//!
//! WHY: finalize writes the turn payload and terminal lifecycle marker through
//! graphe's batched `finalize_turn` transaction. A retry can therefore observe
//! either a pending marker with no committed turn payload or a completed marker
//! with the whole turn, never a durable message prefix that gets appended again.

use koina::ulid::Ulid;
use snafu::ResultExt;
use tracing::{debug, instrument, warn};

use mneme::store::{FinalizeMessage, FinalizeNote, FinalizeTurnRequest, SessionStore};
use mneme::types::{Role, UsageRecord};

use crate::error;
use crate::pipeline::TurnResult;
use crate::session::SessionState;
use crate::turn_record::{TurnAttemptRecord, TurnAttemptStatus};

/// Content marker prefix for assistant turns that ended for an explicitly
/// partial reason. Stored inline because `graphe::Message` has no metadata
/// field for `stop_reason`; this follows existing history conventions
/// (`[tool:...]`, `[System: ...]`, `[Conversation summary FROM compaction]`).
const PARTIAL_MARKER_PREFIX: &str = "[partial: ";

/// Convert a ULID to a globally unique `i64` for use as `turn_seq` in the
/// usage table.
///
/// Uses the upper 63 bits of the 128-bit ULID value. A ULID's upper 48 bits
/// are a millisecond timestamp, so the result is monotonically increasing
/// within each millisecond and practically unique across restarts.
fn turn_seq_from_ulid(ulid: &Ulid) -> i64 {
    // NOTE: ULID is 128-bit: shift right 65 bits to keep upper 63 bits (47-bit timestamp + 16-bit randomness prefix); mask ensures sign bit is zero so cast to i64 never wraps
    let raw = ulid.as_u128();
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
pub(crate) struct FinalizeResult {
    messages_persisted: usize,
    usage_recorded: bool,
}

impl FinalizeResult {
    fn new(messages_persisted: usize, usage_recorded: bool) -> Self {
        Self {
            messages_persisted,
            usage_recorded,
        }
    }

    pub(crate) fn messages_persisted(&self) -> usize {
        self.messages_persisted
    }

    pub(crate) fn usage_recorded(&self) -> bool {
        self.usage_recorded
    }
}

/// Returns true for stop reasons that represent a partial terminal outcome
/// rather than a clean assistant completion.
///
/// `client_disconnect` and `max_tool_iterations` can carry partial content in
/// an otherwise successful `TurnResult`, so durable history must not render
/// them as clean assistant completions.
fn is_partial_stop_reason(stop_reason: &str) -> bool {
    matches!(stop_reason, "client_disconnect" | "max_tool_iterations")
}

/// Format the assistant message content for persistence.
///
/// Partial terminal outcomes are prefixed with a visible marker so they are
/// not indistinguishable from clean `end_turn` assistant messages when later
/// loaded as history. This is a history-convention marker (no schema change);
/// callers that need the raw content can strip the prefix if desired.
fn format_assistant_content(content: &str, stop_reason: &str) -> String {
    if is_partial_stop_reason(stop_reason) {
        if content.is_empty() {
            format!("{PARTIAL_MARKER_PREFIX}{stop_reason}]")
        } else {
            format!("{PARTIAL_MARKER_PREFIX}{stop_reason}] {content}")
        }
    } else {
        content.to_owned()
    }
}

struct PendingFinalizeMessage<'a> {
    role: Role,
    content: String,
    tool_call_id: Option<&'a str>,
    tool_name: Option<&'a str>,
    token_estimate: i64,
}

fn token_count_to_i64(tokens: u64) -> i64 {
    i64::try_from(tokens).unwrap_or(i64::MAX)
}

fn input_token_estimate(content: &str) -> i64 {
    let len = i64::try_from(content.len()).unwrap_or(i64::MAX - 3);
    len.saturating_add(3) / 4
}

fn build_finalize_messages<'a>(
    input_content: &str,
    result: &'a TurnResult,
    persist_messages: bool,
) -> Vec<PendingFinalizeMessage<'a>> {
    if !persist_messages {
        return Vec::new();
    }

    let mut messages = Vec::with_capacity(2 + result.tool_calls.len().saturating_mul(2));
    messages.push(PendingFinalizeMessage {
        role: Role::User,
        content: input_content.to_owned(),
        tool_call_id: None,
        tool_name: None,
        token_estimate: input_token_estimate(input_content),
    });

    for tc in &result.tool_calls {
        let input_json = serde_json::to_string(&tc.input).unwrap_or_else(|e| {
            warn!(error = %e, tool = %tc.name, "failed to serialize tool call input");
            String::new()
        });
        messages.push(PendingFinalizeMessage {
            role: Role::Assistant,
            content: input_json,
            tool_call_id: Some(&tc.id),
            tool_name: Some(&tc.name),
            token_estimate: 0,
        });

        let tool_output = tc.result.as_deref().unwrap_or("[missing tool result]");
        let compact_tagged_output = crate::compact::micro::format_tool_result(
            &tc.name,
            jiff::Timestamp::now(),
            tool_output,
        );
        messages.push(PendingFinalizeMessage {
            role: Role::ToolResult,
            content: compact_tagged_output,
            tool_call_id: Some(&tc.id),
            tool_name: Some(&tc.name),
            token_estimate: 0,
        });
    }

    messages.push(PendingFinalizeMessage {
        role: Role::Assistant,
        content: format_assistant_content(&result.content, &result.stop_reason),
        tool_call_id: None,
        tool_name: None,
        token_estimate: token_count_to_i64(result.usage.output_tokens),
    });
    messages
}

fn usage_record(session: &SessionState, result: &TurnResult, turn_seq: i64) -> UsageRecord {
    UsageRecord {
        session_id: session.id.clone(),
        turn_seq,
        input_tokens: token_count_to_i64(result.usage.input_tokens),
        output_tokens: token_count_to_i64(result.usage.output_tokens),
        cache_read_tokens: token_count_to_i64(result.usage.cache_read_tokens),
        cache_write_tokens: token_count_to_i64(result.usage.cache_write_tokens),
        model: Some(result.model_used.clone()),
    }
}

fn turn_attempt_record(
    session: &SessionState,
    result: &TurnResult,
    status: TurnAttemptStatus,
    expected_messages: usize,
    messages_persisted: Option<usize>,
) -> TurnAttemptRecord {
    let mut record =
        TurnAttemptRecord::new(&session.turn_id, &session.id, &session.nous_id, status);
    record.model = Some(result.model_used.clone());
    record.expected_messages = Some(expected_messages);
    record.messages_persisted = messages_persisted;
    record
}

/// Persist turn results to the session store.
///
/// Errors from the store are propagated but callers should treat them as
/// non-fatal: the user already has their response.
///
/// # Session guarantee
///
/// The nous actor creates sessions in memory (not in the fjall store). We
/// create the session row before the pending lifecycle note, then include the
/// row again in the batched finalize transaction so message, usage, and
/// completion writes share one commit.
#[instrument(skip_all, fields(session_id = %session.id))]
pub(crate) fn finalize(
    store: &SessionStore,
    session: &SessionState,
    input_content: &str,
    result: &TurnResult,
    config: &FinalizeConfig,
) -> error::Result<FinalizeResult> {
    // INVARIANT: dedup guard: skip if this turn was already finalized.
    //
    // WHY: canonical turn_id is the primary idempotency key. The legacy
    // usage_exists_for_turn guard remains for backward compatibility with
    // turns finalized before the turn-attempt note protocol.
    let turn_seq = turn_seq_from_ulid(&session.turn_id);
    let usage_exists = store
        .usage_exists_for_turn(&session.id, turn_seq)
        .context(error::StoreSnafu)?;
    let completed_exists =
        crate::turn_record::is_turn_completed(store, &session.id, &session.turn_id)?;
    if usage_exists || completed_exists {
        if usage_exists && !completed_exists {
            let expected_messages = if config.persist_messages {
                2 + result.tool_calls.len().saturating_mul(2)
            } else {
                0
            };
            let completed = turn_attempt_record(
                session,
                result,
                TurnAttemptStatus::Completed,
                expected_messages,
                Some(expected_messages),
            );
            crate::turn_record::persist_turn_attempt(store, &session.nous_id, &completed)?;
        }
        warn!(
            turn = session.turn,
            turn_id = %session.turn_id,
            "finalize called twice for same turn, skipping duplicate"
        );
        return Ok(FinalizeResult::new(0, false));
    }

    let pending_messages = build_finalize_messages(input_content, result, config.persist_messages);
    let expected_messages = pending_messages.len();

    // WHY: ensure the session row exists before the pending note; the batched
    // store call below still owns the atomic turn payload write.
    store
        .find_or_create_session(
            &session.id,
            &session.nous_id,
            &session.session_key,
            Some(&session.model),
            None,
        )
        .context(error::StoreSnafu)?;

    let pending = turn_attempt_record(
        session,
        result,
        TurnAttemptStatus::FinalizePending,
        expected_messages,
        Some(0),
    );
    crate::turn_record::persist_turn_attempt(store, &session.nous_id, &pending)?;
    #[cfg(test)]
    test_failure_injection::maybe_fail_after_pending()?;

    let finalize_messages: Vec<FinalizeMessage<'_>> = pending_messages
        .iter()
        .map(|msg| FinalizeMessage {
            role: msg.role,
            content: msg.content.as_str(),
            tool_call_id: msg.tool_call_id,
            tool_name: msg.tool_name,
            token_estimate: msg.token_estimate,
        })
        .collect();
    let usage = config
        .record_usage
        .then(|| usage_record(session, result, turn_seq));
    let completed = turn_attempt_record(
        session,
        result,
        TurnAttemptStatus::Completed,
        expected_messages,
        Some(expected_messages),
    );
    let completed_content = crate::turn_record::serialize_turn_attempt(&completed)?;

    let finalized = store
        .finalize_turn(&FinalizeTurnRequest {
            session_id: &session.id,
            nous_id: &session.nous_id,
            session_key: &session.session_key,
            model: Some(&session.model),
            parent_session_id: None,
            messages: &finalize_messages,
            usage: usage.as_ref(),
            completion_note: Some(FinalizeNote {
                category: crate::turn_record::TURN_NOTE_CATEGORY,
                content: &completed_content,
            }),
        })
        .context(error::StoreSnafu)?;

    debug!(
        messages_persisted = finalized.messages_persisted,
        usage_recorded = finalized.usage_recorded,
        "finalize complete"
    );
    Ok(FinalizeResult::new(
        finalized.messages_persisted,
        finalized.usage_recorded,
    ))
}

#[cfg(test)]
mod test_failure_injection {
    use std::cell::Cell;

    use crate::error;

    thread_local! {
        static FAIL_AFTER_PENDING: Cell<bool> = const { Cell::new(false) };
    }

    pub(super) fn fail_after_pending() {
        FAIL_AFTER_PENDING.with(|cell| cell.set(true));
    }

    pub(super) fn maybe_fail_after_pending() -> error::Result<()> {
        FAIL_AFTER_PENDING.with(|cell| {
            if !cell.replace(false) {
                return Ok(());
            }
            Err(error::PipelineStageSnafu {
                stage: "finalize",
                message: "injected failure after pending marker".to_owned(),
            }
            .build())
        })
    }
}

#[expect(
    clippy::indexing_slicing,
    reason = "test: vec indices are valid after asserting len"
)]
#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::config::NousConfig;
    use crate::pipeline::{ToolCall, TurnUsage};

    fn make_store_and_session() -> (SessionStore, SessionState) {
        let store = SessionStore::open_in_memory().expect("in-memory store");
        store
            .create_session("ses-1", "test-nous", "main", None, Some("test-model"))
            .expect("create session");
        let config = NousConfig {
            id: Arc::from("test-nous"),
            generation: crate::config::NousGenerationConfig {
                model: "test-model".to_owned(),
                ..crate::config::NousGenerationConfig::default()
            },
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
            degraded: None,
            reasoning: String::new(),
            model_used: "test-model".to_owned(),
            tool_surface_hashes: Vec::new(),
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
                receipt: None,
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
            degraded: None,
            reasoning: String::new(),
            model_used: "test-model".to_owned(),
            tool_surface_hashes: Vec::new(),
        }
    }

    fn partial_result(stop_reason: &str) -> TurnResult {
        TurnResult {
            stop_reason: stop_reason.to_owned(),
            ..simple_result()
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

    /// Regression test for #4915: partial terminal outcomes must not be
    /// persisted as indistinguishable successful assistant messages. Because
    /// `graphe::Message` has no metadata field for `stop_reason`, we use an
    /// inline history-convention marker.
    #[test]
    fn finalize_marks_partial_stop_reasons() {
        for reason in ["client_disconnect", "max_tool_iterations"] {
            let (store, session) = make_store_and_session();
            let result = partial_result(reason);
            let config = FinalizeConfig::default();

            finalize(&store, &session, "Hi", &result, &config).expect("finalize");

            let history = store.get_history("ses-1", None).expect("history");
            assert_eq!(history.len(), 2, "{reason}: user + assistant");
            assert_eq!(history[1].role, Role::Assistant);
            assert_eq!(
                history[1].content,
                format!("[partial: {reason}] Hello!"),
                "{reason}: assistant message should carry partial marker"
            );
        }
    }

    #[test]
    fn finalize_does_not_mark_clean_end_turn() {
        let (store, session) = make_store_and_session();
        let result = simple_result();
        let config = FinalizeConfig::default();

        finalize(&store, &session, "Hi", &result, &config).expect("finalize");

        let history = store.get_history("ses-1", None).expect("history");
        assert_eq!(history.len(), 2);
        assert_eq!(history[1].role, Role::Assistant);
        assert_eq!(history[1].content, "Hello!");
    }

    #[test]
    fn finalize_partial_marker_handles_empty_content() {
        let (store, session) = make_store_and_session();
        let mut result = partial_result("client_disconnect");
        result.content.clear();
        let config = FinalizeConfig::default();

        finalize(&store, &session, "Hi", &result, &config).expect("finalize");

        let history = store.get_history("ses-1", None).expect("history");
        assert_eq!(history[1].content, "[partial: client_disconnect]");
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
        assert!(history[2].content.starts_with("[tool:read_file@"));
        assert!(history[2].content.contains("file contents here"));
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
    /// record does not yet exist in the fjall store. The nous actor creates sessions in
    /// memory, so the first call to finalize is responsible for ensuring the
    /// row exists before inserting child messages (FOREIGN KEY constraint).
    #[test]
    fn finalize_creates_session_if_missing() {
        let store = SessionStore::open_in_memory().expect("in-memory store");
        // WHY: Do NOT call store.create_session: the actor wouldn't have done so.
        let config_nous = NousConfig {
            id: Arc::from("test-nous"),
            generation: crate::config::NousGenerationConfig {
                model: "test-model".to_owned(),
                ..crate::config::NousGenerationConfig::default()
            },
            ..NousConfig::default()
        };
        let session = SessionState::new("ses-orphan".to_owned(), "main".to_owned(), &config_nous);
        let result = simple_result();
        let config = FinalizeConfig::default();

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
    fn finalize_records_observed_model_used() {
        let (store, session) = make_store_and_session();
        let mut result = simple_result();
        result.model_used = "fallback-model".to_owned();
        let config = FinalizeConfig::default();

        finalize(&store, &session, "Hi", &result, &config).expect("finalize");

        let usage = store.get_usage_for_session("ses-1").expect("usage");
        assert_eq!(usage.len(), 1);
        assert_eq!(usage[0].model.as_deref(), Some("fallback-model"));

        let records =
            crate::turn_record::turn_attempt_records(&store, &session.id, &session.turn_id)
                .expect("turn records");
        assert_eq!(records.len(), 2);
        assert_eq!(
            records[0].status,
            crate::turn_record::TurnAttemptStatus::FinalizePending
        );
        assert_eq!(records[0].model.as_deref(), Some("fallback-model"));
        assert_eq!(
            records[1].status,
            crate::turn_record::TurnAttemptStatus::Completed
        );
        assert_eq!(records[1].model.as_deref(), Some("fallback-model"));
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
        // WHY(#4895): session id is bound to one (nous_id, session_key); session2
        // must match the row created above or find_or_create_session fails closed.
        session2.session_key = "main-2".to_owned();

        // NOTE: One tool call: user + tool_call + tool_result + assistant = 4
        let result = result_with_tools();
        let fr = finalize(&store, &session2, "Read it", &result, &config).expect("finalize");
        assert_eq!(fr.messages_persisted, 4);
    }

    /// Regression test for #758/#916/#923: session ID divergence.
    ///
    /// Simulates the scenario where pylon creates a session with DB ID "A"
    /// while the actor's `SessionState` holds a different ID "B". Without
    /// session-ID adoption, `find_or_create_session` finds the existing session
    /// by (`nous_id`, `session_key`) and returns ID "A" while `append_message`
    /// uses the actor's ID "B", causing an FK constraint violation. The actor
    /// adopts the DB session ID so both match.
    #[test]
    fn finalize_with_matching_session_id_no_fk_violation() {
        let store = SessionStore::open_in_memory().expect("in-memory store");
        let db_session_id = "db-ses-from-pylon";

        store
            .create_session(db_session_id, "test-nous", "main", None, Some("test-model"))
            .expect("create session");

        // WHY: Actor's SessionState must use the SAME ID as the database.
        let config = NousConfig {
            id: Arc::from("test-nous"),
            generation: crate::config::NousGenerationConfig {
                model: "test-model".to_owned(),
                ..crate::config::NousGenerationConfig::default()
            },
            ..NousConfig::default()
        };
        let session = SessionState::new(db_session_id.to_owned(), "main".to_owned(), &config);

        let result = simple_result();
        let finalize_config = FinalizeConfig::default();

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

        let config = NousConfig {
            id: Arc::from("test-nous"),
            generation: crate::config::NousGenerationConfig {
                model: "test-model".to_owned(),
                ..crate::config::NousGenerationConfig::default()
            },
            ..NousConfig::default()
        };
        let divergent_session =
            SessionState::new("actor-generated-id".to_owned(), "main".to_owned(), &config);

        let result = simple_result();
        let finalize_config = FinalizeConfig::default();

        // WHY: finalize calls find_or_create_session with the actor's
        // session.id; an active session already exists for
        // (nous_id, session_key), so the existing "pylon-id" row is returned
        // and no row is created for the actor's ID. append_message then uses
        // "actor-generated-id", which has no row → FK error.
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

    /// Regression test for #4691: a completed turn-attempt note prevents
    /// duplicate finalization even when the legacy usage guard is absent.
    #[test]
    fn finalize_note_dedup_skips_without_usage_row() {
        let (store, session) = make_store_and_session();
        let result = simple_result();
        let config = FinalizeConfig::default();

        finalize(&store, &session, "Hi", &result, &config).expect("first finalize");
        // Remove the usage row so only the completed note remains.
        store
            .cleanup_usage_records("ses-1", 0)
            .expect("clear usage");

        let fr2 =
            finalize(&store, &session, "Hi again", &result, &config).expect("second finalize");
        assert_eq!(fr2.messages_persisted, 0);
        assert!(!fr2.usage_recorded);

        let history = store.get_history("ses-1", None).expect("history");
        assert_eq!(history.len(), 2);
    }

    #[test]
    fn finalize_failure_after_pending_is_detectable_and_retry_does_not_duplicate() {
        let (store, session) = make_store_and_session();
        let result = simple_result();
        let config = FinalizeConfig::default();

        test_failure_injection::fail_after_pending();
        let failed = finalize(&store, &session, "Hi", &result, &config);
        assert!(failed.is_err(), "injected failure should abort finalize");

        let history = store.get_history("ses-1", None).expect("history");
        assert!(
            history.is_empty(),
            "failure before atomic finalize_turn should leave no messages"
        );
        let usage = store.get_usage_for_session("ses-1").expect("usage");
        assert!(usage.is_empty(), "failed turn should not record usage");
        let records =
            crate::turn_record::turn_attempt_records(&store, &session.id, &session.turn_id)
                .expect("turn records");
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].status, TurnAttemptStatus::FinalizePending);
        assert_eq!(records[0].messages_persisted, Some(0));
        assert_eq!(records[0].expected_messages, Some(2));

        let retried = finalize(&store, &session, "Hi", &result, &config).expect("retry finalize");
        assert_eq!(retried.messages_persisted, 2);
        assert!(retried.usage_recorded);

        let history = store
            .get_history("ses-1", None)
            .expect("history after retry");
        assert_eq!(history.len(), 2, "retry should write one clean turn");
        let usage = store
            .get_usage_for_session("ses-1")
            .expect("usage after retry");
        assert_eq!(usage.len(), 1);

        let duplicate =
            finalize(&store, &session, "Hi again", &result, &config).expect("duplicate finalize");
        assert_eq!(duplicate.messages_persisted, 0);
        assert!(!duplicate.usage_recorded);

        let history = store
            .get_history("ses-1", None)
            .expect("history after duplicate");
        assert_eq!(history.len(), 2, "duplicate retry must not append");
        assert!(
            crate::turn_record::is_turn_completed(&store, &session.id, &session.turn_id)
                .expect("completed marker")
        );
    }
}
