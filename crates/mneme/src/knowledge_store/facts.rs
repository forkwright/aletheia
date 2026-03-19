use tracing::instrument;

use super::marshal::{extract_str, fact_to_params, rows_to_facts};
use super::{KnowledgeStore, queries};
#[cfg(feature = "mneme-engine")]
impl KnowledgeStore {
    /// Insert or update a fact.
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
            (0.0..=1.0).contains(&fact.confidence),
            crate::error::InvalidConfidenceSnafu {
                value: fact.confidence
            }
        );
        let params = fact_to_params(fact);
        self.run_mut(&queries::upsert_fact(), params)
    }

    /// Supersede an existing fact with a new one.
    ///
    /// Sets `valid_to` on the old fact to `now` and `superseded_by` to the new
    /// fact's ID, then inserts the new fact.
    #[instrument(skip(self, old_fact, new_fact), fields(old_id = %old_fact.id, new_id = %new_fact.id))]
    pub fn supersede_fact(
        &self,
        old_fact: &crate::knowledge::Fact,
        new_fact: &crate::knowledge::Fact,
    ) -> crate::error::Result<()> {
        use crate::engine::DataValue;
        use crate::knowledge::format_timestamp;
        use std::collections::BTreeMap;

        let now = jiff::Timestamp::now();
        let now_str = format_timestamp(&now);

        let mut params = BTreeMap::new();
        params.insert(
            String::from("old_id"),
            DataValue::Str(old_fact.id.as_str().into()),
        );
        params.insert(
            String::from("old_valid_from"),
            DataValue::Str(format_timestamp(&old_fact.valid_from).into()),
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
            DataValue::from(old_fact.confidence),
        );
        params.insert(
            String::from("old_tier"),
            DataValue::Str(old_fact.tier.as_str().into()),
        );
        params.insert(String::from("now"), DataValue::Str(now_str.as_str().into()));
        params.insert(
            String::from("new_id"),
            DataValue::Str(new_fact.id.as_str().into()),
        );
        params.insert(
            String::from("old_source"),
            DataValue::Str(old_fact.source_session_id.as_deref().unwrap_or("").into()),
        );
        params.insert(
            String::from("old_recorded"),
            DataValue::Str(format_timestamp(&old_fact.recorded_at).into()),
        );
        params.insert(
            String::from("old_access_count"),
            DataValue::from(i64::from(old_fact.access_count)),
        );
        params.insert(
            String::from("old_last_accessed_at"),
            DataValue::Str(
                old_fact
                    .last_accessed_at
                    .as_ref()
                    .map(format_timestamp)
                    .unwrap_or_default()
                    .into(),
            ),
        );
        params.insert(
            String::from("old_stability_hours"),
            DataValue::from(old_fact.stability_hours),
        );
        params.insert(
            String::from("old_fact_type"),
            DataValue::Str(old_fact.fact_type.as_str().into()),
        );
        params.insert(
            String::from("old_is_forgotten"),
            DataValue::Bool(old_fact.is_forgotten),
        );
        params.insert(String::from("old_forgotten_at"), DataValue::Null);
        params.insert(String::from("old_forget_reason"), DataValue::Null);

        params.insert(
            String::from("new_content"),
            DataValue::Str(new_fact.content.as_str().into()),
        );
        params.insert(
            String::from("new_confidence"),
            DataValue::from(new_fact.confidence),
        );
        params.insert(
            String::from("new_tier"),
            DataValue::Str(new_fact.tier.as_str().into()),
        );
        params.insert(
            String::from("source_session_id"),
            DataValue::Str(new_fact.source_session_id.as_deref().unwrap_or("").into()),
        );
        params.insert(
            String::from("stability_hours"),
            DataValue::from(new_fact.stability_hours),
        );
        params.insert(
            String::from("fact_type"),
            DataValue::Str(new_fact.fact_type.as_str().into()),
        );

        self.run_mut(&queries::supersede_fact(), params)
    }

    /// Query current facts for a nous at a given time, up to limit results.
    #[instrument(skip(self))]
    pub fn query_facts(
        &self,
        nous_id: &str,
        now: &str,
        limit: i64,
    ) -> crate::error::Result<Vec<crate::knowledge::Fact>> {
        use crate::engine::DataValue;
        use std::collections::BTreeMap;

        let mut params = BTreeMap::new();
        params.insert(String::from("nous_id"), DataValue::Str(nous_id.into()));
        params.insert(String::from("now"), DataValue::Str(now.into()));
        params.insert(String::from("limit"), DataValue::from(limit));

        let rows = self.run_read(&queries::full_current_facts(), params)?;
        rows_to_facts(rows, nous_id)
    }

    /// Point-in-time fact query.
    #[instrument(skip(self))]
    pub fn query_facts_at(&self, time: &str) -> crate::error::Result<Vec<crate::knowledge::Fact>> {
        use crate::engine::DataValue;
        use std::collections::BTreeMap;

        let mut params = BTreeMap::new();
        params.insert(String::from("time"), DataValue::Str(time.into()));

        let rows = self.run_read(&queries::facts_at_time(), params)?;
        super::marshal::rows_to_facts_partial(rows)
    }

    /// Increment access count and update last-accessed timestamp for the given fact IDs.
    #[instrument(skip(self), fields(count = fact_ids.len()))]
    pub fn increment_access(&self, fact_ids: &[crate::id::FactId]) -> crate::error::Result<()> {
        if fact_ids.is_empty() {
            return Ok(());
        }
        let now = jiff::Timestamp::now();
        for id in fact_ids {
            // WHY: CozoDB in-memory read-modify-write in a single Datalog rule does not
            // reflect the mutation in subsequent reads, so we read-increment-write in Rust.
            let facts = match self.read_facts_by_id(id.as_str()) {
                Ok(f) => f,
                Err(e) => {
                    tracing::warn!(error = %e, fact_id = %id, "failed to read fact for access increment");
                    continue;
                }
            };
            for mut fact in facts {
                fact.access_count = fact.access_count.saturating_add(1);
                fact.last_accessed_at = Some(now);
                if let Err(e) = self.insert_fact(&fact) {
                    tracing::warn!(error = %e, fact_id = %id, "failed to write incremented access count");
                }
            }
        }
        Ok(())
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
        let existing = self.read_facts_by_id(fact_id.as_str())?;
        if existing.is_empty() {
            return Err(crate::error::FactNotFoundSnafu {
                id: fact_id.as_str(),
            }
            .build());
        }

        let now = crate::knowledge::format_timestamp(&jiff::Timestamp::now());
        let script = r"
            ?[id, valid_from, content, nous_id, confidence, tier, valid_to,
              superseded_by, source_session_id, recorded_at,
              access_count, last_accessed_at, stability_hours, fact_type,
              is_forgotten, forgotten_at, forget_reason] :=
                *facts{id, valid_from, content, nous_id, confidence, tier,
                       valid_to, superseded_by, source_session_id, recorded_at,
                       access_count, last_accessed_at, stability_hours, fact_type},
                id = $id,
                is_forgotten = true,
                forgotten_at = $now,
                forget_reason = $reason
            :put facts {id, valid_from => content, nous_id, confidence, tier,
                        valid_to, superseded_by, source_session_id, recorded_at,
                        access_count, last_accessed_at, stability_hours, fact_type,
                        is_forgotten, forgotten_at, forget_reason}
        ";
        let mut params = std::collections::BTreeMap::new();
        params.insert(
            String::from("id"),
            crate::engine::DataValue::Str(fact_id.as_str().into()),
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
    pub fn unforget_fact(
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
              is_forgotten, forgotten_at, forget_reason] :=
                *facts{id, valid_from, content, nous_id, confidence, tier,
                       valid_to, superseded_by, source_session_id, recorded_at,
                       access_count, last_accessed_at, stability_hours, fact_type},
                id = $id,
                is_forgotten = false,
                forgotten_at = null,
                forget_reason = null
            :put facts {id, valid_from => content, nous_id, confidence, tier,
                        valid_to, superseded_by, source_session_id, recorded_at,
                        access_count, last_accessed_at, stability_hours, fact_type,
                        is_forgotten, forgotten_at, forget_reason}
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
    pub fn list_forgotten(
        &self,
        nous_id: &str,
        limit: i64,
    ) -> crate::error::Result<Vec<crate::knowledge::Fact>> {
        use crate::engine::DataValue;
        use std::collections::BTreeMap;

        let mut params = BTreeMap::new();
        params.insert(String::from("nous_id"), DataValue::Str(nous_id.into()));
        params.insert(String::from("limit"), DataValue::from(limit));
        let rows = self.run_read(&queries::forgotten_facts(), params)?;
        rows_to_facts(rows, nous_id)
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
        use crate::engine::DataValue;
        use std::collections::BTreeMap;

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

    /// Query facts valid at a specific point in time.
    /// Returns facts where `valid_from <= at_time` AND `valid_to > at_time`.
    pub fn query_facts_temporal(
        &self,
        nous_id: &str,
        at_time: &str,
        filter: Option<&str>,
    ) -> crate::error::Result<Vec<crate::knowledge::Fact>> {
        use crate::engine::DataValue;
        use std::collections::BTreeMap;

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
        rows_to_facts(rows, nous_id)
    }

    /// Query facts that changed between two timestamps.
    /// Returns facts where `valid_from` is in `(from_time, to_time]` OR
    /// `valid_to` is in `(from_time, to_time]`.
    pub fn query_facts_diff(
        &self,
        nous_id: &str,
        from_time: &str,
        to_time: &str,
    ) -> crate::error::Result<crate::knowledge::FactDiff> {
        use crate::engine::DataValue;
        use std::collections::BTreeMap;

        let mut params = BTreeMap::new();
        params.insert(String::from("nous_id"), DataValue::Str(nous_id.into()));
        params.insert(String::from("from_time"), DataValue::Str(from_time.into()));
        params.insert(String::from("to_time"), DataValue::Str(to_time.into()));

        let added_rows = self.run_read(queries::TEMPORAL_DIFF_ADDED, params.clone())?;
        let added = rows_to_facts(added_rows, nous_id)?;

        let removed_rows = self.run_read(queries::TEMPORAL_DIFF_REMOVED, params)?;
        let removed = rows_to_facts(removed_rows, nous_id)?;

        // NOTE: A fact in "removed" whose superseded_by points to one in "added" is a
        // modification, not a pure removal.
        let added_ids: std::collections::HashSet<&str> =
            added.iter().map(|f| f.id.as_str()).collect();
        let mut modified = Vec::new();
        let mut pure_removed = Vec::new();

        for old in &removed {
            if let Some(ref new_id) = old.superseded_by
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
        use crate::engine::DataValue;
        use std::collections::BTreeMap;

        let mut params = BTreeMap::new();
        params.insert(String::from("limit"), DataValue::from(limit));

        let script = r"
            ?[id, valid_from, content, nous_id, confidence, tier, valid_to,
              superseded_by, source_session_id, recorded_at,
              access_count, last_accessed_at, stability_hours, fact_type,
              is_forgotten, forgotten_at, forget_reason] :=
                *facts{id, valid_from, content, nous_id, confidence, tier,
                       valid_to, superseded_by, source_session_id, recorded_at,
                       access_count, last_accessed_at, stability_hours, fact_type,
                       is_forgotten, forgotten_at, forget_reason}
            :order -recorded_at
            :limit $limit
        ";
        let rows = self.run_read(script, params)?;
        super::marshal::rows_to_raw_facts(rows)
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
        use crate::engine::DataValue;
        use std::collections::BTreeMap;

        let mut params = BTreeMap::new();
        params.insert(String::from("nous_id"), DataValue::Str(nous_id.into()));
        params.insert(String::from("limit"), DataValue::from(limit));

        let rows = self.run_read(&queries::audit_all_facts(), params)?;
        rows_to_facts(rows, nous_id)
    }

    // NOTE: Async wrappers use spawn_blocking because CozoDB is synchronous.

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
}
