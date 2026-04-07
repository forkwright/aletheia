// WHY: Concurrent execution of independent prompts within a DAG frontier group.
// Uses JoinSet for bounded concurrency with per-prompt error isolation so one
// failure doesn't abort siblings. CancellationToken enables graceful abort from
// budget exhaustion or operator cancellation.

use std::sync::Arc;

use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;

use crate::budget::BudgetStatus;
use crate::engine::DispatchEngine;
use crate::prompt::PromptSpec;
use crate::resume::ResumePolicy;
use crate::session::manager::SessionManager;
use crate::session::options::EngineConfig;
use crate::types::{Budget, SessionOutcome, SessionStatus};

/// Execute a group of independent prompts concurrently with bounded parallelism.
///
/// Each prompt runs in its own spawned task with error isolation: one prompt
/// failing does not abort its siblings. The shared [`Budget`] is checked after
/// each prompt completes; if exceeded, remaining prompts are skipped via the
/// [`CancellationToken`].
///
/// # Arguments
///
/// * `prompts` — prompts in this group (all have satisfied dependencies)
/// * `engine` — session execution backend
/// * `budget` — shared budget tracker across all sessions
/// * `resume_policy` — escalation policy for stuck sessions
/// * `options` — session-level engine configuration
/// * `max_concurrent` — semaphore bound for parallelism within this group
/// * `cancel` — token for graceful abort (budget exceeded or operator cancel)
///
/// Returns one [`SessionOutcome`] per prompt, in prompt-number order.
pub(crate) async fn execute_group(
    prompts: &[PromptSpec],
    engine: Arc<dyn DispatchEngine>,
    budget: Arc<Budget>,
    resume_policy: &ResumePolicy,
    options: &EngineConfig,
    max_concurrent: usize,
    cancel: &CancellationToken,
) -> Vec<SessionOutcome> {
    let semaphore = Arc::new(tokio::sync::Semaphore::new(max_concurrent));
    let mut join_set: JoinSet<SessionOutcome> = JoinSet::new();

    for prompt in prompts {
        let engine = Arc::clone(&engine);
        let budget = Arc::clone(&budget);
        let policy = resume_policy.clone();
        let opts = options.clone();
        let sem = Arc::clone(&semaphore);
        let token = cancel.clone();
        let prompt = prompt.clone();

        join_set.spawn(async move {
            // NOTE: Check cancellation before acquiring the semaphore to avoid
            // starting work that will immediately be discarded.
            if token.is_cancelled() {
                return skipped_outcome(&prompt, "dispatch cancelled");
            }

            let Ok(_permit) = sem.acquire().await else {
                return skipped_outcome(&prompt, "semaphore closed");
            };

            if token.is_cancelled() {
                return skipped_outcome(&prompt, "dispatch cancelled");
            }

            let mgr = SessionManager::new(engine, Arc::clone(&budget), policy);

            let outcome = match mgr.execute(&prompt, &opts).await {
                Ok(outcome) => outcome,
                Err(e) => SessionOutcome {
                    prompt_number: prompt.number,
                    status: SessionStatus::Failed,
                    session_id: None,
                    cost_usd: 0.0,
                    num_turns: 0,
                    duration_ms: 0,
                    resume_count: 0,
                    pr_url: None,
                    error: Some(e.to_string()),
                    model: None,
                    blast_radius: prompt.blast_radius.clone(),
                },
            };

            // NOTE: Check budget after execution. If exceeded, signal cancellation
            // so remaining prompts in this group (and future groups) are skipped.
            if let BudgetStatus::Exceeded(reason) = budget.check() {
                tracing::warn!(
                    prompt_number = prompt.number,
                    reason = %reason,
                    "budget exceeded after prompt execution, cancelling group"
                );
                token.cancel();
            }

            outcome
        });
    }

    // NOTE: Collect results as tasks complete. JoinSet returns in completion
    // order; we sort by prompt number afterward for deterministic output.
    let mut outcomes: Vec<SessionOutcome> = Vec::with_capacity(prompts.len());

    while let Some(result) = join_set.join_next().await {
        match result {
            Ok(outcome) => outcomes.push(outcome),
            Err(join_err) => {
                // SAFETY: JoinError means the task panicked or was cancelled.
                // Log and produce a failed outcome so the orchestrator can
                // mark dependents as blocked.
                tracing::error!(error = %join_err, "session task join error");
                outcomes.push(SessionOutcome {
                    prompt_number: 0,
                    status: SessionStatus::InfraFailure,
                    session_id: None,
                    cost_usd: 0.0,
                    num_turns: 0,
                    duration_ms: 0,
                    resume_count: 0,
                    pr_url: None,
                    error: Some(format!("task join error: {join_err}")),
                    model: None,
                    blast_radius: vec![],
                });
            }
        }
    }

    outcomes.sort_by_key(|o| o.prompt_number);
    outcomes
}

fn skipped_outcome(prompt: &PromptSpec, reason: &str) -> SessionOutcome {
    SessionOutcome {
        prompt_number: prompt.number,
        status: SessionStatus::Skipped,
        session_id: None,
        cost_usd: 0.0,
        num_turns: 0,
        duration_ms: 0,
        resume_count: 0,
        pr_url: None,
        error: Some(reason.to_owned()),
        model: None,
        blast_radius: prompt.blast_radius.clone(),
    }
}

#[cfg(test)]
#[expect(
    clippy::indexing_slicing,
    reason = "test assertions on known-length collections"
)]
mod tests {
    use std::sync::Arc;

    use tokio_util::sync::CancellationToken;

    use crate::budget::Budget;
    use crate::engine::{AgentOptions, SessionEvent, SessionResult};
    use crate::http::mock::{MockEngine, MockOutcome};
    use crate::prompt::PromptSpec;
    use crate::resume::ResumePolicy;
    use crate::session::options::EngineConfig;
    use crate::types::SessionStatus;

    use super::execute_group;

    fn sample_prompt_spec(number: u32) -> PromptSpec {
        PromptSpec {
            number,
            description: format!("test prompt {number}"),
            depends_on: vec![],
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

    fn default_config() -> EngineConfig {
        EngineConfig::new(AgentOptions::new())
    }

    #[tokio::test]
    async fn group_executes_all_prompts() {
        let engine = Arc::new(MockEngine::new(vec![
            success_outcome("s1", 0.10, 5),
            success_outcome("s2", 0.20, 8),
            success_outcome("s3", 0.15, 6),
        ]));
        let budget = Arc::new(Budget::new(Some(10.0), Some(500), None));
        let cancel = CancellationToken::new();

        let prompts = vec![sample_prompt_spec(1), sample_prompt_spec(2), sample_prompt_spec(3)];

        let outcomes = execute_group(
            &prompts,
            engine,
            budget.clone(),
            &ResumePolicy::default(),
            &default_config(),
            4,
            &cancel,
        )
        .await;

        assert_eq!(outcomes.len(), 3);
        assert!(outcomes.iter().all(|o| o.status == SessionStatus::Success));
        // NOTE: Results sorted by prompt number.
        assert_eq!(outcomes[0].prompt_number, 1);
        assert_eq!(outcomes[1].prompt_number, 2);
        assert_eq!(outcomes[2].prompt_number, 3);
    }

    #[tokio::test]
    async fn group_isolates_failures() {
        let engine = Arc::new(MockEngine::new(vec![
            success_outcome("s1", 0.10, 5),
            MockOutcome::SpawnFailure {
                detail: "auth error".to_owned(),
            },
            success_outcome("s3", 0.15, 6),
        ]));
        let budget = Arc::new(Budget::new(None, None, None));
        let cancel = CancellationToken::new();

        let prompts = vec![sample_prompt_spec(1), sample_prompt_spec(2), sample_prompt_spec(3)];

        let outcomes = execute_group(
            &prompts,
            engine,
            budget,
            &ResumePolicy::default(),
            &default_config(),
            4,
            &cancel,
        )
        .await;

        assert_eq!(outcomes.len(), 3);
        assert_eq!(outcomes[0].status, SessionStatus::Success);
        assert_eq!(outcomes[1].status, SessionStatus::Failed);
        assert_eq!(outcomes[2].status, SessionStatus::Success);
    }

    #[tokio::test]
    async fn group_respects_cancellation() {
        let engine = Arc::new(MockEngine::new(vec![]));
        let budget = Arc::new(Budget::new(None, None, None));
        let cancel = CancellationToken::new();
        cancel.cancel();

        let prompts = vec![sample_prompt_spec(1), sample_prompt_spec(2)];

        let outcomes = execute_group(
            &prompts,
            engine,
            budget,
            &ResumePolicy::default(),
            &default_config(),
            4,
            &cancel,
        )
        .await;

        assert_eq!(outcomes.len(), 2);
        assert!(outcomes.iter().all(|o| o.status == SessionStatus::Skipped));
    }

    #[tokio::test]
    async fn group_cancels_on_budget_exceeded() {
        // Budget of $0.05. First prompt costs $0.10 -> exceeds budget.
        // Second prompt (concurrency=1) should be skipped.
        let engine = Arc::new(MockEngine::new(vec![success_outcome("s1", 0.10, 5)]));
        let budget = Arc::new(Budget::new(Some(0.05), None, None));
        let cancel = CancellationToken::new();

        let prompts = vec![sample_prompt_spec(1), sample_prompt_spec(2)];

        let outcomes = execute_group(
            &prompts,
            engine,
            budget,
            &ResumePolicy::default(),
            &default_config(),
            1,
            &cancel,
        )
        .await;

        assert_eq!(outcomes.len(), 2);
        assert_eq!(outcomes[0].status, SessionStatus::Success);
        assert_eq!(outcomes[1].status, SessionStatus::Skipped);
        assert!(cancel.is_cancelled());
    }

    #[tokio::test]
    async fn group_bounds_concurrency() {
        // With max_concurrent=1, prompts execute sequentially.
        let engine = Arc::new(MockEngine::new(vec![
            success_outcome("s1", 0.10, 5),
            success_outcome("s2", 0.20, 8),
        ]));
        let budget = Arc::new(Budget::new(None, None, None));
        let cancel = CancellationToken::new();

        let prompts = vec![sample_prompt_spec(1), sample_prompt_spec(2)];

        let outcomes = execute_group(
            &prompts,
            engine,
            budget,
            &ResumePolicy::default(),
            &default_config(),
            1,
            &cancel,
        )
        .await;

        assert_eq!(outcomes.len(), 2);
        assert!(outcomes.iter().all(|o| o.status == SessionStatus::Success));
    }
}
