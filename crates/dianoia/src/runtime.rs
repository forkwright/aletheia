//! Runtime surface for project planning orchestration.

use crate::phase::{Phase, PhaseState};
use crate::project::Project;
use crate::reconciler::{ProjectSnapshot, ReconciliationSummary, reconcile_all};
use crate::stuck::{StuckConfig, StuckDetector, StuckSignal, ToolInvocation};

/// Coordinates project lifecycle state with runtime planning signals.
#[derive(Debug)]
pub struct PlanningRuntime {
    project: Project,
    stuck_detector: StuckDetector,
}

impl PlanningRuntime {
    /// Create a runtime for an existing project and stuck-detection configuration.
    #[must_use]
    pub fn new(project: Project, stuck_config: StuckConfig) -> Self {
        Self {
            project,
            stuck_detector: StuckDetector::new(stuck_config),
        }
    }

    /// Borrow the current project state.
    #[must_use]
    pub fn project(&self) -> &Project {
        &self.project
    }

    /// Mutably borrow the current project state.
    pub fn project_mut(&mut self) -> &mut Project {
        &mut self.project
    }

    /// Borrow the first non-complete phase, if any.
    #[must_use]
    pub fn active_phase(&self) -> Option<&Phase> {
        self.project.active_phase()
    }

    /// Mark the first non-complete phase active and return it.
    pub fn start_active_phase(&mut self) -> Option<&mut Phase> {
        let phase = self.project.active_phase_mut()?;
        phase.state = PhaseState::Active;
        Some(phase)
    }

    /// Record a tool invocation and return a stuck signal when a loop is detected.
    pub fn record_tool_invocation(&mut self, invocation: ToolInvocation) -> Option<StuckSignal> {
        self.stuck_detector.record(invocation)
    }

    /// Clear accumulated stuck-detection history.
    pub fn reset_stuck_detection(&mut self) {
        self.stuck_detector.reset();
    }

    /// Number of invocations currently retained for stuck detection.
    #[must_use]
    pub fn stuck_history_len(&self) -> usize {
        self.stuck_detector.history_len()
    }

    /// Active stuck-detection configuration.
    #[must_use]
    pub fn stuck_config(&self) -> &StuckConfig {
        self.stuck_detector.config()
    }

    /// Reconcile database and filesystem snapshots using the dianoia reconciliation engine.
    #[must_use]
    pub fn reconcile_snapshots(
        db_snapshots: &[ProjectSnapshot],
        fs_snapshots: &[ProjectSnapshot],
        tolerance_secs: i64,
    ) -> ReconciliationSummary {
        reconcile_all(db_snapshots, fs_snapshots, tolerance_secs)
    }
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
mod tests {
    use super::*;
    use crate::phase::Phase;
    use crate::project::{Project, ProjectMode};
    use crate::reconciler::{
        DEFAULT_TIMESTAMP_TOLERANCE_SECS, ReconciliationDirection, SnapshotOrigin,
    };
    use crate::state::{ProjectState, Transition};
    use crate::stuck::{InvocationOutcome, StuckPattern};

    fn invocation(tool_name: &str, arguments: &str, message: &str) -> ToolInvocation {
        ToolInvocation {
            tool_name: tool_name.to_owned(),
            arguments: arguments.to_owned(),
            outcome: InvocationOutcome::Error {
                message: message.to_owned(),
            },
            recorded_at: jiff::Timestamp::now(),
        }
    }

    #[test]
    fn runtime_drives_project_phase_stuck_and_reconciliation_lifecycle() {
        let mut project = Project::new(
            "release".to_owned(),
            "ship a release".to_owned(),
            ProjectMode::Full,
            "alice".to_owned(),
        );
        project.add_phase(Phase::new(
            "Foundation".to_owned(),
            "prepare release inputs".to_owned(),
            1,
        ));

        let config = StuckConfig {
            repeated_error_threshold: 2,
            history_window: 4,
            ..StuckConfig::default()
        };
        let mut runtime = PlanningRuntime::new(project, config);

        runtime
            .project_mut()
            .advance(Transition::StartQuestioning)
            .unwrap();
        runtime
            .project_mut()
            .advance(Transition::StartResearch)
            .unwrap();
        runtime
            .project_mut()
            .advance(Transition::StartScoping)
            .unwrap();
        runtime
            .project_mut()
            .advance(Transition::StartPlanning)
            .unwrap();
        assert_eq!(runtime.project().state, ProjectState::Planning);

        let phase = runtime.start_active_phase().unwrap();
        assert_eq!(phase.state, PhaseState::Active);
        assert_eq!(runtime.active_phase().unwrap().name, "Foundation");

        assert!(
            runtime
                .record_tool_invocation(invocation("build", "{}", "timeout"))
                .is_none()
        );
        let signal = runtime
            .record_tool_invocation(invocation("build", "{}", "timeout"))
            .unwrap();
        assert!(matches!(
            signal.pattern,
            StuckPattern::RepeatedError { count: 2, .. }
        ));
        assert_eq!(runtime.stuck_history_len(), 2);
        assert_eq!(runtime.stuck_config().history_window, 4);

        runtime.reset_stuck_detection();
        assert_eq!(runtime.stuck_history_len(), 0);

        let db = ProjectSnapshot {
            project: runtime.project().clone(),
            origin: SnapshotOrigin::Database,
        };
        let fs = ProjectSnapshot {
            project: runtime.project().clone(),
            origin: SnapshotOrigin::Filesystem,
        };
        let summary =
            PlanningRuntime::reconcile_snapshots(&[db], &[fs], DEFAULT_TIMESTAMP_TOLERANCE_SECS);

        assert_eq!(summary.projects.len(), 1);
        let result = summary.projects.first().unwrap();
        assert_eq!(result.direction, ReconciliationDirection::InSync);
        assert_eq!(summary.total_errors, 0);
    }
}
