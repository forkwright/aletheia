// WHY: Deterministic pre-dispatch gates that run before any mutable work.
// Validates prompt bodies, target crate existence, dispatch locks, and budget
// configuration so that later stages can assume inputs are well-formed.

use std::path::Path;

use snafu::ResultExt as _;

use crate::error::PreflightSnafu;
use crate::pipeline::PipelineStage;
use crate::pipeline::context::PipelineContext;
use crate::pipeline::error::{PipelineError, StageSnafu};

/// Maximum allowed prompt body length in characters.
const MAX_PROMPT_LENGTH: usize = 500_000;

/// Stage 0: deterministic pre-dispatch validation gates.
///
/// Runs before [`PreparationStage`] and [`HealthCheckStage`].
pub(crate) struct ValidationStage;

impl PipelineStage for ValidationStage {
    fn name(&self) -> &'static str {
        "validation"
    }

    async fn run(&self, ctx: &mut PipelineContext) -> Result<(), PipelineError> {
        // 1. Prompts are non-empty and under length limit.
        if ctx.prompts.is_empty() {
            return PreflightSnafu {
                reason: "no prompts to dispatch",
            }
            .fail()
            .context(StageSnafu { stage: self.name() });
        }

        for prompt in &ctx.prompts {
            if prompt.body.is_empty() {
                return PreflightSnafu {
                    reason: format!("prompt {} has empty body", prompt.number),
                }
                .fail()
                .context(StageSnafu { stage: self.name() });
            }
            if prompt.body.len() > MAX_PROMPT_LENGTH {
                return PreflightSnafu {
                    reason: format!(
                        "prompt {} body exceeds {} characters",
                        prompt.number, MAX_PROMPT_LENGTH
                    ),
                }
                .fail()
                .context(StageSnafu { stage: self.name() });
            }
        }

        // 2. Target crate path exists.
        let project_path = Path::new(&ctx.spec.project);
        if !project_path.is_dir() {
            return PreflightSnafu {
                reason: format!(
                    "target crate path does not exist: {}",
                    project_path.display()
                ),
            }
            .fail()
            .context(StageSnafu { stage: self.name() });
        }

        // 3. No existing dispatch lock for this project.
        #[cfg(feature = "storage-fjall")]
        {
            if let Some(ref store) = ctx.store {
                let dispatches = store
                    .list_dispatches(crate::store::SCAN_LIMIT_DISPATCHES)
                    .context(StageSnafu { stage: self.name() })?;

                for dispatch in dispatches {
                    if dispatch.project == ctx.spec.project
                        && dispatch.status == crate::store::records::DispatchStatus::Running
                    {
                        return PreflightSnafu {
                            reason: format!(
                                "existing dispatch lock for project '{}'",
                                ctx.spec.project
                            ),
                        }
                        .fail()
                        .context(StageSnafu { stage: self.name() });
                    }
                }
            }
        }

        // 4. Budget is configured and greater than zero.
        let has_positive_budget = ctx.config.default_budget_usd.is_some_and(|v| v > 0.0)
            || ctx.config.default_budget_turns.is_some_and(|v| v > 0);

        if !has_positive_budget {
            return PreflightSnafu {
                reason: "budget must be greater than 0",
            }
            .fail()
            .context(StageSnafu { stage: self.name() });
        }

        Ok(())
    }
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use std::sync::Arc;

    use tempfile::TempDir;

    use crate::http::mock::MockEngine;
    use crate::orchestrator::OrchestratorConfig;
    use crate::pipeline::PipelineStage as _;
    use crate::pipeline::context::PipelineContext;
    use crate::prompt::PromptSpec;
    use crate::qa::QaGate;
    use crate::types::{DispatchSpec, MechanicalIssue, QaResult, QaVerdict};

    use super::ValidationStage;

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

    fn make_context(
        prompts: Vec<PromptSpec>,
        config: OrchestratorConfig,
        project: String,
        #[cfg(feature = "storage-fjall")] store: Option<Arc<crate::store::EnergeiaStore>>,
    ) -> PipelineContext {
        let engine = Arc::new(MockEngine::new(vec![]));
        let qa = Arc::new(AlwaysPassQa);
        let spec = DispatchSpec::new(project, prompts.iter().map(|p| p.number).collect());
        PipelineContext::new(
            spec,
            prompts,
            engine,
            qa,
            config,
            #[cfg(feature = "storage-fjall")]
            store,
        )
    }

    fn valid_prompt() -> PromptSpec {
        PromptSpec {
            number: 1,
            description: "test".to_owned(),
            depends_on: vec![],
            acceptance_criteria: vec![],
            blast_radius: vec![],
            body: "do the thing".to_owned(),
        }
    }

    fn valid_config() -> OrchestratorConfig {
        OrchestratorConfig::default().default_budget_usd(10.0)
    }

    #[tokio::test]
    async fn empty_prompt_rejected() {
        let mut ctx = make_context(
            vec![],
            valid_config(),
            ".".to_owned(),
            #[cfg(feature = "storage-fjall")]
            None,
        );
        let stage = ValidationStage;
        let err = stage
            .run(&mut ctx)
            .await
            .expect_err("should fail on empty prompts");

        assert_eq!(err.stage(), "validation");
        assert!(err.to_string().contains("no prompts"), "msg: {err}");
    }

    #[tokio::test]
    async fn overlong_prompt_rejected() {
        let long_body = "x".repeat(super::MAX_PROMPT_LENGTH + 1);
        let prompts = vec![PromptSpec {
            number: 1,
            description: "test".to_owned(),
            depends_on: vec![],
            acceptance_criteria: vec![],
            blast_radius: vec![],
            body: long_body,
        }];
        let mut ctx = make_context(
            prompts,
            valid_config(),
            ".".to_owned(),
            #[cfg(feature = "storage-fjall")]
            None,
        );
        let stage = ValidationStage;
        let err = stage
            .run(&mut ctx)
            .await
            .expect_err("should fail on overlong prompt");

        assert_eq!(err.stage(), "validation");
        assert!(err.to_string().contains("exceeds 500000"), "msg: {err}");
    }

    #[tokio::test]
    async fn missing_crate_rejected() {
        let prompts = vec![valid_prompt()];
        let mut ctx = make_context(
            prompts,
            valid_config(),
            "nonexistent-crate-12345".to_owned(),
            #[cfg(feature = "storage-fjall")]
            None,
        );
        let stage = ValidationStage;
        let err = stage
            .run(&mut ctx)
            .await
            .expect_err("should fail on missing crate");

        assert_eq!(err.stage(), "validation");
        assert!(
            err.to_string().contains("target crate path does not exist"),
            "msg: {err}"
        );
    }

    #[cfg(feature = "storage-fjall")]
    #[tokio::test]
    async fn existing_lock_rejected() {
        let temp_dir = TempDir::new().unwrap();
        let project = temp_dir.path().to_str().unwrap().to_owned();

        let db = fjall::Database::builder(temp_dir.path().join("db"))
            .open()
            .unwrap();
        let store = Arc::new(crate::store::EnergeiaStore::new(&db).unwrap());

        // Create a running dispatch for the same project.
        let spec = DispatchSpec::new(project.clone(), vec![1]);
        let _id = store.create_dispatch(&project, &spec).unwrap();

        let prompts = vec![valid_prompt()];
        let mut ctx = make_context(prompts, valid_config(), project, Some(store));
        let stage = ValidationStage;
        let err = stage
            .run(&mut ctx)
            .await
            .expect_err("should fail on existing lock");

        assert_eq!(err.stage(), "validation");
        assert!(
            err.to_string().contains("existing dispatch lock"),
            "msg: {err}"
        );
    }

    #[tokio::test]
    async fn zero_budget_rejected() {
        let prompts = vec![valid_prompt()];
        let config = OrchestratorConfig::default().default_budget_usd(0.0);
        let mut ctx = make_context(
            prompts,
            config,
            ".".to_owned(),
            #[cfg(feature = "storage-fjall")]
            None,
        );
        let stage = ValidationStage;
        let err = stage
            .run(&mut ctx)
            .await
            .expect_err("should fail on zero budget");

        assert_eq!(err.stage(), "validation");
        assert!(
            err.to_string().contains("budget must be greater than 0"),
            "msg: {err}"
        );
    }

    #[tokio::test]
    async fn all_gates_passing_ok() {
        let temp_dir = TempDir::new().unwrap();
        let project = temp_dir.path().to_str().unwrap().to_owned();

        let prompts = vec![valid_prompt()];
        let mut ctx = make_context(
            prompts,
            valid_config(),
            project,
            #[cfg(feature = "storage-fjall")]
            None,
        );
        let stage = ValidationStage;
        stage.run(&mut ctx).await.expect("validation should pass");
    }
}
