//! Atomic multi-item write path for extraction persistence (aletheia#5306).
//!
//! `ExtractionEngine::persist_with_scope` used to write entities, then
//! relationships, then facts as separate single-item transactions. A later
//! failure — a malformed relation, a rejected fact, an engine error — left
//! whatever had already been written standing as orphaned graph state, and a
//! truncated extraction (over `max_entities`/`max_relationships`/`max_facts`)
//! reported plain success while silently dropping the excess. The caller now
//! builds a fully-validated plan first (limits applied, structural checks
//! run, invalid rows classified as skipped with a reason) and hands the
//! surviving items here as one all-or-nothing write.
//!
//! Every entity, relationship, and fact write in the plan runs inside one
//! [`krites::MultiTransaction`](crate::engine::MultiTransaction): the first
//! failing write aborts the whole transaction, so nothing from this batch
//! lands rather than a prefix of it. Fact-entity linking is the one
//! exception — a link failure is recorded and the batch still commits,
//! matching the pre-existing tolerance in `insert_fact_entity` callers: a
//! missing enrichment edge is not a reason to drop an otherwise-valid fact.

use std::collections::HashSet;

use tracing::instrument;

use super::{KnowledgeStore, queries};

/// One fact staged for atomic persistence, paired with the pre-slugified
/// subject/object references extraction produced.
///
/// The slugs are needed to link the fact to any entity this same batch also
/// writes (aletheia#4675); `crate::knowledge::Fact` itself only carries the
/// fact's rendered `content` string, not its raw subject/object.
#[cfg(feature = "mneme-engine")]
pub(crate) struct FactInsert {
    /// The fact to write.
    pub(crate) fact: crate::knowledge::Fact,
    /// `slugify(subject)` of the raw extracted triple.
    pub(crate) subject_slug: String,
    /// `slugify(object)` of the raw extracted triple.
    pub(crate) object_slug: String,
}

/// Counts and non-fatal rejections produced by one atomic persistence batch.
#[cfg(feature = "mneme-engine")]
#[derive(Debug, Clone, Default)]
pub(crate) struct BatchPersistOutcome {
    /// Number of entities written.
    pub(crate) entities_inserted: usize,
    /// Number of relationships written.
    pub(crate) relationships_inserted: usize,
    /// Number of facts written.
    pub(crate) facts_inserted: usize,
    /// Number of fact-entity edges linked.
    pub(crate) fact_entities_inserted: usize,
    /// Number of fact-entity link attempts that failed and were dropped.
    pub(crate) fact_entity_link_failures: usize,
    /// Facts the admission policy rejected before the transaction opened,
    /// as `(content, reason)`. Rejecting one fact does not abort the batch —
    /// it must not cost the rest of the extraction its entities and
    /// relationships.
    pub(crate) admission_rejected: Vec<(String, String)>,
}

fn tx_error(err: crate::engine::MultiTransactionError) -> crate::error::Error {
    crate::error::EngineQuerySnafu {
        message: err.to_string(),
    }
    .build()
}

#[cfg(feature = "mneme-engine")]
impl KnowledgeStore {
    /// Persist a pre-validated extraction plan as one all-or-nothing write.
    ///
    /// `entities`, `relationships`, and `facts` are expected to already be
    /// structurally valid — the caller pre-validates limits, empty fields,
    /// and relation-type/confidence bounds before building this plan — so
    /// every item here is expected to write cleanly. A write that still
    /// fails (an engine-level error) aborts the whole batch: nothing from
    /// it lands, rather than whatever happened to write before the failure.
    ///
    /// Facts are admission-checked before the transaction opens, under the
    /// same `insert_lock` [`insert_fact`](Self::insert_fact) takes for a
    /// single fact — extended here to cover the whole batch, so a
    /// concurrent insert cannot interleave with a partially-admitted batch.
    ///
    /// # Errors
    ///
    /// Returns an error, with nothing written, if any accepted item's write
    /// fails at the engine level.
    #[instrument(skip(self, entities, relationships, facts))]
    #[expect(
        clippy::too_many_lines,
        reason = "one straight-line transaction: admission check, then entities, relationships, facts, links, commit"
    )]
    pub(crate) fn persist_extraction_batch(
        &self,
        entities: &[crate::knowledge::Entity],
        relationships: &[crate::knowledge::Relationship],
        facts: &[FactInsert],
    ) -> crate::error::Result<BatchPersistOutcome> {
        let _guard = self.insert_lock.lock();

        let mut outcome = BatchPersistOutcome::default();
        let mut accepted_facts: Vec<&FactInsert> = Vec::with_capacity(facts.len());
        for item in facts {
            match self.admission_policy.should_admit(&item.fact) {
                crate::admission::AdmissionDecision::Admit => accepted_facts.push(item),
                crate::admission::AdmissionDecision::Reject(rejection) => {
                    tracing::debug!(
                        fact_id = %item.fact.id,
                        factor = ?rejection.factor,
                        reason = %rejection.reason,
                        "fact rejected by admission policy during batch persist"
                    );
                    outcome
                        .admission_rejected
                        .push((item.fact.content.clone(), rejection.reason));
                }
            }
        }

        if entities.is_empty() && relationships.is_empty() && accepted_facts.is_empty() {
            return Ok(outcome);
        }

        let known_entity_ids: HashSet<String> =
            entities.iter().map(|e| e.id.as_str().to_owned()).collect();

        let tx = self.db.multi_transaction(true);

        for entity in entities {
            let params = super::marshal::entity_to_params(entity);
            if let Err(err) = tx.transact(crate::engine::TransactionPayload::Query((
                queries::upsert_entity(),
                params,
            ))) {
                let _ = tx.abort();
                return Err(tx_error(err));
            }
            outcome.entities_inserted += 1;
        }

        for rel in relationships {
            let params = super::marshal::relationship_to_params(rel);
            if let Err(err) = tx.transact(crate::engine::TransactionPayload::Query((
                queries::upsert_relationship(),
                params,
            ))) {
                let _ = tx.abort();
                return Err(tx_error(err));
            }
            outcome.relationships_inserted += 1;
        }

        let now = crate::knowledge::format_timestamp(&jiff::Timestamp::now());
        for item in &accepted_facts {
            let params = super::marshal::fact_to_params(&item.fact);
            if let Err(err) = tx.transact(crate::engine::TransactionPayload::Query((
                queries::upsert_fact(),
                params,
            ))) {
                let _ = tx.abort();
                return Err(tx_error(err));
            }
            outcome.facts_inserted += 1;

            // WHY (#4675): link the fact to the subject/object entities it
            // references so graph-aware recall, scoped dedup, and
            // consolidation see real fact-entity edges. Linking is scoped to
            // entities known from this batch; a subject/object slug that did
            // not resolve to an entity written here is skipped rather than
            // linked to a dangling id.
            let mut linked_this_fact: HashSet<String> = HashSet::new();
            for slug in [item.subject_slug.as_str(), item.object_slug.as_str()] {
                if !known_entity_ids.contains(slug) || !linked_this_fact.insert(slug.to_owned()) {
                    continue;
                }
                let Ok(entity_id) = crate::id::EntityId::new(slug) else {
                    continue;
                };
                let mut link_params = std::collections::BTreeMap::new();
                link_params.insert(
                    "fact_id".to_owned(),
                    crate::engine::DataValue::Str(item.fact.id.as_str().into()),
                );
                link_params.insert(
                    "entity_id".to_owned(),
                    crate::engine::DataValue::Str(entity_id.as_str().into()),
                );
                link_params.insert(
                    "created_at".to_owned(),
                    crate::engine::DataValue::Str(now.clone().into()),
                );
                match tx.transact(crate::engine::TransactionPayload::Query((
                    queries::upsert_fact_entity(),
                    link_params,
                ))) {
                    Ok(_) => outcome.fact_entities_inserted += 1,
                    Err(err) => {
                        // WHY: a link failure is non-fatal (matches the prior
                        // per-item `insert_fact_entity` tolerance) — the
                        // transaction stays open and still commits. Only the
                        // core entity/relationship/fact writes above abort
                        // the batch.
                        outcome.fact_entity_link_failures += 1;
                        tracing::warn!(
                            %err,
                            fact_id = %item.fact.id,
                            entity_id = %entity_id,
                            "failed to link fact to referenced entity during batch persist"
                        );
                    }
                }
            }
        }

        tx.commit().map_err(tx_error)?;

        // WHY: metrics must reflect what actually committed, not what a
        // mid-batch statement merely succeeded at inside the still-open
        // transaction — a later item's failure would have aborted the whole
        // batch and rolled this fact back with it. Record only after commit.
        for item in &accepted_facts {
            crate::metrics::record_fact_inserted(&item.fact.nous_id);
        }

        // WHY (#4662): ontological/derived rules key off entities and facts
        // just written; mark derived materializations stale exactly once for
        // the whole batch now that it has committed, rather than once per
        // item as the single-item insert paths do.
        if outcome.entities_inserted > 0 || outcome.facts_inserted > 0 {
            self.invalidate_derived_facts()?;
        }

        Ok(outcome)
    }
}
