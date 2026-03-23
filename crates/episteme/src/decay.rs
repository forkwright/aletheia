//! Multi-factor temporal decay with lifecycle stages and graduated pruning.
//!
//! Extends the base FSRS decay in [`recall`](crate::recall) with:
//! - **Reinforcement signals**: explicit reinforcement boosts slow decay
//! - **Cross-agent access patterns**: facts accessed by multiple agents decay slower
//! - **Knowledge lifecycle stages**: active → fading → dormant → archived
//! - **Graduated pruning**: stage transitions instead of immediate deletion

use serde::{Deserialize, Serialize};

use crate::knowledge::{EpistemicTier, FactType, KnowledgeStage, StageTransition};

/// Reinforcement boost per explicit reinforcement event.
const REINFORCEMENT_BOOST: f64 = 0.02;

/// Maximum cumulative reinforcement bonus (caps at 50 reinforcements).
const MAX_REINFORCEMENT_BONUS: f64 = 1.0;

/// Multiplier bonus per distinct agent that accessed a fact.
const CROSS_AGENT_BONUS_PER_AGENT: f64 = 0.15;

/// Maximum cross-agent multiplier (caps at 5 distinct agents → 1.75×).
const MAX_CROSS_AGENT_MULTIPLIER: f64 = 1.75;

/// Configuration for multi-factor decay computation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct DecayConfig {
    /// Weight for recency factor (hours since last access). Default: 0.35
    pub recency: f64,
    /// Weight for access frequency factor. Default: 0.25
    pub frequency: f64,
    /// Weight for confidence/tier factor. Default: 0.20
    pub confidence: f64,
    /// Weight for reinforcement signal factor. Default: 0.20
    pub reinforcement: f64,
}

impl Default for DecayConfig {
    fn default() -> Self {
        Self {
            recency: 0.35,
            frequency: 0.25,
            confidence: 0.20,
            reinforcement: 0.20,
        }
    }
}

impl DecayConfig {
    fn total_weight(&self) -> f64 {
        self.recency + self.frequency + self.confidence + self.reinforcement
    }
}

/// Input factors for computing the multi-factor decay score of a single fact.
#[derive(Debug, Clone)]
pub(crate) struct DecayFactors {
    /// Hours since the fact was last accessed (or created if never accessed).
    pub age_hours: f64,
    /// Classified fact type.
    pub fact_type: FactType,
    /// Epistemic confidence tier.
    pub tier: EpistemicTier,
    /// Total access count across all agents.
    pub access_count: u32,
    /// Number of explicit reinforcement events.
    pub reinforcement_count: u32,
    /// Number of distinct agents that accessed this fact.
    pub distinct_agent_count: u32,
    /// Domain volatility score in [0.0, 1.0] (from succession module).
    pub volatility: f64,
}

/// Result of multi-factor decay computation.
#[derive(Debug, Clone)]
pub(crate) struct DecayResult {
    /// Combined decay score in [0.0, 1.0]. Higher = better retained.
    pub score: f64,
    /// Computed lifecycle stage based on the decay score.
    pub stage: KnowledgeStage,
}

/// Compute the multi-factor decay score for a single fact.
///
/// Combines four weighted factors:
/// 1. **Recency**: FSRS power-law decay from last access
/// 2. **Frequency**: logarithmic access count normalization
/// 3. **Confidence**: epistemic tier and base confidence
/// 4. **Reinforcement**: explicit reinforcement events
///
/// The combined score is then multiplied by a cross-agent bonus
/// for facts accessed by multiple distinct agents.
#[must_use]
pub(crate) fn compute_decay(config: &DecayConfig, factors: &DecayFactors) -> DecayResult {
    let effective_stability = compute_effective_stability_with_reinforcement(
        factors.fact_type,
        factors.tier,
        factors.access_count,
        factors.reinforcement_count,
        factors.volatility,
    );

    let recency = score_recency(factors.age_hours, effective_stability);
    let frequency = score_frequency(factors.access_count);
    let confidence = score_confidence(factors.tier);
    let reinforcement = score_reinforcement(factors.reinforcement_count);

    let total_weight = config.total_weight();
    let weighted = if total_weight > 0.0 {
        (recency * config.recency
            + frequency * config.frequency
            + confidence * config.confidence
            + reinforcement * config.reinforcement)
            / total_weight
    } else {
        recency
    };

    let cross_agent_mult = cross_agent_multiplier(factors.distinct_agent_count);
    let score = (weighted * cross_agent_mult).clamp(0.0, 1.0);

    DecayResult {
        score,
        stage: KnowledgeStage::from_decay_score(score),
    }
}

/// FSRS power-law recency score.
///
/// `R(t) = (1 + 19/81 × t/S)^(-0.5)`
#[must_use]
fn score_recency(age_hours: f64, effective_stability: f64) -> f64 {
    if age_hours <= 0.0 {
        return 1.0;
    }
    if effective_stability <= 0.0 {
        return 0.0;
    }
    (1.0 + (19.0 / 81.0) * age_hours / effective_stability).powf(-0.5)
}

/// Logarithmic access frequency score.
///
/// `score = ln(1 + count) / ln(1 + 100)`
///
/// Uses a fixed normalization constant of 100 for consistency across
/// different knowledge bases.
#[must_use]
fn score_frequency(access_count: u32) -> f64 {
    let count = f64::from(access_count);
    let max_norm = (1.0_f64 + 100.0).ln();
    ((1.0 + count).ln() / max_norm).clamp(0.0, 1.0)
}

/// Confidence score from epistemic tier.
#[must_use]
fn score_confidence(tier: EpistemicTier) -> f64 {
    match tier {
        EpistemicTier::Verified => 1.0,
        EpistemicTier::Inferred => 0.6,
        // WHY: Assumed and any future unknown tiers get the lowest score.
        _ => 0.3,
    }
}

/// Reinforcement signal score.
///
/// Each reinforcement event adds a fixed boost, capped at `MAX_REINFORCEMENT_BONUS`.
#[must_use]
fn score_reinforcement(reinforcement_count: u32) -> f64 {
    let bonus = f64::from(reinforcement_count) * REINFORCEMENT_BOOST;
    bonus.min(MAX_REINFORCEMENT_BONUS)
}

/// Cross-agent access multiplier.
///
/// Facts accessed by multiple distinct agents are considered more universally
/// relevant and decay slower. Each additional agent beyond the first adds
/// a bonus multiplier.
#[must_use]
pub(crate) fn cross_agent_multiplier(distinct_agent_count: u32) -> f64 {
    if distinct_agent_count <= 1 {
        return 1.0;
    }
    let bonus = f64::from(distinct_agent_count - 1) * CROSS_AGENT_BONUS_PER_AGENT;
    (1.0 + bonus).min(MAX_CROSS_AGENT_MULTIPLIER)
}

/// Compute effective stability incorporating reinforcement signals.
///
/// Extends [`crate::recall::compute_effective_stability`] with:
/// - Reinforcement bonus: `1 + reinforcement_count × REINFORCEMENT_BOOST`
/// - Volatility adjustment from succession module
#[must_use]
pub(crate) fn compute_effective_stability_with_reinforcement(
    fact_type: FactType,
    tier: EpistemicTier,
    access_count: u32,
    reinforcement_count: u32,
    volatility: f64,
) -> f64 {
    let base = crate::recall::compute_effective_stability(fact_type, tier, access_count);
    let reinforcement_mult =
        1.0 + (f64::from(reinforcement_count) * REINFORCEMENT_BOOST).min(MAX_REINFORCEMENT_BONUS);
    let volatility_mult = crate::succession::volatility_multiplier(volatility);
    base * reinforcement_mult * volatility_mult
}

/// Evaluate lifecycle stage transitions for a batch of facts.
///
/// Compares each fact's current stage against its computed stage from
/// the decay score. Returns transitions that represent stage changes
/// (graduated pruning).
#[must_use]
pub(crate) fn evaluate_transitions(
    facts: &[(crate::id::FactId, KnowledgeStage, f64)],
) -> Vec<StageTransition> {
    let now = jiff::Timestamp::now();
    let mut transitions = Vec::new();

    for (fact_id, current_stage, decay_score) in facts {
        let new_stage = KnowledgeStage::from_decay_score(*decay_score);
        if new_stage != *current_stage {
            transitions.push(StageTransition {
                fact_id: fact_id.clone(),
                from: *current_stage,
                to: new_stage,
                decay_score: *decay_score,
                transitioned_at: now,
            });
        }
    }

    transitions
}

/// Identify facts eligible for graduated pruning.
///
/// Returns fact IDs in the `Archived` stage that have remained there
/// for at least `min_archived_hours` hours. Only archived facts
/// may be permanently removed.
#[must_use]
pub(crate) fn pruning_candidates(
    archived_facts: &[(crate::id::FactId, f64, f64)],
    min_archived_hours: f64,
) -> Vec<crate::id::FactId> {
    archived_facts
        .iter()
        .filter(|(_, decay_score, hours_in_archived)| {
            KnowledgeStage::from_decay_score(*decay_score).is_prunable()
                && *hours_in_archived >= min_archived_hours
        })
        .map(|(id, _, _)| id.clone())
        .collect()
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
#[expect(
    clippy::indexing_slicing,
    reason = "test code with known-valid indices"
)]
mod tests {
    use super::*;

    fn default_factors() -> DecayFactors {
        DecayFactors {
            age_hours: 0.0,
            fact_type: FactType::Observation,
            tier: EpistemicTier::Inferred,
            access_count: 0,
            reinforcement_count: 0,
            distinct_agent_count: 1,
            volatility: 0.5,
        }
    }

    #[test]
    fn fresh_fact_has_high_decay_score() {
        let result = compute_decay(&DecayConfig::default(), &default_factors());
        // WHY: 0.47 — recency is perfect but frequency/reinforcement are zero
        assert!(
            result.score > 0.4,
            "fresh fact should have moderate-high decay score, got {}",
            result.score
        );
        assert_eq!(
            result.stage,
            KnowledgeStage::Fading,
            "fresh unaccessed fact should be Fading"
        );
    }

    #[test]
    fn old_untouched_fact_decays_to_low_score() {
        let factors = DecayFactors {
            age_hours: 5000.0,
            ..default_factors()
        };
        let result = compute_decay(&DecayConfig::default(), &factors);
        assert!(
            result.score < 0.5,
            "old fact should have low decay score, got {}",
            result.score
        );
    }

    #[test]
    fn reinforcement_slows_decay() {
        let base = DecayFactors {
            age_hours: 500.0,
            ..default_factors()
        };
        let reinforced = DecayFactors {
            reinforcement_count: 20,
            ..base.clone()
        };
        let base_result = compute_decay(&DecayConfig::default(), &base);
        let reinforced_result = compute_decay(&DecayConfig::default(), &reinforced);
        assert!(
            reinforced_result.score > base_result.score,
            "reinforced fact should decay slower: {} vs {}",
            reinforced_result.score,
            base_result.score
        );
    }

    #[test]
    fn cross_agent_access_slows_decay() {
        let single = DecayFactors {
            age_hours: 500.0,
            ..default_factors()
        };
        let multi = DecayFactors {
            distinct_agent_count: 4,
            ..single.clone()
        };
        let single_result = compute_decay(&DecayConfig::default(), &single);
        let multi_result = compute_decay(&DecayConfig::default(), &multi);
        assert!(
            multi_result.score > single_result.score,
            "multi-agent access should slow decay: {} vs {}",
            multi_result.score,
            single_result.score
        );
    }

    #[test]
    fn cross_agent_multiplier_bounds() {
        assert!(
            (cross_agent_multiplier(0) - 1.0).abs() < f64::EPSILON,
            "zero agents should give 1.0 multiplier"
        );
        assert!(
            (cross_agent_multiplier(1) - 1.0).abs() < f64::EPSILON,
            "single agent should give 1.0 multiplier"
        );
        let two = cross_agent_multiplier(2);
        assert!(
            (two - 1.15).abs() < f64::EPSILON,
            "two agents should give 1.15, got {two}"
        );
        let capped = cross_agent_multiplier(100);
        assert!(
            (capped - MAX_CROSS_AGENT_MULTIPLIER).abs() < f64::EPSILON,
            "should cap at {MAX_CROSS_AGENT_MULTIPLIER}, got {capped}"
        );
    }

    #[test]
    fn lifecycle_stage_from_decay_score() {
        assert_eq!(
            KnowledgeStage::from_decay_score(1.0),
            KnowledgeStage::Active
        );
        assert_eq!(
            KnowledgeStage::from_decay_score(0.7),
            KnowledgeStage::Active
        );
        assert_eq!(
            KnowledgeStage::from_decay_score(0.69),
            KnowledgeStage::Fading
        );
        assert_eq!(
            KnowledgeStage::from_decay_score(0.3),
            KnowledgeStage::Fading
        );
        assert_eq!(
            KnowledgeStage::from_decay_score(0.29),
            KnowledgeStage::Dormant
        );
        assert_eq!(
            KnowledgeStage::from_decay_score(0.1),
            KnowledgeStage::Dormant
        );
        assert_eq!(
            KnowledgeStage::from_decay_score(0.09),
            KnowledgeStage::Archived
        );
        assert_eq!(
            KnowledgeStage::from_decay_score(0.0),
            KnowledgeStage::Archived
        );
    }

    #[test]
    fn stage_pruning_eligibility() {
        assert!(
            !KnowledgeStage::Active.is_prunable(),
            "Active should not be prunable"
        );
        assert!(
            !KnowledgeStage::Fading.is_prunable(),
            "Fading should not be prunable"
        );
        assert!(
            !KnowledgeStage::Dormant.is_prunable(),
            "Dormant should not be prunable"
        );
        assert!(
            KnowledgeStage::Archived.is_prunable(),
            "Archived should be prunable"
        );
    }

    #[test]
    fn stage_default_recall_inclusion() {
        assert!(
            KnowledgeStage::Active.in_default_recall(),
            "Active should be in default recall"
        );
        assert!(
            KnowledgeStage::Fading.in_default_recall(),
            "Fading should be in default recall"
        );
        assert!(
            !KnowledgeStage::Dormant.in_default_recall(),
            "Dormant should not be in default recall"
        );
        assert!(
            !KnowledgeStage::Archived.in_default_recall(),
            "Archived should not be in default recall"
        );
    }

    #[test]
    fn evaluate_transitions_detects_stage_change() {
        let fact_id = crate::id::FactId::new("f-1").expect("valid test id");
        let facts = vec![(fact_id.clone(), KnowledgeStage::Active, 0.2)];
        let transitions = evaluate_transitions(&facts);
        assert_eq!(transitions.len(), 1, "should detect one transition");
        assert_eq!(transitions[0].from, KnowledgeStage::Active);
        assert_eq!(transitions[0].to, KnowledgeStage::Dormant);
    }

    #[test]
    fn evaluate_transitions_no_change() {
        let fact_id = crate::id::FactId::new("f-1").expect("valid test id");
        let facts = vec![(fact_id, KnowledgeStage::Active, 0.9)];
        let transitions = evaluate_transitions(&facts);
        assert!(transitions.is_empty(), "should detect no transitions");
    }

    #[test]
    fn graduated_pruning_respects_time() {
        let fact_id = crate::id::FactId::new("f-1").expect("valid test id");
        let archived = vec![(fact_id.clone(), 0.05, 100.0)];
        let candidates = pruning_candidates(&archived, 720.0);
        assert!(
            candidates.is_empty(),
            "should not prune fact archived for less than min hours"
        );

        let long_archived = vec![(fact_id, 0.05, 800.0)];
        let candidates = pruning_candidates(&long_archived, 720.0);
        assert_eq!(
            candidates.len(),
            1,
            "should prune fact archived long enough"
        );
    }

    #[test]
    fn verified_fact_decays_slower_than_assumed() {
        let verified = DecayFactors {
            age_hours: 500.0,
            tier: EpistemicTier::Verified,
            ..default_factors()
        };
        let assumed = DecayFactors {
            age_hours: 500.0,
            tier: EpistemicTier::Assumed,
            ..default_factors()
        };
        let v_result = compute_decay(&DecayConfig::default(), &verified);
        let a_result = compute_decay(&DecayConfig::default(), &assumed);
        assert!(
            v_result.score > a_result.score,
            "verified should decay slower: {} vs {}",
            v_result.score,
            a_result.score
        );
    }

    #[test]
    fn identity_fact_decays_slower_than_observation() {
        let identity = DecayFactors {
            age_hours: 2000.0,
            fact_type: FactType::Identity,
            ..default_factors()
        };
        let observation = DecayFactors {
            age_hours: 2000.0,
            fact_type: FactType::Observation,
            ..default_factors()
        };
        let i_result = compute_decay(&DecayConfig::default(), &identity);
        let o_result = compute_decay(&DecayConfig::default(), &observation);
        assert!(
            i_result.score > o_result.score,
            "identity should decay slower: {} vs {}",
            i_result.score,
            o_result.score
        );
    }

    #[test]
    fn volatile_domain_decays_faster() {
        let stable = DecayFactors {
            age_hours: 500.0,
            volatility: 0.0,
            ..default_factors()
        };
        let volatile = DecayFactors {
            age_hours: 500.0,
            volatility: 1.0,
            ..default_factors()
        };
        let s_result = compute_decay(&DecayConfig::default(), &stable);
        let v_result = compute_decay(&DecayConfig::default(), &volatile);
        assert!(
            s_result.score > v_result.score,
            "stable domain should decay slower: {} vs {}",
            s_result.score,
            v_result.score
        );
    }

    #[test]
    fn score_recency_at_zero_is_one() {
        let s = score_recency(0.0, 720.0);
        assert!(
            (s - 1.0).abs() < f64::EPSILON,
            "recency at t=0 should be 1.0, got {s}"
        );
    }

    #[test]
    fn score_recency_at_stability_is_about_090() {
        let s = score_recency(720.0, 720.0);
        assert!(
            (s - 0.9).abs() < 0.01,
            "recency at t=S should be ~0.9, got {s}"
        );
    }

    #[test]
    fn score_frequency_increases_with_access() {
        let low = score_frequency(1);
        let high = score_frequency(50);
        assert!(
            high > low,
            "higher access should give higher frequency score: {high} vs {low}"
        );
    }

    #[test]
    fn score_reinforcement_caps() {
        let capped = score_reinforcement(100);
        assert!(
            (capped - MAX_REINFORCEMENT_BONUS).abs() < f64::EPSILON,
            "reinforcement should cap at {MAX_REINFORCEMENT_BONUS}, got {capped}"
        );
    }

    #[test]
    fn knowledge_stage_serde_roundtrip() {
        for stage in [
            KnowledgeStage::Active,
            KnowledgeStage::Fading,
            KnowledgeStage::Dormant,
            KnowledgeStage::Archived,
        ] {
            let json = serde_json::to_string(&stage).expect("KnowledgeStage serialization");
            let back: KnowledgeStage =
                serde_json::from_str(&json).expect("KnowledgeStage deserialization");
            assert_eq!(stage, back, "KnowledgeStage should survive roundtrip");
        }
    }

    #[test]
    fn knowledge_stage_from_str_roundtrip() {
        for stage in [
            KnowledgeStage::Active,
            KnowledgeStage::Fading,
            KnowledgeStage::Dormant,
            KnowledgeStage::Archived,
        ] {
            let parsed: KnowledgeStage = stage
                .as_str()
                .parse::<KnowledgeStage>()
                .expect("KnowledgeStage should parse from as_str");
            assert_eq!(stage, parsed, "KnowledgeStage roundtrip failed for {stage}");
        }
    }
}
