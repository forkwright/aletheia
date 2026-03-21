//! Concrete prosoche check implementations for Chiron self-auditing.
//!
//! Three default checks:
//! - `KnowledgeConsistencyCheck`: knowledge graph integrity (temporal bounds, supersession chains)
//! - `ToolSuccessRateCheck`: tool call success rate over recent actions
//! - `ResponseQualityCheck`: response quality heuristics (length, empty responses)

use super::{CheckContext, CheckResult, CheckStatus, ProsocheCheck};

/// Minimum number of tool calls required before the success rate check is meaningful.
const MIN_TOOL_CALLS_FOR_RATE: usize = 5;

/// Tool success rate below this triggers a warning.
const TOOL_SUCCESS_WARN_THRESHOLD: f64 = 0.80;

/// Tool success rate below this triggers a failure.
const TOOL_SUCCESS_FAIL_THRESHOLD: f64 = 0.50;

/// Minimum number of responses required before quality check is meaningful.
const MIN_RESPONSES_FOR_QUALITY: usize = 3;

/// Response shorter than this (chars) is considered suspiciously short.
const SHORT_RESPONSE_THRESHOLD: usize = 10;

/// Fraction of short responses that triggers a warning.
const SHORT_RESPONSE_WARN_FRACTION: f64 = 0.30;

/// Fraction of short responses that triggers a failure.
const SHORT_RESPONSE_FAIL_FRACTION: f64 = 0.50;

/// Checks knowledge graph integrity: temporal bounds and supersession chain consistency.
pub(crate) struct KnowledgeConsistencyCheck;

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
pub(crate) struct ResponseQualityCheck;

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chiron::ToolCallRecord;

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

    // --- Trait conformance ---

    #[test]
    fn all_checks_implement_trait() {
        let checks: Vec<Box<dyn ProsocheCheck>> = vec![
            Box::new(KnowledgeConsistencyCheck),
            Box::new(ToolSuccessRateCheck),
            Box::new(ResponseQualityCheck),
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
}
