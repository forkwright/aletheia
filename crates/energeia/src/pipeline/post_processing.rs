// WHY: Post-processing stage records Prometheus metrics for every outcome,
// assembles the final DispatchResult, finishes the store dispatch record, emits
// the completion trace event, and appends an after-action JSONL record.
// Separating this from execution means the metric/store-update/telemetry code
// has a single home and execution can stay focused on session management.

use std::collections::HashMap;
use std::time::Instant;

use aletheia_routing::types::TaskCategory;
use jiff::Timestamp;
use serde::Serialize;
use sha2::{Digest, Sha256};
use snafu::{IntoError as _, ResultExt as _};
use tokio::io::AsyncWriteExt;

use crate::pipeline::PipelineStage;
use crate::pipeline::context::PipelineContext;
use crate::pipeline::error::{PipelineError, StageSnafu};
use crate::types::{DispatchResult, QaVerdict};

/// One line of after-action telemetry per dispatch.
#[derive(Debug, Serialize)]
struct AfterActionRecord {
    dispatch_id: String,
    ts_start: String,
    ts_end: String,
    duration_ms: u64,
    session_outcomes: Vec<AfterActionSessionOutcome>,
    cost_total_cents: u64,
    turns_total: u32,
    stage_latencies_ms: HashMap<String, u64>,
    qa_verdict: String,
    prompt_hash: String,
}

/// Per-session subset emitted in the after-action record.
#[derive(Debug, Serialize)]
struct AfterActionSessionOutcome {
    session_id: Option<String>,
    status: String,
    turns: u32,
    cost_cents: u64,
    pr_url: Option<String>,
    model: Option<String>,
    category: Option<String>,
}

/// Post-processing stage: record metrics, assemble result, finish store record,
/// append after-action JSONL.
pub(crate) struct PostProcessingStage;

impl PipelineStage for PostProcessingStage {
    fn name(&self) -> &'static str {
        "post_processing"
    }

    async fn run(&self, ctx: &mut PipelineContext) -> Result<(), PipelineError> {
        let t0 = Instant::now();

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
        }

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

/// Build and append the after-action JSONL record.
///
/// No-op when `ctx.after_action_log_dir` is `None`.
async fn append_after_action_record(ctx: &PipelineContext) -> Result<(), PipelineError> {
    let Some(ref log_dir) = ctx.after_action_log_dir else {
        return Ok(());
    };

    let record = build_after_action_record(ctx)?;
    let line = serde_json::to_string(&record)
        .map_err(|e| {
            crate::error::SerializationSnafu {
                message: e.to_string(),
            }
            .build()
        })
        .context(StageSnafu {
            stage: "post_processing",
        })?;

    tokio::fs::create_dir_all(log_dir)
        .await
        .map_err(|e| {
            crate::error::IoSnafu {
                path: log_dir.clone(),
            }
            .into_error(e)
        })
        .context(StageSnafu {
            stage: "post_processing",
        })?;

    let date = Timestamp::now().strftime("%Y-%m-%d").to_string();
    let path = log_dir.join(format!("{date}.jsonl"));

    let mut file = tokio::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .await
        .map_err(|e| crate::error::IoSnafu { path: path.clone() }.into_error(e))
        .context(StageSnafu {
            stage: "post_processing",
        })?;

    file.write_all(line.as_bytes())
        .await
        .map_err(|e| crate::error::IoSnafu { path: path.clone() }.into_error(e))
        .context(StageSnafu {
            stage: "post_processing",
        })?;

    file.write_all(b"\n")
        .await
        .map_err(|e| crate::error::IoSnafu { path: path.clone() }.into_error(e))
        .context(StageSnafu {
            stage: "post_processing",
        })?;

    Ok(())
}

/// Build the [`AfterActionRecord`] from the current pipeline context.
fn build_after_action_record(ctx: &PipelineContext) -> Result<AfterActionRecord, PipelineError> {
    let session_outcomes = ctx
        .outcomes
        .iter()
        .map(|o| AfterActionSessionOutcome {
            session_id: o.session_id.clone(),
            status: o.status.to_string(),
            turns: o.num_turns,
            cost_cents: usd_to_cents(o.cost_usd),
            pr_url: o.pr_url.clone(),
            model: o.model.clone(),
            category: ctx
                .prompt_map
                .get(&o.prompt_number)
                .map(|prompt| TaskCategory::from_prompt(&prompt.body).to_string()),
        })
        .collect();

    let cost_total_cents = ctx.outcomes.iter().map(|o| usd_to_cents(o.cost_usd)).sum();
    let turns_total = ctx.outcomes.iter().map(|o| o.num_turns).sum();

    let stage_latencies_ms = ctx
        .stage_latencies
        .iter()
        .map(|(k, v)| {
            (
                k.to_string(),
                u64::try_from(v.as_millis()).unwrap_or(u64::MAX),
            )
        })
        .collect();

    let prompt_hash = compute_prompt_hash(&ctx.prompts).context(StageSnafu {
        stage: "post_processing",
    })?;

    Ok(AfterActionRecord {
        dispatch_id: ctx.dispatch_id.clone(),
        ts_start: ctx.start_ts.strftime("%Y-%m-%dT%H:%M:%SZ").to_string(),
        ts_end: Timestamp::now().strftime("%Y-%m-%dT%H:%M:%SZ").to_string(),
        duration_ms: u64::try_from(ctx.start.elapsed().as_millis()).unwrap_or(u64::MAX),
        session_outcomes,
        cost_total_cents,
        turns_total,
        stage_latencies_ms,
        qa_verdict: aggregate_qa_verdict(&ctx.qa_verdicts).to_string(),
        prompt_hash,
    })
}

/// Convert a USD float to whole cents.
#[expect(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss,
    reason = "f64 to u64: no TryFrom impl; value is clamped to [0, u64::MAX] after round()"
)]
fn usd_to_cents(usd: f64) -> u64 {
    let cents = (usd * 100.0).round();
    let max_as_f64 = u64::MAX as f64; // SAFETY: u64::MAX → f64 is the f64 nearest below u64::MAX; saturation threshold
    if cents.is_nan() || cents < 0.0 {
        0
    } else if cents >= max_as_f64 {
        u64::MAX
    } else {
        cents as u64 // SAFETY: cents in [0.0, u64::MAX as f64) after guards above
    }
}

/// Aggregate QA verdicts: Fail > Partial > Pass.
fn aggregate_qa_verdict(verdicts: &[QaVerdict]) -> QaVerdict {
    if verdicts.contains(&QaVerdict::Fail) {
        QaVerdict::Fail
    } else if verdicts.contains(&QaVerdict::Partial) {
        QaVerdict::Partial
    } else {
        QaVerdict::Pass
    }
}

/// SHA-256 hash of the serialized prompt set, prefixed with `sha256:`.
fn compute_prompt_hash(prompts: &[crate::prompt::PromptSpec]) -> crate::error::Result<String> {
    let bytes = serde_json::to_vec(prompts).map_err(|e| {
        crate::error::SerializationSnafu {
            message: format!("serialize prompts for prompt hash: {e}"),
        }
        .build()
    })?;
    let hash = Sha256::digest(&bytes);
    let hex = hash
        .iter()
        .fold(String::with_capacity(hash.len() * 2), |mut acc, b| {
            use std::fmt::Write;
            // intentional: write to String cannot fail
            // kanon:ignore RUST/no-silent-result-swallow — write! to an in-memory String is infallible by std::fmt::Write invariant
            let _ = write!(acc, "{b:02x}");
            acc
        });
    Ok(format!("sha256:{hex}"))
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
#[expect(
    clippy::indexing_slicing,
    reason = "test assertions on known-length collections"
)]
mod tests {
    use std::sync::Arc;

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

        let entries: Vec<_> = std::fs::read_dir(tmp.path()).expect("read dir").collect();
        assert_eq!(entries.len(), 1, "exactly one JSONL file should exist");

        let path = entries[0].as_ref().expect("valid entry").path();
        let content = std::fs::read_to_string(&path).expect("read file");
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

        let entries: Vec<_> = std::fs::read_dir(tmp.path()).expect("read dir").collect();
        assert_eq!(entries.len(), 1, "same-day dispatches share one file");

        let path = entries[0].as_ref().expect("valid entry").path();
        let content = std::fs::read_to_string(&path).expect("read file");
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

        // Seed an old file to simulate a prior day's log.
        let old_file = tmp.path().join("2026-04-16.jsonl");
        std::fs::write(&old_file, "{\"old\":true}\n").expect("write old file");

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

        let entries: Vec<_> = std::fs::read_dir(tmp.path()).expect("read dir").collect();
        assert_eq!(entries.len(), 2, "old file and new file should both exist");

        let mut found_old = false;
        let mut found_new = false;
        for entry in entries {
            let name = entry.expect("valid entry").file_name();
            let name = name.to_string_lossy();
            if name == "2026-04-16.jsonl" {
                found_old = true;
            } else if name.ends_with(".jsonl") {
                found_new = true;
            }
        }
        assert!(found_old, "old file should be preserved");
        assert!(found_new, "new file for current date should be created");
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
