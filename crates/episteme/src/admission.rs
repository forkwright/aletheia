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
//! [`StructuredAdmissionPolicy`] applies the five-factor decision, including a
//! content-hash novelty check that rejects exact duplicate fact content within
//! the lifetime of the policy instance.

use std::collections::HashSet;
use std::sync::Mutex;

use sha2::{Digest, Sha256};

use crate::knowledge::Fact;

/// Result of an admission check.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
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
#[non_exhaustive]
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
    /// SHA-256 content-hash deduplication: facts whose normalized content
    /// hashes to a value already seen by this policy instance are rejected with
    /// [`RejectionFactor::LowNovelty`]. Set to `false` to disable.
    ///
    /// Default: `true`. The hash set is bounded by the number of admitted facts
    /// so memory growth tracks the knowledge graph itself.
    pub content_hash_dedup: bool,
}

impl Default for StructuredAdmissionConfig {
    fn default() -> Self {
        Self {
            threshold: 0.3,
            min_confidence: 0.1,
            max_age_hours: 2160.0,
            content_hash_dedup: true,
        }
    }
}

/// Five-factor admission policy based on A-MAC (arxiv 2603.04549).
///
/// Evaluates each fact against utility, confidence, novelty, recency, and
/// content type prior. The combined weighted score must exceed the configured
/// threshold for admission.
///
/// Novelty is checked via a SHA-256 content hash: if the normalized content of
/// the incoming fact has been seen before (i.e., its hash is already in the
/// internal set), it is scored as zero novelty and fast-rejected with
/// [`RejectionFactor::LowNovelty`]. Admitted facts have their hash recorded so
/// future duplicates are caught.
///
/// This is a heuristic gate — it catches obvious low-value insertions without
/// requiring LLM evaluation. The weights and thresholds are tunable via
/// [`StructuredAdmissionConfig`].
pub struct StructuredAdmissionPolicy {
    config: StructuredAdmissionConfig,
    /// SHA-256 hashes of content strings already admitted, used for exact-duplicate
    /// detection. Guarded by a `Mutex` so the policy can implement `Sync` while
    /// mutating the set on each call.
    seen_hashes: Mutex<HashSet<[u8; 32]>>,
}

impl std::fmt::Debug for StructuredAdmissionPolicy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StructuredAdmissionPolicy")
            .field("config", &self.config)
            .field(
                "seen_hashes_count",
                &self.seen_hashes.lock().map_or(0, |g| g.len()),
            )
            .finish()
    }
}

impl StructuredAdmissionPolicy {
    /// Create a new structured admission policy with the given configuration.
    pub fn new(config: StructuredAdmissionConfig) -> Self {
        Self {
            config,
            seen_hashes: Mutex::new(HashSet::new()),
        }
    }

    /// Compute the SHA-256 hash of normalized fact content.
    ///
    /// Normalization: trim leading/trailing whitespace and fold to lowercase so
    /// that trivially equivalent statements are treated as duplicates.
    fn content_hash(fact: &Fact) -> [u8; 32] {
        let normalized = fact.content.trim().to_lowercase();
        let digest = Sha256::digest(normalized.as_bytes());
        let mut out = [0u8; 32];
        for (dst, src) in out.iter_mut().zip(digest.iter()) {
            *dst = *src;
        }
        out
    }

    /// Check whether `fact` is an exact duplicate of previously admitted content.
    ///
    /// Returns `true` if the content hash has been seen before (duplicate).
    /// If `content_hash_dedup` is disabled in config, always returns `false`.
    fn is_duplicate(&self, fact: &Fact) -> bool {
        if !self.config.content_hash_dedup {
            return false;
        }
        let hash = Self::content_hash(fact);
        // INVARIANT: lock is never held across await points; no risk of deadlock here.
        let guard = self
            .seen_hashes
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        guard.contains(&hash)
    }

    /// Record the content hash of an admitted fact.
    ///
    /// Must be called only after the fact has been decided as admitted so that
    /// future identical content is caught as a duplicate.
    fn record_admitted(&self, fact: &Fact) {
        if !self.config.content_hash_dedup {
            return;
        }
        let hash = Self::content_hash(fact);
        let mut guard = self
            .seen_hashes
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        guard.insert(hash);
    }

    /// Score a fact across all five factors.
    ///
    /// The `novelty` dimension is set to `0.0` if the fact is an exact
    /// content-hash duplicate of a previously admitted fact, and `1.0`
    /// otherwise. Duplicate detection via the hash set is a fast-reject path
    /// that bypasses the full scoring pipeline in [`should_admit`].
    pub fn score(&self, fact: &Fact) -> AdmissionScores {
        AdmissionScores {
            utility: self.score_utility(fact),
            confidence: self.score_confidence(fact),
            novelty: if self.is_duplicate(fact) { 0.0 } else { 1.0 },
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
        // kanon:ignore RUST/as-cast — usize→f64 for a sigmoid input; precision is irrelevant for content-length scoring and try_from(usize)→f64 doesn't exist.
        let len = fact.content.len() as f64;
        // INVARIANT: sigmoid centered at 50 chars, steepness 0.05
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
        // INVARIANT: exponential decay with half-life at max_age_hours / 3
        let half_life = self.config.max_age_hours / 3.0;
        (-age_hours * (2.0_f64.ln()) / half_life).exp()
    }

    /// Type prior: content types with higher base value get higher scores.
    #[expect(
        clippy::unused_self,
        reason = "method takes &self for API consistency; will use config for type-specific priors"
    )]
    fn score_type_prior(&self, fact: &Fact) -> f64 {
        // INVARIANT: priors rank fact types by retention value —
        // identity/preference highest, then structural/project, behavioral
        // observations, session-scoped (transient); unknown types get a
        // neutral prior.
        match fact.fact_type.as_str() {
            "identity" | "preference" | "user" | "feedback" => 0.9,
            "project" | "reference" | "architecture" | "decision" => 0.8,
            "observation" | "pattern" | "skill" | "instinct" => 0.6,
            "session" | "context" | "ephemeral" => 0.3,
            _ => 0.5,
        }
    }
}

impl AdmissionPolicy for StructuredAdmissionPolicy {
    fn should_admit(&self, fact: &Fact) -> AdmissionDecision {
        // Fast-reject: confidence below floor
        if fact.provenance.confidence < self.config.min_confidence {
            crate::metrics::record_admission_rejected(
                &fact.nous_id,
                &fact.fact_type,
                RejectionFactor::LowConfidence,
            );
            return AdmissionDecision::Reject(AdmissionRejection {
                factor: RejectionFactor::LowConfidence,
                reason: format!(
                    "confidence {:.2} below minimum {:.2}",
                    fact.provenance.confidence, self.config.min_confidence
                ),
            });
        }

        // Fast-reject: exact content-hash duplicate
        if self.is_duplicate(fact) {
            crate::metrics::record_admission_rejected(
                &fact.nous_id,
                &fact.fact_type,
                RejectionFactor::LowNovelty,
            );
            return AdmissionDecision::Reject(AdmissionRejection {
                factor: RejectionFactor::LowNovelty,
                reason: "content is an exact duplicate of a previously admitted fact".to_owned(),
            });
        }

        let scores = self.score(fact);
        let combined = scores.combined();

        if combined < self.config.threshold {
            crate::metrics::record_admission_rejected(
                &fact.nous_id,
                &fact.fact_type,
                RejectionFactor::BelowThreshold,
            );
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

        // Fact is admitted: record its hash to catch future duplicates.
        self.record_admitted(fact);
        crate::metrics::record_admission_ok(&fact.nous_id, &fact.fact_type);
        if fact.provenance.confidence < crate::metrics::LOW_CONFIDENCE_ADMISSION_THRESHOLD {
            crate::metrics::record_low_confidence_admission(&fact.nous_id);
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
            project_id: None,
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
            sensitivity: crate::knowledge::FactSensitivity::Public,
            visibility: crate::knowledge::Visibility::Private,
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
        let long = make_fact(&"x".repeat(200), 0.5, "observation");

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
        assert!(
            (combined - 1.0).abs() < f64::EPSILON,
            "all 1.0 should combine to 1.0"
        );

        let scores_zero = AdmissionScores {
            utility: 0.0,
            confidence: 0.0,
            novelty: 0.0,
            recency: 0.0,
            type_prior: 0.0,
        };
        assert!(
            scores_zero.combined().abs() < f64::EPSILON,
            "all 0.0 should combine to 0.0"
        );
    }

    // ── content-hash deduplication ──────────────────────────────────────────

    #[test]
    fn content_hash_dedup_rejects_exact_duplicate() {
        let policy = StructuredAdmissionPolicy::new(StructuredAdmissionConfig::default());
        let fact = make_fact(
            "The user prefers snake_case naming conventions for Rust variables",
            0.95,
            "preference",
        );

        // First insertion: admitted.
        assert!(
            policy.should_admit(&fact).is_admitted(),
            "first insertion of a high-quality fact must be admitted"
        );

        // Second insertion of identical content: rejected as duplicate.
        let decision = policy.should_admit(&fact);
        assert!(
            !decision.is_admitted(),
            "exact duplicate content must be rejected"
        );
        if let AdmissionDecision::Reject(r) = decision {
            assert_eq!(
                r.factor,
                RejectionFactor::LowNovelty,
                "duplicate must be rejected with LowNovelty"
            );
        }
    }

    #[test]
    fn content_hash_dedup_normalizes_whitespace_and_case() {
        let policy = StructuredAdmissionPolicy::new(StructuredAdmissionConfig::default());
        let fact_a = make_fact(
            "The user prefers snake_case naming for Rust",
            0.95,
            "preference",
        );
        // Same content but with leading/trailing whitespace and different case.
        let fact_b = make_fact(
            "  the user prefers snake_case naming for rust  ",
            0.95,
            "preference",
        );

        assert!(
            policy.should_admit(&fact_a).is_admitted(),
            "first fact admitted"
        );
        let decision = policy.should_admit(&fact_b);
        assert!(
            !decision.is_admitted(),
            "case/whitespace-normalized duplicate must be rejected"
        );
        if let AdmissionDecision::Reject(r) = decision {
            assert_eq!(r.factor, RejectionFactor::LowNovelty);
        }
    }

    #[test]
    fn content_hash_dedup_does_not_fail_open_on_poisoned_mutex() {
        let policy = StructuredAdmissionPolicy::new(StructuredAdmissionConfig::default());
        let fact = make_fact(
            "The user prefers snake_case naming conventions for Rust variables",
            0.95,
            "preference",
        );

        assert!(
            policy.should_admit(&fact).is_admitted(),
            "first insertion of a high-quality fact must be admitted"
        );

        let poison_result = std::thread::scope(|scope| {
            scope
                .spawn(|| {
                    let _guard = policy
                        .seen_hashes
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner);
                    panic!("intentional mutex poison");
                })
                .join()
        });
        assert!(
            poison_result.is_err(),
            "test setup must poison the deduplication mutex"
        );

        let duplicate = make_fact(
            "The user prefers snake_case naming conventions for Rust variables",
            0.95,
            "preference",
        );
        let decision = policy.should_admit(&duplicate);
        assert!(
            !decision.is_admitted(),
            "duplicate must not be treated as novel after mutex poison"
        );
        if let AdmissionDecision::Reject(rejection) = decision {
            assert_eq!(rejection.factor, RejectionFactor::LowNovelty);
        }
    }

    #[test]
    fn content_hash_dedup_admits_distinct_content() {
        let policy = StructuredAdmissionPolicy::new(StructuredAdmissionConfig::default());
        let fact_a = make_fact(
            "The user prefers snake_case naming conventions for Rust variables",
            0.95,
            "preference",
        );
        let fact_b = make_fact(
            "The user prefers camelCase naming conventions for TypeScript",
            0.95,
            "preference",
        );

        assert!(
            policy.should_admit(&fact_a).is_admitted(),
            "first fact admitted"
        );
        assert!(
            policy.should_admit(&fact_b).is_admitted(),
            "distinct content must be admitted independently"
        );
    }

    #[test]
    fn content_hash_dedup_disabled_admits_duplicates() {
        let config = StructuredAdmissionConfig {
            content_hash_dedup: false,
            ..Default::default()
        };
        let policy = StructuredAdmissionPolicy::new(config);
        let fact = make_fact(
            "The user prefers snake_case naming conventions for Rust variables",
            0.95,
            "preference",
        );

        assert!(policy.should_admit(&fact).is_admitted(), "first admitted");
        assert!(
            policy.should_admit(&fact).is_admitted(),
            "with dedup disabled, duplicate must be admitted"
        );
    }

    // ── threshold / min_confidence determinism ──────────────────────────────

    #[test]
    fn min_confidence_boundary_is_inclusive() {
        // Exactly at the boundary: confidence == min_confidence must be admitted.
        let config = StructuredAdmissionConfig {
            min_confidence: 0.5,
            threshold: 0.0, // Disable combined check so we isolate the confidence gate.
            ..Default::default()
        };
        let policy = StructuredAdmissionPolicy::new(config);

        let at_boundary = make_fact(
            "The user prefers detailed explanations with examples and code snippets",
            0.5,
            "preference",
        );
        let below_boundary = make_fact(
            "The user prefers detailed explanations with examples and code snippets extra words",
            0.499,
            "preference",
        );

        assert!(
            policy.should_admit(&at_boundary).is_admitted(),
            "confidence exactly at min_confidence must be admitted"
        );
        let decision = policy.should_admit(&below_boundary);
        assert!(
            !decision.is_admitted(),
            "confidence below min_confidence must be rejected"
        );
        if let AdmissionDecision::Reject(r) = decision {
            assert_eq!(r.factor, RejectionFactor::LowConfidence);
        }
    }

    #[test]
    fn structured_threshold_admits_above_and_rejects_below() {
        // Use a high threshold with a well-scoring fact to verify the boundary.
        let permissive = StructuredAdmissionConfig {
            threshold: 0.01,
            ..Default::default()
        };
        let strict = StructuredAdmissionConfig {
            threshold: 0.99,
            ..Default::default()
        };

        let policy_permissive = StructuredAdmissionPolicy::new(permissive);
        let policy_strict = StructuredAdmissionPolicy::new(strict);

        let fact = make_fact(
            "The user prefers detailed explanations with examples and code snippets",
            0.95,
            "preference",
        );

        assert!(
            policy_permissive.should_admit(&fact).is_admitted(),
            "permissive threshold must admit high-quality fact"
        );
        let decision = policy_strict.should_admit(&fact);
        assert!(
            !decision.is_admitted(),
            "strict threshold 0.99 must reject even a high-quality fact"
        );
        if let AdmissionDecision::Reject(r) = decision {
            assert_eq!(r.factor, RejectionFactor::BelowThreshold);
        }
    }
}
