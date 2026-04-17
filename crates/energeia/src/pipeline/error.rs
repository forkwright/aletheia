// WHY: Stage-identified errors let callers (and logs) know exactly which
// pipeline stage failed without attaching a debugger or reading a backtrace.
// Wraps the underlying orchestration error and annotates it with the stage
// name so failure attribution is immediate.

use snafu::Snafu;

use crate::error::Error as OrchestratorError;

/// A pipeline stage error: the underlying error annotated with the name of
/// the stage that produced it.
#[derive(Debug, Snafu)]
#[snafu(visibility(pub(crate)))]
#[non_exhaustive]
pub(crate) enum PipelineError {
    /// An error that occurred during a named pipeline stage.
    #[snafu(display("[{stage}] {source}"))]
    Stage {
        /// Name of the stage that failed (matches [`PipelineStage::name`]).
        stage: &'static str,
        /// Underlying orchestration error.
        source: OrchestratorError,
        #[snafu(implicit)]
        location: snafu::Location,
    },
}

impl PipelineError {
    /// The name of the stage that produced this error.
    #[cfg(test)]
    pub(crate) fn stage(&self) -> &'static str {
        match self {
            Self::Stage { stage, .. } => stage,
        }
    }
}

impl From<PipelineError> for OrchestratorError {
    fn from(e: PipelineError) -> Self {
        // WHY: The underlying source error carries full context (variant, location).
        // We surface it directly so callers using Result<_, Error> don't lose
        // detail when crossing the pipeline boundary.
        match e {
            PipelineError::Stage { source, .. } => source,
        }
    }
}

#[cfg(test)]
mod tests {
    use snafu::IntoError as _;

    use super::*;
    use crate::error::PreflightSnafu;

    #[test]
    fn stage_error_includes_stage_name_in_display() {
        let inner = PreflightSnafu {
            reason: "no prompts",
        }
        .build();
        let err = StageSnafu {
            stage: "preparation",
        }
        .into_error(inner);

        let msg = err.to_string();
        assert!(msg.contains("preparation"), "missing stage: {msg}");
        assert!(msg.contains("no prompts"), "missing inner: {msg}");
    }

    #[test]
    fn stage_accessor_returns_stage_name() {
        let inner = PreflightSnafu { reason: "test" }.build();
        let err = StageSnafu { stage: "execution" }.into_error(inner);
        assert_eq!(err.stage(), "execution");
    }

    #[test]
    fn into_orchestrator_error_preserves_variant() {
        let inner = PreflightSnafu {
            reason: "test conversion",
        }
        .build();
        let pipeline_err = StageSnafu {
            stage: "preparation",
        }
        .into_error(inner);
        let orch_err: OrchestratorError = pipeline_err.into();
        assert!(orch_err.to_string().contains("test conversion"));
    }
}
