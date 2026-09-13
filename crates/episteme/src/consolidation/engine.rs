//! Engine integration for fact consolidation.
//!
//! Implements consolidation operations on `KnowledgeStore`: candidate
//! identification, LLM-driven consolidation execution, and audit trail.
//!
//! ## Atomicity and idempotency (#5311)
//!
//! A consolidation's full write set — consolidated facts, their multiplicity
//! and provenance side-index rows, the supersession updates on the original
//! facts, and the audit row — commits as one transaction via
//! [`KnowledgeStore::commit_consolidation`](crate::knowledge_store::KnowledgeStore).
//! The minted IDs are deterministic: the run key is a SHA-256 over the nous,
//! trigger, consolidation config, and the sorted source fact IDs, the audit
//! row's ID is `cons-audit-{run key}`, and each consolidated fact's ID
//! (`cons-…`) derives from the run key, its batch's sorted source IDs, and
//! its ordinal within the batch. A retry after a failure therefore converges
//! on the same rows instead of minting fresh ULIDs, and a retry that finds
//! the audit row already committed short-circuits before re-running the LLM.
use std::collections::BTreeMap;
use std::sync::Arc;

use sha2::{Digest, Sha256};
use tracing::instrument;

use super::{
    CLUSTER_FACTS_FOR_CONSOLIDATION, COMMUNITY_OVERFLOW_CANDIDATES, CONSOLIDATION_AUDIT_DDL,
    CONSOLIDATION_AUDIT_OWNER_BACKFILL_DDL, ConsolidatedFact, ConsolidationAuditRecord,
    ConsolidationCandidate, ConsolidationConfig, ConsolidationError, ConsolidationProvenanceRow,
    ConsolidationProvider, ConsolidationResult, ConsolidationTrigger, ConsolidationWritePlan,
    ENTITY_FACTS_FOR_CONSOLIDATION, ENTITY_OVERFLOW_CANDIDATES, FACT_MULTIPLICITY_DDL,
    FactMultiplicity, IncompatibleSourcesSnafu, RateLimitedSnafu, SourceFact, StoreSnafu,
    age_cutoff, batch_facts, consolidation_system_prompt, consolidation_user_message,
    parse_consolidation_response,
};
use crate::engine::DataValue;
use crate::id::{EntityId, FactId};
use crate::knowledge::{
    EpistemicTier, FactAccess, FactLifecycle, FactProvenance, FactSensitivity, FactTemporal,
    MemoryScope, Visibility,
};
use crate::knowledge_store::KnowledgeStore;
use eidos::workspace::ProjectId;

/// Convert a non-negative `i64` from a Datalog row to `usize`.
///
/// Negative values indicate data corruption in the knowledge store (counts
/// should never be negative). When detected, a warning is logged with the
/// raw value and the function returns 0 for operational continuity.
fn i64_as_usize(v: i64) -> usize {
    if let Ok(n) = v.try_into() {
        n
    } else {
        // WHY: negative counts are a data corruption indicator — surface it
        // via logging rather than silently defaulting.
        tracing::warn!(
            raw_value = v,
            "negative i64 encountered where usize expected — possible data corruption, defaulting to 0"
        );
        0
    }
}

impl KnowledgeStore {
    /// Initialize the `consolidation_audit` relation. Called during schema setup.
    #[expect(
        dead_code,
        reason = "knowledge consolidation engine, feature-gated behind mneme-engine"
    )]
    pub(crate) fn init_consolidation_audit(&self) -> crate::error::Result<()> {
        self.run_mut_query(CONSOLIDATION_AUDIT_DDL, BTreeMap::new())?;
        Ok(())
    }

    /// Initialize the `fact_multiplicity` side-index relation (#3634).
    ///
    /// Called during schema setup. Separate from the facts relation so the
    /// fact schema stays stable and legacy records remain valid.
    #[expect(
        dead_code,
        reason = "knowledge consolidation engine, feature-gated behind mneme-engine"
    )]
    pub(crate) fn init_fact_multiplicity(&self) -> crate::error::Result<()> {
        self.run_mut_query(FACT_MULTIPLICITY_DDL, BTreeMap::new())?;
        Ok(())
    }

    /// Find entity-overflow consolidation candidates.
    ///
    /// # Errors
    ///
    /// Returns an error if the knowledge store query fails.
    #[instrument(skip(self))]
    pub fn find_entity_overflow_candidates(
        &self,
        nous_id: &str,
        config: &ConsolidationConfig,
    ) -> Result<Vec<ConsolidationCandidate>, ConsolidationError> {
        let cutoff = age_cutoff(config.min_age_days);
        let mut params = BTreeMap::new();
        params.insert("nous_id".to_owned(), DataValue::Str(nous_id.into()));
        params.insert(
            "min_count".to_owned(),
            DataValue::from(i64::try_from(config.entity_fact_threshold).unwrap_or(i64::MAX)),
        );
        params.insert("cutoff".to_owned(), DataValue::Str(cutoff.clone().into()));

        let result = self
            .run_query(ENTITY_OVERFLOW_CANDIDATES, params)
            .map_err(|e| {
                StoreSnafu {
                    message: e.to_string(),
                }
                .build()
            })?;

        let mut candidates = Vec::new();
        for i in 0..result.row_count() {
            // kanon:ignore RUST/no-result-unwrap-or-default — missing query column handled by EntityId::new failure below
            let entity_id_str = result.get_string(i, "entity_id").unwrap_or_default();
            let fact_count = i64_as_usize(result.get_i64(i, "fact_count").unwrap_or(0));
            let entity_id = EntityId::new(entity_id_str).map_err(|e| {
                StoreSnafu {
                    message: e.to_string(),
                }
                .build()
            })?;

            let facts = self
                .gather_entity_facts(nous_id, &entity_id, &cutoff)
                .map_err(|e| {
                    StoreSnafu {
                        message: e.to_string(),
                    }
                    .build()
                })?;

            let fact_ids: Vec<FactId> = facts.iter().map(|s| s.id.clone()).collect();

            candidates.push(ConsolidationCandidate {
                trigger: ConsolidationTrigger::EntityOverflow {
                    entity_id: entity_id.clone(),
                    fact_count,
                },
                fact_ids,
                fact_count,
                entity_id: Some(entity_id),
                cluster_id: None,
            });
        }
        Ok(candidates)
    }

    /// Find community-overflow consolidation candidates.
    ///
    /// # Errors
    ///
    /// Returns an error if the knowledge store query fails.
    #[instrument(skip(self))]
    pub fn find_community_overflow_candidates(
        &self,
        nous_id: &str,
        config: &ConsolidationConfig,
    ) -> Result<Vec<ConsolidationCandidate>, ConsolidationError> {
        let cutoff = age_cutoff(config.min_age_days);
        let mut params = BTreeMap::new();
        params.insert("nous_id".to_owned(), DataValue::Str(nous_id.into()));
        params.insert(
            "min_count".to_owned(),
            DataValue::from(i64::try_from(config.community_fact_threshold).unwrap_or(i64::MAX)),
        );
        params.insert("cutoff".to_owned(), DataValue::Str(cutoff.clone().into()));

        let result = self
            .run_query(COMMUNITY_OVERFLOW_CANDIDATES, params)
            .map_err(|e| {
                StoreSnafu {
                    message: e.to_string(),
                }
                .build()
            })?;

        let mut candidates = Vec::new();
        for i in 0..result.row_count() {
            let cluster_id = result.get_i64(i, "cluster_id").unwrap_or(-1);
            let fact_count = i64_as_usize(result.get_i64(i, "fact_count").unwrap_or(0));

            let facts = self
                .gather_cluster_facts(nous_id, cluster_id, &cutoff)
                .map_err(|e| {
                    StoreSnafu {
                        message: e.to_string(),
                    }
                    .build()
                })?;

            let fact_ids: Vec<FactId> = facts.iter().map(|s| s.id.clone()).collect();

            candidates.push(ConsolidationCandidate {
                trigger: ConsolidationTrigger::CommunityOverflow {
                    cluster_id,
                    fact_count,
                },
                fact_ids,
                fact_count,
                entity_id: None,
                cluster_id: Some(cluster_id),
            });
        }
        Ok(candidates)
    }

    /// Gather eligible facts for an entity.
    fn gather_entity_facts(
        &self,
        nous_id: &str,
        entity_id: &EntityId,
        cutoff: &str,
    ) -> crate::error::Result<Vec<SourceFact>> {
        let mut params = BTreeMap::new();
        params.insert("nous_id".to_owned(), DataValue::Str(nous_id.into()));
        params.insert(
            "entity_id".to_owned(),
            DataValue::Str(entity_id.as_str().into()),
        );
        params.insert("cutoff".to_owned(), DataValue::Str(cutoff.into()));

        let result = self.run_query(ENTITY_FACTS_FOR_CONSOLIDATION, params)?;
        parse_fact_rows(&result.rows)
    }

    /// Gather eligible facts for a community cluster.
    fn gather_cluster_facts(
        &self,
        nous_id: &str,
        cluster_id: i64,
        cutoff: &str,
    ) -> crate::error::Result<Vec<SourceFact>> {
        let mut params = BTreeMap::new();
        params.insert("nous_id".to_owned(), DataValue::Str(nous_id.into()));
        params.insert("cluster_id".to_owned(), DataValue::from(cluster_id));
        params.insert("cutoff".to_owned(), DataValue::Str(cutoff.into()));

        let result = self.run_query(CLUSTER_FACTS_FOR_CONSOLIDATION, params)?;
        parse_fact_rows(&result.rows)
    }

    /// Execute a consolidation: insert new facts, supersede originals, record audit.
    ///
    /// The write set commits atomically and the minted IDs are deterministic
    /// (#5311): before calling the LLM, the derived run key is checked
    /// against `consolidation_audit`, so a retry of an already-committed run
    /// short-circuits with the recorded counts instead of re-consolidating.
    ///
    /// If `dry_run` is true, returns the proposed result without mutations.
    #[instrument(skip(self, provider, candidate))]
    pub(crate) fn execute_consolidation(
        &self,
        provider: &dyn ConsolidationProvider,
        candidate: &ConsolidationCandidate,
        nous_id: &str,
        config: &ConsolidationConfig,
        dry_run: bool,
    ) -> Result<ConsolidationResult, ConsolidationError> {
        let cutoff = age_cutoff(config.min_age_days);
        let facts = match &candidate.trigger {
            ConsolidationTrigger::EntityOverflow { entity_id, .. } => self
                .gather_entity_facts(nous_id, entity_id, &cutoff)
                .map_err(|e| {
                    StoreSnafu {
                        message: e.to_string(),
                    }
                    .build()
                })?,
            ConsolidationTrigger::CommunityOverflow { cluster_id, .. } => self
                .gather_cluster_facts(nous_id, *cluster_id, &cutoff)
                .map_err(|e| {
                    StoreSnafu {
                        message: e.to_string(),
                    }
                    .build()
                })?,
        };

        if facts.is_empty() {
            return Ok(ConsolidationResult {
                consolidated_facts: Vec::new(),
                superseded_fact_ids: Vec::new(),
                original_count: 0,
                consolidated_count: 0,
            });
        }

        let source_ids: Vec<FactId> = facts.iter().map(|s| s.id.clone()).collect();
        let run_key = consolidation_run_key(candidate, &source_ids, nous_id, config);
        if !dry_run && let Some(recorded) = self.completed_consolidation(&run_key)? {
            // WHY(#5311): the audit row commits in the same transaction as the
            // facts and supersessions, so its presence under the run's
            // idempotency key proves the whole write set already landed. A
            // retry — after a post-commit failure, or a caller that never
            // learned the outcome — must not run the LLM again and mint a
            // second set of outputs.
            tracing::info!(
                run_key,
                nous_id,
                "consolidation run already committed; returning the recorded result"
            );
            return Ok(recorded);
        }

        let LlmConsolidationResult {
            result,
            supersession_batches,
        } = run_llm_consolidation(provider, &facts, config)?;

        if dry_run {
            return Ok(result);
        }

        let plan = build_consolidation_write_plan(
            candidate,
            &result,
            &supersession_batches,
            nous_id,
            &run_key,
        )?;
        self.commit_consolidation(&plan)?;

        Ok(result)
    }

    /// Look up the idempotency record for a consolidation run (#5311).
    ///
    /// Returns the recorded outcome when `consolidation_audit` already holds
    /// this run's `cons-audit-{run key}` row. The returned result carries the
    /// recorded counts and superseded IDs but no `ConsolidatedFact` payloads
    /// — callers only consume counts and IDs.
    fn completed_consolidation(
        &self,
        run_key: &str,
    ) -> Result<Option<ConsolidationResult>, ConsolidationError> {
        let script = r"
?[original_count, consolidated_count, original_fact_ids] :=
    *consolidation_audit{id: $id, original_count, consolidated_count, original_fact_ids}
";
        let mut params = BTreeMap::new();
        params.insert(
            "id".to_owned(),
            DataValue::Str(audit_id_for_run_key(run_key).into()),
        );
        let result = self.run_query(script, params).map_err(|e| {
            StoreSnafu {
                message: e.to_string(),
            }
            .build()
        })?;

        if result.is_empty() {
            return Ok(None);
        }

        let original_count = i64_as_usize(result.get_i64(0, "original_count").unwrap_or(0));
        let consolidated_count = i64_as_usize(result.get_i64(0, "consolidated_count").unwrap_or(0));
        // kanon:ignore RUST/no-result-unwrap-or-default — audit read: empty JSON decodes to an empty list below
        let original_fact_ids_json = result
            .get_string(0, "original_fact_ids")
            .unwrap_or_default();
        let id_strings: Vec<String> =
            serde_json::from_str(&original_fact_ids_json).map_err(|e| {
                StoreSnafu {
                    message: format!(
                        "failed to decode recorded consolidation source fact IDs: {e}"
                    ),
                }
                .build()
            })?;
        let mut superseded_fact_ids = Vec::with_capacity(id_strings.len());
        for id_string in id_strings {
            superseded_fact_ids.push(FactId::new(id_string).map_err(|e| {
                StoreSnafu {
                    message: format!("invalid recorded consolidation source fact ID: {e}"),
                }
                .build()
            })?);
        }

        Ok(Some(ConsolidationResult {
            consolidated_facts: Vec::new(),
            superseded_fact_ids,
            original_count,
            consolidated_count,
        }))
    }

    /// Read the source provenance recorded for a consolidated fact.
    ///
    /// Returns `None` if no provenance side-index row exists. Used by tests
    /// and by recall paths that need to surface why a consolidated fact was
    /// emitted.
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "used by consolidation engine tests")
    )]
    #[expect(
        clippy::type_complexity,
        reason = "simple pair of vectors; aliasing adds no clarity"
    )]
    pub(crate) fn get_consolidation_provenance(
        &self,
        fact_id: &FactId,
    ) -> Result<Option<(Vec<FactId>, Vec<String>)>, ConsolidationError> {
        let script = r"
?[source_fact_ids, source_session_ids] :=
    *consolidation_provenance{consolidated_fact_id: $fact_id, source_fact_ids, source_session_ids}
";
        let mut params = BTreeMap::new();
        params.insert(
            "fact_id".to_owned(),
            DataValue::Str(fact_id.as_str().into()),
        );
        let result = self.run_query(script, params).map_err(|e| {
            StoreSnafu {
                message: e.to_string(),
            }
            .build()
        })?;

        if result.is_empty() {
            return Ok(None);
        }

        let source_fact_ids_json = result.get_string(0, "source_fact_ids").unwrap_or_default();
        let source_session_ids_json = result
            .get_string(0, "source_session_ids")
            .unwrap_or_default();

        let source_fact_id_strings = serde_json::from_str::<Vec<String>>(&source_fact_ids_json)
            .map_err(|e| {
                StoreSnafu {
                    message: format!("failed to decode consolidation source fact IDs: {e}"),
                }
                .build()
            })?;
        let mut source_fact_ids = Vec::with_capacity(source_fact_id_strings.len());
        for source_fact_id in source_fact_id_strings {
            source_fact_ids.push(FactId::new(source_fact_id).map_err(|e| {
                StoreSnafu {
                    message: format!("invalid consolidation source fact ID: {e}"),
                }
                .build()
            })?);
        }
        let source_session_ids = serde_json::from_str::<Vec<String>>(&source_session_ids_json)
            .map_err(|e| {
                StoreSnafu {
                    message: format!("failed to decode consolidation source session IDs: {e}"),
                }
                .build()
            })?;

        Ok(Some((source_fact_ids, source_session_ids)))
    }

    /// Look up multiplicity metadata for a consolidated fact (#3634).
    ///
    /// Returns `None` if no multiplicity record exists (e.g. the fact was
    /// not produced by consolidation, or was persisted before this
    /// side-index was introduced).
    ///
    /// # Errors
    ///
    /// Returns an error if the knowledge store query fails.
    #[instrument(skip(self))]
    pub fn get_fact_multiplicity(
        &self,
        fact_id: &FactId,
    ) -> Result<Option<FactMultiplicity>, ConsolidationError> {
        // WHY(#5672): the batched read is the single owner of the
        // `fact_multiplicity` decode, so the one-fact path delegates rather
        // than carrying a second copy that can drift from it.
        let mut found = self.get_fact_multiplicities(std::slice::from_ref(fact_id))?;
        Ok(found.remove(fact_id.as_str()))
    }

    /// Look up multiplicity metadata for many consolidated facts in one query.
    ///
    /// Returns a map keyed by fact id. Facts with no multiplicity record are
    /// absent from the map rather than present with a zero value, matching
    /// [`Self::get_fact_multiplicity`]'s `None`.
    ///
    /// WHY(#5672): the recall hot path enriches a whole result set at once.
    /// A constant rule of the requested ids joined against the stored relation
    /// keeps the lookup indexed while costing one query instead of one per fact.
    ///
    /// # Errors
    ///
    /// Returns an error if the knowledge store query fails.
    #[instrument(skip(self, fact_ids))]
    pub fn get_fact_multiplicities(
        &self,
        fact_ids: &[FactId],
    ) -> Result<std::collections::HashMap<String, FactMultiplicity>, ConsolidationError> {
        if fact_ids.is_empty() {
            return Ok(std::collections::HashMap::new());
        }

        let script = r"
requested[fact_id] <- $fact_ids
?[fact_id, source_count, first_observed, last_observed, time_spread_seconds, recorded_at] :=
    requested[fact_id],
    *fact_multiplicity{fact_id, source_count, first_observed,
                       last_observed, time_spread_seconds, recorded_at}
";
        let mut params = BTreeMap::new();
        params.insert(
            "fact_ids".to_owned(),
            crate::knowledge_store::id_rows(fact_ids.iter().map(FactId::as_str)),
        );
        let result = self.run_query(script, params).map_err(|e| {
            StoreSnafu {
                message: e.to_string(),
            }
            .build()
        })?;

        let mut found = std::collections::HashMap::with_capacity(result.rows().len());
        for row in 0..result.rows().len() {
            // kanon:ignore RUST/no-result-unwrap-or-default — side-index read: empty default is safe for optional metadata
            let id_str = result.get_string(row, "fact_id").unwrap_or_default();
            let Ok(fact_id) = FactId::new(&id_str) else {
                // WHY: a stored id that no longer parses is unreachable by any
                // caller, which addresses facts by `FactId`. Skip rather than
                // fail the whole batch.
                tracing::warn!(fact_id = %id_str, "skipping undecodable fact id in multiplicity batch");
                continue;
            };
            let source_count_i64 = result.get_i64(row, "source_count").unwrap_or(0);
            let source_count = u32::try_from(source_count_i64).unwrap_or(0);
            // kanon:ignore RUST/no-result-unwrap-or-default — side-index read: empty default is safe for optional metadata
            let first_observed = result.get_string(row, "first_observed").unwrap_or_default();
            // kanon:ignore RUST/no-result-unwrap-or-default — side-index read: empty default is safe for optional metadata
            let last_observed = result.get_string(row, "last_observed").unwrap_or_default();
            let time_spread_seconds = result.get_i64(row, "time_spread_seconds").unwrap_or(0);
            // kanon:ignore RUST/no-result-unwrap-or-default — side-index read: empty default is safe for optional metadata
            let recorded_at = result.get_string(row, "recorded_at").unwrap_or_default();

            found.insert(
                id_str,
                FactMultiplicity {
                    fact_id,
                    source_count,
                    first_observed,
                    last_observed,
                    time_spread_seconds,
                    recorded_at,
                },
            );
        }
        Ok(found)
    }

    /// Ensure the `consolidation_audit` relation carries the owner (`nous_id`)
    /// column, backfilling legacy rows when it does not (#6380).
    ///
    /// Runs `::columns` (a sys-op), so it can never live inside the
    /// consolidation commit's `MultiTransaction` — callers must run it before
    /// the transaction opens. Idempotent.
    pub(crate) fn ensure_consolidation_audit_owner_scope(&self) -> Result<(), ConsolidationError> {
        if self.consolidation_audit_has_nous_id()? {
            return Ok(());
        }

        self.run_mut_query(CONSOLIDATION_AUDIT_OWNER_BACKFILL_DDL, BTreeMap::new())
            .map_err(|e| {
                StoreSnafu {
                    message: e.to_string(),
                }
                .build()
            })?;
        Ok(())
    }

    fn consolidation_audit_has_nous_id(&self) -> Result<bool, ConsolidationError> {
        let result = self
            .run_query("::columns consolidation_audit", BTreeMap::new())
            .map_err(|e| {
                StoreSnafu {
                    message: e.to_string(),
                }
                .build()
            })?;

        for row in 0..result.row_count() {
            if result.get_string(row, "column").as_deref() == Some("nous_id") {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// Query the last consolidation timestamp from the audit trail.
    pub(crate) fn last_consolidation_time(
        &self,
        nous_id: &str,
    ) -> Result<Option<String>, ConsolidationError> {
        self.ensure_consolidation_audit_owner_scope()?;
        let script = r"
?[consolidated_at] := *consolidation_audit{nous_id: $nous_id, consolidated_at}
:sort -consolidated_at
:limit 1
";
        let mut params = BTreeMap::new();
        params.insert("nous_id".to_owned(), DataValue::Str(nous_id.into()));
        let result = self.run_query(script, params).map_err(|e| {
            StoreSnafu {
                message: e.to_string(),
            }
            .build()
        })?;

        if result.is_empty() {
            Ok(None)
        } else {
            Ok(Some(
                // kanon:ignore RUST/no-result-unwrap-or-default — optional timestamp: empty string yields Ok(None) upstream
                result.get_string(0, "consolidated_at").unwrap_or_default(),
            ))
        }
    }

    /// Delete `consolidation_audit` rows for `nous_id` recorded before `cutoff`.
    ///
    /// Returns `(examined, removed)`: the nous's total audit row count before
    /// the delete, and how many of those fell outside the retention window.
    ///
    /// WHY(#5674): the relation is append-only — one row per consolidation run,
    /// keyed on a fresh ULID — with no TTL and no row cap, so it grows for the
    /// life of the instance. `garbage_collect` in the binary crate's
    /// `KnowledgeMaintenanceExecutor` calls this on the scheduled graph-cleanup
    /// cadence.
    ///
    /// `cutoff` must be a timestamp in the same format as the stored
    /// `consolidated_at` values (see [`mneme::knowledge::format_timestamp`]).
    /// The comparison is lexicographic, which is the same ordering
    /// [`Self::last_consolidation_time`] already relies on for `:sort`.
    pub fn prune_consolidation_audit(
        &self,
        nous_id: &str,
        cutoff: &str,
    ) -> Result<(u64, u64), ConsolidationError> {
        self.ensure_consolidation_audit_owner_scope()?;

        let count_script = r"
?[id] := *consolidation_audit{id, nous_id: $nous_id}
";
        let mut count_params = BTreeMap::new();
        count_params.insert("nous_id".to_owned(), DataValue::Str(nous_id.into()));
        let examined = self
            .run_query(count_script, count_params)
            .map_err(|e| {
                StoreSnafu {
                    message: e.to_string(),
                }
                .build()
            })?
            .row_count();

        let expired_script = r"
?[id] := *consolidation_audit{id, nous_id: $nous_id, consolidated_at},
         consolidated_at < $cutoff
";
        let mut expired_params = BTreeMap::new();
        expired_params.insert("nous_id".to_owned(), DataValue::Str(nous_id.into()));
        expired_params.insert("cutoff".to_owned(), DataValue::Str(cutoff.into()));
        let removed = self
            .run_query(expired_script, expired_params)
            .map_err(|e| {
                StoreSnafu {
                    message: e.to_string(),
                }
                .build()
            })?
            .row_count();

        if removed > 0 {
            let rm_script = r"
?[id] := *consolidation_audit{id, nous_id: $nous_id, consolidated_at},
         consolidated_at < $cutoff
:rm consolidation_audit {id}
";
            let mut rm_params = BTreeMap::new();
            rm_params.insert("nous_id".to_owned(), DataValue::Str(nous_id.into()));
            rm_params.insert("cutoff".to_owned(), DataValue::Str(cutoff.into()));
            self.run_mut_query(rm_script, rm_params).map_err(|e| {
                StoreSnafu {
                    message: e.to_string(),
                }
                .build()
            })?;
        }

        Ok((
            u64::try_from(examined).unwrap_or(u64::MAX),
            u64::try_from(removed).unwrap_or(u64::MAX),
        ))
    }

    /// Run a full consolidation cycle for a nous.
    ///
    /// 1. Check rate limit
    /// 2. Find entity and community overflow candidates
    /// 3. Execute consolidation for each candidate
    ///
    /// If `dry_run` is true, reports candidates and proposed consolidations
    /// without executing mutations.
    #[instrument(skip(self, provider))]
    pub fn consolidate_knowledge(
        &self,
        provider: &dyn ConsolidationProvider,
        nous_id: &str,
        config: &ConsolidationConfig,
        dry_run: bool,
    ) -> Result<Vec<ConsolidationResult>, ConsolidationError> {
        if !dry_run {
            self.check_rate_limit(nous_id, config)?;
        }

        let mut results = Vec::new();

        for candidate in &self.find_entity_overflow_candidates(nous_id, config)? {
            results
                .push(self.execute_consolidation(provider, candidate, nous_id, config, dry_run)?);
        }

        for candidate in &self.find_community_overflow_candidates(nous_id, config)? {
            results
                .push(self.execute_consolidation(provider, candidate, nous_id, config, dry_run)?);
        }

        Ok(results)
    }

    /// Check whether the rate limit allows another consolidation cycle.
    fn check_rate_limit(
        &self,
        nous_id: &str,
        config: &ConsolidationConfig,
    ) -> Result<(), ConsolidationError> {
        if let Some(last_time) = self.last_consolidation_time(nous_id)?
            && let Some(last_ts) = crate::knowledge::parse_timestamp(&last_time)
        {
            let now = jiff::Timestamp::now();
            if let Ok(span) = now.since(last_ts) {
                let total_minutes = i64::from(span.get_hours()) * 60 + span.get_minutes();
                #[expect(
                    clippy::as_conversions,
                    clippy::cast_precision_loss,
                    reason = "total_minutes is an elapsed time value; precision loss is acceptable for rate-limit comparison"
                )]
                let elapsed_hours = (total_minutes as f64) / 60.0;
                if elapsed_hours < config.rate_limit_hours {
                    return Err(RateLimitedSnafu {
                        elapsed_hours,
                        min_hours: config.rate_limit_hours,
                    }
                    .build());
                }
            }
        }
        Ok(())
    }
}

/// LLM result plus the batch-local mapping needed to apply supersession.
struct LlmConsolidationResult {
    result: ConsolidationResult,
    supersession_batches: Vec<BatchSupersession>,
}

/// Source facts and canonical first output for one nonempty batch.
struct BatchSupersession {
    source_fact_ids: Arc<[FactId]>,
    consolidated_fact_index: usize,
}

/// Mean confidence across one consolidation batch's source facts.
///
/// WHY(#5853): every source fact is weighted equally — `SourceFact` carries
/// no other signal (explicit trust weight, corroboration count) to
/// differentiate one source from another within a batch, so an unweighted
/// arithmetic mean of the source confidences is the only scheme the data
/// supports without inventing a factor the batch does not actually carry.
///
/// INVARIANT: `batch_facts` (via `slice::chunks`) never yields an empty
/// batch for nonempty input, so the empty branch below is unreachable in
/// practice; it exists to keep this function total and to avoid a NaN
/// confidence reaching a persisted fact if that invariant is ever violated.
fn batch_mean_confidence(batch: &[SourceFact]) -> f64 {
    if batch.is_empty() {
        return 0.5;
    }
    #[expect(
        clippy::as_conversions,
        clippy::cast_precision_loss,
        reason = "batch length is bounded by config.batch_limit (a small batch size); precision loss is acceptable for an averaged confidence value"
    )]
    let count = batch.len() as f64;
    batch.iter().map(|source| source.confidence).sum::<f64>() / count
}

/// Run the LLM consolidation prompt across batches and collect results.
fn run_llm_consolidation(
    provider: &dyn ConsolidationProvider,
    facts: &[SourceFact],
    config: &ConsolidationConfig,
) -> Result<LlmConsolidationResult, ConsolidationError> {
    let batches = batch_facts(facts, config.batch_limit);
    let mut all_consolidated = Vec::new();
    let mut all_superseded = Vec::new();
    let mut supersession_batches = Vec::new();

    for batch in &batches {
        let system = consolidation_system_prompt();
        let user_msg = consolidation_user_message(batch);

        let response = provider.consolidate(system, &user_msg)?;
        let entries = parse_consolidation_response(&response)?;
        // WHY(#5847): lifecycle supersession is single-valued. Preserve the
        // existing first-output behavior within a batch while retaining the
        // correct first output separately for every batch in the run.
        let first_consolidated_fact_index = all_consolidated.len();

        // WHY(#5694): this metadata is per-batch, not per-fact — every output
        // fact in the batch carries the same values. Building each one as an
        // `Arc<[_]>` once means the loop below clones a pointer per output
        // fact instead of a full copy of seven batch-length vectors.
        let batch_fact_ids: Arc<[FactId]> = batch.iter().map(|s| s.id.clone()).collect();
        // WHY(#3634): preserve source recorded_at timestamps so multiplicity
        // metadata (time-spread, first/last observation) can be computed
        // downstream. Aligned by index to `batch_fact_ids`.
        let batch_recorded_ats: Arc<[String]> =
            batch.iter().map(|s| s.recorded_at.clone()).collect();
        // WHY(#4660): carry source policy metadata through the batch so the
        // conservative merge in `persist_consolidated_facts` can enforce scope,
        // project, sensitivity, and visibility boundaries.
        let batch_scopes: Arc<[Option<MemoryScope>]> = batch.iter().map(|s| s.scope).collect();
        let batch_project_ids: Arc<[Option<String>]> =
            batch.iter().map(|s| s.project_id.clone()).collect();
        let batch_sensitivities: Arc<[FactSensitivity]> =
            batch.iter().map(|s| s.sensitivity).collect();
        let batch_visibilities: Arc<[Visibility]> = batch.iter().map(|s| s.visibility).collect();
        let batch_session_ids: Arc<[Option<String>]> =
            batch.iter().map(|s| s.source_session_id.clone()).collect();
        // WHY(#5853): a consolidated fact's confidence reflects the sources
        // it was built from rather than a fixed constant.
        let batch_confidence = batch_mean_confidence(batch);

        for entry in &entries {
            all_consolidated.push(ConsolidatedFact {
                content: entry.content.clone(),
                confidence: batch_confidence,
                tier: "inferred".to_owned(),
                source_fact_ids: Arc::clone(&batch_fact_ids),
                source_recorded_ats: Arc::clone(&batch_recorded_ats),
                source_scopes: Arc::clone(&batch_scopes),
                source_project_ids: Arc::clone(&batch_project_ids),
                source_sensitivities: Arc::clone(&batch_sensitivities),
                source_visibilities: Arc::clone(&batch_visibilities),
                source_session_ids: Arc::clone(&batch_session_ids),
            });
        }

        // WHY(#5849): A batch that produces zero consolidated outputs must not
        // supersede its source facts. Marking originals as superseded with no
        // replacement would silently destroy knowledge.
        if entries.is_empty() {
            tracing::warn!(
                batch_size = batch.len(),
                "LLM consolidation returned no outputs for batch; skipping supersession to avoid data loss"
            );
        } else {
            all_superseded.extend(batch_fact_ids.iter().cloned());
            supersession_batches.push(BatchSupersession {
                source_fact_ids: batch_fact_ids,
                consolidated_fact_index: first_consolidated_fact_index,
            });
        }
    }

    Ok(LlmConsolidationResult {
        result: ConsolidationResult {
            original_count: facts.len(),
            consolidated_count: all_consolidated.len(),
            consolidated_facts: all_consolidated,
            superseded_fact_ids: all_superseded,
        },
        supersession_batches,
    })
}

// ---------------------------------------------------------------------
// Idempotency keys and the atomic write plan (#5311)
// ---------------------------------------------------------------------

/// Length-prefixed SHA-256 field update, matching the deterministic-ID idiom
/// in `extract::engine` — length prefixes keep field concatenation
/// unambiguous.
fn hash_field(hasher: &mut Sha256, field: &str) {
    hasher.update(u64::try_from(field.len()).unwrap_or(u64::MAX).to_le_bytes());
    hasher.update(field.as_bytes());
}

/// Lowercase-hex encode a SHA-256 digest.
fn hex_digest(digest: &[u8]) -> String {
    use std::fmt::Write as _;
    let mut out = String::with_capacity(digest.len() * 2);
    for byte in digest {
        // WHY discard the Result: writing hex digits into a String never
        // fails, and `expect_used` is denied crate-wide.
        let _ = write!(out, "{byte:02x}");
    }
    out
}

/// Fingerprint of the consolidation knobs that shape a run's output, folded
/// into the run's idempotency key (#5311). `rate_limit_hours` participates
/// via its bit pattern so the fingerprint is stable across platforms.
fn consolidation_config_fingerprint(config: &ConsolidationConfig) -> String {
    format!(
        "v1:{}:{}:{}:{}:{:x}",
        config.entity_fact_threshold,
        config.community_fact_threshold,
        config.min_age_days,
        config.batch_limit,
        config.rate_limit_hours.to_bits()
    )
}

/// Derive the idempotency key for one consolidation run: a SHA-256 over the
/// nous, the trigger, the consolidation config fingerprint, and the sorted
/// source fact IDs (#5311). The same inputs always yield the same key, so a
/// retry after a failure identifies the run it resumes.
fn consolidation_run_key(
    candidate: &ConsolidationCandidate,
    source_fact_ids: &[FactId],
    nous_id: &str,
    config: &ConsolidationConfig,
) -> String {
    let mut sorted: Vec<&str> = source_fact_ids.iter().map(FactId::as_str).collect();
    sorted.sort_unstable();

    let mut hasher = Sha256::new();
    hash_field(&mut hasher, nous_id);
    hash_field(&mut hasher, candidate.trigger.trigger_type());
    hash_field(&mut hasher, &candidate.trigger.trigger_id());
    hash_field(&mut hasher, &consolidation_config_fingerprint(config));
    for id in sorted {
        hash_field(&mut hasher, id);
    }
    hex_digest(&hasher.finalize())
}

/// The audit row ID recording one consolidation run. The row commits in the
/// same transaction as the facts and supersessions, so its presence proves
/// the whole run landed — this is the idempotency record a retry checks.
fn audit_id_for_run_key(run_key: &str) -> String {
    format!("cons-audit-{run_key}")
}

/// Derive the deterministic ID of one consolidated output (#5311): the run
/// key, the output batch's sorted source fact IDs (outputs from different
/// batches of one run must not collide), and the output's ordinal within its
/// batch (multiple outputs from one batch must not collide either).
fn consolidated_fact_id(
    run_key: &str,
    consolidated: &ConsolidatedFact,
    ordinal: usize,
) -> Result<FactId, ConsolidationError> {
    let mut sorted: Vec<&str> = consolidated
        .source_fact_ids
        .iter()
        .map(FactId::as_str)
        .collect();
    sorted.sort_unstable();

    let mut hasher = Sha256::new();
    hash_field(&mut hasher, run_key);
    for id in sorted {
        hash_field(&mut hasher, id);
    }
    hash_field(&mut hasher, &ordinal.to_string());
    FactId::new(format!("cons-{}", hex_digest(&hasher.finalize()))).map_err(|e| {
        StoreSnafu {
            message: e.to_string(),
        }
        .build()
    })
}

/// The staged rows for one consolidated output: the fact itself plus its
/// multiplicity and provenance side-index rows.
struct PlannedConsolidatedFact {
    fact: crate::knowledge::Fact,
    multiplicity: FactMultiplicity,
    provenance: ConsolidationProvenanceRow,
}

/// Build one consolidated output's fact row and side-index rows (#5311).
///
/// `ordinal` is the output's position among the outputs sharing its batch's
/// source set — the component of the derived ID that keeps multiple outputs
/// from one batch distinct.
fn plan_consolidated_fact(
    consolidated: &ConsolidatedFact,
    ordinal: usize,
    nous_id: &str,
    run_key: &str,
    now: jiff::Timestamp,
    now_str: &str,
) -> Result<PlannedConsolidatedFact, ConsolidationError> {
    // WHY(#4660): conservative merge of source policy metadata keeps
    // a confidential or project-scoped input from silently becoming
    // public/global.
    let merged = merge_consolidated_metadata(consolidated)?;
    let new_id = consolidated_fact_id(run_key, consolidated, ordinal)?;
    let project_id = match merged.project_id {
        Some(ref raw) => Some(ProjectId::from_sha256_hex(raw).map_err(|e| {
            StoreSnafu {
                message: format!("consolidated source has invalid project_id: {e}"),
            }
            .build()
        })?),
        None => None,
    };
    let fact = crate::knowledge::Fact {
        id: new_id.clone(),
        nous_id: nous_id.to_owned(),
        content: consolidated.content.clone(),
        fact_type: "observation".to_owned(),
        scope: merged.scope,
        project_id,
        temporal: FactTemporal {
            valid_from: now,
            valid_to: crate::knowledge::far_future(),
            recorded_at: now,
        },
        provenance: FactProvenance {
            confidence: consolidated.confidence,
            tier: EpistemicTier::Inferred,
            // Source session IDs are preserved in the side-index below;
            // the single-valued field intentionally stays None because
            // a consolidated fact has multiple sources.
            source_session_id: None,
            stability_hours: crate::knowledge::FactType::Observation.base_stability_hours(),
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
        sensitivity: merged.sensitivity,
        visibility: merged.visibility,
    };

    // WHY(#3634): record multiplicity metadata in the side-index so
    // downstream recall and conflict resolution can weight a
    // consolidated fact by how many independent observations
    // converged on it.
    let multiplicity = compute_multiplicity(&new_id, consolidated, now_str);

    // WHY(#4660): keep source fact IDs and source session IDs
    // inspectable from the consolidated fact's provenance side-index.
    let source_fact_ids_json = serde_json::to_string(
        &consolidated
            .source_fact_ids
            .iter()
            .map(FactId::as_str)
            .collect::<Vec<_>>(),
    )
    .map_err(|e| {
        StoreSnafu {
            message: format!("failed to serialize source fact IDs: {e}"),
        }
        .build()
    })?;
    let source_session_ids: Vec<&str> = consolidated
        .source_session_ids
        .iter()
        .filter_map(|s| s.as_deref())
        .collect();
    let source_session_ids_json = serde_json::to_string(&source_session_ids).map_err(|e| {
        StoreSnafu {
            message: format!("failed to serialize source session IDs: {e}"),
        }
        .build()
    })?;

    Ok(PlannedConsolidatedFact {
        fact,
        multiplicity,
        provenance: ConsolidationProvenanceRow {
            fact_id: new_id,
            source_fact_ids_json,
            source_session_ids_json,
        },
    })
}

/// Build the fully validated write plan for one consolidation (#5311).
///
/// Every fallible step that does not need the store — the policy-metadata
/// merge, deterministic ID derivation, provenance/audit JSON serialization,
/// and the batch-output index validation — runs here, before the commit's
/// transaction opens, so a planning failure leaves the store untouched by
/// construction.
fn build_consolidation_write_plan(
    candidate: &ConsolidationCandidate,
    result: &ConsolidationResult,
    supersession_batches: &[BatchSupersession],
    nous_id: &str,
    run_key: &str,
) -> Result<ConsolidationWritePlan, ConsolidationError> {
    let now = jiff::Timestamp::now();
    let now_str = crate::knowledge::format_timestamp(&now);

    let mut facts = Vec::with_capacity(result.consolidated_facts.len());
    let mut multiplicities = Vec::with_capacity(result.consolidated_facts.len());
    let mut provenance = Vec::with_capacity(result.consolidated_facts.len());
    // WHY(#5694): outputs from one batch share their source set, so the
    // ordinal among same-source-set outputs is what distinguishes them in the
    // derived ID. Ordinal assignment is stable within one run; across runs it
    // only matters after an abort, which left nothing behind.
    let mut seen_source_sets: Vec<&[FactId]> = Vec::with_capacity(result.consolidated_facts.len());
    for consolidated in &result.consolidated_facts {
        let this_set: &[FactId] = &consolidated.source_fact_ids;
        let ordinal = seen_source_sets
            .iter()
            .filter(|prior| ***prior == *this_set)
            .count();
        let planned =
            plan_consolidated_fact(consolidated, ordinal, nous_id, run_key, now, &now_str)?;
        seen_source_sets.push(&consolidated.source_fact_ids);
        facts.push(planned.fact);
        multiplicities.push(planned.multiplicity);
        provenance.push(planned.provenance);
    }

    let new_fact_ids: Vec<FactId> = facts.iter().map(|fact| fact.id.clone()).collect();
    let mut supersessions = Vec::new();
    for batch in supersession_batches {
        let superseding_id = new_fact_ids
            .get(batch.consolidated_fact_index)
            .ok_or_else(|| {
                StoreSnafu {
                    message: format!(
                        "missing planned consolidated fact at batch output index {} ({} IDs planned)",
                        batch.consolidated_fact_index,
                        new_fact_ids.len()
                    ),
                }
                .build()
            })?;
        for original_id in batch.source_fact_ids.iter() {
            supersessions.push((original_id.clone(), superseding_id.clone()));
        }
    }

    let original_ids_json = serde_json::to_string(
        &result
            .superseded_fact_ids
            .iter()
            .map(FactId::as_str)
            .collect::<Vec<_>>(),
    )
    .map_err(|e| {
        StoreSnafu {
            message: format!("failed to serialize original fact IDs for audit: {e}"),
        }
        .build()
    })?;
    let consolidated_ids_json =
        serde_json::to_string(&new_fact_ids.iter().map(FactId::as_str).collect::<Vec<_>>())
            .map_err(|e| {
                StoreSnafu {
                    message: format!("failed to serialize consolidated fact IDs for audit: {e}"),
                }
                .build()
            })?;

    Ok(ConsolidationWritePlan {
        facts,
        multiplicities,
        provenance,
        supersessions,
        audit: ConsolidationAuditRecord {
            id: audit_id_for_run_key(run_key),
            nous_id: nous_id.to_owned(),
            trigger_type: candidate.trigger.trigger_type().to_owned(),
            trigger_id: candidate.trigger.trigger_id(),
            original_count: result.original_count,
            consolidated_count: result.consolidated_count,
            original_fact_ids: original_ids_json,
            consolidated_fact_ids: consolidated_ids_json,
            consolidated_at: now_str.clone(),
        },
        now: now_str,
    })
}

/// Policy-merged metadata for a consolidated fact (#4660).
///
/// Produced by [`merge_consolidated_metadata`] from the source facts that
/// contributed to a single consolidated output.
#[derive(Debug, Clone)]
struct MergedSourceMetadata {
    /// Conservative scope: only set when all sources agree.
    pub scope: Option<MemoryScope>,
    /// Conservative project partition: only set when all sources agree.
    pub project_id: Option<String>,
    /// Most restrictive sensitivity across sources.
    pub sensitivity: FactSensitivity,
    /// Most restrictive visibility across sources.
    pub visibility: Visibility,
}

/// Merge source-fact policy metadata into conservative consolidated metadata.
///
/// # Policy (#4660)
///
/// - **Sensitivity:** take the maximum (most restrictive) value. A single
///   confidential source makes the whole output confidential.
/// - **Visibility:** take the minimum (most restrictive) value. A single
///   private source keeps the output private.
/// - **Scope:** all non-null source scopes must match exactly. Mixed scopes
///   are refused because there is no safe single scope that preserves every
///   source's boundary.
/// - **Project ID:** all non-null source project IDs must match exactly.
///   Mixed project IDs are refused to avoid cross-project leakage.
/// - **Source sessions:** collect distinct non-null session IDs for provenance.
fn merge_consolidated_metadata(
    consolidated: &ConsolidatedFact,
) -> Result<MergedSourceMetadata, ConsolidationError> {
    validate_source_metadata_lengths(consolidated)?;

    let mut sensitivity = FactSensitivity::Public;
    let mut visibility: Option<Visibility> = None;
    let mut scopes = std::collections::HashSet::new();
    let mut project_ids = std::collections::BTreeSet::new();

    for (((scope, project_id), src_sensitivity), src_visibility) in consolidated
        .source_scopes
        .iter()
        .zip(consolidated.source_project_ids.iter())
        .zip(consolidated.source_sensitivities.iter())
        .zip(consolidated.source_visibilities.iter())
    {
        sensitivity = sensitivity.max(*src_sensitivity);

        visibility = Some(match visibility {
            Some(cur) => cur.min(*src_visibility),
            None => *src_visibility,
        });

        if let Some(scope) = scope {
            scopes.insert(*scope);
        }
        if let Some(project_id) = project_id {
            project_ids.insert(project_id.clone());
        }
    }

    let scope = match scopes.len() {
        0 => None,
        1 => scopes.into_iter().next(),
        _ => {
            return Err(IncompatibleSourcesSnafu {
                reason: "mixed memory scopes in consolidation sources".to_owned(),
            }
            .build());
        }
    };

    let project_id = match project_ids.len() {
        0 => None,
        1 => project_ids.into_iter().next(),
        _ => {
            return Err(IncompatibleSourcesSnafu {
                reason: "mixed project IDs in consolidation sources".to_owned(),
            }
            .build());
        }
    };

    Ok(MergedSourceMetadata {
        scope,
        project_id,
        sensitivity,
        visibility: visibility.unwrap_or(Visibility::Private),
    })
}

fn validate_source_metadata_lengths(
    consolidated: &ConsolidatedFact,
) -> Result<(), ConsolidationError> {
    let expected = consolidated.source_fact_ids.len();
    for (field, actual) in [
        ("source_scopes", consolidated.source_scopes.len()),
        ("source_project_ids", consolidated.source_project_ids.len()),
        (
            "source_sensitivities",
            consolidated.source_sensitivities.len(),
        ),
        (
            "source_visibilities",
            consolidated.source_visibilities.len(),
        ),
        ("source_session_ids", consolidated.source_session_ids.len()),
    ] {
        if actual != expected {
            return Err(IncompatibleSourcesSnafu {
                reason: format!(
                    "{field} length {actual} does not match source_fact_ids length {expected}"
                ),
            }
            .build());
        }
    }
    Ok(())
}

/// Compute multiplicity metadata for a consolidated fact (#3634).
///
/// `source_count` is the number of independent source fact IDs. The time
/// window spans the earliest to latest `recorded_at` across those sources;
/// when timestamps are unavailable or unparseable we fall back to `now`
/// for both ends (zero spread) so the record remains well-formed.
fn compute_multiplicity(
    new_id: &FactId,
    consolidated: &ConsolidatedFact,
    now: &str,
) -> FactMultiplicity {
    let source_count = u32::try_from(consolidated.source_fact_ids.len()).unwrap_or(u32::MAX);
    let parsed: Vec<jiff::Timestamp> = consolidated
        .source_recorded_ats
        .iter()
        .filter_map(|s| crate::knowledge::parse_timestamp(s))
        .collect();
    let (first_observed, last_observed, time_spread_seconds) =
        match (parsed.iter().min().copied(), parsed.iter().max().copied()) {
            (Some(min_ts), Some(max_ts)) => {
                let spread = max_ts.since(min_ts).map_or(0_i64, |span| {
                    i64::from(span.get_hours())
                        .saturating_mul(3600)
                        .saturating_add(span.get_minutes().saturating_mul(60))
                        .saturating_add(span.get_seconds())
                });
                (
                    crate::knowledge::format_timestamp(&min_ts),
                    crate::knowledge::format_timestamp(&max_ts),
                    spread,
                )
            }
            _ => (now.to_owned(), now.to_owned(), 0_i64),
        };
    FactMultiplicity {
        fact_id: new_id.clone(),
        source_count,
        first_observed,
        last_observed,
        time_spread_seconds,
        recorded_at: now.to_owned(),
    }
}

/// Parse fact rows from query results into [`SourceFact`] records.
fn parse_fact_rows(rows: &[Vec<DataValue>]) -> crate::error::Result<Vec<SourceFact>> {
    rows.iter()
        .enumerate()
        .map(|(idx, row)| parse_fact_row(row, idx))
        .collect()
}

fn parse_fact_row(row: &[DataValue], idx: usize) -> crate::error::Result<SourceFact> {
    if row.len() < 9 {
        return Err(crate::error::ConversionSnafu {
            message: format!(
                "consolidation source row {idx}: expected 9 columns, got {}",
                row.len()
            ),
        }
        .build());
    }

    let id_raw = required_str(row, 0, "fact_id", idx)?;
    let id = FactId::new(id_raw.to_owned()).map_err(|e| {
        crate::error::ConversionSnafu {
            message: format!("consolidation source row {idx}: invalid fact_id '{id_raw}': {e}"),
        }
        .build()
    })?;
    let content = required_str(row, 1, "content", idx)?.to_owned();
    let confidence = row.get(2).and_then(DataValue::get_float).ok_or_else(|| {
        crate::error::ConversionSnafu {
            message: format!("consolidation source row {idx}: missing confidence"),
        }
        .build()
    })?;
    let recorded_at = required_str(row, 3, "recorded_at", idx)?.to_owned();
    let scope = optional_str(row, 4, "scope", idx)?
        .map(str::parse::<MemoryScope>)
        .transpose()
        .map_err(|e| {
            crate::error::ConversionSnafu {
                message: format!("consolidation source row {idx}: invalid scope: {e}"),
            }
            .build()
        })?;
    let project_id = optional_str(row, 5, "project_id", idx)?.map(str::to_owned);
    let sensitivity_raw = required_str(row, 6, "sensitivity", idx)?;
    let sensitivity = sensitivity_raw.parse::<FactSensitivity>().map_err(|e| {
        crate::error::ConversionSnafu {
            message: format!("consolidation source row {idx}: invalid sensitivity: {e}"),
        }
        .build()
    })?;
    let visibility_raw = required_str(row, 7, "visibility", idx)?;
    let visibility = visibility_raw.parse::<Visibility>().map_err(|e| {
        crate::error::ConversionSnafu {
            message: format!("consolidation source row {idx}: invalid visibility: {e}"),
        }
        .build()
    })?;
    let source_session_id = optional_str(row, 8, "source_session_id", idx)?.map(str::to_owned);

    Ok(SourceFact {
        id,
        content,
        confidence,
        recorded_at,
        scope,
        project_id,
        sensitivity,
        visibility,
        source_session_id,
    })
}

fn required_str<'a>(
    row: &'a [DataValue],
    index: usize,
    name: &str,
    row_idx: usize,
) -> crate::error::Result<&'a str> {
    row.get(index).and_then(DataValue::get_str).ok_or_else(|| {
        crate::error::ConversionSnafu {
            message: format!("consolidation source row {row_idx}: missing {name}"),
        }
        .build()
    })
}

fn optional_str<'a>(
    row: &'a [DataValue],
    index: usize,
    name: &str,
    row_idx: usize,
) -> crate::error::Result<Option<&'a str>> {
    match row.get(index) {
        Some(DataValue::Null) | None => Ok(None),
        Some(value) => value.get_str().map(Some).ok_or_else(|| {
            crate::error::ConversionSnafu {
                message: format!("consolidation source row {row_idx}: invalid {name}"),
            }
            .build()
        }),
    }
}

#[cfg(all(test, feature = "mneme-engine"))]
#[path = "engine_tests.rs"]
mod engine_tests;
