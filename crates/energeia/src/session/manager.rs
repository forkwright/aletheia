// kanon:ignore RUST/file-too-long — session manager orchestrates spawn, monitor, budget enforcement, and resume escalation as a cohesive lifecycle; splitting would fragment the state machine
// WHY: Per-prompt executor that spawns a session, monitors events, enforces
// budget limits, and handles multi-stage resume escalation. Produces a
// SessionOutcome that records cost, turns, duration, and terminal status.

use std::sync::Arc;
use std::time::Instant;

use crate::budget::BudgetStatus;
use crate::engine::{DispatchEngine, SessionSpec};
use crate::error::Result;
use crate::prompt::PromptSpec;
use crate::resume::ResumePolicy;
use crate::types::{Budget, FailureClass, SessionOutcome, SessionStatus};

use super::events::{self, StreamOutcome, extract_pr_url};
use super::options::{ChildSessionProgress, ChildSessionProgressStatus, EngineConfig};

/// Per-prompt session executor with budget enforcement and resume escalation.
///
/// Owns a [`DispatchEngine`], shared [`Budget`], and [`ResumePolicy`] to
/// manage a single prompt through initial execution and graduated resume
/// stages when the session fails or exhausts its turn budget.
pub(crate) struct SessionManager {
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
    /// initial session cannot be created, if the session is cancelled,
    /// or if the engine encounters an error during execution.
    ///
    /// # Cancel safety
    ///
    /// Not cancel-safe. If cancelled after spawning a session but before
    /// collecting its results, the session continues running but its
    /// outcome is lost. Do not use in `select!` branches.
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
        let mut last_output_excerpt: Option<String>;

        let spec = if let Some(ref components) = prompt.prompt_components {
            SessionSpec {
                prompt: components.dynamic_suffix.clone(),
                system_prompt: Some(components.static_prefix.clone()),
                cwd: options.options.cwd.clone(),
                prompt_components: Some(components.clone()),
                output_format: prompt.output_format.clone(),
            }
        } else {
            SessionSpec {
                prompt: prompt.body.clone(),
                system_prompt: options.options.system_prompt.clone(),
                cwd: options.options.cwd.clone(),
                prompt_components: None,
                output_format: prompt.output_format.clone(),
            }
        };

        let initial_opts = options.to_agent_options();

        let model = initial_opts.model.clone();

        let mut handle = match self.engine.spawn_session(&spec, &initial_opts).await {
            Ok(handle) => handle,
            Err(e) => {
                let detail = e.to_string();
                let failure_class = classify_failure(&detail, FailureClass::WorkerRuntime);
                tracing::warn!(
                    prompt_number = prompt.number,
                    failure_class = %failure_class,
                    error = %detail,
                    "session spawn failed"
                );
                let outcome = build_outcome(
                    prompt.number,
                    status_for_failure_class(failure_class),
                    None,
                    0.0,
                    0,
                    start,
                    resume_count,
                    None,
                    Some(detail),
                    Some(failure_class),
                    model.clone(),
                    prompt.blast_radius.clone(),
                    0,
                    0,
                    None,
                );
                emit_child_terminal(options, &outcome, None);
                return Ok(outcome);
            }
        };

        let mut session_id = Some(handle.session_id().to_owned());
        emit_child_progress(
            options,
            prompt.number,
            ChildSessionProgressStatus::Started,
            session_id.as_deref(),
            None,
        );

        let stream_result =
            events::process_events(&mut handle, options.idle_timeout, options.cancel.as_ref())
                .await;

        if let Some(outcome) = abort_terminal_stream(
            &mut handle,
            &stream_result,
            prompt.number,
            session_id.clone(),
            total_cost,
            total_turns,
            start,
            resume_count,
            model.clone(),
            prompt.blast_radius.clone(),
        )
        .await?
        {
            emit_child_terminal(
                options,
                &outcome,
                output_excerpt_from_stream(&stream_result),
            );
            return Ok(outcome);
        }

        let session_result = handle.wait().await;
        let run_metrics = extract_run_metrics(session_result, &stream_result);
        let stream_failure_class = failure_class_from_stream(&stream_result);
        let stream_error = stream_error_message(&stream_result);

        let effective_model = run_metrics.model.clone().or_else(|| model.clone());

        total_cost += run_metrics.cost_usd;
        total_turns += run_metrics.num_turns;
        self.budget
            .record(run_metrics.cost_usd, run_metrics.num_turns);

        let all_text = collect_text(&stream_result, run_metrics.result_text.as_deref());
        last_output_excerpt = output_excerpt(&all_text);
        let structured_output = parse_structured_output(
            prompt.number,
            prompt.output_format.as_ref(),
            run_metrics.result_text.as_deref(),
        );
        if let Some(url) = extract_pr_url(&all_text) {
            pr_url = Some(url.to_owned());
        }

        if let StreamOutcome::Error { message, .. } = &stream_result
            && !run_metrics.success
        {
            tracing::warn!(
                prompt_number = prompt.number,
                error = %message,
                "initial session error, checking resume policy"
            );
        }

        if run_metrics.success {
            let outcome = build_outcome(
                prompt.number,
                SessionStatus::Success,
                session_id,
                total_cost,
                total_turns,
                start,
                resume_count,
                pr_url,
                None,
                None,
                effective_model.clone(),
                prompt.blast_radius.clone(),
                run_metrics.cache_hit_tokens,
                run_metrics.cache_miss_tokens,
                structured_output,
            );
            emit_child_terminal(options, &outcome, last_output_excerpt);
            return Ok(outcome);
        }

        let failure_class = run_metrics
            .failure_class
            .or(stream_failure_class)
            .unwrap_or(FailureClass::Provider);
        let failure_detail = run_metrics.error.or(stream_error);
        if failure_class.is_infrastructure() {
            let outcome = build_outcome(
                prompt.number,
                SessionStatus::InfraFailure,
                session_id,
                total_cost,
                total_turns,
                start,
                resume_count,
                None,
                failure_detail,
                Some(failure_class),
                effective_model.clone(),
                prompt.blast_radius.clone(),
                run_metrics.cache_hit_tokens,
                run_metrics.cache_miss_tokens,
                None,
            );
            emit_child_terminal(options, &outcome, last_output_excerpt);
            return Ok(outcome);
        }
        let mut last_failure_class = Some(failure_class);
        let mut last_failure_detail = failure_detail;
        let mut last_cache_hit_tokens = run_metrics.cache_hit_tokens;
        let mut last_cache_miss_tokens = run_metrics.cache_miss_tokens;

        if let BudgetStatus::Exceeded(reason) = self.budget.check() {
            tracing::warn!(
                prompt_number = prompt.number,
                reason = %reason,
                "budget exceeded after initial run"
            );
            let outcome = build_outcome(
                prompt.number,
                SessionStatus::BudgetExceeded,
                session_id,
                total_cost,
                total_turns,
                start,
                resume_count,
                None,
                Some(reason),
                None,
                effective_model.clone(),
                prompt.blast_radius.clone(),
                run_metrics.cache_hit_tokens,
                run_metrics.cache_miss_tokens,
                None,
            );
            emit_child_terminal(options, &outcome, last_output_excerpt);
            return Ok(outcome);
        }

        if let BudgetStatus::Warning(msg) = self.budget.check() {
            tracing::info!(
                prompt_number = prompt.number,
                warning = %msg,
                "budget warning after initial run, continuing"
            );
        }

        loop {
            let Some(stage) = self.resume_policy.next_stage(total_turns) else {
                tracing::warn!(
                    prompt_number = prompt.number,
                    total_turns,
                    resume_count,
                    "all resume stages exhausted, marking as Stuck"
                );
                let outcome = build_outcome(
                    prompt.number,
                    SessionStatus::Stuck,
                    session_id,
                    total_cost,
                    total_turns,
                    start,
                    resume_count,
                    None,
                    Some(
                        last_failure_detail
                            .clone()
                            .unwrap_or_else(|| "resume policy exhausted".to_owned()),
                    ),
                    last_failure_class.or(Some(FailureClass::Provider)),
                    effective_model.clone(),
                    prompt.blast_radius.clone(),
                    last_cache_hit_tokens,
                    last_cache_miss_tokens,
                    None,
                );
                emit_child_terminal(options, &outcome, last_output_excerpt);
                return Ok(outcome);
            };

            resume_count += 1;

            tracing::info!(
                prompt_number = prompt.number,
                resume_count,
                stage_max_turns = stage.max_turns,
                total_turns,
                "resuming session with escalation"
            );

            let sid = session_id.as_deref().unwrap_or("unknown");
            let resume_opts = options.options_with_turns(stage.max_turns);

            let mut handle = match self
                .engine
                .resume_session(sid, &stage.message, &resume_opts)
                .await
            {
                Ok(h) => h,
                Err(e) => {
                    let detail = format!("resume failed: {e}");
                    let failure_class = classify_failure(&detail, FailureClass::WorkerRuntime);
                    tracing::warn!(
                        prompt_number = prompt.number,
                        failure_class = %failure_class,
                        error = %e,
                        "resume failed"
                    );
                    let outcome = build_outcome(
                        prompt.number,
                        status_for_failure_class(failure_class),
                        session_id,
                        total_cost,
                        total_turns,
                        start,
                        resume_count,
                        None,
                        Some(detail),
                        Some(failure_class),
                        effective_model.clone(),
                        prompt.blast_radius.clone(),
                        last_cache_hit_tokens,
                        last_cache_miss_tokens,
                        None,
                    );
                    emit_child_terminal(options, &outcome, last_output_excerpt);
                    return Ok(outcome);
                }
            };

            session_id = Some(handle.session_id().to_owned());
            emit_child_progress(
                options,
                prompt.number,
                ChildSessionProgressStatus::Started,
                session_id.as_deref(),
                None,
            );

            let stream_result =
                events::process_events(&mut handle, options.idle_timeout, options.cancel.as_ref())
                    .await;

            if let Some(outcome) = abort_terminal_stream(
                &mut handle,
                &stream_result,
                prompt.number,
                session_id.clone(),
                total_cost,
                total_turns,
                start,
                resume_count,
                effective_model.clone(),
                prompt.blast_radius.clone(),
            )
            .await?
            {
                emit_child_terminal(
                    options,
                    &outcome,
                    output_excerpt_from_stream(&stream_result),
                );
                return Ok(outcome);
            }

            let session_result = handle.wait().await;
            let run_metrics = extract_run_metrics(session_result, &stream_result);
            let stream_failure_class = failure_class_from_stream(&stream_result);
            let stream_error = stream_error_message(&stream_result);

            let effective_model = run_metrics
                .model
                .clone()
                .or_else(|| effective_model.clone());

            total_cost += run_metrics.cost_usd;
            total_turns += run_metrics.num_turns;
            self.budget
                .record(run_metrics.cost_usd, run_metrics.num_turns);
            last_cache_hit_tokens = run_metrics.cache_hit_tokens;
            last_cache_miss_tokens = run_metrics.cache_miss_tokens;

            let all_text = collect_text(&stream_result, run_metrics.result_text.as_deref());
            last_output_excerpt = output_excerpt(&all_text);
            let structured_output = parse_structured_output(
                prompt.number,
                prompt.output_format.as_ref(),
                run_metrics.result_text.as_deref(),
            );
            if let Some(url) = extract_pr_url(&all_text) {
                pr_url = Some(url.to_owned());
            }

            if run_metrics.success {
                let outcome = build_outcome(
                    prompt.number,
                    SessionStatus::Success,
                    session_id,
                    total_cost,
                    total_turns,
                    start,
                    resume_count,
                    pr_url,
                    None,
                    None,
                    effective_model.clone(),
                    prompt.blast_radius.clone(),
                    run_metrics.cache_hit_tokens,
                    run_metrics.cache_miss_tokens,
                    structured_output,
                );
                emit_child_terminal(options, &outcome, last_output_excerpt);
                return Ok(outcome);
            }

            let failure_class = run_metrics
                .failure_class
                .or(stream_failure_class)
                .unwrap_or(FailureClass::Provider);
            let failure_detail = run_metrics.error.or(stream_error);
            if failure_class.is_infrastructure() {
                let outcome = build_outcome(
                    prompt.number,
                    SessionStatus::InfraFailure,
                    session_id,
                    total_cost,
                    total_turns,
                    start,
                    resume_count,
                    None,
                    failure_detail,
                    Some(failure_class),
                    effective_model.clone(),
                    prompt.blast_radius.clone(),
                    run_metrics.cache_hit_tokens,
                    run_metrics.cache_miss_tokens,
                    None,
                );
                emit_child_terminal(options, &outcome, last_output_excerpt);
                return Ok(outcome);
            }
            last_failure_class = Some(failure_class);
            last_failure_detail = failure_detail;

            match self.budget.check() {
                BudgetStatus::Exceeded(reason) => {
                    tracing::warn!(
                        prompt_number = prompt.number,
                        reason = %reason,
                        "budget exceeded during resume loop"
                    );
                    let outcome = build_outcome(
                        prompt.number,
                        SessionStatus::BudgetExceeded,
                        session_id,
                        total_cost,
                        total_turns,
                        start,
                        resume_count,
                        None,
                        Some(reason),
                        None,
                        effective_model.clone(),
                        prompt.blast_radius.clone(),
                        run_metrics.cache_hit_tokens,
                        run_metrics.cache_miss_tokens,
                        None,
                    );
                    emit_child_terminal(options, &outcome, last_output_excerpt);
                    return Ok(outcome);
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
        }
    }
}

// ── Helpers ──

/// Extract run metrics from a session result, falling back to the stream
/// accumulator if the result is unavailable.
#[derive(Debug)]
struct RunMetrics {
    cost_usd: f64,
    num_turns: u32,
    success: bool,
    result_text: Option<String>,
    model: Option<String>,
    cache_hit_tokens: u64,
    cache_miss_tokens: u64,
    error: Option<String>,
    failure_class: Option<FailureClass>,
}

fn extract_run_metrics(
    session_result: Result<crate::engine::SessionResult>,
    stream_result: &StreamOutcome,
) -> RunMetrics {
    match session_result {
        Ok(result) => RunMetrics {
            cost_usd: result.cost_usd,
            num_turns: result.num_turns,
            success: result.success,
            result_text: result.result_text,
            model: result.model,
            cache_hit_tokens: result.cache_hit_tokens,
            cache_miss_tokens: result.cache_miss_tokens,
            error: None,
            failure_class: None,
        },
        Err(error) => {
            let acc = accumulator_from(stream_result);
            let detail = error.to_string();
            let failure_class = classify_failure(&detail, FailureClass::WorkerRuntime);
            RunMetrics {
                cost_usd: acc.cost_usd,
                num_turns: acc.num_turns,
                success: false,
                result_text: None,
                model: None,
                cache_hit_tokens: 0,
                cache_miss_tokens: 0,
                error: Some(detail),
                failure_class: Some(failure_class),
            }
        }
    }
}

fn failure_class_from_stream(outcome: &StreamOutcome) -> Option<FailureClass> {
    match outcome {
        StreamOutcome::Error { message, .. } => {
            Some(classify_failure(message, FailureClass::Provider))
        }
        StreamOutcome::Complete(_)
        | StreamOutcome::Timeout { .. }
        | StreamOutcome::Cancelled { .. } => None,
    }
}

fn stream_error_message(outcome: &StreamOutcome) -> Option<String> {
    match outcome {
        StreamOutcome::Error { message, .. } => Some(message.clone()),
        StreamOutcome::Complete(_)
        | StreamOutcome::Timeout { .. }
        | StreamOutcome::Cancelled { .. } => None,
    }
}

fn status_for_failure_class(failure_class: FailureClass) -> SessionStatus {
    if failure_class.is_infrastructure() {
        SessionStatus::InfraFailure
    } else {
        SessionStatus::Failed
    }
}

fn classify_failure(detail: &str, fallback: FailureClass) -> FailureClass {
    let lower = detail.to_ascii_lowercase();
    if contains_any(
        &lower,
        &[
            "auth",
            "unauthorized",
            "unauthorised",
            "forbidden",
            "401",
            "403",
            "api key",
            "apikey",
            "oauth",
            "token",
            "credential",
        ],
    ) {
        return FailureClass::Auth;
    }
    if contains_any(
        &lower,
        &[
            "rate limit",
            "rate_limit",
            "too many requests",
            "throttle",
            "throttled",
            "quota",
            "429",
        ],
    ) {
        return FailureClass::RateLimit;
    }
    if contains_any(&lower, &["timeout", "timed out", "deadline", "elapsed"]) {
        return FailureClass::Timeout;
    }
    if contains_any(
        &lower,
        &[
            "network",
            "dns",
            "connection",
            "connect",
            "refused",
            "reset",
            "broken pipe",
            "econn",
            "tls",
            "ssl",
            "http2",
            "transport",
            "socket",
        ],
    ) {
        return FailureClass::Network;
    }
    if contains_any(
        &lower,
        &[
            "task join",
            "panic",
            "panicked",
            "subprocess",
            "process",
            "spawn",
            "wait",
            "exited without",
            "no result",
            "stdout",
            "stderr",
            "protocol",
            "runtime",
            "worker",
            "killed",
        ],
    ) {
        return FailureClass::WorkerRuntime;
    }
    fallback
}

fn contains_any(haystack: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| haystack.contains(needle))
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
        }
        | StreamOutcome::Cancelled { accumulator: acc } => acc,
    }
}

async fn abort_terminal_stream(
    handle: &mut Box<dyn crate::engine::SessionHandle>,
    stream_result: &StreamOutcome,
    prompt_number: u32,
    session_id: Option<String>,
    total_cost: f64,
    total_turns: u32,
    start: Instant,
    resume_count: u32,
    model: Option<String>,
    blast_radius: Vec<String>,
) -> Result<Option<SessionOutcome>> {
    match stream_result {
        StreamOutcome::Timeout { elapsed, .. } => {
            tracing::warn!(
                prompt_number,
                elapsed_secs = elapsed.as_secs(),
                "session stalled; aborting subprocess"
            );
            handle.abort().await?;
            Ok(Some(build_outcome(
                prompt_number,
                SessionStatus::InfraFailure,
                session_id,
                total_cost,
                total_turns,
                start,
                resume_count,
                None,
                Some(format!("timeout: no events for {}s", elapsed.as_secs())),
                Some(FailureClass::Timeout),
                model,
                blast_radius,
                0,
                0,
                None,
            )))
        }
        StreamOutcome::Cancelled { .. } => {
            tracing::info!(prompt_number, "session cancelled; aborting subprocess");
            handle.abort().await?;
            Ok(Some(build_outcome(
                prompt_number,
                SessionStatus::Aborted,
                session_id,
                total_cost,
                total_turns,
                start,
                resume_count,
                None,
                Some("dispatch cancelled".to_owned()),
                None,
                model,
                blast_radius,
                0,
                0,
                None,
            )))
        }
        StreamOutcome::Complete(_) | StreamOutcome::Error { .. } => Ok(None),
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

fn output_excerpt_from_stream(outcome: &StreamOutcome) -> Option<String> {
    output_excerpt(&accumulator_from(outcome).text_fragments.join(""))
}

fn output_excerpt(text: &str) -> Option<String> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }

    Some(trimmed.chars().take(512).collect())
}

fn parse_structured_output(
    prompt_number: u32,
    output_format: Option<&hermeneus::types::OutputFormat>,
    result_text: Option<&str>,
) -> Option<serde_json::Value> {
    output_format?;
    let text = result_text?.trim();
    if text.is_empty() {
        return None;
    }

    match serde_json::from_str(text) {
        Ok(value) => Some(value),
        Err(error) => {
            tracing::warn!(
                prompt_number,
                error = %error,
                "session declared structured output but final result was not valid JSON"
            );
            None
        }
    }
}

fn emit_child_progress(
    options: &EngineConfig,
    prompt_number: u32,
    status: ChildSessionProgressStatus,
    child_session_id: Option<&str>,
    output_excerpt: Option<String>,
) {
    let Some(tx) = &options.child_progress_tx else {
        return;
    };
    let Some(child_session_id) = child_session_id else {
        return;
    };

    let _ = tx.send(ChildSessionProgress {
        prompt_number,
        status,
        child_session_id: child_session_id.to_owned(),
        output_excerpt,
    });
}

fn emit_child_terminal(
    options: &EngineConfig,
    outcome: &SessionOutcome,
    output_excerpt: Option<String>,
) {
    emit_child_progress(
        options,
        outcome.prompt_number,
        ChildSessionProgressStatus::Finished(outcome.status),
        outcome.session_id.as_deref(),
        output_excerpt,
    );
}

/// Build a [`SessionOutcome`] from the accumulated session state.
#[expect(clippy::too_many_arguments, reason = "session outcome has many fields")]
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
    failure_class: Option<FailureClass>,
    model: Option<String>,
    blast_radius: Vec<String>,
    cache_hit_tokens: u64,
    cache_miss_tokens: u64,
    structured_output: Option<serde_json::Value>,
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
        failure_class,
        model,
        blast_radius,
        corrective_attempts: 0,
        cache_hit_tokens,
        cache_miss_tokens,
        structured_output,
    }
}

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
            context_policy: crate::dag::ContextPolicy::Fresh,
            output_format: None,
            worktree: crate::prompt::WorktreePolicy::default(),
            acceptance_criteria: vec![],
            blast_radius: vec![],
            body: format!("implement task {number}"),
            prompt_components: None,
        }
    }

    fn json_output_format() -> hermeneus::types::OutputFormat {
        hermeneus::types::OutputFormat::JsonSchema {
            name: "result".to_owned(),
            schema: serde_json::json!({"type": "object"}),
            strict: Some(true),
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
                model: Some("claude-3-5-sonnet".to_owned()),
                cache_hit_tokens: 0,
                cache_miss_tokens: 0,
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
                model: Some("claude-3-5-sonnet".to_owned()),
                cache_hit_tokens: 0,
                cache_miss_tokens: 0,
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
                model: Some("claude-3-5-sonnet".to_owned()),
                cache_hit_tokens: 0,
                cache_miss_tokens: 0,
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

    // ── Success on first run ──

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

    #[tokio::test]
    async fn execute_emits_child_session_progress() {
        let engine = Arc::new(MockEngine::new(vec![success_outcome("sess-1", 0.50, 10)]));
        let budget = Arc::new(Budget::new(Some(10.0), Some(100), None));
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();

        let mgr = SessionManager::new(engine, budget, ResumePolicy::default());
        let config = default_config().child_progress_tx(tx);

        let outcome = mgr.execute(&sample_prompt_spec(7), &config).await.unwrap();
        assert_eq!(outcome.status, SessionStatus::Success);

        let started = rx.recv().await.unwrap();
        assert_eq!(started.prompt_number, 7);
        assert_eq!(started.status, ChildSessionProgressStatus::Started);
        assert_eq!(started.child_session_id, "sess-1");
        assert!(started.output_excerpt.is_none());

        let finished = rx.recv().await.unwrap();
        assert_eq!(finished.prompt_number, 7);
        assert_eq!(
            finished.status,
            ChildSessionProgressStatus::Finished(SessionStatus::Success)
        );
        assert_eq!(finished.child_session_id, "sess-1");
        assert_eq!(
            finished.output_excerpt.as_deref(),
            Some("working on it task complete")
        );
    }

    #[tokio::test]
    async fn execute_captures_structured_output_when_declared() {
        let engine = Arc::new(MockEngine::new(vec![MockOutcome::Success {
            events: vec![SessionEvent::TurnComplete { turn: 1 }],
            result: SessionResult {
                session_id: "sess-json".to_owned(),
                cost_usd: 0.10,
                num_turns: 1,
                duration_ms: 100,
                success: true,
                result_text: Some(r#"{"summary":"bounded"}"#.to_owned()),
                model: Some("test-model".to_owned()),
                cache_hit_tokens: 0,
                cache_miss_tokens: 0,
            },
        }]));
        let budget = Arc::new(Budget::new(Some(10.0), Some(100), None));
        let mgr = SessionManager::new(engine, budget, ResumePolicy::default());
        let mut prompt = sample_prompt_spec(9);
        prompt.output_format = Some(json_output_format());

        let outcome = mgr.execute(&prompt, &default_config()).await.unwrap();

        assert_eq!(
            outcome.structured_output,
            Some(serde_json::json!({"summary": "bounded"}))
        );
    }

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
        assert_eq!(outcome.failure_class, Some(FailureClass::Provider));
    }

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

    #[tokio::test]
    async fn execute_spawn_failure() {
        let engine = Arc::new(MockEngine::new(vec![MockOutcome::SpawnFailure {
            detail: "auth expired".to_owned(),
        }]));
        let budget = Arc::new(Budget::new(None, None, None));

        let mgr = SessionManager::new(engine, budget, ResumePolicy::default());

        let outcome = mgr
            .execute(&sample_prompt_spec(1), &default_config())
            .await
            .unwrap();

        assert_eq!(outcome.status, SessionStatus::InfraFailure);
        assert_eq!(outcome.failure_class, Some(FailureClass::Auth));
        assert!(outcome.error.as_deref().unwrap().contains("auth expired"));
    }

    #[tokio::test]
    async fn execute_spawn_network_failure() {
        let engine = Arc::new(MockEngine::new(vec![MockOutcome::SpawnFailure {
            detail: "connection refused while opening provider stream".to_owned(),
        }]));
        let budget = Arc::new(Budget::new(None, None, None));

        let mgr = SessionManager::new(engine, budget, ResumePolicy::default());

        let outcome = mgr
            .execute(&sample_prompt_spec(1), &default_config())
            .await
            .unwrap();

        assert_eq!(outcome.status, SessionStatus::InfraFailure);
        assert_eq!(outcome.failure_class, Some(FailureClass::Network));
    }

    #[tokio::test]
    async fn execute_resume_auth_failure() {
        let engine = Arc::new(MockEngine::new(vec![
            failure_outcome("sess-1", 0.20, 5),
            MockOutcome::SpawnFailure {
                detail: "401 unauthorized during resume".to_owned(),
            },
        ]));
        let budget = Arc::new(Budget::new(None, None, None));

        let mgr = SessionManager::new(engine, budget, two_stage_policy());

        let outcome = mgr
            .execute(&sample_prompt_spec(1), &default_config())
            .await
            .unwrap();

        assert_eq!(outcome.status, SessionStatus::InfraFailure);
        assert_eq!(outcome.failure_class, Some(FailureClass::Auth));
        assert_eq!(outcome.resume_count, 1);
    }

    #[tokio::test]
    async fn execute_wait_timeout_failure() {
        let engine = Arc::new(MockEngine::new(vec![MockOutcome::WaitFailure {
            session_id: "sess-timeout".to_owned(),
            events: vec![SessionEvent::TurnComplete { turn: 1 }],
            detail: "provider request timed out after 30s".to_owned(),
        }]));
        let budget = Arc::new(Budget::new(None, None, None));

        let mgr = SessionManager::new(engine, budget, two_stage_policy());

        let outcome = mgr
            .execute(&sample_prompt_spec(1), &default_config())
            .await
            .unwrap();

        assert_eq!(outcome.status, SessionStatus::InfraFailure);
        assert_eq!(outcome.failure_class, Some(FailureClass::Timeout));
        assert_eq!(outcome.num_turns, 1);
    }

    #[tokio::test]
    async fn execute_wait_rate_limit_failure() {
        let engine = Arc::new(MockEngine::new(vec![MockOutcome::WaitFailure {
            session_id: "sess-rate-limit".to_owned(),
            events: vec![],
            detail: "rate limit utilization exceeded 98%".to_owned(),
        }]));
        let budget = Arc::new(Budget::new(None, None, None));

        let mgr = SessionManager::new(engine, budget, two_stage_policy());

        let outcome = mgr
            .execute(&sample_prompt_spec(1), &default_config())
            .await
            .unwrap();

        assert_eq!(outcome.status, SessionStatus::InfraFailure);
        assert_eq!(outcome.failure_class, Some(FailureClass::RateLimit));
    }

    #[tokio::test]
    async fn execute_wait_worker_runtime_failure() {
        let engine = Arc::new(MockEngine::new(vec![MockOutcome::WaitFailure {
            session_id: "sess-runtime".to_owned(),
            events: vec![],
            detail: "subprocess exited without emitting a result message".to_owned(),
        }]));
        let budget = Arc::new(Budget::new(None, None, None));

        let mgr = SessionManager::new(engine, budget, two_stage_policy());

        let outcome = mgr
            .execute(&sample_prompt_spec(1), &default_config())
            .await
            .unwrap();

        assert_eq!(outcome.status, SessionStatus::InfraFailure);
        assert_eq!(outcome.failure_class, Some(FailureClass::WorkerRuntime));
    }

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
                    model: Some("claude-3-5-sonnet".to_owned()),
                    cache_hit_tokens: 0,
                    cache_miss_tokens: 0,
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
