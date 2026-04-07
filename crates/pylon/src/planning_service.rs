//! Planning service: bridges pylon handlers to the dianoia verification engine.
//!
//! WHY: pylon's planning handlers were stubs (#2034). The dianoia engine has
//! `verify_phase()` and `trace_goals()` ready — this service connects them.

use std::path::PathBuf;
use aletheia_dianoia::phase::Phase;
use aletheia_dianoia::project::Project;
use aletheia_dianoia::verify::{CriterionInput, VerificationResult, verify_phase};
use aletheia_dianoia::workspace::ProjectWorkspace;
use tracing::info;

// ---------------------------------------------------------------------------
// PlanningService
// ---------------------------------------------------------------------------

/// Service that provides planning verification to pylon handlers.
///
/// Wraps a dianoia `ProjectWorkspace` and exposes verification operations
/// that handlers call with project/phase identifiers.
pub struct PlanningService {
    workspace_path: PathBuf,
}

impl PlanningService {
    /// Create a new planning service rooted at the given workspace path.
    #[must_use]
    pub fn new(workspace_path: PathBuf) -> Self {
        info!(path = %workspace_path.display(), "planning service initialized");
        Self { workspace_path }
    }

    /// Load the project from the workspace.
    ///
    /// # Errors
    ///
    /// Returns an error if the workspace or project can't be loaded.
    pub fn load_project(&self) -> Result<Project, String> {
        let workspace = ProjectWorkspace::open(&self.workspace_path)
            .map_err(|e| format!("failed to open workspace: {e}"))?;
        workspace
            .load_project()
            .map_err(|e| format!("failed to load project: {e}"))
    }

    /// Verify a specific phase of the project.
    ///
    /// Returns the dianoia `VerificationResult` directly — the handler
    /// maps it to the pylon response format.
    pub fn verify_phase_by_name(
        &self,
        phase_name: &str,
        inputs: &[CriterionInput],
    ) -> Result<VerificationResult, String> {
        let project = self.load_project()?;

        let phase = project
            .phases
            .iter()
            .find(|p| p.name == phase_name)
            .ok_or_else(|| format!("phase '{phase_name}' not found"))?;

        Ok(verify_phase(phase, inputs))
    }

    /// Get all phases for a project.
    pub fn list_phases(&self) -> Result<Vec<Phase>, String> {
        let project = self.load_project()?;
        Ok(project.phases)
    }
}
