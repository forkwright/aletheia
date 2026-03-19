//! Dianoia-specific errors.

use std::path::PathBuf;

use snafu::Snafu;

/// Errors from planning and project orchestration.
#[derive(Debug, Snafu)]
#[snafu(visibility(pub))]
#[non_exhaustive]
pub enum Error {
    /// A state transition was attempted that is not valid from the current state.
    #[snafu(display("invalid transition {transition:?} from state {state:?}"))]
    InvalidTransition {
        /// The current project state.
        state: crate::state::ProjectState,
        /// The transition that was attempted.
        transition: crate::state::Transition,
        #[snafu(implicit)]
        /// Source location captured by snafu.
        location: snafu::Location,
    },

    /// Requested phase does not exist in the project.
    #[snafu(display("phase not found: {phase_id}"))]
    PhaseNotFound {
        /// The phase identifier that was not found.
        phase_id: String,
        #[snafu(implicit)]
        /// Source location captured by snafu.
        location: snafu::Location,
    },

    /// Requested plan does not exist in the phase.
    #[snafu(display("plan not found: {plan_id}"))]
    PlanNotFound {
        /// The plan identifier that was not found.
        plan_id: String,
        #[snafu(implicit)]
        /// Source location captured by snafu.
        location: snafu::Location,
    },

    /// A plan exceeded its maximum iteration count without completing.
    #[snafu(display("plan {plan_id} stuck after {iterations} iterations"))]
    PlanStuck {
        /// The plan identifier that got stuck.
        plan_id: String,
        /// Number of iterations completed before the limit was hit.
        iterations: u32,
        #[snafu(implicit)]
        /// Source location captured by snafu.
        location: snafu::Location,
    },

    /// Filesystem operation failed in the project workspace.
    #[snafu(display("workspace I/O error at {}", path.display()))]
    WorkspaceIo {
        /// The path at which the I/O error occurred.
        path: PathBuf,
        /// The underlying I/O error.
        source: std::io::Error,
        #[snafu(implicit)]
        /// Source location captured by snafu.
        location: snafu::Location,
    },

    /// Failed to deserialize a project file from JSON.
    #[snafu(display("workspace deserialization error"))]
    WorkspaceDeserialize {
        /// The underlying deserialization error.
        source: serde_json::Error,
        #[snafu(implicit)]
        /// Source location captured by snafu.
        location: snafu::Location,
    },

    /// Failed to serialize a project to JSON.
    #[snafu(display("workspace serialization error"))]
    WorkspaceSerialize {
        /// The underlying serialization error.
        source: serde_json::Error,
        #[snafu(implicit)]
        /// Source location captured by snafu.
        location: snafu::Location,
    },

    /// No project file exists at the expected workspace path.
    #[snafu(display("project not found in workspace at {}", path.display()))]
    ProjectNotFound {
        /// The path at which the project file was expected.
        path: PathBuf,
        #[snafu(implicit)]
        /// Source location captured by snafu.
        location: snafu::Location,
    },
}

/// Convenience alias for `Result` with dianoia's [`Error`] type.
pub type Result<T> = std::result::Result<T, Error>;
