//! Atomic commit path for consolidation writes (aletheia#5311).
//!
//! The consolidation engine used to write each consolidated fact, its
//! multiplicity and provenance side-index rows, the supersession updates on
//! the original facts, and the audit row as separate single-statement
//! transactions. A failure part-way down that sequence left active
//! consolidated facts standing next to the still-active originals, or an
//! applied consolidation with no audit row, and a retry minted fresh ULIDs
//! that compounded the inconsistency.
//!
//! The write set now commits as one [`krites::MultiTransaction`](crate::engine::MultiTransaction)
//! — the same all-or-nothing primitive [`persist_extraction_batch`](super::persist_batch)
//! established for extraction writes: the first failing write aborts the
//! whole transaction and nothing from the consolidation lands. Combined with
//! the deterministic IDs the plan carries (`cons-…` fact IDs and the
//! `cons-audit-…` run key derived from the sorted source fact IDs and the
//! consolidation config), a retry after any failure rewrites the same rows
//! rather than minting new ones, and the committed audit row lets a retry
//! short-circuit before the LLM call ever runs again.
//!
//! Admission policy runs against every planned consolidated fact before the
//! transaction opens — under the same `insert_lock` the single-fact
//! [`insert_fact`](KnowledgeStore::insert_fact) path holds — so a rejected
//! output aborts the consolidation with the originals untouched instead of
//! being dropped while its sources are superseded (the data-loss shape
//! #5849 guards against).

use std::collections::BTreeMap;

use tracing::instrument;

use super::{KnowledgeStore, marshal, queries};
use crate::consolidation::{
    ConsolidationAuditRecord, ConsolidationError, ConsolidationProvenanceRow,
    ConsolidationWritePlan, FactMultiplicity, StoreSnafu,
};
use crate::engine::DataValue;
use crate::id::FactId;

/// Datalog: `:put` one `fact_multiplicity` side-index row (#3634).
const FACT_MULTIPLICITY_PUT: &str = r"
?[fact_id, source_count, first_observed, last_observed, time_spread_seconds, recorded_at] <-
    [[$fact_id, $source_count, $first_observed, $last_observed, $time_spread_seconds, $recorded_at]]

:put fact_multiplicity {fact_id => source_count, first_observed, last_observed,
                        time_spread_seconds, recorded_at}
";

/// Datalog: `:put` one `consolidation_provenance` side-index row (#4660).
const CONSOLIDATION_PROVENANCE_PUT: &str = r"
?[consolidated_fact_id, source_fact_ids, source_session_ids] <-
    [[$fact_id, $source_fact_ids, $source_session_ids]]

:put consolidation_provenance {
    consolidated_fact_id => source_fact_ids, source_session_ids
}
";

/// Datalog: mark one original fact superseded by its consolidated successor,
/// setting `valid_to` and `superseded_by` on the existing row.
const SUPERSEDE_FACT_PUT: &str = r"
?[id, valid_from, content, nous_id, confidence, tier, valid_to, superseded_by,
   source_session_id, recorded_at, access_count, last_accessed_at,
   stability_hours, fact_type, is_forgotten, forgotten_at, forget_reason,
   scope, project_id, visibility, sensitivity] :=
    *facts{id, valid_from, content, nous_id, confidence, tier,
           source_session_id, recorded_at, access_count, last_accessed_at,
           stability_hours, fact_type, is_forgotten, forgotten_at, forget_reason,
           scope, project_id, visibility, sensitivity},
    id = $id,
    valid_to = $now,
    superseded_by = $superseding_id

:put facts {id, valid_from => content, nous_id, confidence, tier, valid_to,
            superseded_by, source_session_id, recorded_at, access_count,
            last_accessed_at, stability_hours, fact_type, is_forgotten,
            forgotten_at, forget_reason, scope, project_id, visibility, sensitivity}
";

/// Datalog: `:put` the `consolidation_audit` row recording this run.
const CONSOLIDATION_AUDIT_PUT: &str = r"
?[id, nous_id, trigger_type, trigger_id, original_count, consolidated_count,
   original_fact_ids, consolidated_fact_ids, consolidated_at] <-
    [[$id, $nous_id, $trigger_type, $trigger_id, $original_count, $consolidated_count,
      $original_fact_ids, $consolidated_fact_ids, $consolidated_at]]

:put consolidation_audit {id => nous_id, trigger_type, trigger_id, original_count,
                          consolidated_count, original_fact_ids,
                          consolidated_fact_ids, consolidated_at}
";

/// The ordered write steps of one consolidation commit (#5311).
///
/// Failure-injection tests arm each step in turn to prove the commit is
/// all-or-nothing; the step order matches the write order in
/// [`transact_consolidation_writes`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WriteStep {
    /// Consolidated fact rows (`facts`).
    Fact,
    /// Multiplicity side-index rows (`fact_multiplicity`).
    Multiplicity,
    /// Provenance side-index rows (`consolidation_provenance`).
    Provenance,
    /// Supersession updates on the original `facts` rows.
    Supersede,
    /// The `consolidation_audit` row.
    Audit,
}

fn tx_error(err: &crate::engine::MultiTransactionError) -> ConsolidationError {
    StoreSnafu {
        message: err.to_string(),
    }
    .build()
}

/// Send one `:put` payload into the transaction.
fn tx_put(
    tx: &crate::engine::MultiTransaction,
    script: &str,
    params: BTreeMap<String, DataValue>,
) -> Result<(), ConsolidationError> {
    tx.transact(crate::engine::TransactionPayload::Query((
        script.to_owned(),
        params,
    )))
    .map(|_| ())
    .map_err(|err| tx_error(&err))
}

fn multiplicity_params(record: &FactMultiplicity) -> BTreeMap<String, DataValue> {
    let str_val = |s: &str| DataValue::Str(s.into());
    let mut params = BTreeMap::new();
    params.insert("fact_id".to_owned(), str_val(record.fact_id.as_str()));
    params.insert(
        "source_count".to_owned(),
        DataValue::from(i64::from(record.source_count)),
    );
    params.insert("first_observed".to_owned(), str_val(&record.first_observed));
    params.insert("last_observed".to_owned(), str_val(&record.last_observed));
    params.insert(
        "time_spread_seconds".to_owned(),
        DataValue::from(record.time_spread_seconds),
    );
    params.insert("recorded_at".to_owned(), str_val(&record.recorded_at));
    params
}

fn provenance_params(row: &ConsolidationProvenanceRow) -> BTreeMap<String, DataValue> {
    let mut params = BTreeMap::new();
    params.insert(
        "fact_id".to_owned(),
        DataValue::Str(row.fact_id.as_str().into()),
    );
    params.insert(
        "source_fact_ids".to_owned(),
        DataValue::Str(row.source_fact_ids_json.clone().into()),
    );
    params.insert(
        "source_session_ids".to_owned(),
        DataValue::Str(row.source_session_ids_json.clone().into()),
    );
    params
}

fn supersede_params(
    original_id: &FactId,
    superseding_id: &FactId,
    now: &str,
) -> BTreeMap<String, DataValue> {
    let mut params = BTreeMap::new();
    params.insert("id".to_owned(), DataValue::Str(original_id.as_str().into()));
    params.insert("now".to_owned(), DataValue::Str(now.into()));
    params.insert(
        "superseding_id".to_owned(),
        DataValue::Str(superseding_id.as_str().into()),
    );
    params
}

fn audit_params(record: &ConsolidationAuditRecord) -> BTreeMap<String, DataValue> {
    let mut params = BTreeMap::new();
    params.insert("id".to_owned(), DataValue::Str(record.id.clone().into()));
    params.insert(
        "nous_id".to_owned(),
        DataValue::Str(record.nous_id.clone().into()),
    );
    params.insert(
        "trigger_type".to_owned(),
        DataValue::Str(record.trigger_type.clone().into()),
    );
    params.insert(
        "trigger_id".to_owned(),
        DataValue::Str(record.trigger_id.clone().into()),
    );
    params.insert(
        "original_count".to_owned(),
        DataValue::from(i64::try_from(record.original_count).unwrap_or(i64::MAX)),
    );
    params.insert(
        "consolidated_count".to_owned(),
        DataValue::from(i64::try_from(record.consolidated_count).unwrap_or(i64::MAX)),
    );
    params.insert(
        "original_fact_ids".to_owned(),
        DataValue::Str(record.original_fact_ids.clone().into()),
    );
    params.insert(
        "consolidated_fact_ids".to_owned(),
        DataValue::Str(record.consolidated_fact_ids.clone().into()),
    );
    params.insert(
        "consolidated_at".to_owned(),
        DataValue::Str(record.consolidated_at.clone().into()),
    );
    params
}

/// Drive every staged write through the open transaction, in issue order:
/// facts, side indices (multiplicity, provenance), supersessions, audit.
fn transact_consolidation_writes(
    tx: &crate::engine::MultiTransaction,
    plan: &ConsolidationWritePlan,
) -> Result<(), ConsolidationError> {
    for fact in &plan.facts {
        failpoint_check(WriteStep::Fact)?;
        tx_put(tx, &queries::upsert_fact(), marshal::fact_to_params(fact))?;
    }
    for record in &plan.multiplicities {
        failpoint_check(WriteStep::Multiplicity)?;
        tx_put(tx, FACT_MULTIPLICITY_PUT, multiplicity_params(record))?;
    }
    for row in &plan.provenance {
        failpoint_check(WriteStep::Provenance)?;
        tx_put(tx, CONSOLIDATION_PROVENANCE_PUT, provenance_params(row))?;
    }
    for (original_id, superseding_id) in &plan.supersessions {
        failpoint_check(WriteStep::Supersede)?;
        tx_put(
            tx,
            SUPERSEDE_FACT_PUT,
            supersede_params(original_id, superseding_id, &plan.now),
        )?;
    }
    failpoint_check(WriteStep::Audit)?;
    tx_put(tx, CONSOLIDATION_AUDIT_PUT, audit_params(&plan.audit))?;
    Ok(())
}

/// Validate one planned consolidated fact with the same rules
/// [`insert_fact`](KnowledgeStore::insert_fact) enforces, so a malformed LLM
/// output rejects the whole consolidation before the transaction opens rather
/// than part-way through it.
fn validate_consolidated_fact(fact: &crate::knowledge::Fact) -> Result<(), ConsolidationError> {
    if fact.content.is_empty() {
        return Err(StoreSnafu {
            message: "consolidated fact has empty content".to_owned(),
        }
        .build());
    }
    if fact.content.len() > crate::knowledge::MAX_CONTENT_LENGTH {
        return Err(StoreSnafu {
            message: format!(
                "consolidated fact content too long: {} bytes (max {})",
                fact.content.len(),
                crate::knowledge::MAX_CONTENT_LENGTH
            ),
        }
        .build());
    }
    if !(0.0..=1.0).contains(&fact.provenance.confidence) {
        return Err(StoreSnafu {
            message: format!(
                "consolidated fact confidence out of range: {}",
                fact.provenance.confidence
            ),
        }
        .build());
    }
    Ok(())
}

impl KnowledgeStore {
    /// Commit one consolidation's write plan as a single all-or-nothing
    /// transaction (#5311).
    ///
    /// The plan is expected to be fully validated already — policy metadata
    /// merged, deterministic IDs derived, JSON serialized — so every write
    /// here is expected to succeed; the first one that still fails aborts the
    /// transaction and nothing from the consolidation lands.
    ///
    /// Returns the IDs of the consolidated facts committed.
    ///
    /// # Errors
    ///
    /// Returns an error — with nothing written — if any planned fact fails
    /// validation or admission, if any transacted write fails, or if the
    /// commit itself fails.
    #[instrument(skip(self, plan))]
    pub(crate) fn commit_consolidation(
        &self,
        plan: &ConsolidationWritePlan,
    ) -> Result<Vec<FactId>, ConsolidationError> {
        // The audit-owner backfill runs `::columns` (a sys-op), which a
        // MultiTransaction cannot host — it must settle before the
        // transaction opens. Idempotent.
        self.ensure_consolidation_audit_owner_scope()?;

        // WHY: the admission gate reads store state and then the transaction
        // writes; holding insert_lock across both (the `persist_batch`
        // pattern) keeps a concurrent single-fact insert from interleaving
        // between the check and the commit.
        let _guard = self.insert_lock.lock();
        for fact in &plan.facts {
            validate_consolidated_fact(fact)?;
            if let crate::admission::AdmissionDecision::Reject(rejection) =
                self.admission_policy.should_admit(fact)
            {
                return Err(StoreSnafu {
                    message: format!(
                        "consolidated fact {} rejected by admission policy: {}",
                        fact.id, rejection.reason
                    ),
                }
                .build());
            }
        }

        let tx = self.db.multi_transaction(true);
        if let Err(err) = transact_consolidation_writes(&tx, plan) {
            // WHY: abort is best-effort cleanup after the real error is
            // already in hand — a failed abort (worker gone) changes nothing
            // about what the caller learns, and dropping the handle releases
            // the worker regardless.
            let _ = tx.abort();
            return Err(err);
        }
        tx.commit().map_err(|err| tx_error(&err))?;

        // WHY: metrics and derived-rule invalidation must reflect what
        // actually committed, so they run strictly after commit — the
        // persist_batch precedent. A post-commit invalidation failure leaves
        // derived materializations stale until the next base write; it never
        // leaves the consolidation itself partially applied, and the retry
        // path (idempotency key or the superseded-source filter) converges
        // without rewriting anything.
        for fact in &plan.facts {
            crate::metrics::record_fact_inserted(&fact.nous_id);
        }
        if !plan.facts.is_empty() || !plan.supersessions.is_empty() {
            self.invalidate_derived_facts().map_err(|e| {
                StoreSnafu {
                    message: e.to_string(),
                }
                .build()
            })?;
        }

        Ok(plan.facts.iter().map(|fact| fact.id.clone()).collect())
    }
}

// ---------------------------------------------------------------------
// Failure-injection seam (#5311)
// ---------------------------------------------------------------------

/// Invoke the test failpoint for `step`.
#[cfg(test)]
fn failpoint_check(step: WriteStep) -> Result<(), ConsolidationError> {
    failpoint::check(step)
}

/// Invoke the test failpoint for `step`. Compiled to a no-op outside tests.
#[cfg(not(test))]
#[expect(
    clippy::unnecessary_wraps,
    reason = "the test build's failpoint_check can fail; the signatures must match so call sites compile unchanged"
)]
fn failpoint_check(_step: WriteStep) -> Result<(), ConsolidationError> {
    Ok(())
}

/// Failure-injection seam for the consolidation write sequence (#5311).
///
/// Tests arm a thread-local step; the commit's next write at that step then
/// fails exactly once with a synthetic store error, letting a test prove
/// that no prefix of the write set survives and that a retry converges.
/// Thread-local (not a process-global hook like `crash_injection`) so
/// parallel `cargo test` threads in one binary arm independent failpoints.
///
/// Production builds contain only the no-op [`failpoint_check`] shim below —
/// the seam is `cfg(test)`-only and cannot be armed outside tests.
#[cfg(test)]
pub(crate) mod failpoint {
    use std::cell::Cell;

    use super::{ConsolidationError, StoreSnafu, WriteStep};

    thread_local! {
        static ARMED: Cell<Option<WriteStep>> = const { Cell::new(None) };
    }

    /// Arm the failpoint: this thread's next consolidation write at `step`
    /// fails once, then the failpoint disarms itself.
    pub(crate) fn arm(step: WriteStep) {
        ARMED.with(|armed| armed.set(Some(step)));
    }

    pub(super) fn check(step: WriteStep) -> Result<(), ConsolidationError> {
        ARMED.with(|armed| {
            if armed.get() == Some(step) {
                armed.set(None);
                return Err(StoreSnafu {
                    message: format!("injected consolidation write failure at {step:?}"),
                }
                .build());
            }
            Ok(())
        })
    }
}
