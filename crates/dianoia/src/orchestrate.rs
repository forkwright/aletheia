//! Active project orchestrator: the conductor that drives execution.
//!
//! The orchestrator sits on top of dianoia's state machine foundation and
//! provides the dynamic layer: analyze phases, slice into plans, allocate
//! resources, dispatch plans in wave order, and synthesize results.
//!
//! WHY: The state machine (project.rs, state.rs) manages *structure* —
//! valid transitions, persistence, gates. The orchestrator manages *execution*
//! — deciding what to do next, tracking progress, and triggering synthesis.

use serde::{Deserialize, Serialize};
use ulid::Ulid;

use crate::error::{self, Result};
use crate::intent::IntentStore;
use crate::phase::{Phase, PhaseState};
use crate::plan::{Plan, PlanState};
use crate::project::Project;
use crate::state::ProjectState;
use crate::stuck::StuckDetector;
use crate::workspace::ProjectWorkspace;

// ---------------------------------------------------------------------------
// Orchestrator
// ---------------------------------------------------------------------------

/// Active execution coordinator for a project.
///
/// Manages the dispatch lifecycle: which phase is active, which plans are
/// ready, what resources they need, and when to synthesize results. Does not
/// own execution — it produces [`Directive`]s that callers act on.
pub struct Orchestrator {
    project: Project,
    workspace: ProjectWorkspace,
    intent_store: IntentStore,
    stuck_detector: StuckDetector,
}

impl Orchestrator {
    /// Create an orchestrator for the given project.
    pub fn new(
        project: Project,
        workspace: ProjectWorkspace,
        intent_store: IntentStore,
        stuck_detector: StuckDetector,
    ) -> Self {
        Self {
            project,
            workspace,
            intent_store,
            stuck_detector,
        }
    }

    /// Immutable access to the managed project.
    #[must_use]
    pub fn project(&self) -> &Project {
        &self.project
    }

    /// The active phase, if any.
    #[must_use]
    pub fn active_phase(&self) -> Option<&Phase> {
        self.project
            .phases
            .iter()
            .find(|p| p.state == PhaseState::Active || p.state == PhaseState::Executing)
    }

    /// Compute the next set of directives the caller should act on.
    ///
    /// Returns an empty vec when the project is complete, abandoned, or paused.
    pub fn next_directives(&self) -> Vec<Directive> {
        let mut directives = Vec::new();

        let Some(phase) = self.active_phase() else {
            return directives;
        };

        // Collect plans whose dependencies are satisfied.
        let ready_plans = self.ready_plans(phase);

        for plan in &ready_plans {
            directives.push(Directive::DispatchPlan {
                phase_id: phase.id,
                plan_id: plan.id,
                title: plan.title.clone(),
                wave: plan.wave,
            });
        }

        // Check if all plans in the current phase are terminal.
        let all_terminal = phase.plans.iter().all(|p| {
            matches!(
                p.state,
                PlanState::Complete | PlanState::Failed | PlanState::Skipped | PlanState::Stuck
            )
        });

        if all_terminal && ready_plans.is_empty() {
            let all_complete = phase
                .plans
                .iter()
                .all(|p| matches!(p.state, PlanState::Complete | PlanState::Skipped));

            if all_complete {
                directives.push(Directive::VerifyPhase {
                    phase_id: phase.id,
                });
            } else {
                directives.push(Directive::PhaseBlocked {
                    phase_id: phase.id,
                    failed_plans: phase
                        .plans
                        .iter()
                        .filter(|p| {
                            matches!(p.state, PlanState::Failed | PlanState::Stuck)
                        })
                        .map(|p| p.id)
                        .collect(),
                });
            }
        }

        // Check if research synthesis is warranted.
        if self.should_synthesize() {
            directives.push(Directive::SynthesizeResearch {
                phase_id: phase.id,
            });
        }

        directives
    }

    /// Plans in the active phase whose dependencies are all satisfied.
    fn ready_plans<'a>(&self, phase: &'a Phase) -> Vec<&'a Plan> {
        let complete_ids: std::collections::HashSet<Ulid> = phase
            .plans
            .iter()
            .filter(|p| matches!(p.state, PlanState::Complete | PlanState::Skipped))
            .map(|p| p.id)
            .collect();

        phase
            .plans
            .iter()
            .filter(|p| p.state == PlanState::Pending || p.state == PlanState::Ready)
            .filter(|p| p.depends_on.iter().all(|dep| complete_ids.contains(dep)))
            .collect()
    }

    /// Record a plan outcome and update project state.
    ///
    /// # Errors
    ///
    /// Returns an error if the plan ID is not found or the workspace fails to persist.
    pub fn record_plan_outcome(
        &mut self,
        plan_id: Ulid,
        outcome: PlanOutcome,
    ) -> Result<()> {
        let phase = self
            .project
            .phases
            .iter_mut()
            .find(|ph| ph.plans.iter().any(|p| p.id == plan_id))
            .ok_or_else(|| {
                error::PlanNotFoundSnafu {
                    plan_id: plan_id.to_string(),
                }
                .build()
            })?;

        let plan = phase
            .plans
            .iter_mut()
            .find(|p| p.id == plan_id)
            .ok_or_else(|| {
                error::PlanNotFoundSnafu {
                    plan_id: plan_id.to_string(),
                }
                .build()
            })?;

        match outcome {
            PlanOutcome::Success { achievements } => {
                plan.state = PlanState::Complete;
                plan.achievements.extend(achievements);
            }
            PlanOutcome::Failed { reason } => {
                plan.iterations += 1;
                if plan.iterations >= plan.max_iterations {
                    plan.state = PlanState::Stuck;
                } else {
                    plan.state = PlanState::Failed;
                }
                plan.blockers.push(crate::plan::Blocker {
                    description: reason,
                    plan_id,
                    detected_at: jiff::Timestamp::now(),
                });
            }
            PlanOutcome::Skipped { reason } => {
                plan.state = PlanState::Skipped;
                plan.achievements.push(format!("Skipped: {reason}"));
            }
        }

        self.workspace.save_project(&self.project)
    }

    /// Whether research synthesis should be triggered.
    ///
    /// Returns true when the project is in a research-bearing state and
    /// active intents indicate synthesis is useful.
    fn should_synthesize(&self) -> bool {
        use crate::state::ProjectState;
        matches!(
            self.project.state,
            ProjectState::Researching | ProjectState::Scoping
        )
    }

    /// Access the intent store for reading standing orders.
    #[must_use]
    pub fn intents(&self) -> &IntentStore {
        &self.intent_store
    }

    /// Access the stuck detector for recording tool invocations.
    #[must_use]
    pub fn stuck_detector(&self) -> &StuckDetector {
        &self.stuck_detector
    }

    /// Mutable access to the stuck detector.
    pub fn stuck_detector_mut(&mut self) -> &mut StuckDetector {
        &mut self.stuck_detector
    }
}

// ---------------------------------------------------------------------------
// Directive
// ---------------------------------------------------------------------------

/// An action the orchestrator recommends the caller take.
///
/// Directives are *recommendations*, not commands. The caller (KAIROS daemon,
/// interactive operator, or dispatch system) decides how to fulfill them.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub enum Directive {
    /// Dispatch a plan for execution.
    DispatchPlan {
        /// Phase this plan belongs to.
        phase_id: Ulid,
        /// Plan to dispatch.
        plan_id: Ulid,
        /// Human-readable title for logging.
        title: String,
        /// Wave number for parallel grouping.
        wave: u32,
    },
    /// All plans complete — verify phase success criteria.
    VerifyPhase {
        /// Phase to verify.
        phase_id: Ulid,
    },
    /// Phase has failed/stuck plans — escalate to operator.
    PhaseBlocked {
        /// Phase that is blocked.
        phase_id: Ulid,
        /// Plans that failed or got stuck.
        failed_plans: Vec<Ulid>,
    },
    /// Research phase data is ready for synthesis.
    SynthesizeResearch {
        /// Phase whose research to synthesize.
        phase_id: Ulid,
    },
}

// ---------------------------------------------------------------------------
// PlanOutcome
// ---------------------------------------------------------------------------

/// Outcome reported after a plan executes.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub enum PlanOutcome {
    /// Plan completed successfully.
    Success {
        /// Notable things accomplished.
        achievements: Vec<String>,
    },
    /// Plan failed to complete.
    Failed {
        /// What went wrong.
        reason: String,
    },
    /// Plan was intentionally skipped.
    Skipped {
        /// Why it was skipped.
        reason: String,
    },
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
mod tests {
    use super::*;

    fn test_phase(plans: Vec<Plan>) -> Phase {
        Phase {
            id: Ulid::new(),
            name: "test phase".to_owned(),
            goal: "test goal".to_owned(),
            requirements: vec![],
            plans,
            state: PhaseState::Executing,
            order: 0,
        }
    }

    fn test_plan(title: &str, wave: u32, depends_on: Vec<Ulid>) -> Plan {
        let mut plan = Plan::new(title.to_owned(), "test".to_owned(), wave);
        plan.depends_on = depends_on;
        plan
    }

    fn test_project(phases: Vec<Phase>) -> Project {
        let mut p = Project::new(
            "test".to_owned(),
            "test project".to_owned(),
            crate::project::ProjectMode::Full,
            "operator".to_owned(),
        );
        p.phases = phases;
        p.state = crate::state::ProjectState::Executing;
        p
    }

    fn test_orchestrator(project: Project) -> Orchestrator {
        let workspace = ProjectWorkspace::create(
            std::env::temp_dir().join(format!("dianoia-test-{}", Ulid::new())),
        )
        .unwrap();
        Orchestrator::new(
            project,
            workspace,
            IntentStore::new(std::path::PathBuf::from("/tmp/test-intents.json")),
            StuckDetector::new(crate::stuck::StuckConfig::default()),
        )
    }

    #[test]
    fn ready_plans_with_no_dependencies() {
        let p1 = test_plan("plan-a", 0, vec![]);
        let p2 = test_plan("plan-b", 0, vec![]);
        let phase = test_phase(vec![p1, p2]);
        let project = test_project(vec![phase]);
        let orch = test_orchestrator(project);

        let directives = orch.next_directives();
        let dispatches: Vec<_> = directives
            .iter()
            .filter(|d| matches!(d, Directive::DispatchPlan { .. }))
            .collect();
        assert_eq!(dispatches.len(), 2);
    }

    #[test]
    fn dependent_plan_waits_for_predecessor() {
        let p1 = test_plan("plan-a", 0, vec![]);
        let dep_id = p1.id;
        let p2 = test_plan("plan-b", 1, vec![dep_id]);
        let phase = test_phase(vec![p1, p2]);
        let project = test_project(vec![phase]);
        let orch = test_orchestrator(project);

        let directives = orch.next_directives();
        let dispatches: Vec<_> = directives
            .iter()
            .filter(|d| matches!(d, Directive::DispatchPlan { .. }))
            .collect();
        // Only plan-a should be ready (plan-b depends on plan-a).
        assert_eq!(dispatches.len(), 1);
    }

    #[test]
    fn all_complete_triggers_verify() {
        let mut p1 = test_plan("plan-a", 0, vec![]);
        p1.state = PlanState::Complete;
        let phase = test_phase(vec![p1]);
        let project = test_project(vec![phase]);
        let orch = test_orchestrator(project);

        let directives = orch.next_directives();
        assert!(directives
            .iter()
            .any(|d| matches!(d, Directive::VerifyPhase { .. })));
    }

    #[test]
    fn failed_plan_triggers_blocked() {
        let mut p1 = test_plan("plan-a", 0, vec![]);
        p1.state = PlanState::Failed;
        let phase = test_phase(vec![p1]);
        let project = test_project(vec![phase]);
        let orch = test_orchestrator(project);

        let directives = orch.next_directives();
        assert!(directives
            .iter()
            .any(|d| matches!(d, Directive::PhaseBlocked { .. })));
    }

    #[test]
    fn record_success_transitions_plan() {
        let p1 = test_plan("plan-a", 0, vec![]);
        let plan_id = p1.id;
        let phase = test_phase(vec![p1]);
        let project = test_project(vec![phase]);
        let mut orch = test_orchestrator(project);

        orch.record_plan_outcome(
            plan_id,
            PlanOutcome::Success {
                achievements: vec!["did the thing".to_owned()],
            },
        )
        .unwrap();

        let plan = orch
            .project
            .phases[0]
            .plans
            .iter()
            .find(|p| p.id == plan_id)
            .unwrap();
        assert_eq!(plan.state, PlanState::Complete);
        assert_eq!(plan.achievements.len(), 1);
    }

    #[test]
    fn record_failure_increments_iterations() {
        let p1 = test_plan("plan-a", 0, vec![]);
        let plan_id = p1.id;
        let phase = test_phase(vec![p1]);
        let project = test_project(vec![phase]);
        let mut orch = test_orchestrator(project);

        orch.record_plan_outcome(
            plan_id,
            PlanOutcome::Failed {
                reason: "compilation error".to_owned(),
            },
        )
        .unwrap();

        let plan = orch
            .project
            .phases[0]
            .plans
            .iter()
            .find(|p| p.id == plan_id)
            .unwrap();
        assert_eq!(plan.iterations, 1);
        assert_eq!(plan.state, PlanState::Failed);
        assert_eq!(plan.blockers.len(), 1);
    }
}
