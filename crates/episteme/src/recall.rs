//! Recall engine: 6-factor scoring for knowledge retrieval.
//!
//! Combines multiple signals to rank recall results:
//!
//! 1. **Vector similarity**: cosine distance from HNSW search
//! 2. **Decay**: FSRS power-law decay from last access time
//! 3. **Relevance**: nous-specific boost (your own memories rank higher)
//! 4. **Epistemic tier**: verified > inferred > assumed
//! 5. **Relationship proximity**: graph distance from query context entities
//! 6. **Access frequency**: memories accessed more often are more salient
//!
//! Each factor produces a score in [0.0, 1.0]. The final score is a weighted
//! combination, configurable per-nous via oikos cascade.

use serde::{Deserialize, Serialize};
use tracing::instrument;

use crate::knowledge::{EpistemicTier, FactType};

/// Tunable weights for the multi-factor recall scoring formula.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecallWeights {
    /// Weight for vector similarity (cosine distance). Default: 0.35
    pub vector_similarity: f64,
    /// Weight for FSRS power-law decay. Default: 0.20
    pub decay: f64,
    /// Weight for nous-relevance (own memories boosted). Default: 0.15
    pub relevance: f64,
    /// Weight for epistemic tier (verified > inferred > assumed). Default: 0.15
    pub epistemic_tier: f64,
    /// Weight for graph relationship proximity. Default: 0.10
    pub relationship_proximity: f64,
    /// Weight for access frequency. Default: 0.05
    pub access_frequency: f64,
}

impl Default for RecallWeights {
    fn default() -> Self {
        Self {
            vector_similarity: 0.35,
            decay: 0.20,
            relevance: 0.15,
            epistemic_tier: 0.15,
            relationship_proximity: 0.10,
            access_frequency: 0.05,
        }
    }
}

impl RecallWeights {
    /// Sum of all weights (for normalization).
    #[must_use]
    #[instrument(skip(self))]
    pub(crate) fn total(&self) -> f64 {
        self.vector_similarity
            + self.decay
            + self.relevance
            + self.epistemic_tier
            + self.relationship_proximity
            + self.access_frequency
    }

    /// Whether the graph intelligence recall pipeline should run.
    ///
    /// Returns `false` when the relationship proximity weight is effectively
    /// zero, meaning graph traversal results would be multiplied by zero and
    /// discarded. Callers should skip expensive graph operations (BFS,
    /// `PageRank`, Louvain) when this returns `false`.
    #[must_use]
    pub(crate) fn graph_recall_active(&self) -> bool {
        self.relationship_proximity >= f64::EPSILON
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
}

/// A scored recall candidate.
#[derive(Debug, Clone)]
pub struct ScoredResult {
    /// Content of the recalled memory.
    pub content: String,
    /// Source type (fact, message, note, document).
    pub source_type: String,
    /// Source ID.
    pub source_id: String,
    /// Which nous this belongs to.
    pub nous_id: String,
    /// Raw factor scores.
    pub factors: FactorScores,
    /// Final weighted score [0.0, 1.0].
    pub score: f64,
}

/// The recall engine.
#[derive(Debug, Clone)]
pub struct RecallEngine {
    weights: RecallWeights,
    /// Maximum access count for frequency normalization.
    max_access_count: f64,
}

impl RecallEngine {
    /// Create a new recall engine with default weights.
    #[must_use]
    #[instrument]
    pub fn new() -> Self {
        Self {
            weights: RecallWeights::default(),
            max_access_count: 100.0,
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
    pub(crate) fn with_max_access_count(mut self, count: f64) -> Self {
        self.max_access_count = count;
        self
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
    #[must_use]
    #[instrument(skip(self))]
    pub fn score_decay(
        &self,
        age_hours: f64,
        fact_type: FactType,
        tier: EpistemicTier,
        access_count: u32,
    ) -> f64 {
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
    pub(crate) fn score_relevance(&self, memory_nous_id: &str, query_nous_id: &str) -> f64 {
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
    pub(crate) fn score_epistemic_tier(&self, tier: &str) -> f64 {
        match tier {
            "verified" => 1.0,
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
            + factors.access_frequency * w.access_frequency;

        raw / total_weight
    }

    /// Score and rank a batch of candidates. Returns sorted by score descending.
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
    pub(crate) fn weights(&self) -> &RecallWeights {
        &self.weights
    }

    /// Epistemic tier score boosted by entity `PageRank` importance.
    ///
    /// Superset of [`score_epistemic_tier`](Self::score_epistemic_tier): calling with `importance=0.0`
    /// produces the same result as the base scorer.
    ///
    /// Returns the base tier score directly when graph recall weight is zero.
    #[must_use]
    #[instrument(skip(self))]
    pub(crate) fn score_epistemic_tier_with_importance(&self, tier: &str, importance: f64) -> f64 {
        let base = self.score_epistemic_tier(tier);
        // PERF: skip graph-enhanced scoring when relationship proximity weight is zero.
        if !self.weights.graph_recall_active() {
            return base;
        }
        crate::graph_intelligence::score_epistemic_tier_with_importance(base, importance)
    }

    /// Relationship proximity score with community-aware floor.
    ///
    /// Superset of [`score_relationship_proximity`](Self::score_relationship_proximity): calling with `same_cluster=false`
    /// produces the same result as the base scorer.
    ///
    /// Returns the base hop score directly when graph recall weight is zero.
    #[must_use]
    #[instrument(skip(self))]
    pub(crate) fn score_relationship_proximity_with_cluster(
        &self,
        hops: Option<u32>,
        same_cluster: bool,
    ) -> f64 {
        let base = self.score_relationship_proximity(hops);
        // PERF: skip graph-enhanced scoring when relationship proximity weight is zero.
        if !self.weights.graph_recall_active() {
            return base;
        }
        crate::graph_intelligence::score_relationship_proximity_with_cluster(base, same_cluster)
    }

    /// Access frequency score with supersession chain evolution bonus.
    ///
    /// Superset of [`score_access_frequency`](Self::score_access_frequency): calling with `chain_length=0`
    /// produces the same result as the base scorer.
    ///
    /// Returns the base access score directly when graph recall weight is zero.
    #[must_use]
    #[instrument(skip(self))]
    pub(crate) fn score_access_with_evolution(&self, access_count: u64, chain_length: u32) -> f64 {
        let base = self.score_access_frequency(access_count);
        // PERF: skip graph-enhanced scoring when relationship proximity weight is zero.
        if !self.weights.graph_recall_active() {
            return base;
        }
        crate::graph_intelligence::score_access_with_evolution(base, chain_length)
    }
}

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
pub(crate) fn refresh_stability_hours(fact_type: &str, tier: &str, access_count: u32) -> f64 {
    let ft = FactType::from_str_lossy(fact_type);
    let et = match tier {
        "verified" => EpistemicTier::Verified,
        "inferred" => EpistemicTier::Inferred,
        _ => EpistemicTier::Assumed,
    };
    compute_effective_stability(ft, et, access_count)
}

#[cfg(test)]
#[path = "recall_tests/mod.rs"]
mod test_suite;
