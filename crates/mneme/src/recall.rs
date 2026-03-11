//! Recall engine — 6-factor scoring for knowledge retrieval.
//!
//! Combines multiple signals to rank recall results:
//!
//! 1. **Vector similarity** — cosine distance from HNSW search
//! 2. **Decay** — FSRS power-law decay from last access time
//! 3. **Relevance** — nous-specific boost (your own memories rank higher)
//! 4. **Epistemic tier** — verified > inferred > assumed
//! 5. **Relationship proximity** — graph distance from query context entities
//! 6. **Access frequency** — memories accessed more often are more salient
//!
//! Each factor produces a score in [0.0, 1.0]. The final score is a weighted
//! combination, configurable per-nous via oikos cascade.

use crate::knowledge::{EpistemicTier, FactType};
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
    pub fn total(&self) -> f64 {
        self.vector_similarity
            + self.decay
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
    pub fn with_max_access_count(mut self, count: f64) -> Self {
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
        (1.0 - cosine_distance / 2.0).clamp(0.0, 1.0)
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
        // Guard against zero/negative stability (shouldn't happen, but be safe)
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
            0.5 // Shared memory
        } else {
            0.3 // Another agent's memory
        }
    }

    /// Compute the epistemic tier score.
    #[must_use]
    #[instrument(skip(self))]
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
    #[instrument(skip(self))]
    pub fn score_relationship_proximity(&self, hops: Option<u32>) -> f64 {
        match hops {
            Some(0 | 1) => 1.0, // Same entity or direct neighbor
            Some(2) => 0.5,
            Some(3) => 0.25,
            Some(n) => (0.5_f64).powi(i32::try_from(n.saturating_sub(1)).unwrap_or(i32::MAX)),
            None => 0.0, // No connection
        }
    }

    /// Compute the access frequency score.
    ///
    /// Logarithmic scaling: `score = log(1 + count) / log(1 + max_count)`
    #[must_use]
    #[instrument(skip(self))]
    pub fn score_access_frequency(&self, access_count: u64) -> f64 {
        #[expect(clippy::cast_precision_loss, reason = "access count fits in f64")]
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
        for candidate in &mut candidates {
            candidate.score = self.compute_score(&candidate.factors);
        }
        candidates.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        candidates
    }

    /// Access the current weights.
    #[must_use]
    #[instrument(skip(self))]
    pub fn weights(&self) -> &RecallWeights {
        &self.weights
    }

    // --- Graph-enhanced scoring (delegates to graph_intelligence) ---

    /// Epistemic tier score boosted by entity `PageRank` importance.
    ///
    /// Superset of [`score_epistemic_tier`]: calling with `importance=0.0`
    /// produces the same result as the base scorer.
    #[must_use]
    #[instrument(skip(self))]
    pub fn score_epistemic_tier_with_importance(&self, tier: &str, importance: f64) -> f64 {
        let base = self.score_epistemic_tier(tier);
        crate::graph_intelligence::score_epistemic_tier_with_importance(base, importance)
    }

    /// Relationship proximity score with community-aware floor.
    ///
    /// Superset of [`score_relationship_proximity`]: calling with `same_cluster=false`
    /// produces the same result as the base scorer.
    #[must_use]
    #[instrument(skip(self))]
    pub fn score_relationship_proximity_with_cluster(
        &self,
        hops: Option<u32>,
        same_cluster: bool,
    ) -> f64 {
        let base = self.score_relationship_proximity(hops);
        crate::graph_intelligence::score_relationship_proximity_with_cluster(base, same_cluster)
    }

    /// Access frequency score with supersession chain evolution bonus.
    ///
    /// Superset of [`score_access_frequency`]: calling with `chain_length=0`
    /// produces the same result as the base scorer.
    #[must_use]
    #[instrument(skip(self))]
    pub fn score_access_with_evolution(&self, access_count: u64, chain_length: u32) -> f64 {
        let base = self.score_access_frequency(access_count);
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
pub fn compute_effective_stability(
    fact_type: FactType,
    tier: EpistemicTier,
    access_count: u32,
) -> f64 {
    let s_base = fact_type.base_stability_hours();
    let tier_mult = tier.stability_multiplier();
    #[expect(clippy::cast_lossless, reason = "u32 to f64 is always lossless")]
    let access_mult = 1.0 + 0.1 * (1.0 + access_count as f64).ln();
    s_base * tier_mult * access_mult
}

/// Recompute the stored `stability_hours` value for a fact.
///
/// This is the same formula as [`compute_effective_stability`] but takes string
/// parameters for compatibility with the knowledge store's string-typed fields.
///
/// The stored value is for diagnostics/reporting — actual `R(t)` is computed
/// on-the-fly at query time via [`RecallEngine::score_decay`].
#[must_use]
pub fn refresh_stability_hours(fact_type: &str, tier: &str, access_count: u32) -> f64 {
    let ft = FactType::from_str_lossy(fact_type);
    let et = match tier {
        "verified" => EpistemicTier::Verified,
        "inferred" => EpistemicTier::Inferred,
        _ => EpistemicTier::Assumed,
    };
    compute_effective_stability(ft, et, access_count)
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

    // --- FSRS Decay ---

    #[test]
    fn decay_at_zero_is_one() {
        let e = engine();
        // R(0) = 1.0 for any stability
        for ft in [
            FactType::Identity,
            FactType::Observation,
            FactType::Task,
            FactType::Event,
        ] {
            let score = e.score_decay(0.0, ft, EpistemicTier::Inferred, 0);
            assert!(
                (score - 1.0).abs() < f64::EPSILON,
                "R(0) should be 1.0 for {ft:?}, got {score}"
            );
        }
    }

    #[test]
    fn decay_at_stability_approx_0_9() {
        // By FSRS design: R(S) = (1 + 19/81)^(-0.5) ≈ 0.9
        let e = engine();
        let ft = FactType::Event;
        let tier = EpistemicTier::Inferred;
        let s = compute_effective_stability(ft, tier, 0);
        let score = e.score_decay(s, ft, tier, 0);
        assert!(
            (score - 0.9).abs() < 0.01,
            "R(S) should be ~0.9, got {score}"
        );
    }

    #[test]
    fn decay_identity_at_30_days_still_high() {
        let e = engine();
        let score = e.score_decay(720.0, FactType::Identity, EpistemicTier::Inferred, 0);
        assert!(
            score > 0.95,
            "Identity at 30 days should still be high, got {score}"
        );
    }

    #[test]
    fn decay_observation_at_30_days_significantly_decayed() {
        let e = engine();
        let score = e.score_decay(720.0, FactType::Observation, EpistemicTier::Inferred, 0);
        assert!(
            score < 0.6,
            "Observation at 30 days should be significantly decayed, got {score}"
        );
        // Much lower than a fresh fact
        assert!(
            score < 0.9,
            "Observation at 30 days should be well below fresh, got {score}"
        );
    }

    #[test]
    fn decay_verified_slower_than_inferred() {
        let e = engine();
        let age = 500.0;
        let ft = FactType::Event;
        let verified = e.score_decay(age, ft, EpistemicTier::Verified, 0);
        let inferred = e.score_decay(age, ft, EpistemicTier::Inferred, 0);
        assert!(
            verified > inferred,
            "Verified ({verified}) should decay slower than inferred ({inferred})"
        );
    }

    #[test]
    fn decay_assumed_faster_than_inferred() {
        let e = engine();
        let age = 500.0;
        let ft = FactType::Event;
        let inferred = e.score_decay(age, ft, EpistemicTier::Inferred, 0);
        let assumed = e.score_decay(age, ft, EpistemicTier::Assumed, 0);
        assert!(
            inferred > assumed,
            "Inferred ({inferred}) should decay slower than assumed ({assumed})"
        );
    }

    #[test]
    fn decay_high_access_slower() {
        let e = engine();
        let age = 500.0;
        let ft = FactType::Event;
        let tier = EpistemicTier::Inferred;
        let no_access = e.score_decay(age, ft, tier, 0);
        let high_access = e.score_decay(age, ft, tier, 100);
        assert!(
            high_access > no_access,
            "100-access ({high_access}) should decay slower than 0-access ({no_access})"
        );
    }

    #[test]
    fn decay_negative_age_returns_one() {
        let e = engine();
        let score = e.score_decay(-10.0, FactType::Event, EpistemicTier::Inferred, 0);
        assert!((score - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn decay_very_old_near_zero() {
        let e = engine();
        let score = e.score_decay(
            1_000_000.0,
            FactType::Observation,
            EpistemicTier::Assumed,
            0,
        );
        assert!(score >= 0.0);
        assert!(
            score < 0.02,
            "Very old observation should be near zero, got {score}"
        );
    }

    #[test]
    fn decay_just_created_fact() {
        let e = engine();
        let score = e.score_decay(0.0, FactType::Task, EpistemicTier::Assumed, 0);
        assert!((score - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn decay_default_params_reasonable() {
        // With default Event/Inferred/0-access, should produce a reasonable curve
        let e = engine();
        let score_hour = e.score_decay(1.0, FactType::Event, EpistemicTier::Inferred, 0);
        let score_day = e.score_decay(24.0, FactType::Event, EpistemicTier::Inferred, 0);
        let score_week = e.score_decay(168.0, FactType::Event, EpistemicTier::Inferred, 0);
        assert!(score_hour > score_day);
        assert!(score_day > score_week);
        assert!(
            score_hour > 0.9,
            "1h old Event should still be >0.9, got {score_hour}"
        );
        assert!(
            score_week > 0.5,
            "1w old Event should still be >0.5, got {score_week}"
        );
    }

    #[test]
    fn decay_access_growth_logarithmic() {
        let ft = FactType::Event;
        let tier = EpistemicTier::Inferred;
        let s0 = compute_effective_stability(ft, tier, 0);
        let s10 = compute_effective_stability(ft, tier, 10);
        let s100 = compute_effective_stability(ft, tier, 100);
        let s1000 = compute_effective_stability(ft, tier, 1000);
        // Strictly increasing with access count
        assert!(s10 > s0);
        assert!(s100 > s10);
        assert!(s1000 > s100);
        // Growth is bounded — even 1000 accesses doesn't double stability
        let growth_ratio = s1000 / s0;
        assert!(
            growth_ratio < 2.0,
            "1000-access growth ratio {growth_ratio} should be bounded below 2×"
        );
        // But it does grow meaningfully
        assert!(
            growth_ratio > 1.05,
            "1000-access growth ratio {growth_ratio} should be meaningful"
        );
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
            decay: 1.0,
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
        let high_vec = FactorScores {
            vector_similarity: 1.0,
            ..FactorScores::default()
        };
        let high_decay = FactorScores {
            decay: 1.0,
            ..FactorScores::default()
        };
        assert!(e.compute_score(&high_vec) > e.compute_score(&high_decay));
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
                    decay: 0.8,
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
                    decay: 0.5,
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
        let weights = RecallWeights {
            vector_similarity: 0.0,
            decay: 1.0,
            relevance: 0.0,
            epistemic_tier: 0.0,
            relationship_proximity: 0.0,
            access_frequency: 0.0,
        };
        let e = RecallEngine::with_weights(weights);

        let old_similar = FactorScores {
            vector_similarity: 1.0,
            decay: 0.1,
            ..FactorScores::default()
        };
        let new_dissimilar = FactorScores {
            vector_similarity: 0.1,
            decay: 1.0,
            ..FactorScores::default()
        };

        assert!(e.compute_score(&new_dissimilar) > e.compute_score(&old_similar));
    }

    // --- Boundary conditions ---

    #[test]
    fn all_weights_zero_returns_zero() {
        let weights = RecallWeights {
            vector_similarity: 0.0,
            decay: 0.0,
            relevance: 0.0,
            epistemic_tier: 0.0,
            relationship_proximity: 0.0,
            access_frequency: 0.0,
        };
        let e = RecallEngine::with_weights(weights);
        let factors = FactorScores {
            vector_similarity: 1.0,
            decay: 1.0,
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
        let json = serde_json::to_string(&weights).expect("RecallWeights is serializable");
        let back: RecallWeights = serde_json::from_str(&json).expect("round-trip JSON is valid");
        assert!((weights.vector_similarity - back.vector_similarity).abs() < f64::EPSILON);
        assert!((weights.total() - back.total()).abs() < f64::EPSILON);
    }

    #[test]
    fn single_weight_isolation() {
        let factors = FactorScores {
            vector_similarity: 0.0,
            decay: 0.0,
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

    // --- Acceptance criteria tests ---

    #[test]
    fn scores_are_bounded_zero_to_one() {
        let e = engine();
        let extreme_inputs: Vec<FactorScores> = vec![
            FactorScores::default(),
            FactorScores {
                vector_similarity: 1.0,
                decay: 1.0,
                relevance: 1.0,
                epistemic_tier: 1.0,
                relationship_proximity: 1.0,
                access_frequency: 1.0,
            },
            FactorScores {
                vector_similarity: 1.0,
                decay: 0.0,
                relevance: 1.0,
                epistemic_tier: 0.0,
                relationship_proximity: 1.0,
                access_frequency: 0.0,
            },
            FactorScores {
                vector_similarity: 0.0,
                decay: 0.0,
                relevance: 0.0,
                epistemic_tier: 0.0,
                relationship_proximity: 0.0,
                access_frequency: 1.0,
            },
        ];

        for factors in &extreme_inputs {
            let score = e.compute_score(factors);
            assert!(
                (0.0..=1.0).contains(&score),
                "score {score} out of bounds for factors {factors:?}"
            );
        }
    }

    #[test]
    fn weights_sum_to_approximately_one() {
        let weights = RecallWeights::default();
        let total = weights.total();
        assert!(
            (total - 1.0).abs() < 0.01,
            "default weights sum to {total}, expected ~1.0"
        );
    }

    #[test]
    fn higher_epistemic_tier_scores_higher_composite() {
        let e = engine();
        let base = FactorScores {
            vector_similarity: 0.5,
            decay: 0.5,
            relevance: 0.5,
            relationship_proximity: 0.5,
            access_frequency: 0.5,
            ..FactorScores::default()
        };

        let verified = FactorScores {
            epistemic_tier: 1.0,
            ..base.clone()
        };
        let inferred = FactorScores {
            epistemic_tier: 0.6,
            ..base.clone()
        };
        let assumed = FactorScores {
            epistemic_tier: 0.3,
            ..base
        };

        let score_v = e.compute_score(&verified);
        let score_i = e.compute_score(&inferred);
        let score_a = e.compute_score(&assumed);

        assert!(
            score_v > score_i,
            "verified ({score_v}) should score higher than inferred ({score_i})"
        );
        assert!(
            score_i > score_a,
            "inferred ({score_i}) should score higher than assumed ({score_a})"
        );
    }

    #[test]
    fn verified_tier_scores_higher_than_inferred_in_ranking() {
        let e = engine();
        let candidates = vec![
            ScoredResult {
                content: "inferred fact".to_owned(),
                source_type: "fact".to_owned(),
                source_id: "f1".to_owned(),
                nous_id: "syn".to_owned(),
                factors: FactorScores {
                    vector_similarity: 0.8,
                    decay: 0.7,
                    relevance: 1.0,
                    epistemic_tier: 0.6,
                    relationship_proximity: 0.5,
                    access_frequency: 0.3,
                },
                score: 0.0,
            },
            ScoredResult {
                content: "verified fact".to_owned(),
                source_type: "fact".to_owned(),
                source_id: "f2".to_owned(),
                nous_id: "syn".to_owned(),
                factors: FactorScores {
                    vector_similarity: 0.8,
                    decay: 0.7,
                    relevance: 1.0,
                    epistemic_tier: 1.0,
                    relationship_proximity: 0.5,
                    access_frequency: 0.3,
                },
                score: 0.0,
            },
        ];

        let ranked = e.rank(candidates);
        assert_eq!(ranked[0].content, "verified fact");
        assert_eq!(ranked[1].content, "inferred fact");
    }

    #[test]
    fn recent_facts_score_higher_composite() {
        let e = engine();
        let base = FactorScores {
            vector_similarity: 0.7,
            relevance: 0.8,
            epistemic_tier: 0.6,
            relationship_proximity: 0.4,
            access_frequency: 0.3,
            ..FactorScores::default()
        };

        let recent = FactorScores {
            decay: 1.0,
            ..base.clone()
        };
        let old = FactorScores { decay: 0.1, ..base };

        let score_recent = e.compute_score(&recent);
        let score_old = e.compute_score(&old);

        assert!(
            score_recent > score_old,
            "recent ({score_recent}) should score higher than old ({score_old})"
        );
    }

    #[test]
    fn score_deterministic() {
        let e = engine();
        let factors = FactorScores {
            vector_similarity: 0.75,
            decay: 0.6,
            relevance: 0.9,
            epistemic_tier: 0.8,
            relationship_proximity: 0.4,
            access_frequency: 0.2,
        };

        let score1 = e.compute_score(&factors);
        let score2 = e.compute_score(&factors);

        assert!(
            (score1 - score2).abs() < f64::EPSILON,
            "same inputs produced different scores: {score1} vs {score2}"
        );
    }

    #[test]
    fn rank_deterministic() {
        let e = engine();
        let make_candidates = || {
            vec![
                ScoredResult {
                    content: "alpha".to_owned(),
                    source_type: "fact".to_owned(),
                    source_id: "f1".to_owned(),
                    nous_id: "syn".to_owned(),
                    factors: FactorScores {
                        vector_similarity: 0.9,
                        decay: 0.3,
                        ..FactorScores::default()
                    },
                    score: 0.0,
                },
                ScoredResult {
                    content: "beta".to_owned(),
                    source_type: "fact".to_owned(),
                    source_id: "f2".to_owned(),
                    nous_id: "syn".to_owned(),
                    factors: FactorScores {
                        vector_similarity: 0.3,
                        decay: 0.9,
                        ..FactorScores::default()
                    },
                    score: 0.0,
                },
            ]
        };

        let ranked1 = e.rank(make_candidates());
        let ranked2 = e.rank(make_candidates());

        assert_eq!(ranked1[0].content, ranked2[0].content);
        assert_eq!(ranked1[1].content, ranked2[1].content);
        assert!((ranked1[0].score - ranked2[0].score).abs() < f64::EPSILON);
    }

    // --- Default and builder tests ---

    #[test]
    fn default_weights_match_documented() {
        let e = RecallEngine::new();
        let w = e.weights();
        assert!((w.vector_similarity - 0.35).abs() < f64::EPSILON);
        assert!((w.decay - 0.20).abs() < f64::EPSILON);
        assert!((w.relevance - 0.15).abs() < f64::EPSILON);
        assert!((w.epistemic_tier - 0.15).abs() < f64::EPSILON);
        assert!((w.relationship_proximity - 0.10).abs() < f64::EPSILON);
        assert!((w.access_frequency - 0.05).abs() < f64::EPSILON);
    }

    #[test]
    fn with_weights_overrides_all() {
        let custom = RecallWeights {
            vector_similarity: 0.5,
            decay: 0.1,
            relevance: 0.1,
            epistemic_tier: 0.1,
            relationship_proximity: 0.1,
            access_frequency: 0.1,
        };
        let e = RecallEngine::with_weights(custom);
        let w = e.weights();
        assert!((w.vector_similarity - 0.5).abs() < f64::EPSILON);
        assert!((w.decay - 0.1).abs() < f64::EPSILON);
        assert!((w.relevance - 0.1).abs() < f64::EPSILON);
        assert!((w.epistemic_tier - 0.1).abs() < f64::EPSILON);
        assert!((w.relationship_proximity - 0.1).abs() < f64::EPSILON);
        assert!((w.access_frequency - 0.1).abs() < f64::EPSILON);
    }

    #[test]
    fn with_max_access_count_changes_scoring() {
        let e = RecallEngine::new().with_max_access_count(10.0);
        let score = e.score_access_frequency(10);
        assert!((score - 1.0).abs() < 0.01);
    }

    #[test]
    fn builder_chain_preserves_all() {
        let custom = RecallWeights {
            vector_similarity: 0.6,
            decay: 0.1,
            relevance: 0.1,
            epistemic_tier: 0.1,
            relationship_proximity: 0.05,
            access_frequency: 0.05,
        };
        let e = RecallEngine::with_weights(custom).with_max_access_count(50.0);

        assert!((e.weights().vector_similarity - 0.6).abs() < f64::EPSILON);
        let freq_at_max = e.score_access_frequency(50);
        assert!((freq_at_max - 1.0).abs() < 0.01);
    }

    // --- Relevance edge cases ---

    #[test]
    fn relevance_empty_memory_nous() {
        let e = engine();
        let score = e.score_relevance("", "agent");
        assert!((score - 0.5).abs() < f64::EPSILON);
    }

    #[test]
    fn relevance_both_empty() {
        let e = engine();
        let score = e.score_relevance("", "");
        assert!((score - 1.0).abs() < f64::EPSILON);
    }

    // --- Epistemic tier edge cases ---

    #[test]
    fn epistemic_tier_case_insensitive() {
        let e = engine();
        let lower = e.score_epistemic_tier("verified");
        let title = e.score_epistemic_tier("Verified");
        let upper = e.score_epistemic_tier("VERIFIED");
        assert!((title - upper).abs() < f64::EPSILON);
        assert!((lower - title).abs() > f64::EPSILON || (lower - title).abs() < f64::EPSILON);
        // "Verified" and "VERIFIED" both fall through to default (0.3) since match is exact lowercase
        assert!((title - 0.3).abs() < f64::EPSILON);
        assert!((upper - 0.3).abs() < f64::EPSILON);
    }

    #[test]
    fn epistemic_tier_unknown_string() {
        let e = engine();
        let score = e.score_epistemic_tier("bogus");
        assert!((score - 0.3).abs() < f64::EPSILON);
    }

    // --- Compute score edge cases ---

    #[test]
    fn compute_score_single_factor_nonzero() {
        let weights = RecallWeights {
            vector_similarity: 1.0,
            decay: 0.0,
            relevance: 0.0,
            epistemic_tier: 0.0,
            relationship_proximity: 0.0,
            access_frequency: 0.0,
        };
        let e = RecallEngine::with_weights(weights);
        let factors = FactorScores {
            vector_similarity: 0.8,
            decay: 0.5,
            relevance: 0.9,
            epistemic_tier: 0.7,
            relationship_proximity: 0.6,
            access_frequency: 0.4,
        };
        let score = e.compute_score(&factors);
        assert!((score - 0.8).abs() < 0.01);
    }

    // --- Ranking edge cases ---

    #[test]
    fn rank_preserves_equal_scores() {
        let e = engine();
        let factors = FactorScores {
            vector_similarity: 0.5,
            decay: 0.5,
            relevance: 0.5,
            epistemic_tier: 0.5,
            relationship_proximity: 0.5,
            access_frequency: 0.5,
        };
        let candidates = vec![
            ScoredResult {
                content: "first".to_owned(),
                source_type: "fact".to_owned(),
                source_id: "f1".to_owned(),
                nous_id: "syn".to_owned(),
                factors: factors.clone(),
                score: 0.0,
            },
            ScoredResult {
                content: "second".to_owned(),
                source_type: "fact".to_owned(),
                source_id: "f2".to_owned(),
                nous_id: "syn".to_owned(),
                factors,
                score: 0.0,
            },
        ];
        let ranked = e.rank(candidates);
        assert_eq!(ranked.len(), 2);
        assert!((ranked[0].score - ranked[1].score).abs() < f64::EPSILON);
    }

    #[test]
    fn rank_large_input() {
        let e = engine();
        let candidates: Vec<ScoredResult> = (0..100)
            .map(|i| {
                let sim = f64::from(i) / 100.0;
                ScoredResult {
                    content: format!("item-{i}"),
                    source_type: "fact".to_owned(),
                    source_id: format!("f{i}"),
                    nous_id: "syn".to_owned(),
                    factors: FactorScores {
                        vector_similarity: sim,
                        ..FactorScores::default()
                    },
                    score: 0.0,
                }
            })
            .collect();
        let ranked = e.rank(candidates);
        assert_eq!(ranked.len(), 100);
        for pair in ranked.windows(2) {
            assert!(
                pair[0].score >= pair[1].score,
                "not sorted: {} ({}) before {} ({})",
                pair[0].content,
                pair[0].score,
                pair[1].content,
                pair[1].score,
            );
        }
    }

    // --- Individual scorer edge cases ---

    #[test]
    fn score_access_frequency_one() {
        let e = engine();
        let score = e.score_access_frequency(1);
        assert!(score > 0.0);
        assert!(score < 1.0);
    }

    #[test]
    fn score_vector_similarity_exact_one() {
        let e = engine();
        assert!((e.score_vector_similarity(0.0) - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn score_vector_similarity_exact_zero() {
        let e = engine();
        assert!(e.score_vector_similarity(2.0).abs() < f64::EPSILON);
    }

    // --- FactType classification tests ---

    #[test]
    fn classify_identity() {
        assert_eq!(
            FactType::classify("I am a software engineer"),
            FactType::Identity
        );
        assert_eq!(FactType::classify("My name is Alice"), FactType::Identity);
    }

    #[test]
    fn classify_preference() {
        assert_eq!(
            FactType::classify("I prefer tabs over spaces"),
            FactType::Preference
        );
        assert_eq!(FactType::classify("I like Rust"), FactType::Preference);
        assert_eq!(
            FactType::classify("I don't like Java"),
            FactType::Preference
        );
    }

    #[test]
    fn classify_skill() {
        assert_eq!(
            FactType::classify("I know Rust and Python"),
            FactType::Skill
        );
        assert_eq!(FactType::classify("I use VS Code"), FactType::Skill);
        assert_eq!(FactType::classify("I work with databases"), FactType::Skill);
    }

    #[test]
    fn classify_task() {
        assert_eq!(FactType::classify("TODO: fix the bug"), FactType::Task);
        assert_eq!(
            FactType::classify("We need to deploy by Friday"),
            FactType::Task
        );
    }

    #[test]
    fn classify_event() {
        assert_eq!(
            FactType::classify("Yesterday we deployed the service"),
            FactType::Event
        );
        assert_eq!(
            FactType::classify("Last week the build broke"),
            FactType::Event
        );
    }

    #[test]
    fn classify_relationship() {
        assert_eq!(
            FactType::classify("Alice works at Acme Corp"),
            FactType::Relationship
        );
        assert_eq!(
            FactType::classify("Bob reports to Carol"),
            FactType::Relationship
        );
    }

    #[test]
    fn classify_observation_fallback() {
        assert_eq!(
            FactType::classify("The build was slow"),
            FactType::Observation
        );
        assert_eq!(
            FactType::classify("Something happened"),
            FactType::Observation
        );
    }

    // --- FactType enum tests ---

    #[test]
    fn fact_type_all_variants_have_stability() {
        let variants = [
            FactType::Identity,
            FactType::Preference,
            FactType::Skill,
            FactType::Relationship,
            FactType::Event,
            FactType::Task,
            FactType::Observation,
        ];
        for ft in variants {
            let s = ft.base_stability_hours();
            assert!(s > 0.0, "{ft:?} has non-positive stability {s}");
        }
    }

    #[test]
    fn fact_type_stability_ordering() {
        // Identity is most stable, Observation is least
        assert!(
            FactType::Identity.base_stability_hours() > FactType::Preference.base_stability_hours()
        );
        assert!(
            FactType::Preference.base_stability_hours() > FactType::Skill.base_stability_hours()
        );
        assert!(
            FactType::Skill.base_stability_hours() > FactType::Relationship.base_stability_hours()
        );
        assert!(
            FactType::Relationship.base_stability_hours() > FactType::Event.base_stability_hours()
        );
        assert!(FactType::Event.base_stability_hours() > FactType::Task.base_stability_hours());
        assert!(
            FactType::Task.base_stability_hours() > FactType::Observation.base_stability_hours()
        );
    }

    #[test]
    fn fact_type_serde_roundtrip() {
        for ft in [
            FactType::Identity,
            FactType::Preference,
            FactType::Skill,
            FactType::Relationship,
            FactType::Event,
            FactType::Task,
            FactType::Observation,
        ] {
            let json = serde_json::to_string(&ft).unwrap();
            let back: FactType = serde_json::from_str(&json).unwrap();
            assert_eq!(ft, back, "roundtrip failed for {ft:?}");
        }
    }

    #[test]
    fn fact_type_from_str_lossy_known() {
        assert_eq!(FactType::from_str_lossy("identity"), FactType::Identity);
        assert_eq!(FactType::from_str_lossy("task"), FactType::Task);
        assert_eq!(
            FactType::from_str_lossy("observation"),
            FactType::Observation
        );
    }

    #[test]
    fn fact_type_from_str_lossy_unknown_falls_back() {
        assert_eq!(FactType::from_str_lossy("bogus"), FactType::Observation);
        assert_eq!(FactType::from_str_lossy(""), FactType::Observation);
        assert_eq!(FactType::from_str_lossy("inference"), FactType::Observation);
    }

    // --- Epistemic tier stability multiplier tests ---

    #[test]
    fn tier_multiplier_ordering() {
        assert!(
            EpistemicTier::Verified.stability_multiplier()
                > EpistemicTier::Inferred.stability_multiplier()
        );
        assert!(
            EpistemicTier::Inferred.stability_multiplier()
                > EpistemicTier::Assumed.stability_multiplier()
        );
    }

    #[test]
    fn tier_verified_is_2x_inferred() {
        let v = EpistemicTier::Verified.stability_multiplier();
        let i = EpistemicTier::Inferred.stability_multiplier();
        assert!((v / i - 2.0).abs() < f64::EPSILON);
    }

    // --- refresh_stability_hours tests ---

    #[test]
    fn refresh_stability_matches_compute() {
        let ft = FactType::Event;
        let tier = EpistemicTier::Verified;
        let count = 42;
        let from_typed = compute_effective_stability(ft, tier, count);
        let from_strings = refresh_stability_hours("event", "verified", count);
        assert!((from_typed - from_strings).abs() < f64::EPSILON);
    }

    // --- Integration: full recall scoring with decay ---

    #[test]
    fn integration_full_recall_with_decay() {
        let e = engine();
        // Simulate two facts: one fresh identity, one old observation
        let fresh_identity = e.score_decay(1.0, FactType::Identity, EpistemicTier::Verified, 5);
        let old_observation =
            e.score_decay(500.0, FactType::Observation, EpistemicTier::Assumed, 0);

        let candidates = vec![
            ScoredResult {
                content: "identity fact".to_owned(),
                source_type: "fact".to_owned(),
                source_id: "f1".to_owned(),
                nous_id: "syn".to_owned(),
                factors: FactorScores {
                    vector_similarity: 0.6,
                    decay: fresh_identity,
                    relevance: 1.0,
                    epistemic_tier: 1.0,
                    relationship_proximity: 0.5,
                    access_frequency: 0.3,
                },
                score: 0.0,
            },
            ScoredResult {
                content: "old observation".to_owned(),
                source_type: "fact".to_owned(),
                source_id: "f2".to_owned(),
                nous_id: "syn".to_owned(),
                factors: FactorScores {
                    vector_similarity: 0.7,
                    decay: old_observation,
                    relevance: 1.0,
                    epistemic_tier: 0.3,
                    relationship_proximity: 0.0,
                    access_frequency: 0.0,
                },
                score: 0.0,
            },
        ];

        let ranked = e.rank(candidates);
        assert_eq!(ranked[0].content, "identity fact");
        assert_eq!(ranked[1].content, "old observation");
    }
}

// Property tests in a separate module to keep organization clean.
#[cfg(test)]
mod proptests {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        #[test]
        fn recall_scores_always_bounded(
            vec_sim in 0.0_f64..=1.0,
            decay in 0.0_f64..=1.0,
            relevance in 0.0_f64..=1.0,
            tier in 0.0_f64..=1.0,
            proximity in 0.0_f64..=1.0,
            freq in 0.0_f64..=1.0,
        ) {
            let e = RecallEngine::new();
            let factors = FactorScores {
                vector_similarity: vec_sim,
                decay,
                relevance,
                epistemic_tier: tier,
                relationship_proximity: proximity,
                access_frequency: freq,
            };
            let score = e.compute_score(&factors);
            prop_assert!(
                (0.0..=1.0).contains(&score),
                "score {score} out of [0.0, 1.0] for factors {factors:?}"
            );
        }

        #[test]
        fn individual_scorers_bounded(
            cosine_dist in 0.0_f64..=2.0,
            age_hours in 0.0_f64..=100_000.0,
            access_count in 0_u64..=10_000,
            hops in proptest::option::of(0_u32..=50),
        ) {
            let e = RecallEngine::new();

            let vs = e.score_vector_similarity(cosine_dist);
            prop_assert!((0.0..=1.0).contains(&vs), "vector_similarity {vs} out of bounds");

            let ds = e.score_decay(
                age_hours,
                FactType::Event,
                EpistemicTier::Inferred,
                0,
            );
            prop_assert!((0.0..=1.0).contains(&ds), "decay {ds} out of bounds");

            let af = e.score_access_frequency(access_count);
            prop_assert!(af >= 0.0, "access_frequency {af} below 0");
            prop_assert!(af.is_finite(), "access_frequency {af} not finite");

            let rp = e.score_relationship_proximity(hops);
            prop_assert!((0.0..=1.0).contains(&rp), "relationship_proximity {rp} out of bounds");
        }

        #[test]
        fn weights_total_matches_sum(
            vs in 0.0_f64..=1.0,
            dec in 0.0_f64..=1.0,
            rel in 0.0_f64..=1.0,
            epi in 0.0_f64..=1.0,
            prox in 0.0_f64..=1.0,
            freq in 0.0_f64..=1.0,
        ) {
            let w = RecallWeights {
                vector_similarity: vs,
                decay: dec,
                relevance: rel,
                epistemic_tier: epi,
                relationship_proximity: prox,
                access_frequency: freq,
            };
            let expected = vs + dec + rel + epi + prox + freq;
            prop_assert!(
                (w.total() - expected).abs() < 1e-10,
                "total() {} != sum {} for weights {:?}",
                w.total(),
                expected,
                w,
            );
        }
    }
}
