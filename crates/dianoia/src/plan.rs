//! Executable plans within a phase.

use serde::{Deserialize, Serialize};
use ulid::Ulid;

use crate::error::{self, Result};

const DEFAULT_MAX_ITERATIONS: u32 = 10;

/// An executable plan within a phase.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Plan {
    /// Unique plan identifier.
    pub id: Ulid,
    /// Short title for the plan.
    pub title: String,
    /// Detailed description of what this plan accomplishes.
    pub description: String,
    /// Execution wave: plans in the same wave can run in parallel.
    pub wave: u32,
    /// Plan IDs that must complete before this plan can start.
    pub depends_on: Vec<Ulid>,
    /// Current lifecycle state.
    pub state: PlanState,
    /// Maximum allowed iterations before the plan is marked stuck.
    pub max_iterations: u32,
    /// Number of iterations executed so far.
    pub iterations: u32,
    /// Blockers discovered during execution.
    pub blockers: Vec<Blocker>,
    /// Notable accomplishments recorded during execution.
    pub achievements: Vec<String>,
}

/// Plan lifecycle states.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PlanState {
    /// Plan has not been evaluated for readiness yet.
    Pending,
    /// All dependencies are satisfied and the plan can execute.
    Ready,
    /// Plan is currently being executed.
    Executing,
    /// Plan finished successfully.
    Complete,
    /// Plan execution failed.
    Failed,
    /// Plan was intentionally skipped (e.g., no longer relevant).
    Skipped,
    /// Plan exceeded its iteration limit without completing.
    Stuck,
}

/// A blocker discovered during plan execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Blocker {
    /// What is blocking progress.
    pub description: String,
    /// The plan that is blocked.
    pub plan_id: Ulid,
    /// When the blocker was identified.
    pub detected_at: jiff::Timestamp,
}

impl Plan {
    /// Create a new plan in the `Pending` state with default iteration limits.
    #[must_use]
    pub fn new(title: String, description: String, wave: u32) -> Self {
        Self {
            id: Ulid::new(),
            title,
            description,
            wave,
            depends_on: Vec::new(),
            state: PlanState::Pending,
            max_iterations: DEFAULT_MAX_ITERATIONS,
            iterations: 0,
            blockers: Vec::new(),
            achievements: Vec::new(),
        }
    }

    /// Check if all dependencies are satisfied given completed plan IDs.
    #[must_use]
    pub fn is_ready(&self, completed: &[Ulid]) -> bool {
        self.depends_on.iter().all(|dep| completed.contains(dep))
    }

    /// Record an iteration. Returns `Err` if `max_iterations` exceeded.
    #[must_use]
    pub fn record_iteration(&mut self) -> Result<()> {
        self.iterations += 1;
        if self.iterations > self.max_iterations {
            self.state = PlanState::Stuck;
            return error::PlanStuckSnafu {
                plan_id: self.id.to_string(),
                iterations: self.iterations,
            }
            .fail();
        }
        Ok(())
    }

    /// Mark as stuck with a blocker.
    pub fn mark_stuck(&mut self, blocker: Blocker) {
        self.state = PlanState::Stuck;
        self.blockers.push(blocker);
    }
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
mod tests {
    use super::*;

    #[test]
    fn dependency_check() {
        let dep1 = Ulid::new();
        let dep2 = Ulid::new();
        let mut plan = Plan::new("task".into(), "do a thing".into(), 1);
        plan.depends_on = vec![dep1, dep2];

        assert!(!plan.is_ready(&[]));
        assert!(!plan.is_ready(&[dep1]));
        assert!(plan.is_ready(&[dep1, dep2]));
        assert!(plan.is_ready(&[dep2, dep1, Ulid::new()]));
    }

    #[test]
    fn iteration_tracking_and_stuck() {
        let mut plan = Plan::new("task".into(), "desc".into(), 1);
        plan.max_iterations = 3;

        plan.record_iteration().unwrap(); // 1
        plan.record_iteration().unwrap(); // 2
        plan.record_iteration().unwrap(); // 3
        let result = plan.record_iteration(); // 4 > 3
        assert!(result.is_err());
        assert_eq!(plan.state, PlanState::Stuck);
    }

    #[test]
    fn wave_ordering() {
        let first = Plan::new("first".into(), "wave 1".into(), 1);
        let second = Plan::new("second".into(), "wave 1".into(), 1);
        let mut dependent = Plan::new("dependent".into(), "wave 2".into(), 2);
        dependent.depends_on = vec![first.id, second.id];

        assert!(first.is_ready(&[]));
        assert!(second.is_ready(&[]));

        assert!(!dependent.is_ready(&[]));
        assert!(!dependent.is_ready(&[first.id]));
        assert!(dependent.is_ready(&[first.id, second.id]));
    }

    #[test]
    fn new_plan_defaults() {
        let plan = Plan::new("task".into(), "desc".into(), 1);
        assert_eq!(plan.title, "task");
        assert_eq!(plan.description, "desc");
        assert_eq!(plan.wave, 1);
        assert_eq!(plan.state, PlanState::Pending);
        assert_eq!(plan.iterations, 0);
        assert_eq!(plan.max_iterations, 10);
        assert!(plan.depends_on.is_empty());
        assert!(plan.blockers.is_empty());
        assert!(plan.achievements.is_empty());
    }

    #[test]
    fn is_ready_no_deps() {
        let plan = Plan::new("task".into(), "desc".into(), 1);
        assert!(plan.is_ready(&[]));
        assert!(plan.is_ready(&[Ulid::new()]));
    }

    #[test]
    fn mark_stuck_sets_state_and_blocker() {
        let mut plan = Plan::new("task".into(), "desc".into(), 1);
        let blocker = Blocker {
            description: "blocked on review".into(),
            plan_id: plan.id,
            detected_at: jiff::Timestamp::now(),
        };
        plan.mark_stuck(blocker);
        assert_eq!(plan.state, PlanState::Stuck);
        assert_eq!(plan.blockers.len(), 1);
        assert_eq!(plan.blockers[0].description, "blocked on review");
    }

    #[test]
    fn record_iteration_within_limit() {
        let mut plan = Plan::new("task".into(), "desc".into(), 1);
        plan.max_iterations = 5;
        for _ in 0..5 {
            assert!(plan.record_iteration().is_ok());
        }
        assert_eq!(plan.iterations, 5);
        assert_eq!(plan.state, PlanState::Pending);
    }

    #[test]
    fn blocker_creation() {
        let plan_id = Ulid::new();
        let now = jiff::Timestamp::now();
        let blocker = Blocker {
            description: "needs API design decision".into(),
            plan_id,
            detected_at: now,
        };
        assert_eq!(blocker.description, "needs API design decision");
        assert_eq!(blocker.plan_id, plan_id);
        assert_eq!(blocker.detected_at, now);
    }

    #[test]
    fn plan_state_serde_roundtrip() {
        let states = [
            PlanState::Pending,
            PlanState::Ready,
            PlanState::Executing,
            PlanState::Complete,
            PlanState::Failed,
            PlanState::Skipped,
            PlanState::Stuck,
        ];
        for state in &states {
            let json = serde_json::to_string(state).unwrap();
            let back: PlanState = serde_json::from_str(&json).unwrap();
            assert_eq!(&back, state, "roundtrip failed for {state:?}");
        }
    }

    #[test]
    fn plan_serde_roundtrip_preserves_fields() {
        let mut plan = Plan::new("test task".into(), "do the thing".into(), 2);
        plan.depends_on = vec![Ulid::new()];
        plan.achievements = vec!["milestone reached".into()];

        let json = serde_json::to_string(&plan).unwrap();
        let back: Plan = serde_json::from_str(&json).unwrap();
        assert_eq!(back.id, plan.id);
        assert_eq!(back.title, "test task");
        assert_eq!(back.description, "do the thing");
        assert_eq!(back.wave, 2);
        assert_eq!(back.depends_on, plan.depends_on);
        assert_eq!(back.achievements, vec!["milestone reached"]);
        assert_eq!(back.max_iterations, 10);
    }

    #[test]
    fn mark_stuck_then_add_another_blocker() {
        let mut plan = Plan::new("task".into(), "desc".into(), 1);
        let blocker1 = Blocker {
            description: "first blocker".into(),
            plan_id: plan.id,
            detected_at: jiff::Timestamp::now(),
        };
        plan.mark_stuck(blocker1);
        assert_eq!(plan.state, PlanState::Stuck);
        assert_eq!(plan.blockers.len(), 1);

        plan.blockers.push(Blocker {
            description: "second blocker".into(),
            plan_id: plan.id,
            detected_at: jiff::Timestamp::now(),
        });
        assert_eq!(plan.blockers.len(), 2);
    }
}
