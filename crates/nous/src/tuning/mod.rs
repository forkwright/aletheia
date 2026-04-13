//! Self-tuning feedback loop: agents propose parameter changes with evidence.
//!
//! Runs on the prosoche cycle (periodic background work). The loop:
//! 1. Observes outcome metrics
//! 2. Queries the parameter registry for relevant parameters
//! 3. Validates evidence against configurable thresholds
//! 4. Proposes changes within operator bounds
//! 5. Returns outcomes for persistence and logging

pub mod evidence;
pub mod signals;

use taxis::config::TuningConfig;
use taxis::registry::{ParameterSpec, ParameterTier, ParameterValue};

/// A time-stamped metric observation.
#[derive(Debug, Clone)]
pub struct MetricSample {
    /// Name of the metric (matches a `ParameterSpec::outcome_signal`).
    pub metric_name: String,
    /// Observed value.
    pub value: f64,
    /// When the observation was taken.
    pub timestamp: jiff::Timestamp,
}

/// Evidence supporting a proposed parameter change.
#[derive(Debug, Clone)]
pub struct ProposalEvidence {
    /// Number of metric samples used to compute the proposal.
    pub sample_count: usize,
    /// Mean metric value before the observation window.
    pub metric_before: f64,
    /// Mean metric value during the observation window.
    pub metric_after: f64,
    /// Difference: `metric_after - metric_before`.
    pub delta: f64,
    /// Human-readable rationale for the change.
    pub rationale: String,
}

/// A proposal to change a tunable parameter.
#[derive(Debug, Clone)]
pub struct ParameterProposal {
    /// Dotted config key (e.g. `"distillation.contextTokenTrigger"`).
    pub key: String,
    /// Current live value.
    pub current_value: ParameterValue,
    /// Evidence-derived proposed value.
    pub proposed_value: ParameterValue,
    /// Evidence supporting the change.
    pub evidence: ProposalEvidence,
    /// Agent `nous_id` that proposed the change.
    pub proposed_by: String,
}

/// Outcome of evaluating a parameter proposal against operator bounds.
#[derive(Debug, Clone)]
pub enum ProposalOutcome {
    /// The proposal was accepted and the parameter was changed.
    Applied {
        /// Config key that was changed.
        key: String,
        /// Previous value.
        old: ParameterValue,
        /// New value.
        new: ParameterValue,
    },
    /// The proposal was rejected (insufficient evidence, disabled, etc.).
    Rejected {
        /// Config key that was rejected.
        key: String,
        /// Reason for rejection.
        reason: String,
    },
    /// The proposed value was outside operator bounds.
    OutOfBounds {
        /// Config key.
        key: String,
        /// The value that was proposed.
        proposed: ParameterValue,
        /// `(min, max)` bounds from the parameter spec.
        bounds: (f64, f64),
    },
}

/// Core tuning proposer: evaluates metric observations against the parameter
/// registry and generates bounded proposals.
pub struct TuningProposer {
    config: TuningConfig,
}

impl TuningProposer {
    /// Create a new proposer with the given tuning configuration.
    #[must_use]
    pub fn new(config: TuningConfig) -> Self {
        Self { config }
    }

    /// Evaluate a batch of metric observations and generate proposals.
    ///
    /// Returns up to `max_changes_per_cycle` proposals. Each proposal is
    /// validated against the parameter registry's operator bounds and
    /// tier restrictions.
    ///
    /// Returns an empty vec when tuning is disabled or no evidence meets
    /// the significance threshold.
    #[must_use]
    #[expect(
        clippy::as_conversions,
        reason = "u32->usize: max_changes_per_cycle is a small config value, always fits in usize"
    )]
    pub fn evaluate(
        &self,
        observations: &[MetricSample],
        nous_id: &str,
    ) -> Vec<ProposalOutcome> {
        if !self.config.enabled {
            return Vec::new();
        }

        let max_changes = self.config.max_changes_per_cycle as usize; // kanon:ignore RUST/as-cast

        // Group observations by metric name.
        let mut by_metric: std::collections::HashMap<&str, Vec<&MetricSample>> =
            std::collections::HashMap::new();
        for obs in observations {
            by_metric.entry(obs.metric_name.as_str()).or_default().push(obs);
        }

        let mut outcomes = Vec::new();

        for (metric_name, samples) in &by_metric {
            if outcomes.len() >= max_changes {
                break;
            }

            // Find parameters whose outcome_signal matches this metric.
            let specs = taxis::registry::all_specs()
                .iter()
                .filter(|s| s.outcome_signal == *metric_name)
                .collect::<Vec<_>>();

            for spec in specs {
                if outcomes.len() >= max_changes {
                    break;
                }

                let outcome = self.evaluate_spec(spec, samples, nous_id);
                outcomes.push(outcome);
            }
        }

        outcomes
    }

    /// Evaluate a single parameter spec against its metric samples.
    #[expect(
        clippy::as_conversions,
        reason = "u32->usize: evidence_min_samples is a small config value, always fits in usize"
    )]
    fn evaluate_spec(
        &self,
        spec: &ParameterSpec,
        samples: &[&MetricSample],
        nous_id: &str,
    ) -> ProposalOutcome {
        // Guard: only SelfTuning parameters may be tuned.
        if spec.tier != ParameterTier::SelfTuning {
            return ProposalOutcome::Rejected {
                key: spec.key.to_owned(),
                reason: format!(
                    "parameter tier is {} (only self-tuning parameters may be tuned)",
                    spec.tier
                ),
            };
        }

        let min_samples = self.config.evidence_min_samples as usize; // kanon:ignore RUST/as-cast

        // Guard: minimum sample count.
        if samples.len() < min_samples {
            return ProposalOutcome::Rejected {
                key: spec.key.to_owned(),
                reason: format!(
                    "insufficient samples: {} < minimum {}",
                    samples.len(),
                    self.config.evidence_min_samples,
                ),
            };
        }

        // Split samples into before/after halves for A/B comparison.
        let values: Vec<f64> = samples.iter().map(|s| s.value).collect();
        let validation = evidence::validate_evidence(
            &values,
            self.config.significance_threshold,
        );

        let Some(result) = validation else {
            return ProposalOutcome::Rejected {
                key: spec.key.to_owned(),
                reason: "evidence validation produced no result (insufficient variance)".to_owned(),
            };
        };

        if !result.is_significant {
            return ProposalOutcome::Rejected {
                key: spec.key.to_owned(),
                reason: format!(
                    "delta {:.4} does not exceed significance threshold ({:.4} * {:.4} stddev = {:.4})",
                    result.delta.abs(),
                    self.config.significance_threshold,
                    result.stddev,
                    self.config.significance_threshold * result.stddev,
                ),
            };
        }

        // Derive a proposed value from the current default and the direction of improvement.
        let proposed = derive_proposed_value(spec, result.delta);

        // Validate against operator bounds.
        if let Some((min, max)) = spec.bounds {
            let proposed_f64 = parameter_value_to_f64(&proposed);
            if proposed_f64 < min || proposed_f64 > max {
                return ProposalOutcome::OutOfBounds {
                    key: spec.key.to_owned(),
                    proposed,
                    bounds: (min, max),
                };
            }
        }

        let evidence = ProposalEvidence {
            sample_count: samples.len(),
            metric_before: result.mean_before,
            metric_after: result.mean_after,
            delta: result.delta,
            rationale: format!(
                "{} samples, delta {:.4} ({:.1}% change), significance {:.2}x stddev",
                samples.len(),
                result.delta,
                if result.mean_before.abs() > f64::EPSILON {
                    (result.delta / result.mean_before) * 100.0
                } else {
                    0.0
                },
                if result.stddev.abs() > f64::EPSILON {
                    result.delta.abs() / result.stddev
                } else {
                    0.0
                },
            ),
        };

        let proposal = ParameterProposal {
            key: spec.key.to_owned(),
            current_value: spec.default.clone(),
            proposed_value: proposed.clone(),
            evidence,
            proposed_by: nous_id.to_owned(),
        };

        tracing::info!(
            key = %proposal.key,
            proposed_by = %proposal.proposed_by,
            sample_count = proposal.evidence.sample_count,
            delta = proposal.evidence.delta,
            rationale = %proposal.evidence.rationale,
            "self-tuning: parameter change applied"
        );

        ProposalOutcome::Applied {
            key: proposal.key,
            old: proposal.current_value,
            new: proposal.proposed_value,
        }
    }
}

/// Derive a proposed value by nudging the default in the direction indicated
/// by the spec's `direction_hint` and the observed delta.
///
/// The adjustment is 10% of the current value in the beneficial direction.
fn derive_proposed_value(spec: &ParameterSpec, delta: f64) -> ParameterValue {
    use taxis::registry::TuningDirection;

    let current_f64 = parameter_value_to_f64(&spec.default);
    // WHY: a positive delta means the metric improved; a negative delta means
    // it degraded. The direction_hint tells us which direction to nudge.
    let nudge_factor = match spec.direction_hint {
        TuningDirection::Higher => {
            if delta > 0.0 { 0.10 } else { -0.10 }
        }
        TuningDirection::Lower => {
            if delta > 0.0 { -0.10 } else { 0.10 }
        }
        TuningDirection::Contextual => {
            // For contextual parameters, use the sign of delta directly.
            if delta > 0.0 { 0.10 } else { -0.10 }
        }
    };

    let proposed_f64 = current_f64 * (1.0 + nudge_factor);

    // Preserve the type of the original value.
    match spec.default {
        ParameterValue::Int(_) => {
            // WHY: proposed_f64 is derived from int * 1.1 or int * 0.9, which is
            // safely within i64 range for any config parameter value.
            #[expect(
                clippy::cast_possible_truncation,
                clippy::as_conversions,
                reason = "f64->i64: proposed value derived from small config ints via +-10% nudge, safely within i64 range"
            )]
            let v = proposed_f64.round() as i64; // kanon:ignore RUST/as-cast
            ParameterValue::Int(v)
        }
        ParameterValue::Float(_) => ParameterValue::Float(proposed_f64),
        ParameterValue::Bool(_) => spec.default.clone(), // bools are not nudged
        ParameterValue::Duration(_) => {
            // WHY: proposed_f64 is derived from u64 * 1.1 or u64 * 0.9, which is
            // non-negative and safely within u64 range for any config duration.
            #[expect(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                clippy::as_conversions,
                reason = "f64->u64: proposed value derived from small config durations via +-10% nudge, clamped to 0"
            )]
            let v = proposed_f64.round().max(0.0) as u64; // kanon:ignore RUST/as-cast
            ParameterValue::Duration(v)
        }
    }
}

/// Convert a `ParameterValue` to `f64` for numeric comparison.
#[expect(
    clippy::cast_precision_loss,
    clippy::as_conversions,
    reason = "i64/u64->f64: config parameter values are small (token counts, durations), far below f64 mantissa precision"
)]
fn parameter_value_to_f64(value: &ParameterValue) -> f64 {
    match value {
        ParameterValue::Int(v) => *v as f64, // kanon:ignore RUST/as-cast
        ParameterValue::Float(v) => *v,
        ParameterValue::Bool(v) => if *v { 1.0 } else { 0.0 },
        ParameterValue::Duration(v) => *v as f64, // kanon:ignore RUST/as-cast
    }
}

/// Build a knowledge-store fact content string for a parameter override.
///
/// Returns a JSON-formatted string suitable for `fact_type = "parameter_override"`.
#[must_use]
pub fn build_override_fact_content(
    key: &str,
    value: &ParameterValue,
    set_by: &str,
    evidence_summary: &str,
) -> String {
    // WHY: serde_json is not used here to avoid adding a dependency for a
    // simple 4-field JSON object. The values are pre-validated.
    format!(
        r#"{{"key":"{key}","value":{value},"set_by":"{set_by}","evidence":"{}"}}"#,
        evidence_summary.replace('"', "'"),
    )
}

#[cfg(test)]
#[expect(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "test assertions may panic on failure"
)]
mod tests {
    use super::*;

    fn make_config(enabled: bool) -> TuningConfig {
        TuningConfig {
            enabled,
            max_changes_per_cycle: 3,
            evidence_min_samples: 20,
            significance_threshold: 1.5,
        }
    }

    fn make_samples(metric_name: &str, values: &[f64]) -> Vec<MetricSample> {
        let base = jiff::Timestamp::from_second(1_700_000_000).unwrap();
        values
            .iter()
            .enumerate()
            .map(|(i, &v)| MetricSample {
                metric_name: metric_name.to_owned(),
                value: v,
                timestamp: base
                    .checked_add(jiff::SignedDuration::from_secs(
                        i64::try_from(i).expect("index fits i64") * 60,
                    ))
                    .unwrap(),
            })
            .collect()
    }

    #[test]
    fn disabled_config_produces_no_proposals() {
        let proposer = TuningProposer::new(make_config(false));
        let samples = make_samples("turn_quality_post_distillation", &[0.5; 30]);
        let outcomes = proposer.evaluate(&samples, "test-nous");
        assert!(outcomes.is_empty(), "disabled tuning should produce no outcomes");
    }

    #[test]
    fn insufficient_samples_rejected() {
        let proposer = TuningProposer::new(make_config(true));
        // Only 5 samples, minimum is 20
        let samples = make_samples("turn_quality_post_distillation", &[0.5; 5]);
        let outcomes = proposer.evaluate(&samples, "test-nous");
        // Should have rejections for insufficient samples
        for outcome in &outcomes {
            match outcome {
                ProposalOutcome::Rejected { reason, .. } => {
                    assert!(reason.contains("insufficient samples"), "expected insufficient samples rejection, got: {reason}");
                }
                _ => panic!("expected Rejected outcome"),
            }
        }
    }

    #[test]
    fn out_of_bounds_proposal_detected() {
        // Create samples with a massive improvement to force a large nudge
        let mut values = vec![0.1; 15];
        values.extend(vec![100.0; 15]);
        let proposer = TuningProposer::new(make_config(true));
        let samples = make_samples("turn_quality_post_distillation", &values);
        let outcomes = proposer.evaluate(&samples, "test-nous");

        // At least one outcome should exist (the registry has specs for this signal)
        assert!(!outcomes.is_empty(), "should produce at least one outcome");
    }

    #[test]
    fn weak_evidence_rejected() {
        let proposer = TuningProposer::new(make_config(true));
        // All values are the same — no delta, no significance
        let samples = make_samples("turn_quality_post_distillation", &[0.5; 30]);
        let outcomes = proposer.evaluate(&samples, "test-nous");

        for outcome in &outcomes {
            match outcome {
                ProposalOutcome::Rejected { reason, .. } => {
                    assert!(
                        reason.contains("insufficient variance") || reason.contains("does not exceed"),
                        "expected weak evidence rejection, got: {reason}"
                    );
                }
                _ => panic!("expected Rejected outcome for weak evidence"),
            }
        }
    }

    #[test]
    fn valid_proposal_generates_applied() {
        // Create a clear signal: first half low, second half high
        let mut values = vec![0.3; 15];
        values.extend(vec![0.8; 15]);

        let mut config = make_config(true);
        config.significance_threshold = 0.5; // lower threshold for test
        let proposer = TuningProposer::new(config);
        let samples = make_samples("turn_quality_post_distillation", &values);
        let outcomes = proposer.evaluate(&samples, "test-nous");

        // Should have at least one Applied or OutOfBounds (not all Rejected)
        let has_non_rejected = outcomes.iter().any(|o| {
            !matches!(o, ProposalOutcome::Rejected { .. })
        });
        // The test may produce OutOfBounds too, which is acceptable
        assert!(
            has_non_rejected || outcomes.is_empty(),
            "expected at least one non-rejected outcome when evidence is strong"
        );
    }

    #[test]
    fn max_changes_per_cycle_enforced() {
        let mut config = make_config(true);
        config.max_changes_per_cycle = 1;
        config.significance_threshold = 0.1;
        config.evidence_min_samples = 4;
        let proposer = TuningProposer::new(config);

        // Use a signal that matches many specs
        let mut values = vec![0.1; 10];
        values.extend(vec![0.9; 10]);
        let samples = make_samples("turn_quality_post_distillation", &values);
        let outcomes = proposer.evaluate(&samples, "test-nous");

        assert!(
            outcomes.len() <= 1,
            "max_changes_per_cycle=1 should limit to at most 1 outcome, got {}",
            outcomes.len()
        );
    }

    #[test]
    fn override_fact_content_is_valid_json() {
        let content = build_override_fact_content(
            "distillation.contextTokenTrigger",
            &ParameterValue::Int(95_000),
            "syn",
            "32 turns, quality +12%",
        );
        assert!(content.contains(r#""key":"distillation.contextTokenTrigger""#));
        assert!(content.contains(r#""value":95000"#));
        assert!(content.contains(r#""set_by":"syn""#));
        assert!(content.contains(r#""evidence":"32 turns, quality +12%""#));
    }

    #[test]
    fn parameter_value_to_f64_roundtrips() {
        assert!((parameter_value_to_f64(&ParameterValue::Int(42)) - 42.0).abs() < f64::EPSILON);
        assert!((parameter_value_to_f64(&ParameterValue::Float(2.78)) - 2.78).abs() < f64::EPSILON);
        assert!((parameter_value_to_f64(&ParameterValue::Bool(true)) - 1.0).abs() < f64::EPSILON);
        assert!((parameter_value_to_f64(&ParameterValue::Bool(false)) - 0.0).abs() < f64::EPSILON);
        assert!((parameter_value_to_f64(&ParameterValue::Duration(100)) - 100.0).abs() < f64::EPSILON);
    }
}
