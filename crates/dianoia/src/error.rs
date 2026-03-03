//! Dianoia-specific errors.

use std::path::PathBuf;

use snafu::Snafu;

/// Errors from planning and project orchestration.
#[derive(Debug, Snafu)]
#[snafu(visibility(pub))]
#[non_exhaustive]
pub enum Error {
    #[snafu(display("invalid transition {transition:?} from state {state:?}"))]
    InvalidTransition {
        state: crate::state::ProjectState,
        transition: crate::state::Transition,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    #[snafu(display("phase not found: {phase_id}"))]
    PhaseNotFound {
        phase_id: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    #[snafu(display("plan not found: {plan_id}"))]
    PlanNotFound {
        plan_id: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    #[snafu(display("plan {plan_id} stuck after {iterations} iterations"))]
    PlanStuck {
        plan_id: String,
        iterations: u32,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    #[snafu(display("workspace I/O error at {}", path.display()))]
    WorkspaceIo {
        path: PathBuf,
        source: std::io::Error,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    #[snafu(display("workspace deserialization error"))]
    WorkspaceDeserialize {
        source: serde_json::Error,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    #[snafu(display("workspace serialization error"))]
    WorkspaceSerialize {
        source: serde_json::Error,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    #[snafu(display("project not found in workspace at {}", path.display()))]
    ProjectNotFound {
        path: PathBuf,
        #[snafu(implicit)]
        location: snafu::Location,
    },
}

pub type Result<T> = std::result::Result<T, Error>;
