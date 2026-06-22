//! Benchmark memory isolation: per-question disposable namespaces.
//!
//! Closing an HTTP session does not purge typed memory, vector indexes, or
//! extracted facts. The types here give benchmark runners a dedicated store
//! pair for each `(eval_run_id, question_id)` so question *N* can only see the
//! haystack seeded for question *N*.

use std::sync::Arc;

use snafu::ResultExt;

use crate::benchmark::error::BenchmarkError;
use crate::id::{EvalRunId, FactId, QuestionId};
use crate::knowledge::{
    EpistemicTier, Fact, FactAccess, FactLifecycle, FactProvenance, FactSensitivity, FactTemporal,
    MemoryScope, Visibility, default_stability_hours, far_future, format_timestamp,
};
use crate::knowledge_store::KnowledgeStore;
use crate::store::SessionStore;
use crate::types::Session;

/// Execution mode for a benchmark run.
///
/// `OfficialParity` matches the standard evaluation protocol: each question
/// starts from a clean memory state. `ContinuousMemory` deliberately reuses
/// state so callers can measure cross-question contamination separately.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum BenchmarkMode {
    /// Clean-state protocol; each question gets its own isolated namespace.
    OfficialParity,
    /// Shared-state protocol; earlier questions remain reachable.
    ContinuousMemory,
}

/// Scope that identifies a single benchmark question inside an eval run.
///
/// All artifacts created through [`IsolatedMemory`] carry this scope so
/// provenance and leakage checks are straightforward.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvalScope {
    /// Eval-run identifier shared by every question in the run.
    pub eval_run_id: EvalRunId,
    /// Question identifier unique within the eval run.
    pub question_id: QuestionId,
    /// Whether this scope should start from a clean state.
    pub mode: BenchmarkMode,
}

impl EvalScope {
    /// Create a new official-parity scope for `(eval_run_id, question_id)`.
    ///
    /// # Errors
    ///
    /// Returns [`crate::id::IdValidationError`] if either identifier is empty
    /// or exceeds the maximum length.
    pub fn new(
        eval_run_id: impl Into<String>,
        question_id: impl Into<String>,
    ) -> Result<Self, crate::id::IdValidationError> {
        Ok(Self {
            eval_run_id: EvalRunId::new(eval_run_id)?,
            question_id: QuestionId::new(question_id)?,
            mode: BenchmarkMode::OfficialParity,
        })
    }

    /// Override the default execution mode.
    #[must_use]
    pub fn with_mode(mut self, mode: BenchmarkMode) -> Self {
        self.mode = mode;
        self
    }

    /// Stable namespace string derived from the eval scope.
    ///
    /// This string is safe to embed in session keys, fact provenance, and
    /// filesystem paths because both identifiers are already constrained to
    /// domain-safe characters.
    #[must_use]
    pub fn namespace(&self) -> String {
        format!("eval-{}/question-{}", self.eval_run_id, self.question_id)
    }

    /// Nous identifier used for every artifact in this scope.
    ///
    /// WHY: A dedicated `nous_id` per scope prevents cluster expansion or
    /// visibility rules from accidentally bleeding facts across questions,
    /// even if two scopes happen to share the same backing store.
    #[must_use]
    pub fn nous_id(&self) -> String {
        format!("benchmark-{}", self.namespace())
    }

    /// Provenance tag written into fact `source_session_id` fields.
    #[must_use]
    pub fn tag(&self) -> String {
        format!("{}:{}", self.eval_run_id, self.question_id)
    }
}

/// A disposable memory pair for one benchmark question.
///
/// The contained [`KnowledgeStore`] and [`SessionStore`] are opened in memory
/// (or over a temporary directory) and dropped when this value drops. This
/// gives a hard guarantee that later questions cannot read artifacts created
/// for this question.
pub struct IsolatedMemory {
    scope: EvalScope,
    knowledge: Arc<KnowledgeStore>,
    sessions: SessionStore,
}

impl IsolatedMemory {
    /// Open a fresh, empty knowledge and session store pair for `scope`.
    ///
    /// # Errors
    ///
    /// Returns [`BenchmarkError::KnowledgeStore`] or [`BenchmarkError::SessionStore`]
    /// when the underlying in-memory stores cannot be initialized.
    pub fn open(scope: EvalScope) -> Result<Self, BenchmarkError> {
        let knowledge =
            KnowledgeStore::open_mem().context(crate::benchmark::error::KnowledgeStoreSnafu)?;
        let sessions =
            SessionStore::open_in_memory().context(crate::benchmark::error::SessionStoreSnafu)?;
        Ok(Self {
            scope,
            knowledge,
            sessions,
        })
    }

    /// Return the scope that owns this isolated memory pair.
    #[must_use]
    pub fn scope(&self) -> &EvalScope {
        &self.scope
    }

    /// Borrow the isolated knowledge store.
    #[must_use]
    pub fn knowledge_store(&self) -> &Arc<KnowledgeStore> {
        &self.knowledge
    }

    /// Borrow the isolated session store.
    #[must_use]
    pub fn session_store(&self) -> &SessionStore {
        &self.sessions
    }

    /// Create a benchmark session tagged with this scope.
    ///
    /// # Errors
    ///
    /// Returns [`BenchmarkError::CreateSession`] when the session store rejects
    /// the create (for example because `session_key` is already in use for this
    /// scope).
    pub fn create_session(&self, session_key: &str) -> Result<Session, BenchmarkError> {
        let id = format!("{}-{}", self.scope.question_id, session_key);
        let session = self
            .sessions
            .create_session(&id, &self.scope.nous_id(), session_key, None, None)
            .context(crate::benchmark::error::CreateSessionSnafu)?;
        Ok(session)
    }

    /// Insert a seed fact into the isolated knowledge store.
    ///
    /// The fact is tagged with this scope via `nous_id` and
    /// `provenance.source_session_id`, so provenance checks can recover the
    /// eval run and question it belongs to.
    ///
    /// # Errors
    ///
    /// Returns [`BenchmarkError::InvalidFactId`] if `fact_id` fails domain
    /// validation, or [`BenchmarkError::InsertFact`] if the store rejects it.
    pub fn seed_fact(&self, fact_id: &str, content: &str) -> Result<FactId, BenchmarkError> {
        let id = FactId::new(fact_id).map_err(|source| {
            crate::benchmark::error::BenchmarkError::InvalidFactId {
                id: fact_id.to_owned(),
                source,
            }
        })?;
        let fact = build_seed_fact(&self.scope, id, content);
        self.knowledge
            .insert_fact(&fact)
            .context(crate::benchmark::error::InsertFactSnafu)?;
        Ok(fact.id)
    }

    /// Return the number of non-forgotten facts visible to this scope.
    ///
    /// # Errors
    ///
    /// Returns [`BenchmarkError::QueryFacts`] if the visibility query fails.
    pub fn visible_fact_count(&self) -> Result<usize, BenchmarkError> {
        let now = jiff::Timestamp::now();
        let now_str = format_timestamp(&now);
        let facts = self
            .knowledge
            .query_visible_facts(&self.scope.nous_id(), &now_str, 10_000)
            .context(crate::benchmark::error::QueryFactsSnafu)?;
        Ok(facts.len())
    }

    /// Verify that this scope contains exactly `expected` visible facts.
    ///
    /// This is the cleanup/verification step requested by the isolation
    /// acceptance criteria: after each question the runner can assert that the
    /// namespace contains only the haystack it was supposed to contain.
    ///
    /// # Errors
    ///
    /// Returns [`BenchmarkError::QueryFacts`] if the count cannot be read, or
    /// an ad-hoc error if the count does not match.
    pub fn verify_fact_count(&self, expected: usize) -> Result<(), BenchmarkError> {
        let actual = self.visible_fact_count()?;
        if actual != expected {
            return Err(crate::benchmark::error::BenchmarkError::Storage {
                message: format!(
                    "isolation verification failed: expected {expected} facts, found {actual}"
                ),
            });
        }
        Ok(())
    }
}

// WHY: The helper builds a fully populated Fact so callers cannot forget
// required fields such as `valid_to` or `visibility`.
fn build_seed_fact(scope: &EvalScope, id: FactId, content: &str) -> Fact {
    let now = jiff::Timestamp::now();
    Fact {
        id,
        nous_id: scope.nous_id(),
        fact_type: String::from("benchmark"),
        content: content.to_owned(),
        scope: Some(MemoryScope::User),
        project_id: None,
        sensitivity: FactSensitivity::Public,
        visibility: Visibility::Private,
        temporal: FactTemporal {
            valid_from: now,
            valid_to: far_future(),
            recorded_at: now,
        },
        provenance: FactProvenance {
            confidence: 1.0,
            tier: EpistemicTier::Verified,
            source_session_id: Some(scope.tag()),
            stability_hours: default_stability_hours("benchmark"),
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
    }
}

/// Sentinel leakage check: prove that two scopes cannot see each other.
///
/// Seeds `sentinel_content` as a fact in `source` and then asserts that
/// `target` sees zero facts. This is the test harness that the isolation
/// acceptance criteria ask for: question B can only pass if question A leaked,
/// and this helper makes that impossible in official-parity mode.
///
/// # Errors
///
/// Returns an error if seeding or the visibility query fails, or if `target`
/// unexpectedly observes a fact.
pub fn assert_scope_isolation(
    source: &IsolatedMemory,
    target: &IsolatedMemory,
    sentinel_content: &str,
) -> Result<(), BenchmarkError> {
    source.seed_fact("sentinel-fact", sentinel_content)?;
    source.verify_fact_count(1)?;

    let leaked = target.visible_fact_count()?;
    if leaked != 0 {
        return Err(crate::benchmark::error::BenchmarkError::Storage {
            message: format!(
                "memory leaked between scopes: target saw {leaked} fact(s) from source"
            ),
        });
    }
    Ok(())
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use super::*;

    #[test]
    fn isolated_scopes_have_distinct_nous_ids() {
        let a = EvalScope::new("run-1", "q-1").expect("valid scope");
        let b = EvalScope::new("run-1", "q-2").expect("valid scope");
        assert_ne!(a.nous_id(), b.nous_id());
        assert!(a.nous_id().contains("run-1"));
        assert!(a.nous_id().contains("q-1"));
    }

    // kanon:ignore TESTING/tautological-test — WHY: verifies the core property
    // required by the issue: separate scopes do not share durable memory.
    #[test]
    fn sentinel_fact_does_not_leak_between_isolated_scopes() {
        let a = IsolatedMemory::open(EvalScope::new("run-1", "q-a").expect("valid scope"))
            .expect("open scope a");
        let b = IsolatedMemory::open(EvalScope::new("run-1", "q-b").expect("valid scope"))
            .expect("open scope b");
        assert_scope_isolation(&a, &b, "sentinel content only a should see")
            .expect("isolation must hold");
    }

    #[test]
    fn seed_fact_increases_visible_count() {
        let scope = IsolatedMemory::open(EvalScope::new("run-1", "q-1").expect("valid scope"))
            .expect("open scope");
        assert_eq!(scope.visible_fact_count().expect("count"), 0);
        scope
            .seed_fact("fact-1", "seeded haystack fact")
            .expect("seed fact");
        assert_eq!(scope.visible_fact_count().expect("count"), 1);
        scope.verify_fact_count(1).expect("verify count");
    }
}
