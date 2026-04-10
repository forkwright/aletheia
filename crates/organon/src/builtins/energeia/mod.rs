//! Energeia capability tool implementations.
//!
//! Wires the 9 energeia agent tools to real subsystem calls:
//! - dromeus → Orchestrator::dispatch / dry_run
//! - dokimasia → qa::run_qa
//! - diorthosis → qa::corrective::generate_corrective
//! - epitropos → steward::service::run_once
//! - parateresis → EnergeiaStore observation pipeline
//! - mathesis → EnergeiaStore::query_lessons / add_lesson
//! - prographe → prompt template rendering
//! - schedion → PromptDag + compute_frontier
//! - metron → MetricsService health / cost / velocity

mod dispatch;
mod metrics;
mod observation;
mod planning;
mod qa;
mod shared;
mod steward;

pub use shared::EnergeiaServices;

use std::sync::Arc;

use crate::error::Result;
use crate::registry::ToolRegistry;

// ── registration ───────────────────────────────────────────────────────────

/// Register all 9 energeia tools with real implementations.
///
/// When `services` is `Some`, tools that need the orchestrator or store call
/// through to the real energeia subsystem. When `None`, those tools return a
/// structured error indicating the missing dependency — they do not panic.
///
/// Tools that are pure computation (schedion, prographe, diorthosis,
/// dokimasia, epitropos) work regardless of whether services are provided.
///
/// # Errors
///
/// Returns an error if any tool name collides with an already-registered tool.
pub fn register(
    registry: &mut ToolRegistry,
    services: Option<Arc<EnergeiaServices>>,
) -> Result<()> {
    let (orchestrator, store) = match &services {
        Some(svc) => (
            Some(Arc::clone(&svc.orchestrator)),
            Some(Arc::clone(&svc.store)),
        ),
        None => (None, None),
    };

    registry.register(
        dispatch::dromeus_def(),
        Box::new(dispatch::DromeusExecutor { orchestrator }),
    )?;
    registry.register(qa::dokimasia_def(), Box::new(qa::DokimasiaExecutor))?;
    registry.register(qa::diorthosis_def(), Box::new(qa::DiorthosisExecutor))?;
    registry.register(steward::epitropos_def(), Box::new(steward::EpitroposExecutor))?;
    registry.register(
        observation::parateresis_def(),
        Box::new(observation::ParateresisExecutor {
            store: store.clone(),
        }),
    )?;
    registry.register(
        observation::mathesis_def(),
        Box::new(observation::MathesisExecutor {
            store: store.clone(),
        }),
    )?;
    registry.register(planning::prographe_def(), Box::new(planning::ProographeExecutor))?;
    registry.register(planning::schedion_def(), Box::new(planning::SchedionExecutor))?;
    registry.register(metrics::metron_def(), Box::new(metrics::MetronExecutor { store }))?;
    Ok(())
}

#[cfg(test)]
mod tests {
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
