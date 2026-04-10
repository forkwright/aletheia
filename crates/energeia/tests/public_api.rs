//! Integration tests for the energeia public API.
//!
//! Covers serde round-trips, builder patterns, cost ledger semantics,
//! error handling, and Send/Sync bounds for the main public types.

#![expect(clippy::expect_used, reason = "test assertions")]
#![expect(clippy::unwrap_used, reason = "test assertions")]
#![expect(clippy::float_cmp, reason = "test assertions")]
#![expect(clippy::indexing_slicing, reason = "test assertions")]

use std::time::Duration;

use aletheia_energeia::cost_ledger::CostLedger;
use aletheia_energeia::engine::{AgentOptions, SessionEvent};
use aletheia_energeia::error::Error;
use aletheia_energeia::orchestrator::OrchestratorConfig;
use aletheia_energeia::types::{
    Budget, BudgetStatus, CriterionType, DispatchSpec, MechanicalIssueKind, QaVerdict,
    ResumePolicy, SessionStatus,
};

// ============================================================================
// DispatchSpec serde round-trips
// ============================================================================

#[test]
fn dispatch_spec_serde_roundtrip_basic() {
    let spec = DispatchSpec::new("my-project".to_owned(), vec![1, 2, 3]);
    let json = serde_json::to_string(&spec).expect("serialize");
    let deserialized: DispatchSpec = serde_json::from_str(&json).expect("deserialize");

    assert_eq!(deserialized.prompt_numbers, vec![1, 2, 3]);
    assert_eq!(deserialized.project, "my-project");
    assert!(deserialized.dag_ref.is_none());
    assert!(deserialized.max_parallel.is_none());
}

#[test]
fn dispatch_spec_serde_with_dag_ref() {
    // Build using serde deserialization since struct is non_exhaustive
    let json = r#"{
        "prompt_numbers": [10, 20, 30],
        "project": "test-proj",
        "dag_ref": "dag.yaml",
        "max_parallel": 8
    }"#;
    let spec: DispatchSpec = serde_json::from_str(json).expect("deserialize");

    assert_eq!(spec.prompt_numbers, vec![10, 20, 30]);
    assert_eq!(spec.project, "test-proj");
    assert_eq!(spec.dag_ref, Some("dag.yaml".to_owned()));
    assert_eq!(spec.max_parallel, Some(8));
}

#[test]
fn dispatch_spec_yaml_roundtrip() {
    let yaml = r"
prompt_numbers:
  - 1
  - 2
project: yaml-test
dag_ref: prompts/dag.yaml
max_parallel: 4
";
    let spec: DispatchSpec = serde_yaml::from_str(yaml).expect("deserialize from yaml");

    assert_eq!(spec.prompt_numbers, vec![1, 2]);
    assert_eq!(spec.project, "yaml-test");
    assert_eq!(spec.dag_ref, Some("prompts/dag.yaml".to_owned()));
    assert_eq!(spec.max_parallel, Some(4));
}

// ============================================================================
// AgentOptions builder chaining + serde
// ============================================================================

#[test]
fn agent_options_builder_chaining() {
    let opts = AgentOptions::new()
        .model("claude-sonnet-4-20250514")
        .system_prompt("You are a helpful coding assistant")
        .cwd("/tmp/project")
        .max_turns(100)
        .permission_mode("plan");

    assert_eq!(opts.model, Some("claude-sonnet-4-20250514".to_owned()));
    assert_eq!(opts.system_prompt, Some("You are a helpful coding assistant".to_owned()));
    assert_eq!(opts.cwd, Some("/tmp/project".to_owned()));
    assert_eq!(opts.max_turns, Some(100));
    assert_eq!(opts.permission_mode, Some("plan".to_owned()));
}

#[test]
fn agent_options_default_is_empty() {
    let opts = AgentOptions::default();
    assert!(opts.model.is_none());
    assert!(opts.system_prompt.is_none());
    assert!(opts.cwd.is_none());
    assert!(opts.max_turns.is_none());
    assert!(opts.permission_mode.is_none());
}

#[test]
fn agent_options_serde_roundtrip() {
    let opts = AgentOptions::new()
        .model("claude-opus-4-20250514")
        .max_turns(50);

    let json = serde_json::to_string(&opts).expect("serialize");
    let deserialized: AgentOptions = serde_json::from_str(&json).expect("deserialize");

    assert_eq!(deserialized.model, Some("claude-opus-4-20250514".to_owned()));
    assert_eq!(deserialized.max_turns, Some(50));
    assert!(deserialized.system_prompt.is_none());
}

#[test]
fn agent_options_serde_partial() {
    let json = r#"{"model":"claude-3-haiku","max_turns":25}"#;
    let opts: AgentOptions = serde_json::from_str(json).expect("deserialize");

    assert_eq!(opts.model, Some("claude-3-haiku".to_owned()));
    assert_eq!(opts.max_turns, Some(25));
    assert!(opts.cwd.is_none());
    assert!(opts.permission_mode.is_none());
}

#[test]
fn agent_options_builder_method_chaining_order_independent() {
    // Verify builder works regardless of order
    let opts1 = AgentOptions::new()
        .model("claude-3")
        .max_turns(50)
        .cwd("/a");

    let opts2 = AgentOptions::new()
        .cwd("/a")
        .model("claude-3")
        .max_turns(50);

    assert_eq!(opts1.model, opts2.model);
    assert_eq!(opts1.cwd, opts2.cwd);
    assert_eq!(opts1.max_turns, opts2.max_turns);
}

// ============================================================================
// OrchestratorConfig defaults and builder
// ============================================================================

#[test]
fn orchestrator_config_defaults() {
    let config = OrchestratorConfig::default();

    assert_eq!(config.max_concurrent, 4);
    assert!(config.default_budget_usd.is_none());
    assert!(config.default_budget_turns.is_none());
    assert!(config.max_duration.is_none());
    assert_eq!(config.session_idle_timeout, Some(Duration::from_secs(600)));
    assert_eq!(config.max_corrective_retries, 1);
}

#[test]
fn orchestrator_config_new_uses_defaults() {
    let config = OrchestratorConfig::new();

    assert_eq!(config.max_concurrent, 4);
    assert!(config.default_budget_usd.is_none());
}

#[test]
fn orchestrator_config_builder_chaining() {
    let config = OrchestratorConfig::new()
        .max_concurrent(8)
        .default_budget_usd(50.0)
        .default_budget_turns(1000)
        .max_duration(Duration::from_secs(3600))
        .session_idle_timeout(Duration::from_secs(300))
        .max_corrective_retries(3);

    assert_eq!(config.max_concurrent, 8);
    assert_eq!(config.default_budget_usd, Some(50.0));
    assert_eq!(config.default_budget_turns, Some(1000));
    assert_eq!(config.max_duration, Some(Duration::from_secs(3600)));
    assert_eq!(config.session_idle_timeout, Some(Duration::from_secs(300)));
    assert_eq!(config.max_corrective_retries, 3);
}

#[test]
fn orchestrator_config_serde_roundtrip() {
    let config = OrchestratorConfig::new()
        .max_concurrent(6)
        .default_budget_usd(25.0)
        .max_duration(Duration::from_secs(1800));

    let json = serde_json::to_string(&config).expect("serialize");
    let deserialized: OrchestratorConfig = serde_json::from_str(&json).expect("deserialize");

    assert_eq!(deserialized.max_concurrent, 6);
    assert_eq!(deserialized.default_budget_usd, Some(25.0));
    assert_eq!(deserialized.max_duration, Some(Duration::from_secs(1800)));
    assert_eq!(deserialized.session_idle_timeout, Some(Duration::from_secs(600)));
}

#[test]
fn orchestrator_config_serde_with_null_durations() {
    // Use JSON deserialization for non_exhaustive struct
    let json = r#"{
        "max_concurrent": 4,
        "default_budget_usd": null,
        "default_budget_turns": null,
        "max_duration": null,
        "session_idle_timeout": null,
        "max_corrective_retries": 1
    }"#;
    let config: OrchestratorConfig = serde_json::from_str(json).expect("deserialize");

    assert!(config.session_idle_timeout.is_none());
    assert!(config.max_duration.is_none());
}

// ============================================================================
// CostLedger record and query semantics
// ============================================================================

#[test]
fn cost_ledger_record_and_query_single() {
    let ledger = CostLedger::new();

    ledger.record("crates/foo/", 1.50, 10, "claude-3-5-sonnet");

    let cost = ledger.query("crates/foo/").expect("should find cost record");
    assert_eq!(cost.blast_radius, "crates/foo/");
    assert!((cost.total_cost_usd - 1.50).abs() < 0.001);
    assert_eq!(cost.total_turns, 10);
    assert_eq!(cost.session_count, 1);
}

#[test]
fn cost_ledger_record_accumulates() {
    let ledger = CostLedger::new();

    ledger.record("crates/foo/", 1.00, 10, "claude-3-5-sonnet");
    ledger.record("crates/foo/", 2.00, 20, "claude-3-5-sonnet");
    ledger.record("crates/foo/", 0.50, 5, "claude-3-haiku");

    let cost = ledger.query("crates/foo/").expect("should find cost record");
    assert!((cost.total_cost_usd - 3.50).abs() < 0.001);
    assert_eq!(cost.total_turns, 35);
    assert_eq!(cost.session_count, 3);
    assert_eq!(cost.cost_by_model.len(), 2);
}

#[test]
fn cost_ledger_query_nonexistent_returns_none() {
    let ledger = CostLedger::new();
    assert!(ledger.query("does-not-exist").is_none());
}

#[test]
fn cost_ledger_query_all_sorted() {
    let ledger = CostLedger::new();

    ledger.record("crates/zulu/", 1.00, 10, "claude-3-5-sonnet");
    ledger.record("crates/alpha/", 2.00, 20, "claude-3-5-sonnet");
    ledger.record("crates/middle/", 0.50, 5, "claude-3-5-sonnet");

    let all = ledger.query_all();
    assert_eq!(all.len(), 3);
    // Should be sorted by blast radius
    assert_eq!(all[0].0, "crates/alpha/");
    assert_eq!(all[1].0, "crates/middle/");
    assert_eq!(all[2].0, "crates/zulu/");
}

#[test]
fn cost_ledger_query_by_model() {
    let ledger = CostLedger::new();

    ledger.record("crates/foo/", 1.00, 10, "claude-3-5-sonnet");
    ledger.record("crates/bar/", 2.00, 20, "claude-3-5-sonnet");
    ledger.record("crates/baz/", 0.50, 5, "claude-3-haiku");

    let by_model = ledger.query_by_model();
    assert_eq!(by_model.len(), 2);

    // Should be sorted by model name
    assert_eq!(by_model[0].0, "claude-3-5-sonnet");
    assert!((by_model[0].1 - 3.00).abs() < 0.001);

    assert_eq!(by_model[1].0, "claude-3-haiku");
    assert!((by_model[1].1 - 0.50).abs() < 0.001);
}

#[test]
fn cost_ledger_multi_radius() {
    let ledger = CostLedger::new();

    ledger.record("crates/foo/", 1.00, 10, "claude-3-5-sonnet");
    ledger.record("crates/bar/", 2.00, 20, "claude-3-5-sonnet");

    let foo_cost = ledger.query("crates/foo/").expect("should find foo");
    let bar_cost = ledger.query("crates/bar/").expect("should find bar");

    assert!((foo_cost.total_cost_usd - 1.00).abs() < 0.001);
    assert!((bar_cost.total_cost_usd - 2.00).abs() < 0.001);
    assert!((ledger.total_cost() - 3.00).abs() < 0.001);
    assert_eq!(ledger.total_sessions(), 2);
}

#[test]
fn cost_ledger_total_cost_and_sessions() {
    let ledger = CostLedger::new();

    ledger.record("area1/", 1.50, 15, "model-a");
    ledger.record("area2/", 2.50, 25, "model-b");
    ledger.record("area1/", 0.50, 5, "model-a");

    assert!((ledger.total_cost() - 4.50).abs() < 0.001);
    assert_eq!(ledger.total_sessions(), 3);
}

#[test]
fn cost_ledger_clear() {
    let ledger = CostLedger::new();

    ledger.record("area/", 1.00, 10, "model");
    assert_eq!(ledger.total_sessions(), 1);

    ledger.clear();
    assert_eq!(ledger.total_sessions(), 0);
    assert!(ledger.query_all().is_empty());
    assert!(ledger.query("area/").is_none());
}

#[test]
fn cost_ledger_zero_cost_skipped() {
    let ledger = CostLedger::new();

    ledger.record("area/", 0.0, 0, "model");
    assert!(ledger.query("area/").is_none());
}

#[test]
fn cost_ledger_zero_cost_with_turns_recorded() {
    let ledger = CostLedger::new();

    ledger.record("area/", 0.0, 5, "model");
    let cost = ledger.query("area/").expect("should be recorded");
    assert_eq!(cost.total_turns, 5);
}

#[test]
fn cost_ledger_cost_by_model_accumulation() {
    let ledger = CostLedger::new();

    // Multiple models for same blast radius
    ledger.record("crates/shared/", 2.00, 20, "claude-opus");
    ledger.record("crates/shared/", 1.50, 15, "claude-sonnet");
    ledger.record("crates/shared/", 0.50, 10, "claude-opus");

    let cost = ledger.query("crates/shared/").expect("should find");

    // Total cost: 2.00 + 1.50 + 0.50 = 4.00
    assert!((cost.total_cost_usd - 4.00).abs() < 0.001);

    // By model: claude-opus = 2.50, claude-sonnet = 1.50
    assert_eq!(cost.cost_by_model.len(), 2);
    let opus_cost = cost.cost_by_model.get("claude-opus").copied().unwrap_or(0.0);
    let sonnet_cost = cost.cost_by_model.get("claude-sonnet").copied().unwrap_or(0.0);
    assert!((opus_cost - 2.50).abs() < 0.001);
    assert!((sonnet_cost - 1.50).abs() < 0.001);
}

// ============================================================================
// Error is Send + Sync
// ============================================================================

#[test]
fn error_is_send_sync() {
    const _: fn() = || {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<Error>();
    };
}

// ============================================================================
// Send + Sync bounds for other types
// ============================================================================

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

// ============================================================================
// Additional type tests
// ============================================================================

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
    assert_eq!(
        MechanicalIssueKind::AntiPattern.to_string(),
        "anti_pattern"
    );
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
    assert!(
        matches!(deserialized, SessionEvent::ToolUse { name, .. } if name == "read_file")
    );
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
    assert!(matches!(deserialized, SessionEvent::TurnComplete { turn: 5 }));

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
    let config1 = OrchestratorConfig::new().max_concurrent(8).default_budget_usd(10.0);
    let config2 = OrchestratorConfig::new().max_concurrent(8).default_budget_usd(10.0);
    let config3 = OrchestratorConfig::new().max_concurrent(4);

    assert_eq!(config1.max_concurrent, config2.max_concurrent);
    assert_eq!(config1.default_budget_usd, config2.default_budget_usd);
    assert_ne!(config1.max_concurrent, config3.max_concurrent);
}
