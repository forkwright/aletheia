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
use crate::orchestrator::OrchestratorConfig;
use crate::pipeline::PipelineStage;
use crate::pipeline::context::PipelineContext;
use crate::pipeline::error::{PipelineError, StageSnafu};
use crate::session::options::EngineConfig;

fn build_engine_config(
    cfg: &OrchestratorConfig,
    after_action_log_dir: Option<std::path::PathBuf>,
    max_turns: Option<u32>,
) -> EngineConfig {
    let mut config = EngineConfig::new(crate::engine::AgentOptions::new())
        .idle_timeout_opt(cfg.session_idle_timeout)
        .routing(cfg.routing.clone())
        .after_action_log_dir(after_action_log_dir);
    for dir in &cfg.additional_dirs {
        config = config.add_dir(dir.clone());
    }
    if let Some(t) = max_turns {
        config = config.max_turns(t);
    }
    config
}

/// Preparation stage: validate, build DAG, compute frontier, initialise shared state.
pub(crate) struct PreparationStage;

impl PipelineStage for PreparationStage {
    fn name(&self) -> &'static str {
        "preparation"
    }

    async fn run(&self, ctx: &mut PipelineContext) -> Result<(), PipelineError> {
        let t0 = std::time::Instant::now();

        if ctx.prompts.is_empty() {
            return PreflightSnafu {
                reason: "no prompts to dispatch",
            }
            .fail()
            .context(StageSnafu { stage: self.name() });
        }

        ctx.dispatch_id = koina::ulid::Ulid::new().to_string();
        ctx.start = std::time::Instant::now();
        ctx.start_ts = jiff::Timestamp::now();

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

        ctx.prompt_map = ctx
            .prompts
            .iter()
            .map(|p| (p.number, p.clone()))
            .collect::<HashMap<_, _>>();

        // WHY: When role or standards are configured, each prompt is split into
        // a static prefix (cacheable across dispatches) and dynamic suffix
        // (per-dispatch state), enabling prompt cache hits at the LLM boundary
        // when dispatched through hermeneus.

        let cfg = &ctx.config;
        let should_split = cfg.role.is_some()
            || cfg.standards_dir.is_some()
            || !cfg.standards.is_empty()
            || cfg.scope.is_some();

        if should_split {
            let cache = crate::prompt_cache::StaticPrefixCache::load(
                cfg.role.as_deref(),
                cfg.standards_dir.as_deref(),
                &cfg.standards,
            )
            .await
            .context(StageSnafu { stage: self.name() })?;

            for prompt in &mut ctx.prompts {
                let components = crate::prompt_cache::PromptComponents::build_with_cache(
                    &cache,
                    &ctx.spec.project,
                    cfg.scope.as_deref(),
                    &prompt.body,
                );
                prompt.prompt_components = Some(components);
            }
            ctx.prompt_map = ctx
                .prompts
                .iter()
                .map(|p| (p.number, p.clone()))
                .collect::<HashMap<_, _>>();
        }

        ctx.budget = Some(Arc::new(Budget::new(
            ctx.spec.budget_usd.or(cfg.default_budget_usd),
            cfg.default_budget_turns,
            cfg.max_duration
                .map(|d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX)),
        )));
        ctx.cost_ledger = Some(Arc::new(CostLedger::new()));

        ctx.resume_policy = crate::resume::ResumePolicy::default();
        ctx.engine_config = Some(build_engine_config(
            cfg,
            ctx.after_action_log_dir.clone(),
            ctx.spec.max_turns,
        ));

        ctx.max_concurrent = usize::try_from(
            ctx.spec
                .max_parallel
                .map_or(cfg.max_concurrent, |p| p.min(cfg.max_concurrent)),
        )
        .unwrap_or(usize::MAX);

        #[cfg(feature = "storage-fjall")]
        {
            if let Some(store) = ctx.store.as_ref() {
                let id = store
                    .create_dispatch(&ctx.spec.project, &ctx.spec)
                    .context(StageSnafu { stage: self.name() })?;
                ctx.store_dispatch_id = Some(id);
            }
        }
        #[cfg(not(feature = "storage-fjall"))]
        let () = ();

        ctx.record_stage_latency(self.name(), t0.elapsed());
        Ok(())
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
                context_policy: crate::dag::ContextPolicy::Fresh,
                output_format: None,
                worktree: crate::prompt::WorktreePolicy::default(),
                acceptance_criteria: vec![],
                blast_radius: vec![],
                body: "do first".to_owned(),

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
            context_policy: crate::dag::ContextPolicy::Fresh,
            output_format: None,
            worktree: crate::prompt::WorktreePolicy::default(),
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
    async fn preparation_prefers_spec_budget_over_config_default() {
        let prompts = vec![PromptSpec {
            number: 1,
            description: "first".to_owned(),
            depends_on: vec![],
            context_policy: crate::dag::ContextPolicy::Fresh,
            output_format: None,
            worktree: crate::prompt::WorktreePolicy::default(),
            acceptance_criteria: vec![],
            blast_radius: vec![],
            body: "do first".to_owned(),
            prompt_components: None,
        }];
        let config = OrchestratorConfig::default().default_budget_usd(10.0);
        let mut ctx = make_context_with_config(prompts, config);
        ctx.spec.budget_usd = Some(2.5);

        let stage = PreparationStage;
        stage
            .run(&mut ctx)
            .await
            .expect("preparation should succeed");

        assert_eq!(ctx.budget().max_cost_usd, Some(2.5));
    }

    #[tokio::test]
    async fn preparation_passes_configured_additional_dirs_to_engine() {
        let prompts = vec![PromptSpec {
            number: 1,
            description: "first".to_owned(),
            depends_on: vec![],
            context_policy: crate::dag::ContextPolicy::Fresh,
            output_format: None,
            worktree: crate::prompt::WorktreePolicy::default(),
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

    #[tokio::test]
    async fn preparation_reuses_static_prefix_cache_for_multiple_prompts() {
        // WHY: Issue 5686: role/standards files must be read once per dispatch,
        // not once per prompt. Loading them for every prompt blocks the Tokio
        // worker and wastes I/O on identical content.
        use std::io::Write as _;

        let dir = tempfile::TempDir::new().expect("create temp dir");
        let role_path = dir.path().join("role.md");
        {
            let mut f = std::fs::File::create(&role_path).expect("create role file");
            f.write_all(b"File-based role definition.")
                .expect("write role file");
        }
        let std_path = dir.path().join("RUST.md");
        {
            let mut f = std::fs::File::create(&std_path).expect("create standard file");
            f.write_all(b"Use clippy.").expect("write standard file");
        }

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
                body: "do first".to_owned(),
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
                body: "do second".to_owned(),
                prompt_components: None,
            },
            PromptSpec {
                number: 3,
                description: "third".to_owned(),
                depends_on: vec![1],
                context_policy: crate::dag::ContextPolicy::Fresh,
                output_format: None,
                worktree: crate::prompt::WorktreePolicy::default(),
                acceptance_criteria: vec![],
                blast_radius: vec![],
                body: "do third".to_owned(),
                prompt_components: None,
            },
        ];

        let config = OrchestratorConfig::default()
            .role(role_path.to_str().expect("role path is utf-8"))
            .standards_dir(dir.path())
            .standards(vec!["RUST".to_owned()]);
        let mut ctx = make_context_with_config(prompts, config);

        PreparationStage
            .run(&mut ctx)
            .await
            .expect("preparation should succeed");

        let prefixes: Vec<String> = ctx
            .prompts
            .iter()
            .map(|p| {
                p.prompt_components
                    .as_ref()
                    .expect("prompt components should be set")
                    .static_prefix
                    .clone()
            })
            .collect();

        assert!(
            prefixes.iter().all(|p| p == &prefixes[0]),
            "all prompts must share the same static prefix"
        );
        assert!(prefixes[0].contains("File-based role definition."));
        assert!(prefixes[0].contains("Use clippy."));
        assert!(prefixes[0].contains("Validation Gate"));
    }
}
