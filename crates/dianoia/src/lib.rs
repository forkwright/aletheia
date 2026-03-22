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
/// Context handoff protocol: continuity across distillation, shutdown, and crash recovery.
pub mod handoff;
/// Phase types within a project: groupings of related plans with lifecycle state.
pub mod phase;
/// Executable plans within a phase: dependency tracking, iteration limits, and blocker management.
pub mod plan;
/// Project types and lifecycle management: creation, phase tracking, and state transitions.
pub mod project;
/// State reconciler: keeps planning state consistent between database and filesystem.
pub mod reconciler;
/// Project lifecycle state machine: valid transitions, pause/resume, and terminal states.
pub mod state;
/// Pattern-based stuck detection: repeated errors, same-args loops, alternating failures, escalating retries.
pub mod research;
/// Stuck detection: prevent blind retry loops via error-pattern hashing.
pub mod stuck;
/// Verification workflow: goal-backward tracing against phase success criteria.
pub mod verify;
/// On-disk workspace persistence: project serialization, blocker files, and directory layout.
pub mod workspace;

#[cfg(test)]
mod assertions {
    use static_assertions::assert_impl_all;

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
