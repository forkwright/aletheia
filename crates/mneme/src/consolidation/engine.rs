//! Engine integration for fact consolidation.
//!
//! Implements consolidation operations on `KnowledgeStore` — candidate
//! identification, LLM-driven consolidation execution, and audit trail.

use std::collections::BTreeMap;
use tracing::instrument;

use super::{
    CLUSTER_FACTS_FOR_CONSOLIDATION, COMMUNITY_OVERFLOW_CANDIDATES, CONSOLIDATION_AUDIT_DDL,
    ConsolidatedFact, ConsolidationAuditRecord, ConsolidationCandidate, ConsolidationConfig,
    ConsolidationError, ConsolidationProvider, ConsolidationResult, ConsolidationTrigger,
    ENTITY_FACTS_FOR_CONSOLIDATION, ENTITY_OVERFLOW_CANDIDATES, RateLimitedSnafu, StoreSnafu,
    age_cutoff, batch_facts, consolidation_system_prompt, consolidation_user_message,
    parse_consolidation_response,
};
use crate::engine::DataValue;
use crate::id::{EntityId, FactId};
use crate::knowledge::EpistemicTier;
use crate::knowledge_store::KnowledgeStore;

/// Convert a non-negative `i64` from a Datalog row to `usize`.
fn i64_as_usize(v: i64) -> usize {
    v.try_into().unwrap_or(0)
}

impl KnowledgeStore {
    /// Initialize the `consolidation_audit` relation. Called during schema setup.
    pub fn init_consolidation_audit(&self) -> crate::error::Result<()> {
        self.run_mut_query(CONSOLIDATION_AUDIT_DDL, BTreeMap::new())?;
        Ok(())
    }

    /// Find entity-overflow consolidation candidates.
    #[instrument(skip(self))]
    #[expect(
        clippy::cast_possible_wrap,
        reason = "entity_fact_threshold is small (typically 10), fits i64"
    )]
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
            DataValue::from(config.entity_fact_threshold as i64),
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
        for row in &result.rows {
            let entity_id_str = row[0].get_str().unwrap_or_default();
            let fact_count = i64_as_usize(row[1].get_int().unwrap_or(0));
            let entity_id = EntityId::from(entity_id_str);

            let facts = self
                .gather_entity_facts(nous_id, &entity_id, &cutoff)
                .map_err(|e| {
                    StoreSnafu {
                        message: e.to_string(),
                    }
                    .build()
                })?;

            let fact_ids: Vec<FactId> = facts.iter().map(|(id, _, _, _)| id.clone()).collect();

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
    #[instrument(skip(self))]
    #[expect(
        clippy::cast_possible_wrap,
        reason = "community_fact_threshold is small (typically 20), fits i64"
    )]
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
            DataValue::from(config.community_fact_threshold as i64),
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
        for row in &result.rows {
            let cluster_id = row[0].get_int().unwrap_or(-1);
            let fact_count = i64_as_usize(row[1].get_int().unwrap_or(0));

            let facts = self
                .gather_cluster_facts(nous_id, cluster_id, &cutoff)
                .map_err(|e| {
                    StoreSnafu {
                        message: e.to_string(),
                    }
                    .build()
                })?;

            let fact_ids: Vec<FactId> = facts.iter().map(|(id, _, _, _)| id.clone()).collect();

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
    ) -> crate::error::Result<Vec<(FactId, String, f64, String)>> {
        let mut params = BTreeMap::new();
        params.insert("nous_id".to_owned(), DataValue::Str(nous_id.into()));
        params.insert(
            "entity_id".to_owned(),
            DataValue::Str(entity_id.as_str().into()),
        );
        params.insert("cutoff".to_owned(), DataValue::Str(cutoff.into()));

        let result = self.run_query(ENTITY_FACTS_FOR_CONSOLIDATION, params)?;
        Ok(parse_fact_rows(&result.rows))
    }

    /// Gather eligible facts for a community cluster.
    fn gather_cluster_facts(
        &self,
        nous_id: &str,
        cluster_id: i64,
        cutoff: &str,
    ) -> crate::error::Result<Vec<(FactId, String, f64, String)>> {
        let mut params = BTreeMap::new();
        params.insert("nous_id".to_owned(), DataValue::Str(nous_id.into()));
        params.insert("cluster_id".to_owned(), DataValue::from(cluster_id));
        params.insert("cutoff".to_owned(), DataValue::Str(cutoff.into()));

        let result = self.run_query(CLUSTER_FACTS_FOR_CONSOLIDATION, params)?;
        Ok(parse_fact_rows(&result.rows))
    }

    /// Execute a consolidation: insert new facts, supersede originals, record audit.
    ///
    /// If `dry_run` is true, returns the proposed result without mutations.
    #[instrument(skip(self, provider, candidate))]
    pub fn execute_consolidation(
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
        self.write_audit_record(candidate, &result, &new_fact_ids)?;

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
        let mut new_fact_ids = Vec::new();

        for consolidated in &result.consolidated_facts {
            let new_id = FactId::from(ulid::Ulid::new().to_string());
            let fact = crate::knowledge::Fact {
                id: new_id.clone(),
                nous_id: nous_id.to_owned(),
                content: consolidated.content.clone(),
                confidence: consolidated.confidence,
                tier: EpistemicTier::Inferred,
                valid_from: now,
                valid_to: far_future,
                superseded_by: None,
                source_session_id: None,
                recorded_at: now,
                access_count: 0,
                last_accessed_at: None,
                stability_hours: crate::knowledge::FactType::Observation.base_stability_hours(),
                fact_type: "observation".to_owned(),
                is_forgotten: false,
                forgotten_at: None,
                forget_reason: None,
            };
            self.insert_fact(&fact).map_err(|e| {
                StoreSnafu {
                    message: e.to_string(),
                }
                .build()
            })?;
            new_fact_ids.push(new_id);
        }
        Ok(new_fact_ids)
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
    ) -> Result<(), ConsolidationError> {
        let now_str = crate::knowledge::format_timestamp(&jiff::Timestamp::now());
        let audit_id = ulid::Ulid::new().to_string();
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
   stability_hours, fact_type, is_forgotten, forgotten_at, forget_reason] :=
    *facts{id, valid_from, content, nous_id, confidence, tier,
           source_session_id, recorded_at, access_count, last_accessed_at,
           stability_hours, fact_type, is_forgotten, forgotten_at, forget_reason},
    id = $id,
    valid_to = $now,
    superseded_by = $superseding_id

:put facts {id, valid_from => content, nous_id, confidence, tier, valid_to,
            superseded_by, source_session_id, recorded_at, access_count,
            last_accessed_at, stability_hours, fact_type, is_forgotten,
            forgotten_at, forget_reason}
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
    #[expect(
        clippy::cast_possible_wrap,
        reason = "fact counts are small, well within i64 range"
    )]
    fn record_consolidation_audit(
        &self,
        record: &ConsolidationAuditRecord,
    ) -> crate::error::Result<()> {
        let script = r"
?[id, trigger_type, trigger_id, original_count, consolidated_count,
   original_fact_ids, consolidated_fact_ids, consolidated_at] <-
    [[$id, $trigger_type, $trigger_id, $original_count, $consolidated_count,
      $original_fact_ids, $consolidated_fact_ids, $consolidated_at]]

:put consolidation_audit {id => trigger_type, trigger_id, original_count,
                          consolidated_count, original_fact_ids,
                          consolidated_fact_ids, consolidated_at}
";
        let mut params = BTreeMap::new();
        params.insert("id".to_owned(), DataValue::Str(record.id.clone().into()));
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
            DataValue::from(record.original_count as i64),
        );
        params.insert(
            "consolidated_count".to_owned(),
            DataValue::from(record.consolidated_count as i64),
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
        Ok(())
    }

    /// Query the last consolidation timestamp from the audit trail.
    pub fn last_consolidation_time(
        &self,
        _nous_id: &str,
    ) -> Result<Option<String>, ConsolidationError> {
        let script = r"
?[consolidated_at] := *consolidation_audit{consolidated_at}
:sort -consolidated_at
:limit 1
";
        let result = self.run_query(script, BTreeMap::new()).map_err(|e| {
            StoreSnafu {
                message: e.to_string(),
            }
            .build()
        })?;

        if let Some(row) = result.rows.first() {
            Ok(Some(row[0].get_str().unwrap_or_default().to_owned()))
        } else {
            Ok(None)
        }
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
                    clippy::cast_precision_loss,
                    reason = "elapsed minutes as f64 is fine for rate limiting"
                )]
                let elapsed_hours = total_minutes as f64 / 60.0;
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
    facts: &[(FactId, String, f64, String)],
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

        let batch_fact_ids: Vec<FactId> = batch.iter().map(|(id, _, _, _)| id.clone()).collect();

        for entry in entries {
            all_consolidated.push(ConsolidatedFact {
                content: entry.content,
                confidence: 0.95,
                tier: "inferred".to_owned(),
                source_fact_ids: batch_fact_ids.clone(),
            });
        }
        all_superseded.extend(batch_fact_ids);
    }

    Ok(ConsolidationResult {
        original_count: facts.len(),
        consolidated_count: all_consolidated.len(),
        consolidated_facts: all_consolidated,
        superseded_fact_ids: all_superseded,
    })
}

/// Parse fact rows from query results.
fn parse_fact_rows(rows: &[Vec<DataValue>]) -> Vec<(FactId, String, f64, String)> {
    rows.iter()
        .map(|row| {
            let id = FactId::from(row[0].get_str().unwrap_or_default());
            let content = row[1].get_str().unwrap_or_default().to_owned();
            let confidence = row[2].get_float().unwrap_or(0.0);
            let recorded_at = row[3].get_str().unwrap_or_default().to_owned();
            (id, content, confidence, recorded_at)
        })
        .collect()
}
