//! Phase types within a project.

use serde::{Deserialize, Serialize};
use ulid::Ulid;

use crate::plan::{Plan, PlanState};

/// A phase within a project (e.g., "Foundation", "Core Features").
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Phase {
    /// Unique phase identifier.
    pub id: Ulid,
    /// Human-readable phase name (e.g., "Foundation", "Core Features").
    pub name: String,
    /// What this phase aims to accomplish.
    pub goal: String,
    /// Preconditions that must hold before this phase can begin.
    pub requirements: Vec<String>,
    /// Executable plans within this phase.
    pub plans: Vec<Plan>,
    /// Current lifecycle state.
    pub state: PhaseState,
    /// Ordering position within the project (lower runs first).
    pub order: u32,
}

/// Phase lifecycle states.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PhaseState {
    /// Phase has not started yet.
    Pending,
    /// Phase is active and plans are being prepared.
    Active,
    /// Plans within this phase are being executed.
    Executing,
    /// Execution is done, verifying outcomes.
    Verifying,
    /// All plans completed successfully.
    Complete,
    /// Phase failed, with a flag indicating whether retry is possible.
    Failed { can_retry: bool },
}

impl Phase {
    /// Create a new phase in the `Pending` state.
    #[must_use]
    pub fn new(name: String, goal: String, order: u32) -> Self {
        Self {
            id: Ulid::new(),
            name,
            goal,
            requirements: Vec::new(),
            plans: Vec::new(),
            state: PhaseState::Pending,
            order,
        }
    }

    /// Add an executable plan to this phase.
    pub fn add_plan(&mut self, plan: Plan) {
        self.plans.push(plan);
    }

    /// Whether this phase has reached the `Complete` state.
    #[must_use]
    pub fn is_complete(&self) -> bool {
        self.state == PhaseState::Complete
    }

    /// Percentage of plans in a terminal state (complete, skipped, failed, stuck).
    #[must_use]
    pub fn completion_percentage(&self) -> f64 {
        if self.plans.is_empty() {
            return 0.0;
        }
        let done = self
            .plans
            .iter()
            .filter(|p| {
                matches!(
                    p.state,
                    PlanState::Complete | PlanState::Skipped | PlanState::Failed | PlanState::Stuck
                )
            })
            .count();
        #[expect(clippy::cast_precision_loss, reason = "plan counts are small")]
        let pct = done as f64 / self.plans.len() as f64 * 100.0;
        pct
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_phase_defaults() {
        let phase = Phase::new("Foundation".into(), "Build foundation".into(), 1);
        assert_eq!(phase.name, "Foundation");
        assert_eq!(phase.goal, "Build foundation");
        assert_eq!(phase.order, 1);
        assert_eq!(phase.state, PhaseState::Pending);
        assert!(phase.plans.is_empty());
        assert!(phase.requirements.is_empty());
    }

    #[test]
    fn add_plan_to_phase() {
        let mut phase = Phase::new("P1".into(), "g".into(), 1);
        let plan = Plan::new("task".into(), "desc".into(), 1);
        phase.add_plan(plan);
        assert_eq!(phase.plans.len(), 1);
    }

    #[test]
    fn is_complete_when_complete() {
        let mut phase = Phase::new("P1".into(), "g".into(), 1);
        phase.state = PhaseState::Complete;
        assert!(phase.is_complete());
    }

    #[test]
    fn is_not_complete_when_pending() {
        let phase = Phase::new("P1".into(), "g".into(), 1);
        assert!(!phase.is_complete());
    }

    #[test]
    fn completion_percentage_empty_plans() {
        let phase = Phase::new("P1".into(), "g".into(), 1);
        assert!((phase.completion_percentage() - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn completion_percentage_all_complete() {
        let mut phase = Phase::new("P1".into(), "g".into(), 1);
        let mut p1 = Plan::new("t1".into(), "d1".into(), 1);
        p1.state = PlanState::Complete;
        let mut p2 = Plan::new("t2".into(), "d2".into(), 1);
        p2.state = PlanState::Complete;
        phase.add_plan(p1);
        phase.add_plan(p2);
        assert!((phase.completion_percentage() - 100.0).abs() < f64::EPSILON);
    }

    #[test]
    fn completion_percentage_mixed() {
        let mut phase = Phase::new("P1".into(), "g".into(), 1);
        let mut p1 = Plan::new("t1".into(), "d1".into(), 1);
        p1.state = PlanState::Complete;
        let p2 = Plan::new("t2".into(), "d2".into(), 1); // Pending
        let mut p3 = Plan::new("t3".into(), "d3".into(), 1);
        p3.state = PlanState::Failed;
        let mut p4 = Plan::new("t4".into(), "d4".into(), 1);
        p4.state = PlanState::Stuck;
        phase.add_plan(p1);
        phase.add_plan(p2);
        phase.add_plan(p3);
        phase.add_plan(p4);
        assert!((phase.completion_percentage() - 75.0).abs() < f64::EPSILON);
    }

    #[test]
    fn completion_percentage_skipped_counts() {
        let mut phase = Phase::new("P1".into(), "g".into(), 1);
        let mut p1 = Plan::new("t1".into(), "d1".into(), 1);
        p1.state = PlanState::Skipped;
        phase.add_plan(p1);
        assert!((phase.completion_percentage() - 100.0).abs() < f64::EPSILON);
    }

    #[test]
    fn phase_state_variants() {
        assert_ne!(PhaseState::Pending, PhaseState::Active);
        assert_ne!(PhaseState::Executing, PhaseState::Verifying);
        assert_eq!(
            PhaseState::Failed { can_retry: true },
            PhaseState::Failed { can_retry: true }
        );
        assert_ne!(
            PhaseState::Failed { can_retry: true },
            PhaseState::Failed { can_retry: false }
        );
    }

    #[test]
    fn phase_state_serde_roundtrip() {
        let states = [
            PhaseState::Pending,
            PhaseState::Active,
            PhaseState::Executing,
            PhaseState::Verifying,
            PhaseState::Complete,
            PhaseState::Failed { can_retry: true },
            PhaseState::Failed { can_retry: false },
        ];
        for state in &states {
            let json = serde_json::to_string(state).unwrap();
            let back: PhaseState = serde_json::from_str(&json).unwrap();
            assert_eq!(&back, state, "roundtrip failed for {state:?}");
        }
    }

    #[test]
    fn completion_percentage_failed_plans_count_as_terminal() {
        let mut phase = Phase::new("P1".into(), "g".into(), 1);
        let mut p1 = Plan::new("t1".into(), "d1".into(), 1);
        p1.state = PlanState::Failed;
        let mut p2 = Plan::new("t2".into(), "d2".into(), 1);
        p2.state = PlanState::Stuck;
        let p3 = Plan::new("t3".into(), "d3".into(), 1);
        phase.add_plan(p1);
        phase.add_plan(p2);
        phase.add_plan(p3);
        let pct = phase.completion_percentage();
        assert!(
            (pct - 66.666_666_666_666_66).abs() < 0.01,
            "expected ~66.67%, got {pct}"
        );
    }

    #[test]
    fn phase_with_single_executing_plan_is_zero_percent() {
        let mut phase = Phase::new("P1".into(), "g".into(), 1);
        let mut plan = Plan::new("t1".into(), "d1".into(), 1);
        plan.state = PlanState::Executing;
        phase.add_plan(plan);
        assert!((phase.completion_percentage() - 0.0).abs() < f64::EPSILON);
    }
}
