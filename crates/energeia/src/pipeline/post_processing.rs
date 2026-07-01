// WHY: Post-processing stage records Prometheus metrics for every outcome,
// assembles the final DispatchResult, finishes the store dispatch record, emits
// the completion trace event, and appends an after-action JSONL record.
// Separating this from execution means the metric/store-update/telemetry code
// has a single home and execution can stay focused on session management.

use std::time::Instant;

use jiff::Timestamp;

use crate::pipeline::PipelineStage;
use crate::pipeline::after_action::append_after_action_record;
use crate::pipeline::context::PipelineContext;
use crate::pipeline::error::PipelineError;
use crate::types::DispatchResult;

/// Post-processing stage: record metrics, assemble result, finish store record,
/// append after-action JSONL.
pub(crate) struct PostProcessingStage;

impl PipelineStage for PostProcessingStage {
    fn name(&self) -> &'static str {
        "post_processing"
    }

    async fn run(&self, ctx: &mut PipelineContext) -> Result<(), PipelineError> {
        let t0 = Instant::now();

        record_outcome_metrics(ctx);

        let total_cost = ctx.outcomes.iter().map(|o| o.cost_usd).sum();
        let duration_ms = u64::try_from(ctx.start.elapsed().as_millis()).unwrap_or(u64::MAX);

        let result = DispatchResult {
            dispatch_id: ctx.dispatch_id.clone(),
            outcomes: ctx.outcomes.clone(),
            total_cost_usd: total_cost,
            duration_ms,
            aborted: ctx.aborted,
            completed_at: Timestamp::now(),
        };

        #[cfg(feature = "storage-fjall")]
        if let (Some(store), Some(store_id)) = (&ctx.store, &ctx.store_dispatch_id) {
            for verdict in &ctx.qa_verdicts {
                if let Err(e) = store.add_qa_verdict(store_id, &ctx.spec.project, *verdict) {
                    tracing::warn!(error = %e, verdict = %verdict, "failed to persist QA verdict");
                }
            }

            // WHY: Create and immediately update one SessionRecord per outcome so
            // status dashboards and training exports reflect real per-session data
            // rather than showing zero sessions for a completed dispatch.
            for outcome in &ctx.outcomes {
                match store.create_session(store_id, outcome.prompt_number) {
                    Ok(session_store_id) => {
                        let update = crate::store::records::SessionUpdate {
                            status: Some(outcome.status),
                            session_id: outcome.session_id.clone(),
                            cost_usd: Some(outcome.cost_usd),
                            num_turns: Some(outcome.num_turns),
                            duration_ms: Some(outcome.duration_ms),
                            pr_url: outcome.pr_url.clone(),
                            error: outcome.error.clone(),
                        };
                        if let Err(e) = store.update_session(&session_store_id, update) {
                            tracing::warn!(
                                error = %e,
                                prompt_number = outcome.prompt_number,
                                "failed to update session record"
                            );
                        }
                    }
                    Err(e) => tracing::warn!(
                        error = %e,
                        prompt_number = outcome.prompt_number,
                        "failed to create session record"
                    ),
                }
            }

            let status = if ctx.aborted {
                crate::store::records::DispatchStatus::Failed
            } else {
                crate::store::records::DispatchStatus::Completed
            };
            if let Err(e) = store.finish_dispatch(store_id, status) {
                tracing::warn!(error = %e, "failed to finish dispatch record");
            }
        }

        // WHY: QA verdict and dispatch metrics are recorded unconditionally so
        // counters move for every dispatch, even when no persistent store is attached.
        for verdict in &ctx.qa_verdicts {
            crate::metrics::prometheus::record_qa_verdict(&ctx.spec.project, &verdict.to_string());
        }
        let dispatch_status = if ctx.aborted { "failed" } else { "completed" };
        crate::metrics::prometheus::record_dispatch(&ctx.spec.project, dispatch_status);

        tracing::info!(
            dispatch_id = %ctx.dispatch_id,
            total_cost = result.total_cost_usd,
            duration_ms = result.duration_ms,
            outcomes = result.outcomes.len(),
            aborted = ctx.aborted,
            "dispatch complete"
        );

        ctx.result = Some(result);

        // WHY: latency recorded before the append so the after-action record includes this stage.

        ctx.record_stage_latency(self.name(), t0.elapsed());

        append_after_action_record(ctx).await?;

        Ok(())
    }
}

fn record_outcome_metrics(ctx: &PipelineContext) {
    for outcome in &ctx.outcomes {
        let model = outcome.model.as_deref().unwrap_or("unknown");
        let blast_radius = outcome
            .blast_radius
            .first()
            .map_or("unknown", String::as_str);

        crate::metrics::prometheus::record_session(
            &ctx.spec.project,
            &outcome.status.to_string(),
            outcome.cost_usd,
            outcome.duration_ms,
            model,
            blast_radius,
        );
        crate::metrics::prometheus::record_turns(
            &ctx.spec.project,
            outcome.num_turns,
            model,
            blast_radius,
        );
        if let Some(failure_class) = outcome.failure_class {
            crate::metrics::prometheus::record_session_failure(
                &ctx.spec.project,
                &outcome.status.to_string(),
                model,
                &failure_class.to_string(),
            );
        }
    }
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
#[expect(
    clippy::indexing_slicing,
    reason = "test assertions on known-length collections"
)]
mod tests {
    use std::sync::Arc;

    use jiff::Timestamp;

    use crate::engine::{SessionEvent, SessionResult};
    use crate::http::mock::{MockEngine, MockOutcome};
    use crate::orchestrator::OrchestratorConfig;
    use crate::pipeline::PipelineStage as _;
    use crate::pipeline::context::PipelineContext;
    use crate::pipeline::execution::ExecutionStage;
    use crate::pipeline::preparation::PreparationStage;
    use crate::prompt::PromptSpec;
    use crate::qa::QaGate;
    use crate::types::{DispatchSpec, MechanicalIssue, QaResult, QaVerdict, SessionStatus};

    use super::PostProcessingStage;

    async fn read_dir_sorted(path: &std::path::Path) -> Vec<tokio::fs::DirEntry> {
        let mut rd = tokio::fs::read_dir(path).await.expect("read dir");
        let mut entries = Vec::new();
        while let Some(e) = rd.next_entry().await.expect("next entry") {
            entries.push(e);
        }
        entries.sort_by_key(tokio::fs::DirEntry::file_name);
        entries
    }

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

    fn make_context_with_prompts(
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
    async fn post_processing_sets_result() {
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
            make_context_with_prompts(vec![success_mock_outcome("s1", 0.50, 10)], prompts);

        PreparationStage
            .run(&mut ctx)
            .await
            .expect("preparation must succeed");
        ExecutionStage
            .run(&mut ctx)
            .await
            .expect("execution must succeed");
        PostProcessingStage
            .run(&mut ctx)
            .await
            .expect("post_processing must succeed");

        let result = ctx.result.expect("result should be set");
        assert!(!result.aborted);
        assert_eq!(result.outcomes.len(), 1);
        assert_eq!(result.outcomes[0].status, SessionStatus::Success);
        assert!((result.total_cost_usd - 0.50).abs() < 0.01);
        assert!(!result.dispatch_id.is_empty());
    }

    // ── After-action JSONL tests ──

    #[tokio::test]
    async fn happy_path_writes_valid_jsonl_line() {
        let tmp = tempfile::tempdir().expect("create temp dir");
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
            make_context_with_prompts(vec![success_mock_outcome("s1", 0.50, 10)], prompts);
        ctx.after_action_log_dir = Some(tmp.path().to_path_buf());

        PreparationStage
            .run(&mut ctx)
            .await
            .expect("preparation must succeed");
        ExecutionStage
            .run(&mut ctx)
            .await
            .expect("execution must succeed");
        PostProcessingStage
            .run(&mut ctx)
            .await
            .expect("post_processing must succeed");

        let entries = read_dir_sorted(tmp.path()).await;
        assert_eq!(entries.len(), 1, "exactly one JSONL file should exist");

        let path = entries[0].path();
        let content = tokio::fs::read_to_string(&path).await.expect("read file");
        let lines: Vec<_> = content.lines().collect();
        assert_eq!(lines.len(), 1, "exactly one line should be written");

        let record: serde_json::Value = serde_json::from_str(lines[0]).expect("valid JSON");
        assert!(
            record.get("dispatch_id").is_some(),
            "dispatch_id should be present"
        );
        assert!(
            record.get("ts_start").is_some(),
            "ts_start should be present"
        );
        assert!(record.get("ts_end").is_some(), "ts_end should be present");
        assert!(
            record.get("duration_ms").is_some(),
            "duration_ms should be present"
        );
        assert!(
            record.get("session_outcomes").is_some(),
            "session_outcomes should be present"
        );
        assert!(
            record.get("cost_total_cents").is_some(),
            "cost_total_cents should be present"
        );
        assert!(
            record.get("turns_total").is_some(),
            "turns_total should be present"
        );
        assert!(
            record.get("stage_latencies_ms").is_some(),
            "stage_latencies_ms should be present"
        );
        assert!(
            record.get("qa_verdict").is_some(),
            "qa_verdict should be present"
        );
        assert!(
            record.get("prompt_hash").is_some(),
            "prompt_hash should be present"
        );

        let hash = record["prompt_hash"]
            .as_str()
            .expect("prompt_hash is string");
        assert!(
            hash.starts_with("sha256:"),
            "prompt_hash should have sha256: prefix"
        );

        let outcomes = record["session_outcomes"]
            .as_array()
            .expect("session_outcomes is array");
        assert_eq!(outcomes.len(), 1);
        assert_eq!(outcomes[0]["status"], "success");
        assert_eq!(outcomes[0]["turns"], 10);
        assert_eq!(outcomes[0]["model"], "claude-3-5-sonnet");
        assert_eq!(outcomes[0]["category"], "feature");
        assert!(outcomes[0]["failure_class"].is_null());
    }

    #[tokio::test]
    async fn infra_failure_writes_failure_class_without_routing_model_bucket() {
        let tmp = tempfile::tempdir().expect("create temp dir");
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
        let mut ctx = make_context_with_prompts(
            vec![MockOutcome::SpawnFailure {
                detail: "auth token expired".to_owned(),
            }],
            prompts,
        );
        ctx.after_action_log_dir = Some(tmp.path().to_path_buf());

        PreparationStage
            .run(&mut ctx)
            .await
            .expect("preparation must succeed");
        ExecutionStage
            .run(&mut ctx)
            .await
            .expect("execution must succeed");
        PostProcessingStage
            .run(&mut ctx)
            .await
            .expect("post_processing must succeed");

        let entries = read_dir_sorted(tmp.path()).await;
        let content = tokio::fs::read_to_string(entries[0].path())
            .await
            .expect("read file");
        let record: serde_json::Value =
            serde_json::from_str(content.lines().next().expect("one jsonl line"))
                .expect("valid JSON");
        let outcomes = record["session_outcomes"]
            .as_array()
            .expect("session_outcomes is array");

        assert_eq!(outcomes[0]["status"], "infra_failure");
        assert_eq!(outcomes[0]["failure_class"], "auth");
        assert!(outcomes[0]["model"].is_null());
    }

    #[tokio::test]
    async fn multi_dispatch_appends() {
        let tmp = tempfile::tempdir().expect("create temp dir");
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

        for _ in 0..2 {
            let mut ctx = make_context_with_prompts(
                vec![success_mock_outcome("s1", 0.50, 10)],
                prompts.clone(),
            );
            ctx.after_action_log_dir = Some(tmp.path().to_path_buf());

            PreparationStage
                .run(&mut ctx)
                .await
                .expect("preparation must succeed");
            ExecutionStage
                .run(&mut ctx)
                .await
                .expect("execution must succeed");
            PostProcessingStage
                .run(&mut ctx)
                .await
                .expect("post_processing must succeed");
        }

        let entries = read_dir_sorted(tmp.path()).await;
        assert_eq!(entries.len(), 1, "same-day dispatches share one file");

        let path = entries[0].path();
        let content = tokio::fs::read_to_string(&path).await.expect("read file");
        let lines: Vec<_> = content.lines().collect();
        assert_eq!(lines.len(), 2, "two dispatches should append as two lines");
    }

    #[tokio::test]
    async fn date_rollover_creates_new_file() {
        let tmp = tempfile::tempdir().expect("create temp dir");
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

        // Seed a file from yesterday to simulate a prior day's log. It is
        // inside the default 7-day window and should survive pruning.
        let yesterday = Timestamp::now()
            .checked_sub(jiff::SignedDuration::from_hours(24))
            .expect("yesterday is a valid timestamp")
            .strftime("%Y-%m-%d")
            .to_string();
        let old_file = tmp.path().join(format!("{yesterday}.jsonl"));
        tokio::fs::write(&old_file, "{\"old\":true}\n")
            .await
            .expect("write old file");

        let mut ctx =
            make_context_with_prompts(vec![success_mock_outcome("s1", 0.50, 10)], prompts);
        ctx.after_action_log_dir = Some(tmp.path().to_path_buf());

        PreparationStage
            .run(&mut ctx)
            .await
            .expect("preparation must succeed");
        ExecutionStage
            .run(&mut ctx)
            .await
            .expect("execution must succeed");
        PostProcessingStage
            .run(&mut ctx)
            .await
            .expect("post_processing must succeed");

        let entries = read_dir_sorted(tmp.path()).await;
        assert_eq!(entries.len(), 2, "old file and new file should both exist");

        let mut found_old = false;
        let mut found_new = false;
        for entry in &entries {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name
                == old_file
                    .file_name()
                    .expect("old_file has a file name")
                    .to_string_lossy()
            {
                found_old = true;
            } else if name.ends_with(".jsonl") {
                found_new = true;
            }
        }
        assert!(found_old, "old file should be preserved");
        assert!(found_new, "new file for current date should be created");
    }

    #[tokio::test]
    async fn prunes_files_outside_routing_window() {
        // WHY: Issue 5669. Day-files older than the configured window must be
        // deleted so the after-actions directory does not grow without bound.
        let tmp = tempfile::tempdir().expect("create temp dir");
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

        let old_date = Timestamp::now()
            .checked_sub(jiff::SignedDuration::from_hours(10 * 24))
            .expect("ten days ago is a valid timestamp")
            .strftime("%Y-%m-%d")
            .to_string();
        let recent_date = Timestamp::now()
            .checked_sub(jiff::SignedDuration::from_hours(2 * 24))
            .expect("two days ago is a valid timestamp")
            .strftime("%Y-%m-%d")
            .to_string();

        let old_file = tmp.path().join(format!("{old_date}.jsonl"));
        let recent_file = tmp.path().join(format!("{recent_date}.jsonl"));
        tokio::fs::write(&old_file, "{\"old\":true}\n")
            .await
            .expect("write old file");
        tokio::fs::write(&recent_file, "{\"recent\":true}\n")
            .await
            .expect("write recent file");

        let mut ctx =
            make_context_with_prompts(vec![success_mock_outcome("s1", 0.50, 10)], prompts);
        ctx.after_action_log_dir = Some(tmp.path().to_path_buf());

        PreparationStage
            .run(&mut ctx)
            .await
            .expect("preparation must succeed");
        ExecutionStage
            .run(&mut ctx)
            .await
            .expect("execution must succeed");
        PostProcessingStage
            .run(&mut ctx)
            .await
            .expect("post_processing must succeed");

        let entries = read_dir_sorted(tmp.path()).await;
        assert!(
            !entries
                .iter()
                .any(|e| { e.file_name().to_string_lossy().starts_with(&old_date) }),
            "old file outside the 7-day window should be pruned"
        );
        assert!(
            entries
                .iter()
                .any(|e| { e.file_name().to_string_lossy().starts_with(&recent_date) }),
            "recent file inside the window should be preserved"
        );
    }

    #[tokio::test]
    async fn missing_directory_is_created() {
        let tmp = tempfile::tempdir().expect("create temp dir");
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

        // Use a nested path that does not yet exist.
        let log_dir = tmp.path().join("deep").join("nested").join("logs");

        let mut ctx =
            make_context_with_prompts(vec![success_mock_outcome("s1", 0.50, 10)], prompts);
        ctx.after_action_log_dir = Some(log_dir.clone());

        PreparationStage
            .run(&mut ctx)
            .await
            .expect("preparation must succeed");
        ExecutionStage
            .run(&mut ctx)
            .await
            .expect("execution must succeed");
        PostProcessingStage
            .run(&mut ctx)
            .await
            .expect("post_processing must succeed");

        assert!(
            log_dir.exists(),
            "missing after-action log directory should be created"
        );
    }
}
