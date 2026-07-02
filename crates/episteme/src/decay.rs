//! Multi-factor temporal decay with lifecycle stages and graduated pruning.
//!
//! Extends the base FSRS decay in [`recall`](crate::recall) with:
//! - **Reinforcement signals**: explicit reinforcement boosts slow decay
//! - **Cross-agent access patterns**: facts accessed by multiple agents decay slower
//! - **Knowledge lifecycle stages**: active → fading → dormant → archived
//! - **Graduated pruning**: stage transitions instead of immediate deletion

use serde::{Deserialize, Serialize};

use crate::knowledge::{EpistemicTier, FactType, KnowledgeStage, StageTransition};

/// Default reinforcement boost per explicit reinforcement event.
///
/// Callers should prefer the value from `taxis::config::KnowledgeConfig::decay_reinforcement_boost`.
pub(crate) const DEFAULT_REINFORCEMENT_BOOST: f64 = 0.02;

/// Default maximum cumulative reinforcement bonus (caps at 50 reinforcements).
///
/// Callers should prefer the value from `taxis::config::KnowledgeConfig::decay_max_reinforcement_bonus`.
pub(crate) const DEFAULT_MAX_REINFORCEMENT_BONUS: f64 = 1.0;

/// Default multiplier bonus per distinct agent that accessed a fact.
///
/// Callers should prefer the value from `taxis::config::KnowledgeConfig::decay_cross_agent_bonus_per_agent`.
pub(crate) const DEFAULT_CROSS_AGENT_BONUS_PER_AGENT: f64 = 0.15;

/// Default maximum cross-agent multiplier (caps at 5 distinct agents → 1.75×).
///
/// Callers should prefer the value from `taxis::config::KnowledgeConfig::decay_max_cross_agent_multiplier`.
pub(crate) const DEFAULT_MAX_CROSS_AGENT_MULTIPLIER: f64 = 1.75;

/// Configuration for multi-factor decay computation.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DecayConfig {
    /// Weight for recency factor (hours since last access). Default: 0.35
    pub recency: f64,
    /// Weight for access frequency factor. Default: 0.25
    pub frequency: f64,
    /// Weight for confidence/tier factor. Default: 0.20
    pub confidence: f64,
    /// Weight for reinforcement signal factor. Default: 0.20
    pub reinforcement: f64,
    /// Boost per reinforcement event. Default: 0.02.
    pub reinforcement_boost: f64,
    /// Maximum cumulative reinforcement bonus. Default: 1.0.
    pub max_reinforcement_bonus: f64,
    /// Bonus per distinct agent access. Default: 0.15.
    pub cross_agent_bonus_per_agent: f64,
    /// Maximum cross-agent multiplier. Default: 1.75.
    pub max_cross_agent_multiplier: f64,
}

impl Default for DecayConfig {
    fn default() -> Self {
        Self {
            recency: 0.35,
            frequency: 0.25,
            confidence: 0.20,
            reinforcement: 0.20,
            reinforcement_boost: DEFAULT_REINFORCEMENT_BOOST,
            max_reinforcement_bonus: DEFAULT_MAX_REINFORCEMENT_BONUS,
            cross_agent_bonus_per_agent: DEFAULT_CROSS_AGENT_BONUS_PER_AGENT,
            max_cross_agent_multiplier: DEFAULT_MAX_CROSS_AGENT_MULTIPLIER,
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
    age_hours: f64,
    /// Classified fact type.
    fact_type: FactType,
    /// Epistemic confidence tier.
    tier: EpistemicTier,
    /// Total access count across all agents.
    access_count: u32,
    /// Number of explicit reinforcement events.
    reinforcement_count: u32,
    /// Number of distinct agents that accessed this fact.
    distinct_agent_count: u32,
    /// Domain volatility score in [0.0, 1.0] (from succession module).
    volatility: f64,
}

/// Result of multi-factor decay computation.
#[derive(Debug, Clone)]
pub(crate) struct DecayResult {
    /// Combined decay score in [0.0, 1.0]. Higher = better retained.
    pub(crate) score: f64,
    /// Computed lifecycle stage based on the decay score.
    pub(crate) stage: KnowledgeStage,
}

impl DecayResult {
    fn new(score: f64) -> Self {
        Self {
            score,
            stage: KnowledgeStage::from_decay_score(score),
        }
    }
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
        config.reinforcement_boost,
        config.max_reinforcement_bonus,
    );

    let recency = score_recency(factors.age_hours, effective_stability);
    let frequency = score_frequency(factors.access_count);
    let confidence = score_confidence(factors.tier);
    let reinforcement = score_reinforcement(
        factors.reinforcement_count,
        config.reinforcement_boost,
        config.max_reinforcement_bonus,
    );

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

    let cross_agent_mult = cross_agent_multiplier(
        factors.distinct_agent_count,
        config.cross_agent_bonus_per_agent,
        config.max_cross_agent_multiplier,
    );
    let score = (weighted * cross_agent_mult).clamp(0.0, 1.0);

    DecayResult::new(score)
}

/// FSRS power-law recency score.
///
/// `R(t) = (1 + 19/81 × t/S)^(-0.5)`
///
/// # Clock-jump handling
///
/// Negative or non-finite `age_hours` is clamped to `0.0` (treat as "just now").
/// WHY: A negative age means the fact's `last_accessed` timestamp is in the
/// future relative to `now` — this happens when the system clock jumps
/// backward (NTP correction, suspend/resume, VM migration). Returning a raw
/// decay score on negative input silently inflates recall toward 1.0; clamping
/// to 0 makes the "fresh" semantics explicit and avoids pathological ranking.
/// NaN is handled identically to defend against arithmetic upstream.
#[must_use]
fn score_recency(age_hours: f64, effective_stability: f64) -> f64 {
    let age_hours = sanitize_age_hours(age_hours);
    if age_hours <= 0.0 {
        return 1.0;
    }
    if effective_stability <= 0.0 {
        return 0.0;
    }
    (1.0 + (19.0 / 81.0) * age_hours / effective_stability).powf(-0.5)
}

/// Clamp `age_hours` to `[0, ∞)`, mapping NaN to 0. Positive infinity passes
/// through so the downstream formula naturally yields 0.
///
/// Emits a `debug` log when clamping fires so operators tracking clock-jump
/// events can correlate against NTP/suspend logs.
#[must_use]
pub(crate) fn sanitize_age_hours(age_hours: f64) -> f64 {
    if age_hours.is_nan() {
        tracing::debug!(
            age_hours,
            "FSRS decay received NaN age_hours, clamping to 0 (treat as just-now)"
        );
        return 0.0;
    }
    if age_hours < 0.0 {
        tracing::debug!(
            age_hours,
            "FSRS decay received negative age_hours (clock jumped backward?), \
             clamping to 0 (treat as just-now)"
        );
        return 0.0;
    }
    age_hours
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
        // WHY: Verified (ground truth) and Training (permanent session outcomes)
        // both receive the maximum confidence score and must not decay via the
        // confidence factor (#5866).
        EpistemicTier::Verified | EpistemicTier::Training => 1.0,
        // WHY: Reflected sits between Verified and Inferred per its tier score
        // (0.83) and stability multiplier (2.5) (#5866).
        EpistemicTier::Reflected => 0.9,
        EpistemicTier::Inferred => 0.6,
        // WHY: Assumed and any genuinely unknown future tiers get the lowest score.
        _ => 0.3,
    }
}

/// Reinforcement signal score.
///
/// Each reinforcement event adds a fixed boost, capped at `max_reinforcement_bonus`.
#[must_use]
fn score_reinforcement(
    reinforcement_count: u32,
    reinforcement_boost: f64,
    max_reinforcement_bonus: f64,
) -> f64 {
    let bonus = f64::from(reinforcement_count) * reinforcement_boost;
    bonus.min(max_reinforcement_bonus)
}

/// Cross-agent access multiplier.
///
/// Facts accessed by multiple distinct agents are considered more universally
/// relevant and decay slower. Each additional agent beyond the first adds
/// a bonus multiplier.
#[must_use]
pub(crate) fn cross_agent_multiplier(
    distinct_agent_count: u32,
    bonus_per_agent: f64,
    max_multiplier: f64,
) -> f64 {
    if distinct_agent_count <= 1 {
        return 1.0;
    }
    let bonus = f64::from(distinct_agent_count - 1) * bonus_per_agent;
    (1.0 + bonus).min(max_multiplier)
}

/// Compute effective stability incorporating reinforcement signals.
///
/// Extends [`crate::recall::compute_effective_stability`] with:
/// - Reinforcement bonus: `1 + reinforcement_count × reinforcement_boost`
/// - Volatility adjustment from succession module
#[must_use]
pub(crate) fn compute_effective_stability_with_reinforcement(
    fact_type: FactType,
    tier: EpistemicTier,
    access_count: u32,
    reinforcement_count: u32,
    volatility: f64,
    reinforcement_boost: f64,
    max_reinforcement_bonus: f64,
) -> f64 {
    let base = crate::recall::compute_effective_stability(fact_type, tier, access_count);
    let reinforcement_mult =
        1.0 + (f64::from(reinforcement_count) * reinforcement_boost).min(max_reinforcement_bonus);

    // WHY: Training facts are permanent records of session outcomes and are
    // not subject to normal memory-decay mechanisms, including domain
    // volatility. Applying the volatility multiplier would inflate their
    // effective stability beyond the documented 4x base tier multiplier.
    if tier == EpistemicTier::Training {
        return base * reinforcement_mult;
    }

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
#[path = "decay_tests.rs"]
mod tests;
