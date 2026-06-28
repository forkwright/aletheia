use tracing::instrument;

use super::marshal::{extract_str, fact_to_params, rows_to_facts, scoped_visibility_rules};
use super::{KnowledgeStore, queries};

#[cfg(feature = "mneme-engine")]
#[derive(Default)]
struct FactAccessLogStats {
    count: u32,
    latest_accessed_at: Option<String>,
}

#[cfg(feature = "mneme-engine")]
fn datalog_string_literal(value: &str) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "\"\"".to_owned())
}

#[cfg(feature = "mneme-engine")]
fn datalog_string_list(values: &[String]) -> String {
    values
        .iter()
        .map(|value| datalog_string_literal(value))
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(feature = "mneme-engine")]
impl KnowledgeStore {
    /// Recompute and persist embeddings for every fact in the store.
    ///
    /// This is the recovery path for restored or migrated stores that still
    /// have facts but lost their vector rows. The provider is expected to be
    /// the active configured embedding backend; the helper reuses the same
    /// batch/backoff logic as the import-time backfill path.
    #[instrument(skip(self, provider))]
    pub fn reembed_all(
        &self,
        provider: &dyn crate::embedding::EmbeddingProvider,
    ) -> crate::error::Result<usize> {
        if crate::embedding::is_degraded_provider(provider) {
            return Err(crate::error::EngineQuerySnafu {
                message: "cannot re-embed facts with degraded embedding provider".to_owned(),
            }
            .build());
        }
        if provider.dimension() != self.dim {
            return Err(crate::error::EmbeddingDimensionMismatchSnafu {
                expected: self.dim,
                actual: provider.dimension(),
            }
            .build());
        }
        let facts = self.list_all_facts(i64::MAX)?;
        let written = self.backfill_fact_embeddings(&facts, provider);
        let expected = u64::try_from(facts.len()).map_err(|err| {
            crate::error::ConversionSnafu {
                message: format!("fact count overflowed u64: {err}"),
            }
            .build()
        })?;
        if written != expected {
            return Err(crate::error::EngineQuerySnafu {
                message: format!(
                    "re-embed incomplete: wrote {written} of {expected} fact embeddings"
                ),
            }
            .build());
        }
        self.replace_embedding_meta(provider.model_name(), provider.dimension())?;
        usize::try_from(written).map_err(|err| {
            crate::error::ConversionSnafu {
                message: format!("reembedded fact count overflowed usize: {err}"),
            }
            .build()
        })
    }

    /// Insert or update a fact.
    ///
    /// The fact is validated (non-empty content, length limit, confidence range)
    /// and then passed through the configured [`AdmissionPolicy`](crate::admission::AdmissionPolicy).
    /// If the policy rejects the fact, returns [`Error::AdmissionRejected`](crate::error::Error::AdmissionRejected).
    #[instrument(skip(self, fact), fields(fact_id = %fact.id))]
    pub fn insert_fact(&self, fact: &crate::knowledge::Fact) -> crate::error::Result<()> {
        use snafu::ensure;
        ensure!(!fact.content.is_empty(), crate::error::EmptyContentSnafu);
        ensure!(
            fact.content.len() <= crate::knowledge::MAX_CONTENT_LENGTH,
            crate::error::ContentTooLongSnafu {
                max: crate::knowledge::MAX_CONTENT_LENGTH,
                actual: fact.content.len()
            }
        );
        ensure!(
            (0.0..=1.0).contains(&fact.provenance.confidence),
            crate::error::InvalidConfidenceSnafu {
                value: fact.provenance.confidence
            }
        );

        // WHY (#5673): admission + insert remains atomic within a nous shard,
        // while unrelated nouses no longer serialize behind one global mutex.
        let Some(insert_lock) = self.insert_lock_for_nous(&fact.nous_id) else {
            return Err(crate::error::EngineQuerySnafu {
                message: "insert lock shards are not initialized".to_owned(),
            }
            .build());
        };
        let _guard = insert_lock.lock();

        // Admission control gate: check policy before persisting.
        let decision = self.admission_policy.should_admit(fact);
        if let crate::admission::AdmissionDecision::Reject(rejection) = decision {
            tracing::debug!(
                fact_id = %fact.id,
                factor = %rejection.factor,
                reason = %rejection.reason,
                "fact rejected by admission policy"
            );
            return Err(crate::error::AdmissionRejectedSnafu {
                reason: rejection.reason,
            }
            .build());
        }

        let params = fact_to_params(fact);
        let result = self.run_mut(&queries::upsert_fact(), params);
        if result.is_ok() {
            crate::metrics::record_fact_inserted(&fact.nous_id);
            // WHY (#4662): a successful base write to `facts` can change the
            // inputs to ontological/causal/defeasible rules, so mark derived
            // materializations stale. Only on success: a rejected/errored write
            // must not spuriously invalidate derived results.
            self.invalidate_derived_facts()?;
        }
        result
    }

    /// Overlay append-only access events onto decoded facts.
    pub(super) fn apply_access_log_to_facts(
        &self,
        facts: &mut [crate::knowledge::Fact],
    ) -> crate::error::Result<()> {
        if facts.is_empty() {
            return Ok(());
        }

        let mut seen = std::collections::HashSet::new();
        let mut fact_ids = Vec::new();
        for fact in facts.iter() {
            let fact_id = fact.id.as_str().to_owned();
            if seen.insert(fact_id.clone()) {
                fact_ids.push(fact_id);
            }
        }
        let id_list = datalog_string_list(&fact_ids);
        let script = format!(
            r"
            ?[fact_id, event_id, accessed_at] :=
                *fact_access_log{{event_id, fact_id, accessed_at}},
                fact_id in [{id_list}]
            "
        );
        let rows = self.run_read(&script, std::collections::BTreeMap::new())?;
        let mut stats_by_fact = std::collections::HashMap::<String, FactAccessLogStats>::new();
        for row in &rows.rows {
            let Some(fact_id) = row.first().and_then(crate::engine::DataValue::get_str) else {
                continue;
            };
            let Some(accessed_at) = row.get(2).and_then(crate::engine::DataValue::get_str) else {
                continue;
            };
            let stats = stats_by_fact.entry(fact_id.to_owned()).or_default();
            stats.count = stats.count.saturating_add(1);
            let should_replace = match stats.latest_accessed_at.as_deref() {
                Some(latest) => accessed_at > latest,
                None => true,
            };
            if should_replace {
                stats.latest_accessed_at = Some(accessed_at.to_owned());
            }
        }

        for fact in facts {
            let Some(stats) = stats_by_fact.get(fact.id.as_str()) else {
                continue;
            };
            fact.access.access_count = fact.access.access_count.saturating_add(stats.count);
            if let Some(accessed_at) = stats
                .latest_accessed_at
                .as_deref()
                .and_then(crate::knowledge::parse_timestamp)
            {
                let should_replace = match fact.access.last_accessed_at {
                    Some(existing) => accessed_at > existing,
                    None => true,
                };
                if should_replace {
                    fact.access.last_accessed_at = Some(accessed_at);
                }
            }
        }
        Ok(())
    }

    /// Decode fact rows and overlay append-only access events.
    pub(super) fn rows_to_facts_with_access(
        &self,
        rows: crate::engine::NamedRows,
        nous_id: &str,
    ) -> crate::error::Result<Vec<crate::knowledge::Fact>> {
        let mut facts = rows_to_facts(rows, nous_id)?;
        self.apply_access_log_to_facts(&mut facts)?;
        Ok(facts)
    }

    /// Decode raw fact rows and overlay append-only access events.
    pub(super) fn rows_to_raw_facts_with_access(
        &self,
        rows: crate::engine::NamedRows,
    ) -> crate::error::Result<Vec<crate::knowledge::Fact>> {
        let mut facts = super::marshal::rows_to_raw_facts(rows)?;
        self.apply_access_log_to_facts(&mut facts)?;
        Ok(facts)
    }

    /// Decode partial fact rows and overlay append-only access events.
    pub(super) fn rows_to_partial_facts_with_access(
        &self,
        rows: crate::engine::NamedRows,
    ) -> crate::error::Result<Vec<crate::knowledge::Fact>> {
        let mut facts = super::marshal::rows_to_facts_partial(rows)?;
        self.apply_access_log_to_facts(&mut facts)?;
        Ok(facts)
    }

    /// Rebuild fact embeddings after a restore/import.
    ///
    /// The importer only serializes typed knowledge, so the HNSW vector index
    /// must be repopulated from the restored fact content. This helper is
    /// best-effort: batch embedding is preferred, but failures fall back to
    /// per-fact embedding and individual insert failures are logged and
    /// skipped.
    #[instrument(skip(self, facts, provider), fields(fact_count = facts.len()))]
    pub fn backfill_fact_embeddings(
        &self,
        facts: &[crate::knowledge::Fact],
        provider: &dyn crate::embedding::EmbeddingProvider,
    ) -> u64 {
        if facts.is_empty() {
            return 0;
        }
        if crate::embedding::is_degraded_provider(provider) {
            tracing::warn!(
                fact_count = facts.len(),
                "backfill_fact_embeddings: provider is degraded; skipping"
            );
            return 0;
        }

        let texts: Vec<&str> = facts.iter().map(|fact| fact.content.as_str()).collect();
        let batch_embeddings = match provider.embed_batch(&texts) {
            Ok(embeddings) if embeddings.len() == facts.len() => Some(embeddings),
            Ok(embeddings) => {
                tracing::warn!(
                    fact_count = facts.len(),
                    returned = embeddings.len(),
                    "backfill_fact_embeddings: batch embed returned unexpected length; falling back to per-fact embedding"
                );
                None
            }
            Err(err) => {
                tracing::warn!(
                    fact_count = facts.len(),
                    error = %err,
                    "backfill_fact_embeddings: batch embed failed; falling back to per-fact embedding"
                );
                None
            }
        };

        let mut inserted = 0u64;
        let mut failures = 0u32;
        if let Some(embeddings) = batch_embeddings {
            for (fact, embedding) in facts.iter().zip(embeddings.into_iter()) {
                insert_fact_embedding(self, fact, embedding, &mut inserted, &mut failures);
            }
        } else {
            for fact in facts {
                match provider.embed(&fact.content) {
                    Ok(embedding) => {
                        insert_fact_embedding(self, fact, embedding, &mut inserted, &mut failures);
                    }
                    Err(err) => {
                        tracing::warn!(
                            fact_id = %fact.id,
                            error = %err,
                            "backfill_fact_embeddings: failed to embed fact"
                        );
                        failures = failures.saturating_add(1);
                    }
                }
            }
        }

        if failures > 0 {
            tracing::info!(
                fact_count = facts.len(),
                inserted,
                failures,
                "backfill_fact_embeddings complete (with failures)"
            );
        } else {
            tracing::info!(
                fact_count = facts.len(),
                inserted,
                "backfill_fact_embeddings complete"
            );
        }

        inserted
    }

    /// Supersede an existing fact with a new one.
    ///
    /// Sets `valid_to` on the old fact to `now` and `superseded_by` to the new
    /// fact's ID, then inserts the new fact.
    #[expect(
        clippy::too_many_lines,
        reason = "sequential param mapping, splitting adds indirection"
    )]
    #[instrument(skip(self, old_fact, new_fact), fields(old_id = %old_fact.id, new_id = %new_fact.id))]
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "fact temporal pipeline — exercised by tests only")
    )]
    pub(crate) fn supersede_fact(
        &self,
        old_fact: &crate::knowledge::Fact,
        new_fact: &crate::knowledge::Fact,
    ) -> crate::error::Result<()> {
        use std::collections::BTreeMap;

        use crate::engine::DataValue;
        use crate::knowledge::format_timestamp;
        let now = jiff::Timestamp::now();
        let now_str = format_timestamp(&now);

        let mut params = BTreeMap::new();
        params.insert(
            String::from("old_id"),
            DataValue::Str(old_fact.id.as_str().into()),
        );
        params.insert(
            String::from("old_valid_from"),
            DataValue::Str(format_timestamp(&old_fact.temporal.valid_from).into()),
        );
        params.insert(
            String::from("old_content"),
            DataValue::Str(old_fact.content.as_str().into()),
        );
        params.insert(
            String::from("nous_id"),
            DataValue::Str(old_fact.nous_id.as_str().into()),
        );
        params.insert(
            String::from("old_confidence"),
            DataValue::from(old_fact.provenance.confidence),
        );
        params.insert(
            String::from("old_tier"),
            DataValue::Str(old_fact.provenance.tier.as_str().into()),
        );
        params.insert(String::from("now"), DataValue::Str(now_str.as_str().into()));
        params.insert(
            String::from("new_id"),
            DataValue::Str(new_fact.id.as_str().into()),
        );
        params.insert(
            String::from("old_source"),
            DataValue::Str(
                old_fact
                    .provenance
                    .source_session_id
                    .as_deref()
                    .unwrap_or("")
                    .into(),
            ),
        );
        params.insert(
            String::from("old_recorded"),
            DataValue::Str(format_timestamp(&old_fact.temporal.recorded_at).into()),
        );
        params.insert(
            String::from("old_access_count"),
            DataValue::from(i64::from(old_fact.access.access_count)),
        );
        params.insert(
            String::from("old_last_accessed_at"),
            DataValue::Str(
                old_fact
                    .access
                    .last_accessed_at
                    .as_ref()
                    .map(format_timestamp)
                    .unwrap_or_default()
                    .into(),
            ),
        );
        params.insert(
            String::from("old_stability_hours"),
            DataValue::from(old_fact.provenance.stability_hours),
        );
        params.insert(
            String::from("old_fact_type"),
            DataValue::Str(old_fact.fact_type.as_str().into()),
        );
        params.insert(
            String::from("old_is_forgotten"),
            DataValue::Bool(old_fact.lifecycle.is_forgotten),
        );
        params.insert(
            String::from("old_forgotten_at"),
            old_fact
                .lifecycle
                .forgotten_at
                .as_ref()
                .map_or(DataValue::Null, |ts| {
                    DataValue::Str(format_timestamp(ts).into())
                }),
        );
        params.insert(
            String::from("old_forget_reason"),
            old_fact
                .lifecycle
                .forget_reason
                .as_ref()
                .map_or(DataValue::Null, |r| DataValue::Str(r.as_str().into())),
        );
        params.insert(
            String::from("old_scope"),
            old_fact
                .scope
                .as_ref()
                .map_or(DataValue::Null, |s| DataValue::Str(s.as_str().into())),
        );
        params.insert(
            String::from("old_project_id"),
            old_fact
                .project_id
                .as_ref()
                .map_or(DataValue::Null, |id| DataValue::Str(id.as_str().into())),
        );
        params.insert(
            String::from("old_visibility"),
            DataValue::Str(old_fact.visibility.as_str().into()),
        );
        params.insert(
            String::from("old_sensitivity"),
            DataValue::Str(old_fact.sensitivity.as_str().into()),
        );

        params.insert(
            String::from("new_content"),
            DataValue::Str(new_fact.content.as_str().into()),
        );
        params.insert(
            String::from("new_confidence"),
            DataValue::from(new_fact.provenance.confidence),
        );
        params.insert(
            String::from("new_tier"),
            DataValue::Str(new_fact.provenance.tier.as_str().into()),
        );
        params.insert(
            String::from("source_session_id"),
            DataValue::Str(
                new_fact
                    .provenance
                    .source_session_id
                    .as_deref()
                    .unwrap_or("")
                    .into(),
            ),
        );
        params.insert(
            String::from("stability_hours"),
            DataValue::from(new_fact.provenance.stability_hours),
        );
        params.insert(
            String::from("fact_type"),
            DataValue::Str(new_fact.fact_type.as_str().into()),
        );
        params.insert(
            String::from("scope"),
            new_fact
                .scope
                .as_ref()
                .map_or(DataValue::Null, |s| DataValue::Str(s.as_str().into())),
        );
        params.insert(
            String::from("project_id"),
            new_fact
                .project_id
                .as_ref()
                .map_or(DataValue::Null, |id| DataValue::Str(id.as_str().into())),
        );
        params.insert(
            String::from("visibility"),
            DataValue::Str(new_fact.visibility.as_str().into()),
        );
        params.insert(
            String::from("sensitivity"),
            DataValue::Str(new_fact.sensitivity.as_str().into()),
        );

        self.run_mut(&queries::supersede_fact(), params)?;
        // WHY (#4662): supersession rewrites `facts` rows (old `valid_to`/
        // `superseded_by` plus the new row), changing derived-rule inputs.
        self.invalidate_derived_facts()
    }

    /// Query current facts for a nous at a given time, up to limit results.
    #[instrument(skip(self))]
    pub fn query_facts(
        &self,
        nous_id: &str,
        now: &str,
        limit: i64,
    ) -> crate::error::Result<Vec<crate::knowledge::Fact>> {
        use std::collections::BTreeMap;

        use crate::engine::DataValue;
        let mut params = BTreeMap::new();
        params.insert(String::from("nous_id"), DataValue::Str(nous_id.into()));
        params.insert(String::from("now"), DataValue::Str(now.into()));
        params.insert(String::from("limit"), DataValue::from(limit));

        let rows = self.run_read(&queries::full_current_facts(), params)?;
        self.rows_to_facts_with_access(rows, nous_id)
    }

    /// Query current facts visible to a requesting nous at a given time.
    ///
    /// The requester can read its own facts plus `Shared` and `Published`
    /// facts from the same store. Foreign `Private`, `Restricted`, and future
    /// unknown visibility values remain hidden.
    #[instrument(skip(self))]
    pub fn query_visible_facts(
        &self,
        requester_nous_id: &str,
        now: &str,
        limit: i64,
    ) -> crate::error::Result<Vec<crate::knowledge::Fact>> {
        use std::collections::BTreeMap;

        use crate::engine::DataValue;
        let script = format!(
            r"
            {visible}
            ?[id, valid_from, content, nous_id, confidence, tier, valid_to,
              superseded_by, source_session_id, recorded_at,
              access_count, last_accessed_at, stability_hours, fact_type,
              is_forgotten, forgotten_at, forget_reason, scope, project_id,
              visibility, sensitivity] :=
                visible_fact[id],
                *facts{{id, valid_from, content, nous_id, confidence, tier,
                       valid_to, superseded_by, source_session_id, recorded_at,
                       access_count, last_accessed_at, stability_hours, fact_type,
                       is_forgotten, forgotten_at, forget_reason, scope, project_id,
                       visibility, sensitivity}},
                valid_from <= $now,
                valid_to > $now,
                is_null(superseded_by),
                is_forgotten == false
            :order -recorded_at
            :limit $limit
            ",
            visible = scoped_visibility_rules()
        );
        let mut params = BTreeMap::new();
        params.insert(
            String::from("requester_nous_id"),
            DataValue::Str(requester_nous_id.into()),
        );
        params.insert(String::from("now"), DataValue::Str(now.into()));
        params.insert(String::from("limit"), DataValue::from(limit));
        let rows = self.run_read(&script, params)?;
        self.rows_to_raw_facts_with_access(rows)
    }

    /// Point-in-time fact query.
    #[instrument(skip(self))]
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "fact temporal operations for knowledge store")
    )]
    pub(crate) fn query_facts_at(
        &self,
        time: &str,
    ) -> crate::error::Result<Vec<crate::knowledge::Fact>> {
        use std::collections::BTreeMap;

        use crate::engine::DataValue;
        let mut params = BTreeMap::new();
        params.insert(String::from("time"), DataValue::Str(time.into()));

        let rows = self.run_read(&queries::facts_at_time(), params)?;
        self.rows_to_partial_facts_with_access(rows)
    }

    /// Increment access count and update last-accessed timestamp for the given fact IDs.
    ///
    /// Appends access events; fact reads lazily aggregate those events into
    /// `FactAccess`.
    #[instrument(skip(self), fields(count = fact_ids.len()))]
    pub(crate) fn increment_access(
        &self,
        fact_ids: &[crate::id::FactId],
    ) -> crate::error::Result<()> {
        if fact_ids.is_empty() {
            return Ok(());
        }
        let now = crate::knowledge::format_timestamp(&jiff::Timestamp::now());
        let rows = fact_ids
            .iter()
            .map(|id| {
                let event_id = self.next_access_event_id();
                format!(
                    "[{}, {}, {}]",
                    datalog_string_literal(id.as_str()),
                    datalog_string_literal(&event_id),
                    datalog_string_literal(&now)
                )
            })
            .collect::<Vec<_>>()
            .join(", ");
        let script = format!(
            r"
            access_events[fact_id, event_id, accessed_at] <- [{rows}]
            ?[event_id, fact_id, accessed_at] :=
                access_events[fact_id, event_id, accessed_at],
                *facts{{id: fact_id}}
            :put fact_access_log {{event_id => fact_id, accessed_at}}
            "
        );
        self.run_mut(&script, std::collections::BTreeMap::new())
    }

    /// Async `increment_access`: wraps sync call in `spawn_blocking`.
    #[instrument(skip(self), fields(count = fact_ids.len()))]
    pub async fn increment_access_async(
        self: &std::sync::Arc<Self>,
        fact_ids: Vec<crate::id::FactId>,
    ) -> crate::error::Result<()> {
        use snafu::ResultExt;
        let this = std::sync::Arc::clone(self);
        tokio::task::spawn_blocking(move || this.increment_access(&fact_ids))
            .await
            .context(crate::error::JoinSnafu)?
    }

    /// Soft-delete a fact: set `is_forgotten = true` with reason and timestamp.
    ///
    /// Returns the forgotten fact. Errors if the fact does not exist.
    #[instrument(skip(self))]
    pub fn forget_fact(
        &self,
        fact_id: &crate::id::FactId,
        reason: crate::knowledge::ForgetReason,
    ) -> crate::error::Result<crate::knowledge::Fact> {
        self.forget_fact_with_content(fact_id, reason, None)
    }

    /// Soft-delete a fact, optionally replacing its content in the same write.
    ///
    /// WHY (#5421): the skill-review path must replace a pending fact's content
    /// with the reviewed copy *and* mark it forgotten. Doing those as two
    /// separate writes left a window where the content was mutated but the
    /// fact was not yet forgotten — a partial state if a later write failed.
    /// Folding the content replacement into the single forget `:put` makes the
    /// pending-fact mutation atomic with the forget. `forget_fact` passes
    /// `None` to preserve the existing content unchanged.
    pub(crate) fn forget_fact_with_content(
        &self,
        fact_id: &crate::id::FactId,
        reason: crate::knowledge::ForgetReason,
        new_content: Option<&str>,
    ) -> crate::error::Result<crate::knowledge::Fact> {
        let existing = self.read_facts_by_id_raw(fact_id.as_str())?;
        let Some(current) = existing.first() else {
            return Err(crate::error::FactNotFoundSnafu {
                id: fact_id.as_str(),
            }
            .build());
        };

        let content = new_content.unwrap_or(current.content.as_str());
        let now = crate::knowledge::format_timestamp(&jiff::Timestamp::now());
        let script = r"
            ?[id, valid_from, content, nous_id, confidence, tier, valid_to,
              superseded_by, source_session_id, recorded_at,
              access_count, last_accessed_at, stability_hours, fact_type,
              is_forgotten, forgotten_at, forget_reason, scope, project_id,
              visibility, sensitivity] :=
                *facts{id, valid_from, nous_id, confidence, tier,
                       valid_to, superseded_by, source_session_id, recorded_at,
                       access_count, last_accessed_at, stability_hours, fact_type,
                       scope, project_id, visibility, sensitivity},
                id = $id,
                content = $content,
                is_forgotten = true,
                forgotten_at = $now,
                forget_reason = $reason
            :put facts {id, valid_from => content, nous_id, confidence, tier,
                        valid_to, superseded_by, source_session_id, recorded_at,
                        access_count, last_accessed_at, stability_hours, fact_type,
                        is_forgotten, forgotten_at, forget_reason, scope, project_id,
                        visibility, sensitivity}
        ";
        let mut params = std::collections::BTreeMap::new();
        params.insert(
            String::from("id"),
            crate::engine::DataValue::Str(fact_id.as_str().into()),
        );
        params.insert(
            String::from("content"),
            crate::engine::DataValue::Str(content.into()),
        );
        params.insert(
            String::from("now"),
            crate::engine::DataValue::Str(now.into()),
        );
        params.insert(
            String::from("reason"),
            crate::engine::DataValue::Str(reason.as_str().into()),
        );
        self.run_mut(script, params)?;

        let facts = self.read_facts_by_id(fact_id.as_str())?;
        facts.into_iter().next().ok_or_else(|| {
            crate::error::FactNotFoundSnafu {
                id: fact_id.as_str(),
            }
            .build()
        })
    }

    /// Reverse a soft-delete: clear `is_forgotten`, `forgotten_at`, `forget_reason`.
    ///
    /// Returns the restored fact. Errors if the fact does not exist.
    #[instrument(skip(self))]
    pub(crate) fn unforget_fact(
        &self,
        fact_id: &crate::id::FactId,
    ) -> crate::error::Result<crate::knowledge::Fact> {
        let existing = self.read_facts_by_id(fact_id.as_str())?;
        if existing.is_empty() {
            return Err(crate::error::FactNotFoundSnafu {
                id: fact_id.as_str(),
            }
            .build());
        }

        let script = r"
            ?[id, valid_from, content, nous_id, confidence, tier, valid_to,
              superseded_by, source_session_id, recorded_at,
              access_count, last_accessed_at, stability_hours, fact_type,
              is_forgotten, forgotten_at, forget_reason, scope, project_id,
              visibility, sensitivity] :=
                *facts{id, valid_from, content, nous_id, confidence, tier,
                       valid_to, superseded_by, source_session_id, recorded_at,
                       access_count, last_accessed_at, stability_hours, fact_type,
                       scope, project_id, visibility, sensitivity},
                id = $id,
                is_forgotten = false,
                forgotten_at = null,
                forget_reason = null
            :put facts {id, valid_from => content, nous_id, confidence, tier,
                        valid_to, superseded_by, source_session_id, recorded_at,
                        access_count, last_accessed_at, stability_hours, fact_type,
                        is_forgotten, forgotten_at, forget_reason, scope, project_id,
                        visibility, sensitivity}
        ";
        let mut params = std::collections::BTreeMap::new();
        params.insert(
            String::from("id"),
            crate::engine::DataValue::Str(fact_id.as_str().into()),
        );
        self.run_mut(script, params)?;

        let facts = self.read_facts_by_id(fact_id.as_str())?;
        facts.into_iter().next().ok_or_else(|| {
            crate::error::FactNotFoundSnafu {
                id: fact_id.as_str(),
            }
            .build()
        })
    }

    /// List only forgotten facts for a given agent, ordered by `forgotten_at`.
    ///
    /// Returns facts where `is_forgotten == true`, with their reasons and timestamps.
    #[instrument(skip(self))]
    pub(crate) fn list_forgotten(
        &self,
        nous_id: &str,
        limit: i64,
    ) -> crate::error::Result<Vec<crate::knowledge::Fact>> {
        use std::collections::BTreeMap;

        use crate::engine::DataValue;
        let mut params = BTreeMap::new();
        params.insert(String::from("nous_id"), DataValue::Str(nous_id.into()));
        params.insert(String::from("limit"), DataValue::from(limit));
        let rows = self.run_read(&queries::forgotten_facts(), params)?;
        self.rows_to_facts_with_access(rows, nous_id)
    }

    /// Async `list_forgotten`: wraps sync call in `spawn_blocking`.
    #[instrument(skip(self))]
    pub async fn list_forgotten_async(
        self: &std::sync::Arc<Self>,
        nous_id: String,
        limit: i64,
    ) -> crate::error::Result<Vec<crate::knowledge::Fact>> {
        use snafu::ResultExt;
        let this = std::sync::Arc::clone(self);
        tokio::task::spawn_blocking(move || this.list_forgotten(&nous_id, limit))
            .await
            .context(crate::error::JoinSnafu)?
    }

    /// Filter hybrid search results to exclude forgotten facts.
    pub(super) fn filter_forgotten_results(
        &self,
        results: Vec<super::HybridResult>,
    ) -> crate::error::Result<Vec<super::HybridResult>> {
        if results.is_empty() {
            return Ok(results);
        }

        // PERF: Batch-check forgotten status in a single query rather than per-result.
        let forgotten_ids = self.forgotten_fact_ids(&results)?;
        if forgotten_ids.is_empty() {
            return Ok(results);
        }

        Ok(results
            .into_iter()
            .filter(|r| !forgotten_ids.contains(r.id.as_str()))
            .collect())
    }

    /// Return the subset of the given fact IDs that are currently marked as forgotten.
    ///
    /// Used by sibling search methods (e.g. `search_vectors`) that retrieve results
    /// from indices which do not carry the `is_forgotten` flag.
    pub(super) fn query_forgotten_ids(
        &self,
        ids: &[&str],
    ) -> crate::error::Result<std::collections::HashSet<String>> {
        use std::collections::BTreeMap;

        use crate::engine::DataValue;
        if ids.is_empty() {
            return Ok(std::collections::HashSet::new());
        }

        let id_list: Vec<String> = ids
            .iter()
            .map(|id| format!("'{}'", id.replace('\'', "''")))
            .collect();
        let script = format!(
            r"?[id] := *facts{{id, is_forgotten}}, is_forgotten == true, id in [{}]",
            id_list.join(", ")
        );
        let rows = self.run_read(&script, BTreeMap::<String, DataValue>::new())?;
        let mut result = std::collections::HashSet::new();
        for row in rows.rows {
            if let Some(val) = row.first()
                && let Ok(s) = extract_str(val)
            {
                result.insert(s);
            }
        }
        Ok(result)
    }

    /// Return the set of fact IDs (from the given results) that are currently forgotten.
    fn forgotten_fact_ids(
        &self,
        results: &[super::HybridResult],
    ) -> crate::error::Result<std::collections::HashSet<String>> {
        let ids: Vec<&str> = results.iter().map(|r| r.id.as_str()).collect();
        self.query_forgotten_ids(&ids)
    }

    /// Read a fact by ID only when visible to the requester.
    pub fn read_visible_facts_by_id(
        &self,
        fact_id: &str,
        requester_nous_id: &str,
    ) -> crate::error::Result<Vec<crate::knowledge::Fact>> {
        use std::collections::BTreeMap;

        use crate::engine::DataValue;
        let script = format!(
            r"
            {visible}
            ?[id, valid_from, content, nous_id, confidence, tier, valid_to,
              superseded_by, source_session_id, recorded_at,
              access_count, last_accessed_at, stability_hours, fact_type,
              is_forgotten, forgotten_at, forget_reason, scope, project_id,
              visibility, sensitivity] :=
                visible_fact[id],
                id == $fact_id,
                *facts{{id, valid_from, content, nous_id, confidence, tier,
                       valid_to, superseded_by, source_session_id, recorded_at,
                       access_count, last_accessed_at, stability_hours, fact_type,
                       is_forgotten, forgotten_at, forget_reason, scope, project_id,
                       visibility, sensitivity}}
            ",
            visible = scoped_visibility_rules()
        );
        let mut params = BTreeMap::new();
        params.insert(String::from("fact_id"), DataValue::Str(fact_id.into()));
        params.insert(
            String::from("requester_nous_id"),
            DataValue::Str(requester_nous_id.into()),
        );
        let rows = self.run_read(&script, params)?;
        self.rows_to_raw_facts_with_access(rows)
    }

    pub(super) fn visible_fact_ids_for_entity(
        &self,
        entity_id: &str,
        requester_nous_id: &str,
    ) -> crate::error::Result<Vec<String>> {
        use std::collections::BTreeMap;

        use crate::engine::DataValue;
        let script = format!(
            r"
            {visible}
            ?[fact_id] :=
                *fact_entities{{fact_id, entity_id: $entity_id}},
                visible_fact[fact_id],
                *facts{{id: fact_id, is_forgotten, superseded_by}},
                is_forgotten == false,
                is_null(superseded_by)
            ",
            visible = scoped_visibility_rules()
        );
        let mut params = BTreeMap::new();
        params.insert(String::from("entity_id"), DataValue::Str(entity_id.into()));
        params.insert(
            String::from("requester_nous_id"),
            DataValue::Str(requester_nous_id.into()),
        );
        let rows = self.run_read(&script, params)?;
        Ok(rows
            .rows
            .iter()
            .filter_map(|row| row.first().and_then(|v| v.get_str()).map(str::to_owned))
            .collect())
    }

    /// Query facts valid at a specific point in time.
    /// Returns facts where `valid_from <= at_time` AND `valid_to > at_time`.
    pub(crate) fn query_facts_temporal(
        &self,
        nous_id: &str,
        at_time: &str,
        filter: Option<&str>,
    ) -> crate::error::Result<Vec<crate::knowledge::Fact>> {
        use std::collections::BTreeMap;

        use crate::engine::DataValue;
        let mut params = BTreeMap::new();
        params.insert(String::from("nous_id"), DataValue::Str(nous_id.into()));
        params.insert(String::from("at_time"), DataValue::Str(at_time.into()));

        let rows = match filter {
            Some(f) if !f.is_empty() => {
                params.insert(String::from("filter"), DataValue::Str(f.into()));
                self.run_read(queries::TEMPORAL_FACTS_FILTERED, params)?
            }
            _ => self.run_read(&queries::temporal_facts(), params)?,
        };
        self.rows_to_facts_with_access(rows, nous_id)
    }

    /// Query facts that changed between two timestamps.
    /// Returns facts where `valid_from` is in `(from_time, to_time]` OR
    /// `valid_to` is in `(from_time, to_time]`.
    pub(crate) fn query_facts_diff(
        &self,
        nous_id: &str,
        from_time: &str,
        to_time: &str,
    ) -> crate::error::Result<crate::knowledge::FactDiff> {
        use std::collections::BTreeMap;

        use crate::engine::DataValue;
        let mut params = BTreeMap::new();
        params.insert(String::from("nous_id"), DataValue::Str(nous_id.into()));
        params.insert(String::from("from_time"), DataValue::Str(from_time.into()));
        params.insert(String::from("to_time"), DataValue::Str(to_time.into()));

        let added_rows = self.run_read(queries::TEMPORAL_DIFF_ADDED, params.clone())?;
        let added = self.rows_to_facts_with_access(added_rows, nous_id)?;

        let removed_rows = self.run_read(queries::TEMPORAL_DIFF_REMOVED, params)?;
        let removed = self.rows_to_facts_with_access(removed_rows, nous_id)?;

        // NOTE: A fact in "removed" whose superseded_by points to one in "added" is a
        // modification, not a pure removal.
        let added_ids: std::collections::HashSet<&str> =
            added.iter().map(|f| f.id.as_str()).collect();
        let mut modified = Vec::new();
        let mut pure_removed = Vec::new();

        for old in &removed {
            if let Some(ref new_id) = old.lifecycle.superseded_by
                && added_ids.contains(new_id.as_str())
                && let Some(new_fact) = added.iter().find(|f| f.id == *new_id)
            {
                modified.push((old.clone(), new_fact.clone()));
                continue;
            }
            pure_removed.push(old.clone());
        }

        let modified_new_ids: std::collections::HashSet<&str> =
            modified.iter().map(|(_, new)| new.id.as_str()).collect();
        let pure_added: Vec<_> = added
            .into_iter()
            .filter(|f| !modified_new_ids.contains(f.id.as_str()))
            .collect();

        Ok(crate::knowledge::FactDiff {
            added: pure_added,
            modified,
            removed: pure_removed,
        })
    }

    /// Async `query_facts_temporal` wrapper.
    #[instrument(skip(self))]
    pub async fn query_facts_temporal_async(
        self: &std::sync::Arc<Self>,
        nous_id: String,
        at_time: String,
        filter: Option<String>,
    ) -> crate::error::Result<Vec<crate::knowledge::Fact>> {
        use snafu::ResultExt;
        let this = std::sync::Arc::clone(self);
        tokio::task::spawn_blocking(move || {
            this.query_facts_temporal(&nous_id, &at_time, filter.as_deref())
        })
        .await
        .context(crate::error::JoinSnafu)?
    }

    /// Async `query_facts_diff` wrapper.
    #[instrument(skip(self))]
    pub async fn query_facts_diff_async(
        self: &std::sync::Arc<Self>,
        nous_id: String,
        from_time: String,
        to_time: String,
    ) -> crate::error::Result<crate::knowledge::FactDiff> {
        use snafu::ResultExt;
        let this = std::sync::Arc::clone(self);
        tokio::task::spawn_blocking(move || this.query_facts_diff(&nous_id, &from_time, &to_time))
            .await
            .context(crate::error::JoinSnafu)?
    }

    /// List all facts across all agents, ordered by `recorded_at` descending.
    ///
    /// Unlike [`audit_all_facts`](Self::audit_all_facts), this does not require
    /// a `nous_id` filter and returns facts from every agent.
    #[instrument(skip(self))]
    pub fn list_all_facts(&self, limit: i64) -> crate::error::Result<Vec<crate::knowledge::Fact>> {
        use std::collections::BTreeMap;

        use crate::engine::DataValue;
        let mut params = BTreeMap::new();
        params.insert(String::from("limit"), DataValue::from(limit));

        let script = r"
            ?[id, valid_from, content, nous_id, confidence, tier, valid_to,
              superseded_by, source_session_id, recorded_at,
              access_count, last_accessed_at, stability_hours, fact_type,
              is_forgotten, forgotten_at, forget_reason, scope, project_id,
              visibility, sensitivity] :=
                *facts{id, valid_from, content, nous_id, confidence, tier,
                       valid_to, superseded_by, source_session_id, recorded_at,
                       access_count, last_accessed_at, stability_hours, fact_type,
                       is_forgotten, forgotten_at, forget_reason, scope, project_id,
                       visibility, sensitivity}
            :order -recorded_at
            :limit $limit
        ";
        let rows = self.run_read(script, params)?;
        self.rows_to_raw_facts_with_access(rows)
    }

    /// Async `list_all_facts` wrapper.
    #[instrument(skip(self))]
    pub async fn list_all_facts_async(
        self: &std::sync::Arc<Self>,
        limit: i64,
    ) -> crate::error::Result<Vec<crate::knowledge::Fact>> {
        use snafu::ResultExt;
        let this = std::sync::Arc::clone(self);
        tokio::task::spawn_blocking(move || this.list_all_facts(limit))
            .await
            .context(crate::error::JoinSnafu)?
    }

    /// Audit query: returns all facts regardless of forgotten/superseded/temporal state.
    #[instrument(skip(self))]
    pub fn audit_all_facts(
        &self,
        nous_id: &str,
        limit: i64,
    ) -> crate::error::Result<Vec<crate::knowledge::Fact>> {
        use std::collections::BTreeMap;

        use crate::engine::DataValue;
        let mut params = BTreeMap::new();
        params.insert(String::from("nous_id"), DataValue::Str(nous_id.into()));
        params.insert(String::from("limit"), DataValue::from(limit));

        let rows = self.run_read(&queries::audit_all_facts(), params)?;
        self.rows_to_facts_with_access(rows, nous_id)
    }

    // NOTE: Async wrappers use spawn_blocking because Krites query execution is synchronous.

    /// Async `forget_fact`: wraps sync call in `spawn_blocking`.
    #[instrument(skip(self))]
    pub async fn forget_fact_async(
        self: &std::sync::Arc<Self>,
        fact_id: crate::id::FactId,
        reason: crate::knowledge::ForgetReason,
    ) -> crate::error::Result<crate::knowledge::Fact> {
        use snafu::ResultExt;
        let this = std::sync::Arc::clone(self);
        tokio::task::spawn_blocking(move || this.forget_fact(&fact_id, reason))
            .await
            .context(crate::error::JoinSnafu)?
    }

    /// Async `unforget_fact`: wraps sync call in `spawn_blocking`.
    #[instrument(skip(self))]
    pub async fn unforget_fact_async(
        self: &std::sync::Arc<Self>,
        fact_id: crate::id::FactId,
    ) -> crate::error::Result<crate::knowledge::Fact> {
        use snafu::ResultExt;
        let this = std::sync::Arc::clone(self);
        tokio::task::spawn_blocking(move || this.unforget_fact(&fact_id))
            .await
            .context(crate::error::JoinSnafu)?
    }

    /// Async `audit_all_facts`: wraps sync call in `spawn_blocking`.
    #[instrument(skip(self))]
    pub async fn audit_all_facts_async(
        self: &std::sync::Arc<Self>,
        nous_id: String,
        limit: i64,
    ) -> crate::error::Result<Vec<crate::knowledge::Fact>> {
        use snafu::ResultExt;
        let this = std::sync::Arc::clone(self);
        tokio::task::spawn_blocking(move || this.audit_all_facts(&nous_id, limit))
            .await
            .context(crate::error::JoinSnafu)?
    }

    /// Update the confidence of a fact in-place.
    ///
    /// Performs a read-modify-write cycle: reads all temporal records for the
    /// fact, overwrites the `confidence` field on each, and re-perserts them.
    ///
    /// Returns the updated fact. Errors if no fact with the given ID exists or
    /// if `confidence` is outside `[0.0, 1.0]`.
    #[instrument(skip(self))]
    pub(crate) fn update_confidence(
        &self,
        fact_id: &crate::id::FactId,
        confidence: f64,
    ) -> crate::error::Result<crate::knowledge::Fact> {
        use snafu::ensure;

        ensure!(
            (0.0..=1.0).contains(&confidence),
            crate::error::InvalidConfidenceSnafu { value: confidence }
        );

        let existing = self.read_facts_by_id(fact_id.as_str())?;
        if existing.is_empty() {
            return Err(crate::error::FactNotFoundSnafu {
                id: fact_id.as_str(),
            }
            .build());
        }

        // WHY (#5673): use raw rows for write-back so append-only access events
        // are not folded into `facts.access_count` and then counted again.
        for mut fact in existing {
            fact.provenance.confidence = confidence;
            self.insert_fact(&fact)?;
        }

        let updated = self.read_facts_by_id(fact_id.as_str())?;
        updated.into_iter().next().ok_or_else(|| {
            crate::error::FactNotFoundSnafu {
                id: fact_id.as_str(),
            }
            .build()
        })
    }

    /// Async `update_confidence`: wraps sync call in `spawn_blocking`.
    #[instrument(skip(self))]
    pub async fn update_confidence_async(
        self: &std::sync::Arc<Self>,
        fact_id: crate::id::FactId,
        confidence: f64,
    ) -> crate::error::Result<crate::knowledge::Fact> {
        use snafu::ResultExt;
        let this = std::sync::Arc::clone(self);
        tokio::task::spawn_blocking(move || this.update_confidence(&fact_id, confidence))
            .await
            .context(crate::error::JoinSnafu)?
    }

    /// Set the data-sovereignty sensitivity for a fact (#3404, #3413).
    ///
    /// Updates every temporal record for the given fact so recall sees a
    /// consistent classification regardless of which snapshot is returned.
    ///
    /// Returns the updated fact. Errors if no fact with the given ID exists.
    ///
    #[instrument(skip(self))]
    pub(crate) fn update_sensitivity(
        &self,
        fact_id: &crate::id::FactId,
        sensitivity: crate::knowledge::FactSensitivity,
    ) -> crate::error::Result<crate::knowledge::Fact> {
        let existing = self.read_facts_by_id_raw(fact_id.as_str())?;
        if existing.is_empty() {
            return Err(crate::error::FactNotFoundSnafu {
                id: fact_id.as_str(),
            }
            .build());
        }
        for mut fact in existing {
            fact.sensitivity = sensitivity;
            self.insert_fact(&fact)?;
        }
        let updated = self.read_facts_by_id(fact_id.as_str())?;
        updated.into_iter().next().ok_or_else(|| {
            crate::error::FactNotFoundSnafu {
                id: fact_id.as_str(),
            }
            .build()
        })
    }

    /// Async `update_sensitivity`: wraps sync call in `spawn_blocking`.
    #[instrument(skip(self))]
    pub async fn update_sensitivity_async(
        self: &std::sync::Arc<Self>,
        fact_id: crate::id::FactId,
        sensitivity: crate::knowledge::FactSensitivity,
    ) -> crate::error::Result<crate::knowledge::Fact> {
        use snafu::ResultExt;
        let this = std::sync::Arc::clone(self);
        tokio::task::spawn_blocking(move || this.update_sensitivity(&fact_id, sensitivity))
            .await
            .context(crate::error::JoinSnafu)?
    }

    /// Async `insert_fact`: wraps sync call in `spawn_blocking`.
    #[instrument(skip(self, fact), fields(fact_id = %fact.id))]
    pub async fn insert_fact_async(
        self: &std::sync::Arc<Self>,
        fact: crate::knowledge::Fact,
    ) -> crate::error::Result<()> {
        use snafu::ResultExt;
        let this = std::sync::Arc::clone(self);
        tokio::task::spawn_blocking(move || this.insert_fact(&fact))
            .await
            .context(crate::error::JoinSnafu)?
    }

    /// Query facts by type for a specific nous, ordered by `recorded_at` descending.
    ///
    /// Useful for retrieving audit results or other typed fact categories.
    ///
    /// # Errors
    ///
    /// Returns an error if the Datalog query fails.
    #[instrument(skip(self))]
    pub fn query_facts_by_type(
        &self,
        nous_id: &str,
        fact_type: &str,
        limit: i64,
    ) -> crate::error::Result<Vec<crate::knowledge::Fact>> {
        use std::collections::BTreeMap;

        use crate::engine::DataValue;
        let mut params = BTreeMap::new();
        params.insert(String::from("nous_id"), DataValue::Str(nous_id.into()));
        params.insert(String::from("fact_type"), DataValue::Str(fact_type.into()));
        params.insert(String::from("limit"), DataValue::from(limit));

        let script = r"
            ?[id, valid_from, content, nous_id, confidence, tier, valid_to,
              superseded_by, source_session_id, recorded_at,
              access_count, last_accessed_at, stability_hours, fact_type,
              is_forgotten, forgotten_at, forget_reason, scope, project_id,
              visibility, sensitivity] :=
                *facts{id, valid_from, content, nous_id, confidence, tier,
                       valid_to, superseded_by, source_session_id, recorded_at,
                       access_count, last_accessed_at, stability_hours, fact_type,
                       is_forgotten, forgotten_at, forget_reason, scope, project_id,
                       visibility, sensitivity},
                nous_id == $nous_id,
                fact_type == $fact_type,
                is_forgotten == false
            :order -recorded_at
            :limit $limit
        ";
        let rows = self.run_read(script, params)?;
        self.rows_to_raw_facts_with_access(rows)
    }

    /// Async `query_facts_by_type`: wraps sync call in `spawn_blocking`.
    #[instrument(skip(self))]
    pub async fn query_facts_by_type_async(
        self: &std::sync::Arc<Self>,
        nous_id: String,
        fact_type: String,
        limit: i64,
    ) -> crate::error::Result<Vec<crate::knowledge::Fact>> {
        use snafu::ResultExt;
        let this = std::sync::Arc::clone(self);
        tokio::task::spawn_blocking(move || this.query_facts_by_type(&nous_id, &fact_type, limit))
            .await
            .context(crate::error::JoinSnafu)?
    }

    /// Async `query_facts`: wraps sync call in `spawn_blocking`.
    #[instrument(skip(self))]
    pub async fn query_facts_async(
        self: &std::sync::Arc<Self>,
        nous_id: String,
        now: String,
        limit: i64,
    ) -> crate::error::Result<Vec<crate::knowledge::Fact>> {
        use snafu::ResultExt;
        let this = std::sync::Arc::clone(self);
        tokio::task::spawn_blocking(move || this.query_facts(&nous_id, &now, limit))
            .await
            .context(crate::error::JoinSnafu)?
    }

    /// Async `query_visible_facts`: wraps sync call in `spawn_blocking`.
    #[instrument(skip(self))]
    pub async fn query_visible_facts_async(
        self: &std::sync::Arc<Self>,
        requester_nous_id: String,
        now: String,
        limit: i64,
    ) -> crate::error::Result<Vec<crate::knowledge::Fact>> {
        use snafu::ResultExt;
        let this = std::sync::Arc::clone(self);
        tokio::task::spawn_blocking(move || {
            this.query_visible_facts(&requester_nous_id, &now, limit)
        })
        .await
        .context(crate::error::JoinSnafu)?
    }
}

fn insert_fact_embedding(
    store: &KnowledgeStore,
    fact: &crate::knowledge::Fact,
    embedding: Vec<f32>,
    inserted: &mut u64,
    failures: &mut u32,
) {
    use crate::id::EmbeddingId;
    use crate::knowledge::EmbeddedChunk;

    let Ok(id) = EmbeddingId::new(format!("emb-{}", fact.id.as_str())) else {
        tracing::warn!(
            fact_id = %fact.id,
            "backfill_fact_embeddings: invalid embedding id; skipping"
        );
        *failures = failures.saturating_add(1);
        return;
    };

    let chunk = EmbeddedChunk {
        id,
        content: fact.content.clone(),
        source_type: "fact".to_owned(),
        source_id: fact.id.as_str().to_owned(),
        nous_id: fact.nous_id.clone(),
        embedding,
        created_at: fact.temporal.recorded_at,
    };

    if let Err(err) = store.insert_embedding(&chunk) {
        tracing::warn!(
            fact_id = %fact.id,
            error = %err,
            "backfill_fact_embeddings: failed to insert embedding"
        );
        *failures = failures.saturating_add(1);
    } else {
        *inserted = inserted.saturating_add(1);
    }
}

#[cfg(all(test, feature = "mneme-engine"))]
mod tests {
    #![expect(clippy::expect_used, reason = "test assertions")]
    #![expect(
        clippy::indexing_slicing,
        reason = "knowledge engine: ported codebase with numeric casts and direct indexing throughout"
    )]

    use crate::knowledge::ForgetReason;
    use crate::test_fixtures::{make_fact, make_store};

    // ── Insertion ──────────────────────────────────────────────────────────────

    #[test]
    fn insert_fact_valid_roundtrips() {
        let store = make_store();
        let fact = make_fact("ins-1", "alice", "Alice uses dark mode");
        store.insert_fact(&fact).expect("insert valid fact");

        let results = store
            .query_facts("alice", "2026-06-01", 10)
            .expect("query after insert");
        assert_eq!(results.len(), 1, "one fact should be present after insert");
        assert_eq!(results[0].id.as_str(), "ins-1");
        assert_eq!(results[0].content, "Alice uses dark mode");
    }

    #[test]
    fn insert_fact_duplicate_id_upserts_not_duplicates() {
        let store = make_store();
        let mut fact = make_fact("dup-1", "alice", "Original content");
        store.insert_fact(&fact).expect("first insert");

        fact.content = "Updated content".to_owned();
        store.insert_fact(&fact).expect("upsert");

        let results = store
            .query_facts("alice", "2026-06-01", 100)
            .expect("query after upsert");
        assert_eq!(results.len(), 1, "upsert must not create a duplicate row");
        assert_eq!(
            results[0].content, "Updated content",
            "content should be the updated value"
        );
    }

    #[test]
    fn insert_fact_empty_content_rejected() {
        let store = make_store();
        let fact = make_fact("empty-content", "alice", "");
        let result = store.insert_fact(&fact);
        assert!(result.is_err(), "empty content should be rejected");
        assert!(
            matches!(
                result.expect_err("must fail"),
                crate::error::Error::EmptyContent { .. }
            ),
            "error variant should be EmptyContent"
        );
    }

    #[test]
    fn insert_fact_content_too_long_rejected() {
        let store = make_store();
        // MAX_CONTENT_LENGTH is 102_400; exceed it by one byte.
        let long = "x".repeat(crate::knowledge::MAX_CONTENT_LENGTH + 1);
        let mut fact = make_fact("too-long", "alice", "placeholder");
        fact.content = long;
        let result = store.insert_fact(&fact);
        assert!(
            result.is_err(),
            "content exceeding max length should be rejected"
        );
        assert!(
            matches!(
                result.expect_err("must fail"),
                crate::error::Error::ContentTooLong { .. }
            ),
            "error variant should be ContentTooLong"
        );
    }

    #[test]
    fn insert_fact_confidence_above_one_rejected() {
        let store = make_store();
        let mut fact = make_fact("conf-high", "alice", "over confidence");
        fact.provenance.confidence = 1.1;
        let result = store.insert_fact(&fact);
        assert!(result.is_err(), "confidence > 1.0 must be rejected");
        assert!(
            matches!(
                result.expect_err("must fail"),
                crate::error::Error::InvalidConfidence { .. }
            ),
            "error variant should be InvalidConfidence"
        );
    }

    #[test]
    fn insert_fact_confidence_below_zero_rejected() {
        let store = make_store();
        let mut fact = make_fact("conf-neg", "alice", "negative confidence");
        fact.provenance.confidence = -0.1;
        let result = store.insert_fact(&fact);
        assert!(result.is_err(), "confidence < 0.0 must be rejected");
        assert!(
            matches!(
                result.expect_err("must fail"),
                crate::error::Error::InvalidConfidence { .. }
            ),
            "error variant should be InvalidConfidence"
        );
    }

    #[test]
    fn insert_fact_boundary_confidence_values_accepted() {
        let store = make_store();

        let mut fact_zero = make_fact("conf-zero", "alice", "zero confidence");
        fact_zero.provenance.confidence = 0.0;
        store
            .insert_fact(&fact_zero)
            .expect("confidence 0.0 should be accepted");

        let mut fact_one = make_fact("conf-one", "alice", "full confidence");
        fact_one.provenance.confidence = 1.0;
        store
            .insert_fact(&fact_one)
            .expect("confidence 1.0 should be accepted");

        let results = store
            .query_facts("alice", "2026-06-01", 100)
            .expect("query boundary confidence facts");
        assert_eq!(
            results.len(),
            2,
            "both boundary-confidence facts should be stored"
        );
    }

    // ── Query operations ───────────────────────────────────────────────────────

    #[test]
    fn query_facts_empty_store_returns_empty() {
        let store = make_store();
        let results = store
            .query_facts("alice", "2026-06-01", 100)
            .expect("query empty store");
        assert!(results.is_empty(), "empty store should return no facts");
    }

    #[test]
    fn query_facts_single_fact() {
        let store = make_store();
        store
            .insert_fact(&make_fact("q-single", "alice", "Single stored fact"))
            .expect("insert");
        let results = store
            .query_facts("alice", "2026-06-01", 100)
            .expect("query single fact");
        assert_eq!(
            results.len(),
            1,
            "single stored fact should return 1 result"
        );
        assert_eq!(results[0].id.as_str(), "q-single");
    }

    #[test]
    fn query_facts_multiple_facts_all_returned() {
        let store = make_store();
        for i in 0..3_u8 {
            store
                .insert_fact(&make_fact(
                    &format!("q-multi-{i}"),
                    "alice",
                    &format!("Fact {i}"),
                ))
                .expect("insert");
        }
        let results = store
            .query_facts("alice", "2026-06-01", 100)
            .expect("query multiple facts");
        assert_eq!(results.len(), 3, "all three facts should be returned");
    }

    #[test]
    fn query_facts_by_type_empty_store_returns_empty() {
        let store = make_store();
        let results = store
            .query_facts_by_type("alice", "preference", 100)
            .expect("query by type on empty store");
        assert!(
            results.is_empty(),
            "empty store should return no typed facts"
        );
    }

    #[test]
    fn query_facts_by_type_single_match() {
        let store = make_store();
        let mut fact = make_fact("qt-1", "alice", "Alice prefers Rust");
        fact.fact_type = "preference".to_owned();
        store.insert_fact(&fact).expect("insert typed fact");

        let results = store
            .query_facts_by_type("alice", "preference", 100)
            .expect("query by type");
        assert_eq!(results.len(), 1, "one typed fact should be returned");
        assert_eq!(results[0].id.as_str(), "qt-1");
        assert_eq!(results[0].fact_type, "preference");
    }

    #[test]
    fn query_facts_by_type_filters_by_type() {
        let store = make_store();
        let mut pref = make_fact("qt-pref", "alice", "Alice prefers Rust");
        pref.fact_type = "preference".to_owned();
        store.insert_fact(&pref).expect("insert preference");

        let mut obs = make_fact("qt-obs", "alice", "Alice attended standup");
        obs.fact_type = "observation".to_owned();
        store.insert_fact(&obs).expect("insert observation");

        let prefs = store
            .query_facts_by_type("alice", "preference", 100)
            .expect("query preference type");
        assert_eq!(
            prefs.len(),
            1,
            "only the preference fact should be returned"
        );
        assert_eq!(prefs[0].id.as_str(), "qt-pref");

        let obs_results = store
            .query_facts_by_type("alice", "observation", 100)
            .expect("query observation type");
        assert_eq!(
            obs_results.len(),
            1,
            "only the observation fact should be returned"
        );
        assert_eq!(obs_results[0].id.as_str(), "qt-obs");
    }

    #[test]
    fn query_facts_by_type_excludes_wrong_nous_id() {
        let store = make_store();
        let mut fact = make_fact("qt-bob", "bob", "Bob fact");
        fact.fact_type = "preference".to_owned();
        store.insert_fact(&fact).expect("insert bob fact");

        let results = store
            .query_facts_by_type("alice", "preference", 100)
            .expect("query alice preference");
        assert!(
            results.is_empty(),
            "bob's facts must not appear in alice's query"
        );
    }

    #[test]
    fn query_facts_by_type_excludes_forgotten() {
        let store = make_store();
        let mut fact = make_fact("qt-forgotten", "alice", "Forgotten typed fact");
        fact.fact_type = "preference".to_owned();
        store.insert_fact(&fact).expect("insert");
        store
            .forget_fact(
                &crate::id::FactId::new("qt-forgotten").expect("valid test id"),
                ForgetReason::Outdated,
            )
            .expect("forget");

        let results = store
            .query_facts_by_type("alice", "preference", 100)
            .expect("query after forget");
        assert!(
            results.is_empty(),
            "forgotten facts must not appear in typed query"
        );
    }

    #[test]
    fn list_all_facts_empty_store_returns_empty() {
        let store = make_store();
        let all = store.list_all_facts(100).expect("list_all_facts empty");
        assert!(
            all.is_empty(),
            "list_all_facts on empty store should return empty"
        );
    }

    #[test]
    fn list_all_facts_returns_facts_across_agents() {
        let store = make_store();
        store
            .insert_fact(&make_fact("la-1", "alice", "Alice fact"))
            .expect("insert alice");
        store
            .insert_fact(&make_fact("la-2", "bob", "Bob fact"))
            .expect("insert bob");

        let all = store.list_all_facts(100).expect("list_all_facts");
        assert_eq!(all.len(), 2, "both agents' facts must be returned");
        let ids: Vec<&str> = all.iter().map(|f| f.id.as_str()).collect();
        assert!(
            ids.contains(&"la-1"),
            "alice's fact must be in list_all_facts"
        );
        assert!(
            ids.contains(&"la-2"),
            "bob's fact must be in list_all_facts"
        );
    }

    #[test]
    fn list_all_facts_respects_limit() {
        let store = make_store();
        for i in 0..5_u8 {
            store
                .insert_fact(&make_fact(
                    &format!("la-limit-{i}"),
                    "alice",
                    &format!("Fact {i}"),
                ))
                .expect("insert");
        }
        let limited = store.list_all_facts(2).expect("list_all_facts limit 2");
        assert_eq!(
            limited.len(),
            2,
            "list_all_facts should honour the limit parameter"
        );
    }

    // ── Soft-delete lifecycle ──────────────────────────────────────────────────

    #[test]
    fn forget_fact_marks_as_forgotten() {
        let store = make_store();
        store
            .insert_fact(&make_fact("sd-1", "alice", "Will be forgotten"))
            .expect("insert");

        let forgotten = store
            .forget_fact(
                &crate::id::FactId::new("sd-1").expect("valid test id"),
                ForgetReason::UserRequested,
            )
            .expect("forget");
        assert!(
            forgotten.lifecycle.is_forgotten,
            "returned fact should be marked forgotten"
        );
        assert_eq!(
            forgotten.lifecycle.forget_reason,
            Some(ForgetReason::UserRequested),
            "forget reason must be preserved on the returned fact"
        );
        assert!(
            forgotten.lifecycle.forgotten_at.is_some(),
            "forgotten_at timestamp must be set after forget"
        );
    }

    #[test]
    fn forget_fact_excluded_from_query_facts() {
        let store = make_store();
        store
            .insert_fact(&make_fact("sd-2", "alice", "Excluded after forget"))
            .expect("insert");
        store
            .forget_fact(
                &crate::id::FactId::new("sd-2").expect("valid test id"),
                ForgetReason::Privacy,
            )
            .expect("forget");

        let results = store
            .query_facts("alice", "2026-06-01", 100)
            .expect("query after forget");
        assert!(
            results.is_empty(),
            "forgotten fact must not appear in query_facts"
        );
    }

    #[test]
    fn unforget_fact_restores_recall_visibility() {
        let store = make_store();
        store
            .insert_fact(&make_fact("sd-3", "alice", "Will be restored"))
            .expect("insert");
        store
            .forget_fact(
                &crate::id::FactId::new("sd-3").expect("valid test id"),
                ForgetReason::Incorrect,
            )
            .expect("forget");

        let restored = store
            .unforget_fact(&crate::id::FactId::new("sd-3").expect("valid test id"))
            .expect("unforget");
        assert!(
            !restored.lifecycle.is_forgotten,
            "restored fact must not be marked as forgotten"
        );
        assert!(
            restored.lifecycle.forgotten_at.is_none(),
            "forgotten_at must be cleared after unforget"
        );
        assert!(
            restored.lifecycle.forget_reason.is_none(),
            "forget_reason must be cleared after unforget"
        );

        let results = store
            .query_facts("alice", "2026-06-01", 100)
            .expect("query after unforget");
        assert_eq!(results.len(), 1, "unforget must restore recall visibility");
        assert_eq!(results[0].id.as_str(), "sd-3");
    }

    #[test]
    fn list_forgotten_returns_only_forgotten_facts() {
        let store = make_store();
        store
            .insert_fact(&make_fact("lf-visible", "alice", "Visible fact"))
            .expect("insert visible");
        store
            .insert_fact(&make_fact("lf-gone", "alice", "Forgotten fact"))
            .expect("insert forgotten");
        store
            .forget_fact(
                &crate::id::FactId::new("lf-gone").expect("valid test id"),
                ForgetReason::Outdated,
            )
            .expect("forget");

        let forgotten = store.list_forgotten("alice", 100).expect("list_forgotten");
        assert_eq!(
            forgotten.len(),
            1,
            "list_forgotten should return only the forgotten fact"
        );
        assert_eq!(forgotten[0].id.as_str(), "lf-gone");
        assert!(
            forgotten[0].lifecycle.is_forgotten,
            "listed fact must be marked as forgotten"
        );
    }

    #[test]
    fn forget_nonexistent_fact_errors() {
        let store = make_store();
        let result = store.forget_fact(
            &crate::id::FactId::new("does-not-exist").expect("valid test id"),
            ForgetReason::UserRequested,
        );
        assert!(result.is_err(), "forgetting a non-existent fact must error");
        let msg = result.expect_err("must fail").to_string();
        assert!(
            msg.contains("not found"),
            "error should mention 'not found': {msg}"
        );
    }

    #[test]
    fn unforget_nonexistent_fact_errors() {
        let store = make_store();
        let result =
            store.unforget_fact(&crate::id::FactId::new("does-not-exist").expect("valid test id"));
        assert!(
            result.is_err(),
            "unforgetting a non-existent fact must error"
        );
    }

    // ── Confidence updates ─────────────────────────────────────────────────────

    #[test]
    fn update_confidence_valid_value() {
        let store = make_store();
        store
            .insert_fact(&make_fact("uc-1", "alice", "Confidence target"))
            .expect("insert");

        let updated = store
            .update_confidence(&crate::id::FactId::new("uc-1").expect("valid test id"), 0.5)
            .expect("update_confidence 0.5");
        assert!(
            (updated.provenance.confidence - 0.5).abs() < f64::EPSILON,
            "confidence should be updated to 0.5"
        );

        let results = store
            .query_facts("alice", "2026-06-01", 100)
            .expect("query after confidence update");
        assert_eq!(
            results.len(),
            1,
            "confidence update should not duplicate fact"
        );
        assert!(
            (results[0].provenance.confidence - 0.5).abs() < f64::EPSILON,
            "persisted confidence should be 0.5"
        );
    }

    #[test]
    fn update_confidence_boundary_zero() {
        let store = make_store();
        store
            .insert_fact(&make_fact("uc-zero", "alice", "Zero confidence target"))
            .expect("insert");

        let updated = store
            .update_confidence(
                &crate::id::FactId::new("uc-zero").expect("valid test id"),
                0.0,
            )
            .expect("update_confidence 0.0");
        assert!(
            updated.provenance.confidence.abs() < f64::EPSILON,
            "confidence should be updated to 0.0"
        );
    }

    #[test]
    fn update_confidence_boundary_one() {
        let store = make_store();
        store
            .insert_fact(&make_fact("uc-one", "alice", "Full confidence target"))
            .expect("insert");

        let updated = store
            .update_confidence(
                &crate::id::FactId::new("uc-one").expect("valid test id"),
                1.0,
            )
            .expect("update_confidence 1.0");
        assert!(
            (updated.provenance.confidence - 1.0).abs() < f64::EPSILON,
            "confidence should be updated to 1.0"
        );
    }

    #[test]
    fn update_confidence_out_of_range_high_errors() {
        let store = make_store();
        store
            .insert_fact(&make_fact("uc-hi", "alice", "High confidence target"))
            .expect("insert");

        let result = store.update_confidence(
            &crate::id::FactId::new("uc-hi").expect("valid test id"),
            1.5,
        );
        assert!(
            result.is_err(),
            "confidence > 1.0 must be rejected by update_confidence"
        );
        assert!(
            matches!(
                result.expect_err("must fail"),
                crate::error::Error::InvalidConfidence { .. }
            ),
            "error variant should be InvalidConfidence"
        );
    }

    #[test]
    fn update_confidence_out_of_range_low_errors() {
        let store = make_store();
        store
            .insert_fact(&make_fact("uc-lo", "alice", "Low confidence target"))
            .expect("insert");

        let result = store.update_confidence(
            &crate::id::FactId::new("uc-lo").expect("valid test id"),
            -0.1,
        );
        assert!(
            result.is_err(),
            "confidence < 0.0 must be rejected by update_confidence"
        );
        assert!(
            matches!(
                result.expect_err("must fail"),
                crate::error::Error::InvalidConfidence { .. }
            ),
            "error variant should be InvalidConfidence"
        );
    }

    #[test]
    fn update_confidence_nonexistent_fact_errors() {
        let store = make_store();
        let result = store.update_confidence(
            &crate::id::FactId::new("no-such-fact").expect("valid test id"),
            0.5,
        );
        assert!(
            result.is_err(),
            "update_confidence on missing fact must error"
        );
        let msg = result.expect_err("must fail").to_string();
        assert!(
            msg.contains("not found"),
            "error should mention 'not found': {msg}"
        );
    }

    // ── Temporal queries ───────────────────────────────────────────────────────

    #[test]
    fn query_facts_at_returns_fact_within_validity_window() {
        let store = make_store();
        let mut fact = make_fact("temp-1", "alice", "Temporal fact");
        fact.temporal.valid_from =
            crate::knowledge::parse_timestamp("2026-01-01").expect("valid_from");
        fact.temporal.valid_to = crate::knowledge::parse_timestamp("2026-06-01").expect("valid_to");
        store.insert_fact(&fact).expect("insert temporal fact");

        let results = store
            .query_facts_at("2026-03-15")
            .expect("query at mid-window");
        assert_eq!(
            results.len(),
            1,
            "fact should be visible inside its validity window"
        );
        assert_eq!(results[0].id.as_str(), "temp-1");
    }

    #[test]
    fn query_facts_at_excludes_fact_after_validity_window() {
        let store = make_store();
        let mut fact = make_fact("temp-2", "alice", "Expired temporal fact");
        fact.temporal.valid_from =
            crate::knowledge::parse_timestamp("2026-01-01").expect("valid_from");
        fact.temporal.valid_to = crate::knowledge::parse_timestamp("2026-06-01").expect("valid_to");
        store.insert_fact(&fact).expect("insert expired fact");

        let results = store
            .query_facts_at("2026-07-01")
            .expect("query after window closes");
        assert!(
            results.is_empty(),
            "fact should not be visible after its validity window ends"
        );
    }

    #[test]
    fn query_facts_temporal_returns_facts_valid_at_time() {
        let store = make_store();

        let mut early = make_fact("temp-early", "alice", "Early fact");
        early.temporal.valid_from =
            crate::knowledge::parse_timestamp("2026-01-01").expect("valid_from early");
        early.temporal.valid_to =
            crate::knowledge::parse_timestamp("2026-04-01").expect("valid_to early");
        store.insert_fact(&early).expect("insert early");

        let mut late = make_fact("temp-late", "alice", "Late fact");
        late.temporal.valid_from =
            crate::knowledge::parse_timestamp("2026-05-01").expect("valid_from late");
        late.temporal.valid_to = crate::knowledge::far_future();
        store.insert_fact(&late).expect("insert late");

        let at_feb = store
            .query_facts_temporal("alice", "2026-02-01", None)
            .expect("query feb");
        assert_eq!(
            at_feb.len(),
            1,
            "only the early fact should be visible in February"
        );
        assert_eq!(at_feb[0].id.as_str(), "temp-early");

        let at_jun = store
            .query_facts_temporal("alice", "2026-06-01", None)
            .expect("query june");
        assert_eq!(
            at_jun.len(),
            1,
            "only the late fact should be visible in June"
        );
        assert_eq!(at_jun[0].id.as_str(), "temp-late");
    }

    #[test]
    fn query_facts_diff_detects_added_and_removed() {
        let store = make_store();

        let mut removed = make_fact("diff-old", "alice", "Old knowledge");
        removed.temporal.valid_from =
            crate::knowledge::parse_timestamp("2026-01-01").expect("valid_from old");
        removed.temporal.valid_to =
            crate::knowledge::parse_timestamp("2026-03-01").expect("valid_to old");
        store.insert_fact(&removed).expect("insert old");

        let mut added = make_fact("diff-new", "alice", "New knowledge");
        added.temporal.valid_from =
            crate::knowledge::parse_timestamp("2026-02-15").expect("valid_from new");
        added.temporal.valid_to = crate::knowledge::far_future();
        store.insert_fact(&added).expect("insert new");

        let diff = store
            .query_facts_diff("alice", "2026-02-01", "2026-04-01")
            .expect("query diff");
        assert_eq!(diff.added.len(), 1, "one fact should be in the added set");
        assert_eq!(diff.added[0].id.as_str(), "diff-new");
        assert_eq!(
            diff.removed.len(),
            1,
            "one fact should be in the removed set"
        );
        assert_eq!(diff.removed[0].id.as_str(), "diff-old");
        assert!(diff.modified.is_empty(), "no modified pairs expected");
    }

    // ── Error paths: invalid fact_id ───────────────────────────────────────────

    #[test]
    fn read_facts_by_id_unknown_id_returns_empty() {
        let store = make_store();
        let results = store
            .read_facts_by_id("unknown-id")
            .expect("read by id succeeds");
        assert!(
            results.is_empty(),
            "reading an unknown id should return an empty vec, not an error"
        );
    }

    // ── query_forgotten_ids ────────────────────────────────────────────────────

    #[test]
    fn query_forgotten_ids_empty_input_returns_empty_set() {
        let store = make_store();
        let ids = store.query_forgotten_ids(&[]).expect("empty input");
        assert!(ids.is_empty(), "empty input should return empty set");
    }

    #[test]
    fn query_forgotten_ids_returns_only_forgotten() {
        let store = make_store();
        store
            .insert_fact(&make_fact("qfi-visible", "alice", "Visible"))
            .expect("insert visible");
        store
            .insert_fact(&make_fact("qfi-gone", "alice", "Forgotten"))
            .expect("insert forgotten");
        store
            .forget_fact(
                &crate::id::FactId::new("qfi-gone").expect("valid test id"),
                ForgetReason::Privacy,
            )
            .expect("forget");

        let forgotten_ids = store
            .query_forgotten_ids(&["qfi-visible", "qfi-gone"])
            .expect("query_forgotten_ids");
        assert!(
            !forgotten_ids.contains("qfi-visible"),
            "visible fact must not appear in forgotten ids"
        );
        assert!(
            forgotten_ids.contains("qfi-gone"),
            "forgotten fact must appear in forgotten ids"
        );
    }

    // ── Supersede provenance ───────────────────────────────────────────────────

    #[test]
    fn supersede_preserves_forget_provenance_on_old_row() {
        let store = make_store();
        let old_fact = make_fact("sup-old", "alice", "Old forgotten fact");
        store.insert_fact(&old_fact).expect("insert old");
        store
            .forget_fact(
                &crate::id::FactId::new("sup-old").expect("valid test id"),
                ForgetReason::Outdated,
            )
            .expect("forget old");

        let new_fact = make_fact("sup-new", "alice", "Replacement fact");
        let forgotten = store
            .read_facts_by_id("sup-old")
            .expect("read old before supersede")
            .into_iter()
            .next()
            .expect("old fact exists");
        let original_forgotten_at = forgotten.lifecycle.forgotten_at.expect("forgotten_at set");

        store
            .supersede_fact(&forgotten, &new_fact)
            .expect("supersede");

        let versions = store
            .read_facts_by_id("sup-old")
            .expect("read after supersede");
        let superseded = versions
            .iter()
            .find(|f| f.lifecycle.superseded_by.is_some())
            .expect("find superseded old row");
        assert_eq!(
            superseded.lifecycle.forgotten_at,
            Some(original_forgotten_at),
            "superseded row must retain forgotten_at"
        );
        assert_eq!(
            superseded.lifecycle.forget_reason,
            Some(ForgetReason::Outdated),
            "superseded row must retain forget_reason"
        );
    }
}
