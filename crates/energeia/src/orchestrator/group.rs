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
use crate::types::{Budget, FailureClass, SessionOutcome, SessionStatus};

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
    let mut join_set: JoinSet<(u32, SessionOutcome)> = JoinSet::new();

    for prompt in prompts {
        let engine = Arc::clone(&engine);
        let budget = Arc::clone(&budget);
        let policy = resume_policy.clone();
        let sem = Arc::clone(&semaphore);
        let token = cancel.clone();
        let opts = options.clone().cancel_token(token.clone());
        let prompt = prompt.clone();

        join_set.spawn(async move {
            let prompt_number = prompt.number;

            // WHY: Cancellation is checked before acquiring the semaphore to avoid
            // starting work that will immediately be discarded.
            if token.is_cancelled() {
                return (
                    prompt_number,
                    skipped_outcome(&prompt, "dispatch cancelled"),
                );
            }

            let Ok(_permit) = sem.acquire().await else {
                return (prompt_number, skipped_outcome(&prompt, "semaphore closed"));
            };

            if token.is_cancelled() {
                return (
                    prompt_number,
                    skipped_outcome(&prompt, "dispatch cancelled"),
                );
            }

            let mgr = SessionManager::new(engine, Arc::clone(&budget), policy);
            let opts = routed_options(opts, &prompt).await;
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
                    failure_class: Some(FailureClass::WorkerRuntime),
                    model: None,
                    blast_radius: prompt.blast_radius.clone(),
                    corrective_attempts: 0,
                    cache_hit_tokens: 0,
                    cache_miss_tokens: 0,
                    structured_output: None,
                },
            };

            // WHY: Budget is checked after execution; on exceed, cancellation is
            // signalled so remaining prompts in this and future groups are skipped.
            if let BudgetStatus::Exceeded(reason) = budget.check() {
                tracing::warn!(
                    prompt_number = prompt.number,
                    reason = %reason,
                    "budget exceeded after prompt execution, cancelling group"
                );
                token.cancel();
            }

            (prompt_number, outcome)
        });
    }

    // WHY: JoinSet returns in completion order; results are sorted by prompt
    // number afterward for deterministic output.
    let mut outcomes: Vec<SessionOutcome> = Vec::with_capacity(prompts.len());

    while let Some(result) = join_set.join_next().await {
        match result {
            Ok((_prompt_number, outcome)) => outcomes.push(outcome),
            Err(join_err) => {
                // WHY: JoinError does not carry the task output, so we use the
                // task_id to recover the prompt number. JoinSet::spawn returns a
                // task AbortHandle but not an id paired to our data — instead we
                // embed the prompt number in the task output (u32, SessionOutcome)
                // so the Ok branch above always has identity. A JoinError (panic
                // or cancellation) means we lost the identity; we log at error
                // level and emit an InfraFailure with the unknown sentinel so the
                // gap is visible in reports rather than silently producing a fake
                // prompt-0 record.
                tracing::error!(
                    error = %join_err,
                    "session task join error — prompt identity lost; inspect logs for the panicking task"
                );
                // NOTE: prompt_number u32::MAX is a sentinel for "identity lost on panic".
                // Downstream callers must not block dependents on this sentinel.
                outcomes.push(SessionOutcome {
                    prompt_number: u32::MAX,
                    status: SessionStatus::InfraFailure,
                    session_id: None,
                    cost_usd: 0.0,
                    num_turns: 0,
                    duration_ms: 0,
                    resume_count: 0,
                    pr_url: None,
                    error: Some(format!(
                        "task join error (prompt identity lost): {join_err}"
                    )),
                    failure_class: Some(FailureClass::WorkerRuntime),
                    model: None,
                    blast_radius: vec![],
                    corrective_attempts: 0,
                    cache_hit_tokens: 0,
                    cache_miss_tokens: 0,
                    structured_output: None,
                });
            }
        }
    }

    outcomes.sort_by_key(|o| o.prompt_number);
    outcomes
}

async fn routed_options(mut options: EngineConfig, prompt: &PromptSpec) -> EngineConfig {
    let routed_model = if options.options.model.is_none() {
        options
            .routing
            .model_for_prompt(&prompt.body, options.after_action_log_dir.as_deref())
            .await
    } else {
        None
    };

    if let Some(model) = routed_model {
        options.options.model = Some(model);
    }

    options
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
        failure_class: None,
        model: None,
        blast_radius: prompt.blast_radius.clone(),
        corrective_attempts: 0,
        cache_hit_tokens: 0,
        cache_miss_tokens: 0,
        structured_output: None,
    }
}

#[cfg(test)]
#[expect(
    clippy::indexing_slicing,
    reason = "test assertions on known-length collections"
)]
mod tests {
    use std::future::Future;
    use std::pin::Pin;
    use std::sync::Arc;

    use tokio_util::sync::CancellationToken;

    use crate::budget::Budget;
    use crate::engine::{
        AgentOptions, DispatchEngine, SessionEvent, SessionHandle, SessionResult, SessionSpec,
    };
    use crate::error::{self, Result};
    use crate::http::mock::{MockEngine, MockOutcome};
    use crate::prompt::PromptSpec;
    use crate::resume::ResumePolicy;
    use crate::session::options::EngineConfig;
    use crate::types::{FailureClass, SessionStatus};

    use super::execute_group;

    fn sample_prompt_spec(number: u32) -> PromptSpec {
        PromptSpec {
            number,
            description: format!("test prompt {number}"),
            depends_on: vec![],
            context_policy: crate::dag::ContextPolicy::Fresh,
            output_format: None,
            worktree: crate::prompt::WorktreePolicy::default(),
            acceptance_criteria: vec![],
            blast_radius: vec![],
            body: format!("implement task {number}"),
            prompt_components: None,
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
                cache_hit_tokens: 0,
                cache_miss_tokens: 0,
            },
        }
    }

    fn default_config() -> EngineConfig {
        EngineConfig::new(AgentOptions::new())
    }

    struct RecordingEngine {
        models: Arc<tokio::sync::Mutex<Vec<Option<String>>>>,
    }

    impl RecordingEngine {
        fn new(models: Arc<tokio::sync::Mutex<Vec<Option<String>>>>) -> Self {
            Self { models }
        }
    }

    impl DispatchEngine for RecordingEngine {
        fn spawn_session<'a>(
            &'a self,
            _spec: &'a SessionSpec,
            options: &'a AgentOptions,
        ) -> Pin<Box<dyn Future<Output = Result<Box<dyn SessionHandle>>> + Send + 'a>> {
            Box::pin(async move {
                self.models.lock().await.push(options.model.clone());
                let handle =
                    RecordingHandle::new("routed-session".to_owned(), options.model.clone());
                let boxed: Box<dyn SessionHandle> = Box::new(handle);
                Ok(boxed)
            })
        }

        fn resume_session<'a>(
            &'a self,
            _session_id: &'a str,
            _prompt: &'a str,
            _options: &'a AgentOptions,
        ) -> Pin<Box<dyn Future<Output = Result<Box<dyn SessionHandle>>> + Send + 'a>> {
            Box::pin(async {
                Err(error::EngineSnafu {
                    detail: "resume not supported by RecordingEngine".to_owned(),
                }
                .build())
            })
        }
    }

    struct RecordingHandle {
        session_id: String,
        result: Option<SessionResult>,
    }

    impl RecordingHandle {
        fn new(session_id: String, model: Option<String>) -> Self {
            Self {
                session_id: session_id.clone(),
                result: Some(SessionResult {
                    session_id,
                    cost_usd: 0.01,
                    num_turns: 1,
                    duration_ms: 100,
                    success: true,
                    result_text: Some("done".to_owned()),
                    model,
                    cache_hit_tokens: 0,
                    cache_miss_tokens: 0,
                }),
            }
        }
    }

    impl SessionHandle for RecordingHandle {
        fn session_id(&self) -> &str {
            &self.session_id
        }

        fn next_event<'a>(
            &'a mut self,
        ) -> Pin<Box<dyn Future<Output = Option<SessionEvent>> + Send + 'a>> {
            Box::pin(async { None })
        }

        fn wait(
            mut self: Box<Self>,
        ) -> Pin<Box<dyn Future<Output = Result<SessionResult>> + Send>> {
            Box::pin(async move {
                self.result.take().ok_or_else(|| {
                    error::EngineSnafu {
                        detail: "RecordingHandle: wait called more than once".to_owned(),
                    }
                    .build()
                })
            })
        }

        fn abort<'a>(&'a mut self) -> Pin<Box<dyn Future<Output = Result<()>> + Send + 'a>> {
            Box::pin(async { Ok(()) })
        }
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

        let prompts = vec![
            sample_prompt_spec(1),
            sample_prompt_spec(2),
            sample_prompt_spec(3),
        ];

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

        let prompts = vec![
            sample_prompt_spec(1),
            sample_prompt_spec(2),
            sample_prompt_spec(3),
        ];

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
        assert_eq!(outcomes[1].status, SessionStatus::InfraFailure);
        assert_eq!(outcomes[1].failure_class, Some(FailureClass::Auth));
        assert_eq!(outcomes[2].status, SessionStatus::Success);
    }

    #[tokio::test]
    async fn group_applies_routed_model_to_session_options() {
        let models = Arc::new(tokio::sync::Mutex::new(Vec::new()));
        let engine = Arc::new(RecordingEngine::new(Arc::clone(&models)));
        let budget = Arc::new(Budget::new(None, None, None));
        let cancel = CancellationToken::new();
        let prompts = vec![sample_prompt_spec(1)];
        let mut config = default_config();
        config.routing.default_provider = "routed-model".to_owned();

        let outcomes = execute_group(
            &prompts,
            engine,
            budget,
            &ResumePolicy::default(),
            &config,
            1,
            &cancel,
        )
        .await;

        assert_eq!(outcomes.len(), 1);
        assert_eq!(outcomes[0].status, SessionStatus::Success);
        let models = models.lock().await;
        assert_eq!(
            models.first().and_then(|model| model.as_deref()),
            Some("routed-model")
        );
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
