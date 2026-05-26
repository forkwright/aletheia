//! Recall pipeline stage: retrieves relevant knowledge and injects into context.

mod reranking;
mod scoring;
mod search;

use std::collections::{HashMap, HashSet};

use mneme::id::FactId;
use mneme::knowledge::MemoryScope;
use tracing::{debug, info, instrument};

use hermeneus::provider::DeploymentTarget;
use mneme::embedding::EmbeddingProvider;
use mneme::knowledge::{FactSensitivity, RecallResult as KnowledgeRecallResult};
use mneme::recall::{FactorScores, RecallEngine, ScoredResult};

use crate::error;

pub use scoring::{RecallConfig, RecallWeights};
pub(crate) use scoring::{estimate_tokens, format_section};
#[cfg(feature = "knowledge-store")]
pub(crate) use search::KnowledgeTextSearch;
#[cfg(feature = "knowledge-store")]
pub use search::KnowledgeVectorSearch;
pub(crate) use search::TextSearch;
pub use search::VectorSearch;

#[cfg(test)]
use reranking::is_stopword;
use reranking::{detect_gaps, discover_terminology};
#[cfg(feature = "knowledge-store")]
use search::vector_search_tiered;
use search::{embed, vector_search};

/// Output of the recall pipeline stage.
#[derive(Debug, Clone)]
pub struct RecallStageResult {
    /// Number of candidates retrieved from knowledge store.
    pub candidates_found: usize,
    /// Number that passed scoring threshold.
    pub results_injected: usize,
    /// Tokens consumed by injected knowledge.
    pub tokens_consumed: u64,
    /// The formatted recall section (appended to system prompt).
    pub recall_section: Option<String>,
    /// Source IDs of facts whose content was injected into the recall
    /// section. Used by the prompt audit log (#3411) so operators can see
    /// which stored facts were included in each outbound request.
    pub fact_ids: Vec<String>,
    /// Provider boundary used for the sovereignty filter.
    pub deployment_target: DeploymentTarget,
    /// Facts dropped because their sensitivity exceeded `deployment_target`.
    pub filtered_facts: Vec<RecallFilteredFact>,
}

/// A fact filtered out before provider dispatch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecallFilteredFact {
    /// Source fact ID.
    pub id: String,
    /// Sensitivity that exceeded the active deployment target.
    pub sensitivity: FactSensitivity,
}

impl RecallStageResult {
    fn empty() -> Self {
        Self {
            candidates_found: 0,
            results_injected: 0,
            tokens_consumed: 0,
            recall_section: None,
            fact_ids: Vec::new(),
            deployment_target: DeploymentTarget::Cloud,
            filtered_facts: Vec::new(),
        }
    }
}

struct SensitivityFilterResult {
    kept: Vec<ScoredResult>,
    filtered: Vec<RecallFilteredFact>,
}

/// Recall stage: scores and formats knowledge for injection into the system prompt.
///
/// # Examples
///
/// ```no_run
/// use nous::recall::{RecallConfig, RecallStage};
///
/// let stage = RecallStage::new(RecallConfig::default());
/// ```
pub struct RecallStage {
    engine: RecallEngine,
    config: RecallConfig,
    /// Optional side-query selected IDs for pre-filtering before 6-factor scoring.
    side_query_ids: Option<HashSet<String>>,
    /// Production side-query selector used to turn the raw recall manifest into
    /// a prefilter for 6-factor scoring.
    side_query_selector: mneme::side_query::SideQuerySelector,
    /// Data-sovereignty target: gates which facts may leave the instance
    /// through this recall pass (#3404, #3413). Defaults to
    /// [`DeploymentTarget::Cloud`] — the safe assumption so callers who do
    /// not thread `with_deployment_target` never leak `Internal` or
    /// `Confidential` facts.
    deployment_target: DeploymentTarget,
    /// Pinned fact IDs (fast lookup set derived from config).
    pinned_facts: HashSet<String>,
    /// When true, recalled knowledge is appended as a system message at the
    /// end of the conversation context rather than injected into the system
    /// prompt.
    late_inject_anchor: bool,
    /// Per-scope minimum result counts with slack-fill.
    scope_quotas: HashMap<MemoryScope, usize>,
    /// Project partition filter applied before scoring thresholds and budgeting.
    project_scope: mneme::recall::ProjectRecallScope,
    /// Optional URL for an HTTP cross-encoder reranker.
    reranker_url: Option<String>,
}

impl RecallStage {
    /// Create a recall stage with mneme's default engine weights.
    ///
    /// Engine weights are defined once in `mneme::recall::RecallWeights` (the
    /// single source of truth) and used directly here. If operator
    /// customization is needed in the future, add it back with a single
    /// source of truth rather than mirroring the struct in taxis.
    #[must_use]
    pub fn new(config: RecallConfig) -> Self {
        let engine = Self::engine_with_reranker_url(config.reranker_url.as_deref());

        let pinned_facts: HashSet<String> = config
            .pinned_facts
            .iter()
            .map(|f| f.as_str().to_owned())
            .collect();
        let late_inject_anchor = config.late_inject_anchor;
        let scope_quotas = config.scope_quotas.clone();
        let reranker_url = config.reranker_url.clone();

        Self {
            engine,
            config,
            side_query_ids: None,
            side_query_selector: mneme::side_query::SideQuerySelector::new(
                mneme::side_query::SideQueryConfig::default(),
            ),
            deployment_target: DeploymentTarget::Cloud,
            pinned_facts,
            late_inject_anchor,
            scope_quotas,
            project_scope: mneme::recall::ProjectRecallScope::Global,
            reranker_url,
        }
    }

    /// Set side-query selected IDs for pre-filtering before 6-factor scoring.
    ///
    /// // WHY: Side-queries (e.g., from tool results or explicit references) can
    /// // identify relevant knowledge IDs directly, bypassing vector search.
    /// // Pre-filtering avoids expensive 6-factor scoring on irrelevant candidates.
    #[must_use]
    pub fn with_side_query_ids(mut self, ids: HashSet<String>) -> Self {
        self.side_query_ids = Some(ids);
        self
    }

    /// Set the deployment target used to filter recalled facts by sensitivity.
    ///
    /// Facts whose [`FactSensitivity`] exceeds the target are dropped in
    /// [`finalize_results`](Self::finalize_results) before the recall section
    /// is built, so they never reach the LLM provider (#3404, #3413).
    #[must_use]
    pub fn with_deployment_target(mut self, target: DeploymentTarget) -> Self {
        self.deployment_target = target;
        self
    }

    /// Set pinned fact IDs for priority recall.
    ///
    /// Pinned facts are slotted before non-pinned results when they appear in
    /// the ranked candidate list. They bypass the `max_results` budget but are
    /// still subject to the token budget.
    #[must_use]
    pub fn with_pinned_facts(mut self, facts: &[FactId]) -> Self {
        self.pinned_facts = facts.iter().map(|f| f.as_str().to_owned()).collect();
        self
    }

    /// Set whether recalled knowledge is injected as a late system message.
    #[must_use]
    pub fn with_late_inject_anchor(mut self, enabled: bool) -> Self {
        self.late_inject_anchor = enabled;
        self
    }

    /// Set per-scope minimum quotas with slack-fill.
    #[must_use]
    pub fn with_scope_quotas(mut self, quotas: HashMap<MemoryScope, usize>) -> Self {
        self.scope_quotas = quotas;
        self
    }

    /// Set project recall scope.
    #[must_use]
    pub fn with_project_scope(mut self, scope: mneme::recall::ProjectRecallScope) -> Self {
        self.project_scope = scope;
        self
    }

    /// Set the URL for an HTTP cross-encoder reranker.
    #[must_use]
    pub fn with_reranker_url(mut self, url: Option<String>) -> Self {
        self.engine = Self::engine_with_reranker_url(url.as_deref());
        self.reranker_url = url;
        self
    }

    #[cfg(feature = "reranker")]
    fn engine_with_reranker_url(url: Option<&str>) -> RecallEngine {
        let engine = RecallEngine::with_weights(mneme::recall::RecallWeights::default());
        // WHY: Wire reranker only when the episteme reranker feature is present.
        // A configured URL uses the HTTP cross-encoder; otherwise NaiveReranker
        // preserves an in-process fallback for feature-enabled deployments.
        let reranker: Option<std::sync::Arc<dyn mneme::recall::reranker::Reranker>> =
            if let Some(url) = url {
                Some(std::sync::Arc::new(
                    mneme::recall::reranker::HttpReranker::new(url.to_owned()),
                ))
            } else {
                Some(std::sync::Arc::new(mneme::recall::reranker::NaiveReranker))
            };
        engine.with_reranker(reranker)
    }

    #[cfg(not(feature = "reranker"))]
    fn engine_with_reranker_url(_url: Option<&str>) -> RecallEngine {
        RecallEngine::with_weights(mneme::recall::RecallWeights::default())
    }

    /// Rank candidates, applying side-query pre-filter when configured.
    fn rank_candidates(&self, candidates: Vec<ScoredResult>) -> Vec<ScoredResult> {
        self.rank_candidates_with_side_ids(candidates, self.side_query_ids.as_ref())
    }

    fn rank_candidates_with_side_ids(
        &self,
        candidates: Vec<ScoredResult>,
        side_ids: Option<&HashSet<String>>,
    ) -> Vec<ScoredResult> {
        match side_ids {
            Some(ids) if !ids.is_empty() => self.engine.rank_with_prefilter(candidates, ids),
            None | Some(_) => self.engine.rank(candidates),
        }
    }

    /// Run recall using BM25 text search only (no vector embeddings required).
    ///
    /// Used as a fallback when the embedding provider is in mock mode.
    /// Scores, ranks, and formats results the same way as [`run`](Self::run).
    ///
    /// # Why
    ///
    /// BM25 is a pure-text fallback when embeddings are unavailable
    /// (mock mode, network issues, or local-only deployments). It provides
    /// reasonable relevance without external service dependencies.
    ///
    /// # Complexity
    ///
    /// O(n log n) where n is the number of candidates retrieved from the search.
    pub(crate) fn run_bm25(
        &self,
        query: &str,
        nous_id: &str,
        text_search: &dyn TextSearch,
        remaining_budget: u64,
    ) -> error::Result<RecallStageResult> {
        if !self.config.enabled {
            debug!("recall disabled");
            return Ok(RecallStageResult::empty());
        }

        let k = self.config.max_results * 3;
        let raw = text_search.search_text(query, k)?;

        if raw.is_empty() {
            debug!("no BM25 recall candidates found");
            return Ok(RecallStageResult::empty());
        }

        let candidates = self.build_candidates(raw, nous_id);
        let ranked = self.rank_candidates(candidates);
        Ok(self.finalize_results(ranked, remaining_budget, nous_id))
    }

    /// Run the recall stage.
    ///
    /// Embeds the query, searches for nearest vectors, scores and ranks results,
    /// then formats the top results as a markdown section for the system prompt.
    /// When `iterative` is enabled, runs a second cycle with terminology-refined queries.
    ///
    /// // WHY: Iterative recall discovers domain terminology from initial results
    /// // and re-queries with expanded terms, improving recall for technical queries
    /// // where the user's vocabulary may not match the knowledge base.
    ///
    /// Non-fatal errors are returned as `Err`: the caller should catch and continue.
    ///
    /// # Errors
    ///
    /// - Returns `RecallEmbedding` if embedding the query fails.
    /// - Returns `RecallSearch` if the vector search fails.
    ///
    /// # Complexity
    ///
    /// O(n log n) where n is the number of candidates retrieved from the search.
    /// In iterative mode, complexity doubles as two searches are performed.
    #[instrument(skip_all, fields(nous_id = %nous_id))]
    pub fn run(
        &self,
        query: &str,
        nous_id: &str,
        embedding_provider: &dyn EmbeddingProvider,
        vector_search: &dyn VectorSearch,
        remaining_budget: u64,
    ) -> error::Result<RecallStageResult> {
        self.run_with_recall_enhancements(
            query,
            nous_id,
            embedding_provider,
            vector_search,
            remaining_budget,
            None,
            None,
        )
    }

    /// Run recall with production side-query and query-rewrite/tiered-search hooks.
    #[expect(
        clippy::too_many_arguments,
        reason = "production recall needs independent provider hooks for side-query and rewrite"
    )]
    pub fn run_with_recall_enhancements(
        &self,
        query: &str,
        nous_id: &str,
        embedding_provider: &dyn EmbeddingProvider,
        vector_search: &dyn VectorSearch,
        remaining_budget: u64,
        side_ranker: Option<&dyn mneme::side_query::SideQueryRanker>,
        rewrite_provider: Option<&dyn mneme::query_rewrite::RewriteProvider>,
    ) -> error::Result<RecallStageResult> {
        if !self.config.enabled {
            debug!("recall disabled");
            return Ok(RecallStageResult::empty());
        }

        if self.config.iterative && self.config.max_cycles > 1 {
            self.run_iterative(
                query,
                nous_id,
                embedding_provider,
                vector_search,
                remaining_budget,
                side_ranker,
                rewrite_provider,
            )
        } else {
            self.run_single(
                query,
                nous_id,
                embedding_provider,
                vector_search,
                remaining_budget,
                side_ranker,
                rewrite_provider,
            )
        }
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "internal recall branch receives same enhancement hooks as public entry point"
    )]
    fn run_single(
        &self,
        query: &str,
        nous_id: &str,
        embedding_provider: &dyn EmbeddingProvider,
        vs: &dyn VectorSearch,
        remaining_budget: u64,
        side_ranker: Option<&dyn mneme::side_query::SideQueryRanker>,
        rewrite_provider: Option<&dyn mneme::query_rewrite::RewriteProvider>,
    ) -> error::Result<RecallStageResult> {
        let k = self.config.max_results * 3;
        #[cfg(not(feature = "knowledge-store"))]
        let _ = rewrite_provider;
        let query_vec = embed(query, embedding_provider)?;
        #[cfg(feature = "knowledge-store")]
        let raw = if let Some(provider) = rewrite_provider {
            vector_search_tiered(vs, query, query_vec, k, provider)?
        } else {
            vector_search(vs, query_vec, k)?
        };
        #[cfg(not(feature = "knowledge-store"))]
        let raw = vector_search(vs, query_vec, k)?;

        if raw.is_empty() {
            debug!("no recall candidates found");
            return Ok(RecallStageResult::empty());
        }

        let side_ids = side_ranker.and_then(|ranker| self.side_query_ids(query, &raw, ranker));
        let candidates = self.build_candidates(raw, nous_id);
        let ranked = self.rank_candidates_with_side_ids(
            candidates,
            self.side_query_ids.as_ref().or(side_ids.as_ref()),
        );
        Ok(self.finalize_results(ranked, remaining_budget, nous_id))
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "internal recall branch receives same enhancement hooks as public entry point"
    )]
    fn run_iterative(
        &self,
        query: &str,
        nous_id: &str,
        embedding_provider: &dyn EmbeddingProvider,
        vs: &dyn VectorSearch,
        remaining_budget: u64,
        side_ranker: Option<&dyn mneme::side_query::SideQueryRanker>,
        rewrite_provider: Option<&dyn mneme::query_rewrite::RewriteProvider>,
    ) -> error::Result<RecallStageResult> {
        let k = self.config.max_results * 3;
        #[cfg(not(feature = "knowledge-store"))]
        let _ = rewrite_provider;

        let query_vec = embed(query, embedding_provider)?;
        #[cfg(feature = "knowledge-store")]
        let raw_cycle1 = if let Some(provider) = rewrite_provider {
            vector_search_tiered(vs, query, query_vec, k, provider)?
        } else {
            vector_search(vs, query_vec, k)?
        };
        #[cfg(not(feature = "knowledge-store"))]
        let raw_cycle1 = vector_search(vs, query_vec, k)?;

        if raw_cycle1.is_empty() {
            debug!("no recall candidates in cycle 1");
            return Ok(RecallStageResult::empty());
        }

        let candidates_c1 = self.build_candidates(raw_cycle1.clone(), nous_id);
        let side_ids_c1 =
            side_ranker.and_then(|ranker| self.side_query_ids(query, &raw_cycle1, ranker));
        let ranked_c1 = self.rank_candidates_with_side_ids(candidates_c1, side_ids_c1.as_ref());

        let terms = discover_terminology(&ranked_c1, query);
        let gaps = detect_gaps(&ranked_c1);

        if terms.is_empty() && gaps.is_empty() {
            debug!("no novel terms or gaps discovered, skipping cycle 2");
            return Ok(self.finalize_results(ranked_c1, remaining_budget, nous_id));
        }

        let mut refined = String::from(query);
        for term in &terms {
            refined.push(' ');
            refined.push_str(term);
        }
        for gap in &gaps {
            refined.push(' ');
            refined.push_str(gap);
        }

        debug!(
            new_terms = terms.len(),
            gaps = gaps.len(),
            refined = refined.as_str(),
            "cycle 2 with refined query"
        );

        let refined_vec = embed(&refined, embedding_provider)?;
        let raw_cycle2 = vector_search(vs, refined_vec, k)?;

        let mut seen: HashSet<String> = HashSet::new();
        let mut merged: Vec<KnowledgeRecallResult> = Vec::new();
        for r in raw_cycle1 {
            if seen.insert(r.source_id.clone()) {
                merged.push(r);
            }
        }
        for r in raw_cycle2 {
            if seen.insert(r.source_id.clone()) {
                merged.push(r);
            }
        }

        debug!(
            unique_candidates = merged.len(),
            "merged results from 2 cycles"
        );

        let candidates = self.build_candidates(merged, nous_id);
        let ranked = self.rank_candidates(candidates);
        Ok(self.finalize_results(ranked, remaining_budget, nous_id))
    }

    fn side_query_ids(
        &self,
        query: &str,
        raw: &[KnowledgeRecallResult],
        ranker: &dyn mneme::side_query::SideQueryRanker,
    ) -> Option<HashSet<String>> {
        let headers = raw
            .iter()
            .enumerate()
            .map(|(idx, result)| {
                let description: String = result.content.chars().take(240).collect();
                mneme::manifest::MemoryHeader::new(
                    result.source_id.clone(),
                    result.source_type.clone(),
                    i64::try_from(raw.len().saturating_sub(idx)).unwrap_or(i64::MAX),
                )
                .with_description(description)
            })
            .collect();
        let manifest = mneme::manifest::MemoryManifest::from_headers(headers);
        match self.side_query_selector.select(query, &manifest, ranker) {
            Ok(result) if !result.selected_ids.is_empty() => {
                Some(result.selected_ids.into_iter().collect())
            }
            Ok(_) => None,
            Err(e) => {
                tracing::warn!(error = %e, "side-query recall ranker failed; continuing without prefilter");
                None
            }
        }
    }

    fn finalize_results(
        &self,
        ranked: Vec<ScoredResult>,
        remaining_budget: u64,
        _nous_id: &str,
    ) -> RecallStageResult {
        let candidates_found = ranked.len();

        // WHY (#3404, #3413): data-sovereignty filter. Drop facts whose
        // sensitivity exceeds the active provider's deployment target BEFORE
        // `min_score` filtering / budgeting, so the dropped facts never
        // contribute to the recall section sent to the LLM. Non-fact sources
        // default to `Public` and pass through unchanged.
        let sensitivity_filter = self.filter_by_sensitivity(ranked);
        let ranked = sensitivity_filter.kept;
        let filtered_facts = sensitivity_filter.filtered;

        // WHY (#208): visibility filter. Default to Private so callers who
        // do not explicitly configure visibility continue to see only their
        // own facts. Existing data backfilled to Private passes through
        // unchanged.
        let ranked =
            mneme::recall::filter_by_visibility(ranked, mneme::knowledge::Visibility::Private);
        let ranked = mneme::recall::filter_by_project_scope(ranked, &self.project_scope);
        // TODO(#208) [deliberate-prudent]: wire `filter_by_cohort_visibility` once
        // `build_candidates` populates `nous_id` from the search layer instead of
        // `String::new()`.

        // Extract pinned facts first (they bypass max_results but are still
        // subject to the token budget).
        let (pinned, rest): (Vec<ScoredResult>, Vec<ScoredResult>) = ranked
            .into_iter()
            .partition(|r| self.pinned_facts.contains(&r.source_id));

        // Deduplicate pinned facts while preserving their ranked order.
        let mut seen_pinned = HashSet::new();
        let pinned: Vec<ScoredResult> = pinned
            .into_iter()
            .filter(|r| seen_pinned.insert(r.source_id.clone()))
            .collect();

        // Apply scope quotas to non-pinned results.
        let rest = self.apply_scope_quotas(rest);

        let filtered = self.filter(rest);

        if pinned.is_empty() && filtered.is_empty() {
            debug!(candidates_found, "all candidates below min_score");
            return RecallStageResult {
                candidates_found,
                deployment_target: self.deployment_target,
                filtered_facts,
                ..RecallStageResult::empty()
            };
        }

        let budget = remaining_budget.min(self.config.max_recall_tokens);
        let combined: Vec<ScoredResult> = pinned.into_iter().chain(filtered).collect();
        let (results_injected, section, tokens, fact_ids) =
            self.format_within_budget(&combined, budget);
        self.side_query_selector.mark_surfaced(&fact_ids);

        debug!(
            candidates_found,
            results_injected,
            tokens_consumed = tokens,
            "recall complete"
        );

        RecallStageResult {
            candidates_found,
            results_injected,
            tokens_consumed: tokens,
            recall_section: if section.is_empty() {
                None
            } else {
                Some(section)
            },
            fact_ids,
            deployment_target: self.deployment_target,
            filtered_facts,
        }
    }

    /// Apply per-scope minimum quotas with slack-fill.
    ///
    /// Two-pass algorithm:
    ///
    /// 1. **Reserve**: For each scope with a quota, extract up to `quota` of
    ///    the highest-scored candidates belonging to that scope.
    /// 2. **Fill**: Append the remaining candidates sorted by overall score.
    ///
    /// If a scope has fewer candidates than its quota, the deficit becomes
    /// slack that the general pool absorbs automatically. The output is
    /// deterministic and does not mutate the input scores.
    ///
    /// # Complexity
    ///
    /// O(n log n) where n is the number of candidates (dominated by sorting).
    fn apply_scope_quotas(&self, results: Vec<ScoredResult>) -> Vec<ScoredResult> {
        if self.scope_quotas.is_empty() {
            return results;
        }

        // Group candidates by scope.
        let mut by_scope: std::collections::HashMap<Option<MemoryScope>, Vec<ScoredResult>> =
            std::collections::HashMap::new();
        for r in results {
            by_scope.entry(r.scope).or_default().push(r);
        }

        // Pass 1: reserve quota slots per scope.
        let mut reserved = Vec::new();
        let mut pool = Vec::new();
        for (scope, mut candidates) in by_scope {
            candidates.sort_by(|a, b| {
                b.score
                    .partial_cmp(&a.score)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
            if let Some(&quota) = scope.and_then(|s| self.scope_quotas.get(&s)) {
                let take = candidates.len().min(quota);
                reserved.extend(candidates.drain(..take));
                pool.extend(candidates);
            } else {
                pool.extend(candidates);
            }
        }

        // Sort reserved and pool by score descending.
        reserved.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        pool.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        // Pass 2: append the general pool.
        reserved.extend(pool);
        reserved
    }

    /// Drop candidates whose sensitivity exceeds the active deployment target.
    ///
    /// Emits an info-level log listing the filtered fact IDs so operators
    /// can audit which memories were withheld from the current provider.
    ///
    /// | Target | Accepts |
    /// |--------|---------|
    /// | `Cloud` | `Public` only |
    /// | `LocalHosted` | `Public`, `Internal` |
    /// | `Embedded` | all sensitivities |
    ///
    /// # Complexity
    ///
    /// O(n) where n is the number of ranked candidates.
    fn filter_by_sensitivity(&self, ranked: Vec<ScoredResult>) -> SensitivityFilterResult {
        let target = self.deployment_target;
        let max_allowed = max_sensitivity_for(target);
        let mut filtered: Vec<RecallFilteredFact> = Vec::new();
        let kept: Vec<ScoredResult> = ranked
            .into_iter()
            .filter(|r| {
                if r.sensitivity <= max_allowed {
                    true
                } else {
                    filtered.push(RecallFilteredFact {
                        id: r.source_id.clone(),
                        sensitivity: r.sensitivity,
                    });
                    false
                }
            })
            .collect();
        if !filtered.is_empty() {
            let filtered_ids: Vec<&str> = filtered.iter().map(|f| f.id.as_str()).collect();
            info!(
                filtered_count = filtered.len(),
                deployment_target = target.as_str(),
                max_sensitivity = max_allowed.as_str(),
                fact_ids = ?filtered_ids,
                "recall: dropped sensitive facts before provider dispatch (sovereignty filter)"
            );
        }
        SensitivityFilterResult { kept, filtered }
    }

    /// Build candidate scores from raw search results.
    ///
    /// # Complexity
    ///
    /// O(n) where n is the number of raw search results.
    fn build_candidates(
        &self,
        raw: Vec<KnowledgeRecallResult>,
        _nous_id: &str,
    ) -> Vec<ScoredResult> {
        let w = &self.config.weights;
        raw.into_iter()
            .map(|r| ScoredResult {
                content: r.content,
                source_type: r.source_type,
                source_id: r.source_id,
                nous_id: String::new(),
                factors: FactorScores {
                    vector_similarity: self.engine.score_vector_similarity(r.distance),
                    decay: w.decay,
                    relevance: w.relevance,
                    epistemic_tier: w.epistemic_tier,
                    relationship_proximity: w.relationship_proximity,
                    access_frequency: w.access_frequency,
                    graph_importance: self.engine.score_graph_importance(r.graph_importance),
                },
                score: 0.0,
                // WHY (#3404, #3413): propagate sensitivity from the search
                // layer so the sovereignty filter in `finalize_results` sees
                // per-fact classification rather than assuming `Public`.
                sensitivity: r.sensitivity,
                scope: r.scope,
                project_id: r.project_id,
                visibility: r.visibility,
            })
            .collect()
    }

    /// Filter candidates by minimum score and take top results.
    ///
    /// # Complexity
    ///
    /// O(n) where n is the number of ranked candidates.
    fn filter(&self, ranked: Vec<ScoredResult>) -> Vec<ScoredResult> {
        ranked
            .into_iter()
            .filter(|r| r.score >= self.config.min_score)
            .take(self.config.max_results)
            .collect()
    }

    /// Format results within the token budget.
    ///
    /// Returns the included count, rendered section, total tokens consumed,
    /// and the source IDs of the included facts (used by the prompt audit
    /// log in #3411 to identify which facts were surfaced).
    ///
    /// # Complexity
    ///
    /// O(n²) where n is the number of results, due to repeated token estimation
    /// for each incremental addition to the output.
    fn format_within_budget(
        &self,
        results: &[ScoredResult],
        budget: u64,
    ) -> (usize, String, u64, Vec<String>) {
        let cpt = self.config.chars_per_token;
        let mut included = Vec::with_capacity(results.len());

        for result in results {
            included.push(result);
            let section = format_section(&included, self.config.inject_metadata);
            let tokens = estimate_tokens(&section, cpt);
            if tokens > budget {
                included.pop();
                break;
            }
        }

        if included.is_empty() {
            return (0, String::new(), 0, Vec::new());
        }

        let section = format_section(&included, self.config.inject_metadata);
        let tokens = estimate_tokens(&section, cpt);
        let fact_ids = included.iter().map(|r| r.source_id.clone()).collect();
        (included.len(), section, tokens, fact_ids)
    }
}

/// Maximum [`FactSensitivity`] this deployment target is permitted to receive.
///
/// - `Cloud` → `Public`
/// - `LocalHosted` → `Internal`
/// - `Embedded` → `Confidential`
///
/// The ordering on `FactSensitivity` mirrors the ordering on
/// `DeploymentTarget`, so admission reduces to `sensitivity <= max`.
fn max_sensitivity_for(target: DeploymentTarget) -> FactSensitivity {
    // WHY: `DeploymentTarget` is `#[non_exhaustive]` — any future boundary
    // this crate has not been taught about falls into the wildcard arm and
    // is treated as `Public`, the safest classification. Operators cannot
    // leak classified facts to an unknown target by accident.
    match target {
        DeploymentTarget::LocalHosted => FactSensitivity::Internal,
        DeploymentTarget::Embedded => FactSensitivity::Confidential,
        DeploymentTarget::Cloud | _ => FactSensitivity::Public,
    }
}

#[cfg(test)]
#[path = "../recall_tests/mod.rs"]
mod tests;
