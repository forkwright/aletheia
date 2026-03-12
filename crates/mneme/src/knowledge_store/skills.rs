use super::KnowledgeStore;
use super::marshal::{compute_name_similarity, compute_tool_overlap, rows_to_facts};
use tracing::instrument;

#[cfg(feature = "mneme-engine")]
impl KnowledgeStore {
    // --- Skill query helpers ---

    /// Find skills by domain tags, ordered by confidence then access count.
    ///
    /// Filters facts where `fact_type = "skill"` and whose JSON content
    /// contains at least one of the given `domain_tags`.
    #[instrument(skip(self))]
    pub fn find_skills_by_domain(
        &self,
        nous_id: &str,
        domain_tags: &[&str],
        limit: usize,
    ) -> crate::error::Result<Vec<crate::knowledge::Fact>> {
        let all = self.find_skills_for_nous(nous_id, 1000)?;
        let mut matched: Vec<crate::knowledge::Fact> = all
            .into_iter()
            .filter(|fact| {
                // Parse content JSON and check domain_tags
                if let Ok(skill) = serde_json::from_str::<crate::skill::SkillContent>(&fact.content)
                {
                    domain_tags
                        .iter()
                        .any(|tag| skill.domain_tags.iter().any(|dt| dt == tag))
                } else {
                    false
                }
            })
            .collect();
        matched.truncate(limit);
        Ok(matched)
    }

    /// Find all skills for a specific nous, ordered by confidence descending
    /// then access count descending.
    #[instrument(skip(self))]
    pub fn find_skills_for_nous(
        &self,
        nous_id: &str,
        limit: usize,
    ) -> crate::error::Result<Vec<crate::knowledge::Fact>> {
        use crate::engine::DataValue;
        use std::collections::BTreeMap;

        let limit_i64 = i64::try_from(limit).unwrap_or(i64::MAX);
        let mut params = BTreeMap::new();
        params.insert("nous_id".to_owned(), DataValue::Str(nous_id.into()));
        params.insert("limit".to_owned(), DataValue::from(limit_i64));

        let script = r"?[id, content, confidence, tier, recorded_at, nous_id,
              valid_from, valid_to, superseded_by, source_session_id,
              access_count, last_accessed_at, stability_hours, fact_type,
              is_forgotten, forgotten_at, forget_reason] :=
            *facts{id, valid_from, content, nous_id, confidence, tier, valid_to,
                   superseded_by, source_session_id, recorded_at,
                   access_count, last_accessed_at, stability_hours, fact_type,
                   is_forgotten, forgotten_at, forget_reason},
            nous_id = $nous_id,
            fact_type = 'skill',
            is_null(superseded_by),
            is_forgotten == false
        :order -confidence, -access_count
        :limit $limit";

        let rows = self.run_read(script, params)?;
        rows_to_facts(rows, nous_id)
    }

    /// Semantic search for skills matching a task description.
    ///
    /// Uses the existing hybrid search infrastructure but post-filters
    /// to only return skill-type facts.
    #[instrument(skip(self))]
    pub fn search_skills(
        &self,
        nous_id: &str,
        query: &str,
        limit: usize,
    ) -> crate::error::Result<Vec<crate::knowledge::Fact>> {
        use crate::engine::DataValue;
        use std::collections::BTreeMap;

        // BM25 search scoped to skill facts
        let limit_i64 = i64::try_from(limit).unwrap_or(i64::MAX);
        let mut params = BTreeMap::new();
        params.insert("query_text".to_owned(), DataValue::Str(query.into()));
        params.insert("nous_id".to_owned(), DataValue::Str(nous_id.into()));
        params.insert("k".to_owned(), DataValue::from(limit_i64 * 3));
        params.insert("limit".to_owned(), DataValue::from(limit_i64));

        // BM25 search on facts content, then filter to skills for this nous
        let script = r"candidates[id, score] :=
                ~facts:content_fts{id | query: $query_text, k: $k, score_kind: 'bm25', bind_score: score}

            ?[id, content, confidence, tier, recorded_at, nous_id,
              valid_from, valid_to, superseded_by, source_session_id,
              access_count, last_accessed_at, stability_hours, fact_type,
              is_forgotten, forgotten_at, forget_reason] :=
                candidates[id, _score],
                *facts{id, valid_from, content, nous_id, confidence, tier, valid_to,
                       superseded_by, source_session_id, recorded_at,
                       access_count, last_accessed_at, stability_hours, fact_type,
                       is_forgotten, forgotten_at, forget_reason},
                nous_id = $nous_id,
                fact_type = 'skill',
                is_null(superseded_by),
                is_forgotten == false
            :order -confidence
            :limit $limit";

        let rows = self.run_read(script, params)?;
        rows_to_facts(rows, nous_id)
    }

    /// Check if a skill with the given name already exists for this nous.
    ///
    /// Returns the fact ID if found.
    #[instrument(skip(self))]
    pub fn find_skill_by_name(
        &self,
        nous_id: &str,
        skill_name: &str,
    ) -> crate::error::Result<Option<String>> {
        let skills = self.find_skills_for_nous(nous_id, 1000)?;
        for fact in skills {
            if let Ok(content) = serde_json::from_str::<crate::skill::SkillContent>(&fact.content) {
                if content.name == skill_name {
                    return Ok(Some(fact.id.to_string()));
                }
            }
        }
        Ok(None)
    }

    /// Find all pending-review skills for a specific nous.
    ///
    /// Pending skills are stored as facts with `fact_type = "skill_pending"`.
    #[instrument(skip(self))]
    pub fn find_pending_skills(
        &self,
        nous_id: &str,
    ) -> crate::error::Result<Vec<crate::knowledge::Fact>> {
        use crate::engine::DataValue;
        use std::collections::BTreeMap;

        let mut params = BTreeMap::new();
        params.insert("nous_id".to_owned(), DataValue::Str(nous_id.into()));

        let script = r"?[id, content, confidence, tier, recorded_at, nous_id,
              valid_from, valid_to, superseded_by, source_session_id,
              access_count, last_accessed_at, stability_hours, fact_type,
              is_forgotten, forgotten_at, forget_reason] :=
            *facts{id, valid_from, content, nous_id, confidence, tier, valid_to,
                   superseded_by, source_session_id, recorded_at,
                   access_count, last_accessed_at, stability_hours, fact_type,
                   is_forgotten, forgotten_at, forget_reason},
            nous_id = $nous_id,
            fact_type = 'skill_pending',
            is_null(superseded_by),
            is_forgotten == false
        :order -recorded_at";

        let rows = self.run_read(script, params)?;
        rows_to_facts(rows, nous_id)
    }

    /// Approve a pending skill — move it from `skill_pending` to `skill`.
    ///
    /// Supersedes the pending fact and creates a new fact with `fact_type = "skill"`.
    /// Returns the new fact ID.
    #[instrument(skip(self))]
    pub fn approve_pending_skill(
        &self,
        pending_fact_id: &crate::id::FactId,
        nous_id: &str,
    ) -> crate::error::Result<crate::id::FactId> {
        // Read the pending fact
        let pending_facts = self.find_pending_skills(nous_id)?;
        let pending = pending_facts
            .iter()
            .find(|f| f.id == *pending_fact_id)
            .ok_or_else(|| {
                crate::error::EngineQuerySnafu {
                    message: format!("pending skill not found: {pending_fact_id}"),
                }
                .build()
            })?;

        // Parse the PendingSkill to get the inner SkillContent
        let mut pending_skill =
            crate::skills::PendingSkill::from_json(&pending.content).map_err(|e| {
                crate::error::EngineQuerySnafu {
                    message: format!("failed to parse pending skill: {e}"),
                }
                .build()
            })?;
        "approved".clone_into(&mut pending_skill.status);

        // Create the approved skill fact with a ULID-based ID
        let new_id = crate::id::FactId::from(ulid::Ulid::new().to_string());
        let skill_json = serde_json::to_string(&pending_skill.skill).map_err(|e| {
            crate::error::EngineQuerySnafu {
                message: format!("failed to serialize skill: {e}"),
            }
            .build()
        })?;

        let now = jiff::Timestamp::now();
        let approved_fact = crate::knowledge::Fact {
            id: new_id.clone(),
            nous_id: nous_id.to_owned(),
            content: skill_json,
            confidence: 0.8,
            tier: crate::knowledge::EpistemicTier::Verified,
            valid_from: now,
            valid_to: jiff::Timestamp::from_second(i64::MAX / 2).unwrap_or(now),
            superseded_by: None,
            source_session_id: None,
            recorded_at: now,
            access_count: 0,
            last_accessed_at: None,
            stability_hours: 2190.0,
            fact_type: "skill".to_owned(),
            is_forgotten: false,
            forgotten_at: None,
            forget_reason: None,
        };

        self.insert_fact(&approved_fact)?;

        // Supersede the pending fact by forgetting it
        self.forget_fact(pending_fact_id, crate::knowledge::ForgetReason::Outdated)?;

        Ok(new_id)
    }

    /// Reject a pending skill — mark it as forgotten.
    #[instrument(skip(self))]
    pub fn reject_pending_skill(
        &self,
        pending_fact_id: &crate::id::FactId,
    ) -> crate::error::Result<()> {
        self.forget_fact(pending_fact_id, crate::knowledge::ForgetReason::Incorrect)?;
        Ok(())
    }

    // ── Skill quality lifecycle ────────────────────────────────────────────

    /// Compute decay scores for all active skills of a nous and apply retirement.
    ///
    /// Returns `(active, needs_review, retired)` counts.
    #[instrument(skip(self))]
    pub fn run_skill_decay(&self, nous_id: &str) -> crate::error::Result<(usize, usize, usize)> {
        let skills = self.find_skills_for_nous(nous_id, 10_000)?;
        let now_secs = jiff::Timestamp::now().as_second();

        let mut active = 0usize;
        let mut needs_review = 0usize;
        let mut retired = 0usize;

        for fact in &skills {
            let reference_secs = fact.last_accessed_at.unwrap_or(fact.valid_from).as_second();
            #[expect(
                clippy::cast_precision_loss,
                reason = "age in seconds → days; sub-second precision unnecessary"
            )]
            let days = ((now_secs - reference_secs).max(0) as f64) / 86_400.0;

            let score = crate::skill::skill_decay_score(days, fact.access_count, fact.confidence);

            if score < crate::skill::decay::RETIRE_THRESHOLD {
                self.forget_fact(&fact.id, crate::knowledge::ForgetReason::Stale)?;
                retired += 1;
            } else if score < crate::skill::decay::NEEDS_REVIEW_THRESHOLD {
                needs_review += 1;
                active += 1;
            } else {
                active += 1;
            }
        }

        Ok((active, needs_review, retired))
    }

    /// Gather skill health metrics for a nous.
    #[instrument(skip(self))]
    pub fn skill_quality_metrics(
        &self,
        nous_id: &str,
    ) -> crate::error::Result<crate::skill::SkillHealthMetrics> {
        let active_skills = self.find_skills_for_nous(nous_id, 10_000)?;
        let now_secs = jiff::Timestamp::now().as_second();

        let total_active = active_skills.len();

        // Count retired skills (forgotten with reason "stale")
        let total_retired = self.count_retired_skills(nous_id)?;

        // Compute usage stats and needs_review count
        let mut usage_counts: Vec<u32> = Vec::with_capacity(total_active);
        let mut days_since_use: Vec<f64> = Vec::with_capacity(total_active);
        let mut needs_review = 0usize;
        let mut named_usage: Vec<(String, u32)> = Vec::with_capacity(total_active);

        for fact in &active_skills {
            usage_counts.push(fact.access_count);

            let ref_secs = fact.last_accessed_at.unwrap_or(fact.valid_from).as_second();
            #[expect(
                clippy::cast_precision_loss,
                reason = "days since use for display; precision loss is acceptable"
            )]
            let days = ((now_secs - ref_secs).max(0) as f64) / 86_400.0;
            days_since_use.push(days);

            let score = crate::skill::skill_decay_score(days, fact.access_count, fact.confidence);
            if score < crate::skill::decay::NEEDS_REVIEW_THRESHOLD {
                needs_review += 1;
            }

            let name = match serde_json::from_str::<crate::skill::SkillContent>(&fact.content) {
                Ok(s) => s.name,
                Err(_) => fact.id.to_string(),
            };
            named_usage.push((name, fact.access_count));
        }

        let avg_usage_count = if total_active > 0 {
            #[expect(
                clippy::cast_precision_loss,
                reason = "total_active is number of skills; far below f64 precision limit"
            )]
            {
                usage_counts.iter().map(|&c| f64::from(c)).sum::<f64>() / total_active as f64
            }
        } else {
            0.0
        };

        let median_days_since_use = if days_since_use.is_empty() {
            0.0
        } else {
            days_since_use.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
            days_since_use[days_since_use.len() / 2]
        };

        // Top 10 and bottom 10
        named_usage.sort_by(|a, b| b.1.cmp(&a.1));
        let top_skills: Vec<(String, u32)> = named_usage.iter().take(10).cloned().collect();
        let bottom_skills: Vec<(String, u32)> =
            named_usage.iter().rev().take(10).cloned().collect();

        Ok(crate::skill::SkillHealthMetrics {
            total_active,
            total_retired,
            total_needs_review: needs_review,
            avg_usage_count,
            median_days_since_use,
            top_skills,
            bottom_skills,
            dedup_discard_count: 0,
            dedup_total_count: 0,
        })
    }

    /// Count skills that were retired (forgotten with reason "stale").
    fn count_retired_skills(&self, nous_id: &str) -> crate::error::Result<usize> {
        use crate::engine::DataValue;
        use std::collections::BTreeMap;

        let mut params = BTreeMap::new();
        params.insert("nous_id".to_owned(), DataValue::Str(nous_id.into()));

        let script = r"?[count(id)] :=
            *facts{id, nous_id, fact_type, is_forgotten, forget_reason},
            nous_id = $nous_id,
            fact_type = 'skill',
            is_forgotten == true,
            forget_reason = 'stale'";

        let rows = self.run_read(script, params)?;
        if let Some(row) = rows.rows.first() {
            if let Some(crate::engine::DataValue::Num(n)) = row.first() {
                return Ok(usize::try_from(n.get_int().unwrap_or(0)).unwrap_or(0));
            }
        }
        Ok(0)
    }

    /// Check if a skill similar to the given content already exists.
    ///
    /// Compares by name similarity (exact match) and by content similarity
    /// using BM25 search. Returns the fact ID of the most similar existing
    /// skill if similarity is high enough to be considered a duplicate.
    #[instrument(skip(self, skill_content))]
    pub fn find_duplicate_skill(
        &self,
        nous_id: &str,
        skill_content: &crate::skill::SkillContent,
    ) -> crate::error::Result<Option<crate::id::FactId>> {
        // First check exact name match
        if let Some(existing_id) = self.find_skill_by_name(nous_id, &skill_content.name)? {
            return Ok(Some(crate::id::FactId::from(existing_id.as_str())));
        }

        // Then do a BM25 search using the skill description as query
        let query = format!("{} {}", skill_content.name, skill_content.description);
        let candidates = self.search_skills(nous_id, &query, 5)?;

        for fact in candidates {
            if let Ok(existing) = serde_json::from_str::<crate::skill::SkillContent>(&fact.content)
            {
                // Check tool overlap as a proxy for content similarity
                let tool_overlap =
                    compute_tool_overlap(&skill_content.tools_used, &existing.tools_used);
                let name_sim = compute_name_similarity(&skill_content.name, &existing.name);

                // High tool overlap + similar name = likely duplicate
                if tool_overlap > 0.85 || (tool_overlap > 0.6 && name_sim > 0.5) {
                    return Ok(Some(fact.id));
                }
            }
        }

        Ok(None)
    }
}
