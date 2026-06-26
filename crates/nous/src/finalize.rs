//! Finalize stage: persists the pipeline result to durable storage.
//!
//! After the execute stage produces a `TurnResult`, the finalize stage:
//! 1. Persists the user's message
//! 2. Persists tool call/result messages
//! 3. Persists the assistant's response
//! 4. Records token usage
//!
//! WHY: graphe commits each append in its own fjall transaction, so finalize
//! records a durable turn-attempt note keyed by the canonical turn id. A
//! `Completed` note short-circuits retries, making finalization idempotent
//! across messages and usage records.

use koina::ulid::Ulid;
use snafu::ResultExt;
use tracing::{debug, instrument, warn};

use mneme::store::SessionStore;
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

/// Persist turn results to the session store.
///
/// Errors from the store are propagated but callers should treat them as
/// non-fatal: the user already has their response.
///
/// # Session guarantee
///
/// The nous actor creates sessions in memory (not in the fjall store). Before
/// appending messages we verify the session record exists in the store,
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
    // INVARIANT: dedup guard: skip if this turn was already finalized.
    //
    // WHY: canonical turn_id is the primary idempotency key. The legacy
    // usage_exists_for_turn guard remains for backward compatibility with
    // turns finalized before the turn-attempt note protocol.
    let turn_seq = turn_seq_from_ulid(&session.turn_id);
    let already_finalized = store
        .usage_exists_for_turn(&session.id, turn_seq)
        .context(error::StoreSnafu)?
        || crate::turn_record::is_turn_completed(store, &session.id, &session.turn_id)?;
    if already_finalized {
        warn!(
            turn = session.turn,
            turn_id = %session.turn_id,
            "finalize called twice for same turn, skipping duplicate"
        );
        return Ok(FinalizeResult::new(0, false));
    }

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

    let mut pending = TurnAttemptRecord::new(
        &session.turn_id,
        &session.id,
        &session.nous_id,
        TurnAttemptStatus::FinalizePending,
    );
    pending.model = Some(session.model.clone());
    crate::turn_record::persist_turn_attempt(store, &session.nous_id, &pending)?;

    let mut messages_persisted = 0usize;

    if config.persist_messages {
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

            let tool_output = tc.result.as_deref().unwrap_or("[missing tool result]");
            let compact_tagged_output = crate::compact::micro::format_tool_result(
                &tc.name,
                jiff::Timestamp::now(),
                tool_output,
            );
            store
                .append_message(
                    &session.id,
                    Role::ToolResult,
                    &compact_tagged_output,
                    Some(&tc.id),
                    Some(&tc.name),
                    0,
                )
                .context(error::StoreSnafu)?;
            messages_persisted += 1;
        }

        let output_tokens = i64::try_from(result.usage.output_tokens).unwrap_or(0);
        let assistant_content = format_assistant_content(&result.content, &result.stop_reason);
        store
            .append_message(
                &session.id,
                Role::Assistant,
                &assistant_content,
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

    let final_status = if result.degraded.is_some() {
        TurnAttemptStatus::Degraded
    } else {
        TurnAttemptStatus::Completed
    };
    let mut completed = TurnAttemptRecord::new(
        &session.turn_id,
        &session.id,
        &session.nous_id,
        final_status,
    );
    completed.model = Some(session.model.clone());

    // WHY: degraded turns must leave a durable provenance record instead of
    // looking like successful assistant turns. Copy fields from the in-memory
    // `DegradedMode` into the turn ledger.
    if let Some(degraded) = &result.degraded {
        let provenance = degraded.provenance();
        completed.attempted_model = Some(provenance.attempted_model.clone());
        completed
            .routed_model_context
            .clone_from(&provenance.routed_model_context);
        completed.error_class = Some(provenance.error_class.clone());
        completed.error_hash = Some(provenance.error_message_hash.clone());
        completed.degradation_source = Some(provenance.source.clone());
        completed
            .distillation_id
            .clone_from(&provenance.distillation_id);
        completed.user_content_saved = Some(messages_persisted >= 1);
    }

    crate::turn_record::persist_turn_attempt(store, &session.nous_id, &completed)?;

    debug!(messages_persisted, usage_recorded, "finalize complete");
    Ok(FinalizeResult::new(messages_persisted, usage_recorded))
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
    use crate::degraded_mode::build_degraded_response;
    use crate::pipeline::{ToolCall, TurnUsage};
    use crate::turn_record::latest_turn_attempt_record;
    use snafu::IntoError as _;

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

    fn degraded_result_with_cache() -> TurnResult {
        let err = crate::error::LlmSnafu.into_error(
            hermeneus::error::RateLimitedSnafu {
                retry_after_ms: 5000u64,
            }
            .build(),
        );
        build_degraded_response(
            "test-nous",
            "ses-1",
            &err,
            Some("cached context"),
            "test-model",
            Some("routed-test-model"),
        )
    }

    fn degraded_result_without_cache() -> TurnResult {
        let err = crate::error::LlmSnafu.into_error(
            hermeneus::error::RateLimitedSnafu {
                retry_after_ms: 5000u64,
            }
            .build(),
        );
        build_degraded_response("test-nous", "ses-1", &err, None, "test-model", None)
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

    /// Regression test for #4914: degraded turns with a distillation cache must
    /// be persisted as terminal `Degraded` outcomes with full provenance.
    #[test]
    fn finalize_persists_degraded_with_cache_provenance() {
        let (store, session) = make_store_and_session();
        let result = degraded_result_with_cache();
        let config = FinalizeConfig::default();

        let fr = finalize(&store, &session, "Hi", &result, &config).expect("finalize");
        assert_eq!(fr.messages_persisted, 2);

        let record = latest_turn_attempt_record(&store, &session.id, &session.turn_id)
            .expect("read")
            .expect("record");
        assert_eq!(record.status, TurnAttemptStatus::Degraded);
        assert_eq!(record.attempted_model.as_deref(), Some("test-model"));
        assert_eq!(
            record.routed_model_context.as_deref(),
            Some("routed-test-model")
        );
        assert_eq!(record.error_class.as_deref(), Some("transient"));
        assert!(record.error_hash.as_deref().expect("error_hash").len() >= 16);
        assert_eq!(
            record.degradation_source.as_deref(),
            Some("distillation_cache")
        );
        assert!(
            record
                .distillation_id
                .as_deref()
                .expect("distillation_id")
                .starts_with("ses-1:distillation:seq=0:")
        );
        assert_eq!(record.user_content_saved, Some(true));

        // WHY: degraded usage must distinguish synthetic output from provider-served output.
        let usage_records = store.get_usage_for_session(&session.id).expect("usage");
        let usage = usage_records
            .into_iter()
            .find(|u| u.turn_seq == turn_seq_from_ulid(&session.turn_id))
            .expect("usage row for turn");
        assert!(usage.output_tokens > 0);
    }

    /// Regression test for #4914: degraded turns without a cache must be
    /// persisted as terminal `Degraded` outcomes with provenance.
    #[test]
    fn finalize_persists_degraded_without_cache_provenance() {
        let (store, session) = make_store_and_session();
        let result = degraded_result_without_cache();
        let config = FinalizeConfig::default();

        let fr = finalize(&store, &session, "Hi", &result, &config).expect("finalize");
        assert_eq!(fr.messages_persisted, 2);

        let record = latest_turn_attempt_record(&store, &session.id, &session.turn_id)
            .expect("read")
            .expect("record");
        assert_eq!(record.status, TurnAttemptStatus::Degraded);
        assert_eq!(record.attempted_model.as_deref(), Some("test-model"));
        assert!(record.routed_model_context.is_none());
        assert_eq!(record.error_class.as_deref(), Some("transient"));
        assert!(record.error_hash.as_deref().expect("error_hash").len() >= 16);
        assert_eq!(record.degradation_source.as_deref(), Some("unavailable"));
        assert!(record.distillation_id.is_none());
        assert_eq!(record.user_content_saved, Some(true));

        let usage_records = store.get_usage_for_session(&session.id).expect("usage");
        let usage = usage_records
            .into_iter()
            .find(|u| u.turn_seq == turn_seq_from_ulid(&session.turn_id))
            .expect("usage row for turn");
        assert!(usage.output_tokens > 0);
    }
}
