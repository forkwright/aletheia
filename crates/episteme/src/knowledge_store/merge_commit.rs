//! Atomic commit for entity-merge writes (aletheia#7289).
//!
//! [`KnowledgeStore::execute_merge`] redirects every relationship and
//! `fact_entities` mapping from the merged entity onto the canonical one,
//! adds the merged name as an alias, deletes the merged entity, and records
//! an audit row. Before this module, each of those was its own independent
//! single-statement write (`run_mut`/`run_read` in a loop): a crash, or any
//! one write failing partway through the sequence, left the graph checker's
//! two consistency signals genuinely wrong rather than merely stale —
//!
//! - relationships and `fact_entities` rows already redirected off the
//!   merged entity, but the entity row itself still present because
//!   `delete_entity` never ran or failed, reads as a real orphaned entity
//!   (`GET /api/v1/knowledge/check`'s `orphaned_entity_count`); and
//! - the merged entity deleted before every relationship referencing it was
//!   redirected reads as a real dangling edge (`dangling_edge_count`).
//!
//! Both surface to the operator as the Memory tab's "Inconsistency
//! detected" badge — a true reading of a structurally broken graph, not a
//! false positive, but one this endpoint exists to prevent by keeping the
//! graph itself consistent.
//!
//! The write set now commits as one [`krites::MultiTransaction`](crate::engine::MultiTransaction)
//! — the same all-or-nothing primitive
//! [`commit_consolidation`](super::consolidation_commit) established for
//! consolidation writes (#5311): the first failing write aborts the whole
//! transaction and nothing from the merge lands, so a retry sees the
//! pre-merge state exactly, never a half-redirected graph.

use std::collections::BTreeMap;

use tracing::instrument;

use super::marshal::{extract_float, extract_str};
use super::{KnowledgeStore, queries};
use crate::engine::{
    Array1, DataValue, MultiTransaction, MultiTransactionError, TransactionPayload, Vector,
};
use crate::error::EngineQuerySnafu;
use crate::id::EntityId;

/// The ordered write steps of one entity-merge commit (#7289).
///
/// Failure-injection tests arm each step in turn to prove the commit is
/// all-or-nothing; the step order matches the write order in
/// [`transact_merge_writes`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MergeWriteStep {
    /// Relationship redirects (`relationships`, src or dst side).
    Relationship,
    /// `fact_entities` transfers.
    FactEntity,
    /// The canonical entity's alias/name-embedding upsert.
    Alias,
    /// Removal of the merged entity row.
    DeleteEntity,
    /// The `merge_audit` row.
    Audit,
}

fn tx_error(err: &MultiTransactionError) -> crate::error::Error {
    EngineQuerySnafu {
        message: err.to_string(),
    }
    .build()
}

/// Send one payload into the transaction, discarding the returned rows.
fn tx_run(
    tx: &MultiTransaction,
    script: &str,
    params: BTreeMap<String, DataValue>,
) -> crate::error::Result<()> {
    tx.transact(TransactionPayload::Query((script.to_owned(), params)))
        .map(|_| ())
        .map_err(|err| tx_error(&err))
}

/// One relationship row read before the transaction opens, resolved to
/// either "redirect onto the canonical entity" or "drop — redirecting
/// would create a self-loop".
struct RelationshipRedirect {
    src: String,
    dst: String,
    relation: String,
    weight: f64,
    created_at: String,
    /// The endpoint that currently reads `from_id` and must become `to_id`.
    redirect_src: bool,
}

impl RelationshipRedirect {
    fn is_self_loop_after_redirect(&self, to_id: &str) -> bool {
        if self.redirect_src {
            self.dst == to_id
        } else {
            self.src == to_id
        }
    }
}

/// Read every relationship where `from_id` is the source or the
/// destination, before the transaction opens (mirrors the read-then-write
/// staging [`super::consolidation_commit`] already established).
///
/// A relationship where `from_id` is *both* endpoints (a self-loop on the
/// entity being merged) satisfies both queries below and would otherwise be
/// staged twice — once per `redirect_src` value — planning two conflicting
/// rewrites of the same row and double-counting it. Keyed on `(src, dst)`
/// so that row is staged exactly once; [`transact_merge_writes`] drops it
/// entirely rather than redirecting only one endpoint (see
/// `RelationshipRedirect::src == RelationshipRedirect::dst` there).
fn plan_relationship_redirects(
    store: &KnowledgeStore,
    from_id: &EntityId,
) -> crate::error::Result<Vec<RelationshipRedirect>> {
    let mut plan: BTreeMap<(String, String), RelationshipRedirect> = BTreeMap::new();
    for (script, redirect_src) in [
        (
            r"?[src, dst, relation, weight, created_at] :=
                *relationships{src, dst, relation, weight, created_at},
                src = $from_id",
            true,
        ),
        (
            r"?[src, dst, relation, weight, created_at] :=
                *relationships{src, dst, relation, weight, created_at},
                dst = $from_id",
            false,
        ),
    ] {
        let mut params = BTreeMap::new();
        params.insert(
            "from_id".to_owned(),
            DataValue::Str(from_id.as_str().into()),
        );
        let rows = store.run_read(script, params)?;
        for row in &rows.rows {
            let [src_v, dst_v, relation_v, weight_v, created_at_v, ..] = row.as_slice() else {
                continue;
            };
            let src = extract_str(src_v)?;
            let dst = extract_str(dst_v)?;
            plan.entry((src.clone(), dst.clone()))
                .or_insert(RelationshipRedirect {
                    src,
                    dst,
                    relation: extract_str(relation_v)?,
                    weight: extract_float(weight_v)?,
                    created_at: extract_str(created_at_v)?,
                    redirect_src,
                });
        }
    }
    Ok(plan.into_values().collect())
}

/// One `fact_entities` row read before the transaction opens.
struct FactEntityTransfer {
    fact_id: String,
    created_at: String,
}

/// Read every `fact_entities` row for `from_id` before the transaction
/// opens.
fn plan_fact_entity_transfers(
    store: &KnowledgeStore,
    from_id: &EntityId,
) -> crate::error::Result<Vec<FactEntityTransfer>> {
    let mut params = BTreeMap::new();
    params.insert(
        "from_id".to_owned(),
        DataValue::Str(from_id.as_str().into()),
    );
    let script = r"?[fact_id, entity_id, created_at] :=
        *fact_entities{fact_id, entity_id, created_at},
        entity_id = $from_id";
    let rows = store.run_read(script, params)?;
    let mut plan = Vec::with_capacity(rows.rows.len());
    for row in &rows.rows {
        let [fact_id_v, _entity_id_v, created_at_v, ..] = row.as_slice() else {
            continue;
        };
        plan.push(FactEntityTransfer {
            fact_id: extract_str(fact_id_v)?,
            created_at: extract_str(created_at_v)?,
        });
    }
    Ok(plan)
}

/// The canonical entity's alias upsert, computed before the transaction
/// opens. `None` when the merged name is already an alias (or the name
/// itself) of the canonical entity — nothing to write.
struct AliasUpsert {
    aliases_str: String,
    name: String,
    entity_type: String,
    created_at: String,
    embedding: Option<Vec<f32>>,
}

fn plan_alias_upsert(
    store: &KnowledgeStore,
    canonical_id: &EntityId,
    merged_name: &str,
) -> crate::error::Result<Option<AliasUpsert>> {
    let entity = store.load_entity(canonical_id)?;
    let lower_new = merged_name.to_lowercase();
    if entity.name.to_lowercase() == lower_new
        || entity.aliases.iter().any(|a| a.to_lowercase() == lower_new)
    {
        return Ok(None);
    }

    let mut aliases = entity.aliases;
    aliases.push(merged_name.to_owned());
    // WHY (#4165 Path A): preserve the existing embedding column value so
    // the alias update does not silently clear a populated embedding.
    let embedding = store.get_entity_name_embedding(canonical_id)?;

    Ok(Some(AliasUpsert {
        aliases_str: aliases.join(","),
        name: entity.name,
        entity_type: entity.entity_type,
        created_at: crate::knowledge::format_timestamp(&entity.created_at),
        embedding,
    }))
}

/// Every write staged for one entity merge, computed before the transaction
/// opens. Bundles [`transact_merge_writes`]'s inputs the same way
/// `ConsolidationWritePlan` bundles
/// [`transact_consolidation_writes`](super::consolidation_commit)'s.
struct MergeWritePlan<'a> {
    canonical_id: &'a EntityId,
    merged_id: &'a EntityId,
    merged_name: &'a str,
    relationships: &'a [RelationshipRedirect],
    fact_entities: &'a [FactEntityTransfer],
    alias: Option<&'a AliasUpsert>,
    now_str: &'a str,
    facts_transferred: u32,
    relationships_redirected: u32,
}

/// Redirect every staged relationship edge onto `canonical_id` (or drop it,
/// per [`RelationshipRedirect::is_self_loop_after_redirect`]).
fn transact_relationship_redirects(
    tx: &MultiTransaction,
    canonical_id: &EntityId,
    relationships: &[RelationshipRedirect],
) -> crate::error::Result<()> {
    for edge in relationships {
        failpoint_check(MergeWriteStep::Relationship)?;
        let mut rm_params = BTreeMap::new();
        rm_params.insert("src".to_owned(), DataValue::Str(edge.src.as_str().into()));
        rm_params.insert("dst".to_owned(), DataValue::Str(edge.dst.as_str().into()));
        tx_run(tx, &queries::rm_relationship(), rm_params)?;

        if edge.src == edge.dst {
            // WHY: `merged_id` is both endpoints (a self-loop on the entity
            // being merged, e.g. `(merged, merged)`) — redirecting only one
            // side would leave the other pointing at the row this commit is
            // about to delete, a dangling edge. Drop it entirely: the row
            // is already removed above, and nothing is written in its
            // place, matching the net effect of the two-pass
            // read-then-write this module replaced (the second pass's read
            // observed the first pass's rewrite and canceled it out).
            continue;
        }

        if edge.is_self_loop_after_redirect(canonical_id.as_str()) {
            // WHY: redirecting would create a self-loop on the canonical
            // entity; the stale row is dropped, never rewritten onto one.
            continue;
        }

        let new_src = if edge.redirect_src {
            canonical_id.as_str()
        } else {
            edge.src.as_str()
        };
        let new_dst = if edge.redirect_src {
            edge.dst.as_str()
        } else {
            canonical_id.as_str()
        };
        let mut put_params = BTreeMap::new();
        put_params.insert("src".to_owned(), DataValue::Str(new_src.into()));
        put_params.insert("dst".to_owned(), DataValue::Str(new_dst.into()));
        put_params.insert(
            "relation".to_owned(),
            DataValue::Str(edge.relation.as_str().into()),
        );
        put_params.insert("weight".to_owned(), DataValue::from(edge.weight));
        put_params.insert(
            "created_at".to_owned(),
            DataValue::Str(edge.created_at.as_str().into()),
        );
        tx_run(tx, &queries::upsert_relationship(), put_params)?;
    }
    Ok(())
}

/// Transfer every staged `fact_entities` row from `merged_id` onto
/// `canonical_id`.
fn transact_fact_entity_transfers(
    tx: &MultiTransaction,
    canonical_id: &EntityId,
    merged_id: &EntityId,
    fact_entities: &[FactEntityTransfer],
) -> crate::error::Result<()> {
    for transfer in fact_entities {
        failpoint_check(MergeWriteStep::FactEntity)?;
        let mut rm_params = BTreeMap::new();
        rm_params.insert(
            "fact_id".to_owned(),
            DataValue::Str(transfer.fact_id.as_str().into()),
        );
        rm_params.insert(
            "entity_id".to_owned(),
            DataValue::Str(merged_id.as_str().into()),
        );
        tx_run(tx, &queries::rm_fact_entity(), rm_params)?;

        let mut put_params = BTreeMap::new();
        put_params.insert(
            "fact_id".to_owned(),
            DataValue::Str(transfer.fact_id.as_str().into()),
        );
        put_params.insert(
            "entity_id".to_owned(),
            DataValue::Str(canonical_id.as_str().into()),
        );
        put_params.insert(
            "created_at".to_owned(),
            DataValue::Str(transfer.created_at.as_str().into()),
        );
        tx_run(tx, &queries::upsert_fact_entity(), put_params)?;
    }
    Ok(())
}

/// Upsert the canonical entity's alias row, when `plan_alias_upsert` staged
/// one (a no-op merge onto an existing alias stages `None`).
fn transact_alias_upsert(
    tx: &MultiTransaction,
    canonical_id: &EntityId,
    alias: Option<&AliasUpsert>,
    now_str: &str,
) -> crate::error::Result<()> {
    let Some(alias) = alias else {
        return Ok(());
    };
    failpoint_check(MergeWriteStep::Alias)?;
    let emb_value = alias.embedding.clone().map_or(DataValue::Null, |v| {
        DataValue::Vec(Vector::F32(Array1::from(v)))
    });
    let mut params = BTreeMap::new();
    params.insert(
        "id".to_owned(),
        DataValue::Str(canonical_id.as_str().into()),
    );
    params.insert(
        "aliases".to_owned(),
        DataValue::Str(alias.aliases_str.as_str().into()),
    );
    params.insert("updated_at".to_owned(), DataValue::Str(now_str.into()));
    params.insert(
        "name".to_owned(),
        DataValue::Str(alias.name.as_str().into()),
    );
    params.insert(
        "entity_type".to_owned(),
        DataValue::Str(alias.entity_type.as_str().into()),
    );
    params.insert(
        "created_at".to_owned(),
        DataValue::Str(alias.created_at.as_str().into()),
    );
    params.insert("name_embedding".to_owned(), emb_value);
    tx_run(tx, &queries::upsert_entity(), params)
}

/// Delete the merged entity row and record the merge audit row.
fn transact_entity_deletion_and_audit(
    tx: &MultiTransaction,
    canonical_id: &EntityId,
    merged_id: &EntityId,
    merged_name: &str,
    now_str: &str,
    facts_transferred: u32,
    relationships_redirected: u32,
) -> crate::error::Result<()> {
    failpoint_check(MergeWriteStep::DeleteEntity)?;
    let mut rm_entity_params = BTreeMap::new();
    rm_entity_params.insert("id".to_owned(), DataValue::Str(merged_id.as_str().into()));
    tx_run(tx, &queries::rm_entity(), rm_entity_params)?;

    failpoint_check(MergeWriteStep::Audit)?;
    let mut audit_params = BTreeMap::new();
    audit_params.insert(
        "canonical_id".to_owned(),
        DataValue::Str(canonical_id.as_str().into()),
    );
    audit_params.insert(
        "merged_id".to_owned(),
        DataValue::Str(merged_id.as_str().into()),
    );
    audit_params.insert("merged_name".to_owned(), DataValue::Str(merged_name.into()));
    audit_params.insert("merge_score".to_owned(), DataValue::from(0.0_f64));
    audit_params.insert(
        "facts_transferred".to_owned(),
        DataValue::from(i64::from(facts_transferred)),
    );
    audit_params.insert(
        "relationships_redirected".to_owned(),
        DataValue::from(i64::from(relationships_redirected)),
    );
    audit_params.insert("merged_at".to_owned(), DataValue::Str(now_str.into()));
    tx_run(tx, &queries::put_merge_audit(), audit_params)
}

/// Drive every staged merge write through the open transaction, in issue
/// order: relationships, `fact_entities`, alias, entity deletion, audit.
fn transact_merge_writes(
    tx: &MultiTransaction,
    plan: &MergeWritePlan<'_>,
) -> crate::error::Result<()> {
    transact_relationship_redirects(tx, plan.canonical_id, plan.relationships)?;
    transact_fact_entity_transfers(tx, plan.canonical_id, plan.merged_id, plan.fact_entities)?;
    transact_alias_upsert(tx, plan.canonical_id, plan.alias, plan.now_str)?;
    transact_entity_deletion_and_audit(
        tx,
        plan.canonical_id,
        plan.merged_id,
        plan.merged_name,
        plan.now_str,
        plan.facts_transferred,
        plan.relationships_redirected,
    )
}

#[cfg(feature = "mneme-engine")]
impl KnowledgeStore {
    /// Execute a merge as one all-or-nothing transaction (#7289): transfer
    /// edges, aliases, `fact_entities`, and record audit. The entity with
    /// `canonical_id` survives; `merged_id` is removed.
    ///
    /// # Errors
    ///
    /// Returns an error — with nothing written — if either entity cannot be
    /// loaded, if any transacted write fails, or if the commit itself fails.
    #[instrument(skip(self))]
    pub(crate) fn commit_entity_merge(
        &self,
        canonical_id: &EntityId,
        merged_id: &EntityId,
    ) -> crate::error::Result<crate::dedup::MergeRecord> {
        // WHY: serializes against a concurrent single-fact insert/merge the
        // same way `commit_consolidation` holds `insert_lock` across its own
        // read-then-write staging.
        let _guard = self.insert_lock.lock();

        let canonical = self.load_entity(canonical_id)?;
        let merged = self.load_entity(merged_id)?;

        let relationships = plan_relationship_redirects(self, merged_id)?;
        let fact_entities = plan_fact_entity_transfers(self, merged_id)?;
        let alias = plan_alias_upsert(self, canonical_id, &merged.name)?;

        let relationships_redirected = u32::try_from(relationships.len()).unwrap_or(0);
        let facts_transferred = u32::try_from(fact_entities.len()).unwrap_or(0);
        let now = jiff::Timestamp::now();
        let now_str = crate::knowledge::format_timestamp(&now);

        let tx = self.db.multi_transaction(true);
        let plan = MergeWritePlan {
            canonical_id,
            merged_id,
            merged_name: &merged.name,
            relationships: &relationships,
            fact_entities: &fact_entities,
            alias: alias.as_ref(),
            now_str: &now_str,
            facts_transferred,
            relationships_redirected,
        };
        if let Err(err) = transact_merge_writes(&tx, &plan) {
            // WHY: abort is best-effort cleanup after the real error is
            // already in hand (the write error `err` below is what the
            // caller sees either way), matching `commit_consolidation`,
            // but a failed abort still needs to be visible to an operator
            // — an engine that cannot roll back its own failed transaction
            // is a signal worth surfacing, not silence.
            if let Err(abort_err) = tx.abort() {
                tracing::warn!(
                    %canonical_id, %merged_id,
                    write_error = %err, abort_error = %abort_err,
                    "merge transaction abort failed after write error"
                );
            }
            return Err(err);
        }
        tx.commit().map_err(|err| tx_error(&err))?;

        // WHY: these are best-effort housekeeping on state the graph
        // checker does not examine (review flags, derived-rule staleness,
        // the pending-merges queue) — they run strictly after commit, never
        // gating whether the merge itself lands, matching
        // `commit_consolidation`'s post-commit invalidation step.
        if let Err(e) = self.clear_entity_flags(merged_id) {
            tracing::warn!(
                %canonical_id, %merged_id, error = %e,
                "failed to clear entity flags after merge"
            );
        }
        if let Err(e) = self.invalidate_derived_facts() {
            tracing::warn!(
                %canonical_id, %merged_id, error = %e,
                "failed to invalidate derived facts after merge"
            );
        }

        Ok(crate::dedup::MergeRecord {
            canonical_entity_id: canonical.id,
            merged_entity_id: merged_id.clone(),
            merged_entity_name: merged.name,
            merge_score: 0.0,
            facts_transferred,
            relationships_redirected,
            merged_at: now,
        })
    }
}

// ---------------------------------------------------------------------
// Failure-injection seam (#7289)
// ---------------------------------------------------------------------

/// Invoke the test failpoint for `step`.
#[cfg(test)]
fn failpoint_check(step: MergeWriteStep) -> crate::error::Result<()> {
    failpoint::check(step)
}

/// Invoke the test failpoint for `step`. Compiled to a no-op outside tests.
#[cfg(not(test))]
#[expect(
    clippy::unnecessary_wraps,
    reason = "the test build's failpoint_check can fail; the signatures must match so call sites compile unchanged"
)]
fn failpoint_check(_step: MergeWriteStep) -> crate::error::Result<()> {
    Ok(())
}

/// Failure-injection seam for the entity-merge write sequence (#7289).
///
/// Tests arm a thread-local step; the commit's next write at that step then
/// fails exactly once with a synthetic store error, letting a test prove
/// that no prefix of the write set survives and that the graph stays
/// consistent. Thread-local (not a process-global hook) so parallel `cargo
/// test` threads in one binary arm independent failpoints.
///
/// Production builds contain only the no-op [`failpoint_check`] shim above
/// — the seam is `cfg(test)`-only and cannot be armed outside tests.
#[cfg(test)]
pub(crate) mod failpoint {
    use std::cell::Cell;

    use super::MergeWriteStep;
    use crate::error::EngineQuerySnafu;

    thread_local! {
        static ARMED: Cell<Option<MergeWriteStep>> = const { Cell::new(None) };
    }

    /// Arm the failpoint: this thread's next merge write at `step` fails
    /// once, then the failpoint disarms itself.
    pub(crate) fn arm(step: MergeWriteStep) {
        ARMED.with(|armed| armed.set(Some(step)));
    }

    pub(super) fn check(step: MergeWriteStep) -> crate::error::Result<()> {
        ARMED.with(|armed| {
            if armed.get() == Some(step) {
                armed.set(None);
                return Err(EngineQuerySnafu {
                    message: format!("injected merge write failure at {step:?}"),
                }
                .build());
            }
            Ok(())
        })
    }
}
