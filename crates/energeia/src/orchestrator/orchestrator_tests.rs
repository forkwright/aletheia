#![expect(clippy::unwrap_used, reason = "test assertions")]
#![expect(
    clippy::indexing_slicing,
    reason = "test assertions on known-length collections"
)]

use std::sync::Arc;

use crate::engine::{SessionEvent, SessionResult};
use crate::http::mock::{MockEngine, MockOutcome};
use crate::prompt::PromptSpec;
use crate::types::{MechanicalIssue, QaResult, QaVerdict, SessionStatus};

use super::*;

// -----------------------------------------------------------------------
// Mock QA gate
// -----------------------------------------------------------------------

struct MockQaGate {
    verdict: QaVerdict,
}

impl MockQaGate {
    fn passing() -> Self {
        Self {
            verdict: QaVerdict::Pass,
        }
    }
}

impl QaGate for MockQaGate {
    fn evaluate<'a>(
        &'a self,
        prompt: &'a crate::qa::PromptSpec,
        pr_number: u64,
        _diff: &'a str,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<QaResult>> + Send + 'a>> {
        Box::pin(async move {
            Ok(QaResult {
                prompt_number: prompt.prompt_number,
                pr_number,
                verdict: self.verdict,
                criteria_results: vec![],
                mechanical_issues: vec![],
                cost_usd: 0.0,
                evaluated_at: Timestamp::now(),
                semantic_evaluated: false,
            })
        })
    }

    fn mechanical_check(
        &self,
        _diff: &str,
        _prompt: &crate::qa::PromptSpec,
    ) -> Vec<MechanicalIssue> {
        vec![]
    }
}

// -----------------------------------------------------------------------
// Test helpers
// -----------------------------------------------------------------------

fn sample_prompt_spec(number: u32, depends_on: Vec<u32>) -> PromptSpec {
    PromptSpec {
        number,
        description: format!("test prompt {number}"),
        depends_on,
        acceptance_criteria: vec![],
        blast_radius: vec![],
        body: format!("implement task {number}"),
    }
}

fn success_outcome(session_id: &str, cost: f64, turns: u32) -> MockOutcome {
    MockOutcome::Success {
        events: vec![SessionEvent::TurnComplete { turn: turns }],
        result: SessionResult {
            session_id: session_id.to_owned(),
            cost_usd: cost,
            num_turns: turns,
            duration_ms: 100,
            success: true,
            result_text: Some("done".to_owned()),
            model: Some("claude-3-5-sonnet".to_owned()),
        },
    }
}

fn failure_outcome(session_id: &str, cost: f64, turns: u32) -> MockOutcome {
    MockOutcome::Success {
        events: vec![SessionEvent::TurnComplete { turn: turns }],
        result: SessionResult {
            session_id: session_id.to_owned(),
            cost_usd: cost,
            num_turns: turns,
            duration_ms: 100,
            success: false,
            result_text: None,
            model: Some("claude-3-5-sonnet".to_owned()),
        },
    }
}

fn sample_dispatch_spec(prompt_numbers: Vec<u32>) -> DispatchSpec {
    DispatchSpec {
        prompt_numbers,
        project: "acme".to_owned(),
        dag_ref: None,
        max_parallel: None,
    }
}

// -----------------------------------------------------------------------
// dispatch() tests
// -----------------------------------------------------------------------

#[tokio::test]
async fn dispatch_single_prompt_success() {
    let engine = Arc::new(MockEngine::new(vec![success_outcome("s1", 0.50, 10)]));
    let qa = Arc::new(MockQaGate::passing());
    let config = OrchestratorConfig::new().max_concurrent(4);

    let orchestrator = Orchestrator::new(engine, qa, config);
    let prompts = vec![sample_prompt_spec(1, vec![])];
    let spec = sample_dispatch_spec(vec![1]);

    let result = orchestrator.dispatch(spec, &prompts).await.unwrap();

    assert!(!result.aborted);
    assert_eq!(result.outcomes.len(), 1);
    assert_eq!(result.outcomes[0].status, SessionStatus::Success);
    assert!((result.total_cost_usd - 0.50).abs() < 0.01);
}

#[tokio::test]
async fn dispatch_diamond_dag() {
    // DAG: 1 -> [2, 3] -> 4
    // Three groups: [1], [2,3], [4]
    let engine = Arc::new(MockEngine::new(vec![
        success_outcome("s1", 0.10, 5),
        success_outcome("s2", 0.20, 8),
        success_outcome("s3", 0.15, 6),
        success_outcome("s4", 0.25, 10),
    ]));
    let qa = Arc::new(MockQaGate::passing());
    let config = OrchestratorConfig::new().max_concurrent(4);

    let orchestrator = Orchestrator::new(engine, qa, config);
    let prompts = vec![
        sample_prompt_spec(1, vec![]),
        sample_prompt_spec(2, vec![1]),
        sample_prompt_spec(3, vec![1]),
        sample_prompt_spec(4, vec![2, 3]),
    ];
    let spec = sample_dispatch_spec(vec![1, 2, 3, 4]);

    let result = orchestrator.dispatch(spec, &prompts).await.unwrap();

    assert!(!result.aborted);
    assert_eq!(result.outcomes.len(), 4);
    assert!(
        result
            .outcomes
            .iter()
            .all(|o| o.status == SessionStatus::Success)
    );
}

#[tokio::test]
async fn dispatch_failure_blocks_dependents() {
    // DAG: 1 -> 2 -> 3
    // Prompt 1 fails -> 2 and 3 should be skipped.
    // Resume policy: stages [80, 100, 50] = 230 total turns.
    // Each failure uses 80 turns: initial 80, resume 80 (=160), resume 80 (=240 > 230).
    // After 3 outcomes the session exhausts all stages -> Stuck.
    let engine = Arc::new(MockEngine::new(vec![
        failure_outcome("s1", 0.10, 80),
        failure_outcome("s1-r1", 0.10, 80),
        failure_outcome("s1-r2", 0.10, 80),
    ]));
    let qa = Arc::new(MockQaGate::passing());
    let config = OrchestratorConfig::new().max_concurrent(4);

    let orchestrator = Orchestrator::new(engine, qa, config);
    let prompts = vec![
        sample_prompt_spec(1, vec![]),
        sample_prompt_spec(2, vec![1]),
        sample_prompt_spec(3, vec![2]),
    ];
    let spec = sample_dispatch_spec(vec![1, 2, 3]);

    let result = orchestrator.dispatch(spec, &prompts).await.unwrap();

    // Prompt 1 stuck (resume exhausted), prompts 2 and 3 skipped.
    let o1 = result
        .outcomes
        .iter()
        .find(|o| o.prompt_number == 1)
        .unwrap();
    assert!(
        matches!(o1.status, SessionStatus::Stuck | SessionStatus::Failed),
        "prompt 1 should be stuck or failed, got {:?}",
        o1.status
    );

    let o2 = result
        .outcomes
        .iter()
        .find(|o| o.prompt_number == 2)
        .unwrap();
    assert_eq!(o2.status, SessionStatus::Skipped);

    let o3 = result
        .outcomes
        .iter()
        .find(|o| o.prompt_number == 3)
        .unwrap();
    assert_eq!(o3.status, SessionStatus::Skipped);
}

#[tokio::test]
async fn dispatch_budget_exceeded_aborts() {
    // Budget of $0.15. First group costs $0.20 -> exceeds.
    let engine = Arc::new(MockEngine::new(vec![success_outcome("s1", 0.20, 10)]));
    let qa = Arc::new(MockQaGate::passing());
    let config = OrchestratorConfig::new()
        .max_concurrent(4)
        .default_budget_usd(0.15);

    let orchestrator = Orchestrator::new(engine, qa, config);
    let prompts = vec![
        sample_prompt_spec(1, vec![]),
        sample_prompt_spec(2, vec![1]),
    ];
    let spec = sample_dispatch_spec(vec![1, 2]);

    let result = orchestrator.dispatch(spec, &prompts).await.unwrap();

    assert!(result.aborted);
    let o2 = result
        .outcomes
        .iter()
        .find(|o| o.prompt_number == 2)
        .unwrap();
    assert_eq!(o2.status, SessionStatus::Skipped);
}

#[tokio::test]
async fn dispatch_empty_prompts_returns_preflight_error() {
    let engine = Arc::new(MockEngine::new(vec![]));
    let qa = Arc::new(MockQaGate::passing());
    let config = OrchestratorConfig::default();

    let orchestrator = Orchestrator::new(engine, qa, config);
    let result = orchestrator
        .dispatch(sample_dispatch_spec(vec![]), &[])
        .await;

    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("no prompts"));
}

#[tokio::test]
async fn dispatch_parallel_independent_prompts() {
    // Three independent prompts — single group, all parallel.
    let engine = Arc::new(MockEngine::new(vec![
        success_outcome("s1", 0.10, 5),
        success_outcome("s2", 0.20, 8),
        success_outcome("s3", 0.15, 6),
    ]));
    let qa = Arc::new(MockQaGate::passing());
    let config = OrchestratorConfig::new().max_concurrent(4);

    let orchestrator = Orchestrator::new(engine, qa, config);
    let prompts = vec![
        sample_prompt_spec(1, vec![]),
        sample_prompt_spec(2, vec![]),
        sample_prompt_spec(3, vec![]),
    ];
    let spec = sample_dispatch_spec(vec![1, 2, 3]);

    let result = orchestrator.dispatch(spec, &prompts).await.unwrap();

    assert!(!result.aborted);
    assert_eq!(result.outcomes.len(), 3);
    assert!(
        result
            .outcomes
            .iter()
            .all(|o| o.status == SessionStatus::Success)
    );
}

// -----------------------------------------------------------------------
// dry_run() tests
// -----------------------------------------------------------------------

#[test]
fn dry_run_returns_execution_plan() {
    let engine = Arc::new(MockEngine::new(vec![]));
    let qa = Arc::new(MockQaGate::passing());
    let config = OrchestratorConfig::new()
        .max_concurrent(4)
        .default_budget_usd(10.0);

    let orchestrator = Orchestrator::new(engine, qa, config);
    let prompts = vec![
        sample_prompt_spec(1, vec![]),
        sample_prompt_spec(2, vec![1]),
        sample_prompt_spec(3, vec![1]),
        sample_prompt_spec(4, vec![2, 3]),
    ];

    let plan = orchestrator.dry_run(&prompts).unwrap();

    assert_eq!(plan.total_prompts, 4);
    assert_eq!(plan.max_concurrent, 4);
    assert_eq!(plan.budget_usd, Some(10.0));
    assert_eq!(plan.groups.len(), 3);

    assert_eq!(plan.groups[0].prompts.len(), 1);
    assert_eq!(plan.groups[0].prompts[0].number, 1);

    assert_eq!(plan.groups[1].prompts.len(), 2);
    let g1_numbers: Vec<u32> = plan.groups[1].prompts.iter().map(|p| p.number).collect();
    assert!(g1_numbers.contains(&2));
    assert!(g1_numbers.contains(&3));

    assert_eq!(plan.groups[2].prompts.len(), 1);
    assert_eq!(plan.groups[2].prompts[0].number, 4);
}

#[test]
fn dry_run_empty_prompts_returns_error() {
    let engine = Arc::new(MockEngine::new(vec![]));
    let qa = Arc::new(MockQaGate::passing());
    let config = OrchestratorConfig::default();

    let orchestrator = Orchestrator::new(engine, qa, config);
    let result = orchestrator.dry_run(&[]);

    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("no prompts"));
}

#[test]
fn dry_run_roundtrip_serialization() {
    let engine = Arc::new(MockEngine::new(vec![]));
    let qa = Arc::new(MockQaGate::passing());
    let config = OrchestratorConfig::default();

    let orchestrator = Orchestrator::new(engine, qa, config);
    let prompts = vec![
        sample_prompt_spec(1, vec![]),
        sample_prompt_spec(2, vec![1]),
    ];

    let plan = orchestrator.dry_run(&prompts).unwrap();
    let json = serde_json::to_string(&plan).unwrap();
    let back: DryRunResult = serde_json::from_str(&json).unwrap();

    assert_eq!(back.total_prompts, 2);
    assert_eq!(back.groups.len(), 2);
}

// -----------------------------------------------------------------------
// Helper function tests
// -----------------------------------------------------------------------

#[test]
fn mark_dependents_blocked_cascades() {
    let mut dag = PromptDag::new();
    dag.add_node(1, vec![]).unwrap();
    dag.add_node(2, vec![1]).unwrap();
    dag.add_node(3, vec![1]).unwrap();
    dag.add_node(4, vec![2]).unwrap();

    dag.set_status(1, PromptStatus::Failed).unwrap();
    dag.set_status(2, PromptStatus::Blocked).unwrap();
    dag.set_status(3, PromptStatus::Ready).unwrap();

    mark_dependents_blocked(1, &mut dag);

    assert_eq!(dag.nodes[&2].status, PromptStatus::Blocked);
    assert_eq!(dag.nodes[&3].status, PromptStatus::Blocked);
    // NOTE: 4 depends on 2, not directly on 1. It is not marked blocked
    // by this call. The orchestrator would mark it in a subsequent pass
    // when processing group results.
}
