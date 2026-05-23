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
use crate::pipeline::PipelineStage;
use crate::pipeline::context::PipelineContext;
use crate::pipeline::error::{PipelineError, StageSnafu};
use crate::session::options::EngineConfig;

/// Preparation stage: validate, build DAG, compute frontier, initialise shared state.
pub(crate) struct PreparationStage;

impl PipelineStage for PreparationStage {
    fn name(&self) -> &'static str {
        "preparation"
    }

    async fn run(&self, ctx: &mut PipelineContext) -> Result<(), PipelineError> {
        let t0 = std::time::Instant::now();

        // --- Validate inputs ---

        if ctx.prompts.is_empty() {
            return PreflightSnafu {
                reason: "no prompts to dispatch",
            }
            .fail()
            .context(StageSnafu { stage: self.name() });
        }

        // --- Assign dispatch ID and record start time ---

        ctx.dispatch_id = koina::ulid::Ulid::new().to_string();
        ctx.start = std::time::Instant::now();
        ctx.start_ts = jiff::Timestamp::now();

        // --- Build DAG and compute frontier ---

        let dag =
            crate::prompt::build_dag(&ctx.prompts).context(StageSnafu { stage: self.name() })?;
        ctx.set_dag_and_compute_frontier(dag);

        if ctx.frontier().is_empty() {
            return PreflightSnafu {
                reason: "all prompts already completed or DAG has no dispatchable nodes",
            }
            .fail()
            .context(StageSnafu { stage: self.name() });
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

        // --- Build prompt cache components ---
        //
        // When role or standards are configured, split each prompt into a
        // static prefix (cacheable across dispatches) and dynamic suffix
        // (per-dispatch state). This enables prompt cache hits at the LLM
        // boundary when dispatched through hermeneus.

        let cfg = &ctx.config;
        let should_split = cfg.role.is_some()
            || cfg.standards_dir.is_some()
            || !cfg.standards.is_empty()
            || cfg.scope.is_some();

        if should_split {
            for prompt in &mut ctx.prompts {
                let components = crate::prompt_cache::PromptComponents::build(
                    cfg.role.as_deref(),
                    &ctx.spec.project,
                    cfg.standards_dir.as_deref(),
                    &cfg.standards,
                    cfg.scope.as_deref(),
                    &prompt.body,
                );
                prompt.prompt_components = Some(components);
            }
            // Sync prompt_map with updated prompts.
            ctx.prompt_map = ctx
                .prompts
                .iter()
                .map(|p| (p.number, p.clone()))
                .collect::<HashMap<_, _>>();
        }

        // --- Budget and cancellation ---

        ctx.budget = Some(Arc::new(Budget::new(
            cfg.default_budget_usd,
            cfg.default_budget_turns,
            cfg.max_duration
                .map(|d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX)),
        )));
        ctx.cost_ledger = Some(Arc::new(CostLedger::new()));

        ctx.resume_policy = crate::resume::ResumePolicy::default();
        let mut engine_config = EngineConfig::new(crate::engine::AgentOptions::new())
            .idle_timeout_opt(cfg.session_idle_timeout);
        for dir in &cfg.additional_dirs {
            engine_config = engine_config.add_dir(dir.clone());
        }
        if let Some(max_turns) = ctx.spec.max_turns {
            engine_config = engine_config.max_turns(max_turns);
        }
        ctx.engine_config = Some(engine_config);

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
        let () = ();

        ctx.record_stage_latency(self.name(), t0.elapsed());
        Ok(())
    }
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use std::sync::Arc;

    use crate::http::mock::MockEngine;
    use crate::orchestrator::OrchestratorConfig;
    use crate::pipeline::PipelineStage as _;
    use crate::pipeline::context::PipelineContext;
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

    fn make_context_with_config(
        prompts: Vec<PromptSpec>,
        config: OrchestratorConfig,
    ) -> PipelineContext {
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
            config,
            #[cfg(feature = "storage-fjall")]
            None,
        )
    }

    fn make_context_with_prompts(prompts: Vec<PromptSpec>) -> PipelineContext {
        make_context_with_config(prompts, OrchestratorConfig::default())
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

                prompt_components: None,
            },
            PromptSpec {
                number: 2,
                description: "second".to_owned(),
                depends_on: vec![1],
                acceptance_criteria: vec![],
                blast_radius: vec![],
                body: "do second".to_owned(),

                prompt_components: None,
            },
        ];
        let mut ctx = make_context_with_prompts(prompts);

        let stage = PreparationStage;
        stage
            .run(&mut ctx)
            .await
            .expect("preparation should succeed");

        assert!(!ctx.dispatch_id.is_empty());
        assert!(ctx.dag.is_some());
        assert_eq!(ctx.frontier().len(), 2); // [1] then [2]
        assert_eq!(ctx.prompt_map.len(), 2);
        assert!(ctx.budget.is_some());
        assert!(ctx.cost_ledger.is_some());
        assert!(ctx.engine_config.is_some());
    }

    #[tokio::test]
    async fn preparation_keeps_parallelism_and_turn_limit_separate() {
        let prompts = vec![PromptSpec {
            number: 1,
            description: "first".to_owned(),
            depends_on: vec![],
            acceptance_criteria: vec![],
            blast_radius: vec![],
            body: "do first".to_owned(),
            prompt_components: None,
        }];
        let mut ctx = make_context_with_prompts(prompts);
        ctx.spec.max_parallel = Some(2);
        ctx.spec.max_turns = Some(7);

        let stage = PreparationStage;
        stage
            .run(&mut ctx)
            .await
            .expect("preparation should succeed");

        assert_eq!(ctx.max_concurrent, 2);
        assert_eq!(ctx.engine_config().options.max_turns, Some(7));
    }

    #[tokio::test]
    async fn preparation_passes_configured_additional_dirs_to_engine() {
        let prompts = vec![PromptSpec {
            number: 1,
            description: "first".to_owned(),
            depends_on: vec![],
            acceptance_criteria: vec![],
            blast_radius: vec![],
            body: "do first".to_owned(),
            prompt_components: None,
        }];
        let config = OrchestratorConfig::default()
            .add_dir("/workspace/shared")
            .add_dir("/workspace/fixtures");
        let mut ctx = make_context_with_config(prompts, config);

        let stage = PreparationStage;
        stage
            .run(&mut ctx)
            .await
            .expect("preparation should succeed");

        assert_eq!(
            ctx.engine_config().additional_dirs,
            vec![
                std::path::PathBuf::from("/workspace/shared"),
                std::path::PathBuf::from("/workspace/fixtures"),
            ]
        );
    }

    #[tokio::test]
    async fn preparation_fails_on_empty_prompts() {
        let mut ctx = make_context_with_prompts(vec![]);

        let stage = PreparationStage;
        let err = stage
            .run(&mut ctx)
            .await
            .expect_err("should fail on empty prompts");

        assert!(
            err.to_string().contains("preparation"),
            "no stage in: {err}"
        );
        assert!(
            err.to_string().contains("no prompts"),
            "no reason in: {err}"
        );
    }
}
