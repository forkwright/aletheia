#![expect(clippy::expect_used, reason = "test assertions")]
#![expect(clippy::unwrap_used, reason = "test assertions")]
#![expect(clippy::float_cmp, reason = "test assertions")]
#![expect(clippy::indexing_slicing, reason = "test assertions")]
#![expect(
    unused_imports,
    reason = "split public_api_*.rs share the same import block"
)]

use std::time::Duration;

use energeia::cost_ledger::CostLedger;
use energeia::engine::{AgentOptions, SessionEvent};
use energeia::error::Error;
use energeia::orchestrator::OrchestratorConfig;
use energeia::types::{
    Budget, BudgetStatus, CriterionType, DispatchSpec, MechanicalIssueKind, QaVerdict,
    ResumePolicy, SessionStatus,
};

#[test]
fn error_is_send_sync() {
    const _: fn() = || {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<Error>();
    };
}

// ── Send + Sync bounds for other types ──

#[test]
fn cost_ledger_is_send_sync() {
    const _: fn() = || {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<CostLedger>();
    };
}

#[test]
fn dispatch_spec_is_send_sync() {
    const _: fn() = || {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<DispatchSpec>();
    };
}

#[test]
fn agent_options_is_send_sync() {
    const _: fn() = || {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<AgentOptions>();
    };
}

#[test]
fn orchestrator_config_is_send_sync() {
    const _: fn() = || {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<OrchestratorConfig>();
    };
}

#[test]
fn budget_is_send_sync() {
    const _: fn() = || {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<Budget>();
    };
}

#[test]
fn budget_status_is_send_sync() {
    const _: fn() = || {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<BudgetStatus>();
    };
}

#[test]
fn session_status_is_send_sync() {
    const _: fn() = || {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<SessionStatus>();
    };
}

#[test]
fn qa_verdict_is_send_sync() {
    const _: fn() = || {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<QaVerdict>();
    };
}

#[test]
fn resume_policy_is_send_sync() {
    const _: fn() = || {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<ResumePolicy>();
    };
}

#[test]
fn criterion_type_is_send_sync() {
    const _: fn() = || {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<CriterionType>();
    };
}

#[test]
fn mechanical_issue_kind_is_send_sync() {
    const _: fn() = || {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<MechanicalIssueKind>();
    };
}

#[test]
fn session_event_is_send_sync() {
    const _: fn() = || {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<SessionEvent>();
    };
}

// ── Additional type tests ──

#[test]
fn session_status_display() {
    assert_eq!(SessionStatus::Success.to_string(), "success");
    assert_eq!(SessionStatus::Failed.to_string(), "failed");
    assert_eq!(SessionStatus::Stuck.to_string(), "stuck");
    assert_eq!(SessionStatus::Aborted.to_string(), "aborted");
    assert_eq!(SessionStatus::BudgetExceeded.to_string(), "budget_exceeded");
    assert_eq!(SessionStatus::Skipped.to_string(), "skipped");
    assert_eq!(SessionStatus::InfraFailure.to_string(), "infra_failure");
}

#[test]
fn qa_verdict_display() {
    assert_eq!(QaVerdict::Pass.to_string(), "pass");
    assert_eq!(QaVerdict::Partial.to_string(), "partial");
    assert_eq!(QaVerdict::Fail.to_string(), "fail");
}

#[test]
fn mechanical_issue_kind_display() {
    assert_eq!(
        MechanicalIssueKind::BlastRadiusViolation.to_string(),
        "blast_radius_violation"
    );
    assert_eq!(MechanicalIssueKind::AntiPattern.to_string(), "anti_pattern");
    assert_eq!(
        MechanicalIssueKind::LintViolation.to_string(),
        "lint_violation"
    );
    assert_eq!(
        MechanicalIssueKind::FormatViolation.to_string(),
        "format_violation"
    );
}

#[test]
fn session_event_serde_roundtrip() {
    let event = SessionEvent::TextDelta {
        text: "Hello, world!".to_owned(),
    };
    let json = serde_json::to_string(&event).expect("serialize");
    let deserialized: SessionEvent = serde_json::from_str(&json).expect("deserialize");
    assert!(matches!(deserialized, SessionEvent::TextDelta { text } if text == "Hello, world!"));

    let event = SessionEvent::ToolUse {
        name: "read_file".to_owned(),
        input: serde_json::json!({"path": "/tmp/test.rs"}),
    };
    let json = serde_json::to_string(&event).expect("serialize");
    let deserialized: SessionEvent = serde_json::from_str(&json).expect("deserialize");
    assert!(matches!(deserialized, SessionEvent::ToolUse { name, .. } if name == "read_file"));
}

#[test]
fn session_event_variants_serde() {
    // Test ToolResult variant
    let event = SessionEvent::ToolResult {
        name: "bash".to_owned(),
        success: true,
    };
    let json = serde_json::to_string(&event).expect("serialize");
    let deserialized: SessionEvent = serde_json::from_str(&json).expect("deserialize");
    assert!(
        matches!(deserialized, SessionEvent::ToolResult { name, success } if name == "bash" && success)
    );

    // Test TurnComplete variant
    let event = SessionEvent::TurnComplete { turn: 5 };
    let json = serde_json::to_string(&event).expect("serialize");
    let deserialized: SessionEvent = serde_json::from_str(&json).expect("deserialize");
    assert!(matches!(
        deserialized,
        SessionEvent::TurnComplete { turn: 5 }
    ));

    // Test Error variant
    let event = SessionEvent::Error {
        message: "something went wrong".to_owned(),
    };
    let json = serde_json::to_string(&event).expect("serialize");
    let deserialized: SessionEvent = serde_json::from_str(&json).expect("deserialize");
    assert!(
        matches!(deserialized, SessionEvent::Error { message } if message == "something went wrong")
    );
}

#[test]
fn budget_semantics() {
    let budget = Budget::new(Some(10.0), Some(100), None);

    assert!((budget.current_cost_usd()).abs() < f64::EPSILON);
    assert_eq!(budget.current_turns(), 0);
    assert_eq!(budget.check(), BudgetStatus::Ok);

    budget.record(5.0, 50);
    assert!((budget.current_cost_usd() - 5.0).abs() < 0.01);
    assert_eq!(budget.current_turns(), 50);
    assert_eq!(budget.check(), BudgetStatus::Ok);

    // At 80% cost threshold should warn
    budget.record(3.5, 0);
    assert!(matches!(budget.check(), BudgetStatus::Warning(_)));

    // Exceed cost limit
    budget.record(2.0, 0);
    assert!(matches!(budget.check(), BudgetStatus::Exceeded(_)));
}

#[test]
fn budget_cost_fraction() {
    let budget = Budget::new(Some(100.0), None, None);
    assert!((budget.cost_fraction()).abs() < f64::EPSILON);

    budget.record(25.0, 0);
    assert!((budget.cost_fraction() - 0.25).abs() < 0.01);

    budget.record(75.0, 0);
    assert!((budget.cost_fraction() - 1.0).abs() < 0.01);

    // Over budget
    budget.record(10.0, 0);
    assert!(budget.cost_fraction() > 1.0);
}

#[test]
fn budget_turn_fraction() {
    let budget = Budget::new(None, Some(200), None);
    assert!((budget.turn_fraction()).abs() < f64::EPSILON);

    budget.record(0.0, 50);
    assert!((budget.turn_fraction() - 0.25).abs() < 0.01);

    budget.record(0.0, 100);
    assert!((budget.turn_fraction() - 0.75).abs() < 0.01);
}

#[test]
fn budget_no_limits() {
    let budget = Budget::new(None, None, None);

    budget.record(999.0, 9999);
    assert_eq!(budget.check(), BudgetStatus::Ok);
    assert!((budget.cost_fraction()).abs() < f64::EPSILON);
    assert!((budget.turn_fraction()).abs() < f64::EPSILON);
}

#[test]
fn budget_duration_exceeded() {
    // Create budget with 0ms limit — elapsed is always >= 0
    let budget = Budget::new(None, None, Some(0));
    assert!(matches!(budget.check(), BudgetStatus::Exceeded(_)));
}

#[test]
fn criterion_type_equality() {
    assert_eq!(CriterionType::Mechanical, CriterionType::Mechanical);
    assert_eq!(CriterionType::Semantic, CriterionType::Semantic);
    assert_ne!(CriterionType::Mechanical, CriterionType::Semantic);
}

#[test]
fn resume_policy_default() {
    let policy = ResumePolicy::default();
    assert_eq!(policy.stages.len(), 3);
    // Verify the stages have expected turn counts
    assert_eq!(policy.stages[0].max_turns, 80);
    assert_eq!(policy.stages[1].max_turns, 100);
    assert_eq!(policy.stages[2].max_turns, 50);
}

#[test]
fn resume_policy_next_stage() {
    let policy = ResumePolicy::default();

    // Stage 0: 0-79 turns
    let stage = policy.next_stage(0).expect("should have stage");
    assert!(stage.message.contains("Plenty of turns"));

    let stage = policy.next_stage(79).expect("should have stage");
    assert!(stage.message.contains("Plenty of turns"));

    // Stage 1: 80-179 turns
    let stage = policy.next_stage(80).expect("should have stage");
    assert!(stage.message.contains("Focus on criteria"));

    // Stage 2: 180-229 turns
    let stage = policy.next_stage(180).expect("should have stage");
    assert!(stage.message.contains("Final attempt"));

    // Exhausted: 230+ turns
    assert!(policy.next_stage(230).is_none());
    assert!(policy.next_stage(500).is_none());
}

#[test]
fn resume_policy_serde_roundtrip() {
    let policy = ResumePolicy::default();
    let json = serde_json::to_string(&policy).expect("serialize");
    let deserialized: ResumePolicy = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(deserialized.stages.len(), 3);
    assert_eq!(deserialized.stages[0].max_turns, 80);
}

#[test]
fn resume_policy_custom_stages_via_serde() {
    let json = r#"{
        "stages": [
            {"max_turns": 10, "message": "first stage"},
            {"max_turns": 5, "message": "second stage"}
        ]
    }"#;
    let policy: ResumePolicy = serde_json::from_str(json).expect("deserialize");

    assert_eq!(policy.stages.len(), 2);
    assert_eq!(policy.next_stage(0).unwrap().message, "first stage");
    assert_eq!(policy.next_stage(9).unwrap().message, "first stage");
    assert_eq!(policy.next_stage(10).unwrap().message, "second stage");
    assert_eq!(policy.next_stage(14).unwrap().message, "second stage");
    assert!(policy.next_stage(15).is_none());
}

#[test]
fn session_status_equality() {
    assert_eq!(SessionStatus::Success, SessionStatus::Success);
    assert_eq!(SessionStatus::Failed, SessionStatus::Failed);
    assert_ne!(SessionStatus::Success, SessionStatus::Failed);
    assert_ne!(SessionStatus::Stuck, SessionStatus::Aborted);
}

#[test]
fn qa_verdict_equality() {
    assert_eq!(QaVerdict::Pass, QaVerdict::Pass);
    assert_eq!(QaVerdict::Fail, QaVerdict::Fail);
    assert_ne!(QaVerdict::Pass, QaVerdict::Fail);
    assert_ne!(QaVerdict::Partial, QaVerdict::Pass);
}

#[test]
fn mechanical_issue_kind_equality() {
    assert_eq!(
        MechanicalIssueKind::BlastRadiusViolation,
        MechanicalIssueKind::BlastRadiusViolation
    );
    assert_ne!(
        MechanicalIssueKind::BlastRadiusViolation,
        MechanicalIssueKind::LintViolation
    );
}

#[test]
fn cost_ledger_clone_shares_state() {
    let ledger1 = CostLedger::new();
    let ledger2 = ledger1.clone();

    ledger1.record("area/", 1.00, 10, "model");

    // Both should see the same data since they share the Arc
    assert_eq!(ledger1.total_sessions(), 1);
    assert_eq!(ledger2.total_sessions(), 1);
    assert!(ledger2.query("area/").is_some());
}

#[test]
fn cost_ledger_default_equals_new() {
    let ledger1 = CostLedger::new();
    let ledger2 = CostLedger::default();

    assert_eq!(ledger1.total_sessions(), ledger2.total_sessions());
    assert_eq!(ledger1.total_cost(), ledger2.total_cost());
}

#[test]
fn cost_ledger_empty_query_all() {
    let ledger = CostLedger::new();
    let all = ledger.query_all();
    assert!(all.is_empty());
}

#[test]
fn cost_ledger_empty_query_by_model() {
    let ledger = CostLedger::new();
    let by_model = ledger.query_by_model();
    assert!(by_model.is_empty());
}

#[test]
fn dispatch_spec_equality() {
    let spec1 = DispatchSpec::new("proj".to_owned(), vec![1, 2, 3]);
    let spec2 = DispatchSpec::new("proj".to_owned(), vec![1, 2, 3]);
    let spec3 = DispatchSpec::new("other".to_owned(), vec![1, 2, 3]);

    assert_eq!(spec1.prompt_numbers, spec2.prompt_numbers);
    assert_eq!(spec1.project, spec2.project);
    assert_ne!(spec1.project, spec3.project);
}

#[test]
fn agent_options_equality() {
    let opts1 = AgentOptions::new().model("claude-3").max_turns(50);
    let opts2 = AgentOptions::new().model("claude-3").max_turns(50);
    let opts3 = AgentOptions::new().model("gpt-4").max_turns(50);

    assert_eq!(opts1.model, opts2.model);
    assert_eq!(opts1.max_turns, opts2.max_turns);
    assert_ne!(opts1.model, opts3.model);
}

#[test]
fn orchestrator_config_equality() {
    let config1 = OrchestratorConfig::new()
        .max_concurrent(8)
        .default_budget_usd(10.0);
    let config2 = OrchestratorConfig::new()
        .max_concurrent(8)
        .default_budget_usd(10.0);
    let config3 = OrchestratorConfig::new().max_concurrent(4);

    assert_eq!(config1.max_concurrent, config2.max_concurrent);
    assert_eq!(config1.default_budget_usd, config2.default_budget_usd);
    assert_ne!(config1.max_concurrent, config3.max_concurrent);
}
