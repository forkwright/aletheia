//! Energeia capability tool implementations.
//!
//! Wires the 9 energeia agent tools to the currently available subsystem calls:
//! - dromeus → Orchestrator::dispatch / dry_run
//! - dokimasia → qa::run_qa mechanical checks on caller-provided diffs
//! - diorthosis → qa::corrective::generate_corrective
//! - epitropos → steward::service::run_once against GithubStewardBackend
//!   (conservative until a diff-fetching backend capability lands -- see
//!   steward::service's classify_pr doc comment)
//! - parateresis → EnergeiaStore observation pipeline
//! - mathesis → EnergeiaStore::query_lessons / add_lesson
//! - prographe → prompt template rendering, not queue allocation or file writes
//! - schedion → empty PromptDag + compute_frontier until a prompt path is supplied
//! - metron → MetricsService health / cost / velocity / status

mod dispatch;
mod metrics;
mod observation;
mod planning;
mod qa;
mod shared;
mod steward;

pub use shared::EnergeiaServices;

use std::sync::Arc;

use koina::id::ToolName;

use crate::error::Result;
use crate::registry::ToolRegistry;
use crate::types::{RollbackSupport, ToolCapabilityMetadata, ToolStability};

/// Every tool in this module is `Experimental`: the whole `energeia` module
/// is behind `#[cfg(feature = "energeia")]` (see
/// crates/organon/src/builtins/mod.rs) -- not compiled by default.
const ENERGEIA_STABILITY: ToolStability = ToolStability::Experimental;

// ── registration ───────────────────────────────────────────────────────────

/// Register all 9 energeia tools.
///
/// When `services` is `Some`, tools that need the orchestrator or store call
/// through to the real energeia subsystem. When `None`, those tools return a
/// structured error indicating the missing dependency — they do not panic.
///
/// Tools that are bounded local computations (`schedion`, `prographe`,
/// `diorthosis`, `dokimasia`, `epitropos`) work regardless of whether services
/// are provided, but their public definitions describe the current limitations.
///
/// # Errors
///
/// Returns an error if any tool name collides with an already-registered tool.
pub fn register(registry: &mut ToolRegistry, services: Option<&EnergeiaServices>) -> Result<()> {
    let (orchestrator, store) = match services {
        Some(svc) => (
            Some(Arc::clone(&svc.orchestrator)),
            Some(Arc::clone(&svc.store)),
        ),
        None => (None, None),
    };
    let (cron_lock_store, cron_task_names) = match services {
        Some(svc) => (svc.cron_lock_store.clone(), svc.cron_task_names.clone()),
        None => (None, Vec::new()),
    };

    registry.register(
        dispatch::dromeus_def(),
        Box::new(dispatch::DromeusExecutor { orchestrator }),
    )?;
    registry.declare_capability(
        ToolName::from_static("dromeus"),
        ToolCapabilityMetadata {
            owner: "organon::builtins::energeia::dispatch".to_owned(),
            stability: ENERGEIA_STABILITY,
            rollback: RollbackSupport::Unsupported {
                reason: "spawns and orchestrates agent sessions per prompt group; their effects \
                         are not tracked for rollback by this tool"
                    .to_owned(),
            },
            ..ToolCapabilityMetadata::default()
        },
    );
    registry.register(qa::dokimasia_def(), Box::new(qa::DokimasiaExecutor))?;
    registry.declare_capability(
        ToolName::from_static("dokimasia"),
        ToolCapabilityMetadata {
            owner: "organon::builtins::energeia::qa".to_owned(),
            stability: ENERGEIA_STABILITY,
            rollback: RollbackSupport::Unsupported {
                reason: "best-effort lesson persistence to the knowledge graph on a Pass/\
                         NeedsReview verdict has no delete/rollback path"
                    .to_owned(),
            },
            ..ToolCapabilityMetadata::default()
        },
    );
    registry.register(qa::diorthosis_def(), Box::new(qa::DiorthosisExecutor))?;
    registry.register(
        steward::epitropos_def(),
        Box::new(steward::EpitroposExecutor),
    )?;
    registry.declare_capability(
        ToolName::from_static("epitropos"),
        ToolCapabilityMetadata {
            owner: "organon::builtins::energeia::steward".to_owned(),
            stability: ENERGEIA_STABILITY,
            rollback: RollbackSupport::Unsupported {
                reason: "acts against the live GitHub REST API; effects occur on an external \
                         system outside aletheia's control"
                    .to_owned(),
            },
            ..ToolCapabilityMetadata::default()
        },
    );
    registry.register(
        observation::parateresis_def(),
        Box::new(observation::ParateresisExecutor {
            store: store.clone(),
        }),
    )?;
    registry.declare_capability(
        ToolName::from_static("parateresis"),
        ToolCapabilityMetadata {
            owner: "organon::builtins::energeia::observation".to_owned(),
            stability: ENERGEIA_STABILITY,
            rollback: RollbackSupport::Unsupported {
                reason: "appends a sentinel query-observation record to the energeia store; no \
                         delete/rollback path exists for energeia store writes"
                    .to_owned(),
            },
            ..ToolCapabilityMetadata::default()
        },
    );
    registry.register(
        observation::mathesis_def(),
        Box::new(observation::MathesisExecutor {
            store: store.clone(),
        }),
    )?;
    registry.register(
        planning::prographe_def(),
        Box::new(planning::ProographeExecutor),
    )?;
    registry.register(
        planning::schedion_def(),
        Box::new(planning::SchedionExecutor),
    )?;
    registry.register(
        metrics::metron_def(),
        Box::new(metrics::MetronExecutor {
            store,
            cron_lock_store,
            cron_task_names,
        }),
    )?;
    Ok(())
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    #![expect(clippy::expect_used, reason = "test assertions")]
    use super::*;
    use crate::registry::ToolRegistry;
    use crate::types::ToolCategory;

    #[test]
    fn all_nine_tools_register_without_collision() {
        let mut registry = ToolRegistry::new();
        register(&mut registry, None).expect("energeia tools registered without collision");
        let defs = registry.definitions();
        assert_eq!(defs.len(), 9, "expected 9 energeia tools registered");
    }

    #[test]
    fn tool_categories_match_design() {
        for def in [
            dispatch::dromeus_def(),
            qa::dokimasia_def(),
            qa::diorthosis_def(),
            steward::epitropos_def(),
            observation::parateresis_def(),
        ] {
            assert_eq!(
                def.category,
                ToolCategory::Agent,
                "{} must be in Agent category",
                def.name
            );
        }
        assert_eq!(observation::mathesis_def().category, ToolCategory::Memory);
        assert_eq!(planning::prographe_def().category, ToolCategory::Planning);
        assert_eq!(planning::schedion_def().category, ToolCategory::Planning);
        assert_eq!(metrics::metron_def().category, ToolCategory::System);
    }

    #[test]
    fn no_tools_auto_activate() {
        for def in [
            dispatch::dromeus_def(),
            qa::dokimasia_def(),
            qa::diorthosis_def(),
            steward::epitropos_def(),
            observation::parateresis_def(),
            observation::mathesis_def(),
            planning::prographe_def(),
            planning::schedion_def(),
            metrics::metron_def(),
        ] {
            assert!(!def.auto_activate, "{} must not auto-activate", def.name);
        }
    }
}
