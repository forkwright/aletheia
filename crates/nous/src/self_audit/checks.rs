//! Concrete prosoche check implementations for self-auditing.
//!
//! Five default checks (registered via [`super::SelfAuditor::register_defaults`]):
//! 1. `ToolSuccessRateCheck`: tool call success rate over recent actions
//! 2. `ResponseCoherenceCheck`: detects response-length drift (shortening over time)
//! 3. `CorrectionFrequencyCheck`: operator correction rate across recent sessions
//! 4. `MemoryUtilizationCheck`: knowledge-graph recall hit rate
//! 5. `SessionContinuityCheck`: context carry-forward and restatement detection
//!
//! Legacy checks (still usable but not in the default set):
//! - `KnowledgeConsistencyCheck`: knowledge graph integrity (temporal bounds, supersession chains)
//! - `ResponseQualityCheck`: response quality heuristics (length, empty responses)

use super::{CheckContext, CheckResult, CheckStatus, ProsocheCheck};

/// Minimum number of tool calls required before the success rate check is meaningful.
const MIN_TOOL_CALLS_FOR_RATE: usize = 5;

/// Tool success rate below this triggers a warning.
const TOOL_SUCCESS_WARN_THRESHOLD: f64 = 0.80;

/// Tool success rate below this triggers a failure.
const TOOL_SUCCESS_FAIL_THRESHOLD: f64 = 0.50;

// ---------------------------------------------------------------------------
// Legacy checks: retained for test coverage and manual registration.
// Not in the default five-check set.
// ---------------------------------------------------------------------------

/// Minimum number of responses required before quality check is meaningful.
#[cfg(test)]
const MIN_RESPONSES_FOR_QUALITY: usize = 3;

/// Response shorter than this (chars) is considered suspiciously short.
#[cfg(test)]
const SHORT_RESPONSE_THRESHOLD: usize = 10;

/// Fraction of short responses that triggers a warning.
#[cfg(test)]
const SHORT_RESPONSE_WARN_FRACTION: f64 = 0.30;

/// Fraction of short responses that triggers a failure.
#[cfg(test)]
const SHORT_RESPONSE_FAIL_FRACTION: f64 = 0.50;

/// Checks knowledge graph integrity: temporal bounds and supersession chain consistency.
#[cfg(test)]
pub(crate) struct KnowledgeConsistencyCheck;

#[cfg(test)]
impl ProsocheCheck for KnowledgeConsistencyCheck {
    fn name(&self) -> &'static str {
        "knowledge_consistency"
    }

    fn description(&self) -> &'static str {
        "Verifies knowledge graph integrity: valid temporal bounds and intact supersession chains"
    }

    fn run(&self, ctx: &CheckContext) -> CheckResult {
        if ctx.fact_count == 0 {
            return CheckResult {
                status: CheckStatus::Pass,
                score: 1.0,
                evidence: String::from("no facts in knowledge graph — nothing to check"),
            };
        }

        let total_violations = ctx
            .temporal_violation_count
            .saturating_add(ctx.broken_chain_count);

        if total_violations == 0 {
            return CheckResult {
                status: CheckStatus::Pass,
                score: 1.0,
                evidence: format!(
                    "{} facts checked, no integrity violations found",
                    ctx.fact_count,
                ),
            };
        }

        #[expect(
            clippy::as_conversions,
            clippy::cast_precision_loss,
            reason = "usize→f64: fact counts are far below f64 precision limits"
        )]
        let violation_rate = total_violations as f64 / ctx.fact_count as f64; // kanon:ignore RUST/as-cast
        let score = (1.0 - violation_rate).max(0.0);

        let evidence = format!(
            "{} facts checked: {} temporal violations, {} broken chains ({:.1}% violation rate)",
            ctx.fact_count,
            ctx.temporal_violation_count,
            ctx.broken_chain_count,
            violation_rate * 100.0,
        );

        if violation_rate > 0.10 {
            CheckResult {
                status: CheckStatus::Fail,
                score,
                evidence,
            }
        } else {
            CheckResult {
                status: CheckStatus::Warn,
                score,
                evidence,
            }
        }
    }
}

/// Checks tool call success rate over recent actions.
pub(crate) struct ToolSuccessRateCheck;

impl ProsocheCheck for ToolSuccessRateCheck {
    fn name(&self) -> &'static str {
        "tool_success_rate"
    }

    fn description(&self) -> &'static str {
        "Monitors tool call success rate; warns below 80%, fails below 50%"
    }

    fn run(&self, ctx: &CheckContext) -> CheckResult {
        let total = ctx.recent_tool_calls.len();
        if total < MIN_TOOL_CALLS_FOR_RATE {
            return CheckResult {
                status: CheckStatus::Pass,
                score: 1.0,
                evidence: format!(
                    "insufficient data: {total} tool calls (need at least {MIN_TOOL_CALLS_FOR_RATE})",
                ),
            };
        }

        let successes = ctx.recent_tool_calls.iter().filter(|r| r.success).count();
        #[expect(
            clippy::as_conversions,
            clippy::cast_precision_loss,
            reason = "usize→f64: tool call counts are far below f64 precision limits"
        )]
        let rate = successes as f64 / total as f64; // kanon:ignore RUST/as-cast

        let evidence = format!(
            "{successes}/{total} tool calls succeeded ({:.1}% success rate)",
            rate * 100.0,
        );

        if rate < TOOL_SUCCESS_FAIL_THRESHOLD {
            CheckResult {
                status: CheckStatus::Fail,
                score: rate,
                evidence,
            }
        } else if rate < TOOL_SUCCESS_WARN_THRESHOLD {
            CheckResult {
                status: CheckStatus::Warn,
                score: rate,
                evidence,
            }
        } else {
            CheckResult {
                status: CheckStatus::Pass,
                score: rate,
                evidence,
            }
        }
    }
}

/// Heuristic check on response quality: flags excessive short or empty responses.
#[cfg(test)]
pub(crate) struct ResponseQualityCheck;

#[cfg(test)]
impl ProsocheCheck for ResponseQualityCheck {
    fn name(&self) -> &'static str {
        "response_quality"
    }

    fn description(&self) -> &'static str {
        "Heuristic quality check: flags excessive short or empty responses"
    }

    fn run(&self, ctx: &CheckContext) -> CheckResult {
        let total = ctx.recent_response_lengths.len();
        if total < MIN_RESPONSES_FOR_QUALITY {
            return CheckResult {
                status: CheckStatus::Pass,
                score: 1.0,
                evidence: format!(
                    "insufficient data: {total} responses (need at least {MIN_RESPONSES_FOR_QUALITY})",
                ),
            };
        }

        let short_count = ctx
            .recent_response_lengths
            .iter()
            .filter(|&&len| len < SHORT_RESPONSE_THRESHOLD)
            .count();

        #[expect(
            clippy::as_conversions,
            clippy::cast_precision_loss,
            reason = "usize→f64: response counts are far below f64 precision limits"
        )]
        let short_fraction = short_count as f64 / total as f64; // kanon:ignore RUST/as-cast
        let score = (1.0 - short_fraction).max(0.0);

        let evidence = format!(
            "{short_count}/{total} responses shorter than {SHORT_RESPONSE_THRESHOLD} chars ({:.1}%)",
            short_fraction * 100.0,
        );

        if short_fraction >= SHORT_RESPONSE_FAIL_FRACTION {
            CheckResult {
                status: CheckStatus::Fail,
                score,
                evidence,
            }
        } else if short_fraction >= SHORT_RESPONSE_WARN_FRACTION {
            CheckResult {
                status: CheckStatus::Warn,
                score,
                evidence,
            }
        } else {
            CheckResult {
                status: CheckStatus::Pass,
                score,
                evidence,
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Check 2: Response coherence (drift detection)
// ---------------------------------------------------------------------------

/// Minimum number of responses before coherence drift is meaningful.
const MIN_RESPONSES_FOR_COHERENCE: usize = 6;

/// A response-length decline ratio exceeding this triggers a warning.
///
/// Computed as `1 - (mean_second_half / mean_first_half)`. A value of 0.40
/// means the second half of responses are 40% shorter on average.
const COHERENCE_DRIFT_WARN_THRESHOLD: f64 = 0.40;

/// A response-length decline ratio exceeding this triggers a failure.
const COHERENCE_DRIFT_FAIL_THRESHOLD: f64 = 0.60;

/// Detects response-length drift: are responses getting progressively shorter?
///
/// Splits recent responses into first and second halves, compares mean lengths.
/// A significant decline signals coherence degradation (the nous is giving up,
/// looping, or losing context).
pub(crate) struct ResponseCoherenceCheck;

impl ProsocheCheck for ResponseCoherenceCheck {
    fn name(&self) -> &'static str {
        "response_coherence"
    }

    fn description(&self) -> &'static str {
        "Detects response-length drift; warns when later responses are significantly shorter than earlier ones"
    }

    fn run(&self, ctx: &CheckContext) -> CheckResult {
        let total = ctx.recent_response_lengths.len();
        if total < MIN_RESPONSES_FOR_COHERENCE {
            return CheckResult {
                status: CheckStatus::Pass,
                score: 1.0,
                evidence: format!(
                    "insufficient data: {total} responses (need at least {MIN_RESPONSES_FOR_COHERENCE})",
                ),
            };
        }

        let mid = total / 2;
        let (first_half, second_half) = ctx.recent_response_lengths.split_at(mid);

        let mean_first = arithmetic_mean(first_half);
        let mean_second = arithmetic_mean(second_half);

        // Guard against division by zero when all early responses are empty.
        if mean_first < f64::EPSILON {
            return CheckResult {
                status: CheckStatus::Pass,
                score: 1.0,
                evidence: String::from("first-half mean is zero; no drift measurable"),
            };
        }

        // Positive drift_ratio means responses are getting shorter.
        let drift_ratio = 1.0 - (mean_second / mean_first);
        let score = (1.0 - drift_ratio.max(0.0)).clamp(0.0, 1.0);

        let evidence = format!(
            "mean response length: first half {mean_first:.0} chars, second half {mean_second:.0} chars \
             (drift ratio {drift_ratio:.2})",
        );

        if drift_ratio >= COHERENCE_DRIFT_FAIL_THRESHOLD {
            CheckResult {
                status: CheckStatus::Fail,
                score,
                evidence,
            }
        } else if drift_ratio >= COHERENCE_DRIFT_WARN_THRESHOLD {
            CheckResult {
                status: CheckStatus::Warn,
                score,
                evidence,
            }
        } else {
            CheckResult {
                status: CheckStatus::Pass,
                score,
                evidence,
            }
        }
    }
}

/// Compute the arithmetic mean of a slice of `usize` values.
#[expect(
    clippy::as_conversions,
    clippy::cast_precision_loss,
    reason = "usize→f64: response lengths are far below f64 precision limits"
)]
fn arithmetic_mean(values: &[usize]) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    let sum: usize = values.iter().sum();
    sum as f64 / values.len() as f64 // kanon:ignore RUST/as-cast
}

// ---------------------------------------------------------------------------
// Check 3: Correction frequency
// ---------------------------------------------------------------------------

/// Minimum number of turns before correction rate is meaningful.
const MIN_TURNS_FOR_CORRECTION: usize = 10;

/// Correction rate above this triggers a warning.
const CORRECTION_WARN_THRESHOLD: f64 = 0.15;

/// Correction rate above this triggers a failure.
const CORRECTION_FAIL_THRESHOLD: f64 = 0.30;

/// Monitors how often the operator corrects the nous.
///
/// A high correction rate indicates the nous is frequently wrong, misaligned,
/// or misunderstanding operator intent.
pub(crate) struct CorrectionFrequencyCheck;

impl ProsocheCheck for CorrectionFrequencyCheck {
    fn name(&self) -> &'static str {
        "correction_frequency"
    }

    fn description(&self) -> &'static str {
        "Monitors operator correction rate; warns above 15%, fails above 30%"
    }

    fn run(&self, ctx: &CheckContext) -> CheckResult {
        let total_turns = ctx.total_turns_in_window;
        if total_turns < MIN_TURNS_FOR_CORRECTION {
            return CheckResult {
                status: CheckStatus::Pass,
                score: 1.0,
                evidence: format!(
                    "insufficient data: {total_turns} turns (need at least {MIN_TURNS_FOR_CORRECTION})",
                ),
            };
        }

        let corrections = ctx.recent_corrections.len();
        #[expect(
            clippy::as_conversions,
            clippy::cast_precision_loss,
            reason = "usize→f64: turn/correction counts are far below f64 precision limits"
        )]
        let rate = corrections as f64 / total_turns as f64; // kanon:ignore RUST/as-cast
        let score = (1.0 - rate).clamp(0.0, 1.0);

        let evidence = format!(
            "{corrections}/{total_turns} turns contained operator corrections ({:.1}%)",
            rate * 100.0,
        );

        if rate >= CORRECTION_FAIL_THRESHOLD {
            CheckResult {
                status: CheckStatus::Fail,
                score,
                evidence,
            }
        } else if rate >= CORRECTION_WARN_THRESHOLD {
            CheckResult {
                status: CheckStatus::Warn,
                score,
                evidence,
            }
        } else {
            CheckResult {
                status: CheckStatus::Pass,
                score,
                evidence,
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Check 4: Memory utilization
// ---------------------------------------------------------------------------

/// Minimum recall attempts before the check is meaningful.
const MIN_RECALL_ATTEMPTS: usize = 5;

/// Recall hit rate below this triggers a warning.
const MEMORY_WARN_THRESHOLD: f64 = 0.30;

/// Recall hit rate below this triggers a failure.
const MEMORY_FAIL_THRESHOLD: f64 = 0.10;

/// Checks whether knowledge-graph recall is producing useful results.
///
/// A low hit rate means the nous is querying its memory but getting nothing
/// back, indicating stale/irrelevant facts or poor query formulation.
pub(crate) struct MemoryUtilizationCheck;

impl ProsocheCheck for MemoryUtilizationCheck {
    fn name(&self) -> &'static str {
        "memory_utilization"
    }

    fn description(&self) -> &'static str {
        "Monitors knowledge-graph recall hit rate; warns below 30%, fails below 10%"
    }

    fn run(&self, ctx: &CheckContext) -> CheckResult {
        let attempts = ctx.memory_recall.recall_attempts;
        if attempts < MIN_RECALL_ATTEMPTS {
            return CheckResult {
                status: CheckStatus::Pass,
                score: 1.0,
                evidence: format!(
                    "insufficient data: {attempts} recall attempts (need at least {MIN_RECALL_ATTEMPTS})",
                ),
            };
        }

        let hits = ctx.memory_recall.recall_hits;
        #[expect(
            clippy::as_conversions,
            clippy::cast_precision_loss,
            reason = "usize→f64: recall counts are far below f64 precision limits"
        )]
        let hit_rate = hits as f64 / attempts as f64; // kanon:ignore RUST/as-cast

        let evidence = format!(
            "{hits}/{attempts} recall attempts returned relevant facts ({:.1}% hit rate)",
            hit_rate * 100.0,
        );

        if hit_rate < MEMORY_FAIL_THRESHOLD {
            CheckResult {
                status: CheckStatus::Fail,
                score: hit_rate,
                evidence,
            }
        } else if hit_rate < MEMORY_WARN_THRESHOLD {
            CheckResult {
                status: CheckStatus::Warn,
                score: hit_rate,
                evidence,
            }
        } else {
            CheckResult {
                status: CheckStatus::Pass,
                score: hit_rate,
                evidence,
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Check 5: Session continuity
// ---------------------------------------------------------------------------

/// Minimum turns before session continuity is meaningful.
const MIN_TURNS_FOR_CONTINUITY: usize = 8;

/// Context-carry rate below this triggers a warning.
const CONTINUITY_CARRY_WARN_THRESHOLD: f64 = 0.25;

/// Context-carry rate below this triggers a failure.
const CONTINUITY_CARRY_FAIL_THRESHOLD: f64 = 0.10;

/// Restatement rate above this triggers a warning (operator re-explaining).
const CONTINUITY_RESTATEMENT_WARN_THRESHOLD: f64 = 0.20;

/// Restatement rate above this triggers a failure.
const CONTINUITY_RESTATEMENT_FAIL_THRESHOLD: f64 = 0.35;

/// Checks whether the nous maintains context across turns.
///
/// Two sub-signals:
/// 1. **Context carry**: fraction of turns that reference prior conversation
///    context. Low carry means the nous is treating each turn in isolation.
/// 2. **Restatement rate**: fraction of turns where the operator had to repeat
///    themselves. High restatement means the nous is losing thread.
///
/// The worse of the two sub-signals determines the overall status.
pub(crate) struct SessionContinuityCheck;

impl ProsocheCheck for SessionContinuityCheck {
    fn name(&self) -> &'static str {
        "session_continuity"
    }

    fn description(&self) -> &'static str {
        "Monitors context carry-forward and operator restatements to detect thread loss"
    }

    fn run(&self, ctx: &CheckContext) -> CheckResult {
        let total = ctx.session_continuity.total_turns;
        if total < MIN_TURNS_FOR_CONTINUITY {
            return CheckResult {
                status: CheckStatus::Pass,
                score: 1.0,
                evidence: format!(
                    "insufficient data: {total} turns (need at least {MIN_TURNS_FOR_CONTINUITY})",
                ),
            };
        }

        #[expect(
            clippy::as_conversions,
            clippy::cast_precision_loss,
            reason = "usize→f64: turn counts are far below f64 precision limits"
        )]
        let carry_rate =
            ctx.session_continuity.context_carry_turns as f64 / total as f64; // kanon:ignore RUST/as-cast
        #[expect(
            clippy::as_conversions,
            clippy::cast_precision_loss,
            reason = "usize→f64: turn counts are far below f64 precision limits"
        )]
        let restatement_rate =
            ctx.session_continuity.restatement_count as f64 / total as f64; // kanon:ignore RUST/as-cast

        let carry_status = if carry_rate < CONTINUITY_CARRY_FAIL_THRESHOLD {
            CheckStatus::Fail
        } else if carry_rate < CONTINUITY_CARRY_WARN_THRESHOLD {
            CheckStatus::Warn
        } else {
            CheckStatus::Pass
        };

        let restatement_status = if restatement_rate >= CONTINUITY_RESTATEMENT_FAIL_THRESHOLD {
            CheckStatus::Fail
        } else if restatement_rate >= CONTINUITY_RESTATEMENT_WARN_THRESHOLD {
            CheckStatus::Warn
        } else {
            CheckStatus::Pass
        };

        // Take the worse of the two signals.
        let status = worse_status(carry_status, restatement_status);

        // Score blends both signals: carry is good (higher=better), restatement is bad.
        let score = f64::midpoint(carry_rate, 1.0 - restatement_rate).clamp(0.0, 1.0);

        let evidence = format!(
            "{}/{total} turns carried context ({:.1}%), {}/{total} required restatement ({:.1}%)",
            ctx.session_continuity.context_carry_turns,
            carry_rate * 100.0,
            ctx.session_continuity.restatement_count,
            restatement_rate * 100.0,
        );

        CheckResult {
            status,
            score,
            evidence,
        }
    }
}

/// Return the worse of two check statuses (Fail > Warn > Pass).
const fn worse_status(a: CheckStatus, b: CheckStatus) -> CheckStatus {
    match (a, b) {
        (CheckStatus::Fail, _) | (_, CheckStatus::Fail) => CheckStatus::Fail,
        (CheckStatus::Warn, _) | (_, CheckStatus::Warn) => CheckStatus::Warn,
        _ => CheckStatus::Pass,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::self_audit::{CorrectionRecord, MemoryRecallStats, SessionContinuityStats, ToolCallRecord};

    // --- KnowledgeConsistencyCheck ---

    #[test]
    fn knowledge_consistency_passes_with_no_facts() {
        let check = KnowledgeConsistencyCheck;
        let ctx = CheckContext::default();
        let result = check.run(&ctx);
        assert_eq!(result.status, CheckStatus::Pass);
        assert!(
            (result.score - 1.0).abs() < f64::EPSILON,
            "empty graph should score 1.0"
        );
    }

    #[test]
    fn knowledge_consistency_passes_with_clean_graph() {
        let check = KnowledgeConsistencyCheck;
        let ctx = CheckContext {
            fact_count: 100,
            temporal_violation_count: 0,
            broken_chain_count: 0,
            ..Default::default()
        };
        let result = check.run(&ctx);
        assert_eq!(result.status, CheckStatus::Pass);
    }

    #[test]
    fn knowledge_consistency_warns_on_minor_violations() {
        let check = KnowledgeConsistencyCheck;
        let ctx = CheckContext {
            fact_count: 100,
            temporal_violation_count: 3,
            broken_chain_count: 2,
            ..Default::default()
        };
        let result = check.run(&ctx);
        assert_eq!(result.status, CheckStatus::Warn);
        assert!(result.score < 1.0, "violations should reduce score");
    }

    #[test]
    fn knowledge_consistency_fails_on_high_violation_rate() {
        let check = KnowledgeConsistencyCheck;
        let ctx = CheckContext {
            fact_count: 100,
            temporal_violation_count: 8,
            broken_chain_count: 5,
            ..Default::default()
        };
        let result = check.run(&ctx);
        assert_eq!(result.status, CheckStatus::Fail);
    }

    // --- ToolSuccessRateCheck ---

    #[test]
    fn tool_success_passes_with_insufficient_data() {
        let check = ToolSuccessRateCheck;
        let ctx = CheckContext {
            recent_tool_calls: vec![
                ToolCallRecord {
                    tool_name: String::from("read"),
                    success: true,
                },
                ToolCallRecord {
                    tool_name: String::from("write"),
                    success: false,
                },
            ],
            ..Default::default()
        };
        let result = check.run(&ctx);
        assert_eq!(result.status, CheckStatus::Pass);
        assert!(
            result.evidence.contains("insufficient"),
            "should mention insufficient data"
        );
    }

    #[test]
    fn tool_success_passes_with_high_rate() {
        let check = ToolSuccessRateCheck;
        let calls: Vec<ToolCallRecord> = (0..10)
            .map(|i| ToolCallRecord {
                tool_name: format!("tool_{i}"),
                success: i < 9, // 90% success
            })
            .collect();
        let ctx = CheckContext {
            recent_tool_calls: calls,
            ..Default::default()
        };
        let result = check.run(&ctx);
        assert_eq!(result.status, CheckStatus::Pass);
    }

    #[test]
    fn tool_success_warns_on_moderate_failures() {
        let check = ToolSuccessRateCheck;
        let calls: Vec<ToolCallRecord> = (0..10)
            .map(|i| ToolCallRecord {
                tool_name: format!("tool_{i}"),
                success: i < 7, // 70% success
            })
            .collect();
        let ctx = CheckContext {
            recent_tool_calls: calls,
            ..Default::default()
        };
        let result = check.run(&ctx);
        assert_eq!(result.status, CheckStatus::Warn);
    }

    #[test]
    fn tool_success_fails_on_low_rate() {
        let check = ToolSuccessRateCheck;
        let calls: Vec<ToolCallRecord> = (0..10)
            .map(|i| ToolCallRecord {
                tool_name: format!("tool_{i}"),
                success: i < 3, // 30% success
            })
            .collect();
        let ctx = CheckContext {
            recent_tool_calls: calls,
            ..Default::default()
        };
        let result = check.run(&ctx);
        assert_eq!(result.status, CheckStatus::Fail);
    }

    // --- ResponseQualityCheck ---

    #[test]
    fn response_quality_passes_with_insufficient_data() {
        let check = ResponseQualityCheck;
        let ctx = CheckContext {
            recent_response_lengths: vec![100, 200],
            ..Default::default()
        };
        let result = check.run(&ctx);
        assert_eq!(result.status, CheckStatus::Pass);
    }

    #[test]
    fn response_quality_passes_with_good_lengths() {
        let check = ResponseQualityCheck;
        let ctx = CheckContext {
            recent_response_lengths: vec![100, 200, 150, 300, 250],
            ..Default::default()
        };
        let result = check.run(&ctx);
        assert_eq!(result.status, CheckStatus::Pass);
    }

    #[test]
    fn response_quality_warns_on_many_short_responses() {
        let check = ResponseQualityCheck;
        // 4/10 = 40% short responses (above 30% threshold)
        let ctx = CheckContext {
            recent_response_lengths: vec![5, 3, 100, 200, 2, 150, 300, 250, 1, 400],
            ..Default::default()
        };
        let result = check.run(&ctx);
        assert_eq!(result.status, CheckStatus::Warn);
    }

    #[test]
    fn response_quality_fails_on_majority_short() {
        let check = ResponseQualityCheck;
        // 6/10 = 60% short responses (above 50% threshold)
        let ctx = CheckContext {
            recent_response_lengths: vec![1, 2, 3, 4, 5, 6, 100, 200, 300, 400],
            ..Default::default()
        };
        let result = check.run(&ctx);
        assert_eq!(result.status, CheckStatus::Fail);
    }

    // --- ResponseCoherenceCheck ---

    #[test]
    fn coherence_passes_with_insufficient_data() {
        let check = ResponseCoherenceCheck;
        let ctx = CheckContext {
            recent_response_lengths: vec![100, 200, 300],
            ..Default::default()
        };
        let result = check.run(&ctx);
        assert_eq!(result.status, CheckStatus::Pass);
        assert!(result.evidence.contains("insufficient"));
    }

    #[test]
    fn coherence_passes_with_stable_lengths() {
        let check = ResponseCoherenceCheck;
        let ctx = CheckContext {
            recent_response_lengths: vec![200, 210, 190, 200, 205, 195],
            ..Default::default()
        };
        let result = check.run(&ctx);
        assert_eq!(result.status, CheckStatus::Pass);
    }

    #[test]
    fn coherence_warns_on_moderate_drift() {
        let check = ResponseCoherenceCheck;
        // First half mean ~300, second half mean ~150 => drift ~0.50
        let ctx = CheckContext {
            recent_response_lengths: vec![300, 300, 300, 150, 150, 150],
            ..Default::default()
        };
        let result = check.run(&ctx);
        assert_eq!(result.status, CheckStatus::Warn);
    }

    #[test]
    fn coherence_fails_on_severe_drift() {
        let check = ResponseCoherenceCheck;
        // First half mean ~400, second half mean ~50 => drift ~0.875
        let ctx = CheckContext {
            recent_response_lengths: vec![400, 400, 400, 50, 50, 50],
            ..Default::default()
        };
        let result = check.run(&ctx);
        assert_eq!(result.status, CheckStatus::Fail);
    }

    #[test]
    fn coherence_passes_when_responses_get_longer() {
        let check = ResponseCoherenceCheck;
        // Getting longer is not a drift signal.
        let ctx = CheckContext {
            recent_response_lengths: vec![100, 100, 100, 300, 300, 300],
            ..Default::default()
        };
        let result = check.run(&ctx);
        assert_eq!(result.status, CheckStatus::Pass);
        assert!(result.score >= 1.0 - f64::EPSILON, "lengthening should score 1.0");
    }

    // --- CorrectionFrequencyCheck ---

    #[test]
    fn correction_passes_with_insufficient_data() {
        let check = CorrectionFrequencyCheck;
        let ctx = CheckContext {
            total_turns_in_window: 5,
            recent_corrections: vec![CorrectionRecord {
                session_id: String::from("s1"),
                turn_number: 3,
            }],
            ..Default::default()
        };
        let result = check.run(&ctx);
        assert_eq!(result.status, CheckStatus::Pass);
        assert!(result.evidence.contains("insufficient"));
    }

    #[test]
    fn correction_passes_with_low_rate() {
        let check = CorrectionFrequencyCheck;
        let ctx = CheckContext {
            total_turns_in_window: 100,
            recent_corrections: vec![CorrectionRecord {
                session_id: String::from("s1"),
                turn_number: 5,
            }],
            ..Default::default()
        };
        let result = check.run(&ctx);
        assert_eq!(result.status, CheckStatus::Pass);
    }

    #[test]
    fn correction_warns_on_moderate_rate() {
        let check = CorrectionFrequencyCheck;
        // 2/10 = 20% correction rate (above 15% warn threshold)
        let corrections: Vec<CorrectionRecord> = (0..2)
            .map(|i| CorrectionRecord {
                session_id: String::from("s1"),
                turn_number: i,
            })
            .collect();
        let ctx = CheckContext {
            total_turns_in_window: 10,
            recent_corrections: corrections,
            ..Default::default()
        };
        let result = check.run(&ctx);
        assert_eq!(result.status, CheckStatus::Warn);
    }

    #[test]
    fn correction_fails_on_high_rate() {
        let check = CorrectionFrequencyCheck;
        // 4/10 = 40% correction rate (above 30% fail threshold)
        let corrections: Vec<CorrectionRecord> = (0..4)
            .map(|i| CorrectionRecord {
                session_id: String::from("s1"),
                turn_number: i,
            })
            .collect();
        let ctx = CheckContext {
            total_turns_in_window: 10,
            recent_corrections: corrections,
            ..Default::default()
        };
        let result = check.run(&ctx);
        assert_eq!(result.status, CheckStatus::Fail);
    }

    // --- MemoryUtilizationCheck ---

    #[test]
    fn memory_passes_with_insufficient_data() {
        let check = MemoryUtilizationCheck;
        let ctx = CheckContext {
            memory_recall: MemoryRecallStats {
                recall_attempts: 3,
                recall_hits: 1,
            },
            ..Default::default()
        };
        let result = check.run(&ctx);
        assert_eq!(result.status, CheckStatus::Pass);
        assert!(result.evidence.contains("insufficient"));
    }

    #[test]
    fn memory_passes_with_high_hit_rate() {
        let check = MemoryUtilizationCheck;
        let ctx = CheckContext {
            memory_recall: MemoryRecallStats {
                recall_attempts: 10,
                recall_hits: 8,
            },
            ..Default::default()
        };
        let result = check.run(&ctx);
        assert_eq!(result.status, CheckStatus::Pass);
    }

    #[test]
    fn memory_warns_on_low_hit_rate() {
        let check = MemoryUtilizationCheck;
        // 2/10 = 20% hit rate (below 30% warn threshold, above 10% fail)
        let ctx = CheckContext {
            memory_recall: MemoryRecallStats {
                recall_attempts: 10,
                recall_hits: 2,
            },
            ..Default::default()
        };
        let result = check.run(&ctx);
        assert_eq!(result.status, CheckStatus::Warn);
    }

    #[test]
    fn memory_fails_on_very_low_hit_rate() {
        let check = MemoryUtilizationCheck;
        // 0/10 = 0% hit rate (below 10% fail threshold)
        let ctx = CheckContext {
            memory_recall: MemoryRecallStats {
                recall_attempts: 10,
                recall_hits: 0,
            },
            ..Default::default()
        };
        let result = check.run(&ctx);
        assert_eq!(result.status, CheckStatus::Fail);
    }

    // --- SessionContinuityCheck ---

    #[test]
    fn continuity_passes_with_insufficient_data() {
        let check = SessionContinuityCheck;
        let ctx = CheckContext {
            session_continuity: SessionContinuityStats {
                total_turns: 5,
                context_carry_turns: 3,
                restatement_count: 0,
            },
            ..Default::default()
        };
        let result = check.run(&ctx);
        assert_eq!(result.status, CheckStatus::Pass);
        assert!(result.evidence.contains("insufficient"));
    }

    #[test]
    fn continuity_passes_with_good_signals() {
        let check = SessionContinuityCheck;
        let ctx = CheckContext {
            session_continuity: SessionContinuityStats {
                total_turns: 20,
                context_carry_turns: 15, // 75% carry
                restatement_count: 1,    // 5% restatement
            },
            ..Default::default()
        };
        let result = check.run(&ctx);
        assert_eq!(result.status, CheckStatus::Pass);
    }

    #[test]
    fn continuity_warns_on_low_carry() {
        let check = SessionContinuityCheck;
        // 2/10 = 20% carry (below 25% warn threshold, above 10% fail)
        let ctx = CheckContext {
            session_continuity: SessionContinuityStats {
                total_turns: 10,
                context_carry_turns: 2,
                restatement_count: 0,
            },
            ..Default::default()
        };
        let result = check.run(&ctx);
        assert_eq!(result.status, CheckStatus::Warn);
    }

    #[test]
    fn continuity_warns_on_high_restatement() {
        let check = SessionContinuityCheck;
        // 3/10 = 30% restatement (above 20% warn, below 35% fail)
        let ctx = CheckContext {
            session_continuity: SessionContinuityStats {
                total_turns: 10,
                context_carry_turns: 8, // carry is fine
                restatement_count: 3,
            },
            ..Default::default()
        };
        let result = check.run(&ctx);
        assert_eq!(result.status, CheckStatus::Warn);
    }

    #[test]
    fn continuity_fails_on_both_bad_signals() {
        let check = SessionContinuityCheck;
        // 0/10 carry (fail) and 4/10 restatement (fail)
        let ctx = CheckContext {
            session_continuity: SessionContinuityStats {
                total_turns: 10,
                context_carry_turns: 0,
                restatement_count: 4,
            },
            ..Default::default()
        };
        let result = check.run(&ctx);
        assert_eq!(result.status, CheckStatus::Fail);
    }

    // --- worse_status ---

    #[test]
    fn worse_status_returns_fail_when_either_fails() {
        assert_eq!(worse_status(CheckStatus::Fail, CheckStatus::Pass), CheckStatus::Fail);
        assert_eq!(worse_status(CheckStatus::Pass, CheckStatus::Fail), CheckStatus::Fail);
        assert_eq!(worse_status(CheckStatus::Fail, CheckStatus::Warn), CheckStatus::Fail);
    }

    #[test]
    fn worse_status_returns_warn_when_either_warns() {
        assert_eq!(worse_status(CheckStatus::Warn, CheckStatus::Pass), CheckStatus::Warn);
        assert_eq!(worse_status(CheckStatus::Pass, CheckStatus::Warn), CheckStatus::Warn);
    }

    #[test]
    fn worse_status_returns_pass_when_both_pass() {
        assert_eq!(worse_status(CheckStatus::Pass, CheckStatus::Pass), CheckStatus::Pass);
    }

    // --- Trait conformance ---

    #[test]
    fn all_checks_implement_trait() {
        let checks: Vec<Box<dyn ProsocheCheck>> = vec![
            Box::new(KnowledgeConsistencyCheck),
            Box::new(ToolSuccessRateCheck),
            Box::new(ResponseQualityCheck),
            Box::new(ResponseCoherenceCheck),
            Box::new(CorrectionFrequencyCheck),
            Box::new(MemoryUtilizationCheck),
            Box::new(SessionContinuityCheck),
        ];
        let ctx = CheckContext::default();
        for check in &checks {
            assert!(!check.name().is_empty(), "check name should not be empty");
            assert!(
                !check.description().is_empty(),
                "check description should not be empty"
            );
            let result = check.run(&ctx);
            assert!(
                (0.0..=1.0).contains(&result.score),
                "score should be between 0.0 and 1.0"
            );
        }
    }

    // --- arithmetic_mean ---

    #[test]
    fn arithmetic_mean_empty() {
        assert!((arithmetic_mean(&[]) - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn arithmetic_mean_single() {
        assert!((arithmetic_mean(&[42]) - 42.0).abs() < f64::EPSILON);
    }

    #[test]
    fn arithmetic_mean_multiple() {
        assert!((arithmetic_mean(&[10, 20, 30]) - 20.0).abs() < f64::EPSILON);
    }
}
