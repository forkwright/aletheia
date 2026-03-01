//! Recall engine — 6-factor scoring for knowledge retrieval.
//!
//! Combines multiple signals to rank recall results:
//!
//! 1. **Vector similarity** — cosine distance from HNSW search
//! 2. **Recency** — exponential decay from recording time
//! 3. **Relevance** — nous-specific boost (your own memories rank higher)
//! 4. **Epistemic tier** — verified > inferred > assumed
//! 5. **Relationship proximity** — graph distance from query context entities
//! 6. **Access frequency** — memories accessed more often are more salient
//!
//! Each factor produces a score in [0.0, 1.0]. The final score is a weighted
//! combination, configurable per-nous via oikos cascade.

use serde::{Deserialize, Serialize};
use tracing::instrument;

/// Configuration for recall scoring weights.
///
/// All weights should sum to ~1.0 for interpretable scores,
/// but this is not enforced — the engine normalizes output.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecallWeights {
    /// Weight for vector similarity (cosine distance). Default: 0.35
    pub vector_similarity: f64,
    /// Weight for recency (exponential decay). Default: 0.20
    pub recency: f64,
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
            recency: 0.20,
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
    pub fn total(&self) -> f64 {
        self.vector_similarity
            + self.recency
            + self.relevance
            + self.epistemic_tier
            + self.relationship_proximity
            + self.access_frequency
    }
}

/// Raw factor scores for a single recall candidate.
#[derive(Debug, Clone, Default)]
pub struct FactorScores {
    /// Cosine similarity score [0.0, 1.0] (1.0 = identical).
    pub vector_similarity: f64,
    /// Recency score [0.0, 1.0] (1.0 = just recorded).
    pub recency: f64,
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
    /// Half-life for recency decay in hours. Default: 168 (1 week).
    recency_half_life_hours: f64,
    /// Maximum access count for frequency normalization.
    max_access_count: f64,
}

impl RecallEngine {
    /// Create a new recall engine with default weights.
    #[must_use]
    pub fn new() -> Self {
        Self {
            weights: RecallWeights::default(),
            recency_half_life_hours: 168.0,
            max_access_count: 100.0,
        }
    }

    /// Create with custom weights.
    #[must_use]
    pub fn with_weights(weights: RecallWeights) -> Self {
        Self {
            weights,
            ..Self::default()
        }
    }

    /// Set the recency half-life in hours.
    #[must_use]
    pub fn with_recency_half_life(mut self, hours: f64) -> Self {
        self.recency_half_life_hours = hours;
        self
    }

    /// Set the max access count for frequency normalization.
    #[must_use]
    pub fn with_max_access_count(mut self, count: f64) -> Self {
        self.max_access_count = count;
        self
    }

    /// Compute the vector similarity score from cosine distance.
    ///
    /// Cosine distance is in [0.0, 2.0] (0 = identical, 2 = opposite).
    /// We convert to a similarity in [0.0, 1.0].
    #[must_use]
    pub fn score_vector_similarity(&self, cosine_distance: f64) -> f64 {
        (1.0 - cosine_distance / 2.0).clamp(0.0, 1.0)
    }

    /// Compute the recency score from age in hours.
    ///
    /// Exponential decay: `score = 0.5^(age / half_life)`
    #[must_use]
    pub fn score_recency(&self, age_hours: f64) -> f64 {
        if age_hours <= 0.0 {
            return 1.0;
        }
        (0.5_f64).powf(age_hours / self.recency_half_life_hours)
    }

    /// Compute the relevance score.
    ///
    /// 1.0 if the memory belongs to the querying nous, 0.5 for shared, 0.3 for other.
    #[must_use]
    pub fn score_relevance(&self, memory_nous_id: &str, query_nous_id: &str) -> f64 {
        if memory_nous_id == query_nous_id {
            1.0
        } else if memory_nous_id.is_empty() {
            0.5 // Shared memory
        } else {
            0.3 // Another agent's memory
        }
    }

    /// Compute the epistemic tier score.
    #[must_use]
    pub fn score_epistemic_tier(&self, tier: &str) -> f64 {
        match tier {
            "verified" => 1.0,
            "inferred" => 0.6,
            // assumed or unknown
            _ => 0.3,
        }
    }

    /// Compute the relationship proximity score from graph hops.
    ///
    /// Direct neighbor = 1.0, 2-hop = 0.5, 3-hop = 0.25, etc.
    /// No connection = 0.0.
    #[must_use]
    pub fn score_relationship_proximity(&self, hops: Option<u32>) -> f64 {
        match hops {
            Some(0 | 1) => 1.0,  // Same entity or direct neighbor
            Some(2) => 0.5,
            Some(3) => 0.25,
            Some(n) => (0.5_f64).powi(i32::try_from(n.saturating_sub(1)).unwrap_or(i32::MAX)),
            None => 0.0,         // No connection
        }
    }

    /// Compute the access frequency score.
    ///
    /// Logarithmic scaling: `score = log(1 + count) / log(1 + max_count)`
    #[must_use]
    pub fn score_access_frequency(&self, access_count: u64) -> f64 {
        #[allow(clippy::cast_precision_loss)]
        let count = access_count as f64;
        (1.0 + count).ln() / (1.0 + self.max_access_count).ln()
    }

    /// Compute the weighted final score from factor scores.
    #[instrument(skip(self, factors))]
    #[must_use]
    pub fn compute_score(&self, factors: &FactorScores) -> f64 {
        let w = &self.weights;
        let total_weight = w.total();
        if total_weight == 0.0 {
            return 0.0;
        }

        let raw = factors.vector_similarity * w.vector_similarity
            + factors.recency * w.recency
            + factors.relevance * w.relevance
            + factors.epistemic_tier * w.epistemic_tier
            + factors.relationship_proximity * w.relationship_proximity
            + factors.access_frequency * w.access_frequency;

        raw / total_weight
    }

    /// Score and rank a batch of candidates. Returns sorted by score descending.
    #[must_use]
    pub fn rank(&self, mut candidates: Vec<ScoredResult>) -> Vec<ScoredResult> {
        for candidate in &mut candidates {
            candidate.score = self.compute_score(&candidate.factors);
        }
        candidates.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
        candidates
    }

    /// Access the current weights.
    #[must_use]
    pub fn weights(&self) -> &RecallWeights {
        &self.weights
    }
}

impl Default for RecallEngine {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn engine() -> RecallEngine {
        RecallEngine::new()
    }

    // --- Vector similarity ---

    #[test]
    fn vector_similarity_identical() {
        let e = engine();
        assert!((e.score_vector_similarity(0.0) - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn vector_similarity_opposite() {
        let e = engine();
        assert!((e.score_vector_similarity(2.0)).abs() < f64::EPSILON);
    }

    #[test]
    fn vector_similarity_midpoint() {
        let e = engine();
        assert!((e.score_vector_similarity(1.0) - 0.5).abs() < f64::EPSILON);
    }

    // --- Recency ---

    #[test]
    fn recency_just_now() {
        let e = engine();
        assert!((e.score_recency(0.0) - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn recency_one_half_life() {
        let e = engine();
        // After one half-life (168h = 1 week), score should be 0.5
        assert!((e.score_recency(168.0) - 0.5).abs() < 0.01);
    }

    #[test]
    fn recency_two_half_lives() {
        let e = engine();
        assert!((e.score_recency(336.0) - 0.25).abs() < 0.01);
    }

    #[test]
    fn recency_custom_half_life() {
        let e = RecallEngine::new().with_recency_half_life(24.0);
        assert!((e.score_recency(24.0) - 0.5).abs() < 0.01);
    }

    // --- Relevance ---

    #[test]
    fn relevance_same_nous() {
        let e = engine();
        assert!((e.score_relevance("syn", "syn") - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn relevance_shared() {
        let e = engine();
        assert!((e.score_relevance("", "syn") - 0.5).abs() < f64::EPSILON);
    }

    #[test]
    fn relevance_other_nous() {
        let e = engine();
        assert!((e.score_relevance("demiurge", "syn") - 0.3).abs() < f64::EPSILON);
    }

    // --- Epistemic tier ---

    #[test]
    fn epistemic_verified_highest() {
        let e = engine();
        let v = e.score_epistemic_tier("verified");
        let i = e.score_epistemic_tier("inferred");
        let a = e.score_epistemic_tier("assumed");
        assert!(v > i);
        assert!(i > a);
    }

    // --- Relationship proximity ---

    #[test]
    fn proximity_direct_neighbor() {
        let e = engine();
        assert!((e.score_relationship_proximity(Some(1)) - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn proximity_two_hops() {
        let e = engine();
        assert!((e.score_relationship_proximity(Some(2)) - 0.5).abs() < f64::EPSILON);
    }

    #[test]
    fn proximity_no_connection() {
        let e = engine();
        assert!((e.score_relationship_proximity(None)).abs() < f64::EPSILON);
    }

    // --- Access frequency ---

    #[test]
    fn access_frequency_zero() {
        let e = engine();
        assert!((e.score_access_frequency(0)).abs() < 0.01);
    }

    #[test]
    fn access_frequency_max() {
        let e = engine();
        assert!((e.score_access_frequency(100) - 1.0).abs() < 0.01);
    }

    #[test]
    fn access_frequency_logarithmic() {
        let e = engine();
        let s10 = e.score_access_frequency(10);
        let s50 = e.score_access_frequency(50);
        let s100 = e.score_access_frequency(100);
        // Logarithmic: each doubling adds less
        assert!(s50 > s10);
        assert!(s100 > s50);
        assert!(s50 - s10 > s100 - s50);
    }

    // --- Composite scoring ---

    #[test]
    fn perfect_score() {
        let e = engine();
        let factors = FactorScores {
            vector_similarity: 1.0,
            recency: 1.0,
            relevance: 1.0,
            epistemic_tier: 1.0,
            relationship_proximity: 1.0,
            access_frequency: 1.0,
        };
        assert!((e.compute_score(&factors) - 1.0).abs() < 0.01);
    }

    #[test]
    fn zero_score() {
        let e = engine();
        let factors = FactorScores::default();
        assert!((e.compute_score(&factors)).abs() < f64::EPSILON);
    }

    #[test]
    fn vector_similarity_dominates() {
        let e = engine();
        // A memory with high vector similarity but low everything else
        // should still score well since vector_similarity has the highest weight
        let high_vec = FactorScores {
            vector_similarity: 1.0,
            ..FactorScores::default()
        };
        let high_recency = FactorScores {
            recency: 1.0,
            ..FactorScores::default()
        };
        assert!(e.compute_score(&high_vec) > e.compute_score(&high_recency));
    }

    // --- Ranking ---

    #[test]
    fn rank_sorts_by_score_descending() {
        let e = engine();
        let candidates = vec![
            ScoredResult {
                content: "low".to_owned(),
                source_type: "fact".to_owned(),
                source_id: "f1".to_owned(),
                nous_id: "syn".to_owned(),
                factors: FactorScores {
                    vector_similarity: 0.2,
                    ..FactorScores::default()
                },
                score: 0.0,
            },
            ScoredResult {
                content: "high".to_owned(),
                source_type: "fact".to_owned(),
                source_id: "f2".to_owned(),
                nous_id: "syn".to_owned(),
                factors: FactorScores {
                    vector_similarity: 0.9,
                    recency: 0.8,
                    relevance: 1.0,
                    epistemic_tier: 1.0,
                    ..FactorScores::default()
                },
                score: 0.0,
            },
            ScoredResult {
                content: "mid".to_owned(),
                source_type: "fact".to_owned(),
                source_id: "f3".to_owned(),
                nous_id: "syn".to_owned(),
                factors: FactorScores {
                    vector_similarity: 0.5,
                    recency: 0.5,
                    ..FactorScores::default()
                },
                score: 0.0,
            },
        ];

        let ranked = e.rank(candidates);
        assert_eq!(ranked[0].content, "high");
        assert_eq!(ranked[1].content, "mid");
        assert_eq!(ranked[2].content, "low");
        assert!(ranked[0].score > ranked[1].score);
        assert!(ranked[1].score > ranked[2].score);
    }

    // --- Custom weights ---

    #[test]
    fn custom_weights_change_ranking() {
        // With only recency weight, a recent low-similarity memory beats
        // an old high-similarity one
        let weights = RecallWeights {
            vector_similarity: 0.0,
            recency: 1.0,
            relevance: 0.0,
            epistemic_tier: 0.0,
            relationship_proximity: 0.0,
            access_frequency: 0.0,
        };
        let e = RecallEngine::with_weights(weights);

        let old_similar = FactorScores {
            vector_similarity: 1.0,
            recency: 0.1,
            ..FactorScores::default()
        };
        let new_dissimilar = FactorScores {
            vector_similarity: 0.1,
            recency: 1.0,
            ..FactorScores::default()
        };

        assert!(e.compute_score(&new_dissimilar) > e.compute_score(&old_similar));
    }

    // --- Boundary conditions ---

    #[test]
    fn all_weights_zero_returns_zero() {
        let weights = RecallWeights {
            vector_similarity: 0.0,
            recency: 0.0,
            relevance: 0.0,
            epistemic_tier: 0.0,
            relationship_proximity: 0.0,
            access_frequency: 0.0,
        };
        let e = RecallEngine::with_weights(weights);
        let factors = FactorScores {
            vector_similarity: 1.0,
            recency: 1.0,
            relevance: 1.0,
            epistemic_tier: 1.0,
            relationship_proximity: 1.0,
            access_frequency: 1.0,
        };
        assert!((e.compute_score(&factors)).abs() < f64::EPSILON);
    }

    #[test]
    fn vector_similarity_negative_clamps() {
        let e = engine();
        assert!((e.score_vector_similarity(-0.5)).abs() < 1.01);
        assert!(e.score_vector_similarity(-0.5) >= 0.0);
    }

    #[test]
    fn vector_similarity_over_two_clamps() {
        let e = engine();
        assert!((e.score_vector_similarity(3.0)).abs() < f64::EPSILON);
    }

    #[test]
    fn recency_negative_age_returns_one() {
        let e = engine();
        assert!((e.score_recency(-10.0) - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn recency_very_old_near_zero() {
        let e = engine();
        let score = e.score_recency(1_000_000.0);
        assert!(score >= 0.0);
        assert!(score < 0.001);
    }

    #[test]
    fn access_frequency_u64_max_no_panic() {
        let e = engine();
        let score = e.score_access_frequency(u64::MAX);
        assert!(score.is_finite());
        assert!(score > 0.0);
    }

    #[test]
    fn proximity_high_hops_near_zero() {
        let e = engine();
        let score = e.score_relationship_proximity(Some(100));
        assert!(score > 0.0);
        assert!(score < 0.001);
    }

    #[test]
    fn proximity_same_entity() {
        let e = engine();
        assert!((e.score_relationship_proximity(Some(0)) - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn rank_empty_vec() {
        let e = engine();
        let ranked = e.rank(vec![]);
        assert!(ranked.is_empty());
    }

    #[test]
    fn rank_single_element() {
        let e = engine();
        let single = vec![ScoredResult {
            content: "only".to_owned(),
            source_type: "fact".to_owned(),
            source_id: "f1".to_owned(),
            nous_id: "syn".to_owned(),
            factors: FactorScores {
                vector_similarity: 0.5,
                ..FactorScores::default()
            },
            score: 0.0,
        }];
        let ranked = e.rank(single);
        assert_eq!(ranked.len(), 1);
        assert!(ranked[0].score > 0.0);
    }

    #[test]
    fn recall_weights_serde_roundtrip() {
        let weights = RecallWeights::default();
        let json = serde_json::to_string(&weights).unwrap();
        let back: RecallWeights = serde_json::from_str(&json).unwrap();
        assert!((weights.vector_similarity - back.vector_similarity).abs() < f64::EPSILON);
        assert!((weights.total() - back.total()).abs() < f64::EPSILON);
    }

    #[test]
    fn single_weight_isolation() {
        let factors = FactorScores {
            vector_similarity: 0.0,
            recency: 0.0,
            relevance: 1.0,
            epistemic_tier: 0.0,
            relationship_proximity: 0.0,
            access_frequency: 0.0,
        };
        let e = engine();
        let score = e.compute_score(&factors);
        let expected = 0.15; // relevance weight
        assert!((score - expected).abs() < 0.01);
    }
}
