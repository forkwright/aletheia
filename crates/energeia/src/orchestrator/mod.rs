// WHY: Top-level dispatch orchestrator wiring DAG/frontier, session management,
// QA evaluation, and state persistence into a single execution pipeline. Given
// a DispatchSpec, executes prompts in dependency order with controlled
// concurrency, runs QA on results, generates corrective prompts for failures,
// and produces a complete DispatchResult.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use jiff::Timestamp;
use tokio_util::sync::CancellationToken;

use crate::budget::BudgetStatus;
use crate::cost_ledger::CostLedger;
use crate::dag::{PromptDag, PromptStatus, compute_frontier};
use crate::engine::DispatchEngine;
use crate::error::{self, Result};
use crate::prompt::PromptSpec;
use crate::qa::QaGate;
use crate::qa::corrective::generate_corrective;
use crate::resume::ResumePolicy;
use crate::session::options::EngineConfig;
use crate::types::{
    Budget, DispatchResult, DispatchSpec, QaVerdict, SessionOutcome, SessionStatus,
};

pub mod config;
pub(crate) mod group;

pub use config::OrchestratorConfig;

// ---------------------------------------------------------------------------
// Orchestrator
// ---------------------------------------------------------------------------

/// Top-level dispatch orchestrator.
///
/// Wires together the DAG/frontier, session management, QA evaluation, and
/// state persistence into a single execution pipeline. The dianoia integration
/// point is [`DispatchSpec`] as the interface type — the orchestrator does not
/// concern itself with who produces specs.
pub struct Orchestrator {
    engine: Arc<dyn DispatchEngine>,
    qa: Arc<dyn QaGate>,
    #[cfg(feature = "storage-fjall")]
    store: Option<Arc<crate::store::EnergeiaStore>>,
    config: OrchestratorConfig,
}

impl Orchestrator {
    /// Create a new orchestrator.
    #[must_use]
    pub fn new(
        engine: Arc<dyn DispatchEngine>,
        qa: Arc<dyn QaGate>,
        config: OrchestratorConfig,
    ) -> Self {
        Self {
            engine,
            qa,
            #[cfg(feature = "storage-fjall")]
            store: None,
            config,
        }
    }

    /// Attach a state persistence store.
    #[cfg(feature = "storage-fjall")]
    #[must_use]
    pub fn with_store(mut self, store: Arc<crate::store::EnergeiaStore>) -> Self {
        self.store = Some(store);
        self
    }

    /// Execute a full dispatch: build DAG, compute frontier, execute groups
    /// in dependency order with QA evaluation and corrective generation.
    ///
    /// # Flow
    ///
    /// 1. Load prompts from spec, build and validate DAG
    /// 2. Compute frontier (topological groups)
    /// 3. Create dispatch record in store (if attached)
    /// 4. For each group in frontier order:
    ///    a. Execute group concurrently (bounded by `max_concurrent`)
    ///    b. For each successful prompt: run QA gate if PR URL present
    ///    c. On QA fail: generate corrective, add to next group
    ///    d. Update DAG statuses (Done, Failed, Blocked)
    ///    e. If budget exceeded or cancelled: skip remaining groups
    /// 5. Finish dispatch record with aggregate results
    /// 6. Return `DispatchResult`
    ///
    /// # Errors
    ///
    /// Returns [`Error::Preflight`] if the prompt set is empty or the DAG
    /// is invalid. Returns [`Error::Aborted`] if the cancellation token is
    /// triggered before any work begins.
    #[expect(
        clippy::too_many_lines,
        reason = "dispatch lifecycle is inherently sequential with DAG iteration, QA, and store updates"
    )]
    pub async fn dispatch(
        &self,
        spec: DispatchSpec,
        prompts: &[PromptSpec],
    ) -> Result<DispatchResult> {
        let start = Instant::now();
        let dispatch_id = aletheia_koina::ulid::Ulid::new().to_string();

        if prompts.is_empty() {
            return error::PreflightSnafu {
                reason: "no prompts to dispatch",
            }
            .fail();
        }

        // --- Build DAG and compute frontier ---

        let mut dag = crate::prompt::build_dag(prompts)?;
        let frontier = compute_frontier(&dag);

        if frontier.is_empty() {
            return error::PreflightSnafu {
                reason: "all prompts already completed or DAG has no dispatchable nodes",
            }
            .fail();
        }

        tracing::info!(
            dispatch_id = %dispatch_id,
            project = %spec.project,
            groups = frontier.len(),
            total_prompts = prompts.len(),
            "starting dispatch"
        );

        // --- Create dispatch record ---

        #[cfg(feature = "storage-fjall")]
        let store_dispatch_id = self.store.as_ref().and_then(|store| {
            match store.create_dispatch(&spec.project, &spec) {
                Ok(id) => Some(id),
                Err(e) => {
                    tracing::warn!(error = %e, "failed to create dispatch record");
                    None
                }
            }
        });

        // --- Build prompt lookup ---

        let prompt_map: HashMap<u32, &PromptSpec> = prompts.iter().map(|p| (p.number, p)).collect();

        // --- Budget and cancellation ---

        let budget = Arc::new(Budget::new(
            self.config.default_budget_usd,
            self.config.default_budget_turns,
            self.config
                .max_duration
                .map(|d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX)),
        ));
        let cost_ledger = Arc::new(CostLedger::new());
        let cancel = CancellationToken::new();
        let resume_policy = ResumePolicy::default();
        let engine_config = EngineConfig::new(crate::engine::AgentOptions::new())
            .idle_timeout_opt(self.config.session_idle_timeout);

        let max_concurrent: usize =
            usize::try_from(spec.max_parallel.map_or(self.config.max_concurrent, |p| {
                p.min(self.config.max_concurrent)
            }))
            .unwrap_or(usize::MAX);

        // --- Execute frontier groups ---

        let mut all_outcomes: Vec<SessionOutcome> = Vec::new();
        let mut correctives: Vec<PromptSpec> = Vec::new();
        let mut aborted = false;

        for (group_idx, group_numbers) in frontier.iter().enumerate() {
            if cancel.is_cancelled() {
                tracing::info!(group = group_idx, "skipping group due to cancellation");
                mark_remaining_skipped(group_numbers, &mut dag, &mut all_outcomes);
                aborted = true;
                continue;
            }

            if let BudgetStatus::Exceeded(reason) = budget.check() {
                tracing::warn!(group = group_idx, reason = %reason, "budget exceeded, skipping group");
                mark_remaining_skipped(group_numbers, &mut dag, &mut all_outcomes);
                aborted = true;
                continue;
            }

            // NOTE: Collect prompts for this group. Skip prompts whose
            // dependencies are Failed or Blocked (cascade from earlier failures).
            let mut group_prompts: Vec<PromptSpec> = Vec::new();
            for &n in group_numbers {
                let Some(prompt) = prompt_map.get(&n) else {
                    continue;
                };
                if has_failed_dependency(n, &dag) {
                    let _ = dag.set_status(n, PromptStatus::Blocked);
                    all_outcomes.push(SessionOutcome {
                        prompt_number: n,
                        status: SessionStatus::Skipped,
                        session_id: None,
                        cost_usd: 0.0,
                        num_turns: 0,
                        duration_ms: 0,
                        resume_count: 0,
                        pr_url: None,
                        error: Some("dependency failed".to_owned()),
                        model: None,
                        blast_radius: prompt.blast_radius.clone(),
                    });
                    mark_dependents_blocked(n, &mut dag);
                } else {
                    group_prompts.push((*prompt).clone());
                }
            }

            // NOTE: Drain correctives into this group's execution.
            group_prompts.append(&mut correctives);

            if group_prompts.is_empty() {
                continue;
            }

            // NOTE: Mark prompts as InProgress before execution.
            for p in &group_prompts {
                let _ = dag.set_status(p.number, PromptStatus::InProgress);
            }

            tracing::info!(
                group = group_idx,
                prompts = ?group_prompts.iter().map(|p| p.number).collect::<Vec<_>>(),
                "executing group"
            );

            let outcomes = group::execute_group(
                &group_prompts,
                Arc::clone(&self.engine),
                Arc::clone(&budget),
                &resume_policy,
                &engine_config,
                max_concurrent,
                &cancel,
            )
            .await;

            // --- Process outcomes: update DAG, run QA, generate correctives, record cost ---

            for outcome in &outcomes {
                let model = outcome.model.as_deref().unwrap_or("unknown");
                let blast_radius = if outcome.blast_radius.is_empty() {
                    "unknown"
                } else {
                    // SAFETY: blast_radius checked non-empty above
                    #[expect(clippy::indexing_slicing, reason = "blast_radius checked non-empty at line above")]
                    outcome.blast_radius[0].as_str()
                };

                // Record cost attribution for this outcome
                if outcome.blast_radius.is_empty() {
                    cost_ledger.record("unknown", outcome.cost_usd, outcome.num_turns, model);
                } else {
                    cost_ledger.record_multi(
                        &outcome.blast_radius,
                        outcome.cost_usd,
                        outcome.num_turns,
                        model,
                    );
                }

                // Record Prometheus metrics
                crate::metrics::prometheus::record_session(
                    &spec.project,
                    &outcome.status.to_string(),
                    outcome.cost_usd,
                    outcome.duration_ms,
                    model,
                    blast_radius,
                );
                crate::metrics::prometheus::record_turns(
                    &spec.project,
                    outcome.num_turns,
                    model,
                    blast_radius,
                );

                match outcome.status {
                    SessionStatus::Success => {
                        let _ = dag.set_status(outcome.prompt_number, PromptStatus::Done);

                        // NOTE: Run QA if a PR URL is available.
                        if let Some(pr_url) = &outcome.pr_url {
                            self.run_qa_and_generate_corrective(
                                outcome,
                                pr_url,
                                &prompt_map,
                                &mut correctives,
                            )
                            .await;
                        }
                    }
                    SessionStatus::Skipped => {
                        // NOTE: Skipped prompts stay in their current DAG state.
                        // They don't block dependents because the group was aborted.
                    }
                    _ => {
                        let _ = dag.set_status(outcome.prompt_number, PromptStatus::Failed);

                        // NOTE: Mark downstream dependents as Blocked.
                        mark_dependents_blocked(outcome.prompt_number, &mut dag);
                    }
                }
            }

            all_outcomes.extend(outcomes);
        }

        // NOTE: Any remaining correctives that didn't get a group to execute in
        // are recorded as skipped.
        for c in &correctives {
            all_outcomes.push(SessionOutcome {
                prompt_number: c.number,
                status: SessionStatus::Skipped,
                session_id: None,
                cost_usd: 0.0,
                num_turns: 0,
                duration_ms: 0,
                resume_count: 0,
                pr_url: None,
                error: Some("corrective prompt had no remaining group to execute in".to_owned()),
                model: None,
                blast_radius: c.blast_radius.clone(),
            });
        }

        let total_cost = all_outcomes.iter().map(|o| o.cost_usd).sum();
        let duration_ms = u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX);

        let result = DispatchResult {
            dispatch_id: dispatch_id.clone(),
            outcomes: all_outcomes,
            total_cost_usd: total_cost,
            duration_ms,
            aborted,
            completed_at: Timestamp::now(),
        };

        // --- Finish dispatch record ---

        #[cfg(feature = "storage-fjall")]
        if let (Some(store), Some(ref store_id)) = (&self.store, store_dispatch_id) {
            let status = if aborted {
                crate::store::records::DispatchStatus::Failed
            } else {
                crate::store::records::DispatchStatus::Completed
            };
            if let Err(e) = store.finish_dispatch(store_id, status) {
                tracing::warn!(error = %e, "failed to finish dispatch record");
            }
        }

        tracing::info!(
            dispatch_id = %dispatch_id,
            total_cost = result.total_cost_usd,
            duration_ms = result.duration_ms,
            outcomes = result.outcomes.len(),
            aborted,
            "dispatch complete"
        );

        Ok(result)
    }

    /// Dry-run: build DAG, compute frontier, return the execution plan without
    /// actually dispatching any sessions.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Preflight`] if the prompt set is empty or the DAG
    /// is invalid.
    #[must_use]
    pub fn dry_run(&self, prompts: &[PromptSpec]) -> Result<DryRunResult> {
        if prompts.is_empty() {
            return error::PreflightSnafu {
                reason: "no prompts to dispatch",
            }
            .fail();
        }

        let dag = crate::prompt::build_dag(prompts)?;
        let frontier = compute_frontier(&dag);

        let groups: Vec<DryRunGroup> = frontier
            .iter()
            .enumerate()
            .map(|(idx, numbers)| {
                let group_prompts: Vec<DryRunPrompt> = numbers
                    .iter()
                    .filter_map(|&n| {
                        prompts
                            .iter()
                            .find(|p| p.number == n)
                            .map(|p| DryRunPrompt {
                                number: p.number,
                                description: p.description.clone(),
                                depends_on: p.depends_on.clone(),
                            })
                    })
                    .collect();
                DryRunGroup {
                    group_index: idx,
                    prompts: group_prompts,
                }
            })
            .collect();

        let total_prompts = prompts.len();
        let max_concurrent = self.config.max_concurrent;

        Ok(DryRunResult {
            groups,
            total_prompts,
            max_concurrent,
            budget_usd: self.config.default_budget_usd,
            budget_turns: self.config.default_budget_turns,
        })
    }

    /// Run QA on a successful session and generate a corrective prompt if needed.
    async fn run_qa_and_generate_corrective(
        &self,
        outcome: &SessionOutcome,
        pr_url: &str,
        prompt_map: &HashMap<u32, &PromptSpec>,
        correctives: &mut Vec<PromptSpec>,
    ) {
        let Some(prompt) = prompt_map.get(&outcome.prompt_number) else {
            return;
        };

        // NOTE: Extract PR number from URL. Expected format:
        // https://github.com/{owner}/{repo}/pull/{number}
        let pr_number = pr_url
            .rsplit('/')
            .next()
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(0);

        let qa_prompt = crate::qa::PromptSpec {
            prompt_number: prompt.number,
            description: prompt.description.clone(),
            acceptance_criteria: prompt.acceptance_criteria.clone(),
            blast_radius: prompt.blast_radius.clone(),
        };

        // NOTE: Evaluate via the QaGate trait. The diff is empty because
        // fetching the PR diff from GitHub is a separate concern (requires
        // network access and gh CLI integration). Mechanical checks on empty
        // diff produce no findings; semantic evaluation still classifies
        // criteria via keyword matching.
        let qa_result = match self.qa.evaluate(&qa_prompt, pr_number, "").await {
            Ok(result) => result,
            Err(e) => {
                tracing::warn!(
                    prompt_number = outcome.prompt_number,
                    error = %e,
                    "QA evaluation failed, skipping corrective generation"
                );
                return;
            }
        };

        tracing::info!(
            prompt_number = outcome.prompt_number,
            pr_number,
            verdict = %qa_result.verdict,
            "QA evaluation complete"
        );

        if qa_result.verdict != QaVerdict::Pass
            && correctives.len()
                < usize::try_from(self.config.max_corrective_retries).unwrap_or(usize::MAX)
            && let Some(corrective) = generate_corrective(&qa_result, &qa_prompt)
        {
            tracing::info!(
                prompt_number = outcome.prompt_number,
                "generated corrective prompt"
            );
            let body = format!(
                "Fix the following issues in PR #{pr_number}:\n\n{}",
                corrective
                    .acceptance_criteria
                    .iter()
                    .map(|c| format!("- {c}"))
                    .collect::<Vec<_>>()
                    .join("\n")
            );
            correctives.push(PromptSpec {
                number: outcome.prompt_number,
                description: corrective.description,
                depends_on: vec![],
                acceptance_criteria: corrective.acceptance_criteria,
                blast_radius: corrective.blast_radius,
                body,
            });
        }
    }
}

impl std::fmt::Debug for Orchestrator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Orchestrator")
            .field("config", &self.config)
            .finish_non_exhaustive()
    }
}

// ---------------------------------------------------------------------------
// Dry-run types
// ---------------------------------------------------------------------------

/// Result of a dry-run: the execution plan without actual dispatch.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[non_exhaustive]
pub struct DryRunResult {
    /// Ordered groups of prompts that would execute together.
    pub groups: Vec<DryRunGroup>,
    /// Total number of prompts in the dispatch.
    pub total_prompts: usize,
    /// Maximum concurrency configured for the dispatch.
    pub max_concurrent: u32,
    /// Budget limit in USD (if configured).
    pub budget_usd: Option<f64>,
    /// Budget limit in turns (if configured).
    pub budget_turns: Option<u32>,
}

/// A group of prompts in a dry-run plan.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[non_exhaustive]
pub struct DryRunGroup {
    /// Zero-based group index (execution order).
    pub group_index: usize,
    /// Prompts in this group.
    pub prompts: Vec<DryRunPrompt>,
}

/// Summary of a prompt in a dry-run plan.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[non_exhaustive]
pub struct DryRunPrompt {
    /// Prompt number.
    pub number: u32,
    /// Task description.
    pub description: String,
    /// Dependencies (prompt numbers).
    pub depends_on: Vec<u32>,
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Mark all prompts in a group as Skipped.
fn mark_remaining_skipped(
    numbers: &[u32],
    dag: &mut PromptDag,
    outcomes: &mut Vec<SessionOutcome>,
) {
    for &n in numbers {
        let _ = dag.set_status(n, PromptStatus::Failed);
        outcomes.push(SessionOutcome {
            prompt_number: n,
            status: SessionStatus::Skipped,
            session_id: None,
            cost_usd: 0.0,
            num_turns: 0,
            duration_ms: 0,
            resume_count: 0,
            pr_url: None,
            error: Some("dispatch aborted".to_owned()),
            model: None,
            blast_radius: vec![],
        });
    }
}

/// Check if any of a prompt's dependencies have Failed or Blocked status.
fn has_failed_dependency(number: u32, dag: &PromptDag) -> bool {
    let Some(node) = dag.nodes.get(&number) else {
        return false;
    };
    node.depends_on.iter().any(|&dep| {
        dag.nodes
            .get(&dep)
            .is_some_and(|d| matches!(d.status, PromptStatus::Failed | PromptStatus::Blocked))
    })
}

/// Mark all prompts that depend on a failed prompt as Blocked.
fn mark_dependents_blocked(failed_number: u32, dag: &mut PromptDag) {
    // NOTE: Collect dependents first to avoid borrow conflict.
    let dependents: Vec<u32> = dag
        .nodes
        .values()
        .filter(|node| node.depends_on.contains(&failed_number))
        .filter(|node| {
            matches!(
                node.status,
                PromptStatus::Pending | PromptStatus::Ready | PromptStatus::Blocked
            )
        })
        .map(|node| node.number)
        .collect();

    for n in dependents {
        let _ = dag.set_status(n, PromptStatus::Blocked);
    }
}

// ---------------------------------------------------------------------------
// EngineConfig extension
// ---------------------------------------------------------------------------

impl EngineConfig {
    /// Set the idle timeout from an `Option<Duration>`, no-op if `None`.
    #[must_use]
    pub(crate) fn idle_timeout_opt(mut self, timeout: Option<std::time::Duration>) -> Self {
        self.idle_timeout = timeout;
        self
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
#[expect(
    clippy::indexing_slicing,
    reason = "test assertions on known-length collections"
)]
mod tests {
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
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<QaResult>> + Send + 'a>>
        {
            Box::pin(async move {
                Ok(QaResult {
                    prompt_number: prompt.prompt_number,
                    pr_number,
                    verdict: self.verdict,
                    criteria_results: vec![],
                    mechanical_issues: vec![],
                    cost_usd: 0.0,
                    evaluated_at: Timestamp::now(),
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
        let prompts = vec![sample_prompt_spec(1, vec![]), sample_prompt_spec(2, vec![1])];
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
        let result = orchestrator.dispatch(sample_dispatch_spec(vec![]), &[]).await;

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
        let prompts = vec![sample_prompt_spec(1, vec![]), sample_prompt_spec(2, vec![1])];

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
}
