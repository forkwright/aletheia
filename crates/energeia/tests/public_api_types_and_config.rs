#![expect(clippy::expect_used, reason = "test assertions")]
#![expect(clippy::indexing_slicing, reason = "test assertions")]

use std::time::Duration;

use energeia::cost_ledger::CostLedger;
use energeia::engine::AgentOptions;
use energeia::orchestrator::OrchestratorConfig;
use energeia::types::DispatchSpec;

// Split: DispatchSpec + AgentOptions + OrchestratorConfig + CostLedger.

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
    assert_eq!(
        opts.system_prompt,
        Some("You are a helpful coding assistant".to_owned())
    );
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

    assert_eq!(
        deserialized.model,
        Some("claude-opus-4-20250514".to_owned())
    );
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
    assert_eq!(config.session_idle_timeout, Some(Duration::from_mins(10)));
    assert_eq!(config.max_corrective_retries, 0);
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
        .max_duration(Duration::from_hours(1))
        .session_idle_timeout(Duration::from_mins(5))
        .max_corrective_retries(3);

    assert_eq!(config.max_concurrent, 8);
    assert_eq!(config.default_budget_usd, Some(50.0));
    assert_eq!(config.default_budget_turns, Some(1000));
    assert_eq!(config.max_duration, Some(Duration::from_hours(1)));
    assert_eq!(config.session_idle_timeout, Some(Duration::from_mins(5)));
    assert_eq!(config.max_corrective_retries, 3);
}

#[test]
fn orchestrator_config_serde_roundtrip() {
    let config = OrchestratorConfig::new()
        .max_concurrent(6)
        .default_budget_usd(25.0)
        .max_duration(Duration::from_mins(30));

    let json = serde_json::to_string(&config).expect("serialize");
    let deserialized: OrchestratorConfig = serde_json::from_str(&json).expect("deserialize");

    assert_eq!(deserialized.max_concurrent, 6);
    assert_eq!(deserialized.default_budget_usd, Some(25.0));
    assert_eq!(deserialized.max_duration, Some(Duration::from_mins(30)));
    assert_eq!(
        deserialized.session_idle_timeout,
        Some(Duration::from_mins(10))
    );
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

    let cost = ledger
        .query("crates/foo/")
        .expect("should find cost record");
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

    let cost = ledger
        .query("crates/foo/")
        .expect("should find cost record");
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
    let opus_cost = cost
        .cost_by_model
        .get("claude-opus")
        .copied()
        .unwrap_or(0.0);
    let sonnet_cost = cost
        .cost_by_model
        .get("claude-sonnet")
        .copied()
        .unwrap_or(0.0);
    assert!((opus_cost - 2.50).abs() < 0.001);
    assert!((sonnet_cost - 1.50).abs() < 0.001);
}

// ============================================================================
