//! `EnergeiaStore` — fjall-backed state persistence for dispatch orchestration.

use std::sync::Arc;

use aletheia_eidos::id::FactId;
use aletheia_eidos::knowledge::{
    EpistemicTier, Fact, FactAccess, FactLifecycle, FactProvenance, FactTemporal,
};

use crate::error::{self, Result};
use crate::store::queries;
use crate::store::records::{
    CiValidationRecord, CiValidationStatus, DispatchId, DispatchRecord, DispatchStatus,
    LessonRecord, NewLesson, NewObservation, ObservationRecord, SessionId, SessionOutcomeData,
    SessionRecord, SessionUpdate,
};

/// Maximum records returned by bulk-scan methods to prevent unbounded memory use.
pub(crate) const SCAN_LIMIT_DISPATCHES: usize = 10_000;
pub(crate) const SCAN_LIMIT_SESSIONS: usize = 100_000;
pub(crate) const SCAN_LIMIT_CI_VALIDATIONS: usize = 200_000;
use crate::store::schema;
use crate::types::{DispatchSpec, SessionOutcome, SessionStatus};

/// Partition name for energeia state within the shared fjall database.
const PARTITION_NAME: &str = "energeia";

// ---------------------------------------------------------------------------
// Error helpers — keep .map_err() calls terse
// ---------------------------------------------------------------------------

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

/// State persistence layer wrapping a fjall keyspace.
///
/// All dispatch, session, lesson, observation, and CI validation records are
/// stored in a dedicated `"energeia"` partition with byte-prefixed keys for
/// efficient prefix scans.
pub struct EnergeiaStore {
    keyspace: Arc<fjall::Keyspace>,
}

impl EnergeiaStore {
    /// Create a new store backed by a dedicated partition in the given database.
    ///
    /// # Errors
    ///
    /// Returns `Error::Store` if the partition cannot be opened.
    #[must_use]
    pub fn new(db: &fjall::Database) -> Result<Self> {
        let keyspace = db
            .keyspace(PARTITION_NAME, fjall::KeyspaceCreateOptions::default)
            .map_err(|e| store_err("open partition", e))?;
        Ok(Self {
            keyspace: Arc::new(keyspace),
        })
    }

    /// Create a store from an already-opened keyspace.
    ///
    /// Use this when the caller manages partition lifecycle (e.g., in tests).
    #[must_use]
    pub fn from_keyspace(keyspace: Arc<fjall::Keyspace>) -> Self {
        Self { keyspace }
    }

    /// The underlying keyspace name.
    #[must_use]
    pub fn partition_name() -> &'static str {
        PARTITION_NAME
    }

    // -----------------------------------------------------------------------
    // Dispatch CRUD
    // -----------------------------------------------------------------------

    /// Create a new dispatch record. Returns the generated `DispatchId`.
    ///
    /// # Errors
    ///
    /// Returns `Error::Store` on write failure, `Error::Serialization` on
    /// encoding failure.
    #[must_use]
    pub fn create_dispatch(&self, project: &str, spec: &DispatchSpec) -> Result<DispatchId> {
        let id = DispatchId::new(aletheia_koina::ulid::Ulid::new().to_string());
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

        Ok(id)
    }

    /// Mark a dispatch as finished with the given status. Aggregates session
    /// costs and counts.
    ///
    /// # Errors
    ///
    /// Returns `Error::NotFound` if the dispatch does not exist.
    #[must_use]
    pub fn finish_dispatch(&self, id: &DispatchId, status: DispatchStatus) -> Result<()> {
        let mut record = self.get_dispatch(id)?.ok_or_else(|| {
            error::NotFoundSnafu {
                what: format!("dispatch {id}"),
            }
            .build()
        })?;

        let sessions = self.list_sessions_for_dispatch(id)?;
        let total_cost: f64 = sessions.iter().map(|s| s.cost_usd).sum();
        #[expect(
            clippy::cast_possible_truncation,
            clippy::as_conversions,
            reason = "session count is bounded by prompt count, always fits u32"
        )]
        let total_sessions = sessions.len() as u32;

        record.status = status;
        record.finished_at = Some(jiff::Timestamp::now());
        record.total_cost_usd = total_cost;
        record.total_sessions = total_sessions;

        let key = schema::dispatch_key(id);
        let value = serialize_msgpack(&record, "updated dispatch")?;

        self.keyspace
            .insert(key.as_bytes(), value)
            .map_err(|e| store_err("update dispatch", e))?;

        Ok(())
    }

    /// Retrieve a dispatch record by ID.
    ///
    /// # Errors
    ///
    /// Returns `Error::Store` on read failure.
    #[must_use]
    pub fn get_dispatch(&self, id: &DispatchId) -> Result<Option<DispatchRecord>> {
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

    // -----------------------------------------------------------------------
    // Session CRUD
    // -----------------------------------------------------------------------

    /// Create a new session record within a dispatch.
    ///
    /// # Errors
    ///
    /// Returns `Error::Store` on write failure.
    #[must_use]
    pub fn create_session(
        &self,
        dispatch_id: &DispatchId,
        prompt_number: u32,
    ) -> Result<SessionId> {
        let id = SessionId::new(aletheia_koina::ulid::Ulid::new().to_string());
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

        Ok(id)
    }

    /// Apply a partial update to an existing session record.
    ///
    /// # Errors
    ///
    /// Returns `Error::NotFound` if the session does not exist.
    #[must_use]
    pub fn update_session(&self, id: &SessionId, update: SessionUpdate) -> Result<()> {
        // WHY: session keys are indexed by (dispatch_id, prompt_number), so we
        // need to scan to find the record by SessionId. For the expected
        // cardinality (<100 sessions per dispatch), this is acceptable.
        let (key_str, mut record) = self.find_session_by_id(id)?;

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

        self.keyspace
            .insert(key_str.as_bytes(), value)
            .map_err(|e| store_err("update session", e))?;

        Ok(())
    }

    /// List all sessions belonging to a dispatch, ordered by prompt number.
    ///
    /// # Errors
    ///
    /// Returns `Error::Store` on read failure.
    #[must_use]
    pub fn list_sessions_for_dispatch(
        &self,
        dispatch_id: &DispatchId,
    ) -> Result<Vec<SessionRecord>> {
        queries::list_sessions_for_dispatch(&self.keyspace, dispatch_id)
    }

    // -----------------------------------------------------------------------
    // Lesson CRUD
    // -----------------------------------------------------------------------

    /// Add a new lesson record.
    ///
    /// # Errors
    ///
    /// Returns `Error::Store` on write failure.
    #[must_use]
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

        let lesson_ulid = aletheia_koina::ulid::Ulid::new().to_string();
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

    // -----------------------------------------------------------------------
    // Observation CRUD
    // -----------------------------------------------------------------------

    /// Add a new observation record.
    ///
    /// # Errors
    ///
    /// Returns `Error::Store` on write failure.
    #[must_use]
    pub fn add_observation(&self, observation: &NewObservation) -> Result<()> {
        let now = jiff::Timestamp::now();
        let obs_ulid = aletheia_koina::ulid::Ulid::new().to_string();

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

    // -----------------------------------------------------------------------
    // CI Validation
    // -----------------------------------------------------------------------

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

    // -----------------------------------------------------------------------
    // Training data integration
    // -----------------------------------------------------------------------

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
        };

        Ok(fact)
    }

    // -----------------------------------------------------------------------
    // Bulk scan (metrics / reporting)
    // -----------------------------------------------------------------------

    /// List all dispatch records ordered by ULID (time-ascending), up to `limit`.
    ///
    /// Intended for metrics computation. Use [`SCAN_LIMIT_DISPATCHES`] as a
    /// sensible default to prevent unbounded memory use.
    ///
    /// # Errors
    ///
    /// Returns `Error::Store` on read failure.
    #[must_use]
    pub fn list_dispatches(&self, limit: usize) -> Result<Vec<DispatchRecord>> {
        queries::list_dispatches(&self.keyspace, limit)
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
    #[must_use]
    pub fn list_all_sessions(&self, limit: usize) -> Result<Vec<SessionRecord>> {
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
    #[must_use]
    pub fn list_all_ci_validations(&self, limit: usize) -> Result<Vec<CiValidationRecord>> {
        queries::list_all_ci_validations(&self.keyspace, limit)
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

    // -----------------------------------------------------------------------
    // Internal helpers
    // -----------------------------------------------------------------------

    /// Find a session record by its `SessionId` via prefix scan over all sessions.
    fn find_session_by_id(&self, id: &SessionId) -> Result<(String, SessionRecord)> {
        let prefix_bytes = schema::session_prefix().as_bytes();
        for guard in self.keyspace.prefix(prefix_bytes) {
            let (key, value) = guard
                .into_inner()
                .map_err(|e| store_err("session scan", e))?;
            let record = queries::deserialize_value::<SessionRecord>(&value)?;
            if record.id == *id {
                let key_str = String::from_utf8_lossy(&key).into_owned();
                return Ok((key_str, record));
            }
        }
        Err(error::NotFoundSnafu {
            what: format!("session {id}"),
        }
        .build())
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
#[expect(clippy::unwrap_used, reason = "test assertions")]
#[expect(
    clippy::indexing_slicing,
    reason = "test assertions on known-length collections"
)]
#[expect(clippy::float_cmp, reason = "test assertions on exact float values")]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn setup_test_store() -> (TempDir, EnergeiaStore) {
        let temp_dir = TempDir::new().unwrap();
        let db = fjall::Database::builder(temp_dir.path()).open().unwrap();
        let store = EnergeiaStore::new(&db).unwrap();
        (temp_dir, store)
    }

    fn sample_dispatch_spec() -> DispatchSpec {
        DispatchSpec {
            prompt_numbers: vec![1, 2, 3],
            project: "acme".to_owned(),
            dag_ref: None,
            max_parallel: Some(2),
        }
    }

    // -----------------------------------------------------------------------
    // Dispatch tests
    // -----------------------------------------------------------------------

    #[test]
    fn create_and_get_dispatch() {
        let (_dir, store) = setup_test_store();
        let spec = sample_dispatch_spec();

        let id = store.create_dispatch("acme", &spec).unwrap();
        let record = store.get_dispatch(&id).unwrap().unwrap();

        assert_eq!(record.project, "acme");
        assert_eq!(record.status, DispatchStatus::Running);
        assert_eq!(record.total_cost_usd, 0.0);
        assert!(record.finished_at.is_none());
    }

    #[test]
    fn get_nonexistent_dispatch_returns_none() {
        let (_dir, store) = setup_test_store();
        let id = DispatchId::new("01NONEXISTENT");
        assert!(store.get_dispatch(&id).unwrap().is_none());
    }

    #[test]
    fn finish_dispatch_aggregates_sessions() {
        let (_dir, store) = setup_test_store();
        let spec = sample_dispatch_spec();
        let dispatch_id = store.create_dispatch("acme", &spec).unwrap();

        let sess1 = store.create_session(&dispatch_id, 1).unwrap();
        let sess2 = store.create_session(&dispatch_id, 2).unwrap();

        store
            .update_session(
                &sess1,
                SessionUpdate {
                    status: Some(SessionStatus::Success),
                    cost_usd: Some(1.50),
                    num_turns: Some(10),
                    ..Default::default()
                },
            )
            .unwrap();
        store
            .update_session(
                &sess2,
                SessionUpdate {
                    status: Some(SessionStatus::Success),
                    cost_usd: Some(2.25),
                    num_turns: Some(8),
                    ..Default::default()
                },
            )
            .unwrap();

        store
            .finish_dispatch(&dispatch_id, DispatchStatus::Completed)
            .unwrap();

        let record = store.get_dispatch(&dispatch_id).unwrap().unwrap();
        assert_eq!(record.status, DispatchStatus::Completed);
        assert!(record.finished_at.is_some());
        assert!((record.total_cost_usd - 3.75).abs() < 0.01);
        assert_eq!(record.total_sessions, 2);
    }

    #[test]
    fn finish_nonexistent_dispatch_returns_not_found() {
        let (_dir, store) = setup_test_store();
        let id = DispatchId::new("01NONEXISTENT");
        let result = store.finish_dispatch(&id, DispatchStatus::Failed);
        assert!(result.is_err());
    }

    // -----------------------------------------------------------------------
    // Session tests
    // -----------------------------------------------------------------------

    #[test]
    fn create_and_list_sessions() {
        let (_dir, store) = setup_test_store();
        let spec = sample_dispatch_spec();
        let dispatch_id = store.create_dispatch("acme", &spec).unwrap();

        store.create_session(&dispatch_id, 1).unwrap();
        store.create_session(&dispatch_id, 2).unwrap();
        store.create_session(&dispatch_id, 3).unwrap();

        let sessions = store.list_sessions_for_dispatch(&dispatch_id).unwrap();
        assert_eq!(sessions.len(), 3);
        assert_eq!(sessions[0].prompt_number, 1);
        assert_eq!(sessions[1].prompt_number, 2);
        assert_eq!(sessions[2].prompt_number, 3);
    }

    #[test]
    fn sessions_isolated_between_dispatches() {
        let (_dir, store) = setup_test_store();
        let spec = sample_dispatch_spec();

        let d1 = store.create_dispatch("project-a", &spec).unwrap();
        let d2 = store.create_dispatch("project-b", &spec).unwrap();

        store.create_session(&d1, 1).unwrap();
        store.create_session(&d1, 2).unwrap();
        store.create_session(&d2, 1).unwrap();

        assert_eq!(store.list_sessions_for_dispatch(&d1).unwrap().len(), 2);
        assert_eq!(store.list_sessions_for_dispatch(&d2).unwrap().len(), 1);
    }

    #[test]
    fn update_session_partial() {
        let (_dir, store) = setup_test_store();
        let spec = sample_dispatch_spec();
        let dispatch_id = store.create_dispatch("acme", &spec).unwrap();
        let session_id = store.create_session(&dispatch_id, 1).unwrap();

        store
            .update_session(
                &session_id,
                SessionUpdate {
                    status: Some(SessionStatus::Success),
                    cost_usd: Some(0.42),
                    pr_url: Some("https://github.com/acme/repo/pull/7".to_owned()),
                    ..Default::default()
                },
            )
            .unwrap();

        let sessions = store.list_sessions_for_dispatch(&dispatch_id).unwrap();
        let session = &sessions[0];
        assert_eq!(session.status, SessionStatus::Success);
        assert_eq!(session.cost_usd, 0.42);
        assert_eq!(
            session.pr_url.as_deref(),
            Some("https://github.com/acme/repo/pull/7")
        );
        assert_eq!(session.num_turns, 0);
    }

    #[test]
    fn update_nonexistent_session_returns_not_found() {
        let (_dir, store) = setup_test_store();
        let id = SessionId::new("01NONEXISTENT");
        let result = store.update_session(&id, SessionUpdate::default());
        assert!(result.is_err());
    }

    // -----------------------------------------------------------------------
    // Lesson tests
    // -----------------------------------------------------------------------

    #[test]
    fn add_and_query_lessons() {
        let (_dir, store) = setup_test_store();

        store
            .add_lesson(&NewLesson {
                source: "steward".to_owned(),
                category: "testing".to_owned(),
                lesson: "Always check clippy".to_owned(),
                evidence: None,
                project: Some("acme".to_owned()),
                prompt_number: Some(1),
            })
            .unwrap();

        store
            .add_lesson(&NewLesson {
                source: "qa".to_owned(),
                category: "style".to_owned(),
                lesson: "Use snafu not thiserror".to_owned(),
                evidence: Some("RUST.md".to_owned()),
                project: Some("acme".to_owned()),
                prompt_number: None,
            })
            .unwrap();

        let all = store.query_lessons(None, None, None, 100).unwrap();
        assert_eq!(all.len(), 2);

        let by_source = store
            .query_lessons(Some("steward"), None, None, 100)
            .unwrap();
        assert_eq!(by_source.len(), 1);
        assert_eq!(by_source[0].lesson, "Always check clippy");

        let by_category = store.query_lessons(None, Some("style"), None, 100).unwrap();
        assert_eq!(by_category.len(), 1);

        let by_project = store.query_lessons(None, None, Some("acme"), 100).unwrap();
        assert_eq!(by_project.len(), 2);
    }

    #[test]
    fn query_lessons_respects_limit() {
        let (_dir, store) = setup_test_store();
        for i in 0..5 {
            store
                .add_lesson(&NewLesson {
                    source: "steward".to_owned(),
                    category: "testing".to_owned(),
                    lesson: format!("Lesson {i}"),
                    evidence: None,
                    project: None,
                    prompt_number: None,
                })
                .unwrap();
        }
        let results = store.query_lessons(None, None, None, 3).unwrap();
        assert_eq!(results.len(), 3);
    }

    // -----------------------------------------------------------------------
    // Observation tests
    // -----------------------------------------------------------------------

    #[test]
    fn add_and_query_observations() {
        let (_dir, store) = setup_test_store();

        store
            .add_observation(&NewObservation {
                project: "acme".to_owned(),
                source: "qa".to_owned(),
                content: "Flaky test in auth module".to_owned(),
                observation_type: "bug".to_owned(),
                session_id: None,
            })
            .unwrap();

        store
            .add_observation(&NewObservation {
                project: "other".to_owned(),
                source: "steward".to_owned(),
                content: "Missing docs".to_owned(),
                observation_type: "doc_gap".to_owned(),
                session_id: None,
            })
            .unwrap();

        let all = store.query_observations(None, None, 100).unwrap();
        assert_eq!(all.len(), 2);

        let acme_only = store.query_observations(Some("acme"), None, 100).unwrap();
        assert_eq!(acme_only.len(), 1);
        assert_eq!(acme_only[0].content, "Flaky test in auth module");
    }

    #[test]
    fn query_observations_respects_limit() {
        let (_dir, store) = setup_test_store();
        for i in 0..5 {
            store
                .add_observation(&NewObservation {
                    project: "acme".to_owned(),
                    source: "qa".to_owned(),
                    content: format!("Observation {i}"),
                    observation_type: "idea".to_owned(),
                    session_id: None,
                })
                .unwrap();
        }
        let results = store.query_observations(None, None, 3).unwrap();
        assert_eq!(results.len(), 3);
    }

    // -----------------------------------------------------------------------
    // CI Validation tests
    // -----------------------------------------------------------------------

    #[test]
    fn add_and_list_ci_validations() {
        let (_dir, store) = setup_test_store();
        let spec = sample_dispatch_spec();
        let dispatch_id = store.create_dispatch("acme", &spec).unwrap();
        let session_id = store.create_session(&dispatch_id, 1).unwrap();

        store
            .add_ci_validation(&session_id, "clippy", 42, CiValidationStatus::Pass, None)
            .unwrap();

        store
            .add_ci_validation(
                &session_id,
                "tests",
                42,
                CiValidationStatus::Fail,
                Some("3 tests failed".to_owned()),
            )
            .unwrap();

        let validations =
            queries::list_ci_validations_for_session(&store.keyspace, &session_id).unwrap();
        assert_eq!(validations.len(), 2);
    }

    // -----------------------------------------------------------------------
    // Training data tests
    // -----------------------------------------------------------------------

    #[test]
    fn record_training_data_produces_fact() {
        let (_dir, store) = setup_test_store();
        let spec = sample_dispatch_spec();
        let dispatch_id = store.create_dispatch("acme", &spec).unwrap();
        let session_id = store.create_session(&dispatch_id, 1).unwrap();

        let sessions = store.list_sessions_for_dispatch(&dispatch_id).unwrap();
        let session = &sessions[0];

        let outcome = SessionOutcome {
            prompt_number: 1,
            status: SessionStatus::Success,
            session_id: Some("cc-sess-abc".to_owned()),
            cost_usd: 0.42,
            num_turns: 15,
            duration_ms: 30_000,
            resume_count: 0,
            pr_url: Some("https://github.com/acme/repo/pull/42".to_owned()),
            error: None,
            model: Some("claude-3-5-sonnet".to_owned()),
            blast_radius: vec!["crates/test/".to_owned()],
        };

        let fact = store.record_training_data(session, &outcome).unwrap();

        assert_eq!(fact.fact_type, "training");
        assert_eq!(fact.provenance.tier, EpistemicTier::Training);
        assert_eq!(fact.provenance.confidence, 1.0);
        assert!(fact.id.as_str().starts_with("training:"));
        assert_eq!(
            fact.id.as_str(),
            format!("training:{}", session_id.as_str())
        );

        let data: SessionOutcomeData = serde_json::from_str(&fact.content).unwrap();
        assert_eq!(data.prompt_number, 1);
        assert_eq!(data.cost_usd, 0.42);
    }

    // -----------------------------------------------------------------------
    // Store construction tests
    // -----------------------------------------------------------------------

    #[test]
    fn from_keyspace_works() {
        let temp_dir = TempDir::new().unwrap();
        let db = fjall::Database::builder(temp_dir.path()).open().unwrap();
        let ks = db
            .keyspace("energeia", fjall::KeyspaceCreateOptions::default)
            .unwrap();
        let store = EnergeiaStore::from_keyspace(Arc::new(ks));

        let spec = sample_dispatch_spec();
        let id = store.create_dispatch("acme", &spec).unwrap();
        assert!(store.get_dispatch(&id).unwrap().is_some());
    }

    #[test]
    fn debug_format() {
        let temp_dir = TempDir::new().unwrap();
        let db = fjall::Database::builder(temp_dir.path()).open().unwrap();
        let store = EnergeiaStore::new(&db).unwrap();
        let debug = format!("{store:?}");
        assert!(debug.contains("energeia"));
    }

    #[test]
    fn store_is_send_sync() {
        const _: fn() = || {
            fn assert<T: Send + Sync>() {}
            assert::<EnergeiaStore>();
        };
    }
}
