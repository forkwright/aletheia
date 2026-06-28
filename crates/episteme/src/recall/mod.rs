//! Recall engine: 11-factor scoring for knowledge retrieval.
//!
//! Combines multiple signals to rank recall results:
//!
//! 1. **Vector similarity**: cosine distance from HNSW search
//! 2. **Decay**: FSRS power-law decay from last access time
//! 3. **Relevance**: nous-specific boost (your own memories rank higher)
//! 4. **Epistemic tier**: verified > inferred > assumed
//! 5. **Relationship proximity**: graph distance from query context entities
//! 6. **Access frequency**: memories accessed more often are more salient
//! 7. **Graph importance**: `PageRank` hub entities rank higher
//! 8. **Surprise**: Bayesian topic-shift signal (`EM-LLM`); default weight 0.0
//! 9. **Evidence coverage**: `MemR3` gap-answering boost; default weight 0.0
//! 10. **Convergence**: consolidated-fact multiplicity (`log(1+sources)`); default weight 0.0
//! 11. **Serendipity**: obscure + distant candidate novelty; default weight 0.0
//!
//! Each factor produces a score in [0.0, 1.0]. The final score is a weighted
//! combination, configurable per-nous via oikos cascade. Factors 8–10 are
//! inert unless their weight is set positive in knowledge config.

use std::collections::HashSet;
use std::hash::BuildHasher;

use eidos::meta::{ArtefactMeta, Stamped};
use eidos::workspace::ProjectId;
use serde::{Deserialize, Serialize};
use tracing::instrument;

use crate::knowledge::{EpistemicTier, FactType, MemoryScope, Visibility};

#[cfg(feature = "reranker")]
pub mod reranker;

/// Explainable recall scoring helpers for HTTP search surfaces.
pub mod explain;

/// Type alias for a recall candidate used by rerankers.
pub type RecallCandidate = ScoredResult;

/// Tunable weights for the multi-factor recall scoring formula.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecallWeights {
    /// Weight for vector similarity (cosine distance). Default: 0.30
    pub vector_similarity: f64,
    /// Weight for FSRS power-law decay. Default: 0.20
    pub decay: f64,
    /// Weight for nous-relevance (own memories boosted). Default: 0.15
    pub relevance: f64,
    /// Weight for epistemic tier (verified > inferred > assumed). Default: 0.10
    pub epistemic_tier: f64,
    /// Weight for graph relationship proximity. Default: 0.10
    pub relationship_proximity: f64,
    /// Weight for access frequency. Default: 0.05
    pub access_frequency: f64,
    /// Weight for graph `PageRank` importance (hub entities boosted).
    /// Default: 0.10
    pub graph_importance: f64,
    /// Weight for serendipity (graph obscurity + semantic distance novelty).
    ///
    /// Non-zero values blend in an unexpectedness score derived from existing
    /// recall fields: `graph_importance` as obscurity (`1 - PageRank`) and
    /// vector distance as novelty. Default: 0.0 (inert — existing behaviour
    /// preserved). Enable by setting a positive weight in config.
    pub serendipity: f64,
    /// Weight for Bayesian surprise (topic-shift signal from EM-LLM).
    ///
    /// Non-zero values blend in a per-candidate surprise score produced by
    /// `SurpriseCalculator`. Default: 0.0 (inert — existing behaviour
    /// preserved). Enable by setting a positive weight in config.
    pub surprise: f64,
    /// Weight for evidence-gap coverage (`MemR3` iterative retrieval).
    ///
    /// Non-zero values blend in how well a candidate's fact ID appears in the
    /// evidence-gap tracker's answered set. Default: 0.0 (inert). Enable by
    /// setting a positive weight in config.
    pub evidence_coverage: f64,
    /// Weight for consolidated-fact convergence (multiplicity / source count).
    ///
    /// Non-zero values blend in `log(1 + source_count)` (from the
    /// `fact_multiplicity` side-index) so facts assembled from more independent
    /// converging observations rank higher. Default: 0.0 (inert). Legacy /
    /// non-consolidated facts have `source_count` 0 and score 0 here, so enabling
    /// the weight never regresses them.
    pub convergence: f64,
}

impl Default for RecallWeights {
    fn default() -> Self {
        Self {
            vector_similarity: 0.30,
            decay: 0.20,
            relevance: 0.15,
            epistemic_tier: 0.10,
            relationship_proximity: 0.10,
            access_frequency: 0.05,
            graph_importance: 0.10,
            serendipity: 0.0,
            surprise: 0.0,
            evidence_coverage: 0.0,
            convergence: 0.0,
        }
    }
}

impl RecallWeights {
    /// Sum of all weights (for normalization).
    #[must_use]
    #[instrument(skip(self))]
    pub fn total(&self) -> f64 {
        self.vector_similarity
            + self.decay
            + self.relevance
            + self.epistemic_tier
            + self.relationship_proximity
            + self.access_frequency
            + self.graph_importance
            + self.serendipity
            + self.surprise
            + self.evidence_coverage
            + self.convergence
    }

    /// Whether the graph intelligence recall pipeline should run.
    ///
    /// Returns `false` when the relationship proximity weight is effectively
    /// zero, meaning graph traversal results would be multiplied by zero and
    /// discarded. Callers should skip expensive graph operations (BFS,
    /// `PageRank`, Louvain) when this returns `false`.
    #[must_use]
    pub(crate) fn graph_recall_active(&self) -> bool {
        self.relationship_proximity >= f64::EPSILON || self.graph_importance >= f64::EPSILON
    }
}

/// Raw factor scores for a single recall candidate.
#[derive(Debug, Clone, Default)]
pub struct FactorScores {
    /// Cosine similarity score [0.0, 1.0] (1.0 = identical).
    pub vector_similarity: f64,
    /// FSRS decay score [0.0, 1.0] (1.0 = just accessed).
    pub decay: f64,
    /// Relevance score [0.0, 1.0] (1.0 = same nous).
    pub relevance: f64,
    /// Epistemic tier score [0.0, 1.0] (1.0 = verified).
    pub epistemic_tier: f64,
    /// Relationship proximity score [0.0, 1.0] (1.0 = direct neighbor).
    pub relationship_proximity: f64,
    /// Access frequency score [0.0, 1.0] (1.0 = most accessed).
    pub access_frequency: f64,
    /// `PageRank` graph importance score [0.0, 1.0] (1.0 = highest hub).
    pub graph_importance: f64,
    /// Serendipity score [0.0, 1.0] (1.0 = obscure and distant).
    ///
    /// Computed from existing recall fields, not a separate discovery pass.
    /// Default: 0.0 — inert unless `RecallWeights::serendipity > 0`.
    pub serendipity: f64,
    /// Bayesian surprise contribution [0.0, 1.0].
    ///
    /// Normalised inverse of the KL-divergence score from `SurpriseCalculator`.
    /// Default: 0.0 — inert unless `RecallWeights::surprise > 0`.
    pub surprise: f64,
    /// Evidence-coverage score [0.0, 1.0].
    ///
    /// 1.0 when the candidate's `source_id` appears in the `EvidenceGapTracker`
    /// answered set with high confidence; 0.0 when absent. Supplied by callers
    /// running iterative retrieval. Default: 0.0 — inert unless
    /// `RecallWeights::evidence_coverage > 0`.
    pub evidence_coverage: f64,
    /// Convergence score [0.0, 1.0] from consolidated-fact multiplicity.
    ///
    /// `log(1 + source_count)` normalised against a saturation point, so facts
    /// built from more independent observations score higher. 0.0 for legacy /
    /// non-consolidated facts (`source_count` 0). Default: 0.0 — inert unless
    /// `RecallWeights::convergence > 0`.
    pub convergence: f64,
}

/// A scored recall candidate.
#[derive(Debug, Clone)]
pub struct ScoredResult {
    /// Content of the recalled memory.
    pub content: String,
    /// Source type (fact, message, note, document).
    pub source_type: String,
    /// Source ID.
    // kanon:ignore RUST/primitive-for-domain-id — cross-engine portability; newtype migration tracked workspace-wide.
    pub source_id: String,
    /// Which nous this belongs to.
    // kanon:ignore RUST/primitive-for-domain-id — cross-engine portability; newtype migration tracked workspace-wide.
    pub nous_id: String,
    /// Raw factor scores.
    pub factors: FactorScores,
    /// Final weighted score [0.0, 1.0].
    pub score: f64,
    /// Data-sovereignty classification carried from the store so the recall
    /// pipeline can filter by the active provider's deployment target
    /// (#3404, #3413).
    pub sensitivity: crate::knowledge::FactSensitivity,
    /// Visibility level controlling which nous / consumers may see this result.
    ///
    /// `Private` is visible only to the owning nous; `Shared` and `Published`
    /// are broadly visible; `Restricted` is retained only for the owning nous
    /// until an access-list model is wired (#R722).
    pub visibility: Visibility,
    /// Memory sharing scope for team-memory quota enforcement.
    ///
    /// `None` for results from non-fact sources or facts created before the
    /// team memory model was introduced.
    pub scope: Option<crate::knowledge::MemoryScope>,
    /// Project partition for project-scoped recall.
    ///
    /// `None` means the result is global or predates project partitioning.
    pub project_id: Option<ProjectId>,
}

impl Stamped for ScoredResult {
    /// Returns provenance metadata for this scored recall result.
    ///
    /// `row_counts` carries `"results"` as 1 (single result) and `"score_tenths"`
    /// as the score multiplied by 10 and truncated, for human-readable bucketing.
    ///
    /// # Note on persist paths
    ///
    /// `ScoredResult` is a transient in-memory value produced by the recall
    /// pipeline. There is no direct disk persist path — `Stamped` is provided
    /// so callers that do persist recall batches (e.g. audit logs, eval
    /// snapshots) can attach provenance without reaching into internals.
    fn stamp(&self) -> ArtefactMeta {
        ArtefactMeta::new(
            concat!("episteme@", env!("CARGO_PKG_VERSION")),
            1,
            jiff::Timestamp::now().to_string(),
        )
        .with_count("results", 1)
    }
}

/// The recall engine.
#[derive(Clone)]
pub struct RecallEngine {
    weights: RecallWeights,
    /// Maximum access count for frequency normalization.
    max_access_count: f64,
    /// Optional reranker applied to the top-K after baseline scoring.
    #[cfg(feature = "reranker")]
    pub reranker: Option<std::sync::Arc<dyn reranker::Reranker>>,
    /// Number of top candidates to pass to the reranker.
    #[cfg(feature = "reranker")]
    pub reranker_top_k: usize,
}

impl std::fmt::Debug for RecallEngine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut d = f.debug_struct("RecallEngine");
        d.field("weights", &self.weights)
            .field("max_access_count", &self.max_access_count);
        #[cfg(feature = "reranker")]
        {
            d.field("reranker", &self.reranker.as_ref().map(|r| r.name()));
            d.field("reranker_top_k", &self.reranker_top_k);
        }
        d.finish()
    }
}

impl RecallEngine {
    /// Create a new recall engine with default weights.
    #[must_use]
    #[instrument]
    pub fn new() -> Self {
        Self {
            weights: RecallWeights::default(),
            max_access_count: 100.0,
            #[cfg(feature = "reranker")]
            reranker: None,
            #[cfg(feature = "reranker")]
            reranker_top_k: 20,
        }
    }

    /// Create with custom weights.
    #[must_use]
    #[instrument(skip(weights))]
    pub fn with_weights(weights: RecallWeights) -> Self {
        Self {
            weights,
            ..Self::default()
        }
    }

    /// Set the max access count for frequency normalization.
    #[must_use]
    #[instrument(skip(self))]
    #[cfg_attr(not(test), expect(dead_code, reason = "recall engine scoring methods"))]
    pub(crate) fn with_max_access_count(mut self, count: f64) -> Self {
        self.max_access_count = count;
        self
    }

    /// Attach an optional reranker to the engine.
    #[cfg(feature = "reranker")]
    #[must_use]
    #[instrument(skip(self, reranker))]
    pub fn with_reranker(
        mut self,
        reranker: Option<std::sync::Arc<dyn reranker::Reranker>>,
    ) -> Self {
        self.reranker = reranker;
        self
    }

    /// Set how many top candidates are passed to the reranker.
    #[cfg(feature = "reranker")]
    #[must_use]
    #[instrument(skip(self))]
    pub fn with_reranker_top_k(mut self, top_k: usize) -> Self {
        self.reranker_top_k = top_k;
        self
    }

    /// Baseline rank followed by an optional reranker pass.
    ///
    /// When [`Self::reranker`] is `None` this is identical to [`Self::rank`].
    /// When it is `Some`, the top [`Self::reranker_top_k`] candidates are
    /// forwarded to the reranker and the result is concatenated with the
    /// remaining tail.
    #[cfg(feature = "reranker")]
    #[must_use]
    #[instrument(skip(self, candidates), fields(count = candidates.len()))]
    pub async fn rank_and_rerank(
        &self,
        query: &str,
        candidates: Vec<ScoredResult>,
    ) -> Vec<ScoredResult> {
        let mut ranked = self.rank(candidates);
        if let Some(ref reranker) = self.reranker {
            let top_k = self.reranker_top_k.min(ranked.len());
            if top_k == 0 {
                return ranked;
            }
            let tail = ranked.split_off(top_k);
            let top_for_rerank = ranked.clone();
            match reranker.rerank(query, top_for_rerank).await {
                Ok(mut reranked_top) => {
                    reranked_top.extend(tail);
                    ranked = reranked_top;
                }
                Err(e) => {
                    tracing::warn!(
                        error = ?e,
                        reranker = reranker.name(),
                        "reranker failed; falling back to baseline ranking for top-k"
                    );
                    ranked.extend(tail);
                }
            }
        }
        ranked
    }

    /// Compute the vector similarity score from cosine distance.
    ///
    /// Cosine distance is in [0.0, 2.0] (0 = identical, 2 = opposite).
    /// We convert to a similarity in [0.0, 1.0].
    #[must_use]
    #[instrument(skip(self))]
    pub fn score_vector_similarity(&self, cosine_distance: f64) -> f64 {
        let raw_similarity = 1.0 - cosine_distance / 2.0;
        if !(-1.0..=1.0).contains(&raw_similarity) {
            tracing::warn!(
                raw_similarity = raw_similarity,
                cosine_distance = cosine_distance,
                "vector may not be normalized: raw_similarity={raw_similarity}"
            );
        }
        raw_similarity.clamp(0.0, 1.0)
    }

    /// Compute the FSRS power-law decay score.
    ///
    /// Formula: `R(t) = (1 + 19/81 × t/S)^(-0.5)`
    ///
    /// Where:
    /// - `t` = hours since last access (or creation if never accessed)
    /// - `S` = effective stability = base × `tier_mult` × `access_mult`
    /// - Access growth: `1 + 0.1 × ln(1 + access_count)` (logarithmic, bounded)
    ///
    /// Properties:
    /// - R(0) = 1.0 for any stability
    /// - R(S) ≈ 0.9 (by design of FSRS 19/81 constant)
    /// - Output ∈ [0.0, 1.0] for any finite input (including negative or NaN
    ///   `age_hours`, which are clamped to 0 — see below).
    ///
    /// # Clock-jump handling (#3392)
    ///
    /// If `age_hours` is negative (system clock jumped backward; e.g. NTP
    /// correction after suspend/resume) or NaN, it is clamped to `0.0` so the
    /// formula returns `1.0` ("just now"). WHY: a negative age would
    /// previously flow through the arithmetic and, combined with cross-agent
    /// multipliers downstream, could inflate recall scores. Clamping is
    /// strictly safer than propagating an error here — ranking must never
    /// crash a recall pipeline.
    #[must_use]
    #[instrument(skip(self))]
    pub fn score_decay(
        &self,
        age_hours: f64,
        fact_type: FactType,
        tier: EpistemicTier,
        access_count: u32,
    ) -> f64 {
        let age_hours = crate::decay::sanitize_age_hours(age_hours);
        if age_hours <= 0.0 {
            return 1.0;
        }
        let s = compute_effective_stability(fact_type, tier, access_count);
        // SAFETY: Guard against zero/negative stability to prevent division by zero.
        if s <= 0.0 {
            return 0.0;
        }
        (1.0 + (19.0 / 81.0) * age_hours / s).powf(-0.5)
    }

    /// Compute the relevance score.
    ///
    /// 1.0 if the memory belongs to the querying nous, 0.5 for shared, 0.3 for other.
    #[must_use]
    #[instrument(skip(self))]
    pub fn score_relevance(&self, memory_nous_id: &str, query_nous_id: &str) -> f64 {
        if memory_nous_id == query_nous_id {
            1.0
        } else if memory_nous_id.is_empty() {
            0.5 // NOTE: Shared memory (no owning nous)
        } else {
            0.3 // NOTE: Another agent's memory
        }
    }

    /// Compute the epistemic tier score.
    #[must_use]
    #[instrument(skip(self))]
    pub fn score_epistemic_tier(&self, tier: &str) -> f64 {
        match tier {
            "verified" => 1.0,
            "reflected" => 0.8,
            "inferred" => 0.6,
            // NOTE: assumed or unknown tiers get lowest weight
            _ => 0.3,
        }
    }

    /// Compute the relationship proximity score from graph hops.
    ///
    /// Same entity (0 hops) or direct neighbor (1 hop) = 1.0, 2-hop = 0.5, 3-hop = 0.25, etc.
    /// No connection = 0.0.
    #[must_use]
    #[instrument(skip(self))]
    #[expect(
        clippy::unused_self,
        reason = "method signature kept for future scorer extensibility"
    )]
    pub(crate) fn score_relationship_proximity(&self, hops: Option<u32>) -> f64 {
        match hops {
            Some(0 | 1) => 1.0,
            Some(2) => 0.5,
            Some(3) => 0.25,
            Some(n) => (0.5_f64).powi(i32::try_from(n.saturating_sub(1)).unwrap_or(i32::MAX)),
            None => 0.0,
        }
    }

    /// Compute the access frequency score.
    ///
    /// Logarithmic scaling: `score = ln(1 + count) / ln(1 + max_count)`
    #[must_use]
    #[instrument(skip(self))]
    pub(crate) fn score_access_frequency(&self, access_count: u64) -> f64 {
        #[expect(
            clippy::cast_precision_loss,
            clippy::as_conversions,
            reason = "u64→f64: access count fits in f64"
        )]
        let count = access_count as f64;
        (1.0 + count).ln() / (1.0 + self.max_access_count).ln()
    }

    /// Compute the weighted final score from factor scores.
    #[instrument(skip(self, factors))]
    #[must_use]
    pub(crate) fn compute_score(&self, factors: &FactorScores) -> f64 {
        let w = &self.weights;
        let total_weight = w.total();
        if total_weight == 0.0 {
            return 0.0;
        }

        let raw = factors.vector_similarity * w.vector_similarity
            + factors.decay * w.decay
            + factors.relevance * w.relevance
            + factors.epistemic_tier * w.epistemic_tier
            + factors.relationship_proximity * w.relationship_proximity
            + factors.access_frequency * w.access_frequency
            + factors.graph_importance * w.graph_importance
            + factors.serendipity * w.serendipity
            + factors.surprise * w.surprise
            + factors.evidence_coverage * w.evidence_coverage
            + factors.convergence * w.convergence;

        raw / total_weight
    }

    /// Pre-filter candidates via side-query selection, then score and rank.
    ///
    /// WHY: runs `pre_filter_by_side_query` before factor scoring so the
    /// expensive `compute_score` loop operates on a narrower candidate set.
    /// When `selected_ids` is empty, all candidates pass through unfiltered.
    ///
    /// # Complexity
    ///
    /// O(C) where C is candidate count. Scoring is O(1) per candidate, sorting
    /// is O(C log C) for final ranking.
    #[must_use]
    #[instrument(skip(self, candidates, selected_ids), fields(count = candidates.len()))]
    pub fn rank_with_prefilter<S: BuildHasher>(
        &self,
        candidates: Vec<ScoredResult>,
        selected_ids: &HashSet<String, S>,
    ) -> Vec<ScoredResult> {
        let filtered = pre_filter_by_side_query(candidates, selected_ids);
        self.rank(filtered)
    }

    /// Score and rank a batch of candidates. Returns sorted by score descending.
    ///
    /// # Complexity
    ///
    /// O(C log C) where C is candidate count. O(C) for scoring, O(C log C) for sort.
    #[must_use]
    #[instrument(skip(self, candidates), fields(count = candidates.len()))]
    pub fn rank(&self, mut candidates: Vec<ScoredResult>) -> Vec<ScoredResult> {
        let start = std::time::Instant::now();
        for candidate in &mut candidates {
            candidate.score = self.compute_score(&candidate.factors);
        }
        candidates.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        crate::metrics::record_recall_duration("_all", start.elapsed().as_secs_f64());
        candidates
    }

    /// Access the current weights.
    #[must_use]
    #[instrument(skip(self))]
    pub fn weights(&self) -> &RecallWeights {
        &self.weights
    }

    /// Apply a graph-enhanced scorer when graph recall is active,
    /// otherwise return the base score unchanged.
    ///
    /// PERF: skips the `enhance` closure entirely when the relationship
    /// proximity weight is zero.
    #[must_use]
    fn graph_enhanced(&self, base: f64, enhance: impl FnOnce(f64) -> f64) -> f64 {
        if self.weights.graph_recall_active() {
            enhance(base)
        } else {
            base
        }
    }

    /// Epistemic tier score boosted by entity `PageRank` importance.
    ///
    /// Superset of [`score_epistemic_tier`](Self::score_epistemic_tier): calling with `importance=0.0`
    /// produces the same result as the base scorer.
    ///
    /// Returns the base tier score directly when graph recall weight is zero.
    #[must_use]
    #[instrument(skip(self))]
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "knowledge pipeline infrastructure")
    )]
    pub(crate) fn score_epistemic_tier_with_importance(&self, tier: &str, importance: f64) -> f64 {
        let base = self.score_epistemic_tier(tier);
        self.graph_enhanced(base, |b| {
            crate::graph_intelligence::score_epistemic_tier_with_importance(b, importance)
        })
    }

    /// Relationship proximity score with community-aware floor.
    ///
    /// Superset of [`score_relationship_proximity`](Self::score_relationship_proximity): calling with `same_cluster=false`
    /// produces the same result as the base scorer.
    ///
    /// Returns the base hop score directly when graph recall weight is zero.
    #[must_use]
    #[instrument(skip(self))]
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "knowledge pipeline infrastructure")
    )]
    pub(crate) fn score_relationship_proximity_with_cluster(
        &self,
        hops: Option<u32>,
        same_cluster: bool,
    ) -> f64 {
        let base = self.score_relationship_proximity(hops);
        self.graph_enhanced(base, |b| {
            crate::graph_intelligence::score_relationship_proximity_with_cluster(b, same_cluster)
        })
    }

    /// Access frequency score with supersession chain evolution bonus.
    ///
    /// Superset of [`score_access_frequency`](Self::score_access_frequency): calling with `chain_length=0`
    /// produces the same result as the base scorer.
    ///
    /// Returns the base access score directly when graph recall weight is zero.
    #[must_use]
    #[instrument(skip(self))]
    #[cfg_attr(not(test), expect(dead_code, reason = "recall engine scoring methods"))]
    pub(crate) fn score_access_with_evolution(&self, access_count: u64, chain_length: u32) -> f64 {
        let base = self.score_access_frequency(access_count);
        self.graph_enhanced(base, |b| {
            crate::graph_intelligence::score_access_with_evolution(b, chain_length)
        })
    }

    /// Compute the graph importance score from normalized `PageRank`.
    ///
    /// Returns the importance value clamped to [0.0, 1.0].
    /// When no graph data is available, importance is 0.0 and this
    /// returns 0.0, producing no boost.
    #[must_use]
    #[instrument(skip(self))]
    pub fn score_graph_importance(&self, importance: f64) -> f64 {
        importance.clamp(0.0, 1.0)
    }

    /// Compute the serendipity score contribution for recall ranking.
    ///
    /// WHY: serendipity should reward obscure, farther-apart candidates using
    /// only fields already present in recall results, without running the
    /// heavier discovery engine on the hot recall path.
    ///
    /// `obscurity = 1 - graph_importance` and `distance_novelty = distance / (1 + distance)`.
    /// The blend is clamped to [0.0, 1.0] so it remains a normalised factor.
    ///
    /// Returns 0.0 when `RecallWeights::serendipity` is effectively zero so
    /// callers can skip the calculation entirely in the common case.
    #[must_use]
    #[instrument(skip(self))]
    pub fn score_serendipity(&self, graph_importance: f64, distance: f64) -> f64 {
        if self.weights.serendipity < f64::EPSILON {
            return 0.0;
        }

        let obscurity = (1.0 - graph_importance.clamp(0.0, 1.0)).clamp(0.0, 1.0);
        let distance = distance.max(0.0);
        let distance_novelty = (distance / (1.0 + distance)).clamp(0.0, 1.0);
        (0.6 * obscurity + 0.4 * distance_novelty).clamp(0.0, 1.0)
    }

    /// Compute the surprise score contribution for recall ranking.
    ///
    /// Converts a raw KL-divergence surprise value (nats, from
    /// `SurpriseCalculator::compute_surprise`) into a normalised [0.0, 1.0]
    /// score using a sigmoid mapping so that high-surprise candidates rank
    /// higher when `RecallWeights::surprise > 0`.
    ///
    /// `surprise_nats = midpoint_nats` → score 0.5 (neutral); values below the
    /// midpoint score toward 0.0 and above toward 1.0. At the default 2.0-nat
    /// midpoint, zero-divergence content scores ≈0.12 (`sigmoid(-2)`). Pass
    /// `DEFAULT_THRESHOLD` from `crate::surprise` for the standard boundary.
    ///
    /// Returns 0.0 when `RecallWeights::surprise` is effectively zero so
    /// callers can skip the `SurpriseCalculator` entirely in the common case.
    #[must_use]
    #[instrument(skip(self))]
    pub fn score_surprise(&self, surprise_nats: f64, midpoint_nats: f64) -> f64 {
        if self.weights.surprise < f64::EPSILON {
            return 0.0;
        }
        // Sigmoid: 1 / (1 + exp(-(x - midpoint))). x = surprise_nats.
        let x = surprise_nats - midpoint_nats;
        1.0 / (1.0 + (-x).exp())
    }

    /// Compute the evidence-coverage score for a candidate.
    ///
    /// Returns the confidence from the `EvidenceGapTracker`'s answered set
    /// when the candidate's `source_id` appears in `answered_ids`, or 0.0
    /// when the ID is absent (meaning the candidate does not directly answer
    /// a known gap).
    ///
    /// Callers running iterative retrieval (`MemR3` style) should supply the
    /// per-round answered map; callers not using evidence-gap tracking should
    /// omit this call (weights default to 0.0 so omitting it is equivalent).
    ///
    /// Returns 0.0 immediately when `RecallWeights::evidence_coverage` is
    /// effectively zero.
    #[must_use]
    #[instrument(skip(self, answered_ids))]
    pub fn score_evidence_coverage(
        &self,
        source_id: &str,
        answered_ids: &std::collections::HashMap<String, f64>,
    ) -> f64 {
        if self.weights.evidence_coverage < f64::EPSILON {
            return 0.0;
        }
        answered_ids
            .get(source_id)
            .copied()
            .unwrap_or(0.0)
            .clamp(0.0, 1.0)
    }

    /// Compute the convergence score for a candidate from its consolidated-fact
    /// source count (`fact_multiplicity` side-index).
    ///
    /// `log(1 + source_count)` normalised against [`CONVERGENCE_SATURATION`] so
    /// the signal is logarithmic — a 50-source fact does not dominate a
    /// 10-source one. `source_count = 0` (legacy / non-consolidated facts, where
    /// `get_fact_multiplicity` returns `None`) scores 0.0, so enabling the
    /// weight never regresses those facts.
    ///
    /// Returns 0.0 immediately when `RecallWeights::convergence` is effectively
    /// zero so callers can skip the side-index lookup in the common case.
    #[must_use]
    #[instrument(skip(self))]
    pub fn score_convergence(&self, source_count: u32) -> f64 {
        if self.weights.convergence < f64::EPSILON {
            return 0.0;
        }
        let n = f64::from(source_count);
        ((1.0 + n).ln() / (1.0 + CONVERGENCE_SATURATION).ln()).clamp(0.0, 1.0)
    }
}

/// Source count at which the convergence score saturates to ~1.0. Facts built
/// from this many or more independent observations receive the maximal
/// convergence boost; the logarithmic curve keeps lower counts well-separated.
const CONVERGENCE_SATURATION: f64 = 32.0;

impl Default for RecallEngine {
    fn default() -> Self {
        Self::new()
    }
}

/// Compute effective stability for FSRS decay.
///
/// `S = base_stability × tier_multiplier × access_growth`
///
/// Access growth is logarithmic: `1 + 0.1 × ln(1 + access_count)`.
/// This ensures frequently accessed facts decay slower, but growth is bounded.
#[must_use]
pub(crate) fn compute_effective_stability(
    fact_type: FactType,
    tier: EpistemicTier,
    access_count: u32,
) -> f64 {
    let s_base = fact_type.base_stability_hours();
    let tier_mult = tier.stability_multiplier();
    let access_mult = 1.0 + 0.1 * (1.0 + f64::from(access_count)).ln();
    s_base * tier_mult * access_mult
}

/// Recompute the stored `stability_hours` value for a fact.
///
/// This is the same formula as [`compute_effective_stability`] but takes string
/// parameters for compatibility with the knowledge store's string-typed fields.
///
/// The stored value is for diagnostics/reporting: actual `R(t)` is computed
/// on-the-fly at query time via [`RecallEngine::score_decay`].
#[must_use]
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "FSRS stability recomputation for knowledge store maintenance"
    )
)]
#[expect(
    clippy::match_same_arms,
    reason = "explicit assumed arm documents the full tier surface; wildcard is a defensive fallback"
)]
pub(crate) fn refresh_stability_hours(fact_type: &str, tier: &str, access_count: u32) -> f64 {
    let ft = FactType::from_str_lossy(fact_type);
    let et = match tier {
        "verified" => EpistemicTier::Verified,
        "reflected" => EpistemicTier::Reflected,
        "inferred" => EpistemicTier::Inferred,
        "assumed" => EpistemicTier::Assumed,
        "training" => EpistemicTier::Training,
        _ => EpistemicTier::Assumed,
    };
    compute_effective_stability(ft, et, access_count)
}

/// Pre-filter recall candidates using side-query selections.
///
/// Retains only candidates whose `source_id` appears in `selected_ids`.
/// Designed to run between vector search retrieval and factor scoring
/// to reduce the candidate set before the more expensive scoring runs.
///
/// # Arguments
///
/// * `candidates` — Raw scored results from vector search.
/// * `selected_ids` — Source IDs chosen by the side-query selector.
///
/// # Returns
///
/// Filtered candidates preserving original order.
#[must_use]
pub fn pre_filter_by_side_query<S: BuildHasher>(
    candidates: Vec<ScoredResult>,
    selected_ids: &HashSet<String, S>,
) -> Vec<ScoredResult> {
    if selected_ids.is_empty() {
        return candidates;
    }
    candidates
        .into_iter()
        .filter(|c| selected_ids.contains(&c.source_id))
        .collect()
}

/// Filter recall candidates by visibility relative to the querying nous.
///
/// Semantics:
/// - `Private` — retained only when the candidate's `nous_id` matches `query_nous_id`.
/// - `Shared` / `Published` — always retained.
/// - `Restricted` — retained only for the owning nous (same as `Private` until an
///   access-list model exists).
///
/// This is a pure, stateless helper designed for use in the recall pipeline
/// after retrieval and before ranking. It does not consult any access-list
/// storage.
///
/// # Complexity
///
/// O(C) where C is the number of candidates.
#[must_use]
pub fn filter_by_cohort_visibility(
    candidates: Vec<ScoredResult>,
    query_nous_id: &str,
) -> Vec<ScoredResult> {
    candidates
        .into_iter()
        .filter(|c| match c.visibility {
            Visibility::Private | Visibility::Restricted => c.nous_id == query_nous_id,
            Visibility::Shared | Visibility::Published => true,
            _ => {
                // WHY: `Visibility` is `#[non_exhaustive]`. Future variants
                // are conservatively treated as Private until the spec defines
                // otherwise.
                c.nous_id == query_nous_id
            }
        })
        .collect()
}

/// Filter recall candidates by visibility level.
///
/// Retains candidates whose visibility is **at most** `min` according to the
/// ordering `Private < Shared < Restricted < Published`. This is an
/// upper-bound filter: more visible (less restrictive) facts are excluded
/// when a low `min` is set.
///
/// | `min`          | Facts retained                          |
/// |----------------|----------------------------------------|
/// | `Private`      | `Private` only                         |
/// | `Shared`       | `Private`, `Shared`                    |
/// | `Restricted`   | `Private`, `Shared`, `Restricted`      |
/// | `Published`    | all visibility levels                  |
///
/// # Complexity
///
/// O(C) where C is the number of candidates.
#[must_use]
pub fn filter_by_visibility(candidates: Vec<ScoredResult>, min: Visibility) -> Vec<ScoredResult> {
    candidates
        .into_iter()
        .filter(|c| c.visibility >= min)
        .collect()
}

/// Project recall scope.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ProjectRecallScope {
    /// Return all projects and global results.
    Global,
    /// Return only this project plus global results.
    Project(ProjectId),
}

/// Filter recall candidates by project partition.
///
/// Project-scoped reads retain true global rows (`project_id = None` and
/// `scope != Project`) so promoted cross-project facts remain visible. Rows
/// marked `scope = Project` without a `project_id` are malformed legacy rows
/// and are excluded from project-scoped reads instead of being treated as global.
///
/// # Complexity
///
/// O(C) where C is the number of candidates.
#[must_use]
pub fn filter_by_project_scope(
    candidates: Vec<ScoredResult>,
    scope: &ProjectRecallScope,
) -> Vec<ScoredResult> {
    match scope {
        ProjectRecallScope::Global => candidates,
        ProjectRecallScope::Project(project_id) => candidates
            .into_iter()
            .filter(|c| match c.project_id.as_ref() {
                Some(id) => id == project_id,
                None => c.scope != Some(MemoryScope::Project),
            })
            .collect(),
    }
}

#[cfg(test)]
#[path = "../recall_tests/mod.rs"]
mod test_suite;
