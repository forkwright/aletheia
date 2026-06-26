//! Durable turn-attempt lifecycle records and finalize state.
//!
//! Turn attempts are persisted as agent notes with category `"context"` so that
//! every attempted turn — success, failure, cancellation, timeout, or partial
//! finalize — leaves a durable, inspectable record. Notes are filtered by
//! canonical turn id; the latest note for a turn is the authoritative lifecycle
//! state.
//!
//! WHY: the graphe store does not expose a multi-write transaction, so nous
//! builds an idempotency/recovery protocol on top of the existing note and
//! message partitions rather than mutating the lower-level store.

use koina::ulid::Ulid;
use serde::{Deserialize, Serialize};
use snafu::ResultExt;

use mneme::store::SessionStore;

use crate::error;

/// Lifecycle statuses for a turn attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum TurnAttemptStatus {
    /// Turn accepted by the actor and the pipeline is starting.
    Accepted,
    /// Turn is actively running inside the pipeline.
    Running,
    /// Turn completed and finalized successfully.
    Completed,
    /// Turn failed with an error before finalize.
    Failed,
    /// Turn was cancelled by operator or system.
    Cancelled,
    /// Turn completed in degraded mode.
    Degraded,
    /// Turn was denied by the approval gate.
    ApprovalDenied,
    /// Turn timed out.
    Timeout,
    /// Finalize stage failed after a successful execute.
    FinalizeFailed,
    /// Finalize is in progress; records how many messages are durable so far.
    FinalizePending,
}

/// Durable record of a turn-attempt state transition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TurnAttemptRecord {
    /// Schema version for forward compatibility.
    pub version: u32,
    /// Canonical turn identity (ULID).
    pub turn_id: String, // kanon:ignore RUST/primitive-for-domain-id
    /// Session this turn belongs to.
    pub session_id: String, // kanon:ignore RUST/primitive-for-domain-id
    /// Owning agent identifier.
    pub nous_id: String, // kanon:ignore RUST/primitive-for-domain-id
    /// Current status.
    pub status: TurnAttemptStatus,
    /// Pipeline stage that emitted this record, when relevant.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stage: Option<String>,
    /// Human-readable error code for failed/cancelled/degraded states.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_code: Option<String>,
    /// Redacted error message.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_message: Option<String>,
    /// Provider/model context, when known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// Provider/model that was attempted before a degraded fallback.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attempted_model: Option<String>,
    /// Routed model context when complexity routing was active.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub routed_model_context: Option<String>,
    /// Error class of the original failure for degraded outcomes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_class: Option<String>,
    /// Stable hash of the original failure message for log correlation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_hash: Option<String>,
    /// Degradation source identifier (e.g. `distillation_cache`, `unavailable`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub degradation_source: Option<String>,
    /// Distillation cache reference when one was used.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub distillation_id: Option<String>,
    /// Whether the original user content was persisted.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_content_saved: Option<bool>,
    /// Number of messages already persisted when status is `FinalizePending`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub messages_persisted: Option<usize>,
    /// Expected total messages for this finalize attempt.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expected_messages: Option<usize>,
    /// ISO 8601 timestamp.
    pub created_at: String,
}

impl TurnAttemptRecord {
    /// Create a new lifecycle record for a turn.
    pub fn new(turn_id: &Ulid, session_id: &str, nous_id: &str, status: TurnAttemptStatus) -> Self {
        Self {
            version: 1,
            turn_id: turn_id.to_string(),
            session_id: session_id.to_owned(),
            nous_id: nous_id.to_owned(),
            status,
            stage: None,
            error_code: None,
            error_message: None,
            model: None,
            attempted_model: None,
            routed_model_context: None,
            error_class: None,
            error_hash: None,
            degradation_source: None,
            distillation_id: None,
            user_content_saved: None,
            messages_persisted: None,
            expected_messages: None,
            created_at: jiff::Timestamp::now().to_string(),
        }
    }
}

const TURN_NOTE_CATEGORY: &str = "context";

/// Persist a turn-attempt lifecycle record as a durable agent note.
///
/// Errors are returned rather than swallowed so callers can decide whether the
/// failure is fatal for the turn. Lifecycle telemetry must never block the
/// response path.
pub fn persist_turn_attempt(
    store: &SessionStore,
    nous_id: &str,
    record: &TurnAttemptRecord,
) -> error::Result<()> {
    let content = serde_json::to_string(record).map_err(|e| {
        error::ContextAssemblySnafu {
            message: format!("turn attempt record serialization failed: {e}"),
        }
        .build()
    })?;
    store
        .add_note(&record.session_id, nous_id, TURN_NOTE_CATEGORY, &content)
        .context(error::StoreSnafu)?;
    // WHY: notes do not call `ensure_durable` by default; for lifecycle records
    // to survive an unclean shutdown we must sync the WAL before continuing.
    store.ensure_durable().context(error::StoreSnafu)?;
    Ok(())
}

/// Read all turn-attempt records for a given turn, oldest first.
pub fn turn_attempt_records(
    store: &SessionStore,
    session_id: &str,
    turn_id: &Ulid,
) -> error::Result<Vec<TurnAttemptRecord>> {
    let notes = store.get_notes(session_id).context(error::StoreSnafu)?;
    let mut records: Vec<TurnAttemptRecord> = notes
        .into_iter()
        .filter_map(|note| {
            if note.category != TURN_NOTE_CATEGORY {
                return None;
            }
            serde_json::from_str::<TurnAttemptRecord>(&note.content).ok()
        })
        .filter(|record: &TurnAttemptRecord| record.turn_id == turn_id.to_string())
        .collect();
    records.sort_by(|a, b| a.created_at.cmp(&b.created_at));
    Ok(records)
}

/// Return the latest turn-attempt record for a turn, if any.
pub fn latest_turn_attempt_record(
    store: &SessionStore,
    session_id: &str,
    turn_id: &Ulid,
) -> error::Result<Option<TurnAttemptRecord>> {
    let mut records = turn_attempt_records(store, session_id, turn_id)?;
    Ok(records.pop())
}

/// Return true if the latest record for this turn is `Completed`.
pub fn is_turn_completed(
    store: &SessionStore,
    session_id: &str,
    turn_id: &Ulid,
) -> error::Result<bool> {
    Ok(latest_turn_attempt_record(store, session_id, turn_id)?
        .is_some_and(|r| r.status == TurnAttemptStatus::Completed))
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
#[expect(
    clippy::indexing_slicing,
    reason = "test assertions on collections with previously verified length"
)]
mod tests {
    use super::*;

    fn make_store_and_session() -> (SessionStore, crate::session::SessionState) {
        let store = SessionStore::open_in_memory().expect("in-memory store");
        store
            .create_session("ses-1", "test-nous", "main", None, Some("test-model"))
            .expect("create session");
        let config = crate::config::NousConfig {
            id: std::sync::Arc::from("test-nous"),
            generation: crate::config::NousGenerationConfig {
                model: "test-model".to_owned(),
                ..crate::config::NousGenerationConfig::default()
            },
            ..crate::config::NousConfig::default()
        };
        let session =
            crate::session::SessionState::new("ses-1".to_owned(), "main".to_owned(), &config);
        (store, session)
    }

    #[test]
    fn persist_and_read_turn_record() {
        let (store, session) = make_store_and_session();
        let record = TurnAttemptRecord::new(
            &session.turn_id,
            &session.id,
            "test-nous",
            TurnAttemptStatus::Running,
        );

        persist_turn_attempt(&store, "test-nous", &record).expect("persist");

        let records = turn_attempt_records(&store, &session.id, &session.turn_id).expect("read");
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].status, TurnAttemptStatus::Running);
        assert_eq!(records[0].turn_id, session.turn_id.to_string());
    }

    #[test]
    fn is_turn_completed_detects_completed_status() {
        let (store, session) = make_store_and_session();
        assert!(!is_turn_completed(&store, &session.id, &session.turn_id).expect("check"));

        let record = TurnAttemptRecord::new(
            &session.turn_id,
            &session.id,
            "test-nous",
            TurnAttemptStatus::Completed,
        );
        persist_turn_attempt(&store, "test-nous", &record).expect("persist");

        assert!(is_turn_completed(&store, &session.id, &session.turn_id).expect("check"));
    }
}
