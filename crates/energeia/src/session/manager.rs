// WHY: Per-prompt executor that spawns a session, monitors events, enforces
// budget limits, and handles multi-stage resume escalation. Produces a
// SessionOutcome that records cost, turns, duration, and terminal status.

use std::sync::Arc;
use std::time::Instant;

use crate::budget::BudgetStatus;
use crate::engine::{DispatchEngine, SessionSpec};
use crate::error::{self, Result};
use crate::prompt::PromptSpec;
use crate::resume::ResumePolicy;
use crate::types::{Budget, SessionOutcome, SessionStatus};

use super::events::{self, StreamOutcome, extract_pr_url};
use super::options::EngineConfig;

// ---------------------------------------------------------------------------
// SessionManager
// ---------------------------------------------------------------------------

/// Per-prompt session executor with budget enforcement and resume escalation.
///
/// Owns a [`DispatchEngine`], shared [`Budget`], and [`ResumePolicy`] to
/// manage a single prompt through initial execution and graduated resume
/// stages when the session fails or exhausts its turn budget.
pub struct SessionManager {
    engine: Arc<dyn DispatchEngine>,
    budget: Arc<Budget>,
    resume_policy: ResumePolicy,
}

impl SessionManager {
    /// Create a new session manager.
    #[must_use]
    pub fn new(
        engine: Arc<dyn DispatchEngine>,
        budget: Arc<Budget>,
        resume_policy: ResumePolicy,
    ) -> Self {
        Self {
            engine,
            budget,
            resume_policy,
        }
    }

    /// Execute a prompt through initial session and resume stages.
    ///
    /// 1. Spawns a session via the engine
    /// 2. Monitors events from the session handle
    /// 3. Tracks cost/turns against the shared budget
    /// 4. On budget warning: logs, continues
    /// 5. On budget exceeded: aborts session, returns `BudgetExceeded`
    /// 6. On session failure: checks resume policy for next stage
    /// 7. Resumes with escalating urgency message
    /// 8. On resume exhaustion: returns `Stuck`
    /// 9. On success: returns `Success` with PR URL if found
    ///
    /// # Errors
    ///
    /// Returns [`Error::SpawnFailed`](crate::error::Error::SpawnFailed) if the
    /// initial session cannot be created.
    #[expect(
        clippy::too_many_lines,
        reason = "session lifecycle is inherently sequential with many branches"
    )]
    pub async fn execute(
        &self,
        prompt: &PromptSpec,
        options: &EngineConfig,
    ) -> Result<SessionOutcome> {
        let start = Instant::now();
        let mut total_cost = 0.0_f64;
        let mut total_turns = 0_u32;
        let mut resume_count = 0_u32;
        let mut pr_url: Option<String> = None;

        // --- Initial session ---

        let spec = SessionSpec {
            prompt: prompt.body.clone(),
            system_prompt: options.options.system_prompt.clone(),
            cwd: options.options.cwd.clone(),
        };

        let initial_opts = options.to_agent_options();

        let mut handle = self
            .engine
            .spawn_session(&spec, &initial_opts)
            .await
            .map_err(|e| {
                error::SpawnFailedSnafu {
                    prompt_number: prompt.number,
                    detail: e.to_string(),
                }
                .build()
            })?;

        let mut session_id = Some(handle.session_id().to_owned());

        let stream_result = events::process_events(&mut handle, options.idle_timeout).await;

        // NOTE: Wait for the session to produce its final result.
        let session_result = handle.wait().await;

        let (run_cost, run_turns, run_success, result_text) =
            extract_run_metrics(session_result, &stream_result);

        total_cost += run_cost;
        total_turns += run_turns;
        self.budget.record(run_cost, run_turns);

        // NOTE: Extract PR URL from all text fragments and result text.
        let all_text = collect_text(&stream_result, result_text.as_deref());
        if let Some(url) = extract_pr_url(&all_text) {
            pr_url = Some(url.to_owned());
        }

        // --- Check stream outcome for early exit ---

        if let StreamOutcome::Timeout { elapsed, .. } = &stream_result {
            tracing::warn!(
                prompt_number = prompt.number,
                elapsed_secs = elapsed.as_secs(),
                "session stalled (no events within timeout)"
            );
            return Ok(build_outcome(
                prompt.number,
                SessionStatus::Stuck,
                session_id,
                total_cost,
                total_turns,
                start,
                resume_count,
                None,
                Some(format!("timeout: no events for {}s", elapsed.as_secs())),
            ));
        }

        if let StreamOutcome::Error { message, .. } = &stream_result
            && !run_success
        {
            // NOTE: Error during initial run with failed result — try resume.
            tracing::warn!(
                prompt_number = prompt.number,
                error = %message,
                "initial session error, checking resume policy"
            );
        }

        // --- Check if initial run succeeded ---

        if run_success {
            return Ok(build_outcome(
                prompt.number,
                SessionStatus::Success,
                session_id,
                total_cost,
                total_turns,
                start,
                resume_count,
                pr_url,
                None,
            ));
        }

        // --- Budget check before entering resume loop ---

        if let BudgetStatus::Exceeded(reason) = self.budget.check() {
            tracing::warn!(
                prompt_number = prompt.number,
                reason = %reason,
                "budget exceeded after initial run"
            );
            return Ok(build_outcome(
                prompt.number,
                SessionStatus::BudgetExceeded,
                session_id,
                total_cost,
                total_turns,
                start,
                resume_count,
                None,
                Some(reason),
            ));
        }

        if let BudgetStatus::Warning(msg) = self.budget.check() {
            tracing::info!(
                prompt_number = prompt.number,
                warning = %msg,
                "budget warning after initial run, continuing"
            );
        }

        // --- Resume loop ---

        loop {
            let Some(stage) = self.resume_policy.next_stage(total_turns) else {
                // NOTE: All resume stages exhausted — mark as Stuck.
                tracing::warn!(
                    prompt_number = prompt.number,
                    total_turns,
                    resume_count,
                    "all resume stages exhausted, marking as Stuck"
                );
                return Ok(build_outcome(
                    prompt.number,
                    SessionStatus::Stuck,
                    session_id,
                    total_cost,
                    total_turns,
                    start,
                    resume_count,
                    None,
                    Some("resume policy exhausted".to_owned()),
                ));
            };

            resume_count += 1;

            tracing::info!(
                prompt_number = prompt.number,
                resume_count,
                stage_max_turns = stage.max_turns,
                total_turns,
                "resuming session with escalation"
            );

            // NOTE: Resume the existing session with the stage's escalation message.
            let sid = session_id.as_deref().unwrap_or("unknown");
            let resume_opts = options.options_with_turns(stage.max_turns);

            let mut handle = match self
                .engine
                .resume_session(sid, &stage.message, &resume_opts)
                .await
            {
                Ok(h) => h,
                Err(e) => {
                    tracing::warn!(
                        prompt_number = prompt.number,
                        error = %e,
                        "resume failed"
                    );
                    return Ok(build_outcome(
                        prompt.number,
                        SessionStatus::Failed,
                        session_id,
                        total_cost,
                        total_turns,
                        start,
                        resume_count,
                        None,
                        Some(format!("resume failed: {e}")),
                    ));
                }
            };

            session_id = Some(handle.session_id().to_owned());

            let stream_result = events::process_events(&mut handle, options.idle_timeout).await;

            let session_result = handle.wait().await;

            let (run_cost, run_turns, run_success, result_text) =
                extract_run_metrics(session_result, &stream_result);

            total_cost += run_cost;
            total_turns += run_turns;
            self.budget.record(run_cost, run_turns);

            // NOTE: Check for PR URL in resume output.
            let all_text = collect_text(&stream_result, result_text.as_deref());
            if let Some(url) = extract_pr_url(&all_text) {
                pr_url = Some(url.to_owned());
            }

            // --- Timeout in resume ---

            if let StreamOutcome::Timeout { elapsed, .. } = &stream_result {
                tracing::warn!(
                    prompt_number = prompt.number,
                    elapsed_secs = elapsed.as_secs(),
                    "session stalled during resume"
                );
                return Ok(build_outcome(
                    prompt.number,
                    SessionStatus::Stuck,
                    session_id,
                    total_cost,
                    total_turns,
                    start,
                    resume_count,
                    None,
                    Some(format!(
                        "timeout during resume: no events for {}s",
                        elapsed.as_secs()
                    )),
                ));
            }

            // --- Success check ---

            if run_success {
                return Ok(build_outcome(
                    prompt.number,
                    SessionStatus::Success,
                    session_id,
                    total_cost,
                    total_turns,
                    start,
                    resume_count,
                    pr_url,
                    None,
                ));
            }

            // --- Budget check before next resume ---

            match self.budget.check() {
                BudgetStatus::Exceeded(reason) => {
                    tracing::warn!(
                        prompt_number = prompt.number,
                        reason = %reason,
                        "budget exceeded during resume loop"
                    );
                    return Ok(build_outcome(
                        prompt.number,
                        SessionStatus::BudgetExceeded,
                        session_id,
                        total_cost,
                        total_turns,
                        start,
                        resume_count,
                        None,
                        Some(reason),
                    ));
                }
                BudgetStatus::Warning(msg) => {
                    tracing::info!(
                        prompt_number = prompt.number,
                        warning = %msg,
                        resume_count,
                        "budget warning, continuing resume"
                    );
                }
                BudgetStatus::Ok => {}
            }

            // NOTE: Loop continues to the next resume stage.
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Extract run metrics from a session result, falling back to the stream
/// accumulator if the result is unavailable.
fn extract_run_metrics(
    session_result: Result<crate::engine::SessionResult>,
    stream_result: &StreamOutcome,
) -> (f64, u32, bool, Option<String>) {
    if let Ok(result) = session_result {
        (
            result.cost_usd,
            result.num_turns,
            result.success,
            result.result_text,
        )
    } else {
        let acc = accumulator_from(stream_result);
        (acc.cost_usd, acc.num_turns, false, None)
    }
}

/// Extract the accumulator from any `StreamOutcome` variant.
fn accumulator_from(outcome: &StreamOutcome) -> &super::events::EventAccumulator {
    match outcome {
        StreamOutcome::Complete(acc)
        | StreamOutcome::Timeout {
            accumulator: acc, ..
        }
        | StreamOutcome::Error {
            accumulator: acc, ..
        } => acc,
    }
}

/// Collect all text from a stream outcome and optional result text.
fn collect_text(outcome: &StreamOutcome, result_text: Option<&str>) -> String {
    let fragments = &accumulator_from(outcome).text_fragments;
    let mut text = fragments.join("");
    if let Some(rt) = result_text {
        text.push(' ');
        text.push_str(rt);
    }
    text
}

/// Build a [`SessionOutcome`] from the accumulated session state.
fn build_outcome(
    prompt_number: u32,
    status: SessionStatus,
    session_id: Option<String>,
    cost_usd: f64,
    num_turns: u32,
    start: Instant,
    resume_count: u32,
    pr_url: Option<String>,
    error: Option<String>,
) -> SessionOutcome {
    SessionOutcome {
        prompt_number,
        status,
        session_id,
        cost_usd,
        num_turns,
        duration_ms: u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX),
        resume_count,
        pr_url,
        error,
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
mod tests {
    use super::*;
    use crate::engine::{AgentOptions, SessionEvent, SessionResult};
    use crate::http::mock::{MockEngine, MockOutcome};
    use crate::resume::ResumeStage;

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
            events: vec![
                SessionEvent::TextDelta {
                    text: "working on it".to_owned(),
                },
                SessionEvent::TurnComplete { turn: turns },
            ],
            result: SessionResult {
                session_id: session_id.to_owned(),
                cost_usd: cost,
                num_turns: turns,
                duration_ms: 1000,
                success: true,
                result_text: Some("task complete".to_owned()),
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
                duration_ms: 1000,
                success: false,
                result_text: Some("stuck".to_owned()),
            },
        }
    }

    fn success_with_pr(session_id: &str, cost: f64, turns: u32) -> MockOutcome {
        MockOutcome::Success {
            events: vec![
                SessionEvent::TextDelta {
                    text: "Created https://github.com/acme/repo/pull/42".to_owned(),
                },
                SessionEvent::TurnComplete { turn: turns },
            ],
            result: SessionResult {
                session_id: session_id.to_owned(),
                cost_usd: cost,
                num_turns: turns,
                duration_ms: 1000,
                success: true,
                result_text: Some("PR: https://github.com/acme/repo/pull/42".to_owned()),
            },
        }
    }

    fn two_stage_policy() -> ResumePolicy {
        ResumePolicy {
            stages: vec![
                ResumeStage {
                    max_turns: 10,
                    message: "Continue the task.".to_owned(),
                },
                ResumeStage {
                    max_turns: 5,
                    message: "Final attempt. Commit and push.".to_owned(),
                },
            ],
        }
    }

    fn default_config() -> EngineConfig {
        EngineConfig::new(AgentOptions::new())
    }

    // ---- Success on first run ----

    #[tokio::test]
    async fn execute_success_first_run() {
        let engine = Arc::new(MockEngine::new(vec![success_outcome("sess-1", 0.50, 10)]));
        let budget = Arc::new(Budget::new(Some(10.0), Some(100), None));

        let mgr = SessionManager::new(engine, budget.clone(), ResumePolicy::default());

        let outcome = mgr
            .execute(&sample_prompt_spec(1), &default_config())
            .await
            .unwrap();

        assert_eq!(outcome.status, SessionStatus::Success);
        assert_eq!(outcome.prompt_number, 1);
        assert_eq!(outcome.session_id.as_deref(), Some("sess-1"));
        assert!((outcome.cost_usd - 0.50).abs() < 0.01);
        assert_eq!(outcome.num_turns, 10);
        assert_eq!(outcome.resume_count, 0);
        assert!(outcome.error.is_none());

        // Budget should be recorded.
        assert!((budget.current_cost_usd() - 0.50).abs() < 0.01);
        assert_eq!(budget.current_turns(), 10);
    }

    // ---- PR URL extraction ----

    #[tokio::test]
    async fn execute_extracts_pr_url() {
        let engine = Arc::new(MockEngine::new(vec![success_with_pr("sess-pr", 0.30, 5)]));
        let budget = Arc::new(Budget::new(None, None, None));

        let mgr = SessionManager::new(engine, budget, ResumePolicy::default());

        let outcome = mgr
            .execute(&sample_prompt_spec(1), &default_config())
            .await
            .unwrap();

        assert_eq!(outcome.status, SessionStatus::Success);
        assert_eq!(
            outcome.pr_url.as_deref(),
            Some("https://github.com/acme/repo/pull/42")
        );
    }

    // ---- Resume on failure then success ----

    #[tokio::test]
    async fn execute_resumes_on_failure_then_succeeds() {
        let engine = Arc::new(MockEngine::new(vec![
            // Initial run: fails.
            failure_outcome("sess-1", 0.20, 5),
            // Resume stage 1: succeeds.
            success_outcome("sess-1-r1", 0.30, 8),
        ]));
        let budget = Arc::new(Budget::new(Some(10.0), Some(100), None));

        let mgr = SessionManager::new(engine, budget.clone(), two_stage_policy());

        let outcome = mgr
            .execute(&sample_prompt_spec(1), &default_config())
            .await
            .unwrap();

        assert_eq!(outcome.status, SessionStatus::Success);
        assert_eq!(outcome.resume_count, 1);
        assert!((outcome.cost_usd - 0.50).abs() < 0.01);
        assert_eq!(outcome.num_turns, 13); // 5 + 8
    }

    // ---- Resume exhaustion -> Stuck ----

    #[tokio::test]
    async fn execute_stuck_when_resume_exhausted() {
        let engine = Arc::new(MockEngine::new(vec![
            // Initial run: fails (5 turns).
            failure_outcome("sess-1", 0.10, 5),
            // Resume stage 1: fails (10 turns -> cumulative 15 > stage threshold).
            failure_outcome("sess-1-r1", 0.10, 10),
            // NOTE: Two-stage policy has thresholds at 10 and 15 cumulative turns.
            // After 15 turns, next_stage(15) returns None -> Stuck.
        ]));
        let budget = Arc::new(Budget::new(Some(50.0), Some(500), None));

        let mgr = SessionManager::new(engine, budget, two_stage_policy());

        let outcome = mgr
            .execute(&sample_prompt_spec(1), &default_config())
            .await
            .unwrap();

        assert_eq!(outcome.status, SessionStatus::Stuck);
        assert!(outcome.error.as_deref().unwrap().contains("exhausted"));
    }

    // ---- Budget exceeded ----

    #[tokio::test]
    async fn execute_budget_exceeded() {
        // Budget of $0.50, session costs $0.60.
        let engine = Arc::new(MockEngine::new(vec![failure_outcome("sess-1", 0.60, 20)]));
        let budget = Arc::new(Budget::new(Some(0.50), None, None));

        let mgr = SessionManager::new(engine, budget, two_stage_policy());

        let outcome = mgr
            .execute(&sample_prompt_spec(1), &default_config())
            .await
            .unwrap();

        assert_eq!(outcome.status, SessionStatus::BudgetExceeded);
    }

    // ---- Budget warning then continue ----

    #[tokio::test]
    async fn execute_budget_warning_continues() {
        // Budget of $1.00. First run costs $0.85 (warning at 80%), resume succeeds.
        let engine = Arc::new(MockEngine::new(vec![
            failure_outcome("sess-1", 0.85, 5),
            success_outcome("sess-1-r1", 0.10, 3),
        ]));
        let budget = Arc::new(Budget::new(Some(1.00), Some(100), None));

        let mgr = SessionManager::new(engine, budget, two_stage_policy());

        let outcome = mgr
            .execute(&sample_prompt_spec(1), &default_config())
            .await
            .unwrap();

        assert_eq!(outcome.status, SessionStatus::Success);
        assert_eq!(outcome.resume_count, 1);
    }

    // ---- Spawn failure ----

    #[tokio::test]
    async fn execute_spawn_failure() {
        let engine = Arc::new(MockEngine::new(vec![MockOutcome::SpawnFailure {
            detail: "auth expired".to_owned(),
        }]));
        let budget = Arc::new(Budget::new(None, None, None));

        let mgr = SessionManager::new(engine, budget, ResumePolicy::default());

        let result = mgr.execute(&sample_prompt_spec(1), &default_config()).await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("auth expired"));
    }

    // ---- Error event during initial session ----

    #[tokio::test]
    async fn execute_error_event_triggers_resume() {
        let engine = Arc::new(MockEngine::new(vec![
            MockOutcome::Success {
                events: vec![SessionEvent::Error {
                    message: "tool failed".to_owned(),
                }],
                result: SessionResult {
                    session_id: "sess-err".to_owned(),
                    cost_usd: 0.05,
                    num_turns: 2,
                    duration_ms: 500,
                    success: false,
                    result_text: None,
                },
            },
            success_outcome("sess-err-r1", 0.10, 5),
        ]));
        let budget = Arc::new(Budget::new(Some(10.0), Some(100), None));

        let mgr = SessionManager::new(engine, budget, two_stage_policy());

        let outcome = mgr
            .execute(&sample_prompt_spec(1), &default_config())
            .await
            .unwrap();
        assert_eq!(outcome.status, SessionStatus::Success);
        assert_eq!(outcome.resume_count, 1);
    }

    // ---- Budget exceeded during resume loop ----

    #[tokio::test]
    async fn execute_budget_exceeded_during_resume() {
        // Budget of $0.50. First run $0.20, resume costs $0.40 -> total $0.60 > $0.50.
        let engine = Arc::new(MockEngine::new(vec![
            failure_outcome("sess-1", 0.20, 3),
            failure_outcome("sess-1-r1", 0.40, 5),
        ]));
        let budget = Arc::new(Budget::new(Some(0.50), None, None));

        let mgr = SessionManager::new(engine, budget, two_stage_policy());

        let outcome = mgr
            .execute(&sample_prompt_spec(1), &default_config())
            .await
            .unwrap();
        assert_eq!(outcome.status, SessionStatus::BudgetExceeded);
        assert_eq!(outcome.resume_count, 1);
    }
}
