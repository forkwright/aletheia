#![deny(missing_docs)]
//! aletheia-dianoia: planning and project orchestration
//!
//! Dianoia (διάνοια): "thinking through." The systematic, step-by-step
//! reasoning process. Manages multi-phase projects from research through
//! execution to verification. Three modes: full project, quick task,
//! autonomous background.
//!
//! Depends on: koina

/// Errors from planning, state transitions, and workspace I/O.
pub(crate) mod error;
/// Phase boundary gates: conditions that must be met before advancing between phases.
pub mod gate;
/// Context handoff protocol: continuity across distillation, shutdown, and crash recovery.
pub mod handoff;
/// Intent persistence with conviction tiers for sustained autonomous governance.
pub mod intent;
/// Prometheus metric definitions for planning and project orchestration.
pub mod metrics;
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
    const _: fn() = || {
        fn assert<T: Send + Sync>() {}
        assert::<crate::gate::GateCondition>();
        assert::<crate::gate::GateResult>();
        assert::<crate::gate::PhaseGate>();
        assert::<crate::project::Project>();
        assert::<crate::phase::Phase>();
        assert::<crate::plan::Plan>();
        assert::<crate::workspace::ProjectWorkspace>();
        assert::<crate::stuck::StuckDetector>();
        assert::<crate::handoff::HandoffFile>();
        assert::<crate::handoff::HandoffContext>();
        assert::<crate::verify::VerificationResult>();
        assert::<crate::reconciler::ReconciliationResult>();
        assert::<crate::reconciler::ReconciliationSummary>();
    };
}
