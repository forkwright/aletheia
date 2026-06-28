//! Engine integration for fact consolidation.
//!
//! Implements consolidation operations on `KnowledgeStore`: candidate
//! identification, LLM-driven consolidation execution, and audit trail.
use std::collections::BTreeMap;
use tracing::instrument;

use super::{
    CLUSTER_FACTS_FOR_CONSOLIDATION, COMMUNITY_OVERFLOW_CANDIDATES, CONSOLIDATION_AUDIT_DDL,
    ConsolidatedFact, ConsolidationAuditRecord, ConsolidationCandidate, ConsolidationConfig,
    ConsolidationError, ConsolidationProvider, ConsolidationResult, ConsolidationTrigger,
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

fn datalog_string_literal(value: &str) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "\"\"".to_owned())
}

fn datalog_row(values: &[&str]) -> String {
    format!(
        "[{}]",
        values
            .iter()
            .map(|value| datalog_string_literal(value))
            .collect::<Vec<_>>()
            .join(", ")
    )
}

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
        self.run_mut_query(super::CONSOLIDATION_AUDIT_RECORDED_AT_DDL, BTreeMap::new())?;
        self.run_mut_query(
            super::CONSOLIDATION_AUDIT_NOUS_RECORDED_AT_DDL,
            BTreeMap::new(),
        )?;
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

        let result = run_llm_consolidation(provider, &facts, config)?;

        if dry_run {
            return Ok(result);
        }

        let new_fact_ids = self.persist_consolidated_facts(&result, nous_id)?;
        self.supersede_originals(&result, &new_fact_ids)?;
        self.write_audit_record(candidate, &result, &new_fact_ids, nous_id)?;

        Ok(result)
    }

    /// Insert consolidated facts into the store.
    fn persist_consolidated_facts(
        &self,
        result: &ConsolidationResult,
        nous_id: &str,
    ) -> Result<Vec<FactId>, ConsolidationError> {
        let now = jiff::Timestamp::now();
        let far_future = crate::knowledge::far_future();
        let now_str = crate::knowledge::format_timestamp(&now);
        let mut new_fact_ids = Vec::new();

        for consolidated in &result.consolidated_facts {
            // WHY (#4660): conservative merge of source policy metadata keeps
            // a confidential or project-scoped input from silently becoming
            // public/global.
            let merged = merge_consolidated_metadata(consolidated)?;
            let new_id = FactId::new(koina::ulid::Ulid::new().to_string()).map_err(|e| {
                StoreSnafu {
                    message: e.to_string(),
                }
                .build()
            })?;
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
                    valid_to: far_future,
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
            self.insert_fact(&fact).map_err(|e| {
                StoreSnafu {
                    message: e.to_string(),
                }
                .build()
            })?;

            // WHY (#3634): record multiplicity metadata in the side-index so
            // downstream recall and conflict resolution can weight a
            // consolidated fact by how many independent observations
            // converged on it. Failing to record multiplicity must not
            // prevent the fact from being persisted, but the error should
            // surface to the caller.
            let multiplicity = compute_multiplicity(&new_id, consolidated, &now_str);
            self.record_fact_multiplicity(&multiplicity)?;

            // WHY (#4660): keep source fact IDs and source session IDs
            // inspectable from the consolidated fact's provenance side-index.
            self.record_consolidation_provenance(&new_id, consolidated)?;

            new_fact_ids.push(new_id);
        }
        Ok(new_fact_ids)
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

    /// Record the source fact/session provenance for a consolidated fact.
    fn record_consolidation_provenance(
        &self,
        fact_id: &FactId,
        consolidated: &ConsolidatedFact,
    ) -> Result<(), ConsolidationError> {
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

        let script = r"
?[consolidated_fact_id, source_fact_ids, source_session_ids] <-
    [[$fact_id, $source_fact_ids, $source_session_ids]]

:put consolidation_provenance {
    consolidated_fact_id => source_fact_ids, source_session_ids
}
";
        let mut params = BTreeMap::new();
        params.insert(
            "fact_id".to_owned(),
            DataValue::Str(fact_id.as_str().into()),
        );
        params.insert(
            "source_fact_ids".to_owned(),
            DataValue::Str(source_fact_ids_json.into()),
        );
        params.insert(
            "source_session_ids".to_owned(),
            DataValue::Str(source_session_ids_json.into()),
        );
        self.run_mut_query(script, params).map(|_| ()).map_err(|e| {
            StoreSnafu {
                message: e.to_string(),
            }
            .build()
        })
    }

    /// Persist a `FactMultiplicity` record for a consolidated fact (#3634).
    fn record_fact_multiplicity(
        &self,
        record: &FactMultiplicity,
    ) -> Result<(), ConsolidationError> {
        let script = r"
?[fact_id, source_count, first_observed, last_observed, time_spread_seconds, recorded_at] <-
    [[$fact_id, $source_count, $first_observed, $last_observed, $time_spread_seconds, $recorded_at]]

:put fact_multiplicity {fact_id => source_count, first_observed, last_observed,
                        time_spread_seconds, recorded_at}
";
        let mut params = BTreeMap::new();
        let str_val = |s: &str| DataValue::Str(s.into());
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
        self.run_mut_query(script, params).map_err(|e| {
            StoreSnafu {
                message: e.to_string(),
            }
            .build()
        })?;
        Ok(())
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
        let script = r"
?[source_count, first_observed, last_observed, time_spread_seconds, recorded_at] :=
    *fact_multiplicity{fact_id: $fact_id, source_count, first_observed,
                       last_observed, time_spread_seconds, recorded_at}
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

        let source_count_i64 = result.get_i64(0, "source_count").unwrap_or(0);
        let source_count = u32::try_from(source_count_i64).unwrap_or(0);
        // kanon:ignore RUST/no-result-unwrap-or-default — side-index read: empty default is safe for optional metadata
        let first_observed = result.get_string(0, "first_observed").unwrap_or_default();
        // kanon:ignore RUST/no-result-unwrap-or-default — side-index read: empty default is safe for optional metadata
        let last_observed = result.get_string(0, "last_observed").unwrap_or_default();
        let time_spread_seconds = result.get_i64(0, "time_spread_seconds").unwrap_or(0);
        // kanon:ignore RUST/no-result-unwrap-or-default — side-index read: empty default is safe for optional metadata
        let recorded_at = result.get_string(0, "recorded_at").unwrap_or_default();

        Ok(Some(FactMultiplicity {
            fact_id: fact_id.clone(),
            source_count,
            first_observed,
            last_observed,
            time_spread_seconds,
            recorded_at,
        }))
    }

    /// Mark original facts as superseded.
    fn supersede_originals(
        &self,
        result: &ConsolidationResult,
        new_fact_ids: &[FactId],
    ) -> Result<(), ConsolidationError> {
        let now_str = crate::knowledge::format_timestamp(&jiff::Timestamp::now());
        let superseding_id = new_fact_ids.first().map(FactId::as_str).unwrap_or_default();

        for original_id in &result.superseded_fact_ids {
            self.supersede_fact_by_id(original_id, superseding_id, &now_str)
                .map_err(|e| {
                    StoreSnafu {
                        message: e.to_string(),
                    }
                    .build()
                })?;
        }
        Ok(())
    }

    /// Write an audit trail record for a consolidation.
    fn write_audit_record(
        &self,
        candidate: &ConsolidationCandidate,
        result: &ConsolidationResult,
        new_fact_ids: &[FactId],
        nous_id: &str,
    ) -> Result<(), ConsolidationError> {
        let now_str = crate::knowledge::format_timestamp(&jiff::Timestamp::now());
        let audit_id = koina::ulid::Ulid::new().to_string();
        let original_ids_json = serde_json::to_string(
            &result
                .superseded_fact_ids
                .iter()
                .map(FactId::as_str)
                .collect::<Vec<_>>(),
        )
        .unwrap_or_else(|_| "[]".to_owned());
        let consolidated_ids_json =
            serde_json::to_string(&new_fact_ids.iter().map(FactId::as_str).collect::<Vec<_>>())
                .unwrap_or_else(|_| "[]".to_owned());

        self.record_consolidation_audit(&ConsolidationAuditRecord {
            id: audit_id,
            nous_id: nous_id.to_owned(),
            trigger_type: candidate.trigger.trigger_type().to_owned(),
            trigger_id: candidate.trigger.trigger_id(),
            original_count: result.original_count,
            consolidated_count: result.consolidated_count,
            original_fact_ids: original_ids_json,
            consolidated_fact_ids: consolidated_ids_json,
            consolidated_at: now_str,
        })
        .map_err(|e| {
            StoreSnafu {
                message: e.to_string(),
            }
            .build()
        })
    }

    /// Mark a fact as superseded by ID, setting `valid_to` and `superseded_by`.
    fn supersede_fact_by_id(
        &self,
        fact_id: &FactId,
        superseding_id: &str,
        now: &str,
    ) -> crate::error::Result<()> {
        let script = r"
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
        let mut params = BTreeMap::new();
        params.insert("id".to_owned(), DataValue::Str(fact_id.as_str().into()));
        params.insert("now".to_owned(), DataValue::Str(now.into()));
        params.insert(
            "superseding_id".to_owned(),
            DataValue::Str(superseding_id.into()),
        );
        self.run_mut_query(script, params)?;
        Ok(())
    }

    /// Record a consolidation audit entry.
    fn record_consolidation_audit(
        &self,
        record: &ConsolidationAuditRecord,
    ) -> crate::error::Result<()> {
        let script = r"
?[id, nous_id, trigger_type, trigger_id, original_count, consolidated_count,
   original_fact_ids, consolidated_fact_ids, consolidated_at] <-
    [[$id, $nous_id, $trigger_type, $trigger_id, $original_count, $consolidated_count,
      $original_fact_ids, $consolidated_fact_ids, $consolidated_at]]

:put consolidation_audit {id => nous_id, trigger_type, trigger_id, original_count,
                          consolidated_count, original_fact_ids,
                          consolidated_fact_ids, consolidated_at}
";
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
        self.run_mut_query(script, params)?;
        let recorded_at_index_script = r"
?[recorded_at, id, nous_id] <- [[$recorded_at, $id, $nous_id]]
:put consolidation_audit_recorded_at {recorded_at, id => nous_id}
";
        let mut index_params = BTreeMap::new();
        index_params.insert("id".to_owned(), DataValue::Str(record.id.clone().into()));
        index_params.insert(
            "nous_id".to_owned(),
            DataValue::Str(record.nous_id.clone().into()),
        );
        index_params.insert(
            "recorded_at".to_owned(),
            DataValue::Str(record.consolidated_at.clone().into()),
        );
        self.run_mut_query(recorded_at_index_script, index_params.clone())?;
        let nous_index_script = r"
?[nous_id, recorded_at, id, present] <- [[$nous_id, $recorded_at, $id, true]]
:put consolidation_audit_nous_recorded_at {nous_id, recorded_at, id => present}
";
        self.run_mut_query(nous_index_script, index_params)?;
        Ok(())
    }

    /// Query the last consolidation timestamp from the audit trail.
    pub(crate) fn last_consolidation_time(
        &self,
        nous_id: &str,
    ) -> Result<Option<String>, ConsolidationError> {
        let script = r"
?[recorded_at] := *consolidation_audit_nous_recorded_at{nous_id: $nous_id, recorded_at}
:sort -recorded_at
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
                result.get_string(0, "recorded_at").unwrap_or_default(),
            ))
        }
    }

    /// Prune consolidation audit rows older than `cutoff`.
    ///
    /// `cutoff` must use the same sortable timestamp format as
    /// `ConsolidationAuditRecord::consolidated_at`.
    pub fn prune_consolidation_audit_before(
        &self,
        cutoff: &str,
    ) -> Result<usize, ConsolidationError> {
        let mut params = BTreeMap::new();
        params.insert("cutoff".to_owned(), DataValue::Str(cutoff.into()));
        let rows = self
            .run_query(
                r"
?[recorded_at, id, nous_id] :=
    *consolidation_audit_recorded_at{recorded_at, id, nous_id},
    recorded_at < $cutoff
",
                params,
            )
            .map_err(|e| {
                StoreSnafu {
                    message: e.to_string(),
                }
                .build()
            })?;
        if rows.is_empty() {
            return Ok(0);
        }

        let mut prune_keys = Vec::<(String, String, String)>::new();
        for row in rows.rows() {
            let Some(recorded_at) = row.first().and_then(DataValue::get_str) else {
                continue;
            };
            let Some(id) = row.get(1).and_then(DataValue::get_str) else {
                continue;
            };
            let Some(nous_id) = row.get(2).and_then(DataValue::get_str) else {
                continue;
            };
            prune_keys.push((recorded_at.to_owned(), id.to_owned(), nous_id.to_owned()));
        }
        if prune_keys.is_empty() {
            return Ok(0);
        }

        let audit_rows = prune_keys
            .iter()
            .map(|(_, id, _)| datalog_row(&[id.as_str()]))
            .collect::<Vec<_>>()
            .join(", ");
        let recorded_at_index_rows = prune_keys
            .iter()
            .map(|(recorded_at, id, _)| datalog_row(&[recorded_at.as_str(), id.as_str()]))
            .collect::<Vec<_>>()
            .join(", ");
        let nous_index_rows = prune_keys
            .iter()
            .map(|(recorded_at, id, nous_id)| {
                datalog_row(&[nous_id.as_str(), recorded_at.as_str(), id.as_str()])
            })
            .collect::<Vec<_>>()
            .join(", ");

        self.run_mut_query(
            &format!("?[id] <- [{audit_rows}] :rm consolidation_audit {{id}}"),
            BTreeMap::new(),
        )
        .map_err(|e| {
            StoreSnafu {
                message: e.to_string(),
            }
            .build()
        })?;
        self.run_mut_query(
            &format!(
                "?[recorded_at, id] <- [{recorded_at_index_rows}] :rm consolidation_audit_recorded_at {{recorded_at, id}}"
            ),
            BTreeMap::new(),
        )
        .map_err(|e| {
            StoreSnafu {
                message: e.to_string(),
            }
            .build()
        })?;
        self.run_mut_query(
            &format!(
                "?[nous_id, recorded_at, id] <- [{nous_index_rows}] :rm consolidation_audit_nous_recorded_at {{nous_id, recorded_at, id}}"
            ),
            BTreeMap::new(),
        )
        .map_err(|e| {
            StoreSnafu {
                message: e.to_string(),
            }
            .build()
        })?;

        Ok(prune_keys.len())
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

/// Run the LLM consolidation prompt across batches and collect results.
fn run_llm_consolidation(
    provider: &dyn ConsolidationProvider,
    facts: &[SourceFact],
    config: &ConsolidationConfig,
) -> Result<ConsolidationResult, ConsolidationError> {
    let batches = batch_facts(facts, config.batch_limit);
    let mut all_consolidated = Vec::new();
    let mut all_superseded = Vec::new();

    for batch in &batches {
        let system = consolidation_system_prompt();
        let user_msg = consolidation_user_message(batch);

        let response = provider.consolidate(system, &user_msg)?;
        let entries = parse_consolidation_response(&response)?;

        let batch_fact_ids: Vec<FactId> = batch.iter().map(|s| s.id.clone()).collect();
        // WHY (#3634): preserve source recorded_at timestamps so multiplicity
        // metadata (time-spread, first/last observation) can be computed
        // downstream. Aligned by index to `batch_fact_ids`.
        let batch_recorded_ats: Vec<String> = batch.iter().map(|s| s.recorded_at.clone()).collect();
        // WHY (#4660): carry source policy metadata through the batch so the
        // conservative merge in `persist_consolidated_facts` can enforce scope,
        // project, sensitivity, and visibility boundaries.
        let batch_scopes: Vec<Option<MemoryScope>> = batch.iter().map(|s| s.scope).collect();
        let batch_project_ids: Vec<Option<String>> =
            batch.iter().map(|s| s.project_id.clone()).collect();
        let batch_sensitivities: Vec<FactSensitivity> =
            batch.iter().map(|s| s.sensitivity).collect();
        let batch_visibilities: Vec<Visibility> = batch.iter().map(|s| s.visibility).collect();
        let batch_session_ids: Vec<Option<String>> =
            batch.iter().map(|s| s.source_session_id.clone()).collect();

        for entry in &entries {
            all_consolidated.push(ConsolidatedFact {
                content: entry.content.clone(),
                confidence: 0.95,
                tier: "inferred".to_owned(),
                // WHY: each ConsolidatedFact owns its source IDs, and we also
                // need the same IDs for all_superseded after the loop; Arc<[FactId]>
                // would eliminate this but ConsolidatedFact is part of the public API.
                source_fact_ids: batch_fact_ids.clone(),
                source_recorded_ats: batch_recorded_ats.clone(),
                source_scopes: batch_scopes.clone(),
                source_project_ids: batch_project_ids.clone(),
                source_sensitivities: batch_sensitivities.clone(),
                source_visibilities: batch_visibilities.clone(),
                source_session_ids: batch_session_ids.clone(),
            });
        }

        // WHY (#5849): A batch that produces zero consolidated outputs must not
        // supersede its source facts. Marking originals as superseded with no
        // replacement would silently destroy knowledge.
        if entries.is_empty() {
            tracing::warn!(
                batch_size = batch.len(),
                "LLM consolidation returned no outputs for batch; skipping supersession to avoid data loss"
            );
        } else {
            all_superseded.extend(batch_fact_ids);
        }
    }

    Ok(ConsolidationResult {
        original_count: facts.len(),
        consolidated_count: all_consolidated.len(),
        consolidated_facts: all_consolidated,
        superseded_fact_ids: all_superseded,
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
        .zip(&consolidated.source_project_ids)
        .zip(&consolidated.source_sensitivities)
        .zip(&consolidated.source_visibilities)
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
