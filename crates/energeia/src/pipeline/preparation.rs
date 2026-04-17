// WHY: Preparation stage validates dispatch inputs, builds the prompt DAG,
// computes the execution frontier, initialises shared state (budget, ledger,
// cancel token), and creates the store dispatch record. Separating this from
// execution means failures here are reported as [preparation] errors and don't
// touch any shared mutable state that would leave the context in an
// inconsistent state.

use std::collections::HashMap;
use std::sync::Arc;

use snafu::ResultExt as _;

use crate::budget::Budget;
use crate::cost_ledger::CostLedger;
use crate::error::PreflightSnafu;
use crate::pipeline::context::PipelineContext;
use crate::pipeline::error::{PipelineError, StageSnafu};
use crate::pipeline::PipelineStage;
use crate::session::options::EngineConfig;

/// Preparation stage: validate, build DAG, compute frontier, initialise shared state.
pub(crate) struct PreparationStage;

impl PipelineStage for PreparationStage {
    fn name(&self) -> &'static str {
        "preparation"
    }

    async fn run(&self, ctx: &mut PipelineContext) -> Result<(), PipelineError> {
        // --- Validate inputs ---

        if ctx.prompts.is_empty() {
            return PreflightSnafu {
                reason: "no prompts to dispatch",
            }
            .fail()
            .context(StageSnafu {
                stage: self.name(),
            });
        }

        // --- Assign dispatch ID and record start time ---

        ctx.dispatch_id = koina::ulid::Ulid::new().to_string();
        ctx.start = std::time::Instant::now();

        // --- Build DAG and compute frontier ---

        let dag = crate::prompt::build_dag(&ctx.prompts).context(StageSnafu {
            stage: self.name(),
        })?;
        ctx.set_dag_and_compute_frontier(dag);

        if ctx.frontier().is_empty() {
            return PreflightSnafu {
                reason: "all prompts already completed or DAG has no dispatchable nodes",
            }
            .fail()
            .context(StageSnafu {
                stage: self.name(),
            });
        }

        tracing::info!(
            dispatch_id = %ctx.dispatch_id,
            project = %ctx.spec.project,
            groups = ctx.frontier().len(),
            total_prompts = ctx.prompts.len(),
            "starting dispatch"
        );

        // --- Build prompt lookup ---

        ctx.prompt_map = ctx
            .prompts
            .iter()
            .map(|p| (p.number, p.clone()))
            .collect::<HashMap<_, _>>();

        // --- Budget and cancellation ---

        let cfg = &ctx.config;
        ctx.budget = Some(Arc::new(Budget::new(
            cfg.default_budget_usd,
            cfg.default_budget_turns,
            cfg.max_duration
                .map(|d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX)),
        )));
        ctx.cost_ledger = Some(Arc::new(CostLedger::new()));

        ctx.resume_policy = crate::resume::ResumePolicy::default();
        ctx.engine_config = Some(
            EngineConfig::new(crate::engine::AgentOptions::new())
                .idle_timeout_opt(cfg.session_idle_timeout),
        );

        ctx.max_concurrent = usize::try_from(
            ctx.spec
                .max_parallel
                .map_or(cfg.max_concurrent, |p| p.min(cfg.max_concurrent)),
        )
        .unwrap_or(usize::MAX);

        // --- Create dispatch record ---

        #[cfg(feature = "storage-fjall")]
        {
            ctx.store_dispatch_id = ctx.store.as_ref().and_then(|store| {
                match store.create_dispatch(&ctx.spec.project, &ctx.spec) {
                    Ok(id) => Some(id),
                    Err(e) => {
                        tracing::warn!(error = %e, "failed to create dispatch record");
                        None
                    }
                }
            });
        }
        #[cfg(not(feature = "storage-fjall"))]
        let _ = ();

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crate::http::mock::MockEngine;
    use crate::orchestrator::OrchestratorConfig;
    use crate::pipeline::context::PipelineContext;
    use crate::pipeline::PipelineStage as _;
    use crate::prompt::PromptSpec;
    use crate::qa::QaGate;
    use crate::types::{DispatchSpec, MechanicalIssue, QaResult, QaVerdict};

    use super::PreparationStage;

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

    fn make_context_with_prompts(prompts: Vec<PromptSpec>) -> PipelineContext {
        let engine = Arc::new(MockEngine::new(vec![]));
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
    async fn preparation_builds_dag_and_frontier() {
        let prompts = vec![
            PromptSpec {
                number: 1,
                description: "first".to_owned(),
                depends_on: vec![],
                acceptance_criteria: vec![],
                blast_radius: vec![],
                body: "do first".to_owned(),
            },
            PromptSpec {
                number: 2,
                description: "second".to_owned(),
                depends_on: vec![1],
                acceptance_criteria: vec![],
                blast_radius: vec![],
                body: "do second".to_owned(),
            },
        ];
        let mut ctx = make_context_with_prompts(prompts);

        let stage = PreparationStage;
        stage.run(&mut ctx).await.expect("preparation should succeed");

        assert!(!ctx.dispatch_id.is_empty());
        assert!(ctx.dag.is_some());
        assert_eq!(ctx.frontier().len(), 2); // [1] then [2]
        assert_eq!(ctx.prompt_map.len(), 2);
        assert!(ctx.budget.is_some());
        assert!(ctx.cost_ledger.is_some());
        assert!(ctx.engine_config.is_some());
    }

    #[tokio::test]
    async fn preparation_fails_on_empty_prompts() {
        let mut ctx = make_context_with_prompts(vec![]);

        let stage = PreparationStage;
        let err = stage
            .run(&mut ctx)
            .await
            .expect_err("should fail on empty prompts");

        assert!(err.to_string().contains("preparation"), "no stage in: {err}");
        assert!(
            err.to_string().contains("no prompts"),
            "no reason in: {err}"
        );
    }
}
