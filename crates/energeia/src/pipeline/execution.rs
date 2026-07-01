// kanon:ignore RUST/file-too-long — execution stage is inherently sequential: group iteration, DAG updates, QA, and corrective generation cannot be decomposed without breaking the single-pass invariant
// WHY: Execution stage drives the frontier group loop: checks budget/cancel
// before each group, builds the group prompt list (skipping blocked prompts),
// drains correctives into the current group, then delegates to
// orchestrator::group::execute_group for concurrent session management.
// Separating this from preparation and post-processing keeps each file focused
// on a single phase and makes the full pipeline readable as a sequence of named
// stages.

use std::collections::HashMap;

use crate::dag::{ContextPolicy, PromptDag, PromptStatus};
use crate::pipeline::PipelineStage;
use crate::pipeline::context::PipelineContext;
use crate::pipeline::error::PipelineError;
use crate::prompt::PromptSpec;
use crate::types::{SessionOutcome, SessionStatus};

/// Execution stage: drives the frontier group loop and collects outcomes.
pub(crate) struct ExecutionStage;

impl PipelineStage for ExecutionStage {
    fn name(&self) -> &'static str {
        "execution"
    }

    #[expect(
        clippy::too_many_lines,
        reason = "execution lifecycle is inherently sequential: group iteration, DAG updates, QA, and corrective generation cannot be further decomposed without breaking the single-pass invariant"
    )]
    async fn run(&self, ctx: &mut PipelineContext) -> Result<(), PipelineError> {
        use crate::budget::BudgetStatus;

        let t0 = std::time::Instant::now();

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

            let mut group_prompts: Vec<PromptSpec> = Vec::new();
            for &n in group_numbers {
                let Some(prompt) = ctx.prompt_map.get(&n).cloned() else {
                    continue;
                };
                if has_failed_dependency(n, ctx.dag_mut()) {
                    // kanon:ignore RUST/no-silent-result-swallow — node was verified present via prompt_map lookup; set_status on existing node is infallible
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
                        failure_class: None,
                        model: None,
                        blast_radius: prompt.blast_radius.clone(),
                        corrective_attempts: 0,
                        cache_hit_tokens: 0,
                        cache_miss_tokens: 0,
                        structured_output: None,
                    });
                    mark_dependents_blocked(n, ctx.dag_mut());
                } else {
                    group_prompts.push(prompt);
                }
            }

            let correctives = std::mem::take(&mut ctx.correctives);
            group_prompts.extend(correctives);
            group_prompts = group_prompts
                .into_iter()
                .map(|prompt| {
                    attach_structured_upstream_outputs(prompt, ctx.dag(), &ctx.structured_outputs)
                })
                .collect();

            if group_prompts.is_empty() {
                continue;
            }

            for p in &group_prompts {
                // kanon:ignore RUST/no-silent-result-swallow — prompt was just added to the group from prompt_map; set_status is infallible here
                let _ = ctx.dag_mut().set_status(p.number, PromptStatus::InProgress);
            }

            tracing::info!(
                group = group_idx,
                prompts = ?group_prompts.iter().map(|p| p.number).collect::<Vec<_>>(),
                "executing group"
            );

            let engine_config = ctx.engine_config().clone();
            let mut outcomes = crate::orchestrator::group::execute_group(
                &group_prompts,
                std::sync::Arc::clone(&ctx.engine),
                std::sync::Arc::clone(ctx.budget()),
                &ctx.resume_policy,
                &engine_config,
                ctx.max_concurrent,
                &ctx.cancel,
            )
            .await;

            for outcome in &mut outcomes {
                outcome.corrective_attempts = ctx
                    .corrective_attempt_counts
                    .get(&outcome.prompt_number)
                    .copied()
                    .unwrap_or(0);
            }

            // NOTE: Post-processing handles metrics and store; this stage only
            // updates the DAG and the correctives list.
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
                        if let Some(structured_output) = &outcome.structured_output {
                            ctx.structured_outputs
                                .insert(outcome.prompt_number, structured_output.clone());
                        }

                        if let Some(pr_url) = &outcome.pr_url
                            && let Some(prompt) =
                                ctx.prompt_map.get(&outcome.prompt_number).cloned()
                        {
                            let budget = std::sync::Arc::clone(ctx.budget());
                            let correctives = &mut ctx.correctives;
                            let counts = &mut ctx.corrective_attempt_counts;
                            if let Some(verdict) = run_qa_and_generate_corrective(
                                &*ctx.qa,
                                ctx.diff_provider.as_deref(),
                                &prompt,
                                pr_url,
                                correctives,
                                ctx.config.max_corrective_retries,
                                counts,
                                &budget,
                            )
                            .await
                            {
                                ctx.qa_verdicts.push(verdict);
                            }
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
                failure_class: None,
                model: None,
                blast_radius: c.blast_radius.clone(),
                corrective_attempts: 0,
                cache_hit_tokens: 0,
                cache_miss_tokens: 0,
                structured_output: None,
            });
        }

        ctx.record_stage_latency(self.name(), t0.elapsed());
        Ok(())
    }
}

fn attach_structured_upstream_outputs(
    mut prompt: PromptSpec,
    dag: &PromptDag,
    structured_outputs: &HashMap<u32, serde_json::Value>,
) -> PromptSpec {
    let selected = selected_upstream_nodes(&prompt, dag);
    let mut entries: Vec<(u32, &serde_json::Value)> = selected
        .into_iter()
        .filter_map(|number| {
            structured_outputs
                .get(&number)
                .map(|output| (number, output))
        })
        .collect();
    entries.sort_by_key(|(number, _)| *number);

    if entries.is_empty() {
        return prompt;
    }

    let section = format_structured_outputs(&entries);
    prompt.body.push_str(&section);
    if let Some(components) = &mut prompt.prompt_components {
        components.dynamic_suffix.push_str(&section);
    }
    prompt
}

fn selected_upstream_nodes(prompt: &PromptSpec, dag: &PromptDag) -> Vec<u32> {
    match &prompt.context_policy {
        ContextPolicy::Fresh => prompt.depends_on.clone(),
        ContextPolicy::Inherit(nodes) => nodes.clone(),
        ContextPolicy::Shared => dag
            .nodes
            .values()
            .filter(|node| node.status == PromptStatus::Done)
            .map(|node| node.number)
            .collect(),
    }
}

fn format_structured_outputs(entries: &[(u32, &serde_json::Value)]) -> String {
    let mut section = String::from(
        "\n\n<upstream_structured_outputs>\nOnly structured outputs from selected upstream prompt nodes are included.\n",
    );
    for (number, output) in entries {
        section.push_str("<node number=\"");
        section.push_str(&number.to_string());
        section.push_str("\">\n");
        match serde_json::to_string_pretty(output) {
            Ok(json) => section.push_str(&json),
            Err(_) => section.push_str(&output.to_string()),
        }
        section.push_str("\n</node>\n");
    }
    section.push_str("</upstream_structured_outputs>\n");
    section
}

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
        // kanon:ignore RUST/no-silent-result-swallow — dependents are filtered from nodes already in the DAG; set_status is infallible
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
            // kanon:ignore RUST/no-silent-result-swallow — n comes from the numbers slice which originates from the DAG; set_status is infallible
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
                failure_class: None,
                model: None,
                blast_radius: vec![],
                corrective_attempts: 0,
                cache_hit_tokens: 0,
                cache_miss_tokens: 0,
                structured_output: None,
            }
        })
        .collect()
}

async fn run_qa_and_generate_corrective(
    qa: &dyn crate::qa::QaGate,
    diff_provider: Option<&dyn crate::qa::DiffProvider>,
    prompt: &crate::prompt::PromptSpec,
    pr_url: &str,
    correctives: &mut Vec<crate::prompt::PromptSpec>,
    max_corrective_retries: u32,
    corrective_attempt_counts: &mut std::collections::HashMap<u32, u32>,
    budget: &crate::types::Budget,
) -> Option<crate::types::QaVerdict> {
    let pr_number = pr_url
        .rsplit('/')
        .next()
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(0);

    let diff_result: std::result::Result<String, String> = if pr_number == 0 {
        Err("could not parse PR number from URL".to_owned())
    } else if let Some(provider) = diff_provider {
        match provider.fetch_diff(pr_url).await {
            Ok(diff) if !diff.trim().is_empty() => Ok(diff),
            Ok(_) => Err("empty diff returned by forge".to_owned()),
            Err(e) => Err(format!("{e}")),
        }
    } else {
        Err("no diff provider configured".to_owned())
    };

    let qa_prompt = crate::qa::PromptSpec {
        prompt_number: prompt.number,
        description: prompt.description.clone(),
        acceptance_criteria: prompt.acceptance_criteria.clone(),
        blast_radius: prompt.blast_radius.clone(),
    };

    let (qa_result, diff_fetch_failed) = match diff_result {
        Ok(ref diff) => match qa.evaluate(&qa_prompt, pr_number, diff).await {
            Ok(result) => (result, false),
            Err(e) => {
                tracing::warn!(
                    prompt_number = prompt.number,
                    error = %e,
                    "QA evaluation failed, skipping corrective generation"
                );
                return None;
            }
        },
        Err(ref reason) => {
            tracing::warn!(
                prompt_number = prompt.number,
                pr_url,
                reason,
                "diff fetch failed — returning degraded QA result"
            );
            let result = crate::types::QaResult {
                prompt_number: prompt.number,
                pr_number,
                verdict: crate::types::QaVerdict::Fail,
                criteria_results: vec![],
                mechanical_issues: vec![],
                reasons: vec![format!("diff fetch failed: {reason}")],
                cost_usd: 0.0,
                evaluated_at: jiff::Timestamp::now(),
                semantic_evaluated: false,
            };
            (result, true)
        }
    };

    tracing::info!(
        prompt_number = prompt.number,
        pr_number,
        verdict = %qa_result.verdict,
        semantic_evaluated = qa_result.semantic_evaluated,
        "QA evaluation complete"
    );

    let current_count = corrective_attempt_counts
        .get(&prompt.number)
        .copied()
        .unwrap_or(0);

    let budget_ok = !matches!(budget.check(), crate::budget::BudgetStatus::Exceeded(_));

    if qa_result.verdict != crate::types::QaVerdict::Pass
        && !diff_fetch_failed
        && current_count < max_corrective_retries
        && budget_ok
        && let Some(corrective) = crate::qa::corrective::generate_corrective(&qa_result, &qa_prompt)
    {
        tracing::info!(prompt_number = prompt.number, "generated corrective prompt");
        let reasons_text = qa_result.reasons.join(", ");
        let body = format!(
            "Your previous attempt had these issues: {reasons_text}. Fix them and push a new commit."
        );
        correctives.push(crate::prompt::PromptSpec {
            number: prompt.number,
            description: corrective.description,
            depends_on: vec![],
            context_policy: crate::dag::ContextPolicy::Fresh,
            output_format: None,
            worktree: crate::prompt::WorktreePolicy::default(),
            acceptance_criteria: corrective.acceptance_criteria,
            blast_radius: corrective.blast_radius,
            body,
            prompt_components: None,
        });
        corrective_attempt_counts.insert(prompt.number, current_count + 1);
    }

    Some(qa_result.verdict)
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
#[expect(clippy::unwrap_used, reason = "test assertions")]
#[expect(
    clippy::indexing_slicing,
    reason = "test assertions on known-length collections"
)]
#[expect(
    clippy::disallowed_types,
    reason = "std::sync::Mutex is sufficient for synchronous capture in test mocks"
)]
mod tests {
    use std::collections::VecDeque;
    use std::future::Future;
    use std::pin::Pin;
    use std::sync::Arc;

    use crate::engine::{
        AgentOptions, DispatchEngine, SessionEvent, SessionHandle, SessionResult, SessionSpec,
    };
    use crate::error::{self, Result};
    use crate::http::mock::{MockEngine, MockOutcome};
    use crate::orchestrator::OrchestratorConfig;
    use crate::pipeline::PipelineStage as _;
    use crate::pipeline::context::PipelineContext;
    use crate::pipeline::preparation::PreparationStage;
    use crate::prompt::PromptSpec;
    use crate::qa::DiffProvider;
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
                    reasons: vec![],
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

    struct PartialThenPassQa {
        call_count: std::sync::atomic::AtomicUsize,
    }

    impl PartialThenPassQa {
        fn new() -> Self {
            Self {
                call_count: std::sync::atomic::AtomicUsize::new(0),
            }
        }
    }

    impl QaGate for PartialThenPassQa {
        fn evaluate<'a>(
            &'a self,
            prompt: &'a crate::qa::PromptSpec,
            pr_number: u64,
            _diff: &'a str,
        ) -> std::pin::Pin<
            Box<dyn std::future::Future<Output = crate::error::Result<QaResult>> + Send + 'a>,
        > {
            use jiff::Timestamp;
            let count = self
                .call_count
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let verdict = if count == 0 {
                QaVerdict::Partial
            } else {
                QaVerdict::Pass
            };
            let reasons = if count == 0 {
                vec!["missing error handling".to_owned()]
            } else {
                vec![]
            };
            Box::pin(async move {
                Ok(QaResult {
                    prompt_number: prompt.prompt_number,
                    pr_number,
                    verdict,
                    criteria_results: vec![],
                    mechanical_issues: vec![],
                    reasons,
                    cost_usd: 0.0,
                    evaluated_at: Timestamp::now(),
                    semantic_evaluated: true,
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

    struct AlwaysFailQa;

    impl QaGate for AlwaysFailQa {
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
                    verdict: QaVerdict::Fail,
                    criteria_results: vec![],
                    mechanical_issues: vec![],
                    reasons: vec!["tests fail".to_owned()],
                    cost_usd: 0.0,
                    evaluated_at: Timestamp::now(),
                    semantic_evaluated: true,
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

    struct RecordingEngine {
        outcomes: tokio::sync::Mutex<VecDeque<MockOutcome>>,
        prompts: Arc<std::sync::Mutex<Vec<String>>>,
    }

    impl RecordingEngine {
        fn new(outcomes: Vec<MockOutcome>, prompts: Arc<std::sync::Mutex<Vec<String>>>) -> Self {
            Self {
                outcomes: tokio::sync::Mutex::new(VecDeque::from(outcomes)),
                prompts,
            }
        }
    }

    impl DispatchEngine for RecordingEngine {
        fn spawn_session<'a>(
            &'a self,
            spec: &'a SessionSpec,
            _options: &'a AgentOptions,
        ) -> Pin<Box<dyn Future<Output = Result<Box<dyn SessionHandle>>> + Send + 'a>> {
            Box::pin(async move {
                self.prompts
                    .lock()
                    .expect("recording prompts lock poisoned")
                    .push(spec.prompt.clone());
                let outcome = self.outcomes.lock().await.pop_front().ok_or_else(|| {
                    error::EngineSnafu {
                        detail: "RecordingEngine: no more configured outcomes".to_owned(),
                    }
                    .build()
                })?;
                match outcome {
                    MockOutcome::Success { events, result } => {
                        let handle =
                            RecordingHandle::new(result.session_id.clone(), events, result);
                        let boxed: Box<dyn SessionHandle> = Box::new(handle);
                        Ok(boxed)
                    }
                    MockOutcome::SpawnFailure { detail }
                    | MockOutcome::WaitFailure { detail, .. } => {
                        Err(error::EngineSnafu { detail }.build())
                    }
                }
            })
        }

        fn resume_session<'a>(
            &'a self,
            _session_id: &'a str,
            prompt: &'a str,
            _options: &'a AgentOptions,
        ) -> Pin<Box<dyn Future<Output = Result<Box<dyn SessionHandle>>> + Send + 'a>> {
            Box::pin(async move {
                self.prompts
                    .lock()
                    .expect("recording prompts lock poisoned")
                    .push(prompt.to_owned());
                let outcome = self.outcomes.lock().await.pop_front().ok_or_else(|| {
                    error::EngineSnafu {
                        detail: "RecordingEngine: no more configured outcomes".to_owned(),
                    }
                    .build()
                })?;
                match outcome {
                    MockOutcome::Success { events, result } => {
                        let handle =
                            RecordingHandle::new(result.session_id.clone(), events, result);
                        let boxed: Box<dyn SessionHandle> = Box::new(handle);
                        Ok(boxed)
                    }
                    MockOutcome::SpawnFailure { detail }
                    | MockOutcome::WaitFailure { detail, .. } => {
                        Err(error::EngineSnafu { detail }.build())
                    }
                }
            })
        }
    }

    struct RecordingHandle {
        session_id: String,
        events: VecDeque<SessionEvent>,
        result: Option<SessionResult>,
    }

    impl RecordingHandle {
        fn new(session_id: String, events: Vec<SessionEvent>, result: SessionResult) -> Self {
            Self {
                session_id,
                events: VecDeque::from(events),
                result: Some(result),
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
            Box::pin(async move { self.events.pop_front() })
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
            Box::pin(async move {
                self.events.clear();
                Ok(())
            })
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
                cache_hit_tokens: 0,
                cache_miss_tokens: 0,
            },
        }
    }

    fn success_mock_outcome_with_pr(session_id: &str, cost: f64, turns: u32) -> MockOutcome {
        MockOutcome::Success {
            events: vec![SessionEvent::TextDelta {
                text: "Created https://github.com/acme/repo/pull/42".to_owned(),
            }],
            result: SessionResult {
                session_id: session_id.to_owned(),
                cost_usd: cost,
                num_turns: turns,
                duration_ms: 100,
                success: true,
                result_text: Some("PR: https://github.com/acme/repo/pull/42".to_owned()),
                model: Some("claude-3-5-sonnet".to_owned()),
                cache_hit_tokens: 0,
                cache_miss_tokens: 0,
            },
        }
    }

    fn success_json_outcome(session_id: &str, result_text: &str) -> MockOutcome {
        MockOutcome::Success {
            events: vec![SessionEvent::TurnComplete { turn: 1 }],
            result: SessionResult {
                session_id: session_id.to_owned(),
                cost_usd: 0.01,
                num_turns: 1,
                duration_ms: 100,
                success: true,
                result_text: Some(result_text.to_owned()),
                model: Some("test-model".to_owned()),
                cache_hit_tokens: 0,
                cache_miss_tokens: 0,
            },
        }
    }

    fn success_json_outcome_with_secret_event(
        session_id: &str,
        result_text: &str,
        secret: &str,
    ) -> MockOutcome {
        MockOutcome::Success {
            events: vec![
                SessionEvent::TextDelta {
                    text: secret.to_owned(),
                },
                SessionEvent::TurnComplete { turn: 1 },
            ],
            result: SessionResult {
                session_id: session_id.to_owned(),
                cost_usd: 0.01,
                num_turns: 1,
                duration_ms: 100,
                success: true,
                result_text: Some(result_text.to_owned()),
                model: Some("test-model".to_owned()),
                cache_hit_tokens: 0,
                cache_miss_tokens: 0,
            },
        }
    }

    fn json_schema(name: &str) -> hermeneus::types::OutputFormat {
        hermeneus::types::OutputFormat::JsonSchema {
            name: name.to_owned(),
            schema: serde_json::json!({
                "type": "object",
                "additionalProperties": true
            }),
            strict: Some(true),
        }
    }

    fn structured_prompt(number: u32, depends_on: Vec<u32>, body: &str) -> PromptSpec {
        PromptSpec {
            number,
            description: format!("prompt {number}"),
            depends_on,
            context_policy: crate::dag::ContextPolicy::Fresh,
            output_format: Some(json_schema(&format!("prompt_{number}_output"))),
            worktree: crate::prompt::WorktreePolicy::default(),
            acceptance_criteria: vec![],
            blast_radius: vec![],
            body: body.to_owned(),
            prompt_components: None,
        }
    }

    struct MockDiffProvider {
        diff: String,
    }

    impl MockDiffProvider {
        fn new(diff: impl Into<String>) -> Self {
            Self { diff: diff.into() }
        }
    }

    impl DiffProvider for MockDiffProvider {
        fn fetch_diff<'a>(
            &'a self,
            _pr_url: &'a str,
        ) -> std::pin::Pin<
            Box<dyn std::future::Future<Output = crate::error::Result<String>> + Send + 'a>,
        > {
            let diff = self.diff.clone();
            Box::pin(async move { Ok(diff) })
        }
    }

    struct FailingDiffProvider;

    impl DiffProvider for FailingDiffProvider {
        fn fetch_diff<'a>(
            &'a self,
            _pr_url: &'a str,
        ) -> std::pin::Pin<
            Box<dyn std::future::Future<Output = crate::error::Result<String>> + Send + 'a>,
        > {
            Box::pin(async move {
                Err(crate::error::PreflightSnafu {
                    reason: "simulated diff fetch failure",
                }
                .build())
            })
        }
    }

    fn make_prepared_context(
        mock_outcomes: Vec<MockOutcome>,
        prompts: Vec<PromptSpec>,
    ) -> PipelineContext {
        make_prepared_context_with_config_and_qa(
            mock_outcomes,
            prompts,
            OrchestratorConfig::default(),
            Arc::new(AlwaysPassQa),
            Some(Arc::new(MockDiffProvider::new("fake diff"))),
        )
    }

    fn make_prepared_context_with_config_and_qa(
        mock_outcomes: Vec<MockOutcome>,
        prompts: Vec<PromptSpec>,
        config: OrchestratorConfig,
        qa: Arc<dyn QaGate>,
        diff_provider: Option<Arc<dyn DiffProvider>>,
    ) -> PipelineContext {
        let engine = Arc::new(MockEngine::new(mock_outcomes));
        let spec = DispatchSpec::new(
            "acme".to_owned(),
            prompts.iter().map(|p| p.number).collect(),
        );
        PipelineContext::new(
            spec,
            prompts,
            engine,
            qa,
            config,
            #[cfg(feature = "storage-fjall")]
            None,
        )
        .with_diff_provider(diff_provider)
    }

    #[tokio::test]
    async fn execution_runs_single_prompt() {
        let prompts = vec![PromptSpec {
            number: 1,
            description: "test".to_owned(),
            depends_on: vec![],
            context_policy: crate::dag::ContextPolicy::Fresh,
            output_format: None,
            worktree: crate::prompt::WorktreePolicy::default(),
            acceptance_criteria: vec![],
            blast_radius: vec![],
            body: "do the thing".to_owned(),

            prompt_components: None,
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
                context_policy: crate::dag::ContextPolicy::Fresh,
                output_format: None,
                worktree: crate::prompt::WorktreePolicy::default(),
                acceptance_criteria: vec![],
                blast_radius: vec![],
                body: "first task".to_owned(),

                prompt_components: None,
            },
            PromptSpec {
                number: 2,
                description: "second".to_owned(),
                depends_on: vec![1],
                context_policy: crate::dag::ContextPolicy::Fresh,
                output_format: None,
                worktree: crate::prompt::WorktreePolicy::default(),
                acceptance_criteria: vec![],
                blast_radius: vec![],
                body: "second task".to_owned(),

                prompt_components: None,
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
        assert!(
            ctx.outcomes
                .iter()
                .all(|o| o.status == SessionStatus::Success)
        );
    }

    #[tokio::test]
    async fn fresh_context_uses_only_structured_dependency_outputs() {
        let full_conversation = "IMPLEMENT_FULL_CONVERSATION_SHOULD_NOT_LEAK ".repeat(80);
        let captured_prompts = Arc::new(std::sync::Mutex::new(Vec::new()));
        let engine = Arc::new(RecordingEngine::new(
            vec![
                success_json_outcome("s1", r#"{"kind":"feature"}"#),
                success_json_outcome("s2", r#"{"research":"small research output"}"#),
                success_json_outcome("s3", r#"{"plan":"small plan output"}"#),
                success_json_outcome_with_secret_event(
                    "s4",
                    r#"{"implementation":"bounded summary"}"#,
                    &full_conversation,
                ),
                success_json_outcome("s5", r#"{"validated":true}"#),
            ],
            Arc::clone(&captured_prompts),
        ));
        let qa = Arc::new(AlwaysPassQa);
        let prompts = vec![
            structured_prompt(1, vec![], "classify"),
            structured_prompt(2, vec![1], "research"),
            structured_prompt(3, vec![2], "plan"),
            structured_prompt(4, vec![3], "implement"),
            PromptSpec {
                number: 5,
                description: "validate".to_owned(),
                depends_on: vec![4],
                context_policy: crate::dag::ContextPolicy::Fresh,
                output_format: None,
                worktree: crate::prompt::WorktreePolicy::default(),
                acceptance_criteria: vec![],
                blast_radius: vec![],
                body: "validate".to_owned(),
                prompt_components: None,
            },
        ];
        let spec = DispatchSpec::new("acme".to_owned(), vec![1, 2, 3, 4, 5]);
        let mut ctx = PipelineContext::new(
            spec,
            prompts,
            engine,
            qa,
            OrchestratorConfig::default().max_concurrent(1),
            #[cfg(feature = "storage-fjall")]
            None,
        )
        .with_diff_provider(Some(Arc::new(MockDiffProvider::new("fake diff"))));

        PreparationStage
            .run(&mut ctx)
            .await
            .expect("preparation must succeed");
        ExecutionStage
            .run(&mut ctx)
            .await
            .expect("execution must succeed");

        assert_eq!(ctx.outcomes.len(), 5);
        assert_eq!(ctx.structured_outputs.len(), 4);

        let prompts = captured_prompts
            .lock()
            .expect("captured prompts lock poisoned");
        assert_eq!(prompts.len(), 5);
        let validate_prompt = &prompts[4];
        assert!(validate_prompt.contains(r#""implementation": "bounded summary""#));
        assert!(!validate_prompt.contains("IMPLEMENT_FULL_CONVERSATION_SHOULD_NOT_LEAK"));
        assert!(
            validate_prompt.len() < full_conversation.len(),
            "validate prompt should be bounded by structured upstream output"
        );
    }

    #[tokio::test]
    async fn execution_qa_pass_no_corrective() {
        // Happy path: QA returns Pass, no corrective generated.
        let prompts = vec![PromptSpec {
            number: 1,
            description: "test".to_owned(),
            depends_on: vec![],
            context_policy: crate::dag::ContextPolicy::Fresh,
            output_format: None,
            worktree: crate::prompt::WorktreePolicy::default(),
            acceptance_criteria: vec![],
            blast_radius: vec![],
            body: "do the thing".to_owned(),

            prompt_components: None,
        }];
        let mut ctx =
            make_prepared_context(vec![success_mock_outcome_with_pr("s1", 0.10, 5)], prompts);
        ctx.config.max_corrective_retries = 1;

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
        assert_eq!(ctx.outcomes[0].corrective_attempts, 0);
        assert!(ctx.correctives.is_empty());
    }

    #[tokio::test]
    async fn execution_one_corrective_partial_then_pass() {
        // One corrective: first QA returns Partial, corrective runs and passes.
        // DAG: 1 -> 2. Group 0 runs prompt 1, group 1 runs prompt 2 + corrective.
        let prompts = vec![
            PromptSpec {
                number: 1,
                description: "first".to_owned(),
                depends_on: vec![],
                context_policy: crate::dag::ContextPolicy::Fresh,
                output_format: None,
                worktree: crate::prompt::WorktreePolicy::default(),
                acceptance_criteria: vec!["feature works".to_owned()],
                blast_radius: vec![],
                body: "do the thing".to_owned(),

                prompt_components: None,
            },
            PromptSpec {
                number: 2,
                description: "second".to_owned(),
                depends_on: vec![1],
                context_policy: crate::dag::ContextPolicy::Fresh,
                output_format: None,
                worktree: crate::prompt::WorktreePolicy::default(),
                acceptance_criteria: vec![],
                blast_radius: vec![],
                body: "second task".to_owned(),

                prompt_components: None,
            },
        ];
        let qa = Arc::new(PartialThenPassQa::new());
        let config = OrchestratorConfig::default()
            .max_corrective_retries(1)
            .max_concurrent(1);
        let mut ctx = make_prepared_context_with_config_and_qa(
            vec![
                success_mock_outcome_with_pr("s1", 0.10, 5), // group 0: prompt 1
                success_mock_outcome_with_pr("s2", 0.10, 5), // group 1: prompt 2
                success_mock_outcome_with_pr("s1-c1", 0.10, 5), // group 1: corrective 1
            ],
            prompts,
            config,
            qa,
            Some(Arc::new(MockDiffProvider::new("fake diff"))),
        );

        PreparationStage
            .run(&mut ctx)
            .await
            .expect("preparation must succeed");
        ExecutionStage
            .run(&mut ctx)
            .await
            .expect("execution must succeed");

        // Original 1 + corrective 1 + original 2 = 3 outcomes.
        assert_eq!(
            ctx.outcomes.len(),
            3,
            "expected 3 outcomes: {:?}",
            ctx.outcomes
        );

        let original = ctx
            .outcomes
            .iter()
            .find(|o| o.session_id.as_deref() == Some("s1"))
            .expect("original outcome should exist");
        assert_eq!(original.status, SessionStatus::Success);
        assert_eq!(original.corrective_attempts, 0);

        let corrective = ctx
            .outcomes
            .iter()
            .find(|o| o.corrective_attempts == 1)
            .expect("corrective outcome should exist");
        assert_eq!(corrective.status, SessionStatus::Success);
        assert_eq!(corrective.prompt_number, 1);
    }

    #[tokio::test]
    async fn execution_corrective_exhausted_by_budget() {
        // QA returns Fail but budget is exhausted after original, so no corrective.
        let prompts = vec![PromptSpec {
            number: 1,
            description: "test".to_owned(),
            depends_on: vec![],
            context_policy: crate::dag::ContextPolicy::Fresh,
            output_format: None,
            worktree: crate::prompt::WorktreePolicy::default(),
            acceptance_criteria: vec!["feature works".to_owned()],
            blast_radius: vec![],
            body: "do the thing".to_owned(),

            prompt_components: None,
        }];
        let qa = Arc::new(AlwaysFailQa);
        let config = OrchestratorConfig::default()
            .max_corrective_retries(1)
            .default_budget_usd(0.05);
        let mut ctx = make_prepared_context_with_config_and_qa(
            vec![success_mock_outcome_with_pr("s1", 0.10, 5)],
            prompts,
            config,
            qa,
            Some(Arc::new(MockDiffProvider::new("fake diff"))),
        );

        PreparationStage
            .run(&mut ctx)
            .await
            .expect("preparation must succeed");
        ExecutionStage
            .run(&mut ctx)
            .await
            .expect("execution must succeed");

        // Original succeeds, budget exhausted, corrective not generated.
        assert_eq!(ctx.outcomes.len(), 1);
        assert_eq!(ctx.outcomes[0].status, SessionStatus::Success);
        assert_eq!(ctx.outcomes[0].corrective_attempts, 0);
        assert!(ctx.correctives.is_empty());
    }

    #[test]
    fn mark_dependents_blocked_cascades() {
        use crate::dag::{PromptDag, PromptStatus};
        let mut dag = PromptDag::new();
        dag.add_node(1, vec![]).unwrap();
        dag.add_node(2, vec![1]).unwrap();
        dag.add_node(3, vec![1]).unwrap();
        dag.add_node(4, vec![2]).unwrap();

        dag.set_status(1, PromptStatus::Failed).unwrap();
        dag.set_status(2, PromptStatus::Blocked).unwrap();
        dag.set_status(3, PromptStatus::Ready).unwrap();

        super::mark_dependents_blocked(1, &mut dag);

        assert_eq!(dag.nodes[&2].status, PromptStatus::Blocked);
        assert_eq!(dag.nodes[&3].status, PromptStatus::Blocked);
        // NOTE: 4 depends on 2, not directly on 1. It is not marked blocked
        // by this call. The orchestrator marks it in a subsequent pass.
    }

    #[tokio::test]
    async fn qa_receives_non_empty_diff() {
        struct RecordingQaGate {
            last_diff: std::sync::Mutex<String>,
        }

        impl QaGate for RecordingQaGate {
            fn evaluate<'a>(
                &'a self,
                prompt: &'a crate::qa::PromptSpec,
                pr_number: u64,
                diff: &'a str,
            ) -> std::pin::Pin<
                Box<dyn std::future::Future<Output = crate::error::Result<QaResult>> + Send + 'a>,
            > {
                let mut last = self.last_diff.lock().unwrap();
                *last = diff.to_owned();
                Box::pin(async move {
                    Ok(QaResult {
                        prompt_number: prompt.prompt_number,
                        pr_number,
                        verdict: QaVerdict::Pass,
                        criteria_results: vec![],
                        mechanical_issues: vec![],
                        reasons: vec![],
                        cost_usd: 0.0,
                        evaluated_at: jiff::Timestamp::now(),
                        semantic_evaluated: true,
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

        let qa = &RecordingQaGate {
            last_diff: std::sync::Mutex::new(String::new()),
        };
        let provider = &MockDiffProvider::new("+real diff content");
        let prompt = crate::prompt::PromptSpec {
            number: 1,
            description: "test".to_owned(),
            depends_on: vec![],
            context_policy: crate::dag::ContextPolicy::Fresh,
            output_format: None,
            worktree: crate::prompt::WorktreePolicy::default(),
            acceptance_criteria: vec![],
            blast_radius: vec![],
            body: "do the thing".to_owned(),
            prompt_components: None,
        };
        let mut correctives = vec![];
        let mut counts = std::collections::HashMap::new();
        let budget = crate::budget::Budget::new(Some(10.0), None, None);

        let verdict = super::run_qa_and_generate_corrective(
            qa,
            Some(provider),
            &prompt,
            "https://github.com/acme/repo/pull/42",
            &mut correctives,
            1,
            &mut counts,
            &budget,
        )
        .await;

        assert_eq!(verdict, Some(QaVerdict::Pass));
        assert_eq!(qa.last_diff.lock().unwrap().as_str(), "+real diff content");
        assert!(correctives.is_empty());
    }

    #[tokio::test]
    async fn qa_skips_semantic_eval_on_diff_fetch_failure() {
        struct PanicQaGate;

        impl QaGate for PanicQaGate {
            fn evaluate<'a>(
                &'a self,
                _prompt: &'a crate::qa::PromptSpec,
                _pr_number: u64,
                _diff: &'a str,
            ) -> std::pin::Pin<
                Box<dyn std::future::Future<Output = crate::error::Result<QaResult>> + Send + 'a>,
            > {
                panic!("evaluate should not be called when diff fetch fails")
            }

            fn mechanical_check(
                &self,
                _diff: &str,
                _prompt: &crate::qa::PromptSpec,
            ) -> Vec<MechanicalIssue> {
                vec![]
            }
        }

        let qa = &PanicQaGate;
        let provider = &FailingDiffProvider;
        let prompt = crate::prompt::PromptSpec {
            number: 1,
            description: "test".to_owned(),
            depends_on: vec![],
            context_policy: crate::dag::ContextPolicy::Fresh,
            output_format: None,
            worktree: crate::prompt::WorktreePolicy::default(),
            acceptance_criteria: vec![],
            blast_radius: vec![],
            body: "do the thing".to_owned(),
            prompt_components: None,
        };
        let mut correctives = vec![];
        let mut counts = std::collections::HashMap::new();
        let budget = crate::budget::Budget::new(Some(10.0), None, None);

        let verdict = super::run_qa_and_generate_corrective(
            qa,
            Some(provider),
            &prompt,
            "https://github.com/acme/repo/pull/42",
            &mut correctives,
            1,
            &mut counts,
            &budget,
        )
        .await;

        assert_eq!(verdict, Some(QaVerdict::Fail));
        assert!(correctives.is_empty());
    }
}
