// WHY: The dispatch pipeline (preparation → health_check → execution →
// post-processing) executes each stage in order. Each stage is a named,
// independently testable unit with a uniform interface. The DispatchPipeline
// driver wires them in order and surfaces which stage failed when an error
// occurs.
//
// Remaining Wave 4 follow-up stages: QA-gate (#3459), validate (#3460),
// record (#3461).

use crate::pipeline::context::PipelineContext;
use crate::pipeline::error::PipelineError;

/// Per-stage execution context (inputs and accumulated outputs).
pub(crate) mod context;
/// Stage-identified error type.
pub(crate) mod error;
/// Stage 2: drive frontier group loop, collect session outcomes.
pub(crate) mod execution;
/// Stage 1b: probe target backend reachability before spawning sessions.
pub(crate) mod health_check;
/// Stage 3: record metrics, assemble result, finish store record.
pub(crate) mod post_processing;
/// Stage 1: validate inputs, build DAG, compute frontier, initialise shared state.
pub(crate) mod preparation;
/// Stage 0: deterministic pre-dispatch validation gates.
pub(crate) mod validation;

// Re-export stage implementations for use by the orchestrator.
pub(crate) use execution::ExecutionStage;
pub(crate) use health_check::HealthCheckStage;
pub(crate) use post_processing::PostProcessingStage;
pub(crate) use preparation::PreparationStage;
pub(crate) use validation::ValidationStage;

// ---------------------------------------------------------------------------
// PipelineStage trait
// ---------------------------------------------------------------------------

/// A single named stage in the dispatch pipeline.
///
/// Stages are executed in order by [`DispatchPipeline`].  Each stage reads
/// from [`PipelineContext`] and writes its outputs back to it so the next
/// stage can consume them.
pub(crate) trait PipelineStage: Send + Sync {
    /// Human-readable stage name used in error messages and tracing spans.
    fn name(&self) -> &'static str;

    /// Execute this stage.
    ///
    /// May read from and write to `ctx`.  Returns a [`PipelineError`] that
    /// wraps the underlying error and identifies the stage that failed.
    fn run(
        &self,
        ctx: &mut PipelineContext,
    ) -> impl std::future::Future<Output = Result<(), PipelineError>> + Send;
}

// ---------------------------------------------------------------------------
// DispatchPipeline driver
// ---------------------------------------------------------------------------

/// Ordered sequence of pipeline stages that executes a dispatch.
///
/// Constructed with [`DispatchPipeline::default`] (the standard 3-stage
/// pipeline) or manually via [`DispatchPipeline::new`] for custom / test
/// configurations.
pub(crate) struct DispatchPipeline {
    stages: Vec<Box<dyn PipelineStageErased>>,
}

impl Default for DispatchPipeline {
    /// Build the standard 5-stage pipeline:
    /// validation → preparation → `health_check` → execution → post-processing.
    fn default() -> Self {
        Self::new(vec![
            Box::new(ValidationStage),
            Box::new(PreparationStage),
            Box::new(HealthCheckStage),
            Box::new(ExecutionStage),
            Box::new(PostProcessingStage),
        ])
    }
}

impl DispatchPipeline {
    /// Construct a pipeline from an explicit list of boxed stages.
    pub(crate) fn new(stages: Vec<Box<dyn PipelineStageErased>>) -> Self {
        Self { stages }
    }

    /// Run all stages in order.
    ///
    /// Returns the first [`PipelineError`] encountered, with `stage()` identifying
    /// which stage failed.  On success, [`PipelineContext::result`] is populated.
    pub(crate) async fn run(&self, ctx: &mut PipelineContext) -> Result<(), PipelineError> {
        for stage in &self.stages {
            stage.run_erased(ctx).await?;
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Object-safe wrapper for PipelineStage
// ---------------------------------------------------------------------------

// WHY: PipelineStage uses `impl Future` which is not object-safe. We bridge
// it with a sealed trait that wraps the async fn in a pinned box so we can
// store heterogeneous stages in a Vec<Box<dyn PipelineStageErased>>.

pub(crate) trait PipelineStageErased: Send + Sync {
    fn run_erased<'a>(
        &'a self,
        ctx: &'a mut PipelineContext,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), PipelineError>> + Send + 'a>>;
}

impl<T: PipelineStage> PipelineStageErased for T {
    fn run_erased<'a>(
        &'a self,
        ctx: &'a mut PipelineContext,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), PipelineError>> + Send + 'a>>
    {
        Box::pin(self.run(ctx))
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

    use crate::engine::{SessionEvent, SessionResult};
    use crate::http::mock::{MockEngine, MockOutcome};
    use crate::orchestrator::OrchestratorConfig;
    use crate::pipeline::context::PipelineContext;
    use crate::prompt::PromptSpec;
    use crate::qa::QaGate;
    use crate::types::{DispatchSpec, MechanicalIssue, QaResult, QaVerdict, SessionStatus};

    use super::DispatchPipeline;

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

    fn make_context(mock_outcomes: Vec<MockOutcome>, prompts: Vec<PromptSpec>) -> PipelineContext {
        let engine = Arc::new(MockEngine::new(mock_outcomes));
        let qa = Arc::new(AlwaysPassQa);
        let spec = DispatchSpec::new(".".to_owned(), prompts.iter().map(|p| p.number).collect());
        PipelineContext::new(
            spec,
            prompts,
            engine,
            qa,
            OrchestratorConfig::default().default_budget_usd(10.0),
            #[cfg(feature = "storage-fjall")]
            None,
        )
    }

    #[tokio::test]
    async fn default_pipeline_runs_happy_path() {
        let prompts = vec![PromptSpec {
            number: 1,
            description: "test task".to_owned(),
            depends_on: vec![],
            acceptance_criteria: vec![],
            blast_radius: vec![],
            body: "do the thing".to_owned(),

            prompt_components: None,
        }];

        let mock_outcomes = vec![MockOutcome::Success {
            events: vec![SessionEvent::TurnComplete { turn: 5 }],
            result: SessionResult {
                session_id: "s1".to_owned(),
                cost_usd: 0.50,
                num_turns: 5,
                duration_ms: 100,
                success: true,
                result_text: Some("done".to_owned()),
                model: Some("claude-3-5-sonnet".to_owned()),
            },
        }];

        let mut ctx = make_context(mock_outcomes, prompts);
        let pipeline = DispatchPipeline::default();
        pipeline
            .run(&mut ctx)
            .await
            .expect("pipeline should succeed");

        let result = ctx.result.expect("result should be set after pipeline");
        assert!(!result.aborted);
        assert_eq!(result.outcomes.len(), 1);
        assert_eq!(result.outcomes[0].status, SessionStatus::Success);
        assert!((result.total_cost_usd - 0.50).abs() < 0.01);
    }

    #[tokio::test]
    async fn pipeline_reports_stage_on_failure() {
        // Empty prompts → preflight error in validation stage.
        let mut ctx = make_context(vec![], vec![]);
        let pipeline = DispatchPipeline::default();
        let err = pipeline
            .run(&mut ctx)
            .await
            .expect_err("should fail on empty prompts");

        assert_eq!(err.stage(), "validation");
        assert!(err.to_string().contains("validation"), "msg: {err}");
    }
}
