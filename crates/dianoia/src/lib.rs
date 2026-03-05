//! aletheia-dianoia — planning and project orchestration
//!
//! Dianoia (διάνοια) — "thinking through." The systematic, step-by-step
//! reasoning process. Manages multi-phase projects from research through
//! execution to verification. Three modes: full project, quick task,
//! autonomous background.
//!
//! Depends on: koina

/// Errors from planning, state transitions, and workspace I/O.
pub mod error;
/// Phase types within a project: groupings of related plans with lifecycle state.
pub mod phase;
/// Executable plans within a phase: dependency tracking, iteration limits, and blocker management.
pub mod plan;
/// Project types and lifecycle management: creation, phase tracking, and state transitions.
pub mod project;
/// Project lifecycle state machine: valid transitions, pause/resume, and terminal states.
pub mod state;
/// On-disk workspace persistence: project serialization, blocker files, and directory layout.
pub mod workspace;

#[cfg(test)]
mod assertions {
    use static_assertions::assert_impl_all;

    assert_impl_all!(crate::project::Project: Send, Sync);
    assert_impl_all!(crate::phase::Phase: Send, Sync);
    assert_impl_all!(crate::plan::Plan: Send, Sync);
    assert_impl_all!(crate::workspace::ProjectWorkspace: Send, Sync);
}
