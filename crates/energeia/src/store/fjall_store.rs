//! `EnergeiaStore` — fjall-backed state persistence for dispatch orchestration.

use std::sync::Arc;

use eidos::id::FactId;
use eidos::knowledge::{
    EpistemicTier, Fact, FactAccess, FactLifecycle, FactProvenance, FactSensitivity, FactTemporal,
    Visibility,
};

use crate::error::{self, Result};
use crate::store::queries;
use crate::store::records::{
    CiValidationRecord, CiValidationStatus, DispatchId, DispatchRecord, DispatchStatus,
    LessonRecord, NewLesson, NewObservation, ObservationRecord, QaVerdictRecord, SessionId,
    SessionOutcomeData, SessionRecord, SessionUpdate,
};

/// Maximum records returned by bulk-scan methods to prevent unbounded memory use.
pub(crate) const SCAN_LIMIT_DISPATCHES: usize = 10_000;
pub(crate) const SCAN_LIMIT_SESSIONS: usize = 100_000;
pub(crate) const SCAN_LIMIT_CI_VALIDATIONS: usize = 200_000;
pub(crate) const SCAN_LIMIT_QA_VERDICTS: usize = 200_000;
use crate::store::schema;
use crate::types::{DispatchSpec, SessionOutcome, SessionStatus};

/// Number of hours after which a `Running` dispatch is considered stale.
const STALE_RUNNING_DISPATCH_THRESHOLD_HOURS: i64 = 1;

/// Partition name for energeia state within the shared fjall database.
const PARTITION_NAME: &str = "energeia";

fn store_err(context: &str, e: impl std::fmt::Display) -> error::Error {
    error::StoreSnafu {
        message: format!("{context}: {e}"),
    }
    .build()
}

fn ser_err(context: &str, e: impl std::fmt::Display) -> error::Error {
    error::SerializationSnafu {
        message: format!("{context}: {e}"),
    }
    .build()
}

fn serialize_msgpack<T: serde::Serialize>(value: &T, context: &str) -> Result<Vec<u8>> {
    rmp_serde::to_vec(value).map_err(|e| ser_err(context, e))
}

/// Runtime policy threshold for stale `Running` dispatch reconciliation.
#[must_use]
pub fn stale_running_dispatch_threshold() -> jiff::SignedDuration {
    jiff::SignedDuration::from_hours(STALE_RUNNING_DISPATCH_THRESHOLD_HOURS)
}

/// State persistence layer wrapping a fjall keyspace.
///
/// All dispatch, session, lesson, observation, and CI validation records are
/// stored in a dedicated `"energeia"` partition with byte-prefixed keys for
/// efficient prefix scans.
pub struct EnergeiaStore {
    keyspace: Arc<fjall::Keyspace>,
    db: fjall::Database,
}

// NOTE: Storage methods stay `pub` for external tooling: steward workflows,
// metrics test fixtures, and the mneme training-data pipeline.
impl EnergeiaStore {
    /// Create a new store backed by a dedicated partition in the given database.
    ///
    /// # Errors
    ///
    /// Returns `Error::Store` if the partition cannot be opened.
    pub fn new(db: &fjall::Database) -> Result<Self> {
        let keyspace = db
            .keyspace(PARTITION_NAME, fjall::KeyspaceCreateOptions::default)
            .map_err(|e| store_err("open partition", e))?;
        Ok(Self {
            keyspace: Arc::new(keyspace),
            db: db.clone(),
        })
    }

    /// Flush the WAL to stable storage so committed writes survive power loss.
    ///
    /// Call this after every `keyspace.insert()` on the operational write path
    /// (dispatch state transitions, session status changes). Analytics records
    /// (lessons, observations) tolerate WAL-only durability.
    fn ensure_durable(&self) -> Result<()> {
        self.db
            .persist(fjall::PersistMode::SyncAll)
            .map_err(|e| store_err("fjall persist", e))
    }

    /// The underlying keyspace name.
    #[must_use]
    pub fn partition_name() -> &'static str {
        PARTITION_NAME
    }

    // ── Dispatch CRUD ──

    /// Create a new dispatch record. Returns the generated `DispatchId`.
    ///
    /// # Errors
    ///
    /// Returns `Error::Store` on write failure, `Error::Serialization` on
    /// encoding failure.
    pub(crate) fn create_dispatch(&self, project: &str, spec: &DispatchSpec) -> Result<DispatchId> {
        let id = DispatchId::new(koina::ulid::Ulid::new().to_string());
        let spec_json =
            serde_json::to_string(spec).map_err(|e| ser_err("serialize dispatch spec", e))?;

        let record = DispatchRecord {
            id: id.clone(),
            project: project.to_owned(),
            spec: spec_json,
            status: DispatchStatus::Running,
            created_at: jiff::Timestamp::now(),
            finished_at: None,
            total_cost_usd: 0.0,
            total_sessions: 0,
        };

        let key = schema::dispatch_key(&id);
        let value = serialize_msgpack(&record, "dispatch record")?;

        self.keyspace
            .insert(key.as_bytes(), value)
            .map_err(|e| store_err("write dispatch", e))?;
        self.ensure_durable()?;

        Ok(id)
    }

    /// Mark a dispatch as finished with the given status. Aggregates session
    /// costs and counts.
    ///
    /// # Errors
    ///
    /// Returns `Error::NotFound` if the dispatch does not exist.
    pub(crate) fn finish_dispatch(&self, id: &DispatchId, status: DispatchStatus) -> Result<()> {
        let mut record = self.get_dispatch(id)?.ok_or_else(|| {
            error::NotFoundSnafu {
                what: format!("dispatch {id}"),
            }
            .build()
        })?;

        let sessions = self.list_sessions_for_dispatch(id)?;
        let total_cost: f64 = sessions.iter().map(|s| s.cost_usd).sum();
        // WHY: session count is bounded by prompt count; saturate as belt-and-braces
        // since u32::MAX sessions per dispatch is unreachable in practice.
        let total_sessions = u32::try_from(sessions.len()).unwrap_or(u32::MAX);

        record.status = status;
        record.finished_at = Some(jiff::Timestamp::now());
        record.total_cost_usd = total_cost;
        record.total_sessions = total_sessions;

        let key = schema::dispatch_key(id);
        let value = serialize_msgpack(&record, "updated dispatch")?;

        self.keyspace
            .insert(key.as_bytes(), value)
            .map_err(|e| store_err("update dispatch", e))?;
        self.ensure_durable()?;

        Ok(())
    }

    /// Retrieve a dispatch record by ID.
    ///
    /// # Errors
    ///
    /// Returns `Error::Store` on read failure.
    pub(crate) fn get_dispatch(&self, id: &DispatchId) -> Result<Option<DispatchRecord>> {
        let key = schema::dispatch_key(id);
        match self
            .keyspace
            .get(key.as_bytes())
            .map_err(|e| store_err("read dispatch", e))?
        {
            Some(value) => Ok(Some(queries::deserialize_value(&value)?)),
            None => Ok(None),
        }
    }

    // ── Session CRUD ──

    /// Create a new session record within a dispatch.
    ///
    /// # Errors
    ///
    /// Returns `Error::Store` on write failure.
    pub fn create_session(
        &self,
        dispatch_id: &DispatchId,
        prompt_number: u32,
    ) -> Result<SessionId> {
        let id = SessionId::new(koina::ulid::Ulid::new().to_string());
        let now = jiff::Timestamp::now();

        let record = SessionRecord {
            id: id.clone(),
            dispatch_id: dispatch_id.clone(),
            prompt_number,
            status: SessionStatus::Skipped,
            session_id: None,
            cost_usd: 0.0,
            num_turns: 0,
            duration_ms: 0,
            pr_url: None,
            error: None,
            created_at: now,
            updated_at: now,
        };

        let key = schema::session_key(dispatch_id, prompt_number);
        let value = serialize_msgpack(&record, "session record")?;

        self.keyspace
            .insert(key.as_bytes(), value)
            .map_err(|e| store_err("write session", e))?;
        let index_key = schema::session_by_id_key(&id);
        let index_value = serialize_msgpack(&record, "session reverse index")?;
        self.keyspace
            .insert(index_key.as_bytes(), index_value)
            .map_err(|e| store_err("write session reverse index", e))?;
        self.ensure_durable()?;

        Ok(id)
    }

    /// Apply a partial update to an existing session record.
    ///
    /// # Errors
    ///
    /// Returns `Error::NotFound` if the session does not exist.
    pub fn update_session(&self, id: &SessionId, update: SessionUpdate) -> Result<()> {
        let index_key = schema::session_by_id_key(id);
        let mut record = match self
            .keyspace
            .get(index_key.as_bytes())
            .map_err(|e| store_err("read session reverse index", e))?
        {
            Some(value) => queries::deserialize_value::<SessionRecord>(&value)?,
            None => {
                return Err(error::NotFoundSnafu {
                    what: format!("session {id}"),
                }
                .build());
            }
        };

        if record.id != *id {
            return Err(error::StoreSnafu {
                message: format!("session reverse index mismatch for {id}"),
            }
            .build());
        }

        if let Some(status) = update.status {
            record.status = status;
        }
        if let Some(session_id) = update.session_id {
            record.session_id = Some(session_id);
        }
        if let Some(cost) = update.cost_usd {
            record.cost_usd = cost;
        }
        if let Some(turns) = update.num_turns {
            record.num_turns = turns;
        }
        if let Some(ms) = update.duration_ms {
            record.duration_ms = ms;
        }
        if let Some(url) = update.pr_url {
            record.pr_url = Some(url);
        }
        if let Some(err) = update.error {
            record.error = Some(err);
        }
        record.updated_at = jiff::Timestamp::now();

        let value = serialize_msgpack(&record, "updated session")?;
        let index_value = serialize_msgpack(&record, "updated session reverse index")?;
        let key = schema::session_key(&record.dispatch_id, record.prompt_number);

        self.keyspace
            .insert(key.as_bytes(), value)
            .map_err(|e| store_err("update session", e))?;
        self.keyspace
            .insert(index_key.as_bytes(), index_value)
            .map_err(|e| store_err("update session reverse index", e))?;
        self.ensure_durable()?;

        Ok(())
    }

    /// List all sessions belonging to a dispatch, ordered by prompt number.
    ///
    /// # Errors
    ///
    /// Returns `Error::Store` on read failure.
    pub(crate) fn list_sessions_for_dispatch(
        &self,
        dispatch_id: &DispatchId,
    ) -> Result<Vec<SessionRecord>> {
        queries::list_sessions_for_dispatch(&self.keyspace, dispatch_id)
    }

    // ── Lesson CRUD ──

    /// Add a new lesson record.
    ///
    /// # Errors
    ///
    /// Returns `Error::Store` on write failure.
    pub fn add_lesson(&self, lesson: &NewLesson) -> Result<()> {
        let now = jiff::Timestamp::now();
        let record = LessonRecord {
            source: lesson.source.clone(),
            category: lesson.category.clone(),
            lesson: lesson.lesson.clone(),
            evidence: lesson.evidence.clone(),
            project: lesson.project.clone(),
            prompt_number: lesson.prompt_number,
            created_at: now,
        };

        let lesson_ulid = koina::ulid::Ulid::new().to_string();
        let key = schema::lesson_key(&lesson.source, now.as_millisecond(), &lesson_ulid);
        let value = serialize_msgpack(&record, "lesson record")?;

        self.keyspace
            .insert(key.as_bytes(), value)
            .map_err(|e| store_err("write lesson", e))?;

        Ok(())
    }

    /// Query lessons with optional filters.
    ///
    /// # Errors
    ///
    /// Returns `Error::Store` on read failure.
    pub fn query_lessons(
        &self,
        source: Option<&str>,
        category: Option<&str>,
        project: Option<&str>,
        limit: usize,
    ) -> Result<Vec<LessonRecord>> {
        queries::query_lessons(&self.keyspace, source, category, project, limit)
    }

    // ── Observation CRUD ──

    /// Add a new observation record.
    ///
    /// # Errors
    ///
    /// Returns `Error::Store` on write failure.
    pub fn add_observation(&self, observation: &NewObservation) -> Result<()> {
        let now = jiff::Timestamp::now();
        let obs_ulid = koina::ulid::Ulid::new().to_string();

        let record = ObservationRecord {
            id: obs_ulid.clone(),
            project: observation.project.clone(),
            source: observation.source.clone(),
            content: observation.content.clone(),
            observation_type: observation.observation_type.clone(),
            session_id: observation.session_id.clone(),
            created_at: now,
        };

        let key = schema::observation_key(now.as_millisecond(), &obs_ulid);
        let value = serialize_msgpack(&record, "observation record")?;

        self.keyspace
            .insert(key.as_bytes(), value)
            .map_err(|e| store_err("write observation", e))?;

        Ok(())
    }

    /// Query observations with optional filters.
    ///
    /// # Errors
    ///
    /// Returns `Error::Store` on read failure.
    pub fn query_observations(
        &self,
        project: Option<&str>,
        days: Option<u32>,
        limit: usize,
    ) -> Result<Vec<ObservationRecord>> {
        queries::query_observations(&self.keyspace, project, days, limit)
    }

    // ── CI Validation ──

    /// Record a CI validation result for a session.
    ///
    /// # Errors
    ///
    /// Returns `Error::Store` on write failure.
    pub fn add_ci_validation(
        &self,
        session_id: &SessionId,
        check_name: &str,
        pr_number: u64,
        status: CiValidationStatus,
        details: Option<String>,
    ) -> Result<()> {
        let record = CiValidationRecord {
            session_id: session_id.clone(),
            check_name: check_name.to_owned(),
            pr_number,
            status,
            details,
            validated_at: jiff::Timestamp::now(),
        };

        let key = schema::ci_validation_key(session_id, check_name);
        let value = serialize_msgpack(&record, "CI validation")?;

        self.keyspace
            .insert(key.as_bytes(), value)
            .map_err(|e| store_err("write CI validation", e))?;

        Ok(())
    }

    /// Record a QA verdict for a dispatch.
    ///
    /// # Errors
    ///
    /// Returns `Error::Store` on write failure.
    pub fn add_qa_verdict(
        &self,
        dispatch_id: &DispatchId,
        project: &str,
        verdict: crate::types::QaVerdict,
    ) -> Result<()> {
        let now = jiff::Timestamp::now();
        let ulid = koina::ulid::Ulid::new().to_string();
        let record = QaVerdictRecord {
            dispatch_id: dispatch_id.clone(),
            project: project.to_owned(),
            verdict,
            recorded_at: now,
        };

        let key = schema::qa_verdict_key(dispatch_id, now.as_millisecond(), &ulid);
        let value = serialize_msgpack(&record, "QA verdict")?;

        self.keyspace
            .insert(key.as_bytes(), value)
            .map_err(|e| store_err("write QA verdict", e))?;

        Ok(())
    }

    // ── Training data integration ──

    /// Extract training signal from a completed session and produce a mneme
    /// `Fact` with the `Training` epistemic tier.
    ///
    /// The returned fact can be persisted through the mneme knowledge pipeline.
    /// The JSONL export becomes a view over mneme facts rather than a primary
    /// store.
    ///
    /// # Errors
    ///
    /// Returns `Error::Serialization` if the outcome cannot be encoded.
    pub fn record_training_data(
        &self,
        session: &SessionRecord,
        outcome: &SessionOutcome,
    ) -> Result<Fact> {
        let outcome_data = SessionOutcomeData {
            prompt_number: outcome.prompt_number,
            status: outcome.status,
            cost_usd: outcome.cost_usd,
            num_turns: outcome.num_turns,
            duration_ms: outcome.duration_ms,
            pr_url: outcome.pr_url.clone(),
            corrective_attempts: outcome.corrective_attempts,
        };

        let content = serde_json::to_string(&outcome_data)
            .map_err(|e| ser_err("serialize training outcome", e))?;

        let fact_id_str = format!("training:{}", session.id.as_str());
        let fact_id = FactId::new(&fact_id_str).map_err(|e| ser_err("invalid fact ID", e))?;

        let now = jiff::Timestamp::now();
        // WHY: far-future sentinel matches eidos convention for open-ended validity.
        let far_future =
            jiff::Timestamp::from_millisecond(253_370_764_800_000).unwrap_or(jiff::Timestamp::MAX);

        let fact = Fact {
            id: fact_id,
            nous_id: String::new(),
            content,
            fact_type: "training".to_owned(),
            scope: None,
            project_id: None,
            temporal: FactTemporal {
                valid_from: now,
                valid_to: far_future,
                recorded_at: now,
            },
            provenance: FactProvenance {
                confidence: 1.0,
                tier: EpistemicTier::Training,
                source_session_id: session.session_id.clone(),
                // WHY: 4 years — training data is a permanent record, not
                // subject to normal FSRS decay.
                stability_hours: 35_040.0,
            },
            lifecycle: FactLifecycle {
                superseded_by: None,
                is_forgotten: false,
                forgotten_at: None,
                forget_reason: None,
            },
            access: FactAccess {
                access_count: 0,
                last_accessed_at: None,
            },
            sensitivity: FactSensitivity::Public,
            visibility: Visibility::Private,
        };

        Ok(fact)
    }

    // ── Startup reconciliation ──

    /// Mark Running dispatch records older than `threshold` as `Failed`.
    ///
    /// Call at startup before accepting new dispatches so interrupted runs from
    /// a previous process do not remain `Running` indefinitely. Returns the
    /// number of records reconciled.
    ///
    /// # Errors
    ///
    /// Returns `Error::Store` on read or write failure.
    pub fn reconcile_stale_running_dispatches(
        &self,
        threshold: jiff::SignedDuration,
    ) -> Result<u32> {
        let cutoff = jiff::Timestamp::now().checked_sub(threshold).map_err(|e| {
            error::StoreSnafu {
                message: format!("duration subtraction in reconcile_stale_running_dispatches: {e}"),
            }
            .build()
        })?;
        let all = queries::list_dispatches(&self.keyspace, SCAN_LIMIT_DISPATCHES)?;
        let stale: Vec<_> = all
            .into_iter()
            .filter(|d| {
                d.status == crate::store::records::DispatchStatus::Running && d.created_at < cutoff
            })
            .collect();
        let count = u32::try_from(stale.len()).unwrap_or(u32::MAX);
        for record in &stale {
            if let Err(e) =
                self.finish_dispatch(&record.id, crate::store::records::DispatchStatus::Failed)
            {
                tracing::warn!(
                    dispatch_id = %record.id,
                    error = %e,
                    "failed to reconcile stale Running dispatch"
                );
            } else {
                tracing::info!(
                    dispatch_id = %record.id,
                    created_at = %record.created_at,
                    "reconciled stale Running dispatch as Failed"
                );
            }
        }
        Ok(count)
    }

    /// Count Running dispatch records older than `threshold` without modifying them.
    ///
    /// # Errors
    ///
    /// Returns `Error::Store` on read failure.
    pub fn stale_running_dispatch_count(&self, threshold: jiff::SignedDuration) -> Result<u32> {
        let cutoff = jiff::Timestamp::now().checked_sub(threshold).map_err(|e| {
            error::StoreSnafu {
                message: format!("duration subtraction in stale_running_dispatch_count: {e}"),
            }
            .build()
        })?;
        let all = queries::list_dispatches(&self.keyspace, SCAN_LIMIT_DISPATCHES)?;
        let count = all
            .into_iter()
            .filter(|d| {
                d.status == crate::store::records::DispatchStatus::Running && d.created_at < cutoff
            })
            .count();
        Ok(u32::try_from(count).unwrap_or(u32::MAX))
    }

    // ── Bulk scan (metrics / reporting) ──

    /// List all dispatch records ordered by ULID (time-ascending), up to `limit`.
    ///
    /// Intended for metrics computation. Use [`SCAN_LIMIT_DISPATCHES`] as a
    /// sensible default to prevent unbounded memory use.
    ///
    /// # Errors
    ///
    /// Returns `Error::Store` on read failure.
    pub(crate) fn list_dispatches(&self, limit: usize) -> Result<Vec<DispatchRecord>> {
        queries::list_dispatches(&self.keyspace, limit)
    }

    /// List newest dispatch records first, up to `limit`.
    ///
    /// # Errors
    ///
    /// Returns `Error::Store` on read failure.
    pub(crate) fn list_recent_dispatches(&self, limit: usize) -> Result<Vec<DispatchRecord>> {
        queries::list_recent_dispatches(&self.keyspace, limit)
    }

    /// List all session records across all dispatches, up to `limit`.
    ///
    /// Ordered approximately by time (dispatch ULID, then prompt number).
    /// Intended for metrics computation. Use [`SCAN_LIMIT_SESSIONS`] as a
    /// sensible default.
    ///
    /// # Errors
    ///
    /// Returns `Error::Store` on read failure.
    pub(crate) fn list_all_sessions(&self, limit: usize) -> Result<Vec<SessionRecord>> {
        queries::list_all_sessions(&self.keyspace, limit)
    }

    /// List all CI validation records across all sessions, up to `limit`.
    ///
    /// Intended for metrics computation. Use [`SCAN_LIMIT_CI_VALIDATIONS`] as a
    /// sensible default.
    ///
    /// # Errors
    ///
    /// Returns `Error::Store` on read failure.
    pub(crate) fn list_all_ci_validations(&self, limit: usize) -> Result<Vec<CiValidationRecord>> {
        queries::list_all_ci_validations(&self.keyspace, limit)
    }

    /// List all QA verdict records, up to `limit`.
    ///
    /// # Errors
    ///
    /// Returns `Error::Store` on read failure.
    pub(crate) fn list_all_qa_verdicts(&self, limit: usize) -> Result<Vec<QaVerdictRecord>> {
        queries::list_all_qa_verdicts(&self.keyspace, limit)
    }

    /// List QA verdict records for a specific dispatch.
    ///
    /// # Errors
    ///
    /// Returns `Error::Store` on read failure.
    pub fn list_qa_verdicts_for_dispatch(
        &self,
        dispatch_id: &DispatchId,
    ) -> Result<Vec<QaVerdictRecord>> {
        queries::list_qa_verdicts_for_dispatch(&self.keyspace, dispatch_id)
    }

    /// List CI validation records for a specific session.
    ///
    /// # Errors
    ///
    /// Returns `Error::Store` on read failure.
    pub fn list_ci_validations_for_session(
        &self,
        session_id: &SessionId,
    ) -> Result<Vec<CiValidationRecord>> {
        queries::list_ci_validations_for_session(&self.keyspace, session_id)
    }

    // ── Internal helpers ──

    /// Back-date a dispatch record's `created_at` by the given duration.
    ///
    /// Used in tests to simulate records created long before `now` so that
    /// stale-dispatch detection thresholds can be exercised without real waits.
    #[cfg(test)]
    pub(crate) fn backdate_dispatch_for_test(
        &self,
        id: &DispatchId,
        amount: jiff::SignedDuration,
    ) -> Result<()> {
        let mut record = self.get_dispatch(id)?.ok_or_else(|| {
            error::NotFoundSnafu {
                what: format!("dispatch {id}"),
            }
            .build()
        })?;
        record.created_at = record.created_at.checked_sub(amount).map_err(|e| {
            error::StoreSnafu {
                message: format!("duration subtraction in backdate_dispatch_for_test: {e}"),
            }
            .build()
        })?;
        let key = schema::dispatch_key(id);
        let value = serialize_msgpack(&record, "backdated dispatch")?;
        self.keyspace
            .insert(key.as_bytes(), value)
            .map_err(|e| store_err("write backdated dispatch", e))?;
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn insert_dispatch_record_for_test(&self, record: &DispatchRecord) -> Result<()> {
        let key = schema::dispatch_key(&record.id);
        let value = serialize_msgpack(record, "test dispatch record")?;
        self.keyspace
            .insert(key.as_bytes(), value)
            .map_err(|e| store_err("write test dispatch", e))?;
        Ok(())
    }
}

impl std::fmt::Debug for EnergeiaStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EnergeiaStore")
            .field("partition", &PARTITION_NAME)
            .finish()
    }
}

#[cfg(test)]
#[path = "fjall_store_tests.rs"]
mod fjall_store_tests;
