use super::KnowledgeStore;

#[cfg(feature = "mneme-engine")]
impl KnowledgeStore {
    pub(super) fn load_graph_context_for_recall(&self) -> crate::graph_intelligence::GraphContext {
        match self.load_graph_context() {
            Ok(ctx) => ctx,
            Err(error) => {
                tracing::warn!(%error, "failed to load graph context; continuing without graph enrichment");
                crate::graph_intelligence::GraphContext::default()
            }
        }
    }

    /// Enrich recall results with graph importance scores from cached `graph_scores`.
    ///
    /// For each fact result, looks up associated entities in `fact_entities`, then
    /// takes the maximum `PageRank` among those entities. Non-fact results are left
    /// unchanged (`graph_importance` stays 0.0).
    ///
    /// WHY (#5663): accepts a pre-loaded `GraphContext` so callers can load it once
    /// and share it across `enrich_recall_results` + `expand_recall_by_cluster`,
    /// eliminating redundant full-table scans of `graph_scores` per search call.
    pub(super) fn enrich_recall_results(
        &self,
        results: &mut [crate::knowledge::RecallResult],
        graph_ctx: &crate::graph_intelligence::GraphContext,
    ) {
        let fact_results: Vec<&crate::knowledge::RecallResult> =
            results.iter().filter(|r| r.source_type == "fact").collect();
        if fact_results.is_empty() {
            return;
        }

        let pageranks = &graph_ctx.pageranks;
        if pageranks.is_empty() {
            return;
        }

        for result in results.iter_mut().filter(|r| r.source_type == "fact") {
            let script = "?[entity_id] := *fact_entities{fact_id: $fid, entity_id}";
            let mut params = std::collections::BTreeMap::new();
            params.insert(
                "fid".to_owned(),
                crate::engine::DataValue::Str(result.source_id.as_str().into()),
            );
            let Ok(entity_rows) = self.run_read(script, params) else {
                continue;
            };
            let max_pr = entity_rows
                .rows
                .iter()
                .filter_map(|row| row.first().and_then(|v| v.get_str()))
                .filter_map(|entity_id| pageranks.get(entity_id).copied())
                .fold(0.0_f64, f64::max);
            result.graph_importance = max_pr;
        }
    }

    /// Populate `source_count` on fact results from the `fact_multiplicity`
    /// side-index so the recall pipeline can score the convergence factor (#4415).
    ///
    /// Non-consolidated / legacy facts have no multiplicity record and keep
    /// `source_count` 0 (convergence scores 0 for them). NOTE: this issues one
    /// indexed point-query per fact result; the convergence recall weight
    /// defaults to 0 so the score — and thus ranking — is unchanged when the
    /// feature is off. A batched side-index load is a future optimisation.
    pub(super) fn enrich_source_counts(&self, results: &mut [crate::knowledge::RecallResult]) {
        for result in results.iter_mut().filter(|r| r.source_type == "fact") {
            let Ok(fact_id) = crate::id::FactId::new(&result.source_id) else {
                continue;
            };
            if let Ok(Some(multiplicity)) = self.get_fact_multiplicity(&fact_id) {
                result.source_count = multiplicity.source_count;
            }
        }
    }

    /// Hydrate recall results with `scope`, `project_id`, `visibility`, and `sensitivity`.
    ///
    /// Semantic search returns from the `embeddings` relation, which does not
    /// carry these fields. This enrichment looks them up from `facts` for
    /// `source_type == "fact"` results so downstream quota and visibility
    /// filters see accurate values.
    pub(super) fn hydrate_recall_scope_visibility(
        &self,
        results: &mut [crate::knowledge::RecallResult],
    ) {
        for result in results.iter_mut().filter(|r| r.source_type == "fact") {
            let script = r"
                ?[scope, project_id, visibility, sensitivity] :=
                    *facts{id: $fid, scope, project_id, visibility, sensitivity}
            ";
            let mut params = std::collections::BTreeMap::new();
            params.insert(
                "fid".to_owned(),
                crate::engine::DataValue::Str(result.source_id.as_str().into()),
            );
            let Ok(rows) = self.run_read(script, params) else {
                continue;
            };
            if let Some(row) = rows.rows.first() {
                if let Some(scope_str) = row.first().and_then(|v| v.get_str())
                    && !scope_str.is_empty()
                {
                    match scope_str.parse::<crate::knowledge::MemoryScope>() {
                        Ok(scope) => result.scope = Some(scope),
                        Err(error) => tracing::warn!(
                            %error,
                            fact_id = %result.source_id,
                            scope = scope_str,
                            "failed to parse recall result memory scope"
                        ),
                    }
                }
                if let Some(project_id) = row
                    .get(1)
                    .and_then(|v| v.get_str())
                    .and_then(|s| eidos::workspace::ProjectId::from_sha256_hex(s).ok())
                {
                    result.project_id = Some(project_id);
                }
                if let Some(vis_str) = row.get(2).and_then(|v| v.get_str())
                    && !vis_str.is_empty()
                {
                    // kanon:ignore RUST/no-result-unwrap-or-default — Visibility::default() IS the documented
                    // fallback for unknown/legacy values from storage; clippy::manual_unwrap_or rejects an
                    // explicit Ok/Err match here.
                    result.visibility = vis_str
                        .parse::<crate::knowledge::Visibility>()
                        .unwrap_or_default();
                }
                if let Some(sensitivity_str) = row.get(3).and_then(|v| v.get_str())
                    && !sensitivity_str.is_empty()
                {
                    if let Ok(s) = sensitivity_str.parse::<crate::knowledge::FactSensitivity>() {
                        result.sensitivity = s;
                    } else {
                        tracing::warn!(
                            sensitivity = sensitivity_str,
                            fact_id = %result.source_id,
                            "hydrated fact has undecodable sensitivity; leaving as-is to avoid widening to Public"
                        );
                    }
                }
            }
        }
    }

    /// Expand recall results with cluster-mate facts.
    ///
    /// Takes the top results, finds their Louvain clusters, and queries for
    /// additional active facts linked to entities in those clusters. Adds
    /// new results with a neutral distance of 1.0, deduplicating by `source_id`.
    /// Limits expansion to at most `k` additional results.
    #[expect(
        clippy::too_many_lines,
        reason = "cluster expansion keeps query, hydration, and append logic together"
    )]
    pub(super) fn expand_recall_by_cluster(
        &self,
        results: &mut Vec<crate::knowledge::RecallResult>,
        k: i64,
        requester_nous_id: Option<&str>,
        preloaded_ctx: Option<&crate::graph_intelligence::GraphContext>,
    ) -> crate::error::Result<()> {
        if results.is_empty() {
            return Ok(());
        }

        let owned_ctx: crate::graph_intelligence::GraphContext;
        let ctx = if let Some(c) = preloaded_ctx {
            c
        } else {
            owned_ctx = self.load_graph_context()?;
            &owned_ctx
        };
        if ctx.clusters.is_empty() {
            return Ok(());
        }

        // Collect clusters from top results.
        let top_n = results.len().min(5);
        let mut context_clusters = std::collections::HashSet::new();
        for result in results.iter().take(top_n) {
            if result.source_type != "fact" {
                continue;
            }
            let script = "?[entity_id] := *fact_entities{fact_id: $fid, entity_id}";
            let mut params = std::collections::BTreeMap::new();
            params.insert(
                "fid".to_owned(),
                crate::engine::DataValue::Str(result.source_id.as_str().into()),
            );
            let Ok(entity_rows) = self.run_read(script, params) else {
                continue;
            };
            for row in &entity_rows.rows {
                if let Some(cid) = row
                    .first()
                    .and_then(|v| v.get_str())
                    .and_then(|entity_id| ctx.clusters.get(entity_id))
                {
                    context_clusters.insert(*cid);
                }
            }
        }

        if context_clusters.is_empty() {
            return Ok(());
        }

        let existing_ids: std::collections::HashSet<String> =
            results.iter().map(|r| r.source_id.clone()).collect();
        let mut added = 0;
        let limit = usize::try_from(k.max(1)).unwrap_or(1);

        for cluster_id in context_clusters {
            // WHY (#5559): filter by nous_id so cross-nous facts cannot be injected
            // via cluster expansion in a shared cohort store.
            let nous_filter = if requester_nous_id.is_some() {
                ", nous_id == $nous_id"
            } else {
                ""
            };
            let script = format!(
                r"
                ?[fact_id, content, nous_id, sensitivity, scope, project_id, visibility] :=
                    *graph_scores{{entity_id, score_type: 'cluster', cluster_id: $cid}},
                    *fact_entities{{fact_id: fid, entity_id}},
                    *facts{{id: fid, content, nous_id, is_forgotten, superseded_by,
                           sensitivity, scope, project_id, visibility}},
                    is_forgotten == false,
                    is_null(superseded_by){nous_filter},
                    fact_id = fid
                :limit $limit
            "
            );
            let mut params = std::collections::BTreeMap::new();
            params.insert("cid".to_owned(), crate::engine::DataValue::from(cluster_id));
            params.insert(
                "limit".to_owned(),
                crate::engine::DataValue::from(i64::try_from(limit).unwrap_or(i64::MAX)),
            );
            if let Some(nid) = requester_nous_id {
                params.insert(
                    "nous_id".to_owned(),
                    crate::engine::DataValue::Str(nid.into()),
                );
            }
            let Ok(rows) = self.run_read(&script, params) else {
                continue;
            };
            for row in &rows.rows {
                let Some(fact_id) = row.first().and_then(|v| v.get_str()) else {
                    continue;
                };
                if existing_ids.contains(fact_id) {
                    continue;
                }
                let content = row
                    .get(1)
                    .and_then(|v| v.get_str())
                    .unwrap_or("")
                    .to_owned();
                let nous_id = row
                    .get(2)
                    .and_then(|v| v.get_str())
                    .unwrap_or("")
                    .to_owned();
                let sensitivity_str = row
                    .get(3)
                    .and_then(|v| v.get_str())
                    .filter(|s| !s.is_empty());
                let sensitivity = if let Some(s) = sensitivity_str {
                    if let Ok(v) = s.parse::<crate::knowledge::FactSensitivity>() {
                        v
                    } else {
                        tracing::warn!(
                            sensitivity = s,
                            fact_id = ?row.first().and_then(|v| v.get_str()),
                            "cluster-expanded fact has undecodable sensitivity; skipping to avoid widening to Public"
                        );
                        continue;
                    }
                } else {
                    crate::knowledge::FactSensitivity::default()
                };
                let scope = row
                    .get(4)
                    .and_then(|v| v.get_str())
                    .filter(|s| !s.is_empty())
                    .and_then(|s| s.parse::<crate::knowledge::MemoryScope>().ok());
                let project_id = row
                    .get(5)
                    .and_then(|v| v.get_str())
                    .and_then(|s| eidos::workspace::ProjectId::from_sha256_hex(s).ok());
                let visibility = row
                    .get(6)
                    .and_then(|v| v.get_str())
                    .and_then(|s| s.parse::<crate::knowledge::Visibility>().ok())
                    .unwrap_or_default();
                results.push(crate::knowledge::RecallResult {
                    content,
                    distance: 1.0,
                    source_type: "fact".to_owned(),
                    source_id: fact_id.to_owned(),
                    nous_id,
                    sensitivity,
                    graph_importance: 0.0,
                    scope,
                    project_id,
                    visibility,
                    source_count: 0,
                });
                added += 1;
                if added >= limit {
                    return Ok(());
                }
            }
        }

        Ok(())
    }

    #[expect(
        clippy::too_many_lines,
        reason = "scoped cluster expansion mirrors the unscoped query, hydration, and append flow"
    )]
    pub(super) fn expand_recall_by_cluster_scoped(
        &self,
        results: &mut Vec<crate::knowledge::RecallResult>,
        k: i64,
        requester_nous_id: &str,
        preloaded_ctx: Option<&crate::graph_intelligence::GraphContext>,
    ) -> crate::error::Result<()> {
        if results.is_empty() {
            return Ok(());
        }

        let owned_ctx: crate::graph_intelligence::GraphContext;
        let ctx = if let Some(c) = preloaded_ctx {
            c
        } else {
            owned_ctx = self.load_graph_context()?;
            &owned_ctx
        };
        if ctx.clusters.is_empty() {
            return Ok(());
        }

        let top_n = results.len().min(5);
        let mut context_clusters = std::collections::HashSet::new();
        for result in results.iter().take(top_n) {
            if result.source_type != "fact" {
                continue;
            }
            let script = "?[entity_id] := *fact_entities{fact_id: $fid, entity_id}";
            let mut params = std::collections::BTreeMap::new();
            params.insert(
                "fid".to_owned(),
                crate::engine::DataValue::Str(result.source_id.as_str().into()),
            );
            let Ok(entity_rows) = self.run_read(script, params) else {
                continue;
            };
            for row in &entity_rows.rows {
                if let Some(cid) = row
                    .first()
                    .and_then(|v| v.get_str())
                    .and_then(|entity_id| ctx.clusters.get(entity_id))
                {
                    context_clusters.insert(*cid);
                }
            }
        }

        if context_clusters.is_empty() {
            return Ok(());
        }

        let existing_ids: std::collections::HashSet<String> =
            results.iter().map(|r| r.source_id.clone()).collect();
        let mut added = 0;
        let limit = usize::try_from(k.max(1)).unwrap_or(1);

        for cluster_id in context_clusters {
            let script = r"
                ?[fact_id, content, nous_id, sensitivity, scope, project_id, visibility] :=
                    *graph_scores{entity_id, score_type: 'cluster', cluster_id: $cid},
                    *fact_entities{fact_id: fid, entity_id},
                    *facts{id: fid, content, nous_id, is_forgotten, superseded_by,
                           sensitivity, scope, project_id, visibility},
                    nous_id == $requester_nous_id,
                    is_forgotten == false,
                    is_null(superseded_by),
                    fact_id = fid
                ?[fact_id, content, nous_id, sensitivity, scope, project_id, visibility] :=
                    *graph_scores{entity_id, score_type: 'cluster', cluster_id: $cid},
                    *fact_entities{fact_id: fid, entity_id},
                    *facts{id: fid, content, nous_id, is_forgotten, superseded_by,
                           sensitivity, scope, project_id, visibility},
                    visibility == 'shared',
                    is_forgotten == false,
                    is_null(superseded_by),
                    fact_id = fid
                ?[fact_id, content, nous_id, sensitivity, scope, project_id, visibility] :=
                    *graph_scores{entity_id, score_type: 'cluster', cluster_id: $cid},
                    *fact_entities{fact_id: fid, entity_id},
                    *facts{id: fid, content, nous_id, is_forgotten, superseded_by,
                           sensitivity, scope, project_id, visibility},
                    visibility == 'published',
                    is_forgotten == false,
                    is_null(superseded_by),
                    fact_id = fid
                :limit $limit
                ";
            let mut params = std::collections::BTreeMap::new();
            params.insert("cid".to_owned(), crate::engine::DataValue::from(cluster_id));
            params.insert(
                "limit".to_owned(),
                crate::engine::DataValue::from(i64::try_from(limit).unwrap_or(i64::MAX)),
            );
            params.insert(
                "requester_nous_id".to_owned(),
                crate::engine::DataValue::Str(requester_nous_id.into()),
            );
            let Ok(rows) = self.run_read(&script, params) else {
                continue;
            };
            for row in &rows.rows {
                let Some(fact_id) = row.first().and_then(|v| v.get_str()) else {
                    continue;
                };
                if existing_ids.contains(fact_id) {
                    continue;
                }
                let content = row
                    .get(1)
                    .and_then(|v| v.get_str())
                    .unwrap_or("")
                    .to_owned();
                let nous_id = row
                    .get(2)
                    .and_then(|v| v.get_str())
                    .unwrap_or("")
                    .to_owned();
                let sensitivity_str = row
                    .get(3)
                    .and_then(|v| v.get_str())
                    .filter(|s| !s.is_empty());
                let sensitivity = if let Some(s) = sensitivity_str {
                    if let Ok(v) = s.parse::<crate::knowledge::FactSensitivity>() {
                        v
                    } else {
                        tracing::warn!(
                            sensitivity = s,
                            fact_id = ?row.first().and_then(|v| v.get_str()),
                            "cluster-expanded fact has undecodable sensitivity; skipping to avoid widening to Public"
                        );
                        continue;
                    }
                } else {
                    crate::knowledge::FactSensitivity::default()
                };
                let scope = row
                    .get(4)
                    .and_then(|v| v.get_str())
                    .filter(|s| !s.is_empty())
                    .and_then(|s| s.parse::<crate::knowledge::MemoryScope>().ok());
                let project_id = row
                    .get(5)
                    .and_then(|v| v.get_str())
                    .and_then(|s| eidos::workspace::ProjectId::from_sha256_hex(s).ok());
                let visibility = row
                    .get(6)
                    .and_then(|v| v.get_str())
                    .and_then(|s| s.parse::<crate::knowledge::Visibility>().ok())
                    .unwrap_or_default();
                results.push(crate::knowledge::RecallResult {
                    content,
                    distance: 1.0,
                    source_type: "fact".to_owned(),
                    source_id: fact_id.to_owned(),
                    nous_id,
                    sensitivity,
                    graph_importance: 0.0,
                    scope,
                    project_id,
                    visibility,
                    source_count: 0,
                });
                added += 1;
                if added >= limit {
                    return Ok(());
                }
            }
        }

        Ok(())
    }

    pub(super) fn increment_recall_access(&self, results: &[crate::knowledge::RecallResult]) {
        let source_ids: Vec<crate::id::FactId> = results
            .iter()
            .filter(|r| r.source_type == "fact")
            .filter_map(|r| crate::id::FactId::new(&r.source_id).ok())
            .collect();
        if let Err(e) = self.increment_access(&source_ids) {
            tracing::warn!(error = %e, "failed to increment access counts");
        }
    }
}
