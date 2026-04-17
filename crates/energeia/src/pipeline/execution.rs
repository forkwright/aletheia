// WHY: Execution stage drives the frontier group loop: checks budget/cancel
// before each group, builds the group prompt list (skipping blocked prompts),
// drains correctives into the current group, then delegates to
// orchestrator::group::execute_group for concurrent session management.
// Separating this from preparation and post-processing keeps each file focused
// on a single phase and makes the full pipeline readable as a sequence of named
// stages.

use crate::dag::{PromptDag, PromptStatus};
use crate::pipeline::context::PipelineContext;
use crate::pipeline::error::PipelineError;
use crate::pipeline::PipelineStage;
use crate::prompt::PromptSpec;
use crate::types::{SessionOutcome, SessionStatus};

/// Execution stage: drives the frontier group loop and collects outcomes.
pub(crate) struct ExecutionStage;

impl PipelineStage for ExecutionStage {
    fn name(&self) -> &'static str {
        "execution"
    }

    async fn run(&self, ctx: &mut PipelineContext) -> Result<(), PipelineError> {
        use crate::budget::BudgetStatus;

        // Clone frontier to avoid borrow conflict while mutating ctx inside the
        // loop. The frontier is immutable after preparation.
        let frontier = ctx.frontier.clone();

        for (group_idx, group_numbers) in frontier.iter().enumerate() {
            if ctx.cancel.is_cancelled() {
                tracing::info!(group = group_idx, "skipping group due to cancellation");
                {
                    let dag = ctx.dag_mut();
                    let skipped = collect_skipped(group_numbers, dag, "dispatch aborted");
                    ctx.outcomes.extend(skipped);
                }
                ctx.aborted = true;
                continue;
            }

            if let BudgetStatus::Exceeded(reason) = ctx.budget().check() {
                tracing::warn!(
                    group = group_idx,
                    reason = %reason,
                    "budget exceeded, skipping group"
                );
                {
                    let dag = ctx.dag_mut();
                    let skipped = collect_skipped(group_numbers, dag, "dispatch aborted");
                    ctx.outcomes.extend(skipped);
                }
                ctx.aborted = true;
                continue;
            }

            // Collect prompts for this group, skipping those whose dependencies
            // failed or are blocked.
            let mut group_prompts: Vec<PromptSpec> = Vec::new();
            for &n in group_numbers {
                let Some(prompt) = ctx.prompt_map.get(&n).cloned() else {
                    continue;
                };
                if has_failed_dependency(n, ctx.dag_mut()) {
                    let _ = ctx.dag_mut().set_status(n, PromptStatus::Blocked);
                    ctx.outcomes.push(SessionOutcome {
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
                    mark_dependents_blocked(n, ctx.dag_mut());
                } else {
                    group_prompts.push(prompt);
                }
            }

            // Drain correctives from the previous group into this execution.
            let correctives = std::mem::take(&mut ctx.correctives);
            group_prompts.extend(correctives);

            if group_prompts.is_empty() {
                continue;
            }

            // Mark prompts as InProgress before execution.
            for p in &group_prompts {
                let _ = ctx.dag_mut().set_status(p.number, PromptStatus::InProgress);
            }

            tracing::info!(
                group = group_idx,
                prompts = ?group_prompts.iter().map(|p| p.number).collect::<Vec<_>>(),
                "executing group"
            );

            let engine_config = ctx.engine_config().clone();
            let outcomes = crate::orchestrator::group::execute_group(
                &group_prompts,
                std::sync::Arc::clone(&ctx.engine),
                std::sync::Arc::clone(ctx.budget()),
                &ctx.resume_policy,
                &engine_config,
                ctx.max_concurrent,
                &ctx.cancel,
            )
            .await;

            // Process outcomes: update DAG, record cost, handle QA and
            // correctives. Post-processing handles metrics and store; this
            // stage only updates the DAG and correctives list.
            for outcome in &outcomes {
                let cost_ledger = std::sync::Arc::clone(ctx.cost_ledger());
                let model = outcome.model.as_deref().unwrap_or("unknown");

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

                match outcome.status {
                    SessionStatus::Success => {
                        let _ = ctx
                            .dag_mut()
                            .set_status(outcome.prompt_number, PromptStatus::Done);

                        if let Some(pr_url) = &outcome.pr_url {
                            let pr_url = pr_url.clone();
                            let outcome_clone = outcome.clone();
                            let prompt_map = &ctx.prompt_map;
                            let correctives = &mut ctx.correctives;
                            run_qa_and_generate_corrective(
                                &*ctx.qa,
                                &outcome_clone,
                                &pr_url,
                                prompt_map,
                                correctives,
                                ctx.config.max_corrective_retries,
                            )
                            .await;
                        }
                    }
                    SessionStatus::Skipped => {
                        // Skipped prompts stay in their current DAG state.
                    }
                    _ => {
                        let _ = ctx
                            .dag_mut()
                            .set_status(outcome.prompt_number, PromptStatus::Failed);
                        mark_dependents_blocked(outcome.prompt_number, ctx.dag_mut());
                    }
                }
            }

            ctx.outcomes.extend(outcomes);
        }

        // Any correctives that had no group to run in are recorded as skipped.
        for c in std::mem::take(&mut ctx.correctives) {
            ctx.outcomes.push(SessionOutcome {
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

        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Helpers (replicated from orchestrator/mod.rs, scoped here for the stage)
// ---------------------------------------------------------------------------

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

fn mark_dependents_blocked(failed_number: u32, dag: &mut PromptDag) {
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

/// Collect skipped outcomes for a group, marking each DAG node as Failed.
///
/// Returns the outcomes rather than taking `&mut Vec<SessionOutcome>` so
/// callers can drop the `dag` borrow before extending the outcome vec.
fn collect_skipped(numbers: &[u32], dag: &mut PromptDag, reason: &str) -> Vec<SessionOutcome> {
    numbers
        .iter()
        .map(|&n| {
            let _ = dag.set_status(n, PromptStatus::Failed);
            SessionOutcome {
                prompt_number: n,
                status: SessionStatus::Skipped,
                session_id: None,
                cost_usd: 0.0,
                num_turns: 0,
                duration_ms: 0,
                resume_count: 0,
                pr_url: None,
                error: Some(reason.to_owned()),
                model: None,
                blast_radius: vec![],
            }
        })
        .collect()
}

async fn run_qa_and_generate_corrective(
    qa: &dyn crate::qa::QaGate,
    outcome: &crate::types::SessionOutcome,
    pr_url: &str,
    prompt_map: &std::collections::HashMap<u32, crate::prompt::PromptSpec>,
    correctives: &mut Vec<crate::prompt::PromptSpec>,
    max_corrective_retries: u32,
) {
    let Some(prompt) = prompt_map.get(&outcome.prompt_number) else {
        return;
    };

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

    let qa_result = match qa.evaluate(&qa_prompt, pr_number, "").await {
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

    if qa_result.verdict != crate::types::QaVerdict::Pass
        && correctives.len()
            < usize::try_from(max_corrective_retries).unwrap_or(usize::MAX)
        && let Some(corrective) =
            crate::qa::corrective::generate_corrective(&qa_result, &qa_prompt)
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
        correctives.push(crate::prompt::PromptSpec {
            number: outcome.prompt_number,
            description: corrective.description,
            depends_on: vec![],
            acceptance_criteria: corrective.acceptance_criteria,
            blast_radius: corrective.blast_radius,
            body,
        });
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crate::engine::{SessionEvent, SessionResult};
    use crate::http::mock::{MockEngine, MockOutcome};
    use crate::orchestrator::OrchestratorConfig;
    use crate::pipeline::context::PipelineContext;
    use crate::pipeline::preparation::PreparationStage;
    use crate::pipeline::PipelineStage as _;
    use crate::prompt::PromptSpec;
    use crate::qa::QaGate;
    use crate::types::{DispatchSpec, MechanicalIssue, QaResult, QaVerdict, SessionStatus};

    use super::ExecutionStage;

    struct AlwaysPassQa;

    impl QaGate for AlwaysPassQa {
        fn evaluate<'a>(
            &'a self,
            prompt: &'a crate::qa::PromptSpec,
            pr_number: u64,
            _diff: &'a str,
        ) -> std::pin::Pin<
            Box<dyn std::future::Future<Output = crate::error::Result<QaResult>> + Send + 'a>,
        > {
            use jiff::Timestamp;
            Box::pin(async move {
                Ok(QaResult {
                    prompt_number: prompt.prompt_number,
                    pr_number,
                    verdict: QaVerdict::Pass,
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

    fn success_mock_outcome(session_id: &str, cost: f64, turns: u32) -> MockOutcome {
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

    fn make_prepared_context(
        mock_outcomes: Vec<MockOutcome>,
        prompts: Vec<PromptSpec>,
    ) -> PipelineContext {
        let engine = Arc::new(MockEngine::new(mock_outcomes));
        let qa = Arc::new(AlwaysPassQa);
        let spec = DispatchSpec::new(
            "acme".to_owned(),
            prompts.iter().map(|p| p.number).collect(),
        );
        PipelineContext::new(
            spec,
            prompts,
            engine,
            qa,
            OrchestratorConfig::default(),
            #[cfg(feature = "storage-fjall")]
            None,
        )
    }

    #[tokio::test]
    async fn execution_runs_single_prompt() {
        let prompts = vec![PromptSpec {
            number: 1,
            description: "test".to_owned(),
            depends_on: vec![],
            acceptance_criteria: vec![],
            blast_radius: vec![],
            body: "do the thing".to_owned(),
        }];
        let mut ctx = make_prepared_context(vec![success_mock_outcome("s1", 0.10, 5)], prompts);

        PreparationStage
            .run(&mut ctx)
            .await
            .expect("preparation must succeed");
        ExecutionStage
            .run(&mut ctx)
            .await
            .expect("execution must succeed");

        assert_eq!(ctx.outcomes.len(), 1);
        assert_eq!(ctx.outcomes[0].status, SessionStatus::Success);
    }

    #[tokio::test]
    async fn execution_respects_dependency_ordering() {
        // DAG: 1 -> 2. Two groups: first runs prompt 1, second runs prompt 2.
        let prompts = vec![
            PromptSpec {
                number: 1,
                description: "first".to_owned(),
                depends_on: vec![],
                acceptance_criteria: vec![],
                blast_radius: vec![],
                body: "first task".to_owned(),
            },
            PromptSpec {
                number: 2,
                description: "second".to_owned(),
                depends_on: vec![1],
                acceptance_criteria: vec![],
                blast_radius: vec![],
                body: "second task".to_owned(),
            },
        ];
        let mut ctx = make_prepared_context(
            vec![
                success_mock_outcome("s1", 0.10, 5),
                success_mock_outcome("s2", 0.20, 8),
            ],
            prompts,
        );

        PreparationStage
            .run(&mut ctx)
            .await
            .expect("preparation must succeed");
        ExecutionStage
            .run(&mut ctx)
            .await
            .expect("execution must succeed");

        assert_eq!(ctx.outcomes.len(), 2);
        assert!(ctx.outcomes.iter().all(|o| o.status == SessionStatus::Success));
    }
}
