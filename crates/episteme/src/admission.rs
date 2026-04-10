//! Memory admission control: structured decision gate for knowledge graph insertion.
//!
//! Based on A-MAC (arxiv 2603.04549, Mar 2026) which formalizes admission as a
//! five-factor decision: utility, confidence, novelty, recency, and content type
//! prior. Without admission control, low-value facts accumulate, redundant facts
//! enter alongside existing knowledge, and hallucinated extractions persist until
//! temporal decay removes them.
//!
//! # Architecture
//!
//! The [`AdmissionPolicy`] trait defines the gate. [`KnowledgeStore::insert_fact`]
//! calls `policy.should_admit()` before writing to the Datalog engine.
//! [`DefaultAdmissionPolicy`] admits everything (preserving current behavior).
//! [`StructuredAdmissionPolicy`] applies the five-factor decision.

use crate::knowledge::Fact;

/// Result of an admission check.
#[derive(Debug, Clone, PartialEq)]
pub enum AdmissionDecision {
    /// Fact should be admitted to the knowledge graph.
    Admit,
    /// Fact should be rejected. The reason is logged but not surfaced to the user.
    Reject(AdmissionRejection),
}

impl AdmissionDecision {
    /// Returns `true` if the fact is admitted.
    pub fn is_admitted(&self) -> bool {
        matches!(self, Self::Admit)
    }
}

/// Reason for rejecting a fact at the admission gate.
#[derive(Debug, Clone, PartialEq)]
pub struct AdmissionRejection {
    /// Which factor(s) caused the rejection.
    pub factor: RejectionFactor,
    /// Human-readable explanation (logged at debug level).
    pub reason: String,
}

/// The factor that caused a rejection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RejectionFactor {
    /// The fact's predicted future utility is too low.
    LowUtility,
    /// The source confidence is below the admission threshold.
    LowConfidence,
    /// The fact is semantically redundant with existing knowledge.
    LowNovelty,
    /// The fact is too old to be worth storing.
    Stale,
    /// The content type has a low prior for admission.
    LowTypePrior,
    /// Combined score across all factors falls below threshold.
    BelowThreshold,
}

impl std::fmt::Display for RejectionFactor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::LowUtility => f.write_str("low_utility"),
            Self::LowConfidence => f.write_str("low_confidence"),
            Self::LowNovelty => f.write_str("low_novelty"),
            Self::Stale => f.write_str("stale"),
            Self::LowTypePrior => f.write_str("low_type_prior"),
            Self::BelowThreshold => f.write_str("below_threshold"),
        }
    }
}

/// Scores from the five admission factors (all in 0.0..=1.0).
#[derive(Debug, Clone)]
pub struct AdmissionScores {
    /// Predicted future utility (will this fact be needed later?).
    pub utility: f64,
    /// Source reliability (LLM extraction < user statement < external source).
    pub confidence: f64,
    /// Semantic novelty (1.0 = completely new, 0.0 = exact duplicate).
    pub novelty: f64,
    /// Temporal recency (1.0 = just now, decays toward 0.0).
    pub recency: f64,
    /// Content type prior (identity facts > preferences > transient observations).
    pub type_prior: f64,
}

impl AdmissionScores {
    /// Weighted combination of all five factors.
    ///
    /// Default weights from A-MAC: `utility` 0.25, `confidence` 0.20, `novelty` 0.25,
    /// `recency` 0.15, `type_prior` 0.15.
    pub fn combined(&self) -> f64 {
        self.utility * 0.25
            + self.confidence * 0.20
            + self.novelty * 0.25
            + self.recency * 0.15
            + self.type_prior * 0.15
    }
}

/// Gate that decides whether a fact should enter the knowledge graph.
///
/// Implementations range from [`DefaultAdmissionPolicy`] (admit all — current
/// behavior) to [`StructuredAdmissionPolicy`] (five-factor A-MAC decision).
pub trait AdmissionPolicy: Send + Sync {
    /// Evaluate whether `fact` should be admitted.
    ///
    /// Implementations may query the existing knowledge graph (e.g. for novelty
    /// checks) but should avoid expensive operations — this runs on every
    /// `insert_fact` call.
    fn should_admit(&self, fact: &Fact) -> AdmissionDecision;
}

/// Admits all facts that pass basic validation.
///
/// This preserves the pre-admission-control behavior: if a fact reaches
/// `insert_fact`, it gets stored. Validation (non-empty content, content length
/// limit, confidence range) is handled separately by `insert_fact` itself.
#[derive(Debug, Clone, Default)]
pub struct DefaultAdmissionPolicy;

impl AdmissionPolicy for DefaultAdmissionPolicy {
    fn should_admit(&self, _fact: &Fact) -> AdmissionDecision {
        AdmissionDecision::Admit
    }
}

/// Configuration for the structured admission policy.
#[derive(Debug, Clone)]
pub struct StructuredAdmissionConfig {
    /// Minimum combined score to admit (0.0..=1.0). Default: 0.3.
    pub threshold: f64,
    /// Minimum confidence to admit without other factors. Default: 0.1.
    pub min_confidence: f64,
    /// Maximum age in hours before a fact is considered stale. Default: 2160 (90 days).
    pub max_age_hours: f64,
}

impl Default for StructuredAdmissionConfig {
    fn default() -> Self {
        Self {
            threshold: 0.3,
            min_confidence: 0.1,
            max_age_hours: 2160.0,
        }
    }
}

/// Five-factor admission policy based on A-MAC (arxiv 2603.04549).
///
/// Evaluates each fact against utility, confidence, novelty, recency, and
/// content type prior. The combined weighted score must exceed the configured
/// threshold for admission.
///
/// This is a heuristic gate — it catches obvious low-value insertions without
/// requiring LLM evaluation. The weights and thresholds are tunable via
/// [`StructuredAdmissionConfig`].
#[derive(Debug, Clone)]
pub struct StructuredAdmissionPolicy {
    config: StructuredAdmissionConfig,
}

impl StructuredAdmissionPolicy {
    /// Create a new structured admission policy with the given configuration.
    pub fn new(config: StructuredAdmissionConfig) -> Self {
        Self { config }
    }

    /// Score a fact across all five factors.
    pub fn score(&self, fact: &Fact) -> AdmissionScores {
        AdmissionScores {
            utility: self.score_utility(fact),
            confidence: self.score_confidence(fact),
            novelty: 1.0, // Novelty requires store query — default to fully novel
            recency: self.score_recency(fact),
            type_prior: self.score_type_prior(fact),
        }
    }

    /// Utility: content length as a rough proxy for information density.
    ///
    /// Very short facts (<10 chars) are likely noise. Very long facts are likely
    /// substantive. The sigmoid maps `[0, 200]` chars to `[0.0, 1.0]`.
    #[expect(
        clippy::cast_precision_loss,
        reason = "content length well within f64 mantissa range for any realistic fact"
    )]
    #[expect(
        clippy::as_conversions,
        reason = "usize to f64 — content length is always small enough"
    )]
    #[expect(
        clippy::unused_self,
        reason = "method takes &self for API consistency; will use config for weight tuning"
    )]
    fn score_utility(&self, fact: &Fact) -> f64 {
        let len = fact.content.len() as f64;
        // Sigmoid centered at 50 chars, steepness 0.05
        1.0 / (1.0 + (-0.05 * (len - 50.0)).exp())
    }

    /// Confidence: direct from the fact's provenance.
    #[expect(
        clippy::unused_self,
        reason = "method takes &self for API consistency; will use config for weight tuning"
    )]
    fn score_confidence(&self, fact: &Fact) -> f64 {
        fact.provenance.confidence
    }

    /// Recency: exponential decay based on how old the fact's `recorded_at` is.
    #[expect(
        clippy::cast_precision_loss,
        reason = "nanosecond delta between two timestamps is well within f64 mantissa range for any realistic timespan"
    )]
    #[expect(
        clippy::as_conversions,
        reason = "i128 nanosecond delta to f64 — precision loss is negligible at this scale"
    )]
    fn score_recency(&self, fact: &Fact) -> f64 {
        let now = jiff::Timestamp::now();
        let age_nanos = now.as_nanosecond() - fact.temporal.recorded_at.as_nanosecond();
        if age_nanos <= 0 {
            return 1.0;
        }
        let age_hours = age_nanos as f64 / 3_600_000_000_000.0;
        // Exponential decay: half-life at max_age_hours / 3
        let half_life = self.config.max_age_hours / 3.0;
        (-age_hours * (2.0_f64.ln()) / half_life).exp()
    }

    /// Type prior: content types with higher base value get higher scores.
    #[expect(
        clippy::unused_self,
        reason = "method takes &self for API consistency; will use config for type-specific priors"
    )]
    fn score_type_prior(&self, fact: &Fact) -> f64 {
        match fact.fact_type.as_str() {
            // Identity and preference facts have highest retention value
            "identity" | "preference" | "user" | "feedback" => 0.9,
            // Structural/project facts are high value
            "project" | "reference" | "architecture" | "decision" => 0.8,
            // Behavioral observations are medium value
            "observation" | "pattern" | "skill" | "instinct" => 0.6,
            // Session-scoped facts are lower value (often transient)
            "session" | "context" | "ephemeral" => 0.3,
            // Unknown types get a neutral prior
            _ => 0.5,
        }
    }
}

impl AdmissionPolicy for StructuredAdmissionPolicy {
    fn should_admit(&self, fact: &Fact) -> AdmissionDecision {
        // Fast-reject: confidence below floor
        if fact.provenance.confidence < self.config.min_confidence {
            return AdmissionDecision::Reject(AdmissionRejection {
                factor: RejectionFactor::LowConfidence,
                reason: format!(
                    "confidence {:.2} below minimum {:.2}",
                    fact.provenance.confidence, self.config.min_confidence
                ),
            });
        }

        let scores = self.score(fact);
        let combined = scores.combined();

        if combined < self.config.threshold {
            return AdmissionDecision::Reject(AdmissionRejection {
                factor: RejectionFactor::BelowThreshold,
                reason: format!(
                    "combined score {combined:.3} below threshold {:.3} \
                     (utility={:.2}, confidence={:.2}, novelty={:.2}, \
                     recency={:.2}, type_prior={:.2})",
                    self.config.threshold,
                    scores.utility,
                    scores.confidence,
                    scores.novelty,
                    scores.recency,
                    scores.type_prior,
                ),
            });
        }

        AdmissionDecision::Admit
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::knowledge::{
        EpistemicTier, Fact, FactAccess, FactLifecycle, FactProvenance, FactTemporal,
    };

    fn make_fact(content: &str, confidence: f64, fact_type: &str) -> Fact {
        let now = jiff::Timestamp::now();
        Fact {
            id: crate::id::FactId::new("test-fact-id").unwrap_or_else(|_| unreachable!()),
            nous_id: "test-nous".to_owned(),
            fact_type: fact_type.to_owned(),
            content: content.to_owned(),
            scope: None,
            temporal: FactTemporal {
                valid_from: now,
                valid_to: crate::knowledge::far_future(),
                recorded_at: now,
            },
            provenance: FactProvenance {
                confidence,
                tier: EpistemicTier::Inferred,
                source_session_id: None,
                stability_hours: 0.0,
            },
            lifecycle: FactLifecycle {
                superseded_by: None,
                is_forgotten: false,
                forgotten_at: None,
                forget_reason: None,
            },
            access: FactAccess {
                access_count: 0,
                last_accessed_at: None,
            },
        }
    }

    #[test]
    fn default_policy_admits_all() {
        let policy = DefaultAdmissionPolicy;
        let fact = make_fact("any content", 0.5, "observation");
        assert!(policy.should_admit(&fact).is_admitted());
    }

    #[test]
    fn structured_policy_admits_high_quality_fact() {
        let policy = StructuredAdmissionPolicy::new(StructuredAdmissionConfig::default());
        let fact = make_fact(
            "The user prefers snake_case naming conventions for Rust variables",
            0.95,
            "preference",
        );
        assert!(policy.should_admit(&fact).is_admitted());
    }

    #[test]
    fn structured_policy_rejects_low_confidence() {
        let policy = StructuredAdmissionPolicy::new(StructuredAdmissionConfig::default());
        let fact = make_fact("might be something", 0.05, "observation");
        let decision = policy.should_admit(&fact);
        assert!(!decision.is_admitted());
        if let AdmissionDecision::Reject(r) = decision {
            assert_eq!(r.factor, RejectionFactor::LowConfidence);
        }
    }

    #[test]
    fn structured_policy_rejects_low_combined_score() {
        let config = StructuredAdmissionConfig {
            threshold: 0.8, // Very strict
            ..Default::default()
        };
        let policy = StructuredAdmissionPolicy::new(config);
        // Short content, low confidence, ephemeral type — should fail
        let fact = make_fact("ok", 0.15, "ephemeral");
        let decision = policy.should_admit(&fact);
        assert!(!decision.is_admitted());
        if let AdmissionDecision::Reject(r) = decision {
            assert_eq!(r.factor, RejectionFactor::BelowThreshold);
        }
    }

    #[test]
    fn type_prior_scores_are_ordered() {
        let policy = StructuredAdmissionPolicy::new(StructuredAdmissionConfig::default());
        let identity = make_fact("x", 0.5, "identity");
        let project = make_fact("x", 0.5, "project");
        let observation = make_fact("x", 0.5, "observation");
        let ephemeral = make_fact("x", 0.5, "ephemeral");

        assert!(policy.score_type_prior(&identity) > policy.score_type_prior(&project));
        assert!(policy.score_type_prior(&project) > policy.score_type_prior(&observation));
        assert!(policy.score_type_prior(&observation) > policy.score_type_prior(&ephemeral));
    }

    #[test]
    fn utility_increases_with_content_length() {
        let policy = StructuredAdmissionPolicy::new(StructuredAdmissionConfig::default());
        let short = make_fact("hi", 0.5, "observation");
        let medium = make_fact(
            "The user prefers detailed explanations with examples",
            0.5,
            "observation",
        );
        let long = make_fact(
            &"x".repeat(200),
            0.5,
            "observation",
        );

        assert!(policy.score_utility(&short) < policy.score_utility(&medium));
        assert!(policy.score_utility(&medium) < policy.score_utility(&long));
    }

    #[test]
    fn recency_decays_with_age() {
        let policy = StructuredAdmissionPolicy::new(StructuredAdmissionConfig::default());
        let now = jiff::Timestamp::now();

        let recent = make_fact("x", 0.5, "observation");

        let mut old = make_fact("x", 0.5, "observation");
        old.temporal.recorded_at = now
            .checked_sub(jiff::SignedDuration::from_hours(720))
            .unwrap_or(now);

        assert!(policy.score_recency(&recent) > policy.score_recency(&old));
    }

    #[test]
    fn combined_score_is_weighted_average() {
        let scores = AdmissionScores {
            utility: 1.0,
            confidence: 1.0,
            novelty: 1.0,
            recency: 1.0,
            type_prior: 1.0,
        };
        let combined = scores.combined();
        assert!((combined - 1.0).abs() < f64::EPSILON, "all 1.0 should combine to 1.0");

        let scores_zero = AdmissionScores {
            utility: 0.0,
            confidence: 0.0,
            novelty: 0.0,
            recency: 0.0,
            type_prior: 0.0,
        };
        assert!(scores_zero.combined().abs() < f64::EPSILON, "all 0.0 should combine to 0.0");
    }
}
