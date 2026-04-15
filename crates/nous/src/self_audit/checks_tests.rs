use super::*;
use crate::self_audit::{
    CorrectionRecord, MemoryRecallStats, SessionContinuityStats, ToolCallRecord,
};

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
    assert!(
        result.score >= 1.0 - f64::EPSILON,
        "lengthening should score 1.0"
    );
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
    assert_eq!(
        worse_status(CheckStatus::Fail, CheckStatus::Pass),
        CheckStatus::Fail
    );
    assert_eq!(
        worse_status(CheckStatus::Pass, CheckStatus::Fail),
        CheckStatus::Fail
    );
    assert_eq!(
        worse_status(CheckStatus::Fail, CheckStatus::Warn),
        CheckStatus::Fail
    );
}

#[test]
fn worse_status_returns_warn_when_either_warns() {
    assert_eq!(
        worse_status(CheckStatus::Warn, CheckStatus::Pass),
        CheckStatus::Warn
    );
    assert_eq!(
        worse_status(CheckStatus::Pass, CheckStatus::Warn),
        CheckStatus::Warn
    );
}

#[test]
fn worse_status_returns_pass_when_both_pass() {
    assert_eq!(
        worse_status(CheckStatus::Pass, CheckStatus::Pass),
        CheckStatus::Pass
    );
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
