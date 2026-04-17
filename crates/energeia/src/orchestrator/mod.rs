// WHY: Thin coordination layer: builds the stage list, creates the pipeline
// context, and runs the pipeline. All dispatch logic lives in the pipeline
// stages (preparation → execution → post_processing). The orchestrator owns
// the public API surface and the store attachment.

use std::sync::Arc;

use crate::engine::DispatchEngine;
use crate::error::{self, Result};
use crate::pipeline::DispatchPipeline;
use crate::pipeline::context::PipelineContext;
use crate::prompt::PromptSpec;
use crate::qa::QaGate;
use crate::session::options::EngineConfig;
use crate::types::{DispatchResult, DispatchSpec};

/// Configuration for the dispatch orchestrator.
pub mod config;
pub(crate) mod group;

pub use config::OrchestratorConfig;

// ---------------------------------------------------------------------------
// Orchestrator
// ---------------------------------------------------------------------------

/// Top-level dispatch orchestrator.
///
/// Builds the pipeline context, runs the 4-stage dispatch pipeline
/// (preparation → execution → `post_processing`), and returns the result.
/// Stage logic lives in [`crate::pipeline`]; this struct owns the
/// engine/QA/store references and exposes the public API.
pub struct Orchestrator {
    engine: Arc<dyn DispatchEngine>,
    qa: Arc<dyn QaGate>,
    #[cfg(feature = "storage-fjall")]
    store: Option<Arc<crate::store::EnergeiaStore>>,
    config: OrchestratorConfig,
}

impl Orchestrator {
    /// Create a new orchestrator.
    #[must_use]
    pub fn new(
        engine: Arc<dyn DispatchEngine>,
        qa: Arc<dyn QaGate>,
        config: OrchestratorConfig,
    ) -> Self {
        Self {
            engine,
            qa,
            #[cfg(feature = "storage-fjall")]
            store: None,
            config,
        }
    }

    /// Attach a state persistence store.
    #[cfg(feature = "storage-fjall")]
    #[must_use]
    pub fn with_store(mut self, store: Arc<crate::store::EnergeiaStore>) -> Self {
        self.store = Some(store);
        self
    }

    /// Execute a full dispatch via the 4-stage pipeline.
    ///
    /// # Flow
    ///
    /// 1. **Preparation** — validate inputs, build DAG, compute frontier,
    ///    initialise budget/ledger/cancel, create store record
    /// 2. **Execution** — frontier group loop: concurrent sessions, DAG
    ///    updates, QA-corrective generation
    /// 3. **Post-processing** — record Prometheus metrics, assemble result,
    ///    finish store record
    ///
    /// # Errors
    ///
    /// Returns [`Error::Preflight`] if the prompt set is empty or the DAG
    /// is invalid. Returns [`Error::Aborted`] if the cancellation token is
    /// triggered before any work begins, or if session execution fails.
    ///
    /// # Cancel safety
    ///
    /// Not cancel-safe. If cancelled mid-dispatch, some sessions may be
    /// spawned but their results never collected, and the DAG state may
    /// be inconsistent. Do not use in `select!` branches.
    pub async fn dispatch(
        &self,
        spec: DispatchSpec,
        prompts: &[PromptSpec],
    ) -> Result<DispatchResult> {
        let mut ctx = PipelineContext::new(
            spec,
            prompts.to_vec(),
            Arc::clone(&self.engine),
            Arc::clone(&self.qa),
            self.config.clone(),
            #[cfg(feature = "storage-fjall")]
            self.store.clone(),
        );

        let pipeline = DispatchPipeline::default();
        pipeline
            .run(&mut ctx)
            .await
            .map_err(crate::error::Error::from)?;

        ctx.result.ok_or_else(|| {
            error::PreflightSnafu {
                reason: "pipeline completed without producing a result",
            }
            .build()
        })
    }

    /// Dry-run: build DAG, compute frontier, return the execution plan without
    /// actually dispatching any sessions.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Preflight`] if the prompt set is empty or the DAG
    /// is invalid.
    pub fn dry_run(&self, prompts: &[PromptSpec]) -> Result<DryRunResult> {
        if prompts.is_empty() {
            return error::PreflightSnafu {
                reason: "no prompts to dispatch",
            }
            .fail();
        }

        let dag = crate::prompt::build_dag(prompts)?;
        let frontier = crate::dag::compute_frontier(&dag);

        let groups: Vec<DryRunGroup> = frontier
            .iter()
            .enumerate()
            .map(|(idx, numbers)| {
                let group_prompts: Vec<DryRunPrompt> = numbers
                    .iter()
                    .filter_map(|&n| {
                        prompts
                            .iter()
                            .find(|p| p.number == n)
                            .map(|p| DryRunPrompt {
                                number: p.number,
                                description: p.description.clone(),
                                depends_on: p.depends_on.clone(),
                            })
                    })
                    .collect();
                DryRunGroup {
                    group_index: idx,
                    prompts: group_prompts,
                }
            })
            .collect();

        let total_prompts = prompts.len();
        let max_concurrent = self.config.max_concurrent;

        Ok(DryRunResult {
            groups,
            total_prompts,
            max_concurrent,
            budget_usd: self.config.default_budget_usd,
            budget_turns: self.config.default_budget_turns,
        })
    }
}

impl std::fmt::Debug for Orchestrator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Orchestrator")
            .field("config", &self.config)
            .finish_non_exhaustive()
    }
}

// ---------------------------------------------------------------------------
// Dry-run types
// ---------------------------------------------------------------------------

/// Result of a dry-run: the execution plan without actual dispatch.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[non_exhaustive]
pub struct DryRunResult {
    /// Ordered groups of prompts that would execute together.
    pub groups: Vec<DryRunGroup>,
    /// Total number of prompts in the dispatch.
    pub total_prompts: usize,
    /// Maximum concurrency configured for the dispatch.
    pub max_concurrent: u32,
    /// Budget limit in USD (if configured).
    pub budget_usd: Option<f64>,
    /// Budget limit in turns (if configured).
    pub budget_turns: Option<u32>,
}

/// A group of prompts in a dry-run plan.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[non_exhaustive]
pub struct DryRunGroup {
    /// Zero-based group index (execution order).
    pub group_index: usize,
    /// Prompts in this group.
    pub prompts: Vec<DryRunPrompt>,
}

/// Summary of a prompt in a dry-run plan.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[non_exhaustive]
pub struct DryRunPrompt {
    /// Prompt number.
    pub number: u32,
    /// Task description.
    pub description: String,
    /// Dependencies (prompt numbers).
    pub depends_on: Vec<u32>,
}

// ---------------------------------------------------------------------------
// EngineConfig extension
// ---------------------------------------------------------------------------

impl EngineConfig {
    /// Set the idle timeout from an `Option<Duration>`, no-op if `None`.
    #[must_use]
    pub(crate) fn idle_timeout_opt(mut self, timeout: Option<std::time::Duration>) -> Self {
        self.idle_timeout = timeout;
        self
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod orchestrator_tests;
