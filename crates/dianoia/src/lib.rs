//! aletheia-dianoia — planning and project orchestration
//!
//! Dianoia (διάνοια) — "thinking through." The systematic, step-by-step
//! reasoning process. Manages multi-phase projects from research through
//! execution to verification. Three modes: full project, quick task,
//! autonomous background.
//!
//! Depends on: koina

pub mod error;
pub mod phase;
pub mod plan;
pub mod project;
pub mod state;
pub mod workspace;

#[cfg(test)]
mod assertions {
    use static_assertions::assert_impl_all;

    assert_impl_all!(crate::project::Project: Send, Sync);
    assert_impl_all!(crate::phase::Phase: Send, Sync);
    assert_impl_all!(crate::plan::Plan: Send, Sync);
    assert_impl_all!(crate::workspace::ProjectWorkspace: Send, Sync);
}
