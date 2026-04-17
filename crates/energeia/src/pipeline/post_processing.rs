// WHY: Post-processing stage records Prometheus metrics for every outcome,
// assembles the final DispatchResult, finishes the store dispatch record, and
// emits the completion trace event. Separating this from execution means the
// metric/store-update code has a single home and execution can stay focused on
// session management.

use jiff::Timestamp;

use crate::pipeline::context::PipelineContext;
use crate::pipeline::error::PipelineError;
use crate::pipeline::PipelineStage;
use crate::types::DispatchResult;

/// Post-processing stage: record metrics, assemble result, finish store record.
pub(crate) struct PostProcessingStage;

impl PipelineStage for PostProcessingStage {
    fn name(&self) -> &'static str {
        "post_processing"
    }

    async fn run(&self, ctx: &mut PipelineContext) -> Result<(), PipelineError> {
        // --- Record Prometheus metrics for all outcomes ---

        for outcome in &ctx.outcomes {
            let model = outcome.model.as_deref().unwrap_or("unknown");
            let blast_radius = if outcome.blast_radius.is_empty() {
                "unknown"
            } else {
                // SAFETY: non-empty checked above
                #[expect(
                    clippy::indexing_slicing,
                    reason = "blast_radius checked non-empty at line above"
                )]
                outcome.blast_radius[0].as_str()
            };

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

        // --- Assemble DispatchResult ---

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

        // --- Finish dispatch record ---

        #[cfg(feature = "storage-fjall")]
        if let (Some(store), Some(store_id)) = (&ctx.store, &ctx.store_dispatch_id) {
            let status = if ctx.aborted {
                crate::store::records::DispatchStatus::Failed
            } else {
                crate::store::records::DispatchStatus::Completed
            };
            if let Err(e) = store.finish_dispatch(store_id, status) {
                tracing::warn!(error = %e, "failed to finish dispatch record");
            }
        }

        tracing::info!(
            dispatch_id = %ctx.dispatch_id,
            total_cost = result.total_cost_usd,
            duration_ms = result.duration_ms,
            outcomes = result.outcomes.len(),
            aborted = ctx.aborted,
            "dispatch complete"
        );

        ctx.result = Some(result);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::Instant;

    use crate::engine::{SessionEvent, SessionResult};
    use crate::http::mock::{MockEngine, MockOutcome};
    use crate::orchestrator::OrchestratorConfig;
    use crate::pipeline::context::PipelineContext;
    use crate::pipeline::preparation::PreparationStage;
    use crate::pipeline::execution::ExecutionStage;
    use crate::pipeline::PipelineStage as _;
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
            acceptance_criteria: vec![],
            blast_radius: vec![],
            body: "do the thing".to_owned(),
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
}
