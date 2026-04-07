#![deny(missing_docs)]
//! aletheia-dianoia: planning and project orchestration
//!
//! Dianoia (διάνοια): "thinking through." The systematic, step-by-step
//! reasoning process. Manages multi-phase projects from research through
//! execution to verification. Three modes: full project, quick task,
//! autonomous background.
//!
//! Depends on: koina

/// Cross-project attention allocation: priority-weighted fair scheduling.
pub mod attention;
/// Errors from planning, state transitions, and workspace I/O.
pub(crate) mod error;
/// Intent persistence with conviction tiers for sustained autonomous governance.
pub mod intent;
/// Context handoff protocol: continuity across distillation, shutdown, and crash recovery.
pub mod handoff;
/// Prometheus metric definitions for planning and project orchestration.
pub mod metrics;
/// Active project orchestrator: wave dispatch, outcome tracking, synthesis triggers.
pub mod orchestrate;
/// Phase boundary gates: conditions that must be met before advancing between phases.
pub mod gate;
/// Phase types within a project: groupings of related plans with lifecycle state.
pub mod phase;
/// Executable plans within a phase: dependency tracking, iteration limits, and blocker management.
pub mod plan;
/// Project types and lifecycle management: creation, phase tracking, and state transitions.
pub mod project;
/// State reconciler: keeps planning state consistent between database and filesystem.
pub mod reconciler;
/// Pattern-based stuck detection: repeated errors, same-args loops, alternating failures, escalating retries.
pub mod research;
/// Project lifecycle state machine: valid transitions, pause/resume, and terminal states.
pub mod state;
/// Stuck detection: prevent blind retry loops via error-pattern hashing.
pub mod stuck;
/// Verification workflow: goal-backward tracing against phase success criteria.
pub mod verify;
/// On-disk workspace persistence: project serialization, blocker files, and directory layout.
pub mod workspace;

#[cfg(test)]
mod assertions {
    use static_assertions::assert_impl_all;

    assert_impl_all!(crate::gate::GateCondition: Send, Sync);
    assert_impl_all!(crate::gate::GateResult: Send, Sync);
    assert_impl_all!(crate::gate::PhaseGate: Send, Sync);
    assert_impl_all!(crate::project::Project: Send, Sync);
    assert_impl_all!(crate::phase::Phase: Send, Sync);
    assert_impl_all!(crate::plan::Plan: Send, Sync);
    assert_impl_all!(crate::workspace::ProjectWorkspace: Send, Sync);
    assert_impl_all!(crate::stuck::StuckDetector: Send, Sync);
    assert_impl_all!(crate::handoff::HandoffFile: Send, Sync);
    assert_impl_all!(crate::handoff::HandoffContext: Send, Sync);
    assert_impl_all!(crate::verify::VerificationResult: Send, Sync);
    assert_impl_all!(crate::reconciler::ReconciliationResult: Send, Sync);
    assert_impl_all!(crate::reconciler::ReconciliationSummary: Send, Sync);
}
